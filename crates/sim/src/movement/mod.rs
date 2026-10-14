//! Unit movement (old `GameContext.move`/`moveUnits`, `moveDetour`, `beginLocalDetour`,
//! `beginTrafficYield`, `askIdleAllyToYield` in `src/sim/game/context.ts`, plus
//! `separation.ts` and `idleMotion.ts`).
//!
//! Each tick every unit with a route turns towards its next waypoint (it must face it
//! before moving), then advances by its speed budget along the route in fixed point. Every
//! step must clear the terrain sweep, the cells claimed by other movers, and other bodies
//! (swept circles; separating motion is always allowed). A blocked mover first asks for
//! deadlock negotiation, then tries a bounded local detour, then asks an idle ally to step
//! aside, then waits and retries.

pub mod facing;
pub mod formation;
pub mod idle;
pub mod local_path;
pub mod separation;
pub mod traffic;

use crate::{
    catalog::Catalog,
    entity::{Detour, Entity, EntityId, Milli, Segment, Yielding},
    navigation::cells::{Adjusted, CellSet, Reserved},
    orders::Order,
    spatial::{Body, Fixed, Obstruction, Spatial, Spot, geometry::length_ceil},
    state::State,
    teams::Teams,
    terrain::round_half_up,
};
use facing::{turn_step, turn_toward};
use local_path::{LocalScratch, local_path};
use std::{collections::BTreeMap, sync::Arc};
use traffic::Request;

/// A unit as movement queries see it.
#[derive(Debug, Clone, Copy)]
pub struct Mover {
    pub id: EntityId,
    pub at: Fixed,
    pub cell: Spot,
    pub node: u32,
    pub body: Body,
}

impl Mover {
    pub fn of(spatial: &Spatial, e: &Entity) -> Self {
        let deck = spatial.deck_of(e);
        let (x, y) = e.position.cell();
        let cell = Spot::new(x, y, deck);
        Self {
            id: e.id,
            at: Fixed::on(e.position.x, e.position.y, deck),
            cell,
            node: spatial.node(cell).unwrap_or(u32::MAX),
            body: spatial.body_of(e),
        }
    }
}

/// Moves `e` to a fixed point (and its floor).
fn place(spatial: &Spatial, e: &mut Entity, p: Fixed) {
    e.position = Milli { x: p.x, y: p.y };
    if spatial.deck_of(e) != p.deck {
        e.surface = p.deck.map(|d| spatial.deck_name(d));
    }
}

fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
}

fn ready(e: &Entity, tick: u32) -> bool {
    alive(e) && e.ready_tick <= tick
}

/// Whether `b` claims cells against `e`'s local search (old `localReservations`): moving
/// bodies and enemies do; stationary friends only collide physically.
fn local_claim(
    teams: &Teams,
    units: &[EntityId],
    tick: u32,
    e: &Mover,
    owner: canopy_content::placement::Owner,
    b: &crate::spatial::BodyRecord,
) -> bool {
    b.id != e.id
        && units.binary_search(&b.id).is_ok()
        && b.ready_tick <= tick
        && b.solid()
        && b.air == e.body.air
        && (!teams.allied(b.owner, owner) || b.has_route)
}

/// Live cell claims for a local search: bodies standing in a node or reserving it.
fn reservations<'a>(
    teams: &'a Teams,
    spatial: &'a Spatial,
    units: &'a [EntityId],
    tick: u32,
    e: &'a Mover,
    owner: canopy_content::placement::Owner,
) -> impl Fn(u32) -> bool + 'a {
    move |node: u32| {
        let s = spatial.spot(node);
        spatial
            .bodies
            .in_cell(s.x, s.y)
            .any(|b| b.node == node && local_claim(teams, units, tick, e, owner, b))
            || spatial
                .bodies
                .reserved_at(node)
                .any(|b| local_claim(teams, units, tick, e, owner, b))
    }
}

/// Integer `round(a · b / c)` with halves up, for `c > 0`.
fn scale(a: i64, b: i64, c: i64) -> i64 {
    (2 * a * b + c).div_euclid(2 * c)
}

