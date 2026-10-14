//! Known scenery as immutable chunks by id range, so views share it and a renderer
//! re-uploads only the chunks that changed (compare chunk pointers). Derived.

use super::knowledge::Sighting;
use crate::entity::EntityId;
use std::{collections::BTreeSet, sync::Arc};

/// Entity ids per chunk.
pub const CHUNK: u32 = 256;

/// Known scenery in id order, split into chunks of [`CHUNK`] ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenery {
    /// Advances whenever a chunk is replaced.
    pub revision: u32,
    pub chunks: Arc<[Arc<[Sighting]>]>,
}

impl Default for Scenery {
    fn default() -> Self {
        Self {
            revision: 0,
            chunks: Arc::from([]),
        }
    }
}

impl Scenery {
    pub fn iter(&self) -> impl Iterator<Item = &Sighting> {
        self.chunks.iter().flat_map(|c| c.iter())
    }

    pub fn len(&self) -> usize {
        self.chunks.iter().map(|c| c.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.iter().all(|c| c.is_empty())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Chunks {
    pub list: Scenery,
    dirty: BTreeSet<u32>,
}

impl Chunks {
    pub fn touch(&mut self, id: EntityId) {
        self.dirty.insert(id.0 / CHUNK);
    }

    /// Marks every chunk below id `end` for rebuilding.
    pub fn touch_all(&mut self, end: u32) {
        self.dirty.extend(0..end.div_ceil(CHUNK));
    }

    /// Rebuilds the touched chunks from `source(first id, end id)`.
    pub fn rebuild(&mut self, source: impl Fn(EntityId, EntityId) -> Vec<Sighting>) {
        if self.dirty.is_empty() {
            return;
        }
        let mut chunks: Vec<Arc<[Sighting]>> = self.list.chunks.to_vec();
        let mut changed = false;
        for &chunk in &self.dirty {
            let fresh = source(EntityId(chunk * CHUNK), EntityId((chunk + 1) * CHUNK));
            let index = chunk as usize;
            if chunks.len() <= index {
                if fresh.is_empty() {
                    continue;
                }
                chunks.resize_with(index + 1, || Arc::from([]));
            }
            if *chunks[index] != *fresh {
                chunks[index] = Arc::from(fresh);
                changed = true;
            }
        }
        self.dirty.clear();
        while chunks.last().is_some_and(|c| c.is_empty()) {
            chunks.pop();
            changed = true;
        }
        if changed {
            self.list = Scenery {
                revision: self.list.revision + 1,
                chunks: Arc::from(chunks),
            };
        }
    }
}
