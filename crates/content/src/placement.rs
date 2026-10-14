//! Things a map places: definition instances and neutral camps.

use crate::id::Id;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// `"none"` or `"player.1"` … `"player.8"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    Nobody,
    /// Zero-based player slot.
    Player(u8),
}

impl Serialize for Owner {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Nobody => serializer.serialize_str("none"),
            Self::Player(slot) => serializer.serialize_str(&format!("player.{}", slot + 1)),
        }
    }
}

impl<'de> Deserialize<'de> for Owner {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        match text.as_str() {
            "none" => Ok(Self::Nobody),
            _ => text
                .strip_prefix("player.")
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| (1..=8).contains(n) && text.len() == 8)
                .map(|n| Self::Player(n - 1))
                .ok_or_else(|| serde::de::Error::custom(format!("invalid owner {text:?}"))),
        }
    }
}

/// A navigation-cell address. Placements of static foundations may sit on half cells.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub surface: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MapKnowledge {
    Public,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Activation {
    /// Created only when a scenario script activates it.
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InitialConstruction {
    Complete,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialState {
    pub health: Option<u32>,
    pub experience: Option<u32>,
    pub construction: Option<InitialConstruction>,
    pub amount: Option<u32>,
    pub inventory: Option<BTreeMap<Id, u32>>,
    pub quantity: Option<u32>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appearance {
    pub asset: Option<Id>,
    pub scale: Option<f64>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Placement {
    pub activation: Option<Activation>,
    pub id: String,
    pub map_knowledge: Option<MapKnowledge>,
    pub definition: Id,
    pub position: Point,
    #[serde(default)]
    pub rotation: f64,
    pub owner: Owner,
    pub initial_state: Option<InitialState>,
    pub appearance: Option<Appearance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Aggression {
    Players,
    Passive,
}

/// A neutral camp: placed members that guard a home point and drop loot.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Camp {
    pub id: String,
    pub map_knowledge: Option<MapKnowledge>,
    /// Placement ids.
    pub members: Vec<String>,
    pub home: Point,
    pub aggro_range: u32,
    pub leash: u32,
    pub aggression: Aggression,
    pub loot_pool: Option<Id>,
    pub fixed_drops: Option<Vec<Id>>,
    pub legendary: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_round_trip() {
        for (text, owner) in [
            ("none", Owner::Nobody),
            ("player.1", Owner::Player(0)),
            ("player.8", Owner::Player(7)),
        ] {
            let parsed: Owner = serde_json::from_value(text.into()).unwrap();
            assert_eq!(parsed, owner);
            assert_eq!(serde_json::to_value(owner).unwrap(), text);
        }
        for text in ["player.0", "player.9", "player.10", "player.01", "nobody"] {
            assert!(
                serde_json::from_value::<Owner>(text.into()).is_err(),
                "{text}"
            );
        }
    }
}
