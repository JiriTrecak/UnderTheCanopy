//! The power network (D66): StarCraft's pylon power as content. Buildings with a
//! `network` power the disc of their radius around their centre; buildings with
//! `power: required` are placed only where their owner's network reaches and pause their
//! work while it does not.
//!
//! - **Sources** (a main hall, a well) power their disc whenever they stand complete.
//! - **Nodes** (the beetles' Brood Den) power theirs only while their own centre is
//!   powered, so the network grows outward in a chain from the sources, and losing a
//!   node unpowers everything that hung on it.
//! - A building under construction neither powers nor needs power: construction goes on
//!   outside the network, and the finished building works once power reaches it.
//! - Research adds `networkRadius` cells to the listed buildings' discs.
//!
//! Everything here is derived from the state: the buildings, their completion and health,
//! and each owner's research. It is recomputed only when one of those changes (a
//! signature over the buildings that take part), in id order and integer milli-cells, so
//! every peer and every restored save agrees.

use super::Index;
use crate::{
    catalog::{Catalog, Def},
    entity::{Entity, EntityId},
    state::State,
};
use canopy_content::{
    Id,
    game::{NetworkRole, Power},
    placement::Owner,
};
use std::collections::{BTreeMap, BTreeSet};

/// One powered disc: a source, or a node reached by the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub building: EntityId,
    pub owner: Owner,
    pub role: NetworkRole,
    /// Centre in milli-cells.
    pub x: i32,
    pub y: i32,
    /// Radius in milli-cells.
    pub radius: i64,
}

impl Field {
    /// Whether the disc reaches the point (milli-cells).
    pub fn covers(&self, x: i64, y: i64) -> bool {
        let (dx, dy) = (x - i64::from(self.x), y - i64::from(self.y));
        dx * dx + dy * dy <= self.radius * self.radius
    }
}

/// What the network depends on for one building that takes part: id, definition, owner
/// and whether it stands complete and alive.
type Entry = (EntityId, Def, Owner, bool);

/// The derived power network of every owner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Network {
    /// Buildings that take part, and every owner's research, as of the last computation.
    signature: (Vec<Entry>, BTreeMap<Owner, usize>),
    /// Powered discs, in owner and then id order.
    fields: Vec<Field>,
    /// Complete buildings that need power and have none, in id order.
    unpowered: BTreeSet<EntityId>,
    /// Whether any definition takes part at all (most content has no network).
    used: Option<bool>,
}

impl Network {
    /// Brings the network up to date with the state; a no-op unless a building that takes
    /// part appeared, finished, died, changed definition or an owner finished research.
    pub fn refresh(&mut self, state: &State, catalog: &Catalog, index: &Index) {
        let used = *self.used.get_or_insert_with(|| {
            catalog
                .content
                .definitions
                .iter()
                .any(|d| d.network.is_some() || d.power.is_some())
        });
        if !used {
            return;
        }
        let entries: Vec<Entry> = index
            .buildings
            .iter()
            .filter_map(|&id| state.entities.get(id))
            .filter(|e| e.owner != Owner::Nobody && takes_part(catalog, e))
            .map(|e| (e.id, e.definition, e.owner, standing(e)))
            .collect();
        let research: BTreeMap<Owner, usize> = state
            .economy
            .research
            .iter()
            .map(|(&owner, list)| (owner, list.len()))
            .collect();
        if self.signature.0 == entries && self.signature.1 == research {
            return;
        }
        let _span = tracing::trace_span!("power network").entered();
        self.signature = (entries, research);
        self.compute(state, catalog);
    }

