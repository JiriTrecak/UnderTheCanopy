//! A team's sight coverage (old `VisionMask`): how many of the team's sensors see each fog
//! node. Sensors add and remove only the nodes their footprint gained or lost; fog cells
//! change only where a count crossed zero. Derived: rebuilt from the sensors on load.

use super::knowledge::{EXPLORED, FogCells, UNEXPLORED, VISIBLE};
use std::{collections::VecDeque, sync::Arc};

/// Fog revisions a view can still be patched from.
const HISTORY: usize = 16;
/// Changed nodes those revisions may hold in total.
const HISTORY_NODES: usize = 16_384;

/// The nodes that changed to reach `revision` from the one before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FogStep {
    pub revision: u32,
    pub nodes: Arc<[u32]>,
}

#[derive(Debug, Clone)]
pub struct Mask {
    counts: Vec<u16>,
    marks: Vec<u32>,
    epoch: u32,
    touched: Vec<u32>,
    /// Advances whenever a fog cell changes; restarts with the world.
    pub revision: u32,
    journal: VecDeque<FogStep>,
    journal_nodes: usize,
    /// The journal as handed to views; replaced only when the fog changes.
    pub history: Arc<[FogStep]>,
}

impl Mask {
    pub fn new(nodes: usize) -> Self {
        Self {
            counts: vec![0; nodes],
            marks: vec![0; nodes],
            epoch: 1,
            touched: Vec::new(),
            revision: 0,
            journal: VecDeque::new(),
            journal_nodes: 0,
            history: Arc::from([]),
        }
    }

    pub fn count(&self, node: u32) -> u16 {
        self.counts[node as usize]
    }

    pub fn counts(&self) -> &[u16] {
        &self.counts
    }

    /// One more (`+1`) or one fewer (`-1`) sensor sees `node`.
    pub fn change(&mut self, node: u32, delta: i32) {
        let n = node as usize;
        let count = i32::from(self.counts[n]) + delta;
        debug_assert!((0..=i32::from(u16::MAX)).contains(&count));
        self.counts[n] = count as u16;
        if self.marks[n] != self.epoch {
            self.marks[n] = self.epoch;
            self.touched.push(node);
        }
    }

    /// Applies the coverage changes since the last call to `cells`: covered nodes become
    /// visible, uncovered visible nodes become explored. Returns the nodes that changed,
    /// in the order they were first touched.
    pub fn settle(&mut self, cells: &mut FogCells) -> Vec<u32> {
        let mut changed = Vec::new();
        for &node in &self.touched {
            let n = node as usize;
            let old = cells.0[n];
            let next = settled(self.counts[n], old);
            if next != old {
                Arc::make_mut(&mut cells.0)[n] = next;
                changed.push(node);
            }
        }
        self.forget_touched();
        self.publish(&changed);
        changed
    }

    fn forget_touched(&mut self) {
        self.touched.clear();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.marks.fill(0);
            self.epoch = 1;
        }
    }

    /// Makes every cell agree with the coverage (after a restore). Returns changed nodes.
    pub fn normalize(&mut self, cells: &mut FogCells) -> Vec<u32> {
        let changed: Vec<u32> = (0..cells.len() as u32)
            .filter(|&n| {
                settled(self.counts[n as usize], cells.0[n as usize]) != cells.0[n as usize]
            })
            .collect();
        if !changed.is_empty() {
            let data = Arc::make_mut(&mut cells.0);
            for &node in &changed {
                data[node as usize] = settled(self.counts[node as usize], data[node as usize]);
            }
        }
        self.forget_touched();
        changed
    }

    fn publish(&mut self, changed: &[u32]) {
        if changed.is_empty() {
            return;
        }
        self.revision += 1;
        self.journal.push_back(FogStep {
            revision: self.revision,
            nodes: Arc::from(changed),
        });
        self.journal_nodes += changed.len();
        while self.journal.len() > HISTORY || self.journal_nodes > HISTORY_NODES {
            let dropped = self.journal.pop_front().expect("journal entry");
            self.journal_nodes -= dropped.nodes.len();
        }
        self.history = self.journal.iter().cloned().collect();
    }
}

fn settled(count: u16, old: u8) -> u8 {
    if count > 0 {
        VISIBLE
    } else if old == UNEXPLORED {
        UNEXPLORED
    } else {
        EXPLORED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_explores_and_keeps_overlaps() {
        let mut mask = Mask::new(8);
        let mut cells = FogCells::filled(8, UNEXPLORED);
        let published = cells.clone();
        for node in [2, 3] {
            mask.change(node, 1);
        }
        mask.change(3, 1);
        assert_eq!(mask.settle(&mut cells), [2, 3]);
        assert_eq!(cells.0.as_slice(), [0, 0, 2, 2, 0, 0, 0, 0]);
        // A view holding the old cells keeps them.
        assert!(published.0.iter().all(|&c| c == UNEXPLORED));
        mask.change(3, -1);
        mask.change(2, -1);
        assert_eq!(mask.settle(&mut cells), [2]);
        assert_eq!(cells.0.as_slice(), [0, 0, 1, 2, 0, 0, 0, 0]);
        assert_eq!(mask.revision, 2);
        assert_eq!(mask.history.len(), 2);
        assert_eq!(mask.settle(&mut cells), [] as [u32; 0]);
        assert_eq!(mask.revision, 2);
    }
}
