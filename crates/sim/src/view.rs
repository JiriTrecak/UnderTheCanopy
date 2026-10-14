//! What one player may know at the current tick: the renderer's and HUD's only input.

use crate::{
    catalog::Catalog,
    entity::{EntityId, Milli},
    state::State,
};
use canopy_content::{Id, placement::Owner};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityView {
    pub id: EntityId,
    pub definition: Id,
    pub owner: Owner,
    pub position: Milli,
    /// Millidegrees.
    pub rotation: i32,
    pub hp: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub tick: u32,
    pub owner: Owner,
    /// In id order.
    pub entities: Vec<EntityView>,
}

impl View {
    pub fn build(state: &State, catalog: &Catalog, owner: Owner) -> Self {
        let entities = state
            .entities
            .iter()
            .map(|e| EntityView {
                id: e.id,
                definition: catalog.id(e.definition).clone(),
                owner: e.owner,
                position: e.position,
                rotation: e.rotation,
                hp: e.hp,
            })
            .collect();
        Self {
            tick: state.tick,
            owner,
            entities,
        }
    }
}
