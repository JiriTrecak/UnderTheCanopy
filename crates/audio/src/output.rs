//! The device side: kira plays the voices the selector granted.
//!
//! Each frame the granted requests are reconciled with kira's playing sounds: new ones
//! start at their offset, kept ones follow their gain, pan and rate with short tweens and
//! are re-synchronised when they drift from their timeline, and the rest fade out. kira's
//! audio thread never allocates: sounds live in fixed-capacity arenas, commands travel
//! through lock-free queues and finished sounds are dropped on this side.

use crate::{
    SoundLibrary,
    library::{ClipData, SoundId},
    limits::{Limits, Request},
    settings::Bus,
};
use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Panning, PlaybackRate, Tween,
    backend::mock::{MockBackend, MockBackendSettings},
    sound::{
        FromFileError, PlaybackState, static_sound::StaticSoundHandle,
        streaming::StreamingSoundHandle,
    },
    track::{TrackBuilder, TrackHandle},
};
use std::time::Duration;

/// Parameter changes glide over this long (the old game smoothed gain over 12 ms and
/// pan over 20 ms).
const GLIDE: Duration = Duration::from_millis(20);
/// New voices fade in over this long, so starts in the middle of a sound do not click.
const FADE_IN: Duration = Duration::from_millis(12);
/// Voices that lose their slot or are stopped fade out over this long.
const FADE_OUT: Duration = Duration::from_millis(30);
/// Bus and master changes glide over this long.
const MIX_GLIDE: Duration = Duration::from_millis(60);
/// A voice further than this from its timeline (seconds) is moved back onto it (the old
/// game's threshold).
const DRIFT: f64 = 0.12;
/// After a seek (or a start partway in) a voice is not re-synchronised for this long: a
/// stream's position catches up only once its decoder has refilled, and seeking again
/// every frame meanwhile scratches.
const SEEK_SETTLE: Duration = Duration::from_millis(500);
/// A one-shot is not started with less than this left (seconds): the tail would only
/// click.
const MIN_REMAINING: f64 = 0.03;

enum Manager {
    Device(Box<AudioManager<DefaultBackend>>),
    Mock(Box<AudioManager<MockBackend>>),
}

impl Manager {
    fn main_volume(&mut self, volume: f32, tween: Tween) {
        let db = decibels(volume);
        match self {
            Manager::Device(m) => m.main_track().set_volume(db, tween),
            Manager::Mock(m) => m.main_track().set_volume(db, tween),
        }
    }

    fn add_track(
        &mut self,
        builder: TrackBuilder,
    ) -> Result<TrackHandle, kira::ResourceLimitReached> {
        match self {
            Manager::Device(m) => m.add_sub_track(builder),
            Manager::Mock(m) => m.add_sub_track(builder),
        }
    }
}

/// A playing sound: decoded frames, or a stream.
enum Playing {
    Frames(StaticSoundHandle),
    Stream(StreamingSoundHandle<FromFileError>),
}

impl Playing {
    fn state(&self) -> PlaybackState {
        match self {
            Self::Frames(h) => h.state(),
            Self::Stream(h) => h.state(),
        }
    }

    fn position(&self) -> f64 {
        match self {
            Self::Frames(h) => h.position(),
            Self::Stream(h) => h.position(),
        }
    }

    fn set_volume(&mut self, volume: Decibels, tween: Tween) {
        match self {
            Self::Frames(h) => h.set_volume(volume, tween),
            Self::Stream(h) => h.set_volume(volume, tween),
        }
    }

    fn set_panning(&mut self, panning: Panning, tween: Tween) {
        match self {
            Self::Frames(h) => h.set_panning(panning, tween),
            Self::Stream(h) => h.set_panning(panning, tween),
        }
    }

    fn set_playback_rate(&mut self, rate: PlaybackRate, tween: Tween) {
        match self {
            Self::Frames(h) => h.set_playback_rate(rate, tween),
            Self::Stream(h) => h.set_playback_rate(rate, tween),
        }
    }

    fn seek_to(&mut self, position: f64) {
        match self {
            Self::Frames(h) => h.seek_to(position),
            Self::Stream(h) => h.seek_to(position),
        }
    }

    fn stop(&mut self, tween: Tween) {
        match self {
            Self::Frames(h) => h.stop(tween),
            Self::Stream(h) => h.stop(tween),
        }
    }
}

struct Voice {
    key: u64,
    sound: SoundId,
    handle: Playing,
    gain: f32,
    pan: f32,
    rate: f64,
    /// The sound reached its end by itself, at this request offset.
    ended: Option<f64>,
    wanted: bool,
    /// When it was last moved onto its timeline (or started partway in).
    seeked: Option<std::time::Instant>,
}

