//! Entities: every unit, building, item and resource, in one table ordered by id.

use crate::{catalog::Def, orders::Order};
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
    /// A tree's felling state (economy).
    pub felling: Option<crate::economy::Felling>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LooseItem {
    pub quantity: u32,
    /// Charges and cooldown a dropped hero item keeps (`items`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<crate::items::ItemRuntime>,
}

/// A route leg in progress: from a fixed point towards a route node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub from: Milli,
    pub to: u32,
    /// Milli-cells, rounded up.
    pub length: u32,
    pub progress: u32,
}

/// A body that agreed to step aside for a stalled leader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Yielding {
    pub leader: EntityId,
    /// The tick the wait ends at the latest.
    pub until: u32,
}

/// A bounded local escape around nearby bodies that rejoins the route at `waypoint`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detour {
    pub goal: u32,
    pub waypoint: u32,
    pub yielding: Option<Yielding>,
    /// Fixed points on the unit's own floor.
    pub points: Vec<Milli>,
}

/// Idle strolling around a home cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Idle {
    pub home: Place,
    pub next_tick: u32,
    pub walking: bool,
}

/// A cell, optionally on a walk surface (bridge deck), as orders name it.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub surface: Option<String>,
}

impl Place {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            surface: None,
        }
    }
}

/// Parts present only on mobile units. Navigation fields refer to navigation nodes: ground
/// cells `y · size + x`, then deck nodes on maps with bridges.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unit {
    pub order: Option<Order>,
    /// Orders to run after the current one; at most [`crate::orders::MAX_QUEUED_ORDERS`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_queue: Vec<Order>,
    /// Waypoint nodes still to reach.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub route: Vec<u32>,
    /// Where the current route ends.
    pub goal: Option<u32>,
    pub segment: Option<Segment>,
    pub detour: Option<Detour>,
    pub last_moved_tick: Option<u32>,
    /// Stopped short of its goal beside the group that arrived there (movement): a move
    /// order counts it as arrived.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub settled: bool,
    /// No blocked-route recovery before this tick.
    #[serde(default)]
    pub retry_at: u32,
    pub idle: Option<Idle>,
    /// The entity it faces and engages (combat).
    pub target: Option<EntityId>,
    /// Waiting to be placed back on the map near this cell.
    pub release: Option<Place>,
    /// Held inside another entity (a training building, a cage).
    pub contained: Option<EntityId>,
    /// Job, cargo and workplace of a worker (economy).
    #[serde(default, skip_serializing_if = "crate::economy::Work::is_idle")]
    pub work: crate::economy::Work,
    /// Weapon timeline, pursuit, charge and slows (`combat`).
    #[serde(default, skip_serializing_if = "crate::combat::UnitCombat::is_idle")]
    pub combat: crate::combat::UnitCombat,
    /// Membership of a neutral camp (`camps`).
    pub camp: Option<crate::camps::Member>,
    /// Standing in a lookout (`garrisons`).
    pub garrison: Option<crate::garrisons::Lookout>,
    /// Health regeneration accumulator: 40,000 units make a point (`items`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub regeneration: u32,
    /// Flying by a form, or landed against its definition (`abilities::forms`).
    pub flight: Option<crate::abilities::Flight>,
    /// Forced to fight a taunting unit (`abilities::taunt`).
    pub taunt: Option<crate::abilities::Taunt>,
    /// Travelling through its owner's tunnels (`tunnels`).
    pub tunnel: Option<crate::tunnels::Trip>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

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
    /// Training, automatic production and the workplace staff (economy).
    pub production: Option<Box<crate::economy::Production>>,
    /// Research tasks queued at this building (economy).
    pub research: Option<crate::economy::ResearchQueue>,
    /// An in-place building upgrade in progress (economy).
    pub upgrade: Option<crate::economy::UpgradeTask>,
    /// Hero revivals queued at this building (economy).
    pub revival: Option<crate::economy::RevivalQueue>,
    /// Experience of a definition with levels (`heroes`).
    pub progression: Option<crate::heroes::Progression>,
    /// A hero that died and waits for revival, kept with its identity (`heroes`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fallen: bool,
    /// Mana, learned ranks, cooldowns and the cast in progress (`abilities`).
    pub caster: Option<Box<crate::abilities::Caster>>,
    /// Spell statuses (`abilities::statuses`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<crate::abilities::Status>,
    /// Summon, split, containment and trigger links (`abilities`).
    pub links: Option<Box<crate::abilities::Links>>,
    /// Hero item slots and item effects (`items`).
    pub equipment: Option<Box<crate::items::Equipment>>,
    /// Ticks towards a building's next income (`economy::income`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub income: u32,
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
            production: None,
            research: None,
            upgrade: None,
            revival: None,
            progression: None,
            fallen: false,
            caster: None,
            statuses: Vec::new(),
            links: None,
            equipment: None,
            income: 0,
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
