//! A mission map's `mission` section: objectives, regions and the script.

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// A named circle scripts refer to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Region {
    pub id: String,
    /// Navigation cell, 0 to 2047.
    pub x: u32,
    pub y: u32,
    /// Above 0, at most 64.
    pub radius: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Objective {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub optional: bool,
}

/// A mission: objectives, regions and the Lua scenario script. Its campaign, its place
/// in it, its presentation, the player's race and the hero level cap are the campaign's
/// (`campaign.json`, docs/campaign.md); a mission map plays on its own too.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mission {
    pub objectives: Option<Vec<Objective>>,
    /// Lua 5.3 source with `on_start` / `on_tick` callbacks.
    pub script: String,
    pub regions: Vec<Region>,
    /// The computer players and the campaign AI that plays them (`players`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub players: Vec<super::players::MissionPlayer>,
}
