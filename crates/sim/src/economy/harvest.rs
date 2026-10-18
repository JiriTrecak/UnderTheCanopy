//! Harvesting (old `gatherOrders`, `startHarvest`, `admitHarvesters`, `advanceHarvest`,
//! `finishHarvest`, `startGathering`, `advanceRegrowth`, `canRegrow`, `regrowthExit`,
//! `gatherAssignments`).
//!
//! A worker with a gather order takes the least crowded source of the same kind near the
//! one it was sent to, walks there, waits its turn (first come, first served, at most
//! `activeWorkers` at once), works one load, and carries it to the nearest drop-off.
//! Trees fall before the load leaves. Depleted resources leave occupancy at once; planted
//! ones regrow when nothing stands in their way.

use super::{
    Ctx, Felling, JobKind, Phase, active, alive, coarse, distance2,
    jobs::{self, Spec},
    precise, production, sites, stock,
    work::{self, RETRY_TICKS, abandon, apply_pending, deposit},
};
use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId},
    orders::Order,
    setup::MatchSetup,
    spatial::Spot,
    state::FactKind,
};
use canopy_content::{
    Id,
    game::{Creation, Definition},
    placement::Owner,
};
use std::collections::{BTreeMap, BTreeSet};

/// The item a worker harvests from `resource`, if it can (old `harvestItem`).
pub fn harvest_item<'c>(
    catalog: &'c Catalog,
    worker: &Entity,
    resource: &Entity,
) -> Option<&'c Id> {
    let source = catalog.id(resource.definition);
    catalog
        .get(worker.definition)
        .behaviors
        .work
        .as_ref()?
        .harvests
        .iter()
        .flatten()
        .find(|id| {
            matches!(
                catalog.content.definition(id.as_str()).and_then(|d| d.creation.as_ref()),
                Some(Creation::Harvest { source: s, .. }) if s == source
            )
        })
}

/// Whether a worker can be sent to harvest `resource` now (old `canAssignGather`).
pub fn can_assign_gather(catalog: &Catalog, worker: &Entity, resource: &Entity) -> bool {
    alive(resource)
        && resource.resource.as_ref().is_some_and(|r| r.amount > 0)
        && harvest_item(catalog, worker, resource).is_some()
}

/// The living building placed on `resource` (`placementOn`), finished or not (D62).
pub fn building_on(ctx: &Ctx, resource: &Entity) -> Option<EntityId> {
    let source = ctx.catalog.id(resource.definition);
    ctx.index.buildings.iter().copied().find(|&id| {
        ctx.state.entities.get(id).is_some_and(|b| {
            alive(b)
                && b.position == resource.position
                && ctx
                    .catalog
                    .get(b.definition)
                    .placement_on
                    .as_ref()
                    .is_some_and(|on| &on.source == source)
        })
    })
}

/// The standing resource a building placed on one (`placementOn`) stands on.
pub fn resource_under(ctx: &Ctx, building: &Entity) -> Option<EntityId> {
    let on = ctx.catalog.get(building.definition).placement_on.as_ref()?;
    let source = ctx.catalog.find(on.source.as_str())?;
    let (x, y) = (
        i64::from(building.position.x),
        i64::from(building.position.y),
    );
    ctx.index
        .resources
        .near(x, y, 1000)
        .into_iter()
        .find(|&id| {
            ctx.state.entities.get(id).is_some_and(|r| {
                r.definition == source && r.position == building.position && alive(r)
            })
        })
}

/// Whether `owner`'s workers may gather `resource`: one that needs a building only through
/// its owner's completed building standing on it (D62).
pub fn open_to(ctx: &Ctx, owner: Owner, resource: &Entity) -> bool {
    let needs = ctx
        .catalog
        .get(resource.definition)
        .harvesting
        .as_ref()
        .is_some_and(|h| h.needs_building());
    !needs
        || building_on(ctx, resource)
            .and_then(|b| ctx.state.entities.get(b))
            .is_some_and(|b| b.owner == owner && b.construction.is_none())
}

/// The resource a gather order on `target` means: the resource under a building placed on
/// one, else the target itself.
pub fn gather_target(ctx: &Ctx, target: EntityId) -> EntityId {
    ctx.state
        .entities
        .get(target)
        .filter(|t| t.resource.is_none())
        .and_then(|t| resource_under(ctx, t))
        .unwrap_or(target)
}

