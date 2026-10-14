//! A set of navigation nodes with O(1) membership and O(members) clearing: the old
//! game's `Set<number>` of reserved cells, without hashing.

/// Node membership, as consulted by sweeps and searches.
pub trait Reserved {
    fn has(&self, node: u32) -> bool;
}

impl<F: Fn(u32) -> bool> Reserved for F {
    fn has(&self, node: u32) -> bool {
        self(node)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CellSet {
    marks: Vec<u32>,
    epoch: u32,
    /// Insertion order; a removed node keeps its stale entry.
    members: Vec<u32>,
    len: usize,
    removed: bool,
}

impl CellSet {
    pub fn new(capacity: usize) -> Self {
        let mut set = Self::default();
        set.reset(capacity);
        set
    }

    /// Empties the set, resizing it for `capacity` nodes.
    pub fn reset(&mut self, capacity: usize) {
        if self.marks.len() != capacity {
            self.marks = vec![0; capacity];
            self.epoch = 0;
        }
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.marks.fill(0);
            self.epoch = 1;
        }
        self.members.clear();
        self.len = 0;
        self.removed = false;
    }

    pub fn contains(&self, node: u32) -> bool {
        self.marks.get(node as usize) == Some(&self.epoch)
    }

    pub fn insert(&mut self, node: u32) -> bool {
        let Some(mark) = self.marks.get_mut(node as usize) else {
            return false;
        };
        if *mark == self.epoch {
            return false;
        }
        *mark = self.epoch;
        self.members.push(node);
        self.len += 1;
        true
    }

    pub fn remove(&mut self, node: u32) -> bool {
        match self.marks.get_mut(node as usize) {
            Some(mark) if *mark == self.epoch => {
                *mark = 0;
                self.len -= 1;
                self.removed = true;
                true
            }
            _ => false,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Members in insertion order. Only for sets nothing was removed from.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        debug_assert!(!self.removed, "iterating a set with removals");
        self.members.iter().copied()
    }
}

impl Reserved for CellSet {
    fn has(&self, node: u32) -> bool {
        self.contains(node)
    }
}

/// One node.
#[derive(Debug, Clone, Copy)]
pub struct Single(pub u32);

impl Reserved for Single {
    fn has(&self, node: u32) -> bool {
        node == self.0
    }
}

/// A set with one node removed and one added: a cheap copy of a claim set.
pub struct Adjusted<'a, R: Reserved + ?Sized> {
    pub base: &'a R,
    pub without: Option<u32>,
    pub with: Option<u32>,
}

impl<R: Reserved + ?Sized> Reserved for Adjusted<'_, R> {
    fn has(&self, node: u32) -> bool {
        self.with == Some(node) || (self.without != Some(node) && self.base.has(node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_survives_clearing_and_reinsertion() {
        let mut set = CellSet::new(16);
        assert!(set.insert(3) && set.insert(7) && !set.insert(3));
        assert_eq!(set.iter().collect::<Vec<_>>(), [3, 7]);
        assert!(set.remove(3) && !set.contains(3));
        assert!(set.insert(3));
        assert_eq!(set.len(), 2);
        set.reset(16);
        assert!(set.is_empty() && !set.contains(7));
        let adjusted = Adjusted {
            base: &set,
            without: Some(5),
            with: Some(9),
        };
        assert!(adjusted.has(9) && !adjusted.has(5));
    }
}
