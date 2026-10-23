//! Workers and the job loop (old `Economy.workers`, `advance`, `deliver`, `reroute`,
//! `abandon`, `interrupt`, `detachUnit`, `remove`, `depositCargo`, `applyPending`).

use super::{
    Ctx, Delivery, JobKind, Phase, active, alive,
    jobs::{self, Spec},
    sites, stock,
};
use crate::{
    entity::{EntityId, Place},
    orders::{Order, discard_navigation, release_combat},
    spatial::Spot,
    state::{FactKind, State},
};
use canopy_content::{Id, placement::Owner};

/// Ticks a worker waits before retrying after a failed assignment.
pub const RETRY_TICKS: u32 = 40;

/// Idle workers of `owner` ready for a task, in id order (old `workers`). `unassigned`
/// skips workplace staff.
pub fn workers(ctx: &Ctx, owner: Owner, unassigned: bool) -> Vec<EntityId> {
    let tick = ctx.state.tick;
    ctx.index
        .units
        .iter()
        .copied()
        .filter(|&id| {
            ctx.state.entities.get(id).is_some_and(|e| {
                let u = e.unit.as_ref().expect("unit");
                e.owner == owner
                    && active(e, tick)
                    && ctx.catalog.get(e.definition).behaviors.work.is_some()
                    && u.order.is_none()
                    && u.order_queue.is_empty()
                    && u.taunt.is_none()
                    && u.work.pending_move.is_none()
                    && u.work.job.is_none()
                    && u.work.cargo.is_none()
                    && (!unassigned || u.work.employment.is_none())
                    && u.retry_at <= tick
            })
        })
        .collect()
}

/// Whether the unit stands exactly on the centre of `spot` (old `atPoint`).
pub fn at(ctx: &Ctx, id: EntityId, spot: Spot) -> bool {
    ctx.state.entities.get(id).is_some_and(|e| {
        ctx.spatial.deck_of(e) == spot.deck
            && e.position.x == spot.x * 1000
            && e.position.y == spot.y * 1000
    })
}

/// Whether the worker has reached the end of its route (no route left, and at its goal
/// if it has one).
pub fn arrived(ctx: &Ctx, id: EntityId) -> bool {
    let Some(u) = ctx.state.entities.get(id).and_then(|e| e.unit.as_ref()) else {
        return false;
    };
    u.route.is_empty() && u.goal.is_none_or(|g| at(ctx, id, ctx.spatial.spot(g)))
}

/// Interrupts a unit's activity (old `Economy.interrupt`): orders, engagement and
/// navigation, and its job and workplace. A carrier on a trip keeps its job and takes
/// `destination` once the goods are delivered. Needs only the state, so commands, missions
/// and other systems can call it.
pub fn interrupt(state: &mut State, id: EntityId, destination: Option<Place>) -> bool {
    let tick = state.tick;
    let Some(e) = state.entities.get_mut(id) else {
        return false;
    };
    let Some(u) = e.unit.as_mut() else {
        return false;
    };
    if u.contained.is_some() || u.release.is_some() {
        return false;
    }
    u.order_queue.clear();
    release_combat(u);
    u.detour = None;
    u.last_moved_tick = None;
    u.idle = None;
    let workplace = u.work.employment.take();
    let carrying = u.work.cargo.is_some() && u.work.job.is_some();
    let job = u.work.job;
    if let Some(p) = workplace
        .and_then(|w| state.entities.get_mut(w))
        .and_then(|w| w.production.as_mut())
    {
        if p.active.as_ref().is_some_and(|a| a.worker == Some(id)) {
            p.active = None;
        }
        if p.staff == Some(id) {
            p.staff = None;
        }
    }
    if carrying {
        let u = state
            .entities
            .get_mut(id)
            .and_then(|e| e.unit.as_mut())
            .expect("unit");
        u.order = None;
        u.work.pending_move = destination;
        return true;
    }
    if let Some(job) = job.and_then(|j| state.economy.job(j)).cloned() {
        // The employment is already released, so only a planter's workplace (the job's
        // target) resets; the worker carries nothing, so nothing is rerouted.
        if job.kind != JobKind::Harvest {
            release_active(state, job.target, id);
        }
        if let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
            u.retry_at = tick + RETRY_TICKS;
        }
        jobs::remove(state, job.id);
    }
    let u = state
        .entities
        .get_mut(id)
        .and_then(|e| e.unit.as_mut())
        .expect("unit");
    u.order = destination.map(|destination| Order::Move {
        destination,
        attack_move: false,
    });
    discard_navigation(u);
    u.work.pending_move = None;
    true
}