    fn compute(&mut self, state: &State, catalog: &Catalog) {
        self.fields.clear();
        self.unpowered.clear();
        let entries = &self.signature.0;
        let owners: BTreeSet<Owner> = entries.iter().map(|e| e.2).collect();
        for owner in owners {
            let researched = state.economy.researched(owner);
            let mut sources = Vec::new();
            let mut nodes = Vec::new();
            for &(id, def, _, standing) in entries.iter().filter(|e| e.2 == owner) {
                let d = catalog.get(def);
                let (Some(network), true) = (&d.network, standing) else {
                    continue;
                };
                let e = state.entities.get(id).expect("network building");
                let cells = i64::from(network.radius) + bonus(catalog, researched, &d.id);
                let field = Field {
                    building: id,
                    owner,
                    role: network.role,
                    x: e.position.x,
                    y: e.position.y,
                    radius: cells * 1000,
                };
                match network.role {
                    NetworkRole::Source => sources.push(field),
                    NetworkRole::Node => nodes.push(field),
                }
            }
            // Nodes join once a powered disc reaches their centre, until none is left
            // to join: the fixed point does not depend on the order, the result is kept
            // in id order.
            let mut powered = sources;
            let mut joined = true;
            while joined {
                joined = false;
                nodes.retain(|node| {
                    let reached = powered
                        .iter()
                        .any(|f| f.covers(i64::from(node.x), i64::from(node.y)));
                    if reached {
                        powered.push(*node);
                        joined = true;
                    }
                    !reached
                });
            }
            powered.sort_by_key(|f| f.building);
            for &(id, def, _, standing) in entries.iter().filter(|e| e.2 == owner) {
                let d = catalog.get(def);
                if !standing || d.power != Some(Power::Required) {
                    continue;
                }
                // Sources power themselves; nodes take part like any other building.
                if d.network
                    .as_ref()
                    .is_some_and(|n| n.role == NetworkRole::Source)
                {
                    continue;
                }
                let e = state.entities.get(id).expect("network building");
                let (x, y) = (i64::from(e.position.x), i64::from(e.position.y));
                if !powered.iter().any(|f| f.covers(x, y)) {
                    self.unpowered.insert(id);
                }
            }
            self.fields.extend(powered);
        }
    }

    /// Whether `owner`'s network reaches the point (milli-cells).
    pub fn covers(&self, owner: Owner, x: i64, y: i64) -> bool {
        self.fields(owner).any(|f| f.covers(x, y))
    }

    /// `owner`'s powered discs, in id order.
    pub fn fields(&self, owner: Owner) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(move |f| f.owner == owner)
    }

    /// Every powered disc, by owner and then id.
    pub fn all_fields(&self) -> &[Field] {
        &self.fields
    }

    /// Whether a building that needs power is complete and outside its owner's network:
    /// its work pauses.
    pub fn unpowered(&self, id: EntityId) -> bool {
        self.unpowered.contains(&id)
    }

    /// Every such building, in id order.
    pub fn unpowered_buildings(&self) -> &BTreeSet<EntityId> {
        &self.unpowered
    }
}

fn takes_part(catalog: &Catalog, e: &Entity) -> bool {
    let d = catalog.get(e.definition);
    d.network.is_some() || d.power.is_some()
}

/// Complete and alive.
fn standing(e: &Entity) -> bool {
    e.construction.is_none() && super::alive(e)
}

/// Per owner, the name of the cheapest constructable node of its race, for the message a
/// refused placement gives ("extend it with a Brood Den").
pub fn hints(catalog: &Catalog, setup: &crate::setup::MatchSetup) -> BTreeMap<Owner, String> {
    use canopy_content::game::{Creation, Kind};
    let rules = &catalog.content.rules;
    let cheapest = |roster: Option<&Vec<Id>>| {
        catalog
            .content
            .definitions
            .iter()
            .filter(|d| {
                d.kind == Kind::Building
                    && d.network
                        .as_ref()
                        .is_some_and(|n| n.role == NetworkRole::Node)
                    && matches!(d.creation, Some(Creation::Construct { .. }))
                    && roster.is_none_or(|r| r.contains(&d.id))
            })
            .min_by_key(|d| {
                let cost: u32 = d
                    .creation
                    .iter()
                    .flat_map(|c| c.items())
                    .map(|c| c.amount)
                    .sum();
                (cost, d.id.clone())
            })
    };
    let mut out = BTreeMap::new();
    for slot in &setup.slots {
        let race = slot.race.as_deref().unwrap_or(&rules.default_race);
        let roster = rules.races.get(race).map(|r| &r.roster);
        // A seat playing buildings of another race (a sandbox, a mission) still gets
        // a name: any race's cheapest node.
        if let Some(d) = cheapest(roster).or_else(|| cheapest(None)) {
            out.insert(Owner::Player(slot.player), d.id.to_string());
        }
    }
    out
}

/// The refusal for a building placed outside the network.
pub fn outside(hint: Option<&str>) -> String {
    match hint {
        Some(node) => crate::msg!(
            "message.build-inside-network-with",
            node = canopy_content::text::name(node)
        ),
        None => "message.build-inside-network".into(),
    }
}

/// Cells `owner`'s research adds to `definition`'s network radius.
fn bonus(catalog: &Catalog, researched: &[Id], definition: &Id) -> i64 {
    researched
        .iter()
        .filter_map(|r| catalog.content.rules.research.get(r))
        .flat_map(|r| &r.effects)
        .filter(|e| e.units.contains(definition))
        .map(|e| i64::from(e.network_radius.unwrap_or(0)))
        .sum()
}
