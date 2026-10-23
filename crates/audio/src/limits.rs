//! Voice limiting: which of the sounds that want to play get a real voice.
//!
//! Every frame, each source that should be heard (effect cue, one-shot, loop, music
//! track) asks for a voice with its current audibility. Sources that get none are
//! *virtual*: they keep their timeline, so if they win a voice later they start at the
//! right offset. Within each category the most audible requests win, subject to the
//! category's voice budget and a per-sound cap, so a battle of 400 units plays the few
//! nearest, loudest instances of each sound instead of hundreds of copies.

use crate::{SoundId, settings::Bus};
use std::collections::HashMap;

/// What kind of sound something is: decides its bus, clock and voice budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    /// Sound layers of ability effects (the only sounds the old game had).
    Effect,
    /// Other sounds of the world the game plays (combat, units, buildings).
    World,
    /// Menus, buttons and notifications: not positional, not paused with the game.
    Interface,
    Music,
    /// A unit answering its player: one at a time, not positional.
    Voice,
    /// Loops of the place: a biome's air, streams and falls (positional, wall clock, so
    /// the world keeps breathing while the game is paused).
    Ambience,
    /// Spoken story: a dialogue line's voice, a page's narration.
    Dialogue,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Effect,
        Category::World,
        Category::Interface,
        Category::Music,
        Category::Voice,
        Category::Ambience,
        Category::Dialogue,
    ];

    /// The category a content sound event plays in.
    pub fn of(category: canopy_content::sounds::SoundCategory) -> Self {
        use canopy_content::sounds::SoundCategory as S;
        match category {
            S::World => Category::World,
            S::Voice => Category::Voice,
            S::Interface => Category::Interface,
            S::Ambience => Category::Ambience,
            S::Music => Category::Music,
            S::Dialogue => Category::Dialogue,
        }
    }

    /// Whether a new sound of the category ends the one playing (a unit's answer, a line).
    pub fn exclusive(self) -> bool {
        matches!(self, Category::Voice | Category::Dialogue)
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }

    pub fn bus(self) -> Bus {
        match self {
            Category::Effect | Category::World | Category::Ambience => Bus::Effects,
            Category::Interface => Bus::Interface,
            Category::Music => Bus::Music,
            Category::Voice | Category::Dialogue => Bus::Voice,
        }
    }

    /// Whether the sound follows game time: it pauses with the game and plays faster at
    /// a higher game speed. Interface sounds and music follow the wall clock.
    pub fn game_time(self) -> bool {
        matches!(self, Category::Effect | Category::World)
    }
}

/// The voice budget of one category.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    /// Voices playing at once.
    pub voices: u16,
    /// Voices of one sound playing at once.
    pub per_sound: u16,
    /// A one-shot of a sound started within this many seconds of another of the same
    /// sound in the category is dropped: simultaneous copies only add loudness and
    /// phasing.
    pub retrigger: f32,
    /// A one-shot that has not had a voice by this many seconds into it never starts:
    /// a transient heard late sounds wrong. Infinite for effect layers, which the old
    /// game started late (at the right offset) when a voice became free.
    pub late_start: f32,
}

/// Voice budgets per category.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub budgets: [Budget; 7],
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            budgets: [
                // The old game's 24 effect voices.
                Budget {
                    voices: 24,
                    per_sound: 6,
                    retrigger: 0.0,
                    late_start: f32::INFINITY,
                },
                Budget {
                    voices: 16,
                    per_sound: 4,
                    retrigger: 0.04,
                    late_start: 0.15,
                },
                Budget {
                    voices: 8,
                    per_sound: 2,
                    retrigger: 0.03,
                    late_start: 0.1,
                },
                // Two tracks while crossfading.
                Budget {
                    voices: 2,
                    per_sound: 2,
                    retrigger: 0.0,
                    late_start: f32::INFINITY,
                },
                // One answer at a time (two while one fades for the next).
                Budget {
                    voices: 2,
                    per_sound: 2,
                    retrigger: 0.0,
                    late_start: 0.25,
                },
                Budget {
                    voices: 8,
                    per_sound: 3,
                    retrigger: 0.0,
                    late_start: f32::INFINITY,
                },
                Budget {
                    voices: 2,
                    per_sound: 1,
                    retrigger: 0.0,
                    late_start: f32::INFINITY,
                },
            ],
        }
    }
}

