//! The settlement economy (old `Economy`, `src/sim/game/economy.ts`, with `population.ts`,
//! `supply.ts`, `goodsView.ts`, `colony.ts`, `research.ts`, `upgrades.ts`, `revival.ts` and
//! the construction and production commands of `game.ts`).
//!
//! Workers carry out jobs (deliver, construct, repair, harvest, plant) through walk, wait,
//! work, fall and return phases. Player wallets fund projects; physical goods move in
//! carriers. Buildings train units, research, upgrade and revive heroes.
//!
//! State lives in entities (unit work, production, research, upgrade and revival parts,
//! felling) and in [`EconomyState`] (jobs, completed research). Everything in [`Index`] is
//! derived: rebuilt on load and checked against the incremental version in tests.

pub mod command;
pub mod construction;
pub mod harvest;
pub mod income;
pub mod index;
pub mod jobs;
pub mod lane;
pub mod network;
pub mod production;
pub mod research;
pub mod revival;
pub mod sites;
pub mod stock;
pub mod supply;
pub mod upgrade;
pub mod view;
pub mod work;

pub use index::Index;
pub use jobs::{Job, JobKind, Jobs, Phase};
pub use network::Network;
pub use work::interrupt;

use crate::{
    catalog::{Catalog, Def},
    entity::{Entity, EntityId, Milli, Place},
    setup::MatchSetup,
    spatial::Spatial,
    state::State,
};
use canopy_content::{Id, placement::Owner};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// Economy records outside entities: active jobs (by id), each owner's completed
/// research (sorted), and damaged buildings no idle worker could reach, with the tick
/// before which nobody tries again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EconomyState {
    pub jobs: Jobs,
    pub research: BTreeMap<Owner, Vec<Id>>,
    #[serde(default)]
    pub repair_retry: BTreeMap<EntityId, u32>,
}

impl EconomyState {
    pub fn job(&self, id: u32) -> Option<&Job> {
        self.jobs.get(id)
    }

    pub fn job_mut(&mut self, id: u32) -> Option<&mut Job> {
        self.jobs.get_mut(id)
    }

    /// Completed research of `owner`.
    pub fn researched(&self, owner: Owner) -> &[Id] {
        self.research.get(&owner).map_or(&[], Vec::as_slice)
    }
}

/// Goods a carrier holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cargo {
    pub item: Id,
    pub amount: u32,
}

/// A worker's part in the economy.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    /// The job it carries out.
    pub job: Option<u32>,
    pub cargo: Option<Cargo>,
    /// The workplace that employs it.
    pub employment: Option<EntityId>,
    /// A move ordered while carrying goods: taken once the goods are delivered.
    pub pending_move: Option<Place>,
}

impl Work {
    pub fn is_idle(&self) -> bool {
        self.job.is_none()
            && self.cargo.is_none()
            && self.employment.is_none()
            && self.pending_move.is_none()
    }
}

/// How a tree falls: health shown while it is cut, the last strike, the tick it fell and
/// the direction it falls in (milli-cells, away from the last cutter).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Felling {
    pub hp: u32,
    pub last_hit_tick: Option<u32>,
    pub fall_tick: Option<u32>,
    pub direction: Milli,
}

impl Felling {
    pub fn standing(max_hp: u32) -> Self {
        Self {
            hp: max_hp,
            last_hit_tick: None,
            fall_tick: None,
            direction: Milli { x: 0, y: 1000 },
        }
    }
}

/// One queued unit, paid when queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    /// Its place in the building's queue (D72): drawn from the match's queue counter, so
    /// the smaller id was queued first.
    pub id: u32,
    pub definition: Def,
}

/// The cycle a workplace is working on.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Active {
    pub definition: Def,
    /// The queue entry it trains; `None` for automatic production.
    pub queue: Option<u32>,
    /// The worker carrying out external work (planting).
    pub worker: Option<EntityId>,
    pub progress: u32,
}

