//! Static occupancy: cells closed by buildings and standing resources (old `Spatial`
//! `occupied`/`resources` grids, `rebuild`, `appendOccupancy`, `refreshAfterRemoval`).
//!
//! Each blocker keeps the exact cells it covers, and each cell counts its blockers, so
//! any appearance, removal or change updates only that entity's cells. Overlapping
//! blockers keep a cell closed until the last one leaves, as the old owner stacks did.

use crate::entity::EntityId;
use std::collections::BTreeMap;

/// The cells one entity closes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cover {
    /// Building footprint cells.
    pub building: Vec<u32>,
    /// Resource collision cells.
    pub resource: Vec<u32>,
}

impl Cover {
    pub fn is_empty(&self) -> bool {
        self.building.is_empty() && self.resource.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Occupancy {
    buildings: Vec<u16>,
    resources: Vec<u16>,
    covers: BTreeMap<EntityId, Cover>,
}

impl Occupancy {
    pub fn new(nodes: usize) -> Self {
        Self {
            buildings: vec![0; nodes],
            resources: vec![0; nodes],
            covers: BTreeMap::new(),
        }
    }

    /// Whether a building or a standing resource covers the node.
    pub fn blocked(&self, node: u32) -> bool {
        let n = node as usize;
        self.buildings[n] != 0 || self.resources[n] != 0
    }

    pub fn building(&self, node: u32) -> bool {
        self.buildings[node as usize] != 0
    }

    /// Sets the cells `id` covers, appending to `changed` every node whose blocked state
    /// flipped (possibly with repeats).
    pub fn set(&mut self, id: EntityId, cover: Cover, changed: &mut Vec<u32>) {
        let old = if cover.is_empty() {
            self.covers.remove(&id)
        } else {
            match self.covers.get(&id) {
                Some(old) if *old == cover => return,
                _ => self.covers.insert(id, cover.clone()),
            }
        };
        if let Some(old) = old {
            self.apply(&old, -1, changed);
        }
        if !cover.is_empty() {
            self.apply(&cover, 1, changed);
        }
    }

    fn apply(&mut self, cover: &Cover, delta: i32, changed: &mut Vec<u32>) {
        for &cell in &cover.building {
            let before = self.blocked(cell);
            let c = &mut self.buildings[cell as usize];
            *c = (i32::from(*c) + delta) as u16;
            if before != self.blocked(cell) {
                changed.push(cell);
            }
        }
        for &cell in &cover.resource {
            let before = self.blocked(cell);
            let c = &mut self.resources[cell as usize];
            *c = (i32::from(*c) + delta) as u16;
            if before != self.blocked(cell) {
                changed.push(cell);
            }
        }
    }

    /// Entities currently covering cells.
    pub fn ids(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.covers.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_blockers_keep_cells_closed_until_both_leave() {
        let mut occupancy = Occupancy::new(16);
        let mut changed = Vec::new();
        let cover = |cells: &[u32]| Cover {
            building: cells.to_vec(),
            resource: Vec::new(),
        };
        occupancy.set(EntityId(1), cover(&[1, 2]), &mut changed);
        occupancy.set(EntityId(2), cover(&[2, 3]), &mut changed);
        assert_eq!(changed, [1, 2, 3]);
        changed.clear();
        occupancy.set(EntityId(1), Cover::default(), &mut changed);
        assert_eq!(changed, [1]);
        assert!(occupancy.blocked(2) && occupancy.blocked(3));
        changed.clear();
        occupancy.set(EntityId(2), cover(&[2, 3]), &mut changed);
        assert!(changed.is_empty());
    }
}
