//! What the HUD and renderer see of the economy (old `Observation.describeActor` work and
//! control fields, `projectColony`, `summarizeGoods`, `baseGathering`).

use super::{
    Cargo, Economy, Index, JobKind, Phase, Production, ResearchQueue, RevivalQueue, Status,
    UpgradeTask, alive, coarse, distance2, stock, supply,
};
use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId, Milli, Place},
    state::State,
};
use canopy_content::{
    Id,
    game::{Creation, Kind, WorkAnimation},
    placement::Owner,
};
use std::collections::BTreeMap;

/// A worker's work animation: what it does, where it faces, and for felling the strike
/// cycle (`ticks` long, `progress` into it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkPose {
    pub animation: WorkAnimation,
    pub target: Milli,
    pub cycle: Option<(u32, u32)>,
}

/// Workers at a harvest site or a base, against the recommended number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Workforce {
    pub workers: u32,
    pub recommended: u32,
}

/// Economy state anyone who sees the entity sees.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PublicWork {
    pub cargo: Option<Cargo>,
    pub pose: Option<WorkPose>,
    /// Workers assigned to a harvest building (a mine).
    pub site: Option<Workforce>,
    /// A building is at work: training, researching, upgrading, reviving, earning its
    /// income or gathered from (workers assigned to it as a harvest site), and not paused
    /// or blocked. Everyone who sees it sees that it works, never at what (its animation
    /// and effects, D65).
    pub busy: bool,
    /// Training cycles the building has finished: a unit stepped out when it grows.
    pub cycles: u32,
    /// A complete building that needs power stands outside its owner's network: its work
    /// pauses (D66). Anyone who sees it sees that it is dark.
    pub unpowered: bool,
}

/// A worker's job as its owner's command card sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobView {
    pub kind: JobKind,
    pub target: EntityId,
    pub source: Option<EntityId>,
    pub item: Option<Id>,
    pub phase: Phase,
}

/// A queue head's progress against its length in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u32,
    pub total: u32,
}

/// Economy state only the owner sees.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrivateWork {
    pub job: Option<JobView>,
    pub employment: Option<EntityId>,
    pub pending_move: Option<Place>,
    /// The old status line: "Available", "harvest", "Delivering before move", …
    pub label: String,
    pub production: Option<Production>,
    /// The workplace status line.
    pub status: Option<String>,
    /// Training or planting progress.
    pub training: Option<Progress>,
    pub construction: Option<Progress>,
    pub research: Option<ResearchQueue>,
    pub research_progress: Option<Progress>,
    pub upgrade: Option<UpgradeTask>,
    pub upgrade_progress: Option<Progress>,
    pub revival: Option<RevivalQueue>,
    pub revival_progress: Option<Progress>,
    /// Workers gathering for this base.
    pub base: Option<Workforce>,
}

/// One good in the owner's ledger (old `GoodsSummary`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goods {
    pub item: Id,
    pub stored: u32,
    pub reserved: u32,
    pub in_transit: u32,
    pub available: u32,
}

/// The owner's colony: goods, supply, workers and research.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colony {
    pub goods: Vec<Goods>,
    pub supply: supply::Supply,
    pub population: supply::Population,
    pub research: Vec<Id>,
}

/// Per-view work data computed once: gathering assignments and base workforces.
pub struct WorkViews {
    sites: BTreeMap<EntityId, u32>,
    bases: BTreeMap<EntityId, Workforce>,
    unpowered: std::collections::BTreeSet<EntityId>,
    /// Buildings placed on a resource (`placementOn`): the workforce of the resource
    /// under them.
    tapped: BTreeMap<EntityId, Workforce>,
}