pub(crate) struct Output {
    manager: Manager,
    buses: Vec<TrackHandle>,
    voices: Vec<Voice>,
    mix: Option<(f32, [f32; 4])>,
    /// Re-synchronising seeks made (for tests).
    seeks: u64,
}

impl Output {
    /// Opens the default output device.
    pub fn device(limits: &Limits) -> Result<Self, String> {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|e| e.to_string())?;
        Self::new(Manager::Device(Box::new(manager)), limits)
    }

    /// Runs the whole mixer without a device; nothing is rendered unless processed.
    pub fn headless(limits: &Limits) -> Result<Self, String> {
        let manager = AudioManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            ..AudioManagerSettings::default()
        })
        .map_err(|()| "mock backend".to_owned())?;
        Self::new(Manager::Mock(Box::new(manager)), limits)
    }

    fn new(mut manager: Manager, limits: &Limits) -> Result<Self, String> {
        let buses = Bus::ALL
            .iter()
            .map(|&bus| {
                manager
                    .add_track(
                        TrackBuilder::new()
                            .sound_capacity(limits.bus_capacity(bus))
                            .sub_track_capacity(0),
                    )
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            manager,
            buses,
            voices: Vec::new(),
            mix: None,
            seeks: 0,
        })
    }

    /// Renders `blocks` mixer blocks (128 frames each) when headless; nothing otherwise.
    pub fn render_headless(&mut self, blocks: usize) {
        if let Manager::Mock(manager) = &mut self.manager {
            for _ in 0..blocks {
                manager.backend_mut().on_start_processing();
                manager.backend_mut().process();
            }
        }
    }

    pub fn is_playing(&self, key: u64) -> bool {
        self.voices
            .iter()
            .any(|v| v.key == key && v.ended.is_none())
    }

    /// Voices playing now (not counting ones that ended or are fading out).
    pub fn playing(&self) -> usize {
        self.voices.iter().filter(|v| v.ended.is_none()).count()
    }

    /// Master and bus volumes (linear).
    pub fn set_mix(&mut self, master: f32, buses: [f32; 4]) {
        if self.mix == Some((master, buses)) {
            return;
        }
        let tween = Tween {
            duration: MIX_GLIDE,
            ..Tween::default()
        };
        self.manager.main_volume(master, tween);
        for (track, volume) in self.buses.iter_mut().zip(buses) {
            track.set_volume(decibels(volume), tween);
        }
        self.mix = Some((master, buses));
    }

    /// Makes the playing voices those of `granted`.
    pub fn reconcile<'a>(
        &mut self,
        granted: impl Iterator<Item = &'a Request>,
        library: &SoundLibrary,
    ) {
        for voice in &mut self.voices {
            voice.wanted = false;
        }
        for request in granted {
            let clip = library.clip(request.sound);
            if let Some(voice) = self
                .voices
                .iter_mut()
                .find(|v| v.key == request.key && v.sound == request.sound)
            {
                voice.wanted = true;
                if voice.ended.is_none() && voice.handle.state() == PlaybackState::Stopped {
                    voice.ended = Some(request.offset);
                }
                match voice.ended {
                    None => {
                        if update(voice, request, clip.duration) {
                            self.seeks += 1;
                        }
                        continue;
                    }
                    // The timeline jumped back (a looping parent effect): play it again.
                    Some(at) if request.offset + DRIFT < at => voice.wanted = false,
                    Some(_) => continue,
                }
            }
            if !request.looping && clip.duration - request.offset < MIN_REMAINING {
                continue;
            }
            let fade_in = Tween {
                duration: FADE_IN,
                ..Tween::default()
            };
            let bus = &mut self.buses[request.category.bus().index()];
            let started = match &clip.data {
                ClipData::Frames(data) => {
                    let data = data
                        .start_position(request.offset)
                        .volume(decibels(request.gain))
                        .panning(panning(request.pan))
                        .playback_rate(PlaybackRate(request.rate))
                        .fade_in_tween(fade_in);
                    let data = if request.looping {
                        data.loop_region(..)
                    } else {
                        data
                    };
                    bus.play(data)
                        .map(Playing::Frames)
                        .map_err(|e| e.to_string())
                }
                ClipData::Stream(encoded) => match encoded.open() {
                    Ok(data) => {
                        let data = data
                            .start_position(request.offset)
                            .volume(decibels(request.gain))
                            .panning(panning(request.pan))
                            .playback_rate(PlaybackRate(request.rate))
                            .fade_in_tween(fade_in);
                        let data = if request.looping {
                            data.loop_region(..)
                        } else {
                            data
                        };
                        bus.play(data)
                            .map(Playing::Stream)
                            .map_err(|e| e.to_string())
                    }
                    Err(e) => Err(e.to_string()),
                },
            };
            match started {
                Ok(handle) => self.voices.push(Voice {
                    key: request.key,
                    sound: request.sound,
                    handle,
                    gain: request.gain,
                    pan: request.pan,
                    rate: request.rate,
                    ended: None,
                    wanted: true,
                    seeked: (request.offset > 0.0).then(std::time::Instant::now),
                }),
                Err(e) => tracing::debug!("audio: voice not started: {e}"),
            }
        }
        self.voices.retain_mut(|voice| {
            if !voice.wanted {
                voice.handle.stop(Tween {
                    duration: FADE_OUT,
                    ..Tween::default()
                });
            }
            voice.wanted
        });
    }
}

