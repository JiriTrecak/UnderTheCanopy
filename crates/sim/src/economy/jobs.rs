//! Jobs: one worker's task at a target (old `Job`, `jobSchema` in `state.ts`).
//!
//! [`Jobs`] keeps them in id order with two aggregates the old code recomputed by scanning
//! every job: goods inbound to each store, and the sites jobs hold. The aggregates are not
//! saved; loading rebuilds them, and every change goes through [`Jobs`] so they stay exact.

use super::Ctx;
use crate::entity::EntityId;
use canopy_content::Id;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Deliver,
    Construct,
    Repair,
    Harvest,
    Plant,
}

impl JobKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Deliver => "deliver",
            Self::Construct => "construct",
            Self::Repair => "repair",
            Self::Harvest => "harvest",
            Self::Plant => "plant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// On the way to the work site.
    Walk,
    /// Arrived; waiting for a free place at the source (harvest).
    Wait,
    Work,
    /// The tree it felled is falling; goods leave once it is down.
    Fall,
    /// Carrying goods to the store.
    Return,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Walk => "walk",
            Self::Wait => "wait",
            Self::Work => "work",
            Self::Fall => "fall",
            Self::Return => "return",
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u32,
    #[serde(rename = "type")]
    pub kind: JobKind,
    pub worker: EntityId,
    /// The building worked on, delivered to or employing the planter.
    pub target: EntityId,
    /// The resource harvested or planted.
    pub source: Option<EntityId>,
    pub item: Option<Id>,
    /// Goods the job claims at its target.
    pub amount: u32,
    pub phase: Phase,
    /// When a harvester arrived at its source: the waiting order.
    pub arrived_tick: Option<u32>,
    pub progress: u32,
}

impl Job {
    /// The site the job holds: construct and repair targets, plant sources.
    pub fn site(&self) -> Option<(JobKind, EntityId)> {
        match self.kind {
            JobKind::Construct | JobKind::Repair => Some((self.kind, self.target)),
            JobKind::Plant => self.source.map(|s| (JobKind::Plant, s)),
            JobKind::Harvest | JobKind::Deliver => None,
        }
    }

    /// Whether the amount is goods on their way to the target.
    pub fn inbound(&self) -> bool {
        matches!(self.kind, JobKind::Harvest | JobKind::Deliver)
    }
}

/// The active jobs in id order, with their aggregates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Jobs {
    list: Vec<Job>,
    /// Goods on their way to each store (harvest and delivery amounts by target).
    incoming: BTreeMap<EntityId, u32>,
    /// Jobs holding each site.
    sites: BTreeMap<(JobKind, EntityId), u32>,
}

impl Jobs {
    pub fn iter(&self) -> impl Iterator<Item = &Job> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn ids(&self) -> Vec<u32> {
        self.list.iter().map(|j| j.id).collect()
    }

    pub fn get(&self, id: u32) -> Option<&Job> {
        self.list
            .binary_search_by_key(&id, |j| j.id)
            .ok()
            .map(|i| &self.list[i])
    }

    /// Phase, progress and arrival only; amounts change through [`Jobs::set_amount`].
    pub fn get_mut(&mut self, id: u32) -> Option<&mut Job> {
        self.list
            .binary_search_by_key(&id, |j| j.id)
            .ok()
            .map(|i| &mut self.list[i])
    }

    /// Adds a job; its id must exceed every other.
    pub fn push(&mut self, job: Job) {
        debug_assert!(self.list.last().is_none_or(|j| j.id < job.id));
        self.count(&job, true);
        self.list.push(job);
    }

    pub fn remove(&mut self, id: u32) -> Option<Job> {
        let at = self.list.binary_search_by_key(&id, |j| j.id).ok()?;
        let job = self.list.remove(at);
        self.count(&job, false);
        Some(job)
    }

    pub fn set_amount(&mut self, id: u32, amount: u32) {
        let Ok(at) = self.list.binary_search_by_key(&id, |j| j.id) else {
            return;
        };
        let mut job = self.list[at].clone();
        self.count(&job, false);
        job.amount = amount;
        self.count(&job, true);
        self.list[at] = job;
    }

