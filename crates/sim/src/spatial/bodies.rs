//! The moving-body broad phase (old `UnitIndex`, `src/sim/game/unitIndex.ts`): one compact
//! record per unit with everything collision and traffic queries read, and per-cell buckets
//! of the bodies that collide. Derived, never saved; refreshed from state at each planning
//! and movement boundary and updated after every body that moves.
//!
//! Every query that reads the buckets is order-independent (a yes/no answer or a list the
//! caller sorts), so bucket order is free.

use crate::{entity::EntityId, terrain::DeckId};
use canopy_content::placement::Owner;
use std::collections::BTreeMap;

const NONE: u32 = u32::MAX;
/// Cells per bucket side (a power of two).
const BLOCK: u32 = 4;
const SHIFT: u32 = 2;

/// What spatial queries know about one unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyRecord {
    pub id: EntityId,
    pub owner: Owner,
    /// Milli-cells.
    pub x: i32,
    pub y: i32,
    pub deck: Option<DeckId>,
    /// Navigation node it stands on.
    pub node: u32,
    /// Radius in milli-cells.
    pub radius: i32,
    pub height: f64,
    /// Floor height in world units (layered maps).
    pub floor: f64,
    pub air: bool,
    /// Ghosts through other bodies (harvest traffic, garrisons).
    pub ignores: bool,
    /// Alive and physically present (not contained, not awaiting release).
    pub active: bool,
    pub ready_tick: u32,
    pub has_route: bool,
    pub goal: Option<u32>,
    /// The waypoint a yielding body has claimed.
    pub reservation: Option<u32>,
}

impl BodyRecord {
    /// Whether the body collides and claims its cell.
    pub fn solid(&self) -> bool {
        self.active && !self.ignores
    }

    /// The ground cell of its position.
    pub fn cell(&self, size: u32) -> u32 {
        let (x, y) = self.xy();
        y as u32 * size + x as u32
    }

    /// Cell coordinates of its position.
    pub fn xy(&self) -> (i32, i32) {
        (
            crate::spatial::geometry::grid(self.x),
            crate::spatial::geometry::grid(self.y),
        )
    }
}

#[derive(Debug, Clone)]
pub struct Bodies {
    size: u32,
    /// Buckets per side.
    blocks: u32,
    /// Entity id → slot + 1.
    index: Vec<u32>,
    records: Vec<BodyRecord>,
    /// Bucket of each slot (NONE when not bucketed) and the next slot in its bucket.
    bucket: Vec<u32>,
    next: Vec<u32>,
    heads: Vec<u32>,
    reserved: BTreeMap<u32, Vec<u32>>,
    seen: Vec<u32>,
    epoch: u32,
    solid: usize,
}

impl Bodies {
    pub fn new(size: u32) -> Self {
        let blocks = size.div_ceil(BLOCK);
        Self {
            size,
            blocks,
            index: Vec::new(),
            records: Vec::new(),
            bucket: Vec::new(),
            next: Vec::new(),
            heads: vec![NONE; (blocks * blocks) as usize],
            reserved: BTreeMap::new(),
            seen: Vec::new(),
            epoch: 0,
            solid: 0,
        }
    }

    fn slot(&self, id: EntityId) -> Option<usize> {
        let slot = *self.index.get(id.0 as usize)?;
        slot.checked_sub(1).map(|s| s as usize)
    }

    pub fn get(&self, id: EntityId) -> Option<&BodyRecord> {
        self.slot(id).map(|s| &self.records[s])
    }

