//! Stationary entities (buildings and resources) on the fog nodes they cover (old
//! `staticCells`/`staticOverlaps`/`staticRecords`). Derived.

use super::knowledge::Sighting;
use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId, Milli},
    terrain::{
        Terrain,
        footprint::{Extent, footprint_cells},
    },
};
use canopy_content::{game::Kind, placement::Owner};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticRecord {
    pub sighting: Sighting,
    /// Fog nodes of its footprint, in row order.
    pub nodes: Vec<u32>,
    /// Neutral harvestable scenery rather than a structure.
    pub scenery: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Statics {
    /// By id.
    pub records: Vec<StaticRecord>,
    /// Per fog node: the id of one static covering it, 0 for none.
    first: Vec<u32>,
    /// Nodes covered by more than one static: all of them.
    overlaps: BTreeMap<u32, Vec<u32>>,
}

impl Statics {
    pub fn new(nodes: usize) -> Self {
        Self {
            records: Vec::new(),
            first: vec![0; nodes],
            overlaps: BTreeMap::new(),
        }
    }

    pub fn get(&self, id: EntityId) -> Option<&StaticRecord> {
        self.records
            .binary_search_by_key(&id, |r| r.sighting.id)
            .ok()
            .map(|i| &self.records[i])
    }

    /// Calls `visit` with every static covering `node`.
    pub fn at(&self, node: u32, mut visit: impl FnMut(EntityId)) {
        match self.overlaps.get(&node) {
            Some(ids) => ids.iter().for_each(|&id| visit(EntityId(id))),
            None => {
                let id = self.first[node as usize];
                if id != 0 {
                    visit(EntityId(id));
                }
            }
        }
    }

    pub fn place(&mut self, id: EntityId, nodes: &[u32]) {
        for &node in nodes {
            let first = &mut self.first[node as usize];
            if *first == 0 {
                *first = id.0;
            } else {
                self.overlaps
                    .entry(node)
                    .or_insert_with(|| vec![*first])
                    .push(id.0);
            }
        }
    }

    pub fn lift(&mut self, id: EntityId, nodes: &[u32]) {
        for &node in nodes {
            if let Some(ids) = self.overlaps.get_mut(&node) {
                ids.retain(|&other| other != id.0);
                self.first[node as usize] = ids.first().copied().unwrap_or(0);
                if ids.len() < 2 {
                    self.overlaps.remove(&node);
                }
            } else if self.first[node as usize] == id.0 {
                self.first[node as usize] = 0;
            }
        }
    }

    /// Who covers which node, for comparisons (overlap order aside).
    pub fn coverage(&self) -> Vec<(u32, Vec<u32>)> {
        let mut out = Vec::new();
        for (node, &first) in self.first.iter().enumerate() {
            let node = node as u32;
            let mut ids = match self.overlaps.get(&node) {
                Some(ids) => ids.clone(),
                None if first != 0 => vec![first],
                None => continue,
            };
            ids.sort_unstable();
            out.push((node, ids));
        }
        out
    }
}

/// Alive: no health, or some left.
pub fn alive(entity: &Entity) -> bool {
    entity.hp.is_none_or(|hp| hp > 0)
}

/// Buildings and resources that are not units stand still and are remembered under fog.
pub fn stationary(catalog: &Catalog, entity: &Entity) -> bool {
    alive(entity)
        && entity.unit.is_none()
        && (entity.resource.is_some() || catalog.kind(entity.definition) == Kind::Building)
}

/// Neutral harvestable scenery: the forest every player knows the shape of.
pub fn scenery(catalog: &Catalog, entity: &Entity) -> bool {
    entity.owner == Owner::Nobody
        && entity.resource.is_some()
        && entity.unit.is_none()
        && entity.item.is_none()
        && entity.construction.is_none()
        && {
            let d = catalog.get(entity.definition);
            d.kind == Kind::Resource && d.body.is_none()
        }
}

/// Where an entity stands on the fog grid: its cell for units (they occupy one), its
/// placed position for everything else.
pub fn anchor(entity: &Entity) -> Milli {
    if entity.unit.is_some() {
        let (x, y) = entity.position.cell();
        Milli {
            x: x * Milli::CELL,
            y: y * Milli::CELL,
        }
    } else {
        entity.position
    }
}

/// The fog nodes a footprint covers on the map: deck nodes on a deck, ground cells
/// elsewhere (old `Spatial.footprint`, which used ground cells even on decks).
pub fn fog_nodes(
    terrain: &Terrain,
    catalog: &Catalog,
    sighting: &Sighting,
    anchor: Milli,
    out: &mut Vec<u32>,
) {
    out.clear();
    let place = Place {
        definition: sighting.definition,
        anchor,
        rotation: sighting.rotation,
        surface: sighting.surface.as_deref(),
    };
    place.nodes(terrain, catalog, |node| out.push(node));
}

/// Where something stands, for finding its fog nodes.
#[derive(Debug, Clone, Copy)]
pub struct Place<'a> {
    pub definition: crate::catalog::Def,
    pub anchor: Milli,
    /// Millidegrees.
    pub rotation: i32,
    pub surface: Option<&'a str>,
}

impl<'a> Place<'a> {
    pub fn of(entity: &'a Entity) -> Self {
        Self {
            definition: entity.definition,
            anchor: anchor(entity),
            rotation: entity.rotation,
            surface: entity.surface.as_deref(),
        }
    }

    /// Calls `visit` with each fog node of the footprint, in row order.
    pub fn nodes(&self, terrain: &Terrain, catalog: &Catalog, mut visit: impl FnMut(u32)) {
        let deck = self.surface.and_then(|name| terrain.deck_id(name));
        let node = |x: i64, y: i64| {
            let cell = terrain.cell(x, y)? as u32;
            Some(
                deck.and_then(|d| terrain.surfaces()?.node_at(x, y, Some(d)))
                    .unwrap_or(cell),
            )
        };
        let Some(footprint) = catalog.get(self.definition).footprint.as_ref() else {
            // One cell: the one the anchor lies in.
            let (x, y) = self.anchor.cell();
            if let Some(n) = node(i64::from(x), i64::from(y)) {
                visit(n);
            }
            return;
        };
        let bounds = footprint_cells(
            f64::from(self.anchor.x) / 1000.0,
            f64::from(self.anchor.y) / 1000.0,
            Extent::of(Some(footprint)),
            f64::from(self.rotation) / 1000.0,
        );
        for (x, y) in bounds.cells() {
            if let Some(n) = node(x, y) {
                visit(n);
            }
        }
    }
}
