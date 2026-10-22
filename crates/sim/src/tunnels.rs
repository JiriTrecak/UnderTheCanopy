//! Tunnels (the ants' signature, docs/ants.md): every building with a `tunnel` is a mound,
//! and its owner's units travel underground between mounds when that is faster than
//! walking.
//!
//! A unit with a move order that is about to route ([`steer`]) compares walking with a
//! trip from the mound nearest it to the mound nearest its destination; if the trip wins
//! by a margin it walks to the first mound instead. At the mound's mouth it queues: a
//! mound lets one unit in every `interval_ticks` (research widens it). Underground the
//! unit is contained by the mound (hidden, untargetable and blind, like any contained
//! unit) for the distance between the mounds at the tunnel's `speed`, then comes up at the
//! other mound's mouth and walks on to its destination with its order unchanged. A mound
//! that falls takes the trips to it with it: units walking or queued to it walk on, and
//! units underground headed to it come up where it stood.

use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId, Milli, Place},
    orders::Order,
    spatial::{Spatial, Spot},
    state::State,
};
use canopy_content::{Id, placement::Owner};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How close to the mound's mouth a unit must come to go under, in cells; a body broader
/// than that (a hero) only has to come within two cells of its own radius, since it cannot
/// stand nearer the mound.
const MOUTH: i64 = 3;
/// A trip must save this much time over walking to be worth it, in ticks.
const MARGIN_TICKS: i64 = 80;
/// Slack beyond twice the straight walk to the mouth and the queue before a unit that has
/// not gone under gives its trip up, in ticks.
const GIVE_UP_TICKS: i64 = 400;
/// Ticks per second.
const SECOND: i64 = 40;

/// A mound works (D65: its animation and effects) this long after a unit went under or
/// came up there, in ticks.
const TRAFFIC_TICKS: u32 = 80;

/// The mounds' traffic (`State::tunnels`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Tunnels {
    /// When each mound lets its next unit under.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub gates: BTreeMap<EntityId, u32>,
    /// When a unit last went under or came up at each mound.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub traffic: BTreeMap<EntityId, u32>,
}

impl Tunnels {
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty() && self.traffic.is_empty()
    }

    /// Whether ants are passing through `mound`: it works while they do.
    pub fn busy(&self, mound: EntityId, tick: u32) -> bool {
        self.traffic
            .get(&mound)
            .is_some_and(|&t| t + TRAFFIC_TICKS >= tick)
    }
}

/// A unit's journey through the tunnels (`Unit::tunnel`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trip {
    pub entry: EntityId,
    pub exit: EntityId,
    /// Underground, coming up at the exit on this tick; `None` while walking to the
    /// entry or queued at its mouth.
    pub arrives: Option<u32>,
    /// The tick by which it should have gone under: a unit still short of the mouth then
    /// (a crowd stands in front of it) gives the trip up and walks.
    #[serde(default)]
    pub until: u32,
}

fn mound<'a>(catalog: &'a Catalog, e: &Entity) -> Option<&'a canopy_content::game::Tunnel> {
    if e.hp.is_some_and(|hp| hp == 0) || e.construction.is_some() {
        return None;
    }
    catalog.get(e.definition).tunnel.as_ref()
}

/// The mounds `owner` can use now, in id order.
fn mounds(state: &State, catalog: &Catalog, owner: Owner) -> Vec<EntityId> {
    state
        .entities
        .iter()
        .filter(|e| e.owner == owner && e.unit.is_none() && mound(catalog, e).is_some())
        .map(|e| e.id)
        .collect()
}

/// A mound's mouth: its entrance, in milli-cells.
fn mouth(catalog: &Catalog, host: &Entity) -> Milli {
    let p = crate::garrisons::entrance(catalog, host);
    Milli {
        x: p.x * 1000,
        y: p.y * 1000,
    }
}

fn distance(a: Milli, b: Milli) -> f64 {
    let (dx, dy) = (f64::from(a.x - b.x), f64::from(a.y - b.y));
    (dx * dx + dy * dy).sqrt() / 1000.0
}

/// Whether `e` may travel underground at all: a ground unit its definition allows.
fn traveller(catalog: &Catalog, spatial: &Spatial, e: &Entity) -> bool {
    let d = catalog.get(e.definition);
    e.unit
        .as_ref()
        .is_some_and(|u| u.contained.is_none() && u.release.is_none() && u.garrison.is_none())
        && d.underground != Some(false)
        && !spatial.profiles.get(e.definition).body.air
}

/// Ticks between two units going under at `host`, after `owner`'s research.
fn interval(state: &State, catalog: &Catalog, host: &Entity) -> u32 {
    let Some(t) = mound(catalog, host) else {
        return u32::MAX;
    };
    let id: &Id = catalog.id(host.definition);
    let bonus: u32 = state
        .economy
        .researched(host.owner)
        .iter()
        .filter_map(|r| catalog.content.rules.research.get(r))
        .flat_map(|r| &r.effects)
        .filter(|e| e.units.contains(id))
        .map(|e| e.tunnel_throughput_permille.unwrap_or(0))
        .sum();
    (u64::from(t.interval_ticks) * 1000 / (1000 + u64::from(bonus))).max(1) as u32
}