fn endless(catalog: &Catalog, resource: &Entity) -> bool {
    catalog
        .get(resource.definition)
        .harvesting
        .as_ref()
        .is_some_and(|h| h.endless())
}

fn recipe(catalog: &Catalog, item: &str) -> Option<HarvestRecipe> {
    match catalog.content.definition(item)?.creation.as_ref()? {
        Creation::Harvest {
            amount,
            work_ticks,
            impact_tick,
            animation_ticks,
            ..
        } => Some(HarvestRecipe {
            amount: *amount,
            work_ticks: *work_ticks,
            impact_tick: *impact_tick,
            cycle: animation_ticks.unwrap_or(*work_ticks),
        }),
        _ => None,
    }
}

struct HarvestRecipe {
    amount: u32,
    work_ticks: u32,
    impact_tick: Option<u32>,
    cycle: u32,
}

/// Each worker's source: its harvest job's, else its gather order's (old
/// `gatherAssignments`). A carrier counts at the source it is returning from.
pub fn assignments(ctx: &Ctx) -> BTreeMap<EntityId, EntityId> {
    let mut out = BTreeMap::new();
    for &id in &ctx.index.units {
        let Some(e) = ctx.state.entities.get(id) else {
            continue;
        };
        if !alive(e) {
            continue;
        }
        let Some(u) = e.unit.as_ref() else {
            continue;
        };
        let job = u
            .work
            .job
            .and_then(|j| ctx.state.economy.job(j))
            .filter(|j| j.kind == JobKind::Harvest)
            .and_then(|j| j.source);
        let order = match &u.order {
            Some(Order::Gather { target }) => Some(*target),
            _ => None,
        };
        if let Some(source) = job.or(order) {
            out.insert(id, source);
        }
    }
    out
}

fn counts(assignments: &BTreeMap<EntityId, EntityId>) -> BTreeMap<EntityId, i64> {
    let mut counts = BTreeMap::new();
    for &source in assignments.values() {
        *counts.entry(source).or_insert(0i64) += 1;
    }
    counts
}

fn active_workers(catalog: &Catalog, e: &Entity) -> i64 {
    catalog
        .get(e.definition)
        .harvesting
        .as_ref()
        .map_or(1, |h| i64::from(h.active_workers.max(1)))
}

/// Living resources of `def` within `radius` cells of `around` with something left, in id
/// order.
fn standing_near(
    ctx: &Ctx,
    around: (i64, i64),
    radius: i64,
    def: crate::catalog::Def,
) -> Vec<EntityId> {
    ctx.index
        .resources
        .near(around.0, around.1, radius * 1000)
        .into_iter()
        .filter(|&id| {
            ctx.state.entities.get(id).is_some_and(|e| {
                alive(e)
                    && e.definition == def
                    && e.resource.as_ref().is_some_and(|r| r.amount > 0)
                    && distance2(coarse(e), around) <= (radius * 1000).pow(2)
            })
        })
        .collect()
}

