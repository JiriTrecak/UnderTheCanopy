//! Campaign mission data embedded in a map (`mission`).

use crate::id::Id;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

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

/// Loading and selection screen material. Images are `asset.interface.*` ids.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Presentation {
    pub loading_background: Option<Id>,
    pub selection_background: Option<Id>,
    pub chapter: Option<String>,
    pub briefing: Option<String>,
    pub tips: Option<Vec<String>>,
    pub features: Option<Vec<String>>,
}

/// A campaign mission: metadata, objectives, regions and the Lua scenario script.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mission {
    /// Campaign id (`rules.campaigns`).
    pub campaign: String,
    pub title: String,
    /// 1 to 100, position within the campaign.
    pub order: u32,
    /// Race of every participant without an entry in `player_races`; defaults to the
    /// campaign's race, then the default race.
    pub race: Option<String>,
    /// `player.N` → race.
    pub player_races: Option<BTreeMap<String, String>>,
    /// Map id of the next mission.
    pub next_mission: Option<String>,
    /// Placement ids of player 1's units carried between chapters.
    pub company: Option<Vec<String>>,
    pub objectives: Option<Vec<Objective>>,
    pub presentation: Option<Presentation>,
    pub hero_level_cap: Option<u32>,
    /// Lua 5.3 source with `on_start` / `on_tick` callbacks.
    pub script: String,
    pub regions: Vec<Region>,
}
