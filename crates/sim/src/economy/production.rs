//! Workplaces: queued training and automatic production, staff, inputs, deployment and
//! rally points (old `head`, `requestInputs`, `startWorkplace`, `employ`, `releaseStaff`,
//! `finishCycle`, `completeUnit`, `queue`, `cancelEntry`, `pause`, the planting branch of
//! `advance`, `workplaceQueue.ts`, and the produce/cancel/pause/rally commands).

use super::{
    Active, Ctx, JobKind, QueueEntry, Rally, Status, alive, coarse, distance2,
    harvest::{can_assign_gather, harvest_item, set_regrowth},
    jobs::{self, Spec},
    ready, sites, stock,
    supply::{self, Cache},
    work::{abandon, workers},
};
use crate::{
    catalog::Def,
    entity::{EntityId, Milli, Place},
    orders::Order,
    state::FactKind,
};
use canopy_content::game::{Creation, ProductionMode};

/// Which kind of work is next in a building's shared lane (old `workplaceHead`): the
/// queue entry with the smaller id, training or revival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Production,
    Revival,
}

pub fn lane(e: &crate::entity::Entity) -> Option<Lane> {
    let production = e
        .production
        .as_ref()
        .and_then(|p| p.queue.first())
        .map(|q| q.id);
    let revival = e
        .revival
        .as_ref()
        .and_then(|r| r.queue.first())
        .map(|q| q.id);
    match (production, revival) {
        (None, None) => None,
        (None, Some(_)) => Some(Lane::Revival),
        (Some(_), None) => Some(Lane::Production),
        (Some(p), Some(r)) => Some(if p < r {
            Lane::Production
        } else {
            Lane::Revival
        }),
    }
}

/// Entries in the shared lane (old `workplaceQueueSize`).
pub fn lane_size(e: &crate::entity::Entity) -> usize {
    e.production.as_ref().map_or(0, |p| p.queue.len())
        + e.revival.as_ref().map_or(0, |r| r.queue.len())
}

/// The cycle a workplace works on next (old `head`).
pub fn head(ctx: &Ctx, id: EntityId) -> Option<(Def, Option<u32>)> {
    let e = ctx.state.entities.get(id)?;
    let p = e.production.as_ref()?;
    let policy = ctx
        .catalog
        .get(e.definition)
        .behaviors
        .production
        .as_ref()?;
    if e.construction.is_some() {
        return None;
    }
    if let Some(a) = &p.active {
        return Some((a.definition, a.queue));
    }
    if p.paused {
        return None;
    }
    match policy.mode {
        ProductionMode::Queued => p.queue.first().map(|q| (q.definition, Some(q.id))),
        ProductionMode::Automatic => {
            Some((ctx.catalog.find(policy.outputs.first()?.as_str())?, None))
        }
    }
}

/// Funds a workplace's next cycle from the owner's goods, all or nothing (old
/// `requestInputs`).
pub fn request_inputs(ctx: &mut Ctx, id: EntityId) {
    let Some((def, _)) = head(ctx, id) else {
        return;
    };
    let e = ctx.state.entities.get(id).expect("workplace");
    let price = stock::price(ctx.catalog, def);
    if !stock::missing(&e.inventory, price) {
        return;
    }
    let short: Vec<canopy_content::game::Cost> = price
        .iter()
        .map(|c| canopy_content::game::Cost {
            item: c.item.clone(),
            amount: c
                .amount
                .saturating_sub(stock::quantity(&e.inventory, c.item.as_str())),
        })
        .collect();
    let missing: u32 = short.iter().map(|c| c.amount).sum();
    if stock::total(&e.inventory) + missing > stock::capacity(ctx.catalog, e) {
        return;
    }
    let owner = e.owner;
    if let Some(reservation) = stock::reserve(ctx.state, ctx.catalog, ctx.index, owner, &short) {
        stock::admit(ctx.state, id, &reservation);
    }
}

fn set_status(ctx: &mut Ctx, id: EntityId, status: Status) {
    if let Some(p) = ctx
        .state
        .entities
        .get_mut(id)
        .and_then(|e| e.production.as_mut())
    {
        p.status = status;
    }
}