/// Where produced units go: a point, or onto a resource (harvesters gather it) or a
/// friendly unit (followed).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rally {
    pub point: Place,
    pub target: Option<EntityId>,
}

/// What a workplace is doing, for the HUD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Idle,
    Paused,
    Upgrading,
    RevivingHero,
    WaitingForStoredResources,
    Training,
    WorkerBusy,
    WaitingForWorker,
    Planting,
    NoReachableResource,
    UnitLimitReached,
    SupplyBlocked,
    /// Research queued before the next unit is under way (D72).
    Researching,
    DeploymentBlocked,
    /// Needs power and stands outside its owner's network (D66).
    Unpowered,
}

impl Status {
    /// The old game's status line; `supply` names the building that adds supply.
    pub fn label(self, supply: Option<&str>) -> String {
        match self {
            Self::Idle => "Idle".into(),
            Self::Paused => "Paused".into(),
            Self::Upgrading => "Upgrading".into(),
            Self::RevivingHero => "Reviving hero".into(),
            Self::WaitingForStoredResources => "Waiting for stored resources".into(),
            Self::Training => "Training".into(),
            Self::WorkerBusy => "Worker busy".into(),
            Self::WaitingForWorker => "Waiting for worker".into(),
            Self::Planting => "Planting".into(),
            Self::NoReachableResource => "No reachable eligible resource".into(),
            Self::UnitLimitReached => "Unit limit reached".into(),
            Self::Researching => "Researching".into(),
            Self::SupplyBlocked => match supply {
                Some(name) => format!("Supply blocked — build a {name} or free supply"),
                None => "Supply blocked — free supply".into(),
            },
            Self::DeploymentBlocked => "Deployment blocked".into(),
            Self::Unpowered => "Unpowered: outside the network".into(),
        }
    }
}

/// A building's production: queued training or automatic output, its staff and rally.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Production {
    pub paused: bool,
    pub queue: Vec<QueueEntry>,
    pub active: Option<Active>,
    pub staff: Option<EntityId>,
    pub produced: u32,
    pub rally: Option<Rally>,
    pub status: Status,
}

impl Default for Production {
    fn default() -> Self {
        Self {
            paused: false,
            queue: Vec::new(),
            active: None,
            staff: None,
            produced: 0,
            rally: None,
            status: Status::Idle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchEntry {
    pub id: Id,
    pub progress: u32,
    /// Its place in the building's queue among training and revivals (D72), from the
    /// same counter as their ids. Saves from before D72 have none (0): loading numbers
    /// them after everything else queued ([`lane::order_legacy`]).
    #[serde(default)]
    pub order: u32,
}

/// Research paid for at a building; the head progresses when it leads the building's
/// queue (D72).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchQueue {
    pub queue: Vec<ResearchEntry>,
}

/// A paid in-place transformation into `target`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeTask {
    pub target: Def,
    pub progress: u32,
}

/// A paid hero revival; the level (and so the price and time) is fixed when queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevivalEntry {
    /// Its place in the building's queue (D72), as [`QueueEntry::id`].
    pub id: u32,
    pub hero: EntityId,
    pub level: u32,
    pub progress: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevivalQueue {
    pub queue: Vec<RevivalEntry>,
}

/// Goods arriving at a drop-off this tick, for presentation (not saved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub tick: u32,
    pub owner: Owner,
    pub item: Id,
    pub amount: u32,
    /// The drop-off, or the building whose income it is.
    pub store: EntityId,
    /// The worker that carried it in (none for a building's income).
    pub carrier: Option<EntityId>,
}

