//! Entities: every unit, building, item and resource, in one table ordered by id.

use crate::catalog::Def;
use canopy_content::{Id, placement::Owner};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// Allocated monotonically and never reused, so id order is creation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub u32);

/// A position in milli-cells (1/1000 of a navigation cell).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct Milli {
    pub x: i32,
    pub y: i32,
}

impl Milli {
    pub const CELL: i32 = 1000;

    /// The navigation cell containing the point (half cells round up, as the old game did).
    pub fn cell(self) -> (i32, i32) {
        let round = |v: i32| (v + Self::CELL / 2).div_euclid(Self::CELL);
        (round(self.x), round(self.y))
    }
}

/// Item stocks keyed by item id.
pub type Stock = BTreeMap<Id, u32>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Construction {
    pub progress: u32,
    pub supported_hp: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub amount: u32,
    pub initial_amount: Option<u32>,
    pub growing_until: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LooseItem {
    pub quantity: u32,
}

/// Parts present only on mobile units.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unit {}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entity {
    pub id: EntityId,
    /// First tick on which the entity takes part in the simulation.
    pub ready_tick: u32,
    /// The map placement it came from, for scripts and starting units.
    pub placement: Option<String>,
    pub definition: Def,
    pub owner: Owner,
    pub position: Milli,
    pub surface: Option<String>,
    /// Millidegrees.
    pub rotation: i32,
    pub hp: Option<u32>,
    pub inventory: Stock,
    pub unit: Option<Box<Unit>>,
    pub construction: Option<Construction>,
    pub resource: Option<Resource>,
    pub item: Option<LooseItem>,
}

/// Entities ordered by id with O(1) lookup. Removal leaves a hole that later compaction
/// closes, so ids keep their order without shifting the whole table on every death.
#[derive(Debug, Clone, Default)]
pub struct Entities {
    slots: Vec<Option<Entity>>,
    /// Entity id → slot + 1 (0: none).
    index: Vec<u32>,
    live: usize,
}

impl Entities {
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Appends an entity; its id must be greater than every existing id.
    pub fn insert(&mut self, entity: Entity) {
        let id = entity.id.0 as usize;
        if let Some(last) = self.slots.iter().rev().flatten().next() {
            assert!(entity.id > last.id, "entity ids must increase");
        }
        if self.index.len() <= id {
            self.index.resize(id + 1, 0);
        }
        self.slots.push(Some(entity));
        self.index[id] = self.slots.len() as u32;
        self.live += 1;
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        let slot = *self.index.get(id.0 as usize)?;
        self.slots.get(slot.checked_sub(1)? as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        let slot = *self.index.get(id.0 as usize)?;
        self.slots.get_mut(slot.checked_sub(1)? as usize)?.as_mut()
    }

    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        let slot = std::mem::take(self.index.get_mut(id.0 as usize)?);
        let entity = self.slots.get_mut(slot.checked_sub(1)? as usize)?.take()?;
        self.live -= 1;
        if self.live * 2 < self.slots.len() {
            self.compact();
        }
        Some(entity)
    }

    /// Live entities in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.slots.iter().flatten()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Entity> {
        self.slots.iter_mut().flatten()
    }

    fn compact(&mut self) {
        self.slots.retain(Option::is_some);
        for (slot, entity) in self.slots.iter().flatten().enumerate() {
            self.index[entity.id.0 as usize] = slot as u32 + 1;
        }
    }
}

/// Equal when the same entities are live, regardless of holes left by removals.
impl PartialEq for Entities {
    fn eq(&self, other: &Self) -> bool {
        self.live == other.live && self.iter().eq(other.iter())
    }
}

impl Serialize for Entities {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for Entities {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let list = Vec::<Entity>::deserialize(deserializer)?;
        let mut entities = Self::default();
        for entity in list {
            if entities
                .iter()
                .last()
                .is_some_and(|last| last.id >= entity.id)
            {
                return Err(serde::de::Error::custom(
                    "entities must be in increasing id order",
                ));
            }
            entities.insert(entity);
        }
        Ok(entities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: u32) -> Entity {
        Entity {
            id: EntityId(id),
            ready_tick: 0,
            placement: None,
            definition: Def(0),
            owner: Owner::Nobody,
            position: Milli::default(),
            surface: None,
            rotation: 0,
            hp: None,
            inventory: Stock::new(),
            unit: None,
            construction: None,
            resource: None,
            item: None,
        }
    }

    #[test]
    fn lookups_survive_removal_and_compaction() {
        let mut entities = Entities::default();
        for id in 1..=10 {
            entities.insert(entity(id));
        }
        for id in [2, 4, 6, 8, 9, 10] {
            assert!(entities.remove(EntityId(id)).is_some());
        }
        assert!(entities.remove(EntityId(2)).is_none());
        let ids: Vec<u32> = entities.iter().map(|e| e.id.0).collect();
        assert_eq!(ids, [1, 3, 5, 7]);
        assert_eq!(entities.get(EntityId(7)).unwrap().id, EntityId(7));
        assert!(entities.get(EntityId(8)).is_none());
        entities.insert(entity(11));
        assert_eq!(entities.len(), 5);
    }

    #[test]
    fn cells_round_half_up() {
        assert_eq!(Milli { x: 1499, y: 1500 }.cell(), (1, 2));
        assert_eq!(Milli { x: -500, y: -501 }.cell(), (0, -1));
    }
}
