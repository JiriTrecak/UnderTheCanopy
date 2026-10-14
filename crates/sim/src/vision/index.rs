//! Broad phase for exact sight queries (old `SectorIndex` of sight records): sensors by
//! the 16-cell sectors their sight circle touches. Derived.

use super::sensor::SensorKey;
use crate::{entity::Milli, math};

/// Cells per sector side.
const SECTOR: i64 = 16;
/// Slack around a sight circle, in milli-cells: units sight from their cell, not their
/// exact position.
const SLACK: i64 = 1500;

/// Inclusive sector range `(x0, y0, x1, y1)`.
pub type Sectors = (usize, usize, usize, usize);

#[derive(Debug, Clone)]
pub struct SectorIndex {
    side: usize,
    buckets: Vec<Vec<SensorKey>>,
}

impl SectorIndex {
    pub fn new(size: u32) -> Self {
        let side = (i64::from(size).max(1) + SECTOR - 1) / SECTOR;
        let side = side as usize;
        Self {
            side,
            buckets: vec![Vec::new(); side * side],
        }
    }

    /// The sectors a box of milli-cells touches, clamped to the map.
    pub fn sectors(&self, min: Milli, max: Milli) -> Sectors {
        let last = self.side as i64 - 1;
        let sector = |v: i32| math::div_floor(i64::from(v), SECTOR * 1000).clamp(0, last) as usize;
        (sector(min.x), sector(min.y), sector(max.x), sector(max.y))
    }

    /// The sectors a sight circle touches.
    pub fn circle(&self, origin: Milli, radius: u32) -> Sectors {
        let r = (i64::from(radius) + SLACK) as i32;
        self.sectors(
            Milli {
                x: origin.x.saturating_sub(r),
                y: origin.y.saturating_sub(r),
            },
            Milli {
                x: origin.x.saturating_add(r),
                y: origin.y.saturating_add(r),
            },
        )
    }

    pub fn insert(&mut self, key: SensorKey, range: Sectors) {
        for y in range.1..=range.3 {
            for x in range.0..=range.2 {
                self.buckets[y * self.side + x].push(key);
            }
        }
    }

    pub fn remove(&mut self, key: SensorKey, range: Sectors) {
        for y in range.1..=range.3 {
            for x in range.0..=range.2 {
                let bucket = &mut self.buckets[y * self.side + x];
                if let Some(i) = bucket.iter().position(|&k| k == key) {
                    bucket.swap_remove(i);
                }
            }
        }
    }

    /// Calls `visit` once for every key whose range overlaps `query`; `range_of` gives a
    /// key's own range. Stops early when `visit` returns true, and returns whether it did.
    pub fn any(
        &self,
        query: Sectors,
        range_of: impl Fn(SensorKey) -> Option<Sectors>,
        mut visit: impl FnMut(SensorKey) -> bool,
    ) -> bool {
        for y in query.1..=query.3 {
            for x in query.0..=query.2 {
                for &key in &self.buckets[y * self.side + x] {
                    // The first sector both ranges share owns the candidate.
                    let Some(r) = range_of(key) else { continue };
                    if x != query.0.max(r.0) || y != query.1.max(r.1) {
                        continue;
                    }
                    if visit(key) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Every key and the sectors holding it, for comparisons.
    pub fn contents(&self) -> Vec<(SensorKey, usize)> {
        let mut out: Vec<_> = self
            .buckets
            .iter()
            .enumerate()
            .flat_map(|(i, b)| b.iter().map(move |&k| (k, i)))
            .collect();
        out.sort_unstable();
        out
    }
}