/// Step 4: workers with gather orders pick a source and set out (old `gatherOrders`).
pub fn gather_orders(ctx: &mut Ctx) {
    let assignments = assignments(ctx);
    let mut counts = counts(&assignments);
    let tick = ctx.state.tick;
    let units = ctx.index.units.clone();
    for id in units {
        let Some(w) = ctx.state.entities.get(id) else {
            continue;
        };
        if !active(w, tick) {
            continue;
        }
        let u = w.unit.as_ref().expect("unit");
        if u.taunt.is_some() {
            continue;
        }
        let Some(Order::Gather { target }) = u.order else {
            continue;
        };
        if u.work.job.is_some() || u.work.cargo.is_some() || u.retry_at > tick {
            continue;
        }
        let origin = ctx
            .state
            .entities
            .get(target)
            .filter(|o| open_to(ctx, w.owner, o));
        let item = origin
            .and_then(|o| harvest_item(ctx.catalog, w, o))
            .cloned();
        let (Some(origin), Some(item)) = (origin, item) else {
            let u = ctx
                .state
                .entities
                .get_mut(id)
                .and_then(|e| e.unit.as_mut())
                .expect("unit");
            u.order = None;
            continue;
        };
        let radius = ctx
            .catalog
            .get(origin.definition)
            .harvesting
            .as_ref()
            .map_or(0.0, |h| h.search_radius);
        let previous = assignments.get(&id).copied();
        let here = coarse(origin);
        let worker_at = coarse(w);
        let mut candidates: Vec<EntityId> = if radius > 0.0 {
            // Search radii are authored in whole cells.
            let r = radius.ceil() as i64;
            ctx.index
                .resources
                .near(here.0, here.1, r * 1000)
                .into_iter()
                .filter(|&c| {
                    ctx.state.entities.get(c).is_some_and(|e| {
                        alive(e)
                            && e.definition == origin.definition
                            && e.resource.as_ref().is_some_and(|r| r.amount > 0)
                            && (distance2(coarse(e), here) as f64) <= (radius * 1000.0).powi(2)
                    })
                })
                .collect()
        } else {
            [target]
                .into_iter()
                .filter(|_| origin.resource.as_ref().is_some_and(|r| r.amount > 0))
                .collect()
        };
        let load = |c: EntityId| {
            let e = ctx.state.entities.get(c).expect("candidate");
            let count = counts.get(&c).copied().unwrap_or(0) - i64::from(previous == Some(c));
            (count, active_workers(ctx.catalog, e))
        };
        candidates.sort_by(|&a, &b| {
            let ((na, da), (nb, db)) = (load(a), load(b));
            (na * db)
                .cmp(&(nb * da))
                .then_with(|| (b == target).cmp(&(a == target)))
                .then_with(|| {
                    let ea = coarse(ctx.state.entities.get(a).expect("candidate"));
                    let eb = coarse(ctx.state.entities.get(b).expect("candidate"));
                    distance2(ea, worker_at).cmp(&distance2(eb, worker_at))
                })
                .then_with(|| a.cmp(&b))
        });
        let owner = w.owner;
        let mut started = false;
        for &c in &candidates {
            // The store nearest the source, where the load will be: a worker starts each
            // trip at the store it last delivered to, so its own position would keep it
            // on that store after a nearer one is built.
            let source = coarse(ctx.state.entities.get(c).expect("candidate"));
            let hall = work::stores(ctx, owner, item.as_str(), 0, source)
                .first()
                .copied();
            if start_harvest(ctx, id, c, &item, hall) {
                if let Some(p) = previous {
                    *counts.entry(p).or_insert(0) -= 1;
                }
                *counts.entry(c).or_insert(0) += 1;
                if let Some(u) = ctx.state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
                    u.order = Some(Order::Gather { target: c });
                }
                started = true;
                break;
            }
        }
        if !started {
            let u = ctx
                .state
                .entities
                .get_mut(id)
                .and_then(|e| e.unit.as_mut())
                .expect("unit");
            if candidates.is_empty() && !u.order_queue.is_empty() {
                u.order = None;
            } else {
                u.retry_at = tick + RETRY_TICKS;
            }
        }
    }
}

/// Sends a worker to harvest one load from `resource` for `hall` (old `startHarvest`).
/// Waiting workers reserve nothing at the source; the load is claimed at the store.
fn start_harvest(
    ctx: &mut Ctx,
    worker: EntityId,
    resource: EntityId,
    item: &Id,
    hall: Option<EntityId>,
) -> bool {
    let Some(hall) = hall else {
        return false;
    };
    let (Some(w), Some(r), Some(h)) = (
        ctx.state.entities.get(worker),
        ctx.state.entities.get(resource),
        ctx.state.entities.get(hall),
    ) else {
        return false;
    };
    if !can_assign_gather(ctx.catalog, w, r) || !open_to(ctx, w.owner, r) {
        return false;
    }
    let Some(recipe) = recipe(ctx.catalog, item.as_str()) else {
        return false;
    };
    let carry = ctx
        .catalog
        .get(w.definition)
        .behaviors
        .work
        .as_ref()
        .map_or(0, |work| work.carry_capacity);
    let amount = recipe.amount.min(carry).min(stock::room(
        ctx.catalog,
        &ctx.state.economy.jobs,
        h,
        Some(item.as_str()),
    ));
    if amount == 0 || sites::work_point(ctx, resource, worker).is_none() {
        return false;
    }
    jobs::create(
        ctx,
        JobKind::Harvest,
        worker,
        hall,
        Spec {
            source: Some(resource),
            item: Some(item.clone()),
            amount,
            ..Spec::default()
        },
    );
    true
}

