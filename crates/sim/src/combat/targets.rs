//! The broad phase of target acquisition (old `TargetIndex`): living bodies in 12-cell
//! buckets, rebuilt every planning pass. Exact range, allegiance, weapon and sight tests
//! and the (distance, id) choice stay with the caller, so bucket order never matters.

use super::senses::half_extents;
use crate::{
    catalog::Catalog,
    entity::{EntityId, Milli},
    state::State,
};

/// Bucket side in milli-cells.
const BUCKET: i64 = 12_000;

#[derive(Debug, Default)]
pub struct TargetIndex {
    side: i64,
    buckets: Vec<Vec<EntityId>>,
    /// Buckets with members, to clear them cheaply.
    used: Vec<usize>,
    /// Per entity id: the query that last returned it.
    seen: Vec<u32>,
    query: u32,
}

impl TargetIndex {
    /// Rebuilds membership from the living bodies of `state` on a map of `size` cells.
    pub fn refresh(&mut self, state: &State, catalog: &Catalog, size: u32) {
        let side = (i64::from(size) * 1000 + BUCKET - 1) / BUCKET + 1;
        if side != self.side {
            self.side = side;
            self.buckets = vec![Vec::new(); (side * side) as usize];
            self.used.clear();
        }
        for &b in &self.used {
            self.buckets[b].clear();
        }
        self.used.clear();
        let ids = state.counters.entity as usize;
        if self.seen.len() < ids {
            self.seen.resize(ids, 0);
        }
        for e in state.entities.iter() {
            if !e.hp.is_some_and(|hp| hp > 0) {
                continue;
            }
            let (hx, hy) = match &e.unit {
                Some(_) => {
                    let r = catalog
                        .get(e.definition)
                        .dimensions
                        .as_ref()
                        .map_or(0.0, |d| d.radius);
                    (r, r)
                }
                None => half_extents(catalog, e),
            };
            let (hx, hy) = ((hx * 1000.0).ceil() as i64, (hy * 1000.0).ceil() as i64);
            let (x, y) = (i64::from(e.position.x), i64::from(e.position.y));
            let (x0, x1) = (self.bucket(x - hx), self.bucket(x + hx));
            let (y0, y1) = (self.bucket(y - hy), self.bucket(y + hy));
            for by in y0..=y1 {
                for bx in x0..=x1 {
                    let b = (by * self.side + bx) as usize;
                    if self.buckets[b].is_empty() {
                        self.used.push(b);
                    }
                    self.buckets[b].push(e.id);
                }
            }
        }
    }

    fn bucket(&self, v: i64) -> i64 {
        v.div_euclid(BUCKET).clamp(0, self.side - 1)
    }

    /// Every indexed body whose bucket overlaps the square of `radius` cells around `p`,
    /// each once, into `out`.
    pub fn near(&mut self, p: Milli, radius: f64, out: &mut Vec<EntityId>) {
        out.clear();
        self.query = self.query.wrapping_add(1);
        if self.query == 0 {
            self.seen.iter_mut().for_each(|s| *s = 0);
            self.query = 1;
        }
        let r = (radius * 1000.0).ceil() as i64;
        let (x, y) = (i64::from(p.x), i64::from(p.y));
        let (x0, x1) = (self.bucket(x - r), self.bucket(x + r));
        let (y0, y1) = (self.bucket(y - r), self.bucket(y + r));
        for by in y0..=y1 {
            for bx in x0..=x1 {
                for &id in &self.buckets[(by * self.side + bx) as usize] {
                    let seen = &mut self.seen[id.0 as usize];
                    if *seen != self.query {
                        *seen = self.query;
                        out.push(id);
                    }
                }
            }
        }
    }
}
