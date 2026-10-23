//! Sounds the game plays directly: one-shots, loops with a handle, interface sounds.

use crate::{
    Category, SoundId, SoundLibrary,
    limits::{Limits, Request},
    spatial::{At, Distances, Listener, hear},
};
use canopy_content::effects::LayerSound;

/// A sound started with [`crate::Audio::play`]: stop or move it with this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle(pub(crate) u64);

/// How to play a sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayOptions {
    pub category: Category,
    /// Linear gain, 0–1.
    pub volume: f32,
    /// Playback rate; changes pitch and length.
    pub pitch: f32,
    pub looping: bool,
    /// Distance falloff for sounds on the ground.
    pub distances: Distances,
    /// Seconds to fade in from the start.
    pub fade_in: f32,
    /// Seconds to fade out before a one-shot's end and after [`crate::Audio::stop`]
    /// (unless the stop names its own fade).
    pub fade_out: f32,
    /// Multiplies audibility when voices are scarce.
    pub priority: f32,
    /// A sound event's group and its most copies at once ([`crate::events`]).
    pub group: Option<(u32, u16)>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            category: Category::World,
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            distances: Distances::default(),
            fade_in: 0.0,
            fade_out: 0.0,
            priority: 1.0,
            group: None,
        }
    }
}

impl PlayOptions {
    pub fn category(category: Category) -> Self {
        Self {
            category,
            ..Self::default()
        }
    }

    /// An effect sound layer's settings (volume, pitch, loop, fades, distances), for
    /// playing a layer outside an ability event, such as an effect preview.
    pub fn from_layer(layer: &LayerSound) -> Self {
        Self {
            category: Category::Effect,
            volume: layer.volume as f32,
            pitch: layer.pitch as f32,
            looping: layer.looping,
            distances: Distances::new(layer.reference_distance as f32, layer.max_distance as f32),
            fade_in: crate::effects::authored_seconds(layer.fade_in_ticks) as f32,
            fade_out: crate::effects::authored_seconds(layer.fade_out_ticks) as f32,
            priority: 1.0,
            group: None,
        }
    }
}

struct Source {
    key: u64,
    sound: SoundId,
    at: At,
    options: PlayOptions,
    /// Clock time (seconds) of offset zero.
    start: f64,
    /// Clock time the stop was asked for, and its fade.
    stop: Option<(f64, f32)>,
}

/// Every source started by the game.
#[derive(Default)]
pub(crate) struct Sources {
    list: Vec<Source>,
}

/// At most this many sources live at once; further plays are refused.
const MAX_SOURCES: usize = 1024;

/// The clocks sources run on, in seconds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clocks {
    pub game: f64,
    pub real: f64,
    pub speed: f64,
}

impl Clocks {
    pub(crate) fn now(&self, category: Category) -> f64 {
        if category.game_time() {
            self.game
        } else {
            self.real
        }
    }
}