/// Ticks underground from `entry` to `exit`.
fn transit(catalog: &Catalog, entry: &Entity, exit: &Entity) -> u32 {
    let speed = mound(catalog, entry).map_or(1, |t| t.speed.max(1));
    (distance(entry.position, exit.position) * SECOND as f64 / f64::from(speed)).ceil() as u32
        + SECOND as u32
}

/// Before `id` routes towards `destination` (cells): starts a tunnel trip when one is
/// faster than walking. Returns where to walk instead: the entry mound's mouth. Only mounds
/// on the ground each leg walks count: the entry where the unit can walk to its mouth, the
/// exit where its mouth can walk to the destination (a mound behind a closed gate is no
/// way in or out).
pub fn steer(
    state: &mut State,
    catalog: &Catalog,
    spatial: &mut Spatial,
    id: EntityId,
    destination: &Place,
) -> Option<Place> {
    let e = state.entities.get(id)?;
    if let Some(trip) = e.unit.as_ref().and_then(|u| u.tunnel.as_ref()) {
        let entry = state.entities.get(trip.entry)?;
        let m = mouth(catalog, entry);
        return Some(Place::new(m.x / 1000, m.y / 1000));
    }
    if !traveller(catalog, spatial, e) || destination.surface.is_some() {
        return None;
    }
    let speed = spatial.profiles.get(e.definition).motion?.speed;
    if speed <= 0.0 {
        return None;
    }
    let target = Milli {
        x: destination.x * 1000,
        y: destination.y * 1000,
    };
    let here = spatial.entity_node(e)?;
    let goal = spatial.node(Spot::new(destination.x, destination.y, None))?;
    // Each mound with its mouth and the node there, in id order.
    let owned: Vec<(&Entity, Milli, u32)> = mounds(state, catalog, e.owner)
        .iter()
        .filter_map(|&m| state.entities.get(m))
        .filter_map(|m| {
            let at = mouth(catalog, m);
            let node = spatial.node(Spot::new(at.x / 1000, at.y / 1000, None))?;
            Some((m, at, node))
        })
        .collect();
    let nearest = |p: Milli, spatial: &mut Spatial, joins: &dyn Fn(&mut Spatial, u32) -> bool| {
        let mut best: Option<(f64, &Entity)> = None;
        for &(m, at, node) in &owned {
            let d = distance(at, p);
            if best.is_some_and(|(b, other)| (b, other.id) <= (d, m.id)) || !joins(spatial, node) {
                continue;
            }
            best = Some((d, m));
        }
        best.map(|(_, m)| m)
    };
    let entry = nearest(e.position, spatial, &|s, node| s.connected(here, node))?;
    let exit = nearest(target, spatial, &|s, node| s.connected(node, goal))?;
    if entry.id == exit.id {
        return None;
    }
    let walk = |a: Milli, b: Milli| (distance(a, b) * SECOND as f64 / speed) as i64;
    let queued = state
        .entities
        .iter()
        .filter(|o| {
            o.unit
                .as_ref()
                .and_then(|u| u.tunnel.as_ref())
                .is_some_and(|t| t.entry == entry.id && t.arrives.is_none())
        })
        .count() as i64;
    let approach = walk(e.position, mouth(catalog, entry));
    let queue = queued * i64::from(interval(state, catalog, entry));
    let through = approach
        + queue
        + i64::from(transit(catalog, entry, exit))
        + walk(mouth(catalog, exit), target);
    if through + MARGIN_TICKS >= walk(e.position, target) {
        return None;
    }
    let trip = Trip {
        entry: entry.id,
        exit: exit.id,
        arrives: None,
        until: state.tick + (2 * approach + queue + GIVE_UP_TICKS) as u32,
    };
    let m = mouth(catalog, entry);
    state.entities.get_mut(id)?.unit.as_mut()?.tunnel = Some(trip);
    Some(Place::new(m.x / 1000, m.y / 1000))
}

/// How far a new mound should stand from the owner's others to be worth founding, in
/// cells.
const FRONTIER: f64 = 64.0;

/// Whether `owner` has no tunnel mound within [`FRONTIER`] of `at`: a computer player
/// founds a mound there.
pub fn frontier(state: &State, catalog: &Catalog, owner: Owner, at: Milli) -> bool {
    mounds(state, catalog, owner)
        .iter()
        .filter_map(|&m| state.entities.get(m))
        .all(|m| distance(m.position, at) > FRONTIER)
}

/// Whether a move order is walking to a tunnel mound rather than to its destination.
pub fn approaching(e: &Entity) -> bool {
    e.unit
        .as_ref()
        .and_then(|u| u.tunnel.as_ref())
        .is_some_and(|t| t.arrives.is_none())
}