/// Arrived harvesters queue at their source and start in arrival order while it has room
/// (old `admitHarvesters`). The arrival order is kept on the job itself.
pub fn admit_harvesters(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    let mut working: BTreeMap<EntityId, u32> = BTreeMap::new();
    let mut waiting: Vec<(u32, u32, EntityId)> = Vec::new();
    for id in ctx.state.economy.jobs.ids() {
        let job = ctx.state.economy.job(id).expect("job").clone();
        if job.kind != JobKind::Harvest {
            continue;
        }
        let Some(source) = job.source else {
            continue;
        };
        let worker_owner = ctx
            .state
            .entities
            .get(job.worker)
            .filter(|w| w.unit.is_some() && alive(w))
            .map(|w| w.owner);
        let valid = worker_owner.is_some()
            && ctx.state.entities.get(job.target).is_some_and(alive)
            && ctx.state.entities.get(source).is_some_and(|s| {
                alive(s)
                    && s.resource.as_ref().is_some_and(|r| r.amount > 0)
                    && worker_owner.is_some_and(|o| open_to(ctx, o, s))
            });
        if !valid {
            continue;
        }
        let mut phase = job.phase;
        let worker = ctx.state.entities.get(job.worker).expect("worker");
        let (taunted, held) = (super::taunted(worker), super::held(ctx.catalog, worker));
        if phase == Phase::Walk && !taunted && work::arrived(ctx, job.worker) {
            let job = ctx.state.economy.job_mut(id).expect("job");
            job.phase = Phase::Wait;
            job.arrived_tick = Some(tick);
            phase = Phase::Wait;
        }
        if phase == Phase::Work {
            *working.entry(source).or_default() += 1;
        } else if phase == Phase::Wait && !held {
            let arrived = ctx
                .state
                .economy
                .job(id)
                .and_then(|j| j.arrived_tick)
                .unwrap_or(0);
            waiting.push((arrived, id, source));
        }
    }
    waiting.sort_unstable();
    for (_, id, source) in waiting {
        let used = working.get(&source).copied().unwrap_or(0);
        let limit = ctx
            .state
            .entities
            .get(source)
            .map_or(1, |s| active_workers(ctx.catalog, s) as u32);
        if used >= limit {
            continue;
        }
        if let Some(job) = ctx.state.economy.job_mut(id) {
            job.phase = Phase::Work;
        }
        working.insert(source, used + 1);
    }
}

/// Felling speed from research (old `treeWorkRate`): the best rate among the owner's
/// completed research for this worker, at least 1.
fn tree_work_rate(ctx: &Ctx, worker: &Entity) -> u32 {
    let id = ctx.catalog.id(worker.definition);
    let rules = &ctx.catalog.content.rules;
    ctx.state
        .economy
        .researched(worker.owner)
        .iter()
        .filter_map(|r| rules.research.get(r.as_str()))
        .flat_map(|r| &r.effects)
        .filter(|e| e.units.contains(id))
        .map(|e| e.tree_work_rate.unwrap_or(1))
        .fold(1, u32::max)
}

