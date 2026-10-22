//! Cinematics in the saved mission state (docs/campaign.md "Scenes"): cinematic mode, the
//! saved-camera shot, the fade, scripted animations and the scene a script plays.
//!
//! The picture follows this state, so a save or a replay shows the same scene: the game
//! computes a camera blend or a fade from its start tick and length at the tick it draws.

use crate::entity::Milli;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Units playing a scripted animation at once.
pub const MAX_ANIMATIONS: usize = 32;
/// Moves a scene remembers to finish when it is skipped.
pub const MAX_SCENE_MOVES: usize = 64;
/// Fully opaque, in thousandths.
pub const OPAQUE: u16 = 1000;

/// Everything cinematic about a mission.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Cinema {
    /// Cinematic mode: letterbox, the HUD hidden, player commands refused.
    pub on: bool,
    /// The camera the view holds or blends to.
    pub shot: Option<Shot>,
    pub fade: Option<Fade>,
    /// Units playing a named animation, oldest first.
    pub animations: Vec<Animation>,
    /// The scene the script plays.
    pub scene: Option<ScenePlay>,
    /// Game tick at which the latest scene was skipped (the picture covers the jump).
    pub skipped: Option<u32>,
}

impl Cinema {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Where a camera stands: a saved camera (optionally panned to another target), or the
/// game's own camera (`camera: None`) looking at `target` (or wherever the player looks).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pose {
    /// A saved camera's id (`mission.cameras`).
    pub camera: Option<String>,
    /// The point looked at instead of the saved camera's own (a pan), milli-cells.
    pub target: Option<Milli>,
}

/// A cut (`ticks` 0) or a blend from one pose to another, eased in and out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub from: Pose,
    pub to: Pose,
    /// Game tick the blend starts.
    pub start: u32,
    pub ticks: u32,
}

impl Shot {
    /// The blend's progress at game tick `tick` (fractions while interpolating), 0 to 1,
    /// before easing.
    pub fn progress(&self, tick: f32) -> f32 {
        progress(self.start, self.ticks, tick)
    }
}

/// A fade filter between two opacities of one colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fade {
    /// Opacity in thousandths at the start and the end.
    pub from: u16,
    pub to: u16,
    pub color: [u8; 3],
    pub start: u32,
    pub ticks: u32,
}

impl Fade {
    /// Opacity in thousandths at game tick `tick`, exactly (the simulation's own use).
    pub fn at(&self, tick: u32) -> u16 {
        let done = tick.saturating_sub(self.start).min(self.ticks);
        if self.ticks == 0 {
            return self.to;
        }
        let (from, to) = (i64::from(self.from), i64::from(self.to));
        (from + (to - from) * i64::from(done) / i64::from(self.ticks)) as u16
    }

    /// Opacity 0 to 1 at game tick `tick` (fractions while interpolating), linear.
    pub fn opacity(&self, tick: f32) -> f32 {
        let t = progress(self.start, self.ticks, tick);
        (f32::from(self.from) + (f32::from(self.to) - f32::from(self.from)) * t) / f32::from(OPAQUE)
    }
}

/// A unit playing a named animation of its model (`attack`, `cast`, …) from `start`, once
/// (then back to its own) or held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Animation {
    pub placement: String,
    pub name: String,
    pub start: u32,
    pub looping: bool,
}

/// A scene in play (`mission.play`). Each run calls the scene's function from its top:
/// the statements already done are passed over, the next one runs, and a wait that is not
/// yet over ends the run there. A skip runs the rest at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenePlay {
    /// The function in the script's `scenes` table.
    pub name: String,
    /// Statements done.
    pub done: u32,
    /// Game tick the latest statement was done (waits count from here).
    pub since: u32,
    /// Escape skips it.
    pub skippable: bool,
    /// The player asked to skip: the next run plays the rest at once.
    pub skipping: bool,
    /// Units the scene sent walking, with where to: a skip puts those still on their
    /// way there.
    pub moves: Vec<SceneMove>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMove {
    pub placement: String,
    pub destination: Milli,
}

fn progress(start: u32, ticks: u32, tick: f32) -> f32 {
    if ticks == 0 {
        return 1.0;
    }
    ((tick - start as f32) / ticks as f32).clamp(0.0, 1.0)
}

/// Smoothstep: a blend that starts and ends gently.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A colour by name or `#rrggbb`.
pub fn color(text: &str) -> Option<[u8; 3]> {
    match text {
        "black" => Some([0, 0, 0]),
        "white" => Some([255, 255, 255]),
        _ => {
            let hex = text.strip_prefix('#').filter(|h| h.len() == 6)?;
            let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
            Some([byte(0)?, byte(2)?, byte(4)?])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_are_exact_in_the_simulation() {
        let fade = Fade {
            from: 1000,
            to: 0,
            color: [0; 3],
            start: 10,
            ticks: 40,
        };
        assert_eq!(fade.at(0), 1000);
        assert_eq!(fade.at(30), 500);
        assert_eq!(fade.at(90), 0);
        assert!((fade.opacity(30.0) - 0.5).abs() < 1e-6);
        assert_eq!(color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(color("red"), None);
        assert_eq!(ease(0.5), 0.5);
    }
}
