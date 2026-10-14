//! The ground navigation mesh for one body radius (old `GroundNavigation`,
//! `src/sim/game/groundNavigation.ts`). Derived from static collision only; local edits
//! rebuild just the tiles whose clearance mask changed, which gives the same mesh as a
//! cold build.

pub mod build;
pub mod earcut;
pub mod polygons;
pub mod query;

use build::{Builder, CLEARANCE_PADDING, MeshInput, TILE};
use polygons::P;
use query::MeshQuery;
use std::collections::BTreeSet;

/// The old prototype weight: a modest bias towards the goal in the polygon search.
const HEURISTIC_WEIGHT: f64 = 1.2;

#[derive(Debug, Clone)]
pub struct GroundMesh {
    radius: f64,
    size: u32,
    builder: Builder,
    query: MeshQuery,
    dirty: BTreeSet<u32>,
}

impl GroundMesh {
    pub fn new(input: MeshInput, radius: f64) -> Self {
        let mut builder = Builder::new(input.size, radius);
        let width = builder.width();
        let tiles: Vec<_> = (0..width * width)
            .map(|id| (id, builder.rebuild(input, id).expect("first build")))
            .collect();
        let mut query = MeshQuery::new(input.size, HEURISTIC_WEIGHT);
        let changed: Vec<_> = tiles.iter().map(|(id, g)| (*id, g.as_ref())).collect();
        query.update(&changed);
        Self {
            radius,
            size: input.size,
            builder,
            query,
            dirty: BTreeSet::new(),
        }
    }

    /// Marks the tiles within this body's reach of changed cells for rebuilding.
    pub fn invalidate(&mut self, cells: &[u32]) {
        let size = self.size as i64;
        let width = i64::from(self.builder.width());
        let tile = i64::from(TILE);
        let halo = self.radius.ceil() as i64 + 1;
        for &cell in cells {
            let (x, z) = (i64::from(cell) % size, i64::from(cell) / size);
            for tz in ((z - halo).div_euclid(tile)).max(0)..=((z + halo).div_euclid(tile)).min(width - 1) {
                for tx in ((x - halo).div_euclid(tile)).max(0)..=((x + halo).div_euclid(tile)).min(width - 1)
                {
                    self.dirty.insert((tz * width + tx) as u32);
                }
            }
        }
    }

    /// Rebuilds invalidated tiles against the current collision.
    pub fn prepare(&mut self, input: MeshInput) {
        if self.dirty.is_empty() {
            return;
        }
        let dirty = std::mem::take(&mut self.dirty);
        let rebuilt: Vec<_> = dirty
            .into_iter()
            .filter_map(|id| self.builder.rebuild(input, id).map(|g| (id, g)))
            .collect();
        if !rebuilt.is_empty() {
            let changed: Vec<_> = rebuilt.iter().map(|(id, g)| (*id, g.as_ref())).collect();
            self.query.update(&changed);
        }
    }

    /// A funnel path in cells between two cell centres; endpoints attach within the
    /// clearance padding.
    pub fn path(&mut self, start: P, goal: P) -> Option<Vec<P>> {
        self.query
            .path(start, goal, f64::from(CLEARANCE_PADDING) / 1000.0)
    }

    /// Polygons the latest search expanded.
    pub fn expanded(&self) -> u32 {
        self.query.expanded
    }
}