/// The workplace stops the cycle `worker` was carrying out.
fn release_active(state: &mut State, workplace: EntityId, worker: EntityId) {
    let Some(p) = state
        .entities
        .get_mut(workplace)
        .and_then(|b| b.production.as_mut())
    else {
        return;
    };
    if p.active.as_ref().and_then(|a| a.worker) != Some(worker) {
        return;
    }
    p.active = None;
    p.status = super::Status::WaitingForWorker;
    if p.paused {
        let staff = p.staff.take();
        if let Some(u) = staff
            .and_then(|s| state.entities.get_mut(s))
            .and_then(|s| s.unit.as_mut())
        {
            u.work.employment = None;
        }
    }
}

/// Gives up a job (old `abandon`): its workplace cycle stops, the worker waits before its
/// next try, and goods it carries look for another store.
pub fn abandon(ctx: &mut Ctx, id: u32) {
    let Some(job) = ctx.state.economy.job(id).cloned() else {
        return;
    };
    let worker = job.worker;
    let workplace = if job.kind == JobKind::Harvest {
        ctx.state
            .entities
            .get(worker)
            .and_then(|w| w.unit.as_ref())
            .and_then(|u| u.work.employment)
    } else {
        Some(job.target)
    };
    if let Some(b) = workplace {
        release_active(ctx.state, b, worker);
    }
    let tick = ctx.state.tick;
    let contained = ctx
        .state
        .entities
        .get(worker)
        .and_then(|w| w.unit.as_ref())
        .and_then(|u| u.contained);
    if contained.is_some() {
        let at = workplace
            .or(Some(worker))
            .and_then(|b| ctx.state.entities.get(b))
            .map(|b| sites::entrance_of(ctx.catalog, b));
        if let Some(at) = at {
            release(ctx, worker, at);
        }
    }
    if let Some(u) = ctx
        .state
        .entities
        .get_mut(worker)
        .and_then(|w| w.unit.as_mut())
    {
        u.retry_at = tick + RETRY_TICKS;
    }
    jobs::erase(ctx, id);
    let carrying = ctx
        .state
        .entities
        .get(worker)
        .and_then(|w| w.unit.as_ref())
        .is_some_and(|u| u.work.cargo.is_some());
    if carrying {
        reroute(ctx, worker);
    }
}

/// Lets a held unit out near `at` (old `GameContext.release`): movement places it on the
/// nearest free cell.
pub fn release(ctx: &mut Ctx, id: EntityId, at: crate::entity::Milli) {
    let Some(u) = ctx.state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) else {
        return;
    };
    release_combat(u);
    u.detour = None;
    u.idle = None;
    u.segment = None;
    u.contained = None;
    u.work.job = None;
    u.work.employment = None;
    u.order = None;
    u.route.clear();
    u.goal = None;
    let (x, y) = at.cell();
    u.release = Some(Place::new(x, y));
}

/// Workers carrying goods without a job look for a store (old `assign` prologue).
pub fn reroute_carriers(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    let carriers: Vec<EntityId> = ctx
        .index
        .units
        .iter()
        .copied()
        .filter(|&id| {
            ctx.state.entities.get(id).is_some_and(|e| {
                active(e, tick)
                    && e.unit.as_ref().is_some_and(|u| {
                        u.taunt.is_none() && u.work.cargo.is_some() && u.work.job.is_none()
                    })
            })
        })
        .collect();
    for id in carriers {
        reroute(ctx, id);
    }
}

/// Completed drop-offs of `owner` that accept `item` and have `need` room, nearest to
/// `from` first (old `hall` and the store search of `reroute`).
pub fn stores(ctx: &Ctx, owner: Owner, item: &str, need: u32, from: (i64, i64)) -> Vec<EntityId> {
    let mut found: Vec<(i64, EntityId)> = ctx
        .index
        .buildings
        .iter()
        .filter_map(|&id| ctx.state.entities.get(id))
        .filter(|e| {
            alive(e)
                && e.owner == owner
                && e.construction.is_none()
                && ctx
                    .catalog
                    .get(e.definition)
                    .behaviors
                    .storage
                    .as_ref()
                    .is_some_and(|s| s.dropoff == Some(true))
                && stock::accepts(ctx.catalog, e, item)
                && stock::room(ctx.catalog, &ctx.state.economy.jobs, e, Some(item)) >= need.max(1)
        })
        .map(|e| (super::distance2(super::coarse(e), from), e.id))
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, id)| id).collect()
}