/// Every worker's source (old `gatherAssignments`), from state alone.
fn assignments(state: &State, index: &Index) -> BTreeMap<EntityId, EntityId> {
    let mut out = BTreeMap::new();
    for e in index.units.iter().filter_map(|&id| state.entities.get(id)) {
        if !alive(e) {
            continue;
        }
        let Some(u) = e.unit.as_ref() else {
            continue;
        };
        let job = u
            .work
            .job
            .and_then(|j| state.economy.job(j))
            .filter(|j| j.kind == JobKind::Harvest)
            .and_then(|j| j.source);
        let order = match &u.order {
            Some(crate::orders::Order::Gather { target }) => Some(*target),
            _ => None,
        };
        if let Some(source) = job.or(order) {
            out.insert(e.id, source);
        }
    }
    out
}

impl WorkViews {
    pub fn new(state: &State, catalog: &Catalog, economy: &Economy) -> Self {
        let index = &economy.index;
        let assignments = assignments(state, index);
        let mut sites = BTreeMap::new();
        for &source in assignments.values() {
            *sites.entry(source).or_default() += 1;
        }
        let bases = base_gathering(state, catalog, index, &assignments);
        let mut tapped = BTreeMap::new();
        for b in index
            .buildings
            .iter()
            .filter_map(|&id| state.entities.get(id))
        {
            let Some(on) = catalog.get(b.definition).placement_on.as_ref() else {
                continue;
            };
            let (x, y) = (i64::from(b.position.x), i64::from(b.position.y));
            let under = index.resources.near(x, y, 1000).into_iter().find(|&r| {
                state.entities.get(r).is_some_and(|r| {
                    r.position == b.position && catalog.id(r.definition) == &on.source
                })
            });
            if let Some(r) = under {
                let recommended = catalog
                    .get(state.entities.get(r).expect("resource").definition)
                    .harvesting
                    .as_ref()
                    .map_or(0, |h| h.recommended_workers);
                tapped.insert(
                    b.id,
                    Workforce {
                        workers: sites.get(&r).copied().unwrap_or(0),
                        recommended,
                    },
                );
            }
        }
        Self {
            sites,
            bases,
            tapped,
            unpowered: economy.network.unpowered_buildings().clone(),
        }
    }
}

/// Each resource belongs to the nearest completed compatible base of each owner; bases
/// count the workers assigned to their resources and the recommended number (old
/// `baseGathering`).
fn base_gathering(
    state: &State,
    catalog: &Catalog,
    index: &Index,
    assignments: &BTreeMap<EntityId, EntityId>,
) -> BTreeMap<EntityId, Workforce> {
    let bases: Vec<&Entity> = index
        .buildings
        .iter()
        .filter_map(|&id| state.entities.get(id))
        .filter(|e| {
            e.owner != Owner::Nobody
                && alive(e)
                && e.construction.is_none()
                && catalog
                    .get(e.definition)
                    .behaviors
                    .storage
                    .as_ref()
                    .is_some_and(|s| s.gathering.is_some())
        })
        .collect();
    let mut result: BTreeMap<EntityId, Workforce> =
        bases.iter().map(|b| (b.id, Workforce::default())).collect();
    if bases.is_empty() {
        return result;
    }
    let mut sites: BTreeMap<EntityId, BTreeMap<Owner, EntityId>> = BTreeMap::new();
    // Only staffed sources (buildings with a harvest policy) belong to bases.
    for &id in &index.buildings {
        let Some(source) = state.entities.get(id) else {
            continue;
        };
        let d = catalog.get(source.definition);
        let (Some(resource), Some(policy)) = (&source.resource, &d.harvesting) else {
            continue;
        };
        if !alive(source) {
            continue;
        }
        let mut owners: BTreeMap<Owner, (i64, EntityId)> = BTreeMap::new();
        for base in &bases {
            let rule = catalog
                .get(base.definition)
                .behaviors
                .storage
                .as_ref()
                .and_then(|s| s.gathering.as_ref())
                .expect("gathering");
            let harvests_here = matches!(
                catalog.content.definition(rule.item.as_str()).and_then(|i| i.creation.as_ref()),
                Some(Creation::Harvest { source: s, .. }) if s == &d.id
            );
            if !harvests_here {
                continue;
            }
            let d2 = distance2(coarse(base), coarse(source));
            if (d2 as f64) > (rule.radius * 1000.0).powi(2) {
                continue;
            }
            let candidate = (d2, base.id);
            owners
                .entry(base.owner)
                .and_modify(|best| {
                    if candidate < *best {
                        *best = candidate;
                    }
                })
                .or_insert(candidate);
        }
        if resource.amount > 0 {
            for &(_, base) in owners.values() {
                result.entry(base).or_default().recommended += policy.recommended_workers;
            }
        }
        sites.insert(
            source.id,
            owners.into_iter().map(|(o, (_, b))| (o, b)).collect(),
        );
    }
    for (&worker, &source) in assignments {
        let owner = state.entities.get(worker).map(|w| w.owner);
        let base = owner.and_then(|o| sites.get(&source)?.get(&o).copied());
        if let Some(base) = base {
            result.entry(base).or_default().workers += 1;
        }
    }
    result
}

