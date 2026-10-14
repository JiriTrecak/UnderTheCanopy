//! Player commands: the frozen multiplayer protocol's `Action`.
//!
//! The JSON shape matches the old game's `actionSchema` exactly; the match server validates
//! it, so it must never change (docs/decisions.md D10).

use canopy_content::Id;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// An entity id as it appears in commands.
pub type Actor = u32;

/// Most units one command may address.
pub const MAX_ACTORS: usize = 160;

/// A whole navigation cell, optionally on a walk surface (bridge deck).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellPoint {
    pub x: u32,
    pub y: u32,
    pub surface: Option<String>,
}

/// A foundation position: whole or half cells.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FoundationPoint {
    pub x: HalfCells,
    pub y: HalfCells,
    pub surface: Option<String>,
}

/// A coordinate in half navigation cells, written as a JSON number (`10`, `10.5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HalfCells(pub u32);

impl Serialize for HalfCells {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.is_multiple_of(2) {
            serializer.serialize_u32(self.0 / 2)
        } else {
            serializer.serialize_f64(f64::from(self.0) / 2.0)
        }
    }
}

impl<'de> Deserialize<'de> for HalfCells {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        let halves = value * 2.0;
        if halves.fract() != 0.0 || !(0.0..=4094.0).contains(&halves) {
            return Err(serde::de::Error::custom(format!(
                "{value} is not a whole or half cell on the map"
            )));
        }
        Ok(Self(halves as u32))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlainPoint {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CastTarget {
    Unit { entity: Actor },
    Point { position: PlainPoint },
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    AbilityAutocast {
        actor: Actor,
        binding: String,
        enabled: bool,
    },
    CastAbility {
        actor: Actor,
        binding: String,
        target: CastTarget,
    },
    Garrison {
        actors: Vec<Actor>,
        target: Actor,
        append: Option<bool>,
    },
    Unload {
        actor: Actor,
    },
    Research {
        actor: Actor,
        research: Id,
    },
    CancelResearch {
        actor: Actor,
        research: Id,
    },
    Upgrade {
        actor: Actor,
    },
    CancelUpgrade {
        actor: Actor,
    },
    Revive {
        actor: Actor,
        hero: Actor,
    },
    CancelRevival {
        actor: Actor,
        hero: Actor,
    },
    LearnAbility {
        actor: Actor,
        ability: String,
    },
    Gather {
        actors: Vec<Actor>,
        target: Actor,
        append: Option<bool>,
    },
    Construct {
        actors: Vec<Actor>,
        target: Actor,
        append: Option<bool>,
    },
    Pickup {
        actor: Actor,
        target: Actor,
        append: Option<bool>,
    },
    DropItem {
        actor: Actor,
        slot: u32,
    },
    UseItem {
        actor: Actor,
        slot: u32,
    },
    #[serde(rename_all = "camelCase")]
    Move {
        actors: Vec<Actor>,
        destination: CellPoint,
        attack_move: Option<bool>,
        append: Option<bool>,
    },
    Attack {
        actors: Vec<Actor>,
        target: Actor,
        force: Option<bool>,
        append: Option<bool>,
    },
    Stop {
        actors: Vec<Actor>,
    },
    Hold {
        actors: Vec<Actor>,
        append: Option<bool>,
    },
    Patrol {
        actors: Vec<Actor>,
        destination: CellPoint,
        append: Option<bool>,
    },
    Follow {
        actors: Vec<Actor>,
        target: Actor,
        append: Option<bool>,
    },
    Build {
        actors: Vec<Actor>,
        definition: Id,
        position: FoundationPoint,
        rotation: Option<i32>,
        append: Option<bool>,
    },
    Produce {
        actor: Actor,
        definition: Id,
    },
    Cancel {
        actor: Actor,
        queue: Option<Actor>,
    },
    Rally {
        actor: Actor,
        /// Always present on the wire; `null` clears the rally point.
        #[serialize_always]
        #[serde(with = "required_nullable")]
        destination: Option<CellPoint>,
        target: Option<Actor>,
    },
    Pause {
        actor: Actor,
        paused: bool,
    },
    Noop,
    Ping,
}

/// Serializes `None` as `null` and requires the key when reading.
mod required_nullable {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(
        value: &Option<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.serialize(serializer)
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<T>, D::Error> {
        Option::<T>::deserialize(deserializer)
    }
}

impl Action {
    /// The structural rules of the protocol beyond field types: what the match server and
    /// every peer reject. Returns the first violation.
    pub fn problem(&self) -> Option<&'static str> {
        let actors = match self {
            Self::Garrison { actors, .. }
            | Self::Gather { actors, .. }
            | Self::Construct { actors, .. }
            | Self::Move { actors, .. }
            | Self::Attack { actors, .. }
            | Self::Stop { actors }
            | Self::Hold { actors, .. }
            | Self::Patrol { actors, .. }
            | Self::Follow { actors, .. }
            | Self::Build { actors, .. } => Some(actors),
            _ => None,
        };
        if let Some(actors) = actors {
            if actors.is_empty() || actors.len() > MAX_ACTORS {
                return Some("commands address 1 to 160 actors");
            }
            let mut sorted = actors.clone();
            sorted.sort_unstable();
            if sorted.windows(2).any(|w| w[0] == w[1]) {
                return Some("duplicate actors");
            }
            if sorted.first() == Some(&0) {
                return Some("entity ids are positive");
            }
        }
        let ids = self.ids();
        if ids.contains(&0) {
            return Some("entity ids are positive");
        }
        match self {
            Self::AbilityAutocast { binding, .. } if binding.chars().count() > 64 => {
                Some("binding too long")
            }
            Self::CastAbility { binding, .. } if !binding_name(binding) => Some("invalid binding"),
            Self::CastAbility {
                target: CastTarget::Point { position },
                ..
            } if !on_map(position.x, position.y) => Some("point outside the map range"),
            Self::LearnAbility { ability, .. } if ability.is_empty() => Some("ability required"),
            Self::DropItem { slot, .. } | Self::UseItem { slot, .. } if *slot > 11 => {
                Some("item slot out of range")
            }
            Self::Move { destination, .. } | Self::Patrol { destination, .. }
                if !cell(destination) =>
            {
                Some("point outside the map range")
            }
            Self::Rally {
                destination: Some(destination),
                ..
            } if !cell(destination) => Some("point outside the map range"),
            Self::Build {
                position, rotation, ..
            } => {
                if !position.surface.as_deref().is_none_or(surface_name) {
                    Some("invalid foundation position")
                } else if rotation.is_some_and(|r| r % 90 != 0) {
                    Some("buildings rotate in quarter turns")
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn ids(&self) -> Vec<Actor> {
        let mut ids = Vec::new();
        match self {
            Self::AbilityAutocast { actor, .. }
            | Self::Unload { actor }
            | Self::Research { actor, .. }
            | Self::CancelResearch { actor, .. }
            | Self::Upgrade { actor }
            | Self::CancelUpgrade { actor }
            | Self::LearnAbility { actor, .. }
            | Self::DropItem { actor, .. }
            | Self::UseItem { actor, .. }
            | Self::Produce { actor, .. }
            | Self::Pause { actor, .. } => ids.push(*actor),
            Self::CastAbility { actor, target, .. } => {
                ids.push(*actor);
                if let CastTarget::Unit { entity } = target {
                    ids.push(*entity);
                }
            }
            Self::Revive { actor, hero } | Self::CancelRevival { actor, hero } => {
                ids.extend([*actor, *hero])
            }
            Self::Pickup { actor, target, .. } => ids.extend([*actor, *target]),
            Self::Cancel { actor, queue } => {
                ids.extend([Some(*actor), *queue].into_iter().flatten())
            }
            Self::Rally { actor, target, .. } => {
                ids.extend([Some(*actor), *target].into_iter().flatten())
            }
            Self::Garrison { target, .. }
            | Self::Gather { target, .. }
            | Self::Construct { target, .. }
            | Self::Attack { target, .. }
            | Self::Follow { target, .. } => ids.push(*target),
            _ => {}
        }
        ids
    }
}

fn on_map(x: u32, y: u32) -> bool {
    x <= 2047 && y <= 2047
}

fn surface_name(surface: &str) -> bool {
    (1..=160).contains(&surface.chars().count())
}

fn cell(point: &CellPoint) -> bool {
    on_map(point.x, point.y) && point.surface.as_deref().is_none_or(surface_name)
}

/// `^[a-z][a-z0-9-]*$`, at most 64 characters.
fn binding_name(text: &str) -> bool {
    let mut chars = text.chars();
    text.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(value: serde_json::Value) -> Action {
        let action: Action = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&action).unwrap(), value);
        action
    }

    #[test]
    fn wire_shapes_round_trip() {
        round_trip(
            json!({"type": "move", "actors": [3, 4], "destination": {"x": 10, "y": 20}, "attackMove": true}),
        );
        round_trip(
            json!({"type": "castAbility", "actor": 3, "binding": "bolt", "target": {"kind": "point", "position": {"x": 1, "y": 2}}}),
        );
        round_trip(
            json!({"type": "build", "actors": [5], "definition": "building.test.hall", "position": {"x": 10.5, "y": 4}, "rotation": 90}),
        );
        round_trip(json!({"type": "rally", "actor": 7, "destination": null}));
        round_trip(
            json!({"type": "rally", "actor": 7, "destination": {"x": 1, "y": 1}, "target": 9}),
        );
        round_trip(json!({"type": "noop"}));
    }

    #[test]
    fn rejects_malformed_commands() {
        let parse = |v| serde_json::from_value::<Action>(v);
        assert!(
            parse(json!({"type": "rally", "actor": 7})).is_err(),
            "destination is required"
        );
        assert!(parse(json!({"type": "stop", "actors": [1], "extra": 1})).is_err());
        assert!(parse(json!({"type": "fly"})).is_err());
        let problem = |v| parse(v).unwrap().problem();
        assert!(problem(json!({"type": "stop", "actors": []})).is_some());
        assert!(problem(json!({"type": "stop", "actors": [2, 2]})).is_some());
        assert!(problem(json!({"type": "stop", "actors": [0]})).is_some());
        assert!(parse(json!({"type": "build", "actors": [1], "definition": "building.a", "position": {"x": 1.25, "y": 1}})).is_err());
        assert!(problem(json!({"type": "build", "actors": [1], "definition": "building.a", "position": {"x": 1, "y": 1}, "rotation": 45})).is_some());
        assert!(problem(json!({"type": "castAbility", "actor": 1, "binding": "Bolt", "target": {"kind": "unit", "entity": 2}})).is_some());
        assert!(problem(json!({"type": "useItem", "actor": 1, "slot": 12})).is_some());
        assert!(
            problem(json!({"type": "move", "actors": [1], "destination": {"x": 2048, "y": 0}}))
                .is_some()
        );
        assert_eq!(
            problem(json!({"type": "move", "actors": [1], "destination": {"x": 2047, "y": 0}})),
            None
        );
    }
}
