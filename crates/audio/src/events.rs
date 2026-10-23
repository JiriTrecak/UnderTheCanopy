//! Sound events (D97) ready to play: each content event's clips found in the library, its
//! ranges, distances, cooldown and limit. Starting one picks a clip (never the last one
//! again when it has others), a volume and a pitch in its ranges. The picks are
//! presentation only and use their own generator; nothing here reaches the simulation.

use crate::{Category, SoundId, SoundLibrary, sources::PlayOptions, spatial::Distances};
use canopy_content::sounds::{SoundEvent, Sounds, default_distance};
use std::collections::HashMap;

/// A bound sound event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EventId(u32);

/// Seconds a loop fades in and out unless its event says otherwise.
const DEFAULT_FADE: f32 = 0.25;

struct Bound {
    category: Category,
    clips: Vec<SoundId>,
    volume: (f32, f32),
    pitch: (f32, f32),
    looping: bool,
    distances: Distances,
    cooldown: f64,
    limit: Option<u16>,
    fade: f32,
    /// The clip picked last, and when (on its category's clock).
    last: Option<(usize, f64)>,
}

/// One start of an event: the clip and how to play it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Start {
    pub sound: SoundId,
    pub options: PlayOptions,
}

/// Every sound event of the content whose clips the library has.
#[derive(Default)]
pub struct SoundBank {
    events: Vec<Bound>,
    by_id: HashMap<String, EventId>,
    seed: u64,
}

impl SoundBank {
    /// Binds `sounds` to the library's clips (an event with no clip found is left out,
    /// with a warning); events already bound under the same id are replaced.
    pub fn bind(&mut self, sounds: &Sounds, library: &SoundLibrary) {
        for event in sounds.iter() {
            self.add(event, library);
        }
    }

    fn add(&mut self, event: &SoundEvent, library: &SoundLibrary) {
        let clips: Vec<SoundId> = event
            .clips
            .iter()
            .filter_map(|asset| library.find_asset(asset.as_str()))
            .collect();
        if clips.is_empty() {
            tracing::warn!(sound = %event.id, "audio: no clip of this sound is in the pack");
            return;
        }
        let range = |r: canopy_content::sounds::Range| {
            let (a, b) = r.bounds();
            (a as f32, b as f32)
        };
        let distance = event
            .distance
            .unwrap_or_else(|| default_distance(event.category));
        let bound = Bound {
            category: Category::of(event.category),
            clips,
            volume: range(event.volume),
            pitch: range(event.pitch),
            looping: event.looping,
            distances: Distances::new(distance.reference as f32, distance.max as f32),
            cooldown: event.cooldown,
            limit: event.limit.map(|l| l.min(u32::from(u16::MAX)) as u16),
            fade: event.fade.map_or(DEFAULT_FADE, |f| f as f32),
            last: None,
        };
        match self.by_id.get(event.id.as_str()) {
            Some(&id) => self.events[id.0 as usize] = bound,
            None => {
                let id = EventId(self.events.len() as u32);
                self.events.push(bound);
                self.by_id.insert(event.id.to_string(), id);
            }
        }
    }

    pub fn find(&self, id: &str) -> Option<EventId> {
        self.by_id.get(id).copied()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn category(&self, event: EventId) -> Category {
        self.events[event.0 as usize].category
    }

    /// Every clip of an event (a music event's tracks).
    pub fn clips(&self, event: EventId) -> &[SoundId] {
        &self.events[event.0 as usize].clips
    }

    /// A start of `event` at `now` (seconds on its category's clock), or `None` while its
    /// cooldown runs.
    pub fn start(&mut self, event: EventId, now: f64) -> Option<Start> {
        let mut roll = {
            let seed = &mut self.seed;
            move || {
                // xorshift64*: presentation only.
                *seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut x = *seed;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
            }
        };
        let e = &mut self.events[event.0 as usize];
        if let Some((_, at)) = e.last
            && e.cooldown > 0.0
            && (now - at).abs() < e.cooldown
        {
            return None;
        }
        let count = e.clips.len();
        let mut pick = ((roll() * count as f64) as usize).min(count - 1);
        if let Some((last, _)) = e.last
            && count > 1
            && pick == last
        {
            pick = (pick + 1 + ((roll() * (count - 1) as f64) as usize).min(count - 2)) % count;
        }
        e.last = Some((pick, now));
        let between = |(a, b): (f32, f32), t: f64| a + (b - a) * t as f32;
        Some(Start {
            sound: e.clips[pick],
            options: PlayOptions {
                category: e.category,
                volume: between(e.volume, roll()),
                pitch: between(e.pitch, roll()),
                looping: e.looping,
                distances: e.distances,
                fade_in: if e.looping { e.fade } else { 0.0 },
                fade_out: if e.looping { e.fade } else { 0.0 },
                priority: 1.0,
                group: e.limit.map(|l| (event.0, l)),
            },
        })
    }

    /// Forgets every event (content changed).
    pub fn clear(&mut self) {
        self.events.clear();
        self.by_id.clear();
    }
}