impl Limits {
    pub fn budget(&self, category: Category) -> &Budget {
        &self.budgets[category.index()]
    }

    pub fn budget_mut(&mut self, category: Category) -> &mut Budget {
        &mut self.budgets[category.index()]
    }

    /// Voices a bus can have at once, playing or fading out.
    pub(crate) fn bus_capacity(&self, bus: Bus) -> usize {
        let voices: usize = Category::ALL
            .iter()
            .filter(|c| c.bus() == bus)
            .map(|&c| usize::from(self.budget(c).voices))
            .sum();
        // Voices that lost their slot fade out for a few milliseconds while their
        // replacements start.
        voices * 2 + 8
    }
}

/// Below this gain a sound is inaudible (−60 dB) and never takes a voice.
pub const AUDIBLE: f32 = 0.001;

/// A source's wish to be heard this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Identifies the source across frames.
    pub key: u64,
    pub sound: SoundId,
    pub category: Category,
    /// Where in the sound to be, in seconds of the sound (already wrapped for loops).
    pub offset: f64,
    /// Playback rate: pitch times game speed.
    pub rate: f64,
    /// Linear gain of the voice: volume, fades and distance (not the bus volumes).
    pub gain: f32,
    pub pan: f32,
    pub looping: bool,
    /// Multiplies audibility when ranking.
    pub priority: f32,
    /// A sound event's group and its most copies at once, across its clips.
    pub group: Option<(u32, u16)>,
}

/// Picks the requests that get voices. `playing` says whether a request's key holds a
/// voice now: playing voices get a bonus so near-equal sources do not flap. Returns the
/// indices of granted requests in `granted`, ordered by rank.
#[derive(Default)]
pub struct Selector {
    order: Vec<(f32, u64, usize)>,
    per_sound: HashMap<(Category, SoundId), u16>,
    per_group: HashMap<u32, u16>,
}

/// How much louder a newcomer must be to take a playing voice's place.
const HOLD: f32 = 1.25;

