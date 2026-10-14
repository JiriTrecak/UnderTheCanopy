//! Saved ability state (old `abilities/state.ts`): what casters, status holders and linked
//! units carry, and the world's deliveries, persistent instances and reaction queue.

use super::library::{AbilityId, StatusKey};
use crate::{
    catalog::Def,
    entity::{EntityId, Milli},
};
use canopy_content::placement::Owner;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// Expiry of statuses and links that last until death.
pub const UNTIL_DEATH: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preparing,
    Channeling,
    Recovering,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub wave: u32,
    pub next_wave_tick: u32,
    pub end_tick: u32,
    /// Where the caster stood at acceptance; moving ends the channel.
    pub origin: Milli,
}

/// A cast from acceptance to the end of its recovery.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub id: u32,
    /// Binding index on the caster's definition.
    pub binding: u8,
    pub ability: AbilityId,
    pub rank: u8,
    pub target: Option<EntityId>,
    /// Point aims: a whole cell, in milli-cells.
    pub point: Option<Milli>,
    pub channel: Option<Channel>,
    pub owner: Owner,
    pub start_tick: u32,
    pub release_tick: u32,
    pub finish_tick: u32,
    pub phase: Phase,
    /// Mana reserved until release.
    pub escrow: u32,
    pub cooldown_ticks: u32,
}

/// A manual weapon cast waiting for the weapon's next release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponOrder {
    pub id: u32,
    pub binding: u8,
    pub ability: AbilityId,
    pub rank: u8,
    pub target: EntityId,
    pub owner: Owner,
    /// The weapon profile it was ordered with.
    pub profile: Def,
    pub started: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// The ability state of a unit with `behaviors.abilities` (old `AbilityState`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caster {
    pub mana: u32,
    /// Regeneration accumulator: 40,000 units make a point.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub regeneration: u32,
    /// Learned rank per binding.
    pub ranks: Vec<u8>,
    /// Autocast toggles per binding index; absent ones use the ability's default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub autocast: BTreeMap<u8, bool>,
    /// The tick each ability is ready again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cooldowns: BTreeMap<AbilityId, u32>,
    pub pending: Option<Pending>,
    pub weapon_order: Option<WeaponOrder>,
}

impl Caster {
    pub fn escrow(&self) -> u32 {
        self.pending.as_ref().map_or(0, |p| p.escrow)
    }
}

/// What a unit was when it set something lasting in motion (old `SpellSource`): owner,
/// camp, position and resources outlive the unit itself.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub source: EntityId,
    pub definition: Def,
    pub owner: Owner,
    pub position: Milli,
    pub surface: Option<String>,
    /// Height of the eye in centimetres above the floor.
    #[serde(default)]
    pub elevation: i32,
    pub air: bool,
    pub camp: Option<String>,
    pub level: u32,
    pub summoned: bool,
    pub hp: u32,
    pub max_hp: u32,
    pub mana: u32,
    pub max_mana: u32,
}

/// A spell status on a unit (old `SpellStatus`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub key: StatusKey,
    pub source: EntityId,
    pub owner: Owner,
    pub rank: u8,
    pub cast: u32,
    pub started: u32,
    pub expires: u32,
    /// Next periodic pulse.
    pub next_tick: u32,
    pub stacks: Option<u32>,
    pub shield: Option<u32>,
    /// Control components refused on application (bits).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub blocked: u8,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub aura: bool,
    pub source_context: Option<Box<Source>>,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// How a summoned or temporary unit is tied to its maker (old `summoned`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summoned {
    pub source: EntityId,
    pub ability: AbilityId,
    pub rank: u8,
    pub cast: u32,
    pub started: u32,
    pub expires: u32,
    /// A member of a split form, named by the split operation.
    pub split: Option<String>,
    /// Ends when the source dies or changes controller (owner and camp it had).
    pub link: Option<(Owner, Option<String>)>,
    pub reanimated_from: Option<EntityId>,
}

/// A unit replaced by linked members (old `SplitForm`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Split {
    pub owner: Owner,
    pub camp: Option<String>,
    pub ability: AbilityId,
    pub operation: String,
    pub rank: u8,
    pub cast: u32,
    pub started: u32,
    pub expires: u32,
    pub members: Vec<EntityId>,
}

/// A unit held inside a carrier (old `Containment`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Containment {
    pub host: EntityId,
    pub owner: Owner,
    pub ability: AbilityId,
    pub operation: String,
    pub rank: u8,
    pub cast: u32,
    pub started: u32,
    pub expires: u32,
    pub next_tick: u32,
}

/// A fallen hero scheduled to return by a `revive` operation (old `HeroReturn`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeroReturn {
    pub ability: AbilityId,
    pub operation: String,
    pub rank: u8,
    pub cast: u32,
    pub owner: Owner,
    pub started: u32,
    pub due: u32,
    pub deadline: u32,
}

/// A trigger of an ability: (ability, trigger index).
pub type TriggerKey = (AbilityId, u8);

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub rank: u8,
    pub owner: Owner,
    pub started: u32,
    pub next_tick: u32,
    /// The cast of the granting status.
    pub grant: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerCooldown {
    pub rank: u8,
    pub started: u32,
    pub expires: u32,
}

/// Per-unit ability links and trigger bookkeeping.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    pub summoned: Option<Summoned>,
    pub split: Option<Split>,
    pub containment: Option<Containment>,
    pub spell_return: Option<HeroReturn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "pairs")]
    pub timers: Vec<(TriggerKey, Timer)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "pairs")]
    pub trigger_cooldowns: Vec<(TriggerKey, TriggerCooldown)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "pairs")]
    pub counters: Vec<(TriggerKey, u32)>,
    /// Owners this carrier lends its sight to (passengers held with `shareVision`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lent_sight: Vec<Owner>,
}