/// Movement and spatial state that is derived, never saved: the spatial model and the
/// reusable scratch of the movement passes.
pub struct Motion {
    pub spatial: Spatial,
    /// Who cooperates in traffic: same-team units negotiate and yield to each other.
    teams: Teams,
    units: Vec<EntityId>,
    ground: CellSet,
    air: CellSet,
    claims: CellSet,
    local: LocalScratch,
}

impl Motion {
    pub fn new(
        terrain: Arc<crate::terrain::Terrain>,
        catalog: &Catalog,
        state: &State,
        teams: Teams,
        scales: BTreeMap<String, f64>,
    ) -> Self {
        let spatial = Spatial::new(terrain, catalog, state, scales);
        let n = spatial.node_count() as usize;
        Self {
            spatial,
            teams,
            units: Vec::new(),
            ground: CellSet::new(n),
            air: CellSet::new(n),
            claims: CellSet::new(n),
            local: LocalScratch::default(),
        }
    }

    /// Ready units physically on the map, in id order (old `activeUnits`).
    fn collect_active(&mut self, state: &State) {
        self.units.clear();
        let tick = state.tick;
        self.units.extend(
            state
                .entities
                .iter()
                .filter(|e| ready(e, tick) && crate::spatial::present(e))
                .map(|e| e.id),
        );
    }

    /// Step 5 movement (old `GameContext.move`): advances every unit along its route.
    pub fn move_units(&mut self, state: &mut State) {
        let _span = tracing::trace_span!("movement").entered();
        self.spatial.sync_bodies(state);
        self.collect_active(state);
        let n = self.spatial.node_count() as usize;
        self.ground.reset(n);
        self.air.reset(n);
        for &id in &self.units {
            let Some(b) = self.spatial.bodies.get(id) else {
                continue;
            };
            if b.ignores {
                continue;
            }
            let set = if b.air {
                &mut self.air
            } else {
                &mut self.ground
            };
            set.insert(b.node);
            if let Some(w) = b.reservation {
                set.insert(w);
            }
        }
        let live: Vec<EntityId> = state
            .entities
            .iter()
            .filter(|e| e.unit.is_some() && alive(e))
            .map(|e| e.id)
            .collect();
        let mut requests: Option<BTreeMap<EntityId, Request>> = None;
        for id in live {
            self.move_unit(state, id, &mut requests);
        }
    }