/// One tick of a harvest job (old `advanceHarvest`).
pub fn advance_harvest(ctx: &mut Ctx, id: u32) {
    let tick = ctx.state.tick;
    let job = ctx.state.economy.job(id).expect("job").clone();
    let (worker, hall) = (job.worker, job.target);
    let item = job.item.clone().expect("harvest item");
    let Some(recipe) = recipe(ctx.catalog, item.as_str()) else {
        abandon(ctx, id);
        return;
    };
    match job.phase {
        Phase::Return => {
            let w = ctx.state.entities.get(worker).expect("worker");
            let h = ctx.state.entities.get(hall).expect("hall");
            let cargo = w.unit.as_ref().and_then(|u| u.work.cargo.clone());
            let fits = cargo.as_ref().is_some_and(|c| {
                stock::accepts(ctx.catalog, h, c.item.as_str())
                    && stock::room(
                        ctx.catalog,
                        &ctx.state.economy.jobs,
                        h,
                        Some(c.item.as_str()),
                    )
                    .saturating_add(job.amount)
                        >= c.amount
            });
            if fits {
                deposit(ctx, worker, hall);
                finish_harvest(ctx, id, worker);
            } else {
                abandon(ctx, id);
            }
            return;
        }
        Phase::Fall => {
            let source = job.source.and_then(|s| ctx.state.entities.get(s));
            let fallen = source.and_then(|s| {
                let at = s.resource.as_ref()?.felling.as_ref()?.fall_tick?;
                Some((
                    at,
                    ctx.catalog.get(s.definition).felling.as_ref()?.fall_ticks,
                ))
            });
            let Some((fallen_at, fall_ticks)) = fallen else {
                abandon(ctx, id);
                return;
            };
            if tick - fallen_at < fall_ticks {
                return;
            }
            ctx.state.economy.job_mut(id).expect("job").phase = Phase::Return;
            if sites::work_point(ctx, hall, worker).is_none() {
                abandon(ctx, id);
            }
            return;
        }
        _ => {}
    }
    let Some(source) = job.source else {
        abandon(ctx, id);
        return;
    };
    let owner = ctx.state.entities.get(worker).map(|w| w.owner);
    let standing = ctx.state.entities.get(source).is_some_and(|s| {
        alive(s)
            && s.resource.as_ref().is_some_and(|r| r.amount > 0)
            && owner.is_some_and(|o| open_to(ctx, o, s))
    });
    if !standing {
        abandon(ctx, id);
        return;
    }
    if job.phase != Phase::Work {
        return;
    }
    let w = ctx.state.entities.get(worker).expect("worker");
    let s = ctx.state.entities.get(source).expect("source");
    let felled = s.resource.as_ref().is_some_and(|r| r.felling.is_some());
    let speed = if felled { tree_work_rate(ctx, w) } else { 1 };
    let progress = job.progress + speed;
    ctx.state.economy.job_mut(id).expect("job").progress = progress;
    if felled
        && let Some(impact) = recipe.impact_tick
        && recipe.cycle > 0
    {
        let phase = progress % recipe.cycle;
        if phase < speed + impact && phase >= impact {
            let felling = felling_mut(ctx, source);
            felling.last_hit_tick = Some(tick);
        }
    }
    if progress < recipe.work_ticks {
        return;
    }
    ctx.state.economy.job_mut(id).expect("job").progress = 0;
    let carried = ctx
        .state
        .entities
        .get(worker)
        .and_then(|w| w.unit.as_ref())
        .and_then(|u| u.work.cargo.as_ref())
        .map_or(0, |c| c.amount);
    let left = ctx
        .state
        .entities
        .get(source)
        .and_then(|s| s.resource.as_ref())
        .map_or(0, |r| r.amount);
    let endless = endless(ctx.catalog, ctx.state.entities.get(source).expect("source"));
    let amount = recipe
        .amount
        .min(job.amount.saturating_sub(carried))
        .min(if endless { u32::MAX } else { left });
    let remaining = if endless { left } else { left - amount };
    let s = ctx.state.entities.get_mut(source).expect("source");
    s.resource.as_mut().expect("resource").amount = remaining;
    let owner = {
        let w = ctx.state.entities.get_mut(worker).expect("worker");
        let u = w.unit.as_mut().expect("unit");
        let total = carried + amount;
        u.work.cargo = Some(super::Cargo {
            item: item.clone(),
            amount: total,
        });
        w.owner
    };
    stock::record(
        ctx.state,
        owner,
        FactKind::Produced,
        "Harvested",
        Some((&item, amount)),
    );
    if felled {
        let def = ctx
            .catalog
            .get(ctx.state.entities.get(source).expect("source").definition);
        let max_hp = def.felling.as_ref().map_or(0, |f| f.max_hp);
        let full = def.resource_yield.unwrap_or(1).max(1);
        let hp = (u64::from(max_hp) * u64::from(remaining)).div_ceil(u64::from(full)) as u32;
        let worker_at = precise(ctx.state.entities.get(worker).expect("worker"));
        let source_at = precise(ctx.state.entities.get(source).expect("source"));
        let felling = felling_mut(ctx, source);
        felling.hp = max_hp.min(hp);
        if remaining == 0 {
            felling.last_hit_tick = Some(tick);
            felling.fall_tick = Some(tick);
            felling.direction = crate::entity::Milli {
                x: (source_at.0 - worker_at.0) as i32,
                y: (source_at.1 - worker_at.1) as i32,
            };
        }
    }
    if remaining == 0 {
        ctx.spatial.sync_entity(ctx.state, source);
    }
    let cargo = carried + amount;
    if cargo >= job.amount || remaining == 0 {
        ctx.state.economy.jobs.set_amount(id, cargo);
        let phase = if felled && remaining == 0 {
            Phase::Fall
        } else {
            Phase::Return
        };
        ctx.state.economy.job_mut(id).expect("job").phase = phase;
        if phase == Phase::Return && sites::work_point(ctx, hall, worker).is_none() {
            abandon(ctx, id);
        }
    }
}