/// The work animation of a unit at its work site (old `describeActor` work pose). `tick`
/// is the latest simulated tick.
fn pose(
    state: &State,
    catalog: &Catalog,
    e: &Entity,
    tick: u32,
    at_goal: bool,
) -> Option<WorkPose> {
    let u = e.unit.as_ref()?;
    if !u.route.is_empty() || u.contained.is_some() || u.release.is_some() || !at_goal {
        return None;
    }
    let job = state.economy.job(u.work.job?)?;
    let workplace = state.entities.get(job.target);
    let creation = match job.kind {
        JobKind::Construct | JobKind::Repair => {
            workplace.and_then(|w| catalog.get(w.definition).creation.as_ref())
        }
        JobKind::Harvest if matches!(job.phase, Phase::Work | Phase::Fall) => job
            .item
            .as_ref()
            .and_then(|i| catalog.content.definition(i.as_str())?.creation.as_ref()),
        _ => None,
    }?;
    let target = if job.kind == JobKind::Harvest {
        job.source.and_then(|s| state.entities.get(s))
    } else {
        workplace
    }?;
    let (animation, strike, cycle) = match creation {
        Creation::Construct {
            work_animation,
            work_ticks,
            ..
        } => (*work_animation, None, *work_ticks),
        Creation::Harvest {
            work_animation,
            impact_tick,
            animation_ticks,
            work_ticks,
            ..
        } => (
            *work_animation,
            *impact_tick,
            animation_ticks.unwrap_or(*work_ticks),
        ),
        _ => return None,
    };
    let animation = animation?;
    let fall = target
        .resource
        .as_ref()
        .and_then(|r| r.felling.as_ref())
        .and_then(|f| f.fall_tick);
    let progress = match (job.phase, fall, strike) {
        (Phase::Fall, Some(fell), Some(impact)) => impact + tick.saturating_sub(fell),
        _ => job.progress % cycle.max(1),
    };
    if strike.is_some() && progress >= cycle {
        return None;
    }
    Some(WorkPose {
        animation,
        target: target.position,
        cycle: strike.map(|_| (cycle, progress)),
    })
}

/// The public economy state of a seen entity.
pub fn public(
    state: &State,
    catalog: &Catalog,
    views: &WorkViews,
    e: &Entity,
    at_goal: bool,
) -> Option<PublicWork> {
    let tick = state.tick.saturating_sub(1);
    let d = catalog.get(e.definition);
    let site = (d.kind == Kind::Building && d.harvesting.is_some())
        .then(|| Workforce {
            workers: views.sites.get(&e.id).copied().unwrap_or(0),
            recommended: d.harvesting.as_ref().map_or(0, |h| h.recommended_workers),
        })
        .or_else(|| views.tapped.get(&e.id).copied());
    let cargo = e.unit.as_ref().and_then(|u| u.work.cargo.clone());
    let pose = pose(state, catalog, e, tick, at_goal);
    let producing = e.production.as_ref().is_some_and(|p| {
        !p.paused
            && p.active.is_some()
            && matches!(
                p.status,
                Status::Training | Status::Planting | Status::Upgrading | Status::RevivingHero
            )
    });
    let unpowered = views.unpowered.contains(&e.id);
    // A harvest building works while workers gather from it (a well pumps, D65).
    let gathered = site.is_some_and(|s| s.workers > 0);
    // A building with income works for as long as it earns (D62: a well pumps by itself).
    let earning = d.income.is_some() && e.owner != Owner::Nobody && alive(e);
    let busy = e.construction.is_none()
        && !unpowered
        && (producing
            || gathered
            || earning
            || e.research.as_ref().is_some_and(|r| !r.queue.is_empty())
            || e.upgrade.is_some()
            || e.revival.as_ref().is_some_and(|r| !r.queue.is_empty()));
    let cycles = e.production.as_ref().map_or(0, |p| p.produced);
    (cargo.is_some() || pose.is_some() || site.is_some() || busy || cycles > 0 || unpowered)
        .then_some(PublicWork {
            cargo,
            pose,
            site,
            busy,
            cycles,
            unpowered,
        })
}