    fn move_unit(
        &mut self,
        state: &mut State,
        id: EntityId,
        requests: &mut Option<BTreeMap<EntityId, Request>>,
    ) {
        let tick = state.tick;
        let spatial = &mut self.spatial;
        let Some(e) = state.entities.get_mut(id) else {
            return;
        };
        let air = spatial.body_of(e).air;
        {
            let u = e.unit.as_mut().expect("unit");
            if let Some(d) = &u.detour
                && (u.goal != Some(d.goal) || u.route.first() != Some(&d.waypoint))
            {
                u.detour = None;
            }
        }
        if let Some(release) = e.unit.as_ref().and_then(|u| u.release.clone()) {
            let deck = release.surface.as_deref().and_then(|s| spatial.deck_id(s));
            let body = spatial.body_of(e);
            if let Some(p) = spatial.nearest(
                Spot::new(release.x, release.y, deck),
                12,
                Some(id),
                Some(&body),
                None,
                None,
            ) {
                place(spatial, e, p.fixed());
                let u = e.unit.as_mut().expect("unit");
                u.segment = None;
                u.release = None;
                spatial.update_body(e);
                let b = spatial.bodies.get(id).copied();
                if let Some(b) = b
                    && !b.ignores
                {
                    let set = if air { &mut self.air } else { &mut self.ground };
                    set.insert(b.node);
                }
            }
            return;
        }
        if !ready(e, tick) || e.unit.as_ref().is_some_and(|u| u.contained.is_some()) {
            return;
        }
        let profile = spatial.profiles.get(e.definition);
        let motion = profile.motion;
        let step = turn_step(motion.map_or(720, |m| m.turn_rate));
        let mover = Mover::of(spatial, e);
        let here = (
            f64::from(e.position.x) / 1000.0,
            f64::from(e.position.y) / 1000.0,
        );
        // Idle units keep facing their target.
        let target = e
            .unit
            .as_ref()
            .and_then(|u| if u.route.is_empty() { u.target } else { None });
        let victim = target
            .and_then(|t| state.entities.get(t))
            .filter(|v| alive(v))
            .map(|v| {
                (
                    f64::from(v.position.x) / 1000.0,
                    f64::from(v.position.y) / 1000.0,
                )
            });
        let e = state.entities.get_mut(id).expect("unit");
        if let Some(victim) = victim {
            turn_toward(&mut e.rotation, here, victim, step);
        }
        let u = e.unit.as_ref().expect("unit");
        let speed = match motion {
            Some(m) if u.idle.as_ref().is_some_and(|i| i.walking) => {
                m.walk_speed.unwrap_or(m.speed)
            }
            Some(m) => m.speed,
            None => 0.0,
        };
        if speed <= 0.0 || u.route.is_empty() {
            return;
        }
        let ignores = spatial.bodies.get(id).is_some_and(|b| b.ignores);
        let yielding_waypoint = u
            .detour
            .as_ref()
            .filter(|d| d.yielding.is_some())
            .map(|d| d.waypoint);
        {
            let set = if air { &mut self.air } else { &mut self.ground };
            if !ignores {
                set.remove(mover.node);
                if let Some(w) = yielding_waypoint {
                    set.remove(w);
                }
            }
        }
        let budget = ((speed * 1000.0 * 1000.0 * 1000.0) / 40_000_000.0).floor() as i64;
        if e.unit.as_ref().is_some_and(|u| u.detour.is_some()) {
            self.move_detour(state, id, budget, step);
        } else {
            self.follow_route(state, id, budget, step, ignores, requests);
        }
        // Re-register the body and its claims.
        let e = state.entities.get(id).expect("unit");
        self.spatial.update_body(e);
        if !ignores {
            let b = *self.spatial.bodies.get(id).expect("body");
            let set = if air { &mut self.air } else { &mut self.ground };
            set.insert(b.node);
            if let Some(w) = b.reservation {
                set.insert(w);
            }
        }
    }