impl Selector {
    pub fn select(
        &mut self,
        requests: &[Request],
        limits: &Limits,
        audible: impl Fn(Category) -> bool,
        playing: impl Fn(u64) -> bool,
        granted: &mut Vec<usize>,
    ) -> [u16; 7] {
        granted.clear();
        self.order.clear();
        self.per_sound.clear();
        self.per_group.clear();
        for (i, r) in requests.iter().enumerate() {
            if !audible(r.category) || r.gain.is_nan() || r.gain < AUDIBLE {
                continue;
            }
            let held = playing(r.key);
            if !held && !r.looping && r.offset > f64::from(limits.budget(r.category).late_start) {
                continue;
            }
            let score = r.gain * r.priority.max(0.0) * if held { HOLD } else { 1.0 };
            self.order.push((score, r.key, i));
        }
        // Highest score first; ties go to the older source (smaller key).
        self.order
            .sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut used = [0u16; 7];
        for &(_, _, i) in &self.order {
            let r = &requests[i];
            let budget = limits.budget(r.category);
            let category = &mut used[r.category.index()];
            if *category >= budget.voices {
                continue;
            }
            if let Some((group, most)) = r.group
                && self.per_group.get(&group).copied().unwrap_or(0) >= most
            {
                continue;
            }
            let same = self.per_sound.entry((r.category, r.sound)).or_insert(0);
            if *same >= budget.per_sound {
                continue;
            }
            *same += 1;
            if let Some((group, _)) = r.group {
                *self.per_group.entry(group).or_insert(0) += 1;
            }
            *category += 1;
            granted.push(i);
        }
        used
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SoundLibrary;

    fn sounds() -> (SoundId, SoundId) {
        let mut library = SoundLibrary::new();
        (
            library.insert_pcm("a", 100, 1, &[0; 100]),
            library.insert_pcm("b", 100, 1, &[0; 100]),
        )
    }

    fn request(key: u64, sound: SoundId, category: Category, gain: f32) -> Request {
        Request {
            key,
            sound,
            category,
            offset: 0.0,
            rate: 1.0,
            gain,
            pan: 0.0,
            looping: false,
            priority: 1.0,
            group: None,
        }
    }

    fn select(requests: &[Request], limits: &Limits, playing: &[u64]) -> Vec<u64> {
        let mut granted = Vec::new();
        Selector::default().select(
            requests,
            limits,
            |_| true,
            |key| playing.contains(&key),
            &mut granted,
        );
        let mut keys: Vec<u64> = granted.iter().map(|&i| requests[i].key).collect();
        keys.sort();
        keys
    }

    #[test]
    fn a_crowd_plays_only_its_loudest_copies() {
        let (a, b) = sounds();
        let limits = Limits::default();
        // 400 copies of one sound at decreasing gain, and one quiet other sound.
        let mut requests: Vec<Request> = (0..400)
            .map(|i| request(i, a, Category::World, 1.0 - i as f32 / 500.0))
            .collect();
        requests.push(request(1000, b, Category::World, 0.01));
        let granted = select(&requests, &limits, &[]);
        let per_sound = usize::from(limits.budget(Category::World).per_sound);
        assert_eq!(granted.len(), per_sound + 1);
        assert_eq!(
            &granted[..per_sound],
            &(0..per_sound as u64).collect::<Vec<_>>()
        );
        assert_eq!(granted[per_sound], 1000);
    }

    #[test]
    fn category_budgets_are_independent() {
        let (a, b) = sounds();
        let mut limits = Limits::default();
        limits.budget_mut(Category::Effect).voices = 3;
        limits.budget_mut(Category::Effect).per_sound = 3;
        let mut requests: Vec<Request> = (0..10)
            .map(|i| {
                let sound = if i % 2 == 0 { a } else { b };
                request(i, sound, Category::Effect, 0.5 + i as f32 / 100.0)
            })
            .collect();
        requests.push(request(100, a, Category::Interface, 0.01));
        let granted = select(&requests, &limits, &[]);
        // The three loudest effects, and the interface sound regardless of loudness.
        assert_eq!(granted, vec![7, 8, 9, 100]);
    }

    #[test]
    fn inaudible_late_and_muted_requests_get_nothing() {
        let (a, _) = sounds();
        let limits = Limits::default();
        let mut late = request(2, a, Category::World, 1.0);
        late.offset = 0.5;
        let mut late_loop = late.clone();
        late_loop.key = 3;
        late_loop.looping = true;
        let mut late_effect = late.clone();
        late_effect.key = 4;
        late_effect.category = Category::Effect;
        let requests = vec![
            request(1, a, Category::World, AUDIBLE / 2.0),
            late.clone(),
            late_loop,
            late_effect,
            request(5, a, Category::Music, 1.0),
        ];
        assert_eq!(select(&requests, &limits, &[]), vec![3, 4, 5]);
        // A voice that is already playing keeps going however far in it is.
        assert_eq!(select(&requests, &limits, &[2]), vec![2, 3, 4, 5]);
        let mut granted = Vec::new();
        Selector::default().select(
            &requests,
            &limits,
            |c| c != Category::Music,
            |_| false,
            &mut granted,
        );
        assert!(
            granted
                .iter()
                .all(|&i| requests[i].category != Category::Music)
        );
    }

    #[test]
    fn playing_voices_hold_against_slightly_louder_newcomers() {
        let (a, _) = sounds();
        let mut limits = Limits::default();
        limits.budget_mut(Category::World).per_sound = 1;
        let requests = vec![
            request(1, a, Category::World, 0.5),
            request(2, a, Category::World, 0.6),
        ];
        assert_eq!(select(&requests, &limits, &[]), vec![2]);
        assert_eq!(select(&requests, &limits, &[1]), vec![1]);
        let louder = vec![
            request(1, a, Category::World, 0.5),
            request(2, a, Category::World, 0.7),
        ];
        assert_eq!(select(&louder, &limits, &[1]), vec![2]);
    }
}