/// Lookups compiled once from content and the match setup.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    /// Per owner: the name of the cheapest constructable supply building of its race, for
    /// supply messages (the old game always said "Mound").
    pub hints: BTreeMap<Owner, String>,
    /// Per building definition: the buildings whose requirement it fulfils, itself and
    /// every building that upgrades into it (old `fulfilledPrerequisites`).
    pub fulfils: Vec<Vec<Def>>,
    /// Per owner: the name of the cheapest constructable network node of its race, for
    /// placement messages (D66).
    pub network: BTreeMap<Owner, String>,
    /// Per owner: gathered currencies count this many permille (a campaign AI's income
    /// handicap); 1000 when absent.
    pub income: BTreeMap<Owner, u32>,
}

/// Derived economy data kept beside the state: indexes, compiled tables and the tick's
/// deliveries. Never saved.
pub struct Economy {
    pub(crate) index: Index,
    /// The power network (D66), derived from the buildings and research.
    pub(crate) network: Network,
    tables: Tables,
    /// Goods delivered during the latest tick.
    pub deliveries: Vec<Delivery>,
}

/// Everything economy passes work on.
pub struct Ctx<'a> {
    pub state: &'a mut State,
    pub catalog: &'a Catalog,
    pub spatial: &'a mut Spatial,
    pub index: &'a mut Index,
    /// The power network as of this context's creation.
    pub network: &'a Network,
    pub tables: &'a Tables,
    pub deliveries: &'a mut Vec<Delivery>,
}

impl Economy {
    pub fn new(state: &State, catalog: &Catalog, setup: &MatchSetup) -> Self {
        let index = Index::rebuild(state, catalog);
        let mut network = Network::default();
        network.refresh(state, catalog, &index);
        Self {
            index,
            network,
            tables: Tables {
                hints: supply::hints(catalog, setup),
                fulfils: supply::fulfils(catalog),
                network: network::hints(catalog, setup),
                income: BTreeMap::new(),
            },
            deliveries: Vec::new(),
        }
    }

    pub fn tables(&self) -> &Tables {
        &self.tables
    }

    /// Makes `owner`'s gathered currencies count `permille` (a campaign Skirmish player's
    /// income handicap). Derived: set again after a restore.
    pub fn set_income(&mut self, owner: Owner, permille: u32) {
        self.tables.income.insert(owner, permille);
    }

    pub fn ctx<'a>(
        &'a mut self,
        state: &'a mut State,
        catalog: &'a Catalog,
        spatial: &'a mut Spatial,
    ) -> Ctx<'a> {
        self.index.sync(state, catalog);
        self.network.refresh(state, catalog, &self.index);
        Ctx {
            state,
            catalog,
            spatial,
            index: &mut self.index,
            network: &self.network,
            tables: &self.tables,
            deliveries: &mut self.deliveries,
        }
    }

    /// The derived index, brought up to date with entity creation and removal.
    pub fn index(&mut self, state: &State, catalog: &Catalog) -> &Index {
        self.index.sync(state, catalog);
        self.network.refresh(state, catalog, &self.index);
        &self.index
    }

    /// The power network (D66) as of the latest sync.
    pub fn network(&self) -> &Network {
        &self.network
    }

    /// The node building name used in network messages for `owner`.
    pub fn network_hint(&self, owner: Owner) -> Option<&str> {
        self.tables.network.get(&owner).map(String::as_str)
    }

    /// The supply building name used in messages for `owner`.
    pub fn hint(&self, owner: Owner) -> Option<&str> {
        self.tables.hints.get(&owner).map(String::as_str)
    }

    /// Checks the incrementally maintained index against a cold rebuild.
    pub fn verify(&mut self, state: &State, catalog: &Catalog) -> Result<(), String> {
        self.index.sync(state, catalog);
        let cold = Index::rebuild(state, catalog);
        self.index.verify(&cold)?;
        self.network.refresh(state, catalog, &self.index);
        let mut fresh = Network::default();
        fresh.refresh(state, catalog, &cold);
        if fresh != self.network {
            return Err("power network differs from a rebuild".into());
        }
        Ok(())
    }
}