    fn follow_route(
        &mut self,
        state: &mut State,
        id: EntityId,
        mut budget: i64,
        step: f64,
        ignores: bool,
        requests: &mut Option<BTreeMap<EntityId, Request>>,
    ) {
        let tick = state.tick;
        let spatial = &self.spatial;
        let e = state.entities.get_mut(id).expect("unit");
        let body = spatial.body_of(e);
        let deck = spatial.deck_of(e);
        // Retry and yield routes may begin exactly where the unit stands.
        {
            let u = e.unit.as_mut().expect("unit");
            while let Some(&first) = u.route.first() {
                let anchor = spatial.point(first);
                if anchor.x != e.position.x || anchor.y != e.position.y || anchor.deck != deck {
                    break;
                }
                u.route.remove(0);
                u.segment = None;
            }
            if u.route.is_empty() {
                return;
            }
        }
        let first = spatial.point(e.unit.as_ref().expect("unit").route[0]);
        let here = (
            f64::from(e.position.x) / 1000.0,
            f64::from(e.position.y) / 1000.0,
        );
        if !turn_toward(
            &mut e.rotation,
            here,
            (f64::from(first.x) / 1000.0, f64::from(first.y) / 1000.0),
            step,
        ) {
            return;
        }
        let air = body.air;
        while budget > 0 {
            let e = state.entities.get_mut(id).expect("unit");
            let current = Fixed::on(e.position.x, e.position.y, self.spatial.deck_of(e));
            let u = e.unit.as_mut().expect("unit");
            let Some(&next) = u.route.first() else {
                break;
            };
            let goal = self.spatial.point(next);
            if u.segment.as_ref().is_none_or(|s| s.to != next) {
                let length =
                    length_ceil(i64::from(goal.x - current.x), i64::from(goal.y - current.y));
                if length == 0 {
                    u.route.remove(0);
                    u.segment = None;
                    continue;
                }
                u.segment = Some(Segment {
                    from: Milli {
                        x: current.x,
                        y: current.y,
                    },
                    to: next,
                    length: length as u32,
                    progress: 0,
                });
            }
            let segment = u.segment.clone().expect("segment");
            let distance = i64::from(segment.length - segment.progress);
            let travel = budget.min(distance);
            let progress = i64::from(segment.progress) + travel;
            let length = i64::from(segment.length);
            let mut proposed = if progress == length {
                goal
            } else {
                Fixed::on(
                    segment.from.x
                        + scale(i64::from(goal.x - segment.from.x), progress, length) as i32,
                    segment.from.y
                        + scale(i64::from(goal.y - segment.from.y), progress, length) as i32,
                    current.deck,
                )
            };
            self.spatial.adopt_surface(current, &mut proposed, goal);
            let occupied: &CellSet = if air { &self.air } else { &self.ground };
            let obstruction = self.spatial.movement_blocker(
                current,
                proposed,
                (!ignores).then_some(occupied as &dyn Reserved),
                &body,
            );
            if obstruction == Some(Obstruction::Terrain) {
                let u = state
                    .entities
                    .get_mut(id)
                    .expect("unit")
                    .unit
                    .as_mut()
                    .expect("unit");
                u.route.clear();
                u.segment = None;
                u.retry_at = tick + 6;
                break;
            }
            if obstruction == Some(Obstruction::Reservation)
                || !self
                    .spatial
                    .unit_segment_clear(current, proposed, Some(id), None, None, None)
            {
                let (retry_at, goal_node) = {
                    let u = state
                        .entities
                        .get(id)
                        .expect("unit")
                        .unit
                        .as_ref()
                        .expect("unit");
                    (u.retry_at, u.goal)
                };
                if tick >= retry_at && goal_node.is_some() {
                    self.recover(state, id, current, proposed, requests);
                }
                break;
            }
            let e = state.entities.get_mut(id).expect("unit");
            let u = e.unit.as_mut().expect("unit");
            if proposed.x != current.x || proposed.y != current.y {
                u.last_moved_tick = Some(tick);
            }
            if let Some(s) = u.segment.as_mut() {
                s.progress = progress as u32;
            }
            let arrived = travel == distance;
            if arrived {
                u.route.remove(0);
                u.segment = None;
            }
            place(&self.spatial, e, proposed);
            budget -= travel;
            if arrived {
                break;
            }
        }
    }

    /// A blocked mover's recovery: yield negotiation, local detour, idle ally, side step.
    fn recover(
        &mut self,
        state: &mut State,
        id: EntityId,
        current: Fixed,
        proposed: Fixed,
        requests: &mut Option<BTreeMap<EntityId, Request>>,
    ) {
        let tick = state.tick;
        if requests.is_none() {
            let _span = tracing::trace_span!("traffic requests").entered();
            *requests = Some(traffic::requests(
                &self.spatial,
                &self.teams,
                state,
                &self.units,
                &mut self.local,
                &mut self.claims,
            ));
        }
        if let Some(&request) = requests.as_ref().and_then(|r| r.get(&id))
            && self.begin_traffic_yield(state, id, request)
        {
            return;
        }
        let e = state.entities.get(id).expect("unit");
        let u = e.unit.as_ref().expect("unit");
        let desired = self.spatial.spot(u.goal.expect("goal"));
        let arrival = match (&u.order, u.target) {
            (Some(Order::Move { destination, .. }), None) => Spot::new(
                destination.x,
                destination.y,
                destination
                    .surface
                    .as_deref()
                    .and_then(|s| self.spatial.deck_id(s)),
            ),
            _ => desired,
        };
        let (dx, dy) = (
            f64::from(desired.x) - f64::from(current.x) / 1000.0,
            f64::from(desired.y) - f64::from(current.y) / 1000.0,
        );
        let body = self.spatial.body_of(e);
        let target = if (dx * dx + dy * dy).sqrt() <= 7.0 {
            self.spatial
                .nearest(arrival, 3, Some(id), Some(&body), None, None)
        } else {
            None
        };
        if self.begin_local_detour(state, id, target) {
            return;
        }
        if self.ask_idle_ally_to_yield(state, id, current, proposed) {
            return;
        }
        // Stable yielding lets opposing friendly traffic pass without teleports.
        let e = state.entities.get(id).expect("unit");
        let (cx, cy) = e.position.cell();
        let owner = e.owner;
        let near = self.units.iter().any(|&b| {
            b < id
                && self.spatial.bodies.get(b).is_some_and(|r| {
                    self.teams.allied(r.owner, owner)
                        && r.has_route
                        && (crate::spatial::geometry::grid(r.x) - cx).abs() <= 1
                        && (crate::spatial::geometry::grid(r.y) - cy).abs() <= 1
                })
        });
        let mut aside_node = None;
        if near {
            let deck = self.spatial.deck_of(e);
            let occupied: &CellSet = if body.air { &self.air } else { &self.ground };
            for (ax, ay) in [(cx, cy - 1), (cx - 1, cy), (cx + 1, cy), (cx, cy + 1)] {
                let p = Spot::new(ax, ay, deck);
                if self.spatial.free(p, Some(id), None, None)
                    && self
                        .spatial
                        .clear_segment(current, p.fixed(), Some(occupied), &body)
                    && self.spatial.unit_segment_clear(
                        current,
                        p.fixed(),
                        Some(id),
                        None,
                        None,
                        None,
                    )
                {
                    aside_node = self.spatial.node(p);
                    break;
                }
            }
        }
        let u = state
            .entities
            .get_mut(id)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit");
        if let Some(node) = aside_node {
            u.route.insert(0, node);
        }
        u.retry_at = tick + 6;
    }