/// Sends a carrier to the nearest store with room for its goods (old `reroute`).
pub fn reroute(ctx: &mut Ctx, id: EntityId) {
    let tick = ctx.state.tick;
    let Some(e) = ctx.state.entities.get(id) else {
        return;
    };
    let Some(u) = e.unit.as_ref() else {
        return;
    };
    let Some(cargo) = u.work.cargo.clone() else {
        return;
    };
    if !alive(e) || u.work.job.is_some() || tick < u.retry_at {
        return;
    }
    let constructing = matches!(u.order, Some(Order::Construct { .. }));
    let stores = stores(
        ctx,
        e.owner,
        cargo.item.as_str(),
        cargo.amount,
        super::coarse(e),
    );
    for &b in &stores {
        if sites::work_point(ctx, b, id).is_some() {
            jobs::create(
                ctx,
                JobKind::Deliver,
                id,
                b,
                Spec {
                    phase: Phase::Return,
                    item: Some(cargo.item.clone()),
                    amount: cargo.amount,
                    ..Spec::default()
                },
            );
            return;
        }
    }
    if stores.is_empty() && constructing {
        return;
    }
    let u = ctx
        .state
        .entities
        .get_mut(id)
        .and_then(|e| e.unit.as_mut())
        .expect("unit");
    u.retry_at = tick + RETRY_TICKS;
    if let Some(destination) = u.work.pending_move.take() {
        u.order = Some(Order::Move {
            destination,
            attack_move: false,
        });
    }
}

/// Unloads a carrier's goods: currencies into its owner's wallet, other goods into the
/// store (old `depositCargo`).
pub fn deposit(ctx: &mut Ctx, worker: EntityId, store: EntityId) {
    let Some(w) = ctx.state.entities.get_mut(worker) else {
        return;
    };
    let owner = w.owner;
    let Some(cargo) = w.unit.as_mut().and_then(|u| u.work.cargo.take()) else {
        return;
    };
    let Some(store_entity) = ctx.state.entities.get(store) else {
        return;
    };
    let dropoff = ctx
        .catalog
        .get(store_entity.definition)
        .behaviors
        .storage
        .as_ref()
        .is_some_and(|s| s.dropoff == Some(true));
    let store_owner = store_entity.owner;
    if stock::currency(ctx.catalog, cargo.item.as_str()) {
        let permille = ctx.tables.income.get(&owner).copied().unwrap_or(1000);
        stock::add(
            ctx.state.wallets.entry(owner).or_default(),
            &cargo.item,
            (u64::from(cargo.amount) * u64::from(permille) / 1000) as u32,
        );
    } else if let Some(s) = ctx.state.entities.get_mut(store) {
        stock::add(&mut s.inventory, &cargo.item, cargo.amount);
    }
    if dropoff {
        ctx.deliveries.push(Delivery {
            tick: ctx.state.tick,
            owner: store_owner,
            item: cargo.item,
            amount: cargo.amount,
            store,
            carrier: Some(worker),
        });
    }
}

/// A carrier that was told to move while delivering now takes that move (old
/// `applyPending`).
pub fn apply_pending(ctx: &mut Ctx, worker: EntityId) {
    let Some(u) = ctx
        .state
        .entities
        .get_mut(worker)
        .and_then(|e| e.unit.as_mut())
    else {
        return;
    };
    if u.work.cargo.is_some() {
        return;
    }
    if let Some(destination) = u.work.pending_move.take() {
        let attack_move = matches!(
            u.order,
            Some(Order::Move {
                attack_move: true,
                ..
            })
        );
        u.order = Some(Order::Move {
            destination,
            attack_move,
        });
    }
}