fn progress(done: u32, total: u32) -> Progress {
    Progress { done, total }
}

/// The owner-only economy state of an entity.
pub fn private(
    state: &State,
    catalog: &Catalog,
    economy: &Economy,
    views: &WorkViews,
    e: &Entity,
) -> PrivateWork {
    let d = catalog.get(e.definition);
    let mut out = PrivateWork {
        base: views.bases.get(&e.id).copied(),
        ..PrivateWork::default()
    };
    if let Some(u) = e.unit.as_ref() {
        let job = u.work.job.and_then(|j| state.economy.job(j));
        out.job = job.map(|j| JobView {
            kind: j.kind,
            target: j.target,
            source: j.source,
            item: j.item.clone(),
            phase: j.phase,
        });
        out.employment = u.work.employment;
        out.pending_move = u.work.pending_move.clone();
        let label = u
            .work
            .employment
            .and_then(|w| state.entities.get(w))
            .and_then(|w| {
                catalog
                    .get(w.definition)
                    .behaviors
                    .production
                    .as_ref()?
                    .job_name
                    .clone()
            });
        out.label = if u.work.pending_move.is_some() {
            "Delivering before move".into()
        } else if u.release.is_some() {
            "Waiting for release".into()
        } else if u.contained.is_some() {
            "Training".into()
        } else if u.work.cargo.is_some() && job.is_none() {
            "Cargo blocked".into()
        } else if let Some(job) = job {
            match &label {
                Some(label) => format!("{label} · {}", job.kind.name()),
                None => job.kind.name().into(),
            }
        } else if let Some(label) = label {
            format!("{label} · waiting")
        } else if u.order.is_some() {
            "Following order".into()
        } else {
            "Available".into()
        };
    } else {
        out.label = "Available".into();
    }
    if let Some(c) = &e.construction {
        let total = match &d.creation {
            Some(Creation::Construct { work_ticks, .. }) => *work_ticks,
            _ => 0,
        };
        out.construction = Some(progress(c.progress, total));
    }
    if let Some(p) = &e.production {
        let hint = economy.hint(e.owner);
        out.status = Some(match p.status {
            Status::SupplyBlocked => Status::SupplyBlocked.label(hint),
            status => status.label(None),
        });
        out.training = p.active.as_ref().map(|a| {
            let total = match &catalog.get(a.definition).creation {
                Some(Creation::Train { work_ticks, .. } | Creation::Plant { work_ticks, .. }) => {
                    *work_ticks
                }
                _ => 0,
            };
            progress(a.progress, total)
        });
        out.production = Some((**p).clone());
    }
    if let Some(r) = &e.research {
        out.research_progress = r.queue.first().map(|q| {
            let total = catalog
                .content
                .rules
                .research
                .get(q.id.as_str())
                .map_or(0, |r| r.work_ticks);
            progress(q.progress, total)
        });
        out.research = Some(r.clone());
    }
    if let Some(u) = &e.upgrade {
        let total = d.upgrade.as_ref().map_or(0, |u| u.work_ticks);
        out.upgrade_progress = Some(progress(u.progress, total));
        out.upgrade = Some(u.clone());
    }
    if let Some(r) = &e.revival {
        out.revival_progress = r
            .queue
            .first()
            .zip(d.behaviors.revival.as_ref())
            .map(|(q, policy)| progress(q.progress, super::revival::terms(policy, q.level).1));
        out.revival = Some(r.clone());
    }
    out
}

