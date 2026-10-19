//! Definitions (units, buildings, items, resources), behaviour sets, command metadata and
//! rules, as the base package's files describe them (docs/packages.md).
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

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cost {
    pub item: Id,
    pub amount: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WorkAnimation {
    Build,
    Chop,
}

/// How a definition comes into being.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Projectile {
    pub speed: u32,
}

/// A missile that jumps on from each victim to the nearest other enemy (arcs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WeaponChain {
    /// Further victims after the first, 1–8.
    pub bounces: u32,
    /// How far from a victim the next one may stand, world units.
    pub radius: f64,
    /// Damage each jump keeps of the one before, permille.
    pub retention_permille: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shell {
    pub flight_ticks: u32,
    pub radius: f64,
    pub slow_permille: u32,
    pub slow_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Combat {
    pub damage: u32,
    pub damage_type: Id,
    /// Allowed target locomotion; ground for melee and siege, both for missiles when absent.
    pub targets: Option<Vec<Locomotion>>,
    /// Weapon reach beyond body edges, in world units.
    pub range: f64,
    /// Targets closer than this (body edges, world units) cannot be hit: siege weapons
    /// that are weak up close.
    pub min_range: Option<f64>,
    pub cooldown_ticks: u32,
    #[serde(default)]
    pub attack: AttackTiming,
    pub aggro_range: u32,
    pub projectile: Option<Projectile>,
    /// Missiles only: each hit jumps on to further enemies.
    pub chain: Option<WeaponChain>,
    pub shell: Option<Shell>,
    pub charge: Option<Charge>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub carry_capacity: u32,
    pub builds: Vec<Id>,
    pub harvests: Option<Vec<Id>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaseGathering {
    pub item: Id,
    pub radius: f64,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Storage {
    pub capacity: u32,
    pub accepts: Vec<Id>,
    pub dropoff: Option<bool>,
    /// Resource-site workforce shown above this base (not a worker cap).
    pub gathering: Option<BaseGathering>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProductionMode {
    Automatic,
    Queued,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Progression {
    pub levels: Vec<Level>,
    pub experience_radius: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Inventory {
    pub slots: u32,
    pub pickup_range: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchQueue {
    pub outputs: Vec<Id>,
    pub queue_capacity: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revival {
    pub items: Vec<Cost>,
    pub items_per_level: Vec<Cost>,
    pub work_ticks: u32,
    pub work_ticks_per_level: u32,
    pub queue_capacity: u32,
}

/// A behaviour with no settings; its presence switches it on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Enabled {}

/// Resolved behaviours of a definition, after behaviour sets are merged.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dimensions {
    pub radius: f64,
    pub height: f64,
    pub formation_spacing: f64,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Garrison {
    /// Always 1.
    pub capacity: u32,
    pub accepts: Vec<Id>,
    pub height: f64,
    pub lookout_radius: Option<f64>,
    /// A tower's own gunner: this unit stands in the lookout whenever the building is
    /// complete, cannot leave or take orders, and goes with the building.
    pub crew: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Upgrade {
    pub target: Id,
    pub items: Vec<Cost>,
    pub work_ticks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Body {
    pub max_hp: u32,
    pub armor: u32,
    pub armor_type: Id,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Footprint {
    pub width: u32,
    pub depth: u32,
}

/// Offset of a building's entrance from its centre, in half navigation cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entrance {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceStage {
    pub remaining: f64,
    pub asset: Id,
}

/// Full alternatives and shared depletion models, chosen by remaining/initial amount.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceVisuals {
    pub variants: Vec<Id>,
    pub stages: Vec<ResourceStage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Felling {
    pub max_hp: u32,
    pub fall_ticks: u32,
    pub decay_ticks: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Harvesting {
    pub active_workers: u32,
    pub recommended_workers: u32,
    pub search_radius: f64,
    /// Gathered only through a completed building placed on it (`placementOn`), and only
    /// by that building's owner's workers (D62: a Substance Well on its deposit).
    pub needs_building: Option<bool>,
    /// Never runs out: loads leave its amount as it is (D62).
    pub endless: Option<bool>,
}

impl Harvesting {
    pub fn needs_building(&self) -> bool {
        self.needs_building == Some(true)
    }

    pub fn endless(&self) -> bool {
        self.endless == Some(true)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlacementNear {
    pub source: Id,
    pub radius: u32,
}

/// What a building does in its race's power network (D66).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkRole {
    /// Powers the ground around it whenever it stands complete (a main hall, a well).
    Source,
    /// Extends the network around it, but only while it stands inside the network
    /// itself: a chain of nodes must reach back to a source.
    Node,
}

/// A building's part in its owner's power network (D66): the disc of `radius` cells
/// around its centre is powered while it is complete and, for a node, powered itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Network {
    pub role: NetworkRole,
    /// Cells from the building's centre; research may add to it (`networkRadius`).
    pub radius: u32,
}

/// What a building needs from its owner's power network (D66).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Power {
    /// Placed only where its centre is powered, and its work (training, research,
    /// upgrades, revival, a lookout's shots) pauses while it is not.
    Required,
}

/// A building placed exactly on a standing resource of `source` (a refinery on its
/// geyser): it snaps to the resource, covers its footprint, and leaves it intact when it
/// is gone (D62).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlacementOn {
    pub source: Id,
}

/// Goods a building makes by itself (D62: a Substance Well pumps its deposit): while it
/// stands complete (and powered, when it needs power, D66) it adds `amount` of `item` to
/// its owner's stock every `intervalTicks`. The item is a currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Income {
    pub item: Id,
    pub amount: u32,
    pub interval_ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SelectionClass {
    Army,
    Worker,
}

/// A unit, building, item or resource after behaviour sets are resolved.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// The model asset (or a piece of scenery) it is drawn with.
    pub model: Id,
    pub icon: Id,
    /// How this definition shows its model, beyond the model's own display.
    pub presentation: Option<DefinitionPresentation>,
    pub display_order: Option<i64>,
    pub hero: Option<bool>,
    /// Maximum distinct colony heroes, provided by the highest completed hall.
    pub hero_capacity: Option<u32>,
    pub unit_nature: Option<UnitNature>,
    pub dimensions: Option<Dimensions>,
    /// Presentation only; never changes movement, collision or weapon range.
    pub model_scale: Option<f64>,
    pub supply_cost: Option<u32>,
    /// At most this many living or in training per player (a unit: one Juggernaut).
    pub max_owned: Option<u32>,
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
    /// Sources and nodes of the power network (D66).
    pub network: Option<Network>,
    /// Buildings that only work inside the power network (D66).
    pub power: Option<Power>,
    pub placement_on: Option<PlacementOn>,
    /// Goods the building makes by itself while it stands complete.
    pub income: Option<Income>,
    pub creation: Option<Creation>,
    pub behaviors: Behaviors,
}

/// What a definition adds to its model's display: weapon visuals, cast timing, camera,
/// and the model's health bar height or animation profile when they differ for this use.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefinitionPresentation {
    pub character: Option<Id>,
    pub health_height: Option<f64>,
    /// Visual effect of its missiles.
    pub projectile: Option<Id>,
    /// Model node missiles leave from.
    pub projectile_socket: Option<String>,
    /// Visual effect where its shells and missiles land, and where its melee blows strike.
    pub impact_effect: Option<Id>,
    /// Visual effect played from the projectile socket as each missile or shell leaves
    /// (muzzle flash, steam, a beam to the target), lasting the flight.
    pub launch_effect: Option<Id>,
    /// Share of the cast animation at which a spell lands.
    pub cast_contact: Option<f64>,
    pub camera_anchor: Option<CameraAnchor>,
    /// A building's effects at work (D65): smoke while it works, sparks on a strike.
    pub effects: Option<Vec<StateEffect>>,
    /// An item's colour (`#rrggbb`) where amounts of it show in the world: the floating
    /// text of a delivery.
    pub color: Option<String>,
}

/// An effect a building or unit plays from one of its model's sockets: looping for as
/// long as it is in `state` ([`StateEffect::BUILDING_STATES`],
/// [`StateEffect::UNIT_STATES`]), or once whenever its clip crosses the animation event
/// `event`. Exactly one of `state` and `event`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateEffect {
    pub effect: Id,
    pub state: Option<String>,
    pub event: Option<String>,
    /// Socket name (`capabilities.sockets`) or a model node named `socket*`; the model's
    /// origin without one.
    pub socket: Option<String>,
}

impl StateEffect {
    /// A building's states (D65): `idle` while it rests (complete, nothing to do), `work`
    /// while it trains, researches, upgrades, revives, earns or is gathered from, `spawn` once
    /// per trained unit, `attack` once per shot of a tower's crew, `always` whenever it
    /// stands complete in sight.
    pub const BUILDING_STATES: &[&str] = &["idle", "work", "spawn", "attack", "always"];
    /// A unit's states: its animation states, `move` (walking, running, carrying or
    /// charging), `cast` (any cast in progress) and `always` (while it is in sight).
    pub const UNIT_STATES: &[&str] = &[
        "always",
        "move",
        "idle",
        "walk",
        "run",
        "charge",
        "attack",
        "hit",
        "cast",
        "carry",
        "carry_walk",
        "carry_run",
        "build",
        "chop",
    ];
}

impl Definition {
    pub fn is_hero(&self) -> bool {
        self.hero == Some(true)
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ActionPlacement {
    Banner,
    BottomRow,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Navigation {
    pub back: NavigationMeta,
    pub camera: Option<NavigationMeta>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionOverride {
    /// `null` removes the category; absent keeps it.
    #[serde(default, with = "serde_with::rust::double_option")]
    #[schemars(with = "Option<Option<Id>>")]
    pub category: Option<Option<Id>>,
    pub hidden: Option<bool>,
    pub priority: Option<i64>,
    pub hotkey: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Actions {
    pub navigation: Navigation,
    #[serde(default)]
    pub categories: BTreeMap<Id, CategoryMeta>,
    pub actions: BTreeMap<String, ActionMeta>,
    pub overrides: BTreeMap<String, ActionOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArmorType {
    pub name: String,
    pub icon: Id,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Faster weapon cycles: 250 makes 25% more attacks.
    pub attack_speed_permille: Option<u32>,
    /// Faster movement: 200 is 20% faster.
    pub move_speed_permille: Option<u32>,
    /// Ability bindings (by binding id) raised to at least this rank: 1 unlocks an
    /// ability the unit trains without (`initialRank` 0), 2 strengthens it.
    pub ability_ranks: Option<BTreeMap<String, u32>>,
    /// Cells added to the network radius of the listed buildings (D66); `units` then
    /// lists buildings with a `network`.
    pub network_radius: Option<u32>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Research {
    pub name: String,
    pub description: String,
    pub icon: Id,
    pub priority: i64,
    pub requires: Option<Vec<Id>>,
    /// The previous level of a research chain (Plating I before Plating II). The command
    /// card shows a chain one level at a time in its first level's slot: this level stays
    /// hidden until the previous one is queued, under way or done, then takes its place
    /// (disabled until the previous one is done). It must also be in `requires`.
    pub follows: Option<Id>,
    pub items: Vec<Cost>,
    pub work_ticks: u32,
    pub effects: Vec<ResearchEffect>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LootEntry {
    /// `None` is an empty roll.
    pub item: Option<Id>,
    pub weight: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LootPool {
    pub rolls: u32,
    pub max_tier: Option<u32>,
    pub entries: Vec<LootEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DamageType {
    pub name: String,
    pub applies_armor: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridOffset {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingHero {
    pub default: Id,
    pub choices: Vec<Id>,
    pub offset: GridOffset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingGathering {
    pub item: Id,
    pub workers: u32,
    pub radius: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingUnit {
    pub definition: Id,
    pub offset: GridOffset,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartingSetup {
    pub id: Id,
    pub fort: Id,
    pub hero: Option<StartingHero>,
    pub gathering: Option<Vec<StartingGathering>>,
    pub inventory: BTreeMap<Id, u32>,
    pub units: Vec<StartingUnit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Race {
    pub name: String,
    pub description: String,
    pub hud: Option<RaceHud>,
    pub construction: Option<ConstructionPresentation>,
    pub starting_setup: Option<StartingSetup>,
    pub roster: Vec<Id>,
}

/// Optional interface art; absent pieces keep the UI theme's default.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RaceHud {
    pub frame: Option<Id>,
    pub resources: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Campaign {
    pub name: String,
    pub race: String,
}

fn default_race() -> String {
    "ants".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rules {
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    pub launch_effect: Option<Id>,
    pub projectile_socket: Option<String>,
    pub sockets: Option<Vec<Socket>>,
    pub cast_contact: Option<f64>,
    pub scenery_asset: Option<String>,
    pub effects: Option<Vec<StateEffect>>,
}

fn default_anchor_distance() -> f64 {
    7.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CameraAnchor {
    pub height: f64,
    #[serde(default)]
    pub forward: f64,
    #[serde(default = "default_anchor_distance")]
    pub distance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Socket {
    pub name: String,
    pub node: String,
    pub offset: [f64; 3],
}

impl RenderAsset {
    /// A render asset with nothing but an id to be set.
    pub fn empty() -> Self {
        Self {
            id: Id::new("empty").expect("valid id"),
            file: None,
            image: None,
            harvest_animation: None,
            dimensions: None,
            carry_asset: None,
            character: None,
            scale: None,
            health_height: None,
            camera_anchor: None,
            stack_height: None,
            stack_columns: None,
            projectile: None,
            impact_effect: None,
            launch_effect: None,
            projectile_socket: None,
            sockets: None,
            cast_contact: None,
            scenery_asset: None,
            effects: None,
        }
    }
}