/// Delivers goods at the end of a delivery trip (old `deliver`).
fn deliver(ctx: &mut Ctx, id: u32, worker: EntityId, store: EntityId) {
    let job_amount = ctx.state.economy.job(id).map_or(0, |j| j.amount);
    let w = ctx.state.entities.get(worker).expect("worker");
    let b = ctx.state.entities.get(store).expect("store");
    let cargo = w.unit.as_ref().and_then(|u| u.work.cargo.clone());
    let valid = cargo.as_ref().is_some_and(|cargo| {
        let item = cargo.item.as_str();
        ctx.catalog
            .get(b.definition)
            .behaviors
            .storage
            .as_ref()
            .is_some_and(|s| s.dropoff == Some(true))
            && stock::accepts(ctx.catalog, b, item)
            && b.owner == w.owner
            && stock::room(ctx.catalog, &ctx.state.economy.jobs, b, Some(item))
                .saturating_add(job_amount)
                >= cargo.amount
    });
    if !valid {
        abandon(ctx, id);
        return;
    }
    deposit(ctx, worker, store);
    jobs::erase(ctx, id);
    apply_pending(ctx, worker);
}

/// Step 9 jobs pass (old `Economy.advance` up to training): harvesters queue at their
/// sources, then every job advances in id order.
pub fn advance_jobs(ctx: &mut Ctx) {
    super::harvest::admit_harvesters(ctx);
    let tick = ctx.state.tick;
    let ids: Vec<u32> = ctx.state.economy.jobs.iter().map(|j| j.id).collect();
    for id in ids {
        let Some(job) = ctx.state.economy.job(id).cloned() else {
            continue;
        };
        let (worker, target) = (job.worker, job.target);
        let valid = ctx
            .state
            .entities
            .get(worker)
            .is_some_and(|w| w.unit.is_some() && alive(w))
            && ctx.state.entities.get(target).is_some_and(alive);
        if !valid {
            abandon(ctx, id);
            continue;
        }
        let w = ctx.state.entities.get(worker).expect("worker");
        if super::held(ctx.catalog, w) {
            continue;
        }
        let u = w.unit.as_ref().expect("unit");
        if !u.route.is_empty() {
            continue;
        }
        if let Some(goal) = u.goal {
            let spot = ctx.spatial.spot(goal);
            if !at(ctx, worker, spot) {
                if tick >= u.retry_at {
                    if !sites::route(ctx, worker, spot) {
                        abandon(ctx, id);
                    }
                    if let Some(u) = ctx
                        .state
                        .entities
                        .get_mut(worker)
                        .and_then(|w| w.unit.as_mut())
                    {
                        u.retry_at = tick + RETRY_TICKS;
                    }
                }
                continue;
            }
        }
        match job.kind {
            JobKind::Deliver => deliver(ctx, id, worker, target),
            JobKind::Construct => super::construction::advance_construct(ctx, id),
            JobKind::Repair => super::construction::advance_repair(ctx, id),
            JobKind::Harvest => super::harvest::advance_harvest(ctx, id),
            JobKind::Plant => super::production::advance_plant(ctx, id),
        }
    }
}

/// Releases everything that refers to a unit changing hands, keeping its goods (old
/// `detachUnit`).
pub fn detach_unit(ctx: &mut Ctx, id: EntityId) {
    interrupt(ctx.state, id, None);
    jobs::refresh_body(ctx, id);
    let refs: Vec<u32> = ctx
        .state
        .economy
        .jobs
        .iter()
        .filter(|j| j.worker == id || j.target == id || j.source == Some(id))
        .map(|j| j.id)
        .collect();
    for job in refs {
        jobs::erase(ctx, job);
    }
    for &b in &ctx.index.buildings.clone() {
        if let Some(p) = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.production.as_mut())
        {
            if p.staff == Some(id) {
                p.staff = None;
            }
            if p.active.as_ref().is_some_and(|a| a.worker == Some(id)) {
                p.active = None;
            }
        }
    }
}