/// Step 4 (old `Economy.assign`): funding and work allocation before movement.
pub fn assign(ctx: &mut Ctx) {
    let _span = tracing::trace_span!("economy assign").entered();
    ctx.spatial.sync_bodies(ctx.state);
    work::reroute_carriers(ctx);
    let tick = ctx.state.tick;
    let buildings: Vec<EntityId> = ctx
        .index
        .buildings
        .iter()
        .copied()
        .filter(|&id| {
            ctx.state
                .entities
                .get(id)
                .is_some_and(|e| ready(e, tick) && e.owner != Owner::Nobody)
        })
        .collect();
    let constructing = |ctx: &Ctx, id: EntityId| {
        ctx.state
            .entities
            .get(id)
            .is_some_and(|e| e.construction.is_some())
    };
    for &id in &buildings {
        if constructing(ctx, id) {
            production::request_inputs(ctx, id);
        }
    }
    construction::construction_orders(ctx);
    for &id in &buildings {
        let automatic = ctx.state.entities.get(id).is_some_and(|e| {
            e.production.is_some()
                && e.construction.is_none()
                && ctx
                    .catalog
                    .get(e.definition)
                    .behaviors
                    .production
                    .as_ref()
                    .is_some_and(|p| p.mode == canopy_content::game::ProductionMode::Automatic)
        });
        if automatic {
            production::request_inputs(ctx, id);
        }
    }
    let mut supply = supply::Cache::default();
    for &id in &buildings {
        let workplace = ctx
            .state
            .entities
            .get(id)
            .is_some_and(|e| e.production.is_some() && e.construction.is_none());
        if workplace {
            production::start_workplace(ctx, id, &mut supply);
        }
    }
    harvest::gather_orders(ctx);
    construction::assign_repairs(ctx, &buildings);
}

/// Step 9 (old `Revival.tick`, `Economy.advance`, `BuildingUpgrades.tick`,
/// `Research.tick`): revivals, jobs, training and regrowth, upgrades, research. Of a
/// building's training, research and revivals only the one queued first advances (D72).
pub fn advance(ctx: &mut Ctx) {
    let _span = tracing::trace_span!("economy advance").entered();
    revival::advance(ctx);
    work::advance_jobs(ctx);
    production::advance_training(ctx);
    harvest::advance_regrowth(ctx);
    upgrade::advance(ctx);
    research::advance(ctx);
    income::advance(ctx);
}

pub(crate) fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
}

pub(crate) fn ready(e: &Entity, tick: u32) -> bool {
    alive(e) && e.ready_tick <= tick
}

/// Ready, alive and physically on the map (old `activeUnits` membership).
pub(crate) fn active(e: &Entity, tick: u32) -> bool {
    ready(e, tick)
        && e.unit
            .as_ref()
            .is_some_and(|u| u.contained.is_none() && u.release.is_none())
}

/// A taunt holds the unit on its challenger: no work, no hauling (old `u.taunt`).
pub(crate) fn taunted(e: &Entity) -> bool {
    e.unit.as_ref().is_some_and(|u| u.taunt.is_some())
}

/// Taunted or stunned: the worker neither starts nor advances a job.
pub(crate) fn held(catalog: &Catalog, e: &Entity) -> bool {
    taunted(e) || crate::abilities::statuses::stunned(catalog, e)
}

/// Squared distance in milli-cells² between the coarse positions the old game compared:
/// a unit's cell, everything else's placement point.
pub(crate) fn distance2(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)
}

/// The old `e.x`/`e.y` in milli-cells: a unit's current cell, a static entity's position.
pub(crate) fn coarse(e: &Entity) -> (i64, i64) {
    if e.unit.is_some() {
        let (x, y) = e.position.cell();
        (i64::from(x) * 1000, i64::from(y) * 1000)
    } else {
        (i64::from(e.position.x), i64::from(e.position.y))
    }
}

/// The exact position in milli-cells (old `precise`).
pub(crate) fn precise(e: &Entity) -> (i64, i64) {
    (i64::from(e.position.x), i64::from(e.position.y))
}
