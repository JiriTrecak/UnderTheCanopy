//! Unit classifications used for targeting.

use serde::{Deserialize, Serialize};

/// Ground units path around obstacles; air units ignore them and collide only with air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Locomotion {
    Ground,
    Air,
}

/// Spell-targeting classification; never changes armour or movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnitNature {
    Organic,
    Undead,
    Mechanical,
}
