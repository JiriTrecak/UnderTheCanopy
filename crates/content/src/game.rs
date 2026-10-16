//! `content/game.json`: definitions, behaviour sets, command metadata and rules.
//!
//! Field names, optionality and defaults match the authoring format exactly. `*Ticks`
//! values are authored in 25 ms ticks ([`AUTHORED_TICK_MS`]).

use crate::{
    abilities::AbilityCaster,
    conditions::{Locomotion, UnitNature},
    id::Id,
    items::ItemEffect,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// Length of one authored tick. Content durations named `*Ticks` count these.
pub const AUTHORED_TICK_MS: u32 = 25;

/// The document as authored: definitions still reference behaviour sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameSource {
    pub behavior_sets: Vec<serde_json::Value>,
    pub actions: serde_json::Value,
    pub rules: serde_json::Value,
    pub definitions: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Unit,
    Building,
    Item,
    Resource,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Building => "building",
            Self::Item => "item",
            Self::Resource => "resource",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cost {
    pub item: Id,
    pub amount: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkAnimation {
    Build,
    Chop,
}

/// How a definition comes into being.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase", deny_unknown_fields)]
pub enum Creation {
    #[serde(rename_all = "camelCase")]
    Construct {
        work_animation: Option<WorkAnimation>,
        items: Vec<Cost>,
        work_ticks: u32,
    },
    #[serde(rename_all = "camelCase")]
    Train { items: Vec<Cost>, work_ticks: u32 },
    #[serde(rename_all = "camelCase")]
    Harvest {
        work_animation: Option<WorkAnimation>,
        items: Vec<Cost>,
        source: Id,
        amount: u32,
        impact_tick: Option<u32>,
        animation_ticks: Option<u32>,
        work_ticks: u32,
    },
    #[serde(rename_all = "camelCase")]
    Plant { items: Vec<Cost>, work_ticks: u32 },
}

impl Creation {
    pub fn items(&self) -> &[Cost] {
        match self {
            Self::Construct { items, .. }
            | Self::Train { items, .. }
            | Self::Harvest { items, .. }
            | Self::Plant { items, .. } => items,
        }
    }

    pub fn method(&self) -> &'static str {
        match self {
            Self::Construct { .. } => "construct",
            Self::Train { .. } => "train",
            Self::Harvest { .. } => "harvest",
            Self::Plant { .. } => "plant",
        }
    }
}

fn default_turn_rate() -> u32 {
    720
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Movement {
    pub speed: f64,
    pub walk_speed: Option<f64>,
    #[serde(default = "default_turn_rate")]
    pub turn_rate: u32,
    pub idle_wander: Option<bool>,
    pub locomotion: Option<Locomotion>,
    /// Height above terrain, water or walk deck for air units; 6 when absent.
    pub flight_height: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackTiming {
    pub windup_ticks: u32,
    pub recovery_ticks: u32,
    pub range_buffer: f64,
}

impl Default for AttackTiming {
    fn default() -> Self {
        Self {
            windup_ticks: 12,
            recovery_ticks: 12,
            range_buffer: 0.75,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Projectile {
    pub speed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shell {
    pub flight_ticks: u32,
    pub radius: f64,
    pub slow_permille: u32,
    pub slow_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Charge {
    pub min_range: u32,
    pub max_range: u32,
    pub cooldown_ticks: u32,
    pub duration_ticks: u32,
    pub speed_permille: u32,
    pub damage_permille: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Combat {
    pub damage: u32,
    pub damage_type: Id,
    /// Allowed target locomotion; ground for melee and siege, both for missiles when absent.
    pub targets: Option<Vec<Locomotion>>,
    /// Weapon reach beyond body edges, in world units.
    pub range: f64,
    pub cooldown_ticks: u32,
    #[serde(default)]
    pub attack: AttackTiming,
    pub aggro_range: u32,
    pub projectile: Option<Projectile>,
    pub shell: Option<Shell>,
    pub charge: Option<Charge>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub carry_capacity: u32,
    pub builds: Vec<Id>,
    pub harvests: Option<Vec<Id>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaseGathering {
    pub item: Id,
    pub radius: f64,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Storage {
    pub capacity: u32,
    pub accepts: Vec<Id>,
    pub dropoff: Option<bool>,
    /// Resource-site workforce shown above this base (not a worker cap).
    pub gathering: Option<BaseGathering>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProductionMode {
    Automatic,
    Queued,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Production {
    pub mode: ProductionMode,
    pub outputs: Vec<Id>,
    /// 0 or 1.
    pub worker_slots: u32,
    pub queue_capacity: Option<u32>,
    pub work_radius: Option<u32>,
    pub job_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Level {
    pub experience: u32,
    pub max_hp: u32,
    pub damage: u32,
    pub armor: u32,
    pub cooldown_ticks: u32,
    pub max_mana: u32,
    pub health_regen_per_second: f64,
    pub mana_regen_per_second: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Progression {
    pub levels: Vec<Level>,
    pub experience_radius: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Inventory {
    pub slots: u32,
    pub pickup_range: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchQueue {
    pub outputs: Vec<Id>,
    pub queue_capacity: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revival {
    pub items: Vec<Cost>,
    pub items_per_level: Vec<Cost>,
    pub work_ticks: u32,
    pub work_ticks_per_level: u32,
    pub queue_capacity: u32,
}

/// A behaviour with no settings; its presence switches it on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enabled {}

/// Resolved behaviours of a definition, after behaviour sets are merged.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Behaviors {
    pub research: Option<ResearchQueue>,
    pub movement: Option<Movement>,
    pub player_control: Option<Enabled>,
    pub combat: Option<Combat>,
    pub work: Option<Work>,
    pub storage: Option<Storage>,
    pub production: Option<Production>,
    pub camp_defense: Option<Enabled>,
    pub progression: Option<Progression>,
    pub inventory: Option<Inventory>,
    pub abilities: Option<AbilityCaster>,
    pub revival: Option<Revival>,
}

/// Behaviour names in declaration order, as used by `disabledBehaviors`.
pub const BEHAVIOR_NAMES: [&str; 12] = [
    "research",
    "movement",
    "playerControl",
    "combat",
    "work",
    "storage",
    "production",
    "campDefense",
    "progression",
    "inventory",
    "abilities",
    "revival",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dimensions {
    pub radius: f64,
    pub height: f64,
    pub formation_spacing: f64,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Garrison {
    /// Always 1.
    pub capacity: u32,
    pub accepts: Vec<Id>,
    pub height: f64,
    pub lookout_radius: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Upgrade {
    pub target: Id,
    pub items: Vec<Cost>,
    pub work_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Body {
    pub max_hp: u32,
    pub armor: u32,
    pub armor_type: Id,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Footprint {
    pub width: u32,
    pub depth: u32,
}

/// Offset of a building's entrance from its centre, in half navigation cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entrance {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceStage {
    pub remaining: f64,
    pub asset: Id,
}

/// Full alternatives and shared depletion models, chosen by remaining/initial amount.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceVisuals {
    pub variants: Vec<Id>,
    pub stages: Vec<ResourceStage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Felling {
    pub max_hp: u32,
    pub fall_ticks: u32,
    pub decay_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Harvesting {
    pub active_workers: u32,
    pub recommended_workers: u32,
    pub search_radius: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlacementNear {
    pub source: Id,
    pub radius: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionClass {
    Army,
    Worker,
}

/// A unit, building, item or resource after behaviour sets are resolved.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definition {
    pub id: Id,
    pub garrison: Option<Garrison>,
    pub requires: Option<Vec<Id>>,
    pub upgrade: Option<Upgrade>,
    pub category: Option<Id>,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub asset: Id,
    pub icon: Id,
    pub display_order: Option<i64>,
    pub hero: Option<bool>,
    /// Maximum distinct colony heroes, provided by the highest completed hall.
    pub hero_capacity: Option<u32>,
    pub unit_nature: Option<UnitNature>,
    pub dimensions: Option<Dimensions>,
    /// Presentation only; never changes movement, collision or weapon range.
    pub model_scale: Option<f64>,
    pub supply_cost: Option<u32>,
    pub supply_provided: Option<u32>,
    pub level: Option<u32>,
    pub experience_yield: Option<u32>,
    pub item_tier: Option<u32>,
    pub item_effect: Option<ItemEffect>,
    pub selectable: Option<bool>,
    pub selection_class: Option<SelectionClass>,
    pub vision: Option<u32>,
    pub body: Option<Body>,
    pub footprint: Option<Footprint>,
    pub entrance: Option<Entrance>,
    pub stack_limit: Option<u32>,
    #[serde(rename = "yield")]
    pub resource_yield: Option<u32>,
    pub resource_visuals: Option<ResourceVisuals>,
    pub felling: Option<Felling>,
    pub harvesting: Option<Harvesting>,
    pub gathering_unit_collision: Option<bool>,
    /// Movement-only blocking disc (cells, before appearance scale) around a standing
    /// resource, so neighbouring forest trees join into a wall.
    pub collision_radius: Option<f64>,
    pub currency: Option<bool>,
    pub regrowth_ticks: Option<u32>,
    pub construction_clearance: Option<u32>,
    pub placement_near: Option<PlacementNear>,
    pub creation: Option<Creation>,
    pub behaviors: Behaviors,
}

impl Definition {
    pub fn is_hero(&self) -> bool {
        self.hero == Some(true)
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionMeta {
    pub category: Option<Id>,
    pub name: String,
    pub description: String,
    pub icon: Id,
    pub priority: i64,
    pub placement: Option<ActionPlacement>,
    pub hotkey: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionPlacement {
    Banner,
    BottomRow,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavigationMeta {
    pub name: String,
    pub description: String,
    pub icon: Id,
    pub priority: i64,
    pub placement: Option<ActionPlacement>,
    pub hotkey: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Navigation {
    pub back: NavigationMeta,
    pub camera: Option<NavigationMeta>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CategoryMeta {
    pub name: String,
    pub description: String,
    pub icon: Id,
    pub priority: i64,
    pub placement: Option<ActionPlacement>,
    pub hotkey: Option<String>,
    pub parent: Option<Id>,
}

/// Command card names; overrides key `build:<id>` and `produce:<id>`.
pub const ACTION_NAMES: [&str; 17] = [
    "move",
    "attack",
    "stop",
    "hold",
    "patrol",
    "follow",
    "garrison",
    "unload",
    "build",
    "produce",
    "cancel",
    "rally",
    "pause",
    "upgrade",
    "cancelUpgrade",
    "research",
    "cancelResearch",
];

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionOverride {
    /// `null` removes the category; absent keeps it.
    #[serde(default, with = "serde_with::rust::double_option")]
    pub category: Option<Option<Id>>,
    pub hidden: Option<bool>,
    pub priority: Option<i64>,
    pub hotkey: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Actions {
    pub navigation: Navigation,
    #[serde(default)]
    pub categories: BTreeMap<Id, CategoryMeta>,
    pub actions: BTreeMap<String, ActionMeta>,
    pub overrides: BTreeMap<String, ActionOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArmorType {
    pub name: String,
    pub icon: Id,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchEffect {
    pub units: Vec<Id>,
    pub max_hp: Option<u32>,
    pub armor: Option<u32>,
    pub damage_permille: Option<u32>,
    pub tree_work_rate: Option<u32>,
    pub splash_radius: Option<f64>,
    pub splash_slow_permille: Option<u32>,
    pub charge_cooldown_permille: Option<u32>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Research {
    pub name: String,
    pub description: String,
    pub icon: Id,
    pub priority: i64,
    pub requires: Option<Vec<Id>>,
    pub items: Vec<Cost>,
    pub work_ticks: u32,
    pub effects: Vec<ResearchEffect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LootEntry {
    /// `None` is an empty roll.
    pub item: Option<Id>,
    pub weight: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LootPool {
    pub rolls: u32,
    pub max_tier: Option<u32>,
    pub entries: Vec<LootEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DamageType {
    pub name: String,
    pub applies_armor: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Experience {
    pub unit_rewards: Vec<u32>,
    pub hero_rewards: Vec<u32>,
    pub neutral_level_cap: u32,
    pub neutral_multipliers_permille: Vec<u32>,
    pub summon_multiplier_permille: u32,
    pub global_fallback: bool,
    pub solo_hero_tier_bonus_permille: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridOffset {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingHero {
    pub default: Id,
    pub choices: Vec<Id>,
    pub offset: GridOffset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingGathering {
    pub item: Id,
    pub workers: u32,
    pub radius: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingUnit {
    pub definition: Id,
    pub offset: GridOffset,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingSetup {
    pub id: Id,
    pub fort: Id,
    pub hero: Option<StartingHero>,
    pub gathering: Option<Vec<StartingGathering>>,
    pub inventory: BTreeMap<Id, u32>,
    pub units: Vec<StartingUnit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConstructionPresentation {
    pub scaffold: Id,
    pub padding: f64,
    pub reveal_at: f64,
    pub retract_until: f64,
    pub dust_effect: Id,
    pub dust_interval_ticks: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Race {
    pub name: String,
    pub description: String,
    pub construction: Option<ConstructionPresentation>,
    pub starting_setup: Option<StartingSetup>,
    pub roster: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Campaign {
    pub name: String,
    pub race: String,
}

fn default_race() -> String {
    "ants".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rules {
    pub id: String,
    pub armor_types: BTreeMap<Id, ArmorType>,
    pub max_units: u32,
    pub max_supply: u32,
    pub supply_icon: Id,
    pub max_buildings: u32,
    pub construction_hp_permille: u32,
    pub construction_refund_permille: u32,
    pub repair_ticks: u32,
    pub research: BTreeMap<Id, Research>,
    pub loot_pools: BTreeMap<Id, LootPool>,
    pub armor_coefficient: f64,
    pub damage_types: BTreeMap<Id, DamageType>,
    pub hero_stun_duration_permille: u32,
    pub experience: Experience,
    pub damage_multipliers: BTreeMap<Id, BTreeMap<Id, u32>>,
    pub starting_setup: StartingSetup,
    #[serde(default = "default_race")]
    pub default_race: String,
    #[serde(default)]
    pub races: BTreeMap<String, Race>,
    #[serde(default)]
    pub campaigns: BTreeMap<String, Campaign>,
}

/// A race with the global starting setup filled in where it has none.
#[derive(Debug, Clone, PartialEq)]
pub struct RaceProfile<'a> {
    pub id: &'a str,
    pub race: &'a Race,
    pub starting_setup: &'a StartingSetup,
}

impl Rules {
    pub fn race(&self, id: &str) -> Option<RaceProfile<'_>> {
        let (id, race) = self.races.get_key_value(id)?;
        Some(RaceProfile {
            id,
            race,
            starting_setup: race.starting_setup.as_ref().unwrap_or(&self.starting_setup),
        })
    }
}

/// A render asset declared by a published asset record (`assets/manifest.json` → `render`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderAsset {
    pub id: Id,
    pub file: Option<String>,
    pub image: Option<String>,
    pub harvest_animation: Option<String>,
    pub dimensions: Option<Dimensions>,
    pub carry_asset: Option<Id>,
    pub character: Option<Id>,
    pub scale: Option<f64>,
    pub health_height: Option<f64>,
    pub camera_anchor: Option<CameraAnchor>,
    pub stack_height: Option<f64>,
    pub stack_columns: Option<u32>,
    pub projectile: Option<Id>,
    pub impact_effect: Option<Id>,
    pub projectile_socket: Option<String>,
    pub sockets: Option<Vec<Socket>>,
    pub cast_contact: Option<f64>,
    pub scenery_asset: Option<String>,
}

fn default_anchor_distance() -> f64 {
    7.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CameraAnchor {
    pub height: f64,
    #[serde(default)]
    pub forward: f64,
    #[serde(default = "default_anchor_distance")]
    pub distance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Socket {
    pub name: String,
    pub node: String,
    pub offset: [f64; 3],
}