fn felling_mut<'s>(ctx: &'s mut Ctx, source: EntityId) -> &'s mut Felling {
    ctx.state
        .entities
        .get_mut(source)
        .and_then(|s| s.resource.as_mut())
        .and_then(|r| r.felling.as_mut())
        .expect("felling")
}

/// Goods delivered: the job ends, an employing workplace finishes its cycle, a pending move
/// starts, and a gather order with orders queued behind it ends (old `finishHarvest`).
fn finish_harvest(ctx: &mut Ctx, id: u32, worker: EntityId) {
    let workplace = ctx
        .state
        .entities
        .get(worker)
        .and_then(|w| w.unit.as_ref())
        .and_then(|u| u.work.employment);
    jobs::erase(ctx, id);
    if let Some(b) = workplace {
        let staffed = ctx
            .state
            .entities
            .get(b)
            .and_then(|b| b.production.as_ref())
            .and_then(|p| p.active.as_ref())
            .is_some_and(|a| a.worker == Some(worker));
        if staffed {
            production::finish_cycle(ctx, b);
        }
    }
    apply_pending(ctx, worker);
    if let Some(u) = ctx
        .state
        .entities
        .get_mut(worker)
        .and_then(|w| w.unit.as_mut())
        && matches!(u.order, Some(Order::Gather { .. }))
        && !u.order_queue.is_empty()
    {
        u.order = None;
    }
}

/// Step 9: planted resources whose time has come regrow when nothing blocks them (old
/// `advanceRegrowth`).
pub fn advance_regrowth(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    let due: Vec<EntityId> = ctx
        .index
        .regrowing
        .iter()
        .copied()
        .filter(|&id| {
            ctx.state.entities.get(id).is_some_and(|e| {
                alive(e)
                    && e.resource
                        .as_ref()
                        .and_then(|r| r.growing_until)
                        .is_some_and(|until| until <= tick)
            })
        })
        .collect();
    for id in due {
        if !can_regrow(ctx, id) {
            continue;
        }
        let e = ctx.state.entities.get(id).expect("resource");
        let def: &Definition = ctx.catalog.get(e.definition);
        let full = def.resource_yield.unwrap_or(0);
        let felling = def.felling.as_ref().map(|f| Felling::standing(f.max_hp));
        let r = ctx
            .state
            .entities
            .get_mut(id)
            .and_then(|e| e.resource.as_mut())
            .expect("resource");
        r.amount = full;
        r.growing_until = None;
        if felling.is_some() {
            r.felling = felling;
        }
        ctx.index.regrowing.remove(&id);
        ctx.spatial.sync_entity(ctx.state, id);
    }
}

/// Starts a resource's regrowth timer (old `setRegrowth`).
pub fn set_regrowth(ctx: &mut Ctx, id: EntityId, until: Option<u32>) {
    if let Some(r) = ctx
        .state
        .entities
        .get_mut(id)
        .and_then(|e| e.resource.as_mut())
    {
        r.growing_until = until;
        if until.is_some() {
            ctx.index.regrowing.insert(id);
        } else {
            ctx.index.regrowing.remove(&id);
        }
    }
}

