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
    Train {
        items: Vec<Cost>,
        work_ticks: u32,
        /// Units hatched by one training, for its one price (the ants' Raiders come in
        /// pairs); each still uses its own supply. Default 1.
        count: Option<u32>,
    },
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
    /// Units one training makes (1 unless a `train` creation sets `count`).
    pub fn count(&self) -> u32 {
        match self {
            Creation::Train { count, .. } => count.unwrap_or(1).max(1),
            _ => 1,
        }
    }

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

/// A tunnel mound (`Definition::tunnel`). Units ordered far away walk to their nearest
/// mound, go under one at a time (one every `interval_ticks`; research makes it faster),
/// travel at `speed` cells a second and come up at the mound nearest their destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tunnel {
    pub interval_ticks: u32,
    pub speed: u32,
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

/// A breakable object (Warcraft III's crates and barrels; also rubble that closes a path):
/// a neutral building any unit may attack when ordered to, which no unit attacks by
/// itself. When it breaks it drops what it holds: the items its placement put in it
/// (`initialState.inventory`), else a roll of its placement's `lootPool`, else a roll of
/// this pool, else these `drops`. Its footprint blocks the ground until it breaks.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Breakable {
    /// The loot pool rolled when the placement names no items or pool of its own.
    pub loot_pool: Option<Id>,
    /// Items dropped when neither the placement nor `lootPool` names any.
    pub drops: Option<Vec<Id>>,
}

/// A spring (Warcraft III's Fountain of Health and Fountain of Mana): every
/// `intervalTicks` every living unit within `radius` cells of its footprint, of any owner,
/// regains `health` plus `healthPermille` of its maximum health, and `mana` plus
/// `manaPermille` of its maximum mana.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Restoration {
    /// Cells from the footprint's edge.
    pub radius: f64,
    pub interval_ticks: u32,
    pub health: Option<u32>,
    pub health_permille: Option<u32>,
    pub mana: Option<u32>,
    pub mana_permille: Option<u32>,
}

/// One kind of item a shop sells: its price, the most it holds, and how long it takes to
/// grow back by one after a sale. A shop starts with every ware fully stocked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ware {
    pub item: Id,
    pub price: Vec<Cost>,
    pub stock: u32,
    pub restock_ticks: u32,
}

/// A shop (Warcraft III's Goblin Merchant): a player buys one of its wares for a hero of
/// theirs standing within `range` cells of its footprint, with goods from their stock. The
/// item goes straight into the hero's inventory. A placement may sell other wares
/// (`initialState.wares`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shop {
    /// Cells from the footprint's edge.
    pub range: f64,
    pub wares: Vec<Ware>,
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
    /// Warcraft III's bounty: currencies its killer's owner gains when an enemy kills it
    /// (a supply cart spills its amber for whoever breaks it).
    pub bounty: Option<Vec<Cost>>,
    /// A tunnel mound: its owner's units travel underground between such buildings
    /// when that is faster than walking (the ants' tunnels).
    pub tunnel: Option<Tunnel>,
    /// Whether this unit may travel through its owner's tunnels (default true; flyers
    /// never do). The ants' mutated castes are too big for them.
    pub underground: Option<bool>,
    /// A neutral crate, barrel or rubble: attacked only when ordered, drops loot.
    pub breakable: Option<Breakable>,
    /// A spring that restores the health and mana of units near it.
    pub restoration: Option<Restoration>,
    /// A neutral shop where heroes buy items.
    pub shop: Option<Shop>,
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
    /// An item's state: `always`, while it lies on the ground in sight.
    pub const ITEM_STATES: &[&str] = &["always"];
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
    /// A fixed cell of the command card (1–12, row by row) the category's button keeps on
    /// every page; other commands flow around it.
    pub slot: Option<u32>,
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
    /// More units a second through the listed tunnel mounds: 500 lets 50% more through.
    pub tunnel_throughput_permille: Option<u32>,
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
    /// Effects every item lying on the ground plays whose own `presentation.effects`
    /// declares none (D90): what marks dropped loot. States: `always`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub item_effects: Vec<StateEffect>,
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
    /// The model's close dialogue portrait, when it frames it itself.
    pub portrait: Option<PortraitFrame>,
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
            portrait: None,
        }
    }
}

/// What a model's close dialogue portrait frames (asset.json `display.portrait`), in the
/// model's own space (its metres, before its display scale): a cube of side `size` round
/// `focus`, for a model whose automatic framing misses its face; `turn` turns the model
/// that many degrees about its vertical axis (default −18, a little to the viewer's
/// left), for a face its gear hides from the front.
#[skip_serializing_none]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortraitFrame {
    pub focus: [f64; 3],
    pub size: f64,
    pub turn: Option<f64>,
}