fn employ(ctx: &mut Ctx, b: EntityId, w: EntityId) {
    if let Some(p) = ctx
        .state
        .entities
        .get_mut(b)
        .and_then(|e| e.production.as_mut())
    {
        p.staff = Some(w);
    }
    if let Some(u) = ctx.state.entities.get_mut(w).and_then(|e| e.unit.as_mut()) {
        u.work.employment = Some(b);
    }
}

/// The workplace lets its staff go (old `releaseStaff`).
pub fn release_staff(ctx: &mut Ctx, b: EntityId) {
    let staff = ctx
        .state
        .entities
        .get_mut(b)
        .and_then(|e| e.production.as_mut())
        .and_then(|p| p.staff.take());
    if let Some(u) = staff
        .and_then(|w| ctx.state.entities.get_mut(w))
        .and_then(|w| w.unit.as_mut())
    {
        u.work.employment = None;
    }
}

/// Step 4: a workplace starts its next cycle if it can (old `startWorkplace`).
pub fn start_workplace(ctx: &mut Ctx, b: EntityId, supply: &mut Cache) {
    let e = ctx.state.entities.get(b).expect("workplace");
    if e.upgrade.is_some() {
        set_status(ctx, b, Status::Upgrading);
        return;
    }
    let p = e.production.as_ref().expect("production");
    if p.paused {
        if p.active.is_none() {
            release_staff(ctx, b);
            set_status(ctx, b, Status::Paused);
        }
        return;
    }
    if ctx.network.unpowered(b) {
        set_status(ctx, b, Status::Unpowered);
        return;
    }
    if lane(e) == Some(Lane::Revival) {
        set_status(ctx, b, Status::RevivingHero);
        return;
    }
    let Some((def, queue)) = head(ctx, b) else {
        set_status(ctx, b, Status::Idle);
        return;
    };
    let e = ctx.state.entities.get(b).expect("workplace");
    let p = e.production.as_ref().expect("production");
    if p.active.is_some() {
        return;
    }
    if stock::missing(&e.inventory, stock::price(ctx.catalog, def)) {
        set_status(ctx, b, Status::WaitingForStoredResources);
        return;
    }
    let owner = e.owner;
    let target = ctx.catalog.get(def);
    match target.creation.as_ref() {
        Some(Creation::Train { .. }) => {
            let cost = target.supply_cost.unwrap_or(0);
            if let Some(reason) = supply::start(&supply.get(ctx, owner), cost) {
                set_status(ctx, b, reason);
                return;
            }
            let p = ctx
                .state
                .entities
                .get_mut(b)
                .and_then(|e| e.production.as_mut())
                .expect("production");
            p.active = Some(Active {
                definition: def,
                queue,
                worker: None,
                progress: 0,
            });
            p.status = Status::Training;
            supply.started(owner, cost);
        }
        Some(Creation::Plant { .. }) => {
            let staff = p.staff;
            let busy = staff
                .and_then(|w| ctx.state.entities.get(w))
                .and_then(|w| w.unit.as_ref())
                .is_some_and(|u| {
                    u.taunt.is_some()
                        || u.work.job.is_some()
                        || u.order.is_some()
                        || !u.order_queue.is_empty()
                        || u.work.cargo.is_some()
                        || u.work.pending_move.is_some()
                });
            if busy {
                set_status(ctx, b, Status::WorkerBusy);
                return;
            }
            let worker = match staff.filter(|&w| ctx.state.entities.get(w).is_some_and(alive)) {
                Some(w) => w,
                None => match workers(ctx, owner, true).first() {
                    Some(&w) => w,
                    None => {
                        set_status(ctx, b, Status::WaitingForWorker);
                        return;
                    }
                },
            };
            plant(ctx, b, def, worker);
        }
        // Automatic harvest workplaces never start a cycle in the old game either.
        _ => {}
    }
}