    fn begin_traffic_yield(&mut self, state: &mut State, id: EntityId, request: Request) -> bool {
        let tick = state.tick;
        let (Some(e), Some(parent)) = (state.entities.get(id), state.entities.get(request.parent))
        else {
            return false;
        };
        let mover = Mover::of(&self.spatial, e);
        let parent = Mover::of(&self.spatial, parent);
        let occupied: &CellSet = if mover.body.air {
            &self.air
        } else {
            &self.ground
        };
        let Some(plan) = traffic::escape(
            &self.spatial,
            &mut self.local,
            &mover,
            &parent,
            occupied,
            None,
        ) else {
            return false;
        };
        let u = state
            .entities
            .get_mut(id)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit");
        let Some(goal) = u.goal else {
            return false;
        };
        u.route.insert(0, plan.waypoint);
        u.segment = None;
        u.detour = Some(Detour {
            goal,
            waypoint: plan.waypoint,
            yielding: Some(Yielding {
                leader: request.leader,
                until: tick + 120,
            }),
            points: plan
                .points
                .iter()
                .map(|p| Milli { x: p.x, y: p.y })
                .collect(),
        });
        true
    }

    /// A bounded escape around nearby bodies that rejoins the route (old
    /// `beginLocalDetour`). A distant order never disables clearance at the unit's feet.
    fn begin_local_detour(
        &mut self,
        state: &mut State,
        id: EntityId,
        destination: Option<Spot>,
    ) -> bool {
        let tick = state.tick;
        let e = state.entities.get(id).expect("unit");
        let u = e.unit.as_ref().expect("unit");
        let mover = Mover::of(&self.spatial, e);
        let owner = e.owner;
        let current = mover.at;
        let probe = mover.body.radius.ceil().max(1.0) as i32;
        let reach = f64::from(4 * probe);
        if u.goal.is_none() || u.route.is_empty() {
            return false;
        }
        let spatial = &self.spatial;
        let local = &mut self.local;
        let cells = |p: Spot| {
            (
                f64::from(p.x) - f64::from(current.x) / 1000.0,
                f64::from(p.y) - f64::from(current.y) / 1000.0,
            )
        };
        let near_destination = destination.is_some_and(|d| {
            let (dx, dy) = cells(d);
            (dx * dx + dy * dy).sqrt() <= reach
        });
        // A parked ally may sit on an intermediate waypoint planned earlier.
        let rejoin: Option<usize> = if near_destination {
            Some(0)
        } else {
            u.route.iter().position(|&n| {
                let p = spatial.point(n);
                spatial.unit_segment_clear(p, p, Some(id), None, None, None)
            })
        };
        let replace_goal = rejoin.is_none()
            && destination
                .is_some_and(|d| spatial.clear_segment(current, d.fixed(), None, &mover.body));
        if rejoin.is_none() && !replace_goal {
            return false;
        }
        let next = if near_destination || replace_goal {
            destination.expect("destination")
        } else {
            spatial.spot(u.route[rejoin.expect("rejoin")])
        };
        let (dx, dy) = cells(next);
        let distance = (dx * dx + dy * dy).sqrt();
        let reservations = reservations(&self.teams, spatial, &self.units, tick, &mover, owner);
        let edge = (i64::from(spatial.size()) - 1) * 1000;
        let projected = (
            f64::from(current.x) / 1000.0 + dx / distance * 3.0 * f64::from(probe),
            f64::from(current.y) / 1000.0 + dy / distance * 3.0 * f64::from(probe),
        );
        let center = (
            round_half_up(projected.0) as i32,
            round_half_up(projected.1) as i32,
        );
        let deck = mover.cell.deck;
        let by_projection = |a: &Spot, b: &Spot| {
            let d = |p: &Spot| {
                let (x, y) = (f64::from(p.x) - projected.0, f64::from(p.y) - projected.1);
                x * x + y * y
            };
            d(a).total_cmp(&d(b))
                .then(a.y.cmp(&b.y))
                .then(a.x.cmp(&b.x))
        };
        let usable = |p: &Spot| {
            let (x, y) = cells(*p);
            (x * x + y * y).sqrt() <= reach
                && spatial.clear_segment(p.fixed(), next.fixed(), None, &mover.body)
                && spatial.clear_segment(p.fixed(), p.fixed(), Some(&reservations), &mover.body)
                && spatial.unit_segment_clear(p.fixed(), p.fixed(), Some(id), None, None, None)
        };
        let target = if distance <= reach {
            Some(next).filter(usable)
        } else {
            let mut ring: Vec<Spot> = (-1..=1)
                .flat_map(|y| (-1..=1).map(move |x| (x, y)))
                .map(|(x, y)| Spot::new(center.0 + x * probe, center.1 + y * probe, deck))
                .collect();
            ring.sort_by(by_projection);
            ring.into_iter().find(usable).or_else(|| {
                // A packed destination can fill the whole 3×3 projection.
                let mut outer: Vec<Spot> = (-2..=2)
                    .flat_map(|y| (-2..=2).map(move |x| (x, y)))
                    .filter(|&(x, y): &(i32, i32)| x.abs() == 2 || y.abs() == 2)
                    .map(|(x, y)| Spot::new(center.0 + x * probe, center.1 + y * probe, deck))
                    .collect();
                outer.sort_by(by_projection);
                outer.into_iter().find(usable)
            })
        };
        let Some(target) = target else {
            return false;
        };
        let end = target.fixed();
        let points = local_path(local, current, end, 250 * probe, |a, b| {
            i64::from(b.x) >= 0
                && i64::from(b.y) >= 0
                && i64::from(b.x) <= edge
                && i64::from(b.y) <= edge
                && spatial.unit_segment_clear(a, b, Some(id), None, None, None)
                && spatial.clear_local_segment(a, b, &reservations, &mover.body)
        });
        let Some(points) = points else {
            return false;
        };
        let Some(waypoint) = spatial.node(target) else {
            return false;
        };
        let destination_node = destination.and_then(|d| spatial.node(d));
        drop(reservations);
        let u = state
            .entities
            .get_mut(id)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit");
        if near_destination {
            u.goal = Some(waypoint);
            u.route = vec![waypoint];
        } else if replace_goal {
            let goal = destination_node.expect("destination node");
            u.goal = Some(goal);
            u.route = if waypoint == goal {
                vec![waypoint]
            } else {
                vec![waypoint, goal]
            };
        } else {
            u.route.drain(..rejoin.expect("rejoin"));
            if u.route.first() != Some(&waypoint) {
                u.route.insert(0, waypoint);
            }
        }
        u.detour = Some(Detour {
            goal: u.goal.expect("goal"),
            waypoint,
            yielding: None,
            points: points.iter().map(|p| Milli { x: p.x, y: p.y }).collect(),
        });
        u.segment = None;
        true
    }