/// Whether a regrown resource can reclaim its collision disc (old `canRegrow`): no
/// building on its footprint, no doorway inside the disc, no body in it. Idle bodies in
/// the way are walked out.
fn can_regrow(ctx: &mut Ctx, id: EntityId) -> bool {
    let tick = ctx.state.tick;
    let r = ctx.state.entities.get(id).expect("resource");
    let size = ctx.spatial.size() as i64;
    let mut cells = ctx.spatial.collision_cells(r);
    cells.sort_unstable();
    let footprint = crate::terrain::footprint::footprint_cells(
        f64::from(r.position.x) / 1000.0,
        f64::from(r.position.y) / 1000.0,
        crate::terrain::footprint::Extent::of(ctx.catalog.get(r.definition).footprint.as_ref()),
        f64::from(r.rotation) / 1000.0,
    );
    let under_building = footprint.cells().any(|(x, y)| {
        x >= 0 && y >= 0 && x < size && y < size && ctx.spatial.building_at((y * size + x) as u32)
    });
    if under_building {
        return false;
    }
    let contains = |cells: &[u32], x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < size
            && y < size
            && cells.binary_search(&((y * size + x) as u32)).is_ok()
    };
    for &b in &ctx.index.buildings {
        let Some(building) = ctx.state.entities.get(b) else {
            continue;
        };
        if !alive(building) {
            continue;
        }
        let (x, y) = sites::entrance_of(ctx.catalog, building).cell();
        if contains(&cells, i64::from(x), i64::from(y)) {
            return false;
        }
    }
    let (rx, ry) = precise(r);
    let reach = cells
        .iter()
        .map(|&c| {
            let (qx, qy) = (i64::from(c) % size * 1000, i64::from(c) / size * 1000);
            (qx - rx).abs().max((qy - ry).abs())
        })
        .max()
        .unwrap_or(0);
    let mut inside = Vec::new();
    for &u in &ctx.index.units {
        let Some(e) = ctx.state.entities.get(u) else {
            continue;
        };
        let Some(unit) = e.unit.as_ref() else {
            continue;
        };
        if !alive(e) || unit.contained.is_some() || unit.release.is_some() {
            continue;
        }
        let (cx, cy) = e.position.cell();
        if contains(&cells, i64::from(cx), i64::from(cy)) {
            inside.push(u);
            continue;
        }
        let (px, py) = precise(e);
        let clearance = 500 + ctx.spatial.profiles.get(e.definition).body.radius_milli() as i64;
        if (px - rx).abs() > reach + clearance || (py - ry).abs() > reach + clearance {
            continue;
        }
        let near = cells.iter().any(|&c| {
            let (qx, qy) = (i64::from(c) % size * 1000, i64::from(c) / size * 1000);
            (px - qx).abs() <= clearance && (py - qy).abs() <= clearance
        });
        if near {
            inside.push(u);
        }
    }
    for &u in &inside {
        let e = ctx.state.entities.get(u).expect("unit");
        let unit = e.unit.as_ref().expect("unit");
        if unit.order.is_some()
            || unit.work.job.is_some()
            || !unit.route.is_empty()
            || unit.retry_at > tick
        {
            continue;
        }
        let exit = regrowth_exit(ctx, id, reach, &cells, u);
        let routed = exit.is_some_and(|spot| sites::route(ctx, u, spot));
        if !routed && let Some(unit) = ctx.state.entities.get_mut(u).and_then(|e| e.unit.as_mut()) {
            unit.retry_at = tick + 20;
        }
    }
    inside.is_empty()
}