/// Sends the workplace's planter to the nearest felled, unplanted resource in reach.
fn plant(ctx: &mut Ctx, b: EntityId, def: Def, worker: EntityId) {
    let e = ctx.state.entities.get(b).expect("workplace");
    let here = coarse(e);
    let radius = ctx
        .catalog
        .get(e.definition)
        .behaviors
        .production
        .as_ref()
        .and_then(|p| p.work_radius)
        .map_or(0, i64::from);
    let mut sources: Vec<(i64, EntityId)> = ctx
        .index
        .resources
        .near(here.0, here.1, radius * 1000)
        .into_iter()
        .filter_map(|id| ctx.state.entities.get(id))
        .filter(|r| {
            alive(r)
                && r.definition == def
                && r.resource
                    .as_ref()
                    .is_some_and(|res| res.amount == 0 && res.growing_until.is_none())
                && distance2(coarse(r), here) <= (radius * 1000).pow(2)
        })
        .map(|r| (distance2(coarse(r), here), r.id))
        .collect();
    sources.sort_unstable();
    for (_, resource) in sources {
        if ctx.state.economy.jobs.occupied(JobKind::Plant, resource) {
            continue;
        }
        if sites::work_point(ctx, resource, worker).is_none() {
            continue;
        }
        jobs::create(
            ctx,
            JobKind::Plant,
            worker,
            b,
            Spec {
                source: Some(resource),
                ..Spec::default()
            },
        );
        employ(ctx, b, worker);
        let p = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.production.as_mut())
            .expect("production");
        p.active = Some(Active {
            definition: def,
            queue: None,
            worker: Some(worker),
            progress: 0,
        });
        p.status = Status::Planting;
        return;
    }
    set_status(ctx, b, Status::NoReachableResource);
}

/// One tick of planting (old plant branch of `advance`).
pub fn advance_plant(ctx: &mut Ctx, id: u32) {
    let job = ctx.state.economy.job(id).expect("job").clone();
    let active = ctx
        .state
        .entities
        .get(job.target)
        .and_then(|b| b.production.as_ref())
        .and_then(|p| p.active.clone())
        .filter(|a| a.worker == Some(job.worker));
    let Some(active) = active else {
        abandon(ctx, id);
        return;
    };
    let resource = job.source.filter(|&s| {
        ctx.state
            .entities
            .get(s)
            .is_some_and(|r| r.resource.is_some())
    });
    let Some(resource) = resource else {
        abandon(ctx, id);
        return;
    };
    let target = ctx.catalog.get(active.definition);
    let work_ticks = match &target.creation {
        Some(Creation::Plant { work_ticks, .. }) => *work_ticks,
        _ => 0,
    };
    let progress = job.progress + 1;
    ctx.state.economy.job_mut(id).expect("job").progress = progress;
    if progress >= work_ticks {
        let until = ctx.state.tick + target.regrowth_ticks.unwrap_or(0);
        set_regrowth(ctx, resource, Some(until));
        jobs::erase(ctx, id);
        finish_cycle(ctx, job.target);
    }
}

/// Closes a workplace's cycle (old `finishCycle`).
pub fn finish_cycle(ctx: &mut Ctx, b: EntityId) {
    let Some(p) = ctx
        .state
        .entities
        .get_mut(b)
        .and_then(|e| e.production.as_mut())
    else {
        return;
    };
    let Some(active) = p.active.take() else {
        return;
    };
    if let Some(q) = active.queue {
        p.queue.retain(|e| e.id != q);
    }
    p.produced += 1;
    p.status = Status::Idle;
    if p.paused {
        release_staff(ctx, b);
    }
}

/// Takes a cycle's inputs from the workplace's stock (old `inputConsume`).
fn consume_inputs(ctx: &mut Ctx, b: EntityId, def: Def) {
    let owner = ctx
        .state
        .entities
        .get(b)
        .map(|e| e.owner)
        .expect("workplace");
    for cost in stock::price(ctx.catalog, def) {
        if let Some(e) = ctx.state.entities.get_mut(b) {
            stock::take(&mut e.inventory, cost.item.as_str(), cost.amount);
        }
        stock::record(
            ctx.state,
            owner,
            FactKind::Consumed,
            "Production input consumed",
            Some((&cost.item, cost.amount)),
        );
    }
}

/// Step 9: training advances and completes (old training branch of `advance`).
pub fn advance_training(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    let buildings = ctx.index.buildings.clone();
    for b in buildings {
        let Some(e) = ctx.state.entities.get(b) else {
            continue;
        };
        if !ready(e, tick) || e.construction.is_some() || e.upgrade.is_some() {
            continue;
        }
        let Some(p) = e.production.as_ref() else {
            continue;
        };
        let Some(active) = p.active.as_ref() else {
            continue;
        };
        let Some(Creation::Train { work_ticks, .. }) = &ctx.catalog.get(active.definition).creation
        else {
            continue;
        };
        let work_ticks = *work_ticks;
        let p = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.production.as_mut())
            .expect("production");
        if p.paused {
            p.status = Status::Paused;
            continue;
        }
        if ctx.network.unpowered(b) {
            p.status = Status::Unpowered;
            continue;
        }
        p.status = Status::Training;
        let a = p.active.as_mut().expect("active");
        a.progress = work_ticks.min(a.progress + 1);
        if a.progress >= work_ticks {
            complete_unit(ctx, b);
        }
    }
}