/// Each tick: units at a mouth go under when the mound lets them, and units whose time
/// underground is up come out.
pub fn tick(state: &mut State, catalog: &Catalog, spatial: &mut Spatial) {
    let tick = state.tick;
    let travellers: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| e.unit.as_ref().is_some_and(|u| u.tunnel.is_some()))
        .map(|e| e.id)
        .collect();
    for id in travellers {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        let trip = e
            .unit
            .as_ref()
            .and_then(|u| u.tunnel.clone())
            .expect("trip");
        let moving = matches!(
            e.unit.as_ref().and_then(|u| u.order.as_ref()),
            Some(Order::Move { .. })
        );
        let (entry, exit) = (
            state.entities.get(trip.entry),
            state.entities.get(trip.exit),
        );
        let usable = |m: Option<&Entity>| {
            m.is_some_and(|m| m.owner == e.owner && mound(catalog, m).is_some())
        };
        match trip.arrives {
            None => {
                // Walking or queued: a lost order or mound ends the trip.
                if !moving || !usable(entry) || !usable(exit) {
                    clear(state, id);
                    continue;
                }
                let (entry, exit) = (entry.expect("entry"), exit.expect("exit"));
                let reach =
                    (MOUTH as f64).max(spatial.profiles.get(e.definition).body.radius + 2.0);
                let near = distance(e.position, mouth(catalog, entry)) <= reach;
                let open = state
                    .tunnels
                    .gates
                    .get(&entry.id)
                    .is_none_or(|&t| t <= tick);
                if near && open {
                    let arrives = tick + transit(catalog, entry, exit);
                    let next = tick + interval(state, catalog, entry);
                    let (entry_id, position) = (entry.id, entry.position);
                    state.tunnels.gates.insert(entry_id, next);
                    state.tunnels.traffic.insert(entry_id, tick);
                    go_under(state, spatial, id, entry_id, position, arrives);
                } else if !near && tick > trip.until {
                    give_up(state, spatial, id);
                }
            }
            Some(arrives) if arrives <= tick => {
                let at = match exit.filter(|m| usable(Some(m))) {
                    Some(m) => {
                        let at = mouth(catalog, m);
                        state.tunnels.traffic.insert(m.id, tick);
                        at
                    }
                    // The exit fell: come up where it stood.
                    None => exit.or(entry).map_or(e.position, |m| m.position),
                };
                come_up(state, spatial, id, at);
            }
            Some(_) => {}
        }
    }
    let entities = &state.entities;
    state
        .tunnels
        .gates
        .retain(|&g, &mut t| t >= tick && entities.get(g).is_some());
    state
        .tunnels
        .traffic
        .retain(|&m, &mut t| t + TRAFFIC_TICKS >= tick && entities.get(m).is_some());
}

fn clear(state: &mut State, id: EntityId) {
    if let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
        u.tunnel = None;
        crate::orders::discard_navigation(u);
        u.retry_at = 0;
    }
}

/// A trip whose mouth the unit has not reached in time ends, and the unit walks to its
/// destination instead.
fn give_up(state: &mut State, spatial: &mut Spatial, id: EntityId) {
    clear(state, id);
    let e = state.entities.get_mut(id).expect("traveller");
    if let Some(Order::Move { destination, .. }) = e.unit.as_ref().and_then(|u| u.order.clone())
        && destination.surface.is_none()
    {
        spatial.route_default(e, Spot::new(destination.x, destination.y, None));
    }
}

fn go_under(
    state: &mut State,
    spatial: &mut Spatial,
    id: EntityId,
    entry: EntityId,
    at: Milli,
    arrives: u32,
) {
    let e = state.entities.get_mut(id).expect("traveller");
    e.position = at;
    let u = e.unit.as_mut().expect("unit");
    crate::orders::discard_navigation(u);
    crate::orders::release_combat(u);
    u.contained = Some(entry);
    if let Some(t) = &mut u.tunnel {
        t.arrives = Some(arrives);
    }
    spatial.update_body(e);
}

fn come_up(state: &mut State, spatial: &mut Spatial, id: EntityId, at: Milli) {
    let tick = state.tick;
    let e = state.entities.get_mut(id).expect("traveller");
    let u = e.unit.as_mut().expect("unit");
    u.contained = None;
    u.tunnel = None;
    u.release = Some(Place::new(at.x / 1000, at.y / 1000));
    u.retry_at = tick;
    crate::garrisons::settle(spatial, e);
}

/// A mound leaves the match: trips through it end; units underground headed to it come
/// up where it stood (their entry mound may stand far behind them).
pub fn collapse(state: &mut State, spatial: &mut Spatial, host: EntityId) {
    let Some(at) = state.entities.get(host).map(|h| h.position) else {
        return;
    };
    state.tunnels.gates.remove(&host);
    state.tunnels.traffic.remove(&host);
    let affected: Vec<(EntityId, bool)> = state
        .entities
        .iter()
        .filter_map(|e| {
            let t = e.unit.as_ref()?.tunnel.as_ref()?;
            (t.entry == host || t.exit == host)
                .then_some((e.id, t.arrives.is_some() && t.exit == host))
        })
        .collect();
    for (id, underground) in affected {
        if underground {
            come_up(state, spatial, id, at);
        } else if !state
            .entities
            .get(id)
            .is_some_and(|e| e.unit.as_ref().is_some_and(|u| u.contained.is_some()))
        {
            clear(state, id);
        }
    }
}
