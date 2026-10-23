//! What units and buildings sound like as they are drawn (D97): a definition's
//! `presentation.sounds` loop while it is in a state, play once on clip events it crosses
//! and at moments of its life (spawn, complete, death, impact). The scene only says what
//! should be heard where; the game plays it.

use canopy_content::sounds::{Moment, SoundBinding};
use glam::Vec3;

/// A sound the scene asks for this tick.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundCue {
    /// The sound event.
    pub sound: String,
    /// Where on the ground (x, z).
    pub at: [f32; 2],
    /// A loop, held for as long as this key is asked for; `None` plays once.
    pub hold: Option<u64>,
}

/// The loop key of binding `index` of `entity`.
fn key(entity: u32, index: usize) -> u64 {
    (u64::from(entity) << 8) | (index as u64 & 0xff)
}

/// The state loops and clip-event one-shots of one model this tick.
pub fn of_state(
    out: &mut Vec<SoundCue>,
    sounds: &[SoundBinding],
    entity: u32,
    shown: bool,
    in_state: &dyn Fn(&str) -> bool,
    events: &[String],
    at: Vec3,
) {
    if !shown {
        return;
    }
    for (i, binding) in sounds.iter().enumerate() {
        let cue = |hold| SoundCue {
            sound: binding.sound.to_string(),
            at: [at.x, at.z],
            hold,
        };
        if let Some(state) = &binding.state {
            if in_state(state) {
                out.push(cue(Some(key(entity, i))));
            }
        } else if let Some(event) = &binding.event {
            for _ in events.iter().filter(|e| *e == event) {
                out.push(cue(None));
            }
        }
    }
}

/// The one-shots of a moment.
pub fn of_moment(out: &mut Vec<SoundCue>, sounds: &[SoundBinding], moment: Moment, at: [f32; 2]) {
    for binding in sounds.iter().filter(|b| b.on == Some(moment)) {
        out.push(SoundCue {
            sound: binding.sound.to_string(),
            at,
            hold: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn states_hold_events_play_and_moments_mark() {
        let sounds: Vec<SoundBinding> = serde_json::from_value(json!([
            {"sound": "sound.t.hum", "state": "work"},
            {"sound": "sound.t.clank", "event": "strike"},
            {"sound": "sound.t.fall", "on": "death"},
        ]))
        .unwrap();
        let mut out = Vec::new();
        let working = |s: &str| s == "work";
        let events = ["strike".to_string(), "strike".to_string()];
        of_state(
            &mut out,
            &sounds,
            7,
            true,
            &working,
            &events,
            Vec3::new(1.0, 5.0, 2.0),
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].hold, Some(key(7, 0)));
        assert_eq!(out[0].at, [1.0, 2.0]);
        assert!(
            out[1..]
                .iter()
                .all(|c| c.hold.is_none() && c.sound == "sound.t.clank")
        );
        out.clear();
        of_state(&mut out, &sounds, 7, false, &working, &events, Vec3::ZERO);
        assert!(out.is_empty(), "out of sight, silent");
        of_moment(&mut out, &sounds, Moment::Death, [3.0, 4.0]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sound, "sound.t.fall");
    }
}