/// A trained unit leaves its building (old `completeUnit`): on the side facing its rally
/// (a followed unit is tracked live), else the doorway side. Harvesters rallied onto a
/// resource gather it, units rallied onto a friendly unit follow it, others walk to the
/// flag.
fn complete_unit(ctx: &mut Ctx, b: EntityId) -> bool {
    let e = ctx.state.entities.get(b).expect("workplace");
    let owner = e.owner;
    let p = e.production.as_ref().expect("production");
    let def = p.active.as_ref().expect("active").definition;
    if supply::living_units(ctx, owner) >= ctx.catalog.content.rules.max_units as usize {
        set_status(ctx, b, Status::UnitLimitReached);
        return false;
    }
    let rally = p.rally.clone();
    let target = rally
        .as_ref()
        .and_then(|r| r.target)
        .and_then(|t| ctx.state.entities.get(t));
    let toward = match (target.filter(|t| alive(t)), &rally) {
        (Some(t), _) => {
            let (x, y) = coarse(t);
            Milli {
                x: x as i32,
                y: y as i32,
            }
        }
        (None, Some(r)) => Milli {
            x: r.point.x * 1000,
            y: r.point.y * 1000,
        },
        (None, None) => sites::entrance_of(ctx.catalog, e),
    };
    let Some(exit) = sites::deployment(ctx, b, toward, def, 8) else {
        set_status(ctx, b, Status::DeploymentBlocked);
        return false;
    };
    let e = ctx.state.entities.get(b).expect("workplace");
    if stock::missing(&e.inventory, stock::price(ctx.catalog, def)) {
        return false;
    }
    let unit = crate::spawn::insert(
        ctx.state,
        ctx.catalog,
        crate::spawn::Spawn {
            definition: def,
            owner,
            position: Milli {
                x: exit.x * 1000,
                y: exit.y * 1000,
            },
            surface: None,
            rotation: 0,
            placement: None,
            initial: Default::default(),
            complete: true,
            ready_tick: ctx.state.tick + 1,
        },
    );
    ctx.index.sync(ctx.state, ctx.catalog);
    let target = rally.as_ref().and_then(|r| r.target);
    let order = {
        let u = ctx.state.entities.get(unit).expect("unit");
        let t = target.and_then(|t| ctx.state.entities.get(t));
        match t {
            Some(t)
                if t.resource.is_some()
                    && harvest_item(ctx.catalog, u, t).is_some()
                    && can_assign_gather(ctx.catalog, u, t) =>
            {
                Some(Order::Gather { target: t.id })
            }
            Some(t)
                if alive(t)
                    && t.owner == owner
                    && t.unit.as_ref().is_some_and(|tu| tu.contained.is_none()) =>
            {
                Some(Order::Follow {
                    target: t.id,
                    escort: false,
                })
            }
            _ => rally.map(|r| Order::Move {
                destination: r.point,
                attack_move: false,
            }),
        }
    };
    if let Some(u) = ctx
        .state
        .entities
        .get_mut(unit)
        .and_then(|e| e.unit.as_mut())
    {
        u.order = order;
    }
    consume_inputs(ctx, b, def);
    finish_cycle(ctx, b);
    true
}

/// Queues a unit, paying its whole bill into the building (old `queue`); the command
/// checks come first in [`produce`].
fn enqueue(ctx: &mut Ctx, b: EntityId, def: Def) -> bool {
    let e = ctx.state.entities.get(b).expect("workplace");
    let owner = e.owner;
    let Some(reservation) = stock::reserve(
        ctx.state,
        ctx.catalog,
        ctx.index,
        owner,
        stock::price(ctx.catalog, def),
    ) else {
        return false;
    };
    stock::admit(ctx.state, b, &reservation);
    let id = ctx.state.counters.queue;
    ctx.state.counters.queue += 1;
    ctx.state
        .entities
        .get_mut(b)
        .and_then(|e| e.production.as_mut())
        .expect("production")
        .queue
        .push(QueueEntry {
            id,
            definition: def,
        });
    true
}