    /// Records in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &BodyRecord> {
        self.records.iter()
    }

    /// Starts a full refresh: records not [`Bodies::update`]d before [`Bodies::end_sync`]
    /// are dropped.
    pub fn begin_sync(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if self.seen.len() < self.records.len() {
            self.seen.resize(self.records.len(), 0);
        }
    }

    pub fn end_sync(&mut self) {
        let mut slot = 0;
        while slot < self.records.len() {
            if self.seen.get(slot) == Some(&self.epoch) {
                slot += 1;
                continue;
            }
            self.remove_slot(slot);
        }
    }

    /// Drops a unit's record at once (it left the world mid-tick).
    pub fn remove(&mut self, id: EntityId) {
        if let Some(slot) = self.slot(id) {
            self.remove_slot(slot);
        }
    }

    fn remove_slot(&mut self, slot: usize) {
        self.unbucket(slot);
        self.unreserve(slot);
        let last = self.records.len() - 1;
        let id = self.records[slot].id;
        self.index[id.0 as usize] = 0;
        if slot != last {
            // Move the last record into the hole, relinking its memberships.
            let moved = self.records[last];
            let cell = self.bucket[last];
            let reservation = moved.reservation.filter(|_| moved.solid());
            self.unbucket(last);
            self.unreserve(last);
            self.records[slot] = moved;
            self.seen[slot] = self.seen[last];
            self.bucket[slot] = NONE;
            self.index[moved.id.0 as usize] = slot as u32 + 1;
            if cell != NONE {
                self.link(slot, cell);
            }
            if let Some(node) = reservation {
                self.reserved.entry(node).or_default().push(slot as u32);
            }
        }
        self.records.pop();
        self.bucket.pop();
        self.next.pop();
        self.seen.truncate(self.records.len());
    }

    /// The bucket of a body: its cell's block, if the cell is on the map.
    fn block(&self, record: &BodyRecord) -> Option<u32> {
        let (x, y) = record.xy();
        let size = self.size as i32;
        (x >= 0 && y >= 0 && x < size && y < size)
            .then(|| (y as u32 >> SHIFT) * self.blocks + (x as u32 >> SHIFT))
    }

    fn link(&mut self, slot: usize, cell: u32) {
        self.bucket[slot] = cell;
        self.next[slot] = self.heads[cell as usize];
        self.heads[cell as usize] = slot as u32;
        self.solid += 1;
    }

    fn unbucket(&mut self, slot: usize) {
        let cell = self.bucket[slot];
        if cell == NONE {
            return;
        }
        let following = self.next[slot];
        if self.heads[cell as usize] == slot as u32 {
            self.heads[cell as usize] = following;
        } else {
            let mut at = self.heads[cell as usize] as usize;
            while self.next[at] != slot as u32 {
                at = self.next[at] as usize;
            }
            self.next[at] = following;
        }
        self.bucket[slot] = NONE;
        self.solid -= 1;
    }

    fn unreserve(&mut self, slot: usize) {
        let Some(node) = self.records[slot]
            .reservation
            .filter(|_| self.records[slot].solid())
        else {
            return;
        };
        if let Some(list) = self.reserved.get_mut(&node) {
            list.retain(|&s| s != slot as u32);
            if list.is_empty() {
                self.reserved.remove(&node);
            }
        }
    }

    /// Inserts or refreshes a unit's record.
    pub fn update(&mut self, record: BodyRecord) {
        let slot = match self.slot(record.id) {
            Some(slot) => {
                let old = self.records[slot];
                if old == record {
                    if let Some(seen) = self.seen.get_mut(slot) {
                        *seen = self.epoch;
                    }
                    return;
                }
                let old_reservation = old.reservation.filter(|_| old.solid());
                let new_reservation = record.reservation.filter(|_| record.solid());
                if old_reservation != new_reservation {
                    self.unreserve(slot);
                }
                let block = self.block(&record).unwrap_or(NONE);
                if self.bucket[slot] != NONE && (!record.solid() || self.bucket[slot] != block) {
                    self.unbucket(slot);
                }
                self.records[slot] = record;
                if old_reservation != new_reservation
                    && let Some(node) = new_reservation
                {
                    self.reserved.entry(node).or_default().push(slot as u32);
                }
                slot
            }
            None => {
                let slot = self.records.len();
                if self.index.len() <= record.id.0 as usize {
                    self.index.resize(record.id.0 as usize + 1, 0);
                }
                self.index[record.id.0 as usize] = slot as u32 + 1;
                self.records.push(record);
                self.bucket.push(NONE);
                self.next.push(NONE);
                if let Some(node) = record.reservation.filter(|_| record.solid()) {
                    self.reserved.entry(node).or_default().push(slot as u32);
                }
                slot
            }
        };
        if self.seen.len() <= slot {
            self.seen.resize(slot + 1, 0);
        }
        self.seen[slot] = self.epoch;
        if record.solid()
            && self.bucket[slot] == NONE
            && let Some(block) = self.block(&record)
        {
            self.link(slot, block);
        }
    }

    /// Solid bodies standing in ground cell `(x, y)`.
    pub fn in_cell(&self, x: i32, y: i32) -> impl Iterator<Item = &BodyRecord> + '_ {
        let size = self.size as i32;
        let mut at = if x >= 0 && y >= 0 && x < size && y < size {
            self.heads[((y as u32 >> SHIFT) * self.blocks + (x as u32 >> SHIFT)) as usize]
        } else {
            NONE
        };
        std::iter::from_fn(move || {
            while at != NONE {
                let slot = at as usize;
                at = self.next[slot];
                let record = &self.records[slot];
                if record.xy() == (x, y) {
                    return Some(record);
                }
            }
            None
        })
    }

    /// Whether any body holds a reservation.
    pub fn any_reserved(&self) -> bool {
        !self.reserved.is_empty()
    }

    /// Solid bodies that reserved `node` for a yield.
    pub fn reserved_at(&self, node: u32) -> impl Iterator<Item = &BodyRecord> + '_ {
        self.reserved
            .get(&node)
            .into_iter()
            .flatten()
            .map(|&slot| &self.records[slot as usize])
    }

    /// Solid bodies whose cell lies in the fixed-point rectangle, each once.
    pub fn within(
        &self,
        min_x: i64,
        min_y: i64,
        max_x: i64,
        max_y: i64,
        mut visit: impl FnMut(&BodyRecord) -> bool,
    ) -> bool {
        let last = i64::from(self.size) - 1;
        let x0 = ((min_x + 500).div_euclid(1000)).max(0);
        let x1 = ((max_x + 500).div_euclid(1000)).min(last);
        let y0 = ((min_y + 500).div_euclid(1000)).max(0);
        let y1 = ((max_y + 500).div_euclid(1000)).min(last);
        if x0 > x1 || y0 > y1 {
            return true;
        }
        // A cell `c` holds the milli-cell positions `1000·c − 500 ≤ v < 1000·c + 500`, so
        // the cell test reads the positions without dividing.
        let (lx, hx) = (x0 * 1000 - 500, x1 * 1000 + 500);
        let (ly, hy) = (y0 * 1000 - 500, y1 * 1000 + 500);
        let inside = |record: &BodyRecord| {
            let (x, y) = (i64::from(record.x), i64::from(record.y));
            x >= lx && x < hx && y >= ly && y < hy
        };
        let (bx0, bx1) = ((x0 >> SHIFT) as u32, (x1 >> SHIFT) as u32);
        let (by0, by1) = ((y0 >> SHIFT) as u32, (y1 >> SHIFT) as u32);
        // Long rectangles are mostly empty: scan the bodies instead.
        if u64::from(bx1 - bx0 + 1) * u64::from(by1 - by0 + 1) > self.solid as u64 * 2 {
            for (slot, record) in self.records.iter().enumerate() {
                if self.bucket[slot] != NONE && inside(record) && !visit(record) {
                    return false;
                }
            }
            return true;
        }
        for by in by0..by1 + 1 {
            for bx in bx0..bx1 + 1 {
                let mut at = self.heads[(by * self.blocks + bx) as usize];
                while at != NONE {
                    let slot = at as usize;
                    let record = &self.records[slot];
                    if inside(record) && !visit(record) {
                        return false;
                    }
                    at = self.next[slot];
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(id: u32, x: i32, y: i32) -> BodyRecord {
        BodyRecord {
            id: EntityId(id),
            owner: Owner::Player(0),
            x,
            y,
            deck: None,
            node: 0,
            radius: 400,
            height: 1.0,
            floor: 0.0,
            air: false,
            ignores: false,
            active: true,
            ready_tick: 0,
            has_route: false,
            goal: None,
            reservation: None,
        }
    }

    fn ids(bodies: &Bodies, rect: (i64, i64, i64, i64)) -> Vec<u32> {
        let mut out = Vec::new();
        bodies.within(rect.0, rect.1, rect.2, rect.3, |b| {
            out.push(b.id.0);
            true
        });
        out.sort_unstable();
        out
    }

    #[test]
    fn buckets_follow_moves_and_removals() {
        let mut bodies = Bodies::new(32);
        bodies.update(body(1, 5000, 5000));
        bodies.update(body(2, 5400, 5000));
        bodies.update(body(3, 20_000, 20_000));
        assert_eq!(ids(&bodies, (4000, 4000, 6000, 6000)), [1, 2]);
        bodies.update(body(2, 9000, 5000));
        assert_eq!(ids(&bodies, (4000, 4000, 6000, 6000)), [1]);
        let mut gone = body(1, 5000, 5000);
        gone.active = false;
        bodies.update(gone);
        assert!(ids(&bodies, (4000, 4000, 6000, 6000)).is_empty());
        bodies.begin_sync();
        bodies.update(body(3, 20_000, 20_000));
        bodies.end_sync();
        assert!(bodies.get(EntityId(1)).is_none() && bodies.get(EntityId(2)).is_none());
        assert_eq!(ids(&bodies, (0, 0, 31_000, 31_000)), [3]);
        let mut yielding = body(3, 20_000, 20_000);
        yielding.reservation = Some(77);
        bodies.update(yielding);
        assert_eq!(bodies.reserved_at(77).count(), 1);
    }
}