impl Links {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Sorted key-value lists serialized as plain sequences.
mod pairs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<K: Serialize, V: Serialize, S: Serializer>(
        v: &[(K, V)],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        v.serialize(s)
    }
    pub fn deserialize<'de, K: Deserialize<'de>, V: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<(K, V)>, D::Error> {
        Vec::<(K, V)>::deserialize(d)
    }
}

/// Sorted map helpers over `Vec<(K, V)>`.
pub fn entry<K: Ord + Copy, V>(list: &mut Vec<(K, V)>, key: K, make: impl FnOnce() -> V) -> &mut V {
    let i = match list.binary_search_by(|(k, _)| k.cmp(&key)) {
        Ok(i) => i,
        Err(i) => {
            list.insert(i, (key, make()));
            i
        }
    };
    &mut list[i].1
}

pub fn lookup<K: Ord + Copy, V>(list: &[(K, V)], key: K) -> Option<&V> {
    list.binary_search_by(|(k, _)| k.cmp(&key))
        .ok()
        .map(|i| &list[i].1)
}

/// A point in flight in 1/128,000 cells (exact for both milli-cells and the 1/1024 grid
/// deliveries step on); height in 1/128,000 world units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Fine {
    pub x: i64,
    pub y: i64,
    pub height: i64,
}

pub const FINE: f64 = 128_000.0;

impl Fine {
    pub fn of(p: Milli, height: f64) -> Self {
        Self {
            x: i64::from(p.x) * 128,
            y: i64::from(p.y) * 128,
            height: (height * FINE).round() as i64,
        }
    }

    pub fn xf(&self) -> f64 {
        self.x as f64 / FINE
    }

    pub fn yf(&self) -> f64 {
        self.y as f64 / FINE
    }

    pub fn hf(&self) -> f64 {
        self.height as f64 / FINE
    }

    /// Snapped to the 1/1024 grid (old `Math.round(v*1024)/1024`).
    pub fn snapped(x: f64, y: f64, h: f64) -> Self {
        let snap = |v: f64| ((v * 1024.0 + 0.5).floor() as i64) * 125;
        Self {
            x: snap(x),
            y: snap(y),
            height: snap(h),
        }
    }

    /// The nearest milli-cell.
    pub fn milli(&self) -> Milli {
        let r = |v: i64| (v + 64).div_euclid(128) as i32;
        Milli {
            x: r(self.x),
            y: r(self.y),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeekerPhase {
    Waiting,
    Seeking,
    Returning,
}

/// A swarm seeker (old `swarmSeekerSchema`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seeker {
    pub group: u32,
    pub index: u32,
    pub launch_tick: u32,
    pub expires: u32,
    pub phase: SeekerPhase,
    pub hits: u32,
    pub health: u32,
    pub mana: u32,
}

/// A projectile, chain, line, charge or swarm seeker (old `SpellDelivery`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub cast: u32,
    pub ability: AbilityId,
    pub rank: u8,
    pub source: EntityId,
    pub owner: Owner,
    pub target: Option<EntityId>,
    pub origin: Fine,
    pub position: Fine,
    pub point: Fine,
    pub started: u32,
    pub next_tick: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hit: Vec<EntityId>,
    pub power: u32,
    pub swarm: Option<Seeker>,
    pub source_context: Option<Box<Source>>,
}

/// A persistent zone, toggle or sustained effect (old `SpellInstance`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub cast: u32,
    pub ability: AbilityId,
    pub rank: u8,
    pub source: EntityId,
    pub owner: Owner,
    pub target: Option<EntityId>,
    pub point: Milli,
    pub origin: Milli,
    pub started: u32,
    pub next_tick: u32,
    pub expires: u32,
    pub source_context: Option<Box<Source>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LifecycleEvent {
    Interval,
    Death,
    Kill,
    WeaponRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WeaponKind {
    Melee,
    Projectile,
    Siege,
}

/// A queued lifecycle reaction (old `LifecycleReaction`): it keeps its captured source,
/// rank and point after the entities involved are gone.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub event: LifecycleEvent,
    pub source: Source,
    pub timer_grant: Option<u32>,
    pub timer_started: Option<u32>,
    pub weapon: Option<WeaponKind>,
    pub subject: EntityId,
    pub subject_at: Milli,
    pub cast: u32,
    pub ability: AbilityId,
    pub rank: u8,
    pub trigger: u8,
    pub tick: u32,
    pub viewers: Vec<Owner>,
}

/// Damage dealt, for hit and damage triggers at the next reaction pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatEvent {
    pub source: EntityId,
    pub target: EntityId,
    pub damage: u32,
    pub weapon: bool,
    pub melee: bool,
}

/// Most queued lifecycle reactions; further ones are dropped.
pub const MAX_REACTIONS: usize = 512;
/// Lifecycle reactions run per tick.
pub const REACTIONS_PER_TICK: usize = 128;
/// Most recorded combat events between reaction passes.
pub const MAX_COMBAT_EVENTS: usize = 2048;
/// Most deliveries (each seeker counts) and persistent instances in the world.
pub const MAX_DELIVERIES: usize = 512;
pub const MAX_INSTANCES: usize = 512;
/// Most spell statuses on one unit.
pub const MAX_STATUSES: usize = 32;
/// Most trigger timers, cooldowns and counters per unit.
pub const MAX_TRIGGER_RECORDS: usize = 128;

/// World-level ability state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityWorld {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deliveries: Vec<Delivery>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<Instance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<Reaction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub combat_events: Vec<CombatEvent>,
}

impl AbilityWorld {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