/// Follows the request's gain, pan and rate; moves the voice back onto its timeline when it
/// drifted (and has settled since the last move). Returns whether it seeked.
fn update(voice: &mut Voice, request: &Request, duration: f64) -> bool {
    let glide = Tween {
        duration: GLIDE,
        ..Tween::default()
    };
    if (voice.gain - request.gain).abs() > 1e-4 {
        voice.handle.set_volume(decibels(request.gain), glide);
        voice.gain = request.gain;
    }
    if (voice.pan - request.pan).abs() > 1e-3 {
        voice.handle.set_panning(panning(request.pan), glide);
        voice.pan = request.pan;
    }
    if (voice.rate - request.rate).abs() > 1e-6 {
        voice
            .handle
            .set_playback_rate(PlaybackRate(request.rate), glide);
        voice.rate = request.rate;
    }
    if voice.seeked.is_some_and(|at| at.elapsed() < SEEK_SETTLE) {
        return false;
    }
    let position = voice.handle.position();
    let drift = if request.looping && duration > 0.0 {
        let d = (request.offset - position).rem_euclid(duration);
        d.min(duration - d)
    } else {
        (request.offset - position).abs()
    };
    if drift <= DRIFT {
        return false;
    }
    voice.handle.seek_to(request.offset);
    voice.seeked = Some(std::time::Instant::now());
    true
}

/// Linear gain as kira's decibels (−60 dB and below is silence).
pub(crate) fn decibels(gain: f32) -> Decibels {
    if gain <= 0.001 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * gain.log10())
    }
}

/// The old game's pan (a Web Audio stereo panner: equal power, `cos`/`sin` of the
/// position) in kira's equal-power law (`sqrt` of the position): `sin(p·π/2)` gives both
/// channels exactly the old gains.
pub(crate) fn panning(pan: f32) -> Panning {
    Panning((pan.clamp(-1.0, 1.0) * std::f32::consts::FRAC_PI_2).sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_resumed_partway_is_not_seeked_every_frame() {
        let limits = Limits::default();
        let mut output = Output::headless(&limits).unwrap();
        let mut library = SoundLibrary::new();
        let tone = library
            .insert_encoded("tone", include_bytes!("../tests/data/tone.mp3"))
            .unwrap();
        // Back from the background: the music resumes 0.3 s into its loop, and frames
        // come before the stream's decoder has caught up.
        let mut request = Request {
            key: 1,
            sound: tone,
            category: crate::limits::Category::Music,
            offset: 0.3,
            rate: 1.0,
            gain: 1.0,
            pan: 0.0,
            looping: true,
            priority: 1.0,
            group: None,
        };
        for _ in 0..10 {
            output.reconcile(std::iter::once(&request), &library);
            request.offset += 0.016;
        }
        assert_eq!(output.playing(), 1);
        assert_eq!(output.seeks, 0, "seeked while the stream was catching up");
    }

    #[test]
    fn decibels_and_silence() {
        assert_eq!(decibels(1.0), Decibels::IDENTITY);
        assert!((decibels(0.5).0 + 6.0206).abs() < 1e-3);
        assert_eq!(decibels(0.0), Decibels::SILENCE);
        assert_eq!(decibels(0.0005), Decibels::SILENCE);
        assert!((decibels(0.5).as_amplitude() - 0.5).abs() < 1e-5);
    }

    /// With mono sounds stored at −3 dB, kira's pan law reproduces the old Web Audio
    /// stereo panner's channel gains for every pan.
    #[test]
    fn panning_matches_the_web_audio_panner() {
        let mono = std::f32::consts::FRAC_1_SQRT_2;
        for step in -10..=10 {
            let pan = step as f32 / 10.0;
            let frame = kira::Frame::from_mono(mono).panned(panning(pan));
            let x = (pan + 1.0) / 2.0 * std::f32::consts::FRAC_PI_2;
            assert!((frame.left - x.cos()).abs() < 1e-5, "{pan}: {frame:?}");
            assert!((frame.right - x.sin()).abs() < 1e-5, "{pan}: {frame:?}");
        }
    }
}