impl Sources {
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &mut self,
        key: u64,
        sound: SoundId,
        at: At,
        options: PlayOptions,
        clocks: &Clocks,
        limits: &Limits,
    ) -> Option<Handle> {
        let now = clocks.now(options.category);
        let retrigger = f64::from(limits.budget(options.category).retrigger);
        if !options.looping
            && retrigger > 0.0
            && self.list.iter().any(|s| {
                s.sound == sound
                    && s.options.category == options.category
                    && !s.options.looping
                    && (now - s.start).abs() < retrigger
            })
        {
            return None;
        }
        if self.list.len() >= MAX_SOURCES {
            tracing::debug!("audio: too many sources, a sound was dropped");
            return None;
        }
        self.list.push(Source {
            key,
            sound,
            at,
            options,
            start: now,
            stop: None,
        });
        Some(Handle(key))
    }

    /// Stops with a fade (seconds; `None` uses the sound's own fade-out).
    pub fn stop(&mut self, handle: Handle, fade: Option<f32>, clocks: &Clocks) {
        if let Some(s) = self.list.iter_mut().find(|s| s.key == handle.0)
            && s.stop.is_none()
        {
            let now = clocks.now(s.options.category);
            s.stop = Some((now, fade.unwrap_or(s.options.fade_out).max(0.0)));
        }
    }

    pub fn set_at(&mut self, handle: Handle, at: At) {
        if let Some(s) = self.list.iter_mut().find(|s| s.key == handle.0) {
            s.at = at;
        }
    }

    pub fn is_live(&self, handle: Handle) -> bool {
        self.list.iter().any(|s| s.key == handle.0)
    }

    /// Forgets every source that runs on game time (a new match or a loaded game).
    pub fn clear_game(&mut self) {
        self.list.retain(|s| !s.options.category.game_time());
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Requests of every audible source; drops finished ones.
    pub fn requests(
        &mut self,
        clocks: &Clocks,
        listener: &Listener,
        library: &SoundLibrary,
        out: &mut Vec<Request>,
    ) {
        self.list.retain(|s| {
            let category = s.options.category;
            let now = clocks.now(category);
            let age = now - s.start;
            if age < -1.0 {
                // The game clock went back (a loaded game): this belongs to another timeline.
                return false;
            }
            let duration = library.clip(s.sound).duration;
            let pitch = f64::from(s.options.pitch.max(0.01));
            let offset = age * pitch;
            if !s.options.looping && offset >= duration {
                return false;
            }
            let mut gain = s.options.volume;
            if let Some((stopped, fade)) = s.stop {
                let since = (now - stopped) as f32;
                if since >= fade {
                    return false;
                }
                gain *= 1.0 - since / fade.max(f32::EPSILON);
            }
            if age < 0.0 || (category.game_time() && clocks.speed <= 0.0) {
                return true;
            }
            if s.options.fade_in > 0.0 {
                gain *= (age as f32 / s.options.fade_in).min(1.0);
            }
            if !s.options.looping && s.options.fade_out > 0.0 {
                let remaining = ((duration - offset) / pitch) as f32;
                gain *= (remaining / s.options.fade_out).min(1.0);
            }
            let Some(heard) = hear(listener, s.at, s.options.distances) else {
                return true;
            };
            let speed = if category.game_time() {
                clocks.speed
            } else {
                1.0
            };
            out.push(Request {
                key: s.key,
                sound: s.sound,
                category,
                offset: if s.options.looping {
                    offset.rem_euclid(duration.max(f64::EPSILON))
                } else {
                    offset
                },
                rate: pitch * speed,
                gain: gain * heard.gain,
                pan: heard.pan,
                looping: s.options.looping,
                priority: s.options.priority,
                group: s.options.group,
            });
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (SoundLibrary, SoundId) {
        let mut library = SoundLibrary::new();
        let one_second = library.insert_pcm("s", 100, 1, &[0; 100]);
        (library, one_second)
    }

    fn requests(sources: &mut Sources, clocks: &Clocks, library: &SoundLibrary) -> Vec<Request> {
        let mut out = Vec::new();
        sources.requests(clocks, &Listener::default(), library, &mut out);
        out
    }

    #[test]
    fn one_shots_follow_their_clock_and_end() {
        let (library, sound) = setup();
        let limits = Limits::default();
        let mut sources = Sources::default();
        let mut clocks = Clocks {
            game: 10.0,
            real: 0.0,
            speed: 2.0,
        };
        let options = PlayOptions {
            pitch: 0.5,
            ..PlayOptions::default()
        };
        sources.play(1, sound, At::ground(3.0, 4.0), options, &clocks, &limits);
        clocks.game = 11.0;
        let r = requests(&mut sources, &clocks, &library);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].offset, 0.5);
        assert_eq!(r[0].rate, 1.0);
        // Paused: silent but alive.
        clocks.speed = 0.0;
        assert!(requests(&mut sources, &clocks, &library).is_empty());
        assert_eq!(sources.len(), 1);
        // Past its end at pitch 0.5 (two seconds): gone.
        clocks.game = 12.0;
        assert!(requests(&mut sources, &clocks, &library).is_empty());
        assert_eq!(sources.len(), 0);
    }

    #[test]
    fn retriggers_are_merged_and_loops_stop_with_a_fade() {
        let (library, sound) = setup();
        let limits = Limits::default();
        let mut sources = Sources::default();
        let mut clocks = Clocks {
            game: 0.0,
            real: 5.0,
            speed: 1.0,
        };
        let ui = PlayOptions::category(Category::Interface);
        assert!(
            sources
                .play(1, sound, At::Listener, ui, &clocks, &limits)
                .is_some()
        );
        assert!(
            sources
                .play(2, sound, At::Listener, ui, &clocks, &limits)
                .is_none()
        );
        let looping = PlayOptions {
            looping: true,
            category: Category::Interface,
            ..PlayOptions::default()
        };
        let handle = sources
            .play(3, sound, At::Listener, looping, &clocks, &limits)
            .unwrap();
        clocks.real = 7.25;
        let r = requests(&mut sources, &clocks, &library);
        let looped = r.iter().find(|r| r.key == 3).unwrap();
        assert_eq!(looped.offset, 0.25);
        sources.stop(handle, Some(1.0), &clocks);
        clocks.real = 7.75;
        let r = requests(&mut sources, &clocks, &library);
        assert_eq!(r.iter().find(|r| r.key == 3).unwrap().gain, 0.5);
        clocks.real = 8.5;
        requests(&mut sources, &clocks, &library);
        assert!(!sources.is_live(handle));
    }

    #[test]
    fn fades_and_distance_shape_the_gain() {
        let (library, sound) = setup();
        let limits = Limits::default();
        let mut sources = Sources::default();
        let mut clocks = Clocks {
            game: 0.0,
            real: 0.0,
            speed: 1.0,
        };
        let options = PlayOptions {
            fade_in: 0.2,
            fade_out: 0.4,
            distances: Distances::new(10.0, 50.0),
            ..PlayOptions::default()
        };
        sources.play(1, sound, At::ground(30.0, 0.0), options, &clocks, &limits);
        clocks.game = 0.1;
        let r = requests(&mut sources, &clocks, &library);
        // Half faded in, a quarter from distance.
        assert!((r[0].gain - 0.5 * 0.25).abs() < 1e-6);
        assert_eq!(r[0].pan, 1.0);
        clocks.game = 0.8;
        let r = requests(&mut sources, &clocks, &library);
        assert!((r[0].gain - 0.5 * 0.25).abs() < 1e-6);
        // Out of range: silent but alive.
        sources.set_at(Handle(1), At::ground(60.0, 0.0));
        assert!(requests(&mut sources, &clocks, &library).is_empty());
        assert!(sources.is_live(Handle(1)));
    }
}