/// The owner's colony ledger (old `projectColony`, `summarizeGoods`).
pub fn colony(state: &State, catalog: &Catalog, economy: &Economy, owner: Owner) -> Colony {
    let index = &economy.index;
    let wallet = state.wallets.get(&owner);
    let owned: Vec<&Entity> = index
        .units
        .iter()
        .chain(&index.buildings)
        .filter_map(|&id| state.entities.get(id))
        .filter(|e| e.owner == owner)
        .collect();
    let mut currencies: Vec<&Id> = catalog
        .content
        .definitions
        .iter()
        .flat_map(|d| d.behaviors.storage.iter())
        .filter(|s| s.dropoff == Some(true))
        .flat_map(|s| &s.accepts)
        .collect();
    currencies.sort();
    currencies.dedup();
    let mut items: Vec<&canopy_content::game::Definition> = catalog
        .content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Item && currencies.binary_search(&&d.id).is_ok())
        .collect();
    items.sort_by_key(|d| d.display_order.unwrap_or(1000));
    let goods = items
        .into_iter()
        .map(|item| {
            let id = item.id.as_str();
            let funds = wallet.map_or(0, |w| stock::quantity(w, id));
            let mut stored = funds;
            let mut in_transit = 0;
            let mut free = funds;
            for e in &owned {
                stored += stock::quantity(&e.inventory, id);
                stored += paid_tasks(catalog, e)
                    .iter()
                    .filter(|(i, _)| i.as_str() == id)
                    .map(|(_, n)| n)
                    .sum::<u32>();
                if let Some(c) = e.unit.as_ref().and_then(|u| u.work.cargo.as_ref())
                    && c.item.as_str() == id
                {
                    in_transit += c.amount;
                }
                free += stock::available(catalog, e, id);
            }
            Goods {
                item: item.id.clone(),
                stored,
                in_transit,
                available: free,
                reserved: stored.saturating_sub(free),
            }
        })
        .collect();
    Colony {
        goods,
        supply: supply::colony(state, catalog, index, owner),
        population: supply::population(state, catalog, index, owner),
        research: state.economy.researched(owner).to_vec(),
    }
}

/// Bills held by a building's paid tasks: its upgrade, research and revivals.
fn paid_tasks(catalog: &Catalog, e: &Entity) -> Vec<(Id, u32)> {
    let d = catalog.get(e.definition);
    let mut out = Vec::new();
    if e.upgrade.is_some()
        && let Some(u) = &d.upgrade
    {
        out.extend(stock::bill(&u.items));
    }
    for q in e.research.iter().flat_map(|r| &r.queue) {
        if let Some(r) = catalog.content.rules.research.get(q.id.as_str()) {
            out.extend(stock::bill(&r.items));
        }
    }
    if let Some(policy) = &d.behaviors.revival {
        for q in e.revival.iter().flat_map(|r| &r.queue) {
            out.extend(super::revival::terms(policy, q.level).0);
        }
    }
    out
}

/// Whether the unit is where its route ends (for the work pose).
pub fn at_goal(terrain: &crate::terrain::Terrain, e: &Entity) -> bool {
    let Some(u) = e.unit.as_ref() else {
        return false;
    };
    let Some(goal) = u.goal else {
        return true;
    };
    let size = terrain.size();
    let (x, y, deck) = match terrain.surfaces().and_then(|s| s.node(goal)) {
        Some(n) => (
            n.x as i32,
            n.y as i32,
            n.deck
                .map(|d| terrain.surfaces().expect("decks").deck(d).id.clone()),
        ),
        None => ((goal % size) as i32, (goal / size) as i32, None),
    };
    e.surface == deck && e.position.x == x * 1000 && e.position.y == y * 1000
}
