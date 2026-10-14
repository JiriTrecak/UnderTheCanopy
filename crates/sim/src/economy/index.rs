//! Derived economy indexes: unit and building membership, resources by area, regrowing
//! resources, and per-job aggregates. Replaces the old per-call scans (`hall()`,
//! `incoming()`, `colonySupply`, `jobs.some(...)` in loops).
//!
//! Membership follows entity creation and removal lazily: ids are never reused, so a new
//! entity shows as an advanced id counter and a removal as a smaller live count.
//! [`Index::rebuild`] gives the same answer from state alone. Job aggregates live with
//! the jobs ([`super::jobs::Jobs`]).

use crate::{catalog::Catalog, entity::EntityId, state::State};
use canopy_content::game::Kind;
use std::collections::{BTreeMap, BTreeSet};


/// Cells per resource bucket side.
const BUCKET: i64 = 16;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    /// The entity id counter and live count at the last sync.
    counter: u32,
    live: usize,
    /// Every entity with a unit part, in id order (old `unitEntities`).
    pub units: Vec<EntityId>,
    /// Every building, in id order (old `buildingEntities`).
    pub buildings: Vec<EntityId>,
    /// Every entity with a resource part, by area.
    pub resources: Resources,
    /// Resources waiting to regrow.
    pub regrowing: BTreeSet<EntityId>,
}

/// Resource entities in square buckets of [`BUCKET`] cells, each in id order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resources {
    buckets: BTreeMap<(i64, i64), Vec<EntityId>>,
    count: usize,
}

impl Resources {
    fn key(x: i32, y: i32) -> (i64, i64) {
        (
            (i64::from(x)).div_euclid(BUCKET * 1000),
            (i64::from(y)).div_euclid(BUCKET * 1000),
        )
    }

    fn insert(&mut self, id: EntityId, x: i32, y: i32) {
        let bucket = self.buckets.entry(Self::key(x, y)).or_default();
        match bucket.binary_search(&id) {
            Ok(_) => return,
            Err(at) => bucket.insert(at, id),
        }
        self.count += 1;
    }

    fn retain(&mut self, keep: impl Fn(EntityId) -> bool) {
        let mut count = 0;
        self.buckets.retain(|_, ids| {
            ids.retain(|&id| keep(id));
            count += ids.len();
            !ids.is_empty()
        });
        self.count = count;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Resources whose position may lie within the square of `radius` milli-cells around
    /// `(x, y)`, in id order. Callers filter the exact distance.
    pub fn near(&self, x: i64, y: i64, radius: i64) -> Vec<EntityId> {
        let span = BUCKET * 1000;
        let (x0, x1) = ((x - radius).div_euclid(span), (x + radius).div_euclid(span));
        let (y0, y1) = ((y - radius).div_euclid(span), (y + radius).div_euclid(span));
        let mut out = Vec::new();
        for by in y0..=y1 {
            for bx in x0..=x1 {
                if let Some(ids) = self.buckets.get(&(bx, by)) {
                    out.extend_from_slice(ids);
                }
            }
        }
        out.sort_unstable();
        out
    }
}

impl Index {
    /// The index of `state` from scratch.
    pub fn rebuild(state: &State, catalog: &Catalog) -> Self {
        let mut index = Self {
            counter: 0,
            live: 0,
            ..Self::default()
        };
        for e in state.entities.iter() {
            index.admit(e, catalog);
        }
        index.counter = state.counters.entity;
        index.live = state.entities.len();
        index
    }

    fn admit(&mut self, e: &crate::entity::Entity, catalog: &Catalog) {
        if e.unit.is_some() {
            self.units.push(e.id);
        }
        if catalog.kind(e.definition) == Kind::Building {
            self.buildings.push(e.id);
        }
        if let Some(r) = &e.resource {
            self.resources.insert(e.id, e.position.x, e.position.y);
            if r.growing_until.is_some() {
                self.regrowing.insert(e.id);
            }
        }
    }

    /// Follows entities created or removed since the last sync.
    pub fn sync(&mut self, state: &State, catalog: &Catalog) {
        let counter = state.counters.entity;
        let mut expected = self.live;
        if counter != self.counter {
            for id in self.counter..counter {
                if let Some(e) = state.entities.get(EntityId(id)) {
                    self.admit(e, catalog);
                    expected += 1;
                }
            }
            self.counter = counter;
        }
        if expected != state.entities.len() {
            let exists = |id: EntityId| state.entities.get(id).is_some();
            self.units.retain(|&id| exists(id));
            self.buildings.retain(|&id| exists(id));
            self.resources.retain(exists);
            self.regrowing.retain(|&id| exists(id));
        }
        self.live = state.entities.len();
    }

    /// Compares with a cold rebuild.
    pub fn verify(&self, cold: &Self) -> Result<(), String> {
        let check = |name: &str, same: bool| {
            if same {
                Ok(())
            } else {
                Err(format!("economy index {name} differs from a rebuild"))
            }
        };
        check("units", self.units == cold.units)?;
        check("buildings", self.buildings == cold.buildings)?;
        check("resources", self.resources == cold.resources)?;
        check("regrowing", self.regrowing == cold.regrowing)
    }
}
