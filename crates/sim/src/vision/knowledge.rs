//! What players know, as saved state (old `Observation` memories and `spellVisions`).
//!
//! Allies share all ordinary sight, so fog and remembered scenery belong to a *team*: the
//! slot's `team`, or its player index when it has none (the old hostility rule). Eyewitness
//! reports of deaths stay per owner.

use crate::{
    catalog::Def,
    entity::{Entity, EntityId, Milli, Resource},
};
use canopy_content::placement::Owner;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{collections::BTreeMap, sync::Arc};

/// Fog cell: never seen.
pub const UNEXPLORED: u8 = 0;
/// Fog cell: seen before, not now.
pub const EXPLORED: u8 = 1;
/// Fog cell: in sight now.
pub const VISIBLE: u8 = 2;

/// Eyewitness reports older than this many ticks are forgotten.
pub const DEATH_MEMORY_TICKS: u32 = 400;
/// Most eyewitness reports an owner keeps.
pub const MAX_OBSERVED_DEATHS: usize = 1280;
/// Death cues are shown for this many ticks.
pub const DEATH_CUE_TICKS: u32 = 80;
/// Most temporary sight sources at once.
pub const MAX_SPELL_VISIONS: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Knowledge {
    /// One per team of the match, by team number.
    pub teams: Vec<TeamKnowledge>,
    /// One per player of the match, by owner.
    pub owners: Vec<OwnerKnowledge>,
    /// Temporary sight sources from abilities, by id.
    pub spells: Vec<SpellVision>,
    /// Recent deaths and who saw them, oldest first.
    pub cues: Vec<DeathCue>,
}

impl Knowledge {
    pub fn team(&self, team: u8) -> Option<&TeamKnowledge> {
        self.teams
            .binary_search_by_key(&team, |t| t.team)
            .ok()
            .map(|i| &self.teams[i])
    }

    pub fn owner(&self, owner: Owner) -> Option<&OwnerKnowledge> {
        self.owners
            .binary_search_by_key(&owner, |o| o.owner)
            .ok()
            .map(|i| &self.owners[i])
    }
}

/// What one team has seen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamKnowledge {
    pub team: u8,
    /// [`UNEXPLORED`], [`EXPLORED`] or [`VISIBLE`] per fog node: ground cells first, then
    /// bridge-deck nodes in walk-surface order.
    pub cells: FogCells,
    /// Neutral harvestable scenery as last seen, live while in sight.
    #[serde(with = "by_id")]
    pub scenery: BTreeMap<EntityId, Sighting>,
    /// Other stationary entities (buildings, owned resources) as last seen.
    #[serde(with = "by_id")]
    pub structures: BTreeMap<EntityId, Sighting>,
}

impl TeamKnowledge {
    /// Remembered stationary entity, scenery or structure.
    pub fn remembered(&self, id: EntityId) -> Option<&Sighting> {
        self.scenery.get(&id).or_else(|| self.structures.get(&id))
    }
}

/// What one owner witnessed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerKnowledge {
    pub owner: Owner,
    /// Deaths the owner saw in the last [`DEATH_MEMORY_TICKS`], oldest first. The AI
    /// confirms kills from these without reading hidden deaths.
    pub deaths: Vec<ObservedDeath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedDeath {
    pub id: EntityId,
    pub definition: Def,
    pub position: Milli,
    pub tick: u32,
}

/// A death shown to everyone who saw the unit at its last moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeathCue {
    pub tick: u32,
    pub sighting: Sighting,
    /// Owners that saw it, in owner order.
    pub viewers: Vec<Owner>,
}

/// The public face of an entity at one moment: what anyone who sees it learns, and what
/// is remembered of stationary entities under fog.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sighting {
    pub id: EntityId,
    pub definition: Def,
    pub owner: Owner,
    pub position: Milli,
    /// Millidegrees.
    pub rotation: i32,
    /// The bridge deck it stands on.
    pub surface: Option<String>,
    pub hp: Option<u32>,
    /// Construction progress of an unfinished building.
    pub construction: Option<u32>,
    pub resource: Option<Resource>,
    /// Quantity of a loose item.
    pub item: Option<u32>,
}

impl Sighting {
    pub fn of(entity: &Entity) -> Self {
        Self {
            id: entity.id,
            definition: entity.definition,
            owner: entity.owner,
            position: entity.position,
            rotation: entity.rotation,
            surface: entity.surface.clone(),
            hp: entity.hp,
            construction: entity.construction.as_ref().map(|c| c.progress),
            resource: entity.resource.clone(),
            item: entity.item.as_ref().map(|i| i.quantity),
        }
    }