/// The nearest free cell just outside the disc's bounding ring (old `regrowthExit`).
fn regrowth_exit(
    ctx: &Ctx,
    resource: EntityId,
    reach: i64,
    cells: &[u32],
    unit: EntityId,
) -> Option<Spot> {
    let r = ctx.state.entities.get(resource)?;
    let e = ctx.state.entities.get(unit)?;
    let origin = precise(e);
    let (rx, ry) = r.position.cell();
    let size = ctx.spatial.size() as i32;
    let radius = ctx.spatial.profiles.get(e.definition).body.radius;
    let first = ((reach as f64 / 1000.0) + 0.5 + radius).ceil() as i32 + 1;
    for ring in first..=first + 3 {
        let mut around = Vec::new();
        for dy in -ring..=ring {
            let mut dx = -ring;
            while dx <= ring {
                around.push((rx + dx, ry + dy));
                dx += if dy.abs() == ring { 1 } else { 2 * ring };
            }
        }
        around.sort_by_key(|&(x, y)| {
            (
                distance2((i64::from(x) * 1000, i64::from(y) * 1000), origin),
                y,
                x,
            )
        });
        for (x, y) in around {
            if x < 0 || y < 0 || x >= size || y >= size {
                continue;
            }
            if cells.binary_search(&((y * size + x) as u32)).is_ok() {
                continue;
            }
            let spot = Spot::new(x, y, None);
            if ctx.spatial.free(spot, Some(unit), None, None) {
                return Some(spot);
            }
        }
    }
    None
}

/// Sends the starting workers to gather near their main fort, spread over the nearest
/// sources (old `startGathering`). `starting` lists the units each player started with:
/// only they are sent.
pub fn start_gathering(ctx: &mut Ctx, setup: &MatchSetup, starting: &BTreeSet<EntityId>) {
    let objectives: Vec<(Owner, u32)> =
        ctx.state.objectives.iter().map(|(&o, &f)| (o, f)).collect();
    for (owner, fort) in objectives {
        let Some(hall) = ctx.state.entities.get(EntityId(fort)) else {
            continue;
        };
        let hall_at = coarse(hall);
        let workers: Vec<EntityId> = work::workers(ctx, owner, true)
            .into_iter()
            .filter(|w| starting.contains(w))
            .collect();
        let mut used = BTreeSet::new();
        let race = match owner {
            Owner::Player(p) => setup
                .slots
                .iter()
                .find(|s| s.player == p)
                .and_then(|s| s.race.clone()),
            Owner::Nobody => None,
        };
        let rules = &ctx.catalog.content.rules;
        let key = race.unwrap_or_else(|| rules.default_race.clone());
        let tasks = rules
            .races
            .get(&key)
            .and_then(|r| r.starting_setup.as_ref())
            .unwrap_or(&rules.starting_setup)
            .gathering
            .clone()
            .unwrap_or_default();
        for task in tasks {
            let Some(Creation::Harvest { source, .. }) = ctx
                .catalog
                .content
                .definition(task.item.as_str())
                .and_then(|d| d.creation.as_ref())
            else {
                continue;
            };
            let Some(source) = ctx.catalog.find(source.as_str()) else {
                continue;
            };
            let mut resources = standing_near(ctx, hall_at, i64::from(task.radius), source);
            resources.sort_by_key(|&r| {
                let e = ctx.state.entities.get(r).expect("resource");
                (distance2(coarse(e), hall_at), r)
            });
            if resources.is_empty() {
                continue;
            }
            let selected: Vec<EntityId> = workers
                .iter()
                .copied()
                .filter(|w| {
                    !used.contains(w)
                        && resources.iter().any(|&r| {
                            can_assign_gather(
                                ctx.catalog,
                                ctx.state.entities.get(*w).expect("worker"),
                                ctx.state.entities.get(r).expect("resource"),
                            )
                        })
                })
                .take(task.workers as usize)
                .collect();
            let mut assigned = counts(&assignments(ctx));
            for w in selected {
                let worker = ctx.state.entities.get(w).expect("worker");
                let pick = resources
                    .iter()
                    .copied()
                    .filter(|&r| {
                        can_assign_gather(
                            ctx.catalog,
                            worker,
                            ctx.state.entities.get(r).expect("resource"),
                        )
                    })
                    .min_by_key(|&r| {
                        let e = ctx.state.entities.get(r).expect("resource");
                        (
                            assigned.get(&r).copied().unwrap_or(0),
                            distance2(coarse(e), hall_at),
                            r,
                        )
                    });
                let Some(resource) = pick else {
                    continue;
                };
                if let Some(u) = ctx.state.entities.get_mut(w).and_then(|e| e.unit.as_mut()) {
                    u.order = Some(Order::Gather { target: resource });
                }
                *assigned.entry(resource).or_insert(0) += 1;
                used.insert(w);
            }
        }
    }
}