/// The `produce` command after the actor checks (old `Game.command` produce branch and
/// `Economy.queue`).
pub fn produce(ctx: &mut Ctx, b: EntityId, definition: &str) -> Result<(), String> {
    let e = ctx.state.entities.get(b).expect("workplace");
    let owner = e.owner;
    let policy = ctx
        .catalog
        .get(e.definition)
        .behaviors
        .production
        .as_ref()
        .expect("production");
    let def = ctx
        .catalog
        .find(definition)
        .filter(|_| policy.mode == ProductionMode::Queued)
        .filter(|_| policy.outputs.iter().any(|o| o.as_str() == definition))
        .ok_or("Unsupported output")?;
    if lane_size(e) >= policy.queue_capacity.unwrap_or(0) as usize {
        return Err("Queue is full".into());
    }
    let target = ctx.catalog.get(def);
    if let Some(reason) = supply::prerequisite(ctx, owner, target.requires.as_deref()) {
        return Err(reason);
    }
    if let Some(reason) = supply::owned_limit(ctx, owner, def) {
        return Err(reason);
    }
    let roster = crate::heroes::Roster::of(ctx.state, ctx.catalog, owner);
    if let Some(reason) = supply::hero_admission(ctx.catalog, def, &roster) {
        return Err(reason);
    }
    let colony = supply::colony(ctx.state, ctx.catalog, ctx.index, owner);
    let hint = ctx.tables.hints.get(&owner).map(String::as_str);
    if let Some(reason) = supply::admission(&colony, target.supply_cost.unwrap_or(0), hint) {
        return Err(reason);
    }
    if !enqueue(ctx, b, def) {
        return Err("Insufficient unreserved materials".into());
    }
    Ok(())
}

/// Cancels a queued unit and refunds its bill (old `cancelEntry`).
pub fn cancel_entry(ctx: &mut Ctx, b: EntityId, entry: u32) -> bool {
    let Some(p) = ctx
        .state
        .entities
        .get(b)
        .and_then(|e| e.production.as_ref())
    else {
        return false;
    };
    let Some(def) = p.queue.iter().find(|q| q.id == entry).map(|q| q.definition) else {
        return false;
    };
    if let Some(active) = p.active.as_ref().filter(|a| a.queue == Some(entry)) {
        let job = active
            .worker
            .and_then(|w| ctx.state.entities.get(w))
            .and_then(|w| w.unit.as_ref())
            .and_then(|u| u.work.job);
        if let Some(job) = job {
            abandon(ctx, job);
        }
        if let Some(p) = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.production.as_mut())
        {
            p.active = None;
        }
    }
    if let Some(p) = ctx
        .state
        .entities
        .get_mut(b)
        .and_then(|e| e.production.as_mut())
    {
        p.queue.retain(|q| q.id != entry);
    }
    let bill = stock::bill(stock::price(ctx.catalog, def));
    stock::refund(ctx, b, &bill);
    true
}

/// Pauses or resumes a workplace (old `pause`).
pub fn pause(ctx: &mut Ctx, b: EntityId, paused: bool) {
    let Some(p) = ctx
        .state
        .entities
        .get_mut(b)
        .and_then(|e| e.production.as_mut())
    else {
        return;
    };
    p.paused = paused;
    if paused && p.active.is_none() {
        release_staff(ctx, b);
    }
}

/// Sets the rally: onto a target entity (its position, tracked live when it moves), a
/// point, or none.
pub fn set_rally(ctx: &mut Ctx, b: EntityId, rally: Option<Rally>) {
    if let Some(p) = ctx
        .state
        .entities
        .get_mut(b)
        .and_then(|e| e.production.as_mut())
    {
        p.rally = rally;
    }
}

/// A rally onto `target`: its cell (and deck) as the flag.
pub fn rally_on(target: &crate::entity::Entity) -> Rally {
    let (x, y) = target.position.cell();
    Rally {
        point: Place {
            x,
            y,
            surface: target.surface.clone(),
        },
        target: Some(target.id),
    }
}
