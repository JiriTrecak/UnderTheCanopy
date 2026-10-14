//! Definitions compiled once into simulation form, addressed by a small index instead of
//! string ids on hot paths.

use canopy_content::{
    Content, Id,
    game::{Definition, Kind},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Index of a definition in [`Catalog::definitions`] (sorted by id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Def(pub u16);

/// The loaded content plus lookups the simulation needs.
#[derive(Debug)]
pub struct Catalog {
    pub content: Arc<Content>,
}

impl Catalog {
    pub fn new(content: Arc<Content>) -> Self {
        assert!(
            content.definitions.len() <= usize::from(u16::MAX),
            "too many definitions"
        );
        Self { content }
    }

    pub fn find(&self, id: &str) -> Option<Def> {
        self.content
            .definitions
            .binary_search_by(|d| d.id.as_str().cmp(id))
            .ok()
            .map(|index| Def(index as u16))
    }

    pub fn get(&self, def: Def) -> &Definition {
        &self.content.definitions[usize::from(def.0)]
    }

    pub fn id(&self, def: Def) -> &Id {
        &self.get(def).id
    }

    pub fn kind(&self, def: Def) -> Kind {
        self.get(def).kind
    }
}
