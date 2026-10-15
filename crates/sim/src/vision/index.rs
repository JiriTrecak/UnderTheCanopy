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

/// A sensor in a sector: its key, the teams sharing its sight and its own sector range,
/// so queries skip other teams' sensors and duplicates without looking sensors up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    key: SensorKey,
    teams: u16,
    range: Sectors,
}

#[derive(Debug, Clone)]
pub struct SectorIndex {
    side: usize,
    buckets: Vec<Vec<Entry>>,
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

    /// Adds `key`, shared by the team bits `teams`, to the sectors of `range`.
    pub fn insert(&mut self, key: SensorKey, teams: u16, range: Sectors) {
        let entry = Entry { key, teams, range };
        for y in range.1..=range.3 {
            for x in range.0..=range.2 {
                self.buckets[y * self.side + x].push(entry);
            }
        }
    }

    pub fn remove(&mut self, key: SensorKey, range: Sectors) {
        for y in range.1..=range.3 {
            for x in range.0..=range.2 {
                let bucket = &mut self.buckets[y * self.side + x];
                if let Some(i) = bucket.iter().position(|e| e.key == key) {
                    bucket.swap_remove(i);
                }
            }
        }
    }

    /// Changes the teams sharing `key` in place (its range is unchanged).
    pub fn reteam(&mut self, key: SensorKey, teams: u16, range: Sectors) {
        for y in range.1..=range.3 {
            for x in range.0..=range.2 {
                let bucket = &mut self.buckets[y * self.side + x];
                if let Some(e) = bucket.iter_mut().find(|e| e.key == key) {
                    e.teams = teams;
                }
            }
        }
    }

    /// Calls `visit` once for every key whose range overlaps `query` and whose teams
    /// include one of `teams` (`None`: every key). Stops early when `visit` returns
    /// true, and returns whether it did.
    pub fn any(
        &self,
        query: Sectors,
        teams: Option<u16>,
        mut visit: impl FnMut(SensorKey) -> bool,
    ) -> bool {
        for y in query.1..=query.3 {
            for x in query.0..=query.2 {
                for e in &self.buckets[y * self.side + x] {
                    if teams.is_some_and(|t| e.teams & t == 0) {
                        continue;
                    }
                    // The first sector both ranges share owns the candidate.
                    let r = e.range;
                    if x != query.0.max(r.0) || y != query.1.max(r.1) {
                        continue;
                    }
                    if visit(e.key) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Every entry (key, teams, range) and the sector holding it, for comparisons.
    pub fn contents(&self) -> Vec<(SensorKey, u16, Sectors, usize)> {
        let mut out: Vec<_> = self
            .buckets
            .iter()
            .enumerate()
            .flat_map(|(i, b)| b.iter().map(move |e| (e.key, e.teams, e.range, i)))
            .collect();
        out.sort_unstable();
        out
    }
}