    /// A finished formation member may enclose another's route: a truly idle ally steps
    /// aside once ordinary recovery has stalled (old `askIdleAllyToYield`).
    fn ask_idle_ally_to_yield(
        &mut self,
        state: &mut State,
        id: EntityId,
        from: Fixed,
        to: Fixed,
    ) -> bool {
        let tick = state.tick;
        let e = state.entities.get(id).expect("unit");
        let u = e.unit.as_ref().expect("unit");
        let since = u.last_moved_tick.unwrap_or(e.ready_tick);
        if tick.saturating_sub(since) < 40 || !matches!(u.order, Some(Order::Move { .. })) {
            return false;
        }
        let mover = Mover::of(&self.spatial, e);
        let owner = e.owner;
        let waypoint = self.spatial.point(u.route[0]);
        let (dx, dy) = (
            f64::from(waypoint.x - from.x),
            f64::from(waypoint.y - from.y),
        );
        let length = (dx * dx + dy * dy).sqrt();
        let reach = length.min(4000.0 * mover.body.radius.ceil().max(1.0));
        let ahead = if length > 0.0 {
            Fixed::on(
                from.x + round_half_up(dx / length * reach) as i32,
                from.y + round_half_up(dy / length * reach) as i32,
                from.deck,
            )
        } else {
            to
        };
        let mut blockers = Vec::new();
        self.spatial
            .unit_segment_clear(from, ahead, Some(id), None, None, Some(&mut blockers));
        blockers.sort_unstable();
        for ally_id in blockers {
            let Some(ally) = state.entities.get(ally_id) else {
                continue;
            };
            let Some(idle) = ally.unit.as_ref() else {
                continue;
            };
            let moves = self
                .spatial
                .profiles
                .get(ally.definition)
                .motion
                .is_some_and(|m| m.speed > 0.0);
            if !self.teams.allied(ally.owner, owner)
                || !ready(ally, tick)
                || idle.order.is_some()
                || !idle.order_queue.is_empty()
                || !idle.route.is_empty()
                || idle.contained.is_some()
                || idle.release.is_some()
                || idle.target.is_some()
                || !moves
            {
                continue;
            }
            let ally_mover = Mover::of(&self.spatial, ally);
            let occupied: &CellSet = if mover.body.air {
                &self.air
            } else {
                &self.ground
            };
            let claims = Adjusted {
                base: occupied,
                without: Some(ally_mover.node),
                with: Some(mover.node),
            };
            let separation = (mover.body.radius + ally_mover.body.radius) * 1000.0;
            let (ax, ay) = (f64::from(ahead.x - from.x), f64::from(ahead.y - from.y));
            let square = ax * ax + ay * ay;
            // Never park across the same approach again.
            let accept = |p: Spot| {
                let (px, py) = (
                    f64::from(p.x * 1000 - from.x),
                    f64::from(p.y * 1000 - from.y),
                );
                let t = if square != 0.0 {
                    ((px * ax + py * ay) / square).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (ex, ey) = (px - t * ax, py - t * ay);
                ex * ex + ey * ey >= separation * separation
            };
            let Some(plan) = traffic::escape(
                &self.spatial,
                &mut self.local,
                &ally_mover,
                &mover,
                &claims,
                Some(&accept),
            ) else {
                continue;
            };
            let ally = state.entities.get_mut(ally_id).expect("ally");
            let idle = ally.unit.as_mut().expect("unit");
            idle.segment = None;
            idle.goal = Some(plan.waypoint);
            idle.route = vec![plan.waypoint];
            idle.detour = Some(Detour {
                goal: plan.waypoint,
                waypoint: plan.waypoint,
                yielding: Some(Yielding {
                    leader: id,
                    until: tick + 120,
                }),
                points: plan
                    .points
                    .iter()
                    .map(|p| Milli { x: p.x, y: p.y })
                    .collect(),
            });
            self.spatial.update_body(ally);
            let occupied = if mover.body.air {
                &mut self.air
            } else {
                &mut self.ground
            };
            occupied.insert(plan.waypoint);
            let u = state
                .entities
                .get_mut(id)
                .expect("unit")
                .unit
                .as_mut()
                .expect("unit");
            u.retry_at = tick + 6;
            return true;
        }
        false
    }

    /// Follows a detour's points (old `moveDetour`). A yielding body waits at its pocket
    /// until the leader passes, gives up, or the wait times out.
    fn move_detour(&mut self, state: &mut State, id: EntityId, budget: i64, step: f64) {
        let tick = state.tick;
        let e = state.entities.get(id).expect("unit");
        let mover = Mover::of(&self.spatial, e);
        let owner = e.owner;
        let current = mover.at;
        let detour = e
            .unit
            .as_ref()
            .expect("unit")
            .detour
            .clone()
            .expect("detour");
        let target = Fixed::on(detour.points[0].x, detour.points[0].y, current.deck);
        let mut finished_yield = false;
        if let Some(yielding) = detour.yielding
            && detour.points.len() == 1
            && current.same_place(target)
        {
            let waiting = state.entities.get(yielding.leader).is_some_and(|leader| {
                alive(leader)
                    && self.teams.allied(leader.owner, owner)
                    && leader.unit.as_ref().is_some_and(|l| l.goal.is_some())
                    && self.spatial.point_range(
                        f64::from(current.x) / 1000.0,
                        f64::from(current.y) / 1000.0,
                        leader,
                    ) < 25.0
            });
            if tick < yielding.until && waiting {
                return;
            }
            let u = state
                .entities
                .get_mut(id)
                .expect("unit")
                .unit
                .as_mut()
                .expect("unit");
            if let Some(d) = u.detour.as_mut() {
                d.yielding = None;
            }
            finished_yield = true;
        }
        let e = state.entities.get_mut(id).expect("unit");
        let here = (f64::from(current.x) / 1000.0, f64::from(current.y) / 1000.0);
        if !turn_toward(
            &mut e.rotation,
            here,
            (f64::from(target.x) / 1000.0, f64::from(target.y) / 1000.0),
            step,
        ) {
            return;
        }
        let length = length_ceil(
            i64::from(target.x - current.x),
            i64::from(target.y - current.y),
        );
        let travel = length.min(budget);
        let proposed = if travel == length {
            target
        } else {
            Fixed::on(
                current.x + scale(i64::from(target.x - current.x), travel, length) as i32,
                current.y + scale(i64::from(target.y - current.y), travel, length) as i32,
                current.deck,
            )
        };
        let clear = {
            let reservations =
                reservations(&self.teams, &self.spatial, &self.units, tick, &mover, owner);
            self.spatial
                .clear_segment(current, proposed, Some(&reservations), &mover.body)
                && self
                    .spatial
                    .unit_segment_clear(current, proposed, Some(id), None, None, None)
        };
        let e = state.entities.get_mut(id).expect("unit");
        let u = e.unit.as_mut().expect("unit");
        if !clear {
            u.detour = None;
            u.retry_at = tick + 6;
            return;
        }
        if !proposed.same_place(current) {
            u.last_moved_tick = Some(tick);
        }
        u.segment = None;
        e.position = Milli {
            x: proposed.x,
            y: proposed.y,
        };
        if travel != length {
            return;
        }
        let u = e.unit.as_mut().expect("unit");
        let d = u.detour.as_mut().expect("detour");
        if d.points.len() == 1 && d.yielding.is_some() {
            return;
        }
        d.points.remove(0);
        if !d.points.is_empty() {
            return;
        }
        let waypoint = d.waypoint;
        u.detour = None;
        if !u.route.is_empty() {
            u.route.remove(0);
        }
        if finished_yield && let Some(goal) = u.goal {
            if u.order.is_none() && goal == waypoint {
                u.goal = None;
            } else {
                let destination = self.spatial.spot(goal);
                self.spatial.route(e, destination, false);
            }
        }
    }

    /// Step 5 separation (old `separateOverlaps`): idle solid bodies stacked by spawns or
    /// releases are routed apart, each checked every 20 ticks staggered by id.
    pub fn separate(&mut self, state: &mut State) {
        separation::separate(self, state);
    }

    /// Step 5 idle strolls (old `idleMotion`).
    pub fn idle(&mut self, state: &mut State) {
        idle::idle(self, state);
    }

    /// Ids of the active units, refreshed for a pass.
    fn active(&mut self, state: &State) -> Vec<EntityId> {
        self.collect_active(state);
        self.units.clone()
    }
}