    /// Same definition and placement: a remembered entity whose place changed is stale.
    pub fn same_place(&self, other: &Self) -> bool {
        self.definition == other.definition
            && self.position == other.position
            && self.rotation == other.rotation
            && self.surface == other.surface
    }

    /// Whether `entity` still looks exactly like this.
    pub fn matches(&self, entity: &Entity) -> bool {
        self.id == entity.id
            && self.definition == entity.definition
            && self.owner == entity.owner
            && self.position == entity.position
            && self.rotation == entity.rotation
            && self.surface == entity.surface
            && self.hp == entity.hp
            && self.construction == entity.construction.as_ref().map(|c| c.progress)
            && self.resource == entity.resource
            && self.item == entity.item.as_ref().map(|i| i.quantity)
    }
}

/// A temporary sight source placed by an ability (old `spellVisions`; the `vision`
/// operation). It stays where it was created and explores fog like any sensor.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpellVision {
    pub id: u32,
    pub owner: Owner,
    pub point: Milli,
    /// The bridge deck the point is on.
    pub surface: Option<String>,
    /// Height of the eye above the floor, in centimetres.
    pub elevation: i32,
    /// Milli-cells.
    pub radius: u32,
    /// Sees every cell in the radius, whatever the terrain.
    pub ignore_terrain: bool,
    /// Also reveals concealed units in the radius.
    pub detect_invisible: bool,
    /// First tick on which it no longer exists.
    pub expires: u32,
    /// Ends when this unit dies (the ability runtime calls `end_with_caster`).
    pub caster: Option<EntityId>,
}

/// Fog cells, shared copy-on-write with the views handed out (see `View::fog`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FogCells(pub Arc<Vec<u8>>);

impl FogCells {
    pub fn filled(len: usize, value: u8) -> Self {
        Self(Arc::new(vec![value; len]))
    }

    pub fn get(&self, node: u32) -> u8 {
        self.0.get(node as usize).copied().unwrap_or(UNEXPLORED)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Saved as runs: each value `run << 2 | cell`.
impl Serialize for FogCells {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut runs: Vec<u64> = Vec::new();
        let mut cells = self.0.iter().copied().peekable();
        while let Some(value) = cells.next() {
            let mut run = 1u64;
            while cells.next_if_eq(&value).is_some() {
                run += 1;
            }
            runs.push(run << 2 | u64::from(value));
        }
        serializer.collect_seq(runs)
    }
}

impl<'de> Deserialize<'de> for FogCells {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// Larger than any map's node count.
        const MAX_CELLS: u64 = 1 << 26;
        let runs = Vec::<u64>::deserialize(deserializer)?;
        let mut cells = Vec::new();
        for packed in runs {
            let (run, value) = (packed >> 2, (packed & 3) as u8);
            if value > VISIBLE || run == 0 || cells.len() as u64 + run > MAX_CELLS {
                return Err(serde::de::Error::custom("invalid fog cells"));
            }
            cells.resize(cells.len() + run as usize, value);
        }
        Ok(Self(Arc::new(cells)))
    }
}

/// A map keyed by entity id, saved as the list of its values.
mod by_id {
    use super::Sighting;
    use crate::entity::EntityId;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(
        map: &BTreeMap<EntityId, Sighting>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(map.values())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<EntityId, Sighting>, D::Error> {
        let list = Vec::<Sighting>::deserialize(deserializer)?;
        let count = list.len();
        let map: BTreeMap<_, _> = list.into_iter().map(|s| (s.id, s)).collect();
        if map.len() != count {
            return Err(D::Error::custom("an entity is remembered twice"));
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_cells_round_trip_as_runs() {
        let mut cells = vec![EXPLORED; 1000];
        cells[3] = VISIBLE;
        cells[4] = VISIBLE;
        cells[999] = UNEXPLORED;
        let fog = FogCells(Arc::new(cells));
        let text = serde_json::to_string(&fog).unwrap();
        assert_eq!(
            text,
            format!(
                "[{},{},{},{}]",
                3 << 2 | 1,
                2 << 2 | 2,
                994 << 2 | 1,
                1 << 2
            )
        );
        assert_eq!(serde_json::from_str::<FogCells>(&text).unwrap(), fog);
        assert!(serde_json::from_str::<FogCells>("[3]").is_err());
        assert!(serde_json::from_str::<FogCells>("[1]").is_err());
    }
}