    fn count(&mut self, job: &Job, add: bool) {
        fn bump<K: Ord + Copy>(map: &mut BTreeMap<K, u32>, key: K, by: u32, add: bool) {
            let value = map.get(&key).copied().unwrap_or(0);
            let value = if add {
                value + by
            } else {
                value.saturating_sub(by)
            };
            if value == 0 {
                map.remove(&key);
            } else {
                map.insert(key, value);
            }
        }
        if job.inbound() && job.amount > 0 {
            bump(&mut self.incoming, job.target, job.amount, add);
        }
        if let Some(site) = job.site() {
            bump(&mut self.sites, site, 1, add);
        }
    }

    /// Goods on their way to `store` (old `incoming`).
    pub fn incoming(&self, store: EntityId) -> u32 {
        self.incoming.get(&store).copied().unwrap_or(0)
    }

    /// Whether a job of `kind` holds the site `id`.
    pub fn occupied(&self, kind: JobKind, id: EntityId) -> bool {
        self.sites.contains_key(&(kind, id))
    }

    /// Checks the aggregates against a recount.
    pub fn verify(&self) -> Result<(), String> {
        let mut cold = Self::default();
        for job in &self.list {
            cold.push(job.clone());
        }
        if cold.incoming != self.incoming || cold.sites != self.sites {
            return Err("job aggregates differ from a recount".into());
        }
        Ok(())
    }
}

impl Serialize for Jobs {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.list.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Jobs {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let list = Vec::<Job>::deserialize(deserializer)?;
        let mut jobs = Self::default();
        for job in list {
            if jobs.list.last().is_some_and(|j| j.id >= job.id) {
                return Err(serde::de::Error::custom("jobs must be in increasing id order"));
            }
            jobs.push(job);
        }
        Ok(jobs)
    }
}

/// What a new job starts with beyond its kind, worker and target.
#[derive(Debug, Clone)]
pub struct Spec {
    pub source: Option<EntityId>,
    pub item: Option<Id>,
    pub amount: u32,
    pub phase: Phase,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            source: None,
            item: None,
            amount: 0,
            phase: Phase::Walk,
        }
    }
}

/// Starts a job for `worker` (old `Economy.job`).
pub fn create(ctx: &mut Ctx, kind: JobKind, worker: EntityId, target: EntityId, spec: Spec) -> u32 {
    let id = ctx.state.counters.job;
    ctx.state.counters.job += 1;
    ctx.state.economy.jobs.push(Job {
        id,
        kind,
        worker,
        target,
        source: spec.source,
        item: spec.item,
        amount: spec.amount,
        phase: spec.phase,
        arrived_tick: None,
        progress: 0,
    });
    if let Some(u) = ctx
        .state
        .entities
        .get_mut(worker)
        .and_then(|e| e.unit.as_mut())
    {
        u.work.job = Some(id);
    }
    refresh_body(ctx, worker);
    id
}

/// Ends a job without consequences for its workplace (old `eraseJob`): the worker drops
/// its route.
pub fn erase(ctx: &mut Ctx, id: u32) {
    let Some(job) = remove(ctx.state, id) else {
        return;
    };
    refresh_body(ctx, job.worker);
}

/// [`erase`] with only the state (bodies refresh at the next sync).
pub fn remove(state: &mut crate::state::State, id: u32) -> Option<Job> {
    let job = state.economy.jobs.remove(id)?;
    if let Some(u) = state
        .entities
        .get_mut(job.worker)
        .and_then(|e| e.unit.as_mut())
        && u.work.job == Some(id)
    {
        u.work.job = None;
        u.route.clear();
        u.goal = None;
        u.detour = None;
    }
    Some(job)
}

/// Whether the worker ghosts through bodies may have changed: refresh its body.
pub fn refresh_body(ctx: &mut Ctx, worker: EntityId) {
    if let Some(e) = ctx.state.entities.get(worker) {
        ctx.spatial.refresh_body(ctx.state, e);
    }
}
