//! A mission's saved cameras (`mission.cameras`): named views a scene script cuts or
//! blends to (`mission.camera_to`), placed and framed in the map editor. Warcraft III's
//! camera objects: a target on the ground, the eye's distance, angle of attack and
//! rotation around it, the lens, and a roll.

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Eye-to-target distance, world units (one per navigation cell).
pub const CAMERA_DISTANCE: std::ops::RangeInclusive<f64> = 2.0..=400.0;
/// Angle of attack, degrees below the horizon.
pub const CAMERA_PITCH: std::ops::RangeInclusive<f64> = -30.0..=89.0;
/// Vertical field of view, degrees.
pub const CAMERA_FOV: std::ops::RangeInclusive<f64> = 10.0..=100.0;
/// Roll, degrees either way.
pub const CAMERA_ROLL: std::ops::RangeInclusive<f64> = -90.0..=90.0;
/// The target's lift above the ground, world units.
pub const CAMERA_HEIGHT: std::ops::RangeInclusive<f64> = -10.0..=60.0;

/// A saved camera. Yaw 0 puts the eye due south of the target looking north, as the
/// game's own camera; positive yaw turns the eye clockwise seen from above (towards the
/// east).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapCamera {
    /// Unique among the mission's cameras; scripts name it.
    pub id: String,
    /// What it looks at, navigation cells (fractions allowed).
    pub x: f64,
    pub y: f64,
    /// The target's lift above the ground under it, world units (0 when absent).
    pub height: Option<f64>,
    /// Eye-to-target distance, world units (2 to 400; the game's camera stands 117 away).
    pub distance: f64,
    /// Angle of attack, degrees below the horizon (−30 to 89; the game's is 56).
    pub pitch: f64,
    /// Rotation round the target, degrees (any; 0 looks north).
    pub yaw: f64,
    /// Vertical field of view, degrees (10 to 100; the game's is 34.3 at 16:9).
    pub fov: f64,
    /// Roll, degrees (−90 to 90; 0 when absent).
    pub roll: Option<f64>,
}

impl MapCamera {
    /// What is wrong with the camera on a map of `size` cells, if anything.
    pub fn problem(&self, size: u32) -> Option<String> {
        let id = &self.id;
        let within = |value: f64, range: &std::ops::RangeInclusive<f64>| {
            value.is_finite() && range.contains(&value)
        };
        if id.is_empty() || id.chars().count() > 64 {
            return Some("Camera ids are 1 to 64 characters".into());
        }
        let edge = f64::from(size);
        if !(within(self.x, &(0.0..=edge)) && within(self.y, &(0.0..=edge))) {
            return Some(format!("Camera {id} looks outside the map"));
        }
        let checks = [
            (Some(self.distance), &CAMERA_DISTANCE, "distance"),
            (Some(self.pitch), &CAMERA_PITCH, "pitch"),
            (Some(self.fov), &CAMERA_FOV, "field of view"),
            (self.roll, &CAMERA_ROLL, "roll"),
            (self.height, &CAMERA_HEIGHT, "height"),
        ];
        for (value, range, name) in checks {
            if let Some(value) = value
                && !within(value, range)
            {
                return Some(format!(
                    "Camera {id}: {name} must be {} to {}",
                    range.start(),
                    range.end()
                ));
            }
        }
        (!self.yaw.is_finite()).then(|| format!("Camera {id}: yaw must be a number"))
    }
}
