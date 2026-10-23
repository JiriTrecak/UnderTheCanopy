//! Music: a playlist played track after track with equal-power crossfades.
//!
//! No music ships yet; the player is ready for it. Tracks are ordinary sounds from the
//! library (see `docs/audio.md` on streaming once real music arrives).

use crate::{
    SoundId, SoundLibrary,
    limits::{Category, Request},
};
use std::f64::consts::FRAC_PI_2;

/// What to play and how.
#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
    pub tracks: Vec<SoundId>,
    /// Play in a random order (a new order every round, never the same track twice in
    /// a row).
    pub shuffle: bool,
    /// Start again after the last track; otherwise music ends with it.
    pub repeat: bool,
    /// Seconds two tracks overlap, also used when another playlist takes over.
    pub crossfade: f32,
    /// Linear gain of the music, before the music bus.
    pub volume: f32,
}

impl Playlist {
    pub fn new(tracks: Vec<SoundId>) -> Self {
        Self {
            tracks,
            shuffle: false,
            repeat: true,
            crossfade: 4.0,
            volume: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Track {
    key: u64,
    sound: SoundId,
    volume: f32,
    /// Wall-clock seconds the track began.
    start: f64,
    fade_in: f64,
    /// When it started fading out, and for how long.
    fade_out: Option<(f64, f64)>,
}

impl Track {
    /// Equal-power crossfade gain at `now`; `None` once the fade-out is over.
    fn gain(&self, now: f64) -> Option<f32> {
        let rise = if self.fade_in > 0.0 {
            ((now - self.start) / self.fade_in).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let fall = match self.fade_out {
            None => 1.0,
            Some((from, length)) => {
                let progress = if length > 0.0 {
                    (now - from) / length
                } else {
                    1.0
                };
                if progress >= 1.0 {
                    return None;
                }
                (progress.max(0.0) * FRAC_PI_2).cos()
            }
        };
        Some(self.volume * ((rise * FRAC_PI_2).sin() * fall) as f32)
    }
}

/// The music player.
pub(crate) struct Music {
    playlist: Option<Playlist>,
    order: Vec<usize>,
    /// Index into `order` of the current track.
    position: usize,
    current: Option<Track>,
    /// Tracks fading out.
    fading: Vec<Track>,
    rng: u64,
}

impl Default for Music {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self::with_seed(seed)
    }
}

impl Music {
    pub fn with_seed(seed: u64) -> Self {
        Self {
            playlist: None,
            order: Vec::new(),
            position: 0,
            current: None,
            fading: Vec::new(),
            rng: seed | 1,
        }
    }

    /// Starts a playlist, crossfading from whatever plays.
    pub fn play(&mut self, playlist: Playlist, now: f64, keys: &mut u64) {
        let crossfade = f64::from(playlist.crossfade.max(0.0));
        self.fade_out_current(now, crossfade);
        self.order = (0..playlist.tracks.len()).collect();
        let shuffle = playlist.shuffle;
        self.playlist = Some(playlist);
        if shuffle {
            self.shuffle(None);
        }
        self.position = 0;
        self.start_track(now, crossfade, keys);
    }

    /// Fades the music out over `fade` seconds, including tracks already fading.
    pub fn stop(&mut self, now: f64, fade: f32) {
        let fade = f64::from(fade.max(0.0));
        for track in &mut self.fading {
            if let Some((from, length)) = track.fade_out
                && from + length > now + fade
            {
                // Continue from the gain it has now, over the shorter fade.
                let fall = track.gain(now).unwrap_or(0.0) / track.volume.max(f32::EPSILON);
                track.volume *= fall;
                track.fade_in = 0.0;
                track.fade_out = Some((now, fade));
            }
        }
        self.fade_out_current(now, fade);
        self.playlist = None;
    }

    pub fn current(&self) -> Option<SoundId> {
        self.current.map(|t| t.sound)
    }

    fn fade_out_current(&mut self, now: f64, length: f64) {
        if let Some(mut track) = self.current.take() {
            track.fade_out = Some((now, length));
            self.fading.push(track);
        }
    }

    fn start_track(&mut self, now: f64, fade_in: f64, keys: &mut u64) {
        let Some(playlist) = &self.playlist else {
            return;
        };
        let Some(&index) = self.order.get(self.position) else {
            return;
        };
        *keys += 1;
        self.current = Some(Track {
            key: *keys,
            sound: playlist.tracks[index],
            volume: playlist.volume,
            start: now,
            fade_in,
            fade_out: None,
        });
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64*: music order needs variety, not quality.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A new random order; `avoid_first` is not played first (the track just heard).
    fn shuffle(&mut self, avoid_first: Option<usize>) {
        for i in (1..self.order.len()).rev() {
            let j = (self.next_random() % (i as u64 + 1)) as usize;
            self.order.swap(i, j);
        }
        if self.order.len() > 1 && self.order.first().copied() == avoid_first {
            let j = 1 + (self.next_random() % (self.order.len() as u64 - 1)) as usize;
            self.order.swap(0, j);
        }
    }

    /// Advances the playlist and asks for voices.
    pub fn requests(
        &mut self,
        now: f64,
        library: &SoundLibrary,
        keys: &mut u64,
        out: &mut Vec<Request>,
    ) {
        if let Some(track) = self.current {
            let duration = library.clip(track.sound).duration;
            let remaining = duration - (now - track.start);
            let crossfade = self
                .playlist
                .as_ref()
                .map_or(0.0, |p| f64::from(p.crossfade.max(0.0)));
            if remaining <= crossfade.min(duration / 2.0) {
                // Hand over to the next track; the current one fades out as it ends.
                self.fade_out_current(now, remaining.max(0.0));
                self.advance();
                self.start_track(now, crossfade.min(duration / 2.0), keys);
            }
        }
        self.fading
            .retain(|t| t.gain(now).is_some() && now - t.start < library.clip(t.sound).duration);
        for track in self.fading.iter().chain(&self.current) {
            let offset = now - track.start;
            if offset < 0.0 || offset >= library.clip(track.sound).duration {
                continue;
            }
            if let Some(gain) = track.gain(now) {
                out.push(Request {
                    key: track.key,
                    sound: track.sound,
                    category: Category::Music,
                    offset,
                    rate: 1.0,
                    gain,
                    pan: 0.0,
                    looping: false,
                    priority: 1.0,
                    group: None,
                });
            }
        }
    }

    fn advance(&mut self) {
        let Some(playlist) = &self.playlist else {
            return;
        };
        self.position += 1;
        if self.position < self.order.len() {
            return;
        }
        if !playlist.repeat {
            self.playlist = None;
            return;
        }
        let last = self.order.last().copied();
        self.position = 0;
        if playlist.shuffle {
            self.shuffle(last);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> (SoundLibrary, Vec<SoundId>) {
        let mut library = SoundLibrary::new();
        // Tracks of 10, 20 and 30 seconds at 10 Hz.
        let tracks = (1..=3)
            .map(|i| library.insert_pcm(&format!("t{i}"), 10, 1, &vec![0; 100 * i]))
            .collect();
        (library, tracks)
    }

    fn step(music: &mut Music, now: f64, library: &SoundLibrary, keys: &mut u64) -> Vec<Request> {
        let mut out = Vec::new();
        music.requests(now, library, keys, &mut out);
        out
    }

    #[test]
    fn tracks_follow_each_other_with_an_equal_power_crossfade() {
        let (library, tracks) = library();
        let mut music = Music::with_seed(7);
        let mut keys = 0;
        let mut playlist = Playlist::new(tracks.clone());
        playlist.crossfade = 2.0;
        music.play(playlist, 0.0, &mut keys);
        // Fading in from the start.
        let r = step(&mut music, 1.0, &library, &mut keys);
        assert_eq!(r.len(), 1);
        assert!((r[0].gain - (std::f32::consts::FRAC_PI_4).sin()).abs() < 1e-5);
        let r = step(&mut music, 5.0, &library, &mut keys);
        assert_eq!((r[0].sound, r[0].gain, r[0].offset), (tracks[0], 1.0, 5.0));
        // Two seconds before the end, the next track starts.
        step(&mut music, 8.0, &library, &mut keys);
        assert_eq!(music.current(), Some(tracks[1]));
        let r = step(&mut music, 9.0, &library, &mut keys);
        assert_eq!(r.len(), 2);
        let power: f32 = r.iter().map(|r| r.gain * r.gain).sum();
        assert!((power - 1.0).abs() < 1e-4, "{power}");
        // The first track is gone once it ended.
        let r = step(&mut music, 10.5, &library, &mut keys);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].sound, tracks[1]);
        assert!((r[0].offset - 2.5).abs() < 1e-9);
    }

    #[test]
    fn playlists_end_or_repeat_and_stop_fades() {
        let (library, tracks) = library();
        let mut keys = 0;
        let mut music = Music::with_seed(3);
        let mut once = Playlist::new(vec![tracks[0]]);
        once.repeat = false;
        once.crossfade = 0.0;
        music.play(once, 0.0, &mut keys);
        step(&mut music, 5.0, &library, &mut keys);
        assert!(step(&mut music, 10.0, &library, &mut keys).is_empty());
        assert!(step(&mut music, 30.0, &library, &mut keys).is_empty());

        let mut looped = Playlist::new(vec![tracks[0]]);
        looped.crossfade = 0.0;
        music.play(looped, 100.0, &mut keys);
        step(&mut music, 110.0, &library, &mut keys);
        assert_eq!(music.current(), Some(tracks[0]));
        let r = step(&mut music, 111.0, &library, &mut keys);
        assert!((r[0].offset - 1.0).abs() < 1e-9);
        music.stop(111.0, 1.0);
        let r = step(&mut music, 111.5, &library, &mut keys);
        assert!((r[0].gain - (std::f64::consts::FRAC_PI_4.cos()) as f32).abs() < 1e-5);
        assert!(step(&mut music, 112.0, &library, &mut keys).is_empty());
        assert_eq!(music.current(), None);
    }

    #[test]
    fn shuffle_plays_every_track_each_round_without_repeats() {
        let mut music = Music::with_seed(11);
        let mut library = SoundLibrary::new();
        let tracks: Vec<SoundId> = (0..5)
            .map(|i| library.insert_pcm(&format!("t{i}"), 10, 1, &[0; 10]))
            .collect();
        let mut playlist = Playlist::new(tracks.clone());
        playlist.shuffle = true;
        playlist.crossfade = 0.0;
        let mut keys = 0;
        music.play(playlist, 0.0, &mut keys);
        let mut heard = vec![music.current().unwrap()];
        let mut now = 0.0;
        for _ in 0..19 {
            now += 1.0;
            step(&mut music, now, &library, &mut keys);
            heard.push(music.current().unwrap());
        }
        for round in heard.chunks(5) {
            let mut round = round.to_vec();
            round.sort();
            assert_eq!(round, tracks);
        }
        assert!(heard.windows(2).all(|w| w[0] != w[1]));
    }
}
