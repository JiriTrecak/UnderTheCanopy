//! The saved part of a mission: script progress, presentation state and the pause clock.

use crate::entity::{EntityId, Milli};
use canopy_content::Id;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// Variables a script may hold at the end of a run.
pub const MAX_VARIABLES: usize = 128;
/// UTF-16 code units in a variable's string value.
pub const MAX_VARIABLE_TEXT: usize = 2000;
/// Units turning toward a scripted facing at once.
pub const MAX_FACING: usize = 32;

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionState {
    /// `on_start` has run; later runs call `on_tick`.
    pub started: bool,
    /// Ticks the game stood still for cinematic dialogue. The lockstep clock is the game
    /// tick plus these.
    pub paused_ticks: u32,
    pub variables: BTreeMap<String, Variable>,
    /// Script-activated placements already spawned, in spawn order.
    pub spawned: Vec<String>,
    pub objective_states: BTreeMap<String, ObjectiveState>,
    /// The objective line the HUD shows.
    pub objective: String,
    pub dialogue: Option<Dialogue>,
    /// A scripted camera scene; player commands are refused while one is set.
    pub scene: Option<Scene>,
    /// Units turning toward scripted points, advanced every tick.
    pub facing: Vec<Facing>,
    pub next_dialogue: u32,
    /// Set once by the first failed run; the script never runs again.
    pub error: Option<String>,
    /// Cinematic mode, shots, fades, animations and the scene in play (`cinema.rs`).
    #[serde(default, skip_serializing_if = "super::cinema::Cinema::is_empty")]
    pub cinema: super::cinema::Cinema,
    /// How often the campaign player clicked each placed entity (`mission.clicks`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub clicks: BTreeMap<String, u32>,
}

impl Default for MissionState {
    fn default() -> Self {
        Self {
            started: false,
            paused_ticks: 0,
            variables: BTreeMap::new(),
            spawned: Vec::new(),
            objective_states: BTreeMap::new(),
            objective: String::new(),
            dialogue: None,
            scene: None,
            facing: Vec::new(),
            next_dialogue: 1,
            error: None,
            cinema: super::cinema::Cinema::default(),
            clicks: BTreeMap::new(),
        }
    }
}

/// A script variable. Lua integers and floats stay distinct, as the script sees them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Variable {
    String(String),
    Integer(i64),
    /// Saved as its shortest exact decimal text, so restoring never changes a bit.
    Number(#[serde(with = "exact")] f64),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectiveState {
    Active,
    Completed,
    Failed,
}

/// The line being spoken.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialogue {
    pub id: u32,
    /// The speaking unit, shown with the line.
    pub actor: Option<EntityId>,
    pub speaker: String,
    /// The definition whose icon is the portrait.
    pub portrait: Id,
    pub text: String,
    pub duration_ticks: u32,
    /// A cinematic line pauses the game until it ends.
    pub cinematic: bool,
    /// Paused ticks left of a cinematic line.
    pub remaining: u32,
    /// Game tick at which a line stops showing.
    pub until: u32,
}

/// A scripted camera scene.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub position: Milli,
    pub camera: Option<Camera>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraMode {
    Rts,
    ThirdPerson,
    FirstPerson,
}

impl CameraMode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "rts" => Some(Self::Rts),
            "third-person" => Some(Self::ThirdPerson),
            "first-person" => Some(Self::FirstPerson),
            _ => None,
        }
    }
}

/// A unit camera shot.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Camera {
    pub mode: CameraMode,
    /// Placement id of the subject unit.
    pub entity: String,
    /// Placement id of a unit the camera looks at.
    pub look_at: Option<String>,
    /// Milli-cells.
    pub distance: Option<u32>,
    /// Milli-cells.
    pub height: Option<u32>,
    /// Field of view in millidegrees.
    pub fov: Option<u32>,
    pub transition_ms: u32,
}

/// A unit turning toward a scripted point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facing {
    pub placement: String,
    pub target: Milli,
}

/// `f64` as the shortest decimal text that parses back to the same bits (JSON number
/// parsing is not guaranteed to round-trip every float).
mod exact {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| D::Error::custom(format!("invalid number {text:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_round_trip_bit_exactly() {
        for value in [
            0.1 + 0.2,
            1e-300,
            5e-324,
            1.7976931348623157e308,
            -0.0,
            1.0 / 3.0,
        ] {
            let text = serde_json::to_string(&Variable::Number(value)).unwrap();
            let Variable::Number(back) = serde_json::from_str(&text).unwrap() else {
                panic!("{text}");
            };
            assert_eq!(back.to_bits(), value.to_bits(), "{text}");
        }
    }
}