/// Everything the economy must settle before an entity leaves the match (old `remove`
/// up to the removal itself): unfinished paid technology and carried goods are lost, jobs
/// that refer to it are given up, staff and employment are released, and a cancelled
/// project refunds its share. Returns the facts' wording for the goods left behind.
pub fn release_references(ctx: &mut Ctx, id: EntityId, cancel: bool) {
    let Some(e) = ctx.state.entities.get(id) else {
        return;
    };
    let owner = e.owner;
    let def = ctx.catalog.get(e.definition);
    let mut paid: Vec<(Id, u32)> = Vec::new();
    if e.upgrade.is_some()
        && let Some(upgrade) = &def.upgrade
    {
        paid.extend(stock::bill(&upgrade.items));
    }
    for q in e.research.iter().flat_map(|r| &r.queue) {
        if let Some(r) = ctx.catalog.content.rules.research.get(q.id.as_str()) {
            paid.extend(stock::bill(&r.items));
        }
    }
    if let Some(policy) = &def.behaviors.revival {
        for q in e.revival.iter().flat_map(|r| &r.queue) {
            paid.extend(super::revival::terms(policy, q.level).0);
        }
    }
    for (item, amount) in &paid {
        stock::record(
            ctx.state,
            owner,
            FactKind::Lost,
            "message.unfinished-technology-lost",
            Some((item, *amount)),
        );
    }
    let refs: Vec<u32> = ctx
        .state
        .economy
        .jobs
        .iter()
        .filter(|j| j.worker == id || j.target == id || j.source == Some(id))
        .map(|j| j.id)
        .collect();
    for job in refs {
        abandon(ctx, job);
    }
    for &u in &ctx.index.units.clone() {
        if let Some(unit) = ctx.state.entities.get_mut(u).and_then(|e| e.unit.as_mut())
            && unit.work.employment == Some(id)
        {
            unit.work.employment = None;
        }
    }
    for &b in &ctx.index.buildings.clone() {
        if let Some(p) = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.production.as_mut())
            && p.staff == Some(id)
        {
            p.staff = None;
        }
    }
    let e = ctx.state.entities.get(id).expect("entity");
    if let Some(cargo) = e.unit.as_ref().and_then(|u| u.work.cargo.clone()) {
        stock::record(
            ctx.state,
            owner,
            FactKind::Lost,
            "message.carrier-cargo-lost",
            Some((&cargo.item, cargo.amount)),
        );
    }
    let e = ctx.state.entities.get(id).expect("entity");
    let held = e
        .production
        .as_ref()
        .and_then(|p| p.active.as_ref())
        .and_then(|a| a.worker)
        .filter(|&w| {
            ctx.state
                .entities
                .get(w)
                .and_then(|w| w.unit.as_ref())
                .is_some_and(|u| u.contained == Some(id))
        });
    if let Some(w) = held {
        let at = sites::entrance_of(ctx.catalog, e);
        release(ctx, w, at);
    }
    if cancel {
        let e = ctx.state.entities.get(id).expect("entity");
        let fraction = if e.construction.is_some() {
            ctx.catalog.content.rules.construction_refund_permille
        } else {
            1000
        };
        let bill: Vec<(Id, u32)> = e
            .inventory
            .iter()
            .map(|(item, &amount)| {
                (
                    item.clone(),
                    (u64::from(amount) * u64::from(fraction) / 1000) as u32,
                )
            })
            .collect();
        stock::refund(ctx, id, &bill);
    }
}

/// Removes an entity from the match with its economic consequences (old `Economy.remove`):
/// see [`release_references`]; then whatever it still holds is lost. `cancel` refunds a
/// project.
pub fn remove(ctx: &mut Ctx, id: EntityId, cancel: bool) {
    release_references(ctx, id, cancel);
    crate::garrisons::evict(ctx.state, ctx.spatial, id);
    crate::tunnels::collapse(ctx.state, ctx.spatial, id);
    let Some(e) = ctx.state.entities.remove(id) else {
        return;
    };
    ctx.index.sync(ctx.state, ctx.catalog);
    ctx.spatial.sync_entity(ctx.state, id);
    record_losses(ctx.state, ctx.catalog, &e, cancel);
}

/// Facts for the goods an entity leaving the match still holds (the end of old
/// `Economy.remove`).
pub fn record_losses(
    state: &mut State,
    catalog: &crate::catalog::Catalog,
    e: &crate::entity::Entity,
    cancel: bool,
) {
    let message = if cancel {
        "Unrefunded project resources"
    } else {
        "Stored resources lost"
    };
    for (item, &amount) in &e.inventory {
        stock::record(
            state,
            e.owner,
            FactKind::Lost,
            message,
            Some((item, amount)),
        );
    }
    if let Some(item) = &e.item {
        let id = catalog.id(e.definition).clone();
        stock::record(
            state,
            e.owner,
            FactKind::Lost,
            "message.ground-goods-lost",
            Some((&id, item.quantity)),
        );
    }
}
