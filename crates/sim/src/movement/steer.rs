//! Steering locomotion (docs/decisions.md D51, D100), in the manner of StarCraft II: a unit
//! moves the moment it has a route, turning as it goes, towards the farthest waypoint it
//! can see. Nearby bodies push it apart a little so groups keep their spacing. Only foes
//! and planted allies are walls to it (allies on the move or idle are walked into, and
//! `push` parts them); when one is in the straight step it swerves (right side first, so
//! units passing each other agree on a side). Near its goal a unit that bumps
//! into allies already standing there stops beside them instead of fighting for the spot,
//! and a unit stuck for a second asks for a new route around the bodies in its way. All
//! in integer milli-cells, so every seat moves identically.

use super::{
    Motion, alive, place,
    probe::{Outcome, Trace, Verdict, note},
    ready, scale,
};
use crate::{
    entity::EntityId,
    spatial::{Body, Fixed, geometry::length_ceil},
    state::State,
};

/// Headings tried when the straight step is blocked, as (cos, sin) × 4096 of 0°, ±22°,
/// ±45°, ±68° and ±90°: the right-hand side first.
const SWERVE: [(i64, i64); 9] = [
    (4096, 0),
    (3798, 1534),
    (3798, -1534),
    (2896, 2896),
    (2896, -2896),
    (1534, 3798),
    (1534, -3798),
    (0, 4096),
    (0, -4096),
];

fn rotate((x, y): (i64, i64), (c, s): (i64, i64)) -> (i64, i64) {
    (
        (x * c - y * s).div_euclid(4096),
        (x * s + y * c).div_euclid(4096),
    )
}

impl Motion {
    /// One tick of locomotion for a unit with a route, `budget` milli-cells far.
    pub(super) fn steer(
        &mut self,
        state: &mut State,
        id: EntityId,
        budget: i64,
        step: f64,
        requests: &mut Option<std::collections::BTreeMap<EntityId, super::traffic::Request>>,
    ) {
        let mut trace = self.probe.is_some().then(Trace::default);
        self.steer_traced(state, id, budget, step, requests, &mut trace);
        if let (Some(p), Some(t)) = (self.probe.as_mut(), trace) {
            p.units.insert(id, t);
        }
    }

    fn steer_traced(
        &mut self,
        state: &mut State,
        id: EntityId,
        budget: i64,
        step: f64,
        requests: &mut Option<std::collections::BTreeMap<EntityId, super::traffic::Request>>,
        trace: &mut Option<Trace>,
    ) {
        let tick = state.tick;
        let e = state.entities.get(id).expect("unit");
        let body = self.spatial.body_of(e);
        let deck = self.spatial.deck_of(e);
        let current = Fixed::on(e.position.x, e.position.y, deck);
        let owner = e.owner;
        let u = e.unit.as_ref().expect("unit");
        let mut route = u.route.clone();
        let since = u.last_moved_tick.unwrap_or(e.ready_tick);
        // Only a move order arrives with its group; other routes (work, pursuit, strolls,
        // separation) end on their spot.
        let group_move = matches!(u.order, Some(crate::orders::Order::Move { .. }));
        // Waypoints reached, then the next one if the one after is already in sight.
        while route
            .first()
            .is_some_and(|&n| self.spatial.point(n).same_place(current))
        {
            route.remove(0);
        }
        if route.len() >= 2
            && self
                .spatial
                .clear_segment(current, self.spatial.point(route[1]), None, &body)
        {
            route.remove(0);
        }
        let Some(&next) = route.first() else {
            self.write_route(state, id, route);
            return;
        };
        let target = self.spatial.point(next);
        note(trace, |t| {
            t.waypoint = Some((target.x, target.y));
            t.stalled = tick.saturating_sub(since);
        });
        let (dx, dy) = (
            i64::from(target.x - current.x),
            i64::from(target.y - current.y),
        );
        let distance = length_ceil(dx, dy);
        let last = route.len() == 1 && group_move;
        let radius = i64::from(body.radius_milli());
        let arrive = self.spatial.tuning.arrive + 3 * radius;
        // Near the goal but off its own spot, touching an ally already standing there:
        // arrived with the group. On its spot's doorstep it finishes the step instead.
        if last
            && distance <= arrive
            && distance > radius
            && self.touching_standing_ally(state, id, current, &body, owner)
        {
            let u = state
                .entities
                .get_mut(id)
                .and_then(|e| e.unit.as_mut())
                .expect("unit");
            u.settled = true;
            u.goal = None;
            u.route.clear();
            u.segment = None;
            note(trace, |t| t.outcome = Outcome::Settled);
            return;
        }
        let reach = budget.min(distance);
        let mut velocity = (scale(dx, reach, distance), scale(dy, reach, distance));
        note(trace, |t| t.desired = velocity);
        // Keep a little room from neighbours and sidestep bodies ahead early, except on
        // the final step onto the goal.
        if reach < distance && !(last && distance <= 2 * radius + self.spatial.tuning.gap) {
            let (sx, sy) = self.separation(state, id, current, &body);
            let (ax, ay) = self.avoidance(state, id, current, velocity, budget, &body);
            note(trace, |t| {
                t.separation = (sx / 2, sy / 2);
                t.avoidance = (ax, ay);
            });
            if sx != 0 || sy != 0 || ax != 0 || ay != 0 {
                let (vx, vy) = (velocity.0 + sx / 2 + ax, velocity.1 + sy / 2 + ay);
                let l = length_ceil(vx, vy);
                velocity = if l > reach {
                    (scale(vx, reach, l), scale(vy, reach, l))
                } else {
                    (vx, vy)
                };
            }
        }
        note(trace, |t| t.velocity = velocity);
        // Terrain across the straight way to the waypoint (a building placed since the route
        // was planned): plan again rather than slide along it.
        let straight = Fixed::on(
            current.x + velocity.0 as i32,
            current.y + velocity.1 as i32,
            current.deck,
        );
        if !self.spatial.clear_segment(current, straight, None, &body)
            && !self.spatial.clear_segment(current, target, None, &body)
            && tick
                >= state
                    .entities
                    .get(id)
                    .and_then(|e| e.unit.as_ref())
                    .map_or(0, |u| u.retry_at)
        {
            let e = state.entities.get_mut(id).expect("unit");
            if let Some(goal) = e.unit.as_ref().and_then(|u| u.goal) {
                let destination = self.spatial.spot(goal);
                if !self.spatial.route(e, destination, false)
                    && let Some(u) = e.unit.as_mut()
                {
                    // No way there now: drop the route; the order plans again shortly.
                    u.route.clear();
                    u.segment = None;
                }
                if let Some(u) = e.unit.as_mut() {
                    u.retry_at = tick + 20;
                }
                note(trace, |t| t.outcome = Outcome::Rerouted);
                return;
            }
        }
        let mut moved = None;
        let mut beside_allies = false;
        for (i, &turn) in SWERVE.iter().enumerate() {
            let (cx, cy) = if i == 0 {
                velocity
            } else {
                rotate(velocity, turn)
            };
            if cx == 0 && cy == 0 {
                continue;
            }
            let mut proposed = if i == 0 && reach == distance {
                target
            } else {
                Fixed::on(current.x + cx as i32, current.y + cy as i32, current.deck)
            };
            self.spatial.adopt_surface(current, &mut proposed, target);
            if !self.spatial.clear_segment(current, proposed, None, &body) {
                note(trace, |t| t.tried.push(((cx, cy), Verdict::Terrain)));
                continue;
            }
            let mut blockers = Vec::new();
            // Only foes and planted allies are walls (D100): allies walking or idle are
            // walked into, and the push pass parts them.
            let counts = |b: &crate::spatial::BodyRecord| self.solid_to(state, owner, b.id);
            if self.spatial.unit_segment_clear(
                current,
                proposed,
                Some(id),
                None,
                Some(&counts as &dyn Fn(&crate::spatial::BodyRecord) -> bool),
                Some(&mut blockers),
            ) {
                note(trace, |t| t.tried.push(((cx, cy), Verdict::Clear)));
                moved = Some(proposed);
                break;
            }
            if i == 0 {
                blockers.sort_unstable();
                blockers.dedup();
                beside_allies = blockers
                    .iter()
                    .any(|&b| self.standing_ally(state, b, owner));
                // Near the goal, its way onto it blocked by the group standing there, or
                // within its own radius of the spot and blocked at all: stop here rather
                // than circle round them.
                if last && ((beside_allies && distance <= arrive) || distance <= radius) {
                    note(trace, |t| t.tried.push(((cx, cy), Verdict::Bodies)));
                    break;
                }
            }
            note(trace, |t| t.tried.push(((cx, cy), Verdict::Bodies)));
        }
        let e = state.entities.get_mut(id).expect("unit");
        if let Some(proposed) = moved {
            let from = (f64::from(current.x) / 1000.0, f64::from(current.y) / 1000.0);
            let to = (
                f64::from(proposed.x) / 1000.0,
                f64::from(proposed.y) / 1000.0,
            );
            // More than a right angle off its facing (an order reversed): turn first, an
            // eighth of a second at most, rather than walk backwards.
            let current_heading = f64::from(e.rotation) / 1000.0;
            let wanted = super::facing::heading(from, to, current_heading);
            if super::facing::turn_difference(current_heading, wanted).abs() > 90.0 {
                super::facing::turn_toward(&mut e.rotation, from, to, step);
                self.write_route(state, id, route);
                note(trace, |t| t.outcome = Outcome::Turned);
                return;
            }
            super::facing::turn_toward(&mut e.rotation, from, to, step);
            place(&self.spatial, e, proposed);
            let u = e.unit.as_mut().expect("unit");
            // Progress is getting closer to the waypoint: shuffling sideways in a jam
            // counts as held up.
            let left = length_ceil(
                i64::from(target.x - proposed.x),
                i64::from(target.y - proposed.y),
            );
            if distance - left >= budget / 4 || proposed.same_place(target) {
                u.last_moved_tick = Some(tick);
            }
            if proposed.same_place(target) {
                route.remove(0);
            }
            self.write_route(state, id, route);
            return;
        }
        // Blocked. Beside a goal the group already holds, stop there.
        let stalled = tick.saturating_sub(since);
        note(trace, |t| t.outcome = Outcome::Blocked);
        if last
            && distance <= arrive
            && (beside_allies || distance <= radius || stalled >= self.spatial.tuning.settle_ticks)
        {
            let u = e.unit.as_mut().expect("unit");
            u.settled = true;
            u.goal = None;
            u.route.clear();
            u.segment = None;
            note(trace, |t| t.outcome = Outcome::Settled);
            return;
        }
        if (self.spatial.tuning.negotiate_ticks..self.spatial.tuning.reroute_ticks)
            .contains(&stalled)
            && tick
                >= state
                    .entities
                    .get(id)
                    .and_then(|e| e.unit.as_ref())
                    .map_or(0, |u| u.retry_at)
            && state
                .entities
                .get(id)
                .and_then(|e| e.unit.as_ref())
                .is_some_and(|u| u.goal.is_some())
        {
            self.write_route(state, id, route);
            let straight = Fixed::on(
                current.x + velocity.0 as i32,
                current.y + velocity.1 as i32,
                current.deck,
            );
            self.recover(state, id, current, straight, requests);
            note(trace, |t| t.outcome = Outcome::Recovering);
            return;
        }
        let e = state.entities.get_mut(id).expect("unit");
        if stalled >= self.spatial.tuning.reroute_ticks {
            let goal = e.unit.as_ref().and_then(|u| u.goal);
            if let Some(goal) = goal {
                let destination = self.spatial.spot(goal);
                if !self.spatial.route(e, destination, true) {
                    self.spatial.route(e, destination, false);
                }
                if let Some(u) = e.unit.as_mut() {
                    u.last_moved_tick = Some(tick);
                }
            }
            note(trace, |t| t.outcome = Outcome::Replanned);
            return;
        }
        self.write_route(state, id, route);
    }

    fn write_route(&self, state: &mut State, id: EntityId, route: Vec<u32>) {
        if let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
            u.route = route;
            u.segment = None;
        }
    }

    /// How neighbours closer than touching plus the tuning's gap push this unit apart, milli-cells:
    /// each by its overlap, half as hard for allies on the move.
    fn separation(&self, state: &State, id: EntityId, at: Fixed, body: &Body) -> (i64, i64) {
        let radius = i64::from(body.radius_milli());
        let reach = radius + i64::from(self.spatial.profiles.max_radius) + self.spatial.tuning.gap;
        let (x, y) = (i64::from(at.x), i64::from(at.y));
        let owner = state.entities.get(id).map(|e| e.owner);
        let mut push = (0i64, 0i64);
        self.spatial
            .bodies
            .within(x - reach, y - reach, x + reach, y + reach, |b| {
                if b.id == id || !b.solid() || b.air != body.air || b.deck != at.deck {
                    return true;
                }
                let (ox, oy) = (x - i64::from(b.x), y - i64::from(b.y));
                let d = length_ceil(ox, oy);
                let room = radius + i64::from(b.radius) + self.spatial.tuning.gap;
                if d == 0 || d >= room {
                    return true;
                }
                let overlap = room - d;
                let weight = if b.has_route && owner.is_some_and(|o| self.teams.allied(o, b.owner))
                {
                    2
                } else {
                    1
                };
                push.0 += scale(ox, overlap, d) / weight;
                push.1 += scale(oy, overlap, d) / weight;
                true
            });
        push
    }

    /// A sideways nudge away from bodies in the path ahead (about the tuning's look-ahead of
    /// travel), so units ease around each other before touching. A body dead ahead is
    /// passed on the mover's preferred side: two units meeting head on prefer opposite
    /// sides and slip past. Idle allies are walked through (they get pushed) and nudge
    /// less.
    fn avoidance(
        &self,
        state: &State,
        id: EntityId,
        at: Fixed,
        velocity: (i64, i64),
        budget: i64,
        body: &Body,
    ) -> (i64, i64) {
        let speed = length_ceil(velocity.0, velocity.1);
        if speed == 0 {
            return (0, 0);
        }
        let radius = i64::from(body.radius_milli());
        let look = budget * self.spatial.tuning.look_ticks + radius;
        let reach = look + i64::from(self.spatial.profiles.max_radius) + self.spatial.tuning.gap;
        let (x, y) = (i64::from(at.x), i64::from(at.y));
        let Some(owner) = state.entities.get(id).map(|e| e.owner) else {
            return (0, 0);
        };
        // The preferred side: the mover's velocity turned a right angle (as SWERVE's first
        // turn).
        let side = (-velocity.1, velocity.0);
        let mut nudge = (0i64, 0i64);
        self.spatial
            .bodies
            .within(x - reach, y - reach, x + reach, y + reach, |b| {
                if b.id == id || !b.solid() || b.air != body.air || b.deck != at.deck {
                    return true;
                }
                let (rx, ry) = (i64::from(b.x) - x, i64::from(b.y) - y);
                let along = (rx * velocity.0 + ry * velocity.1) / speed;
                let clearance = radius + i64::from(b.radius) + self.spatial.tuning.gap;
                if along <= 0 || along > look + i64::from(b.radius) {
                    return true;
                }
                let lateral = (rx * side.0 + ry * side.1) / speed;
                if lateral.abs() >= clearance || self.pushable(state, b.id, owner) {
                    return true;
                }
                // Bodies walking the same way are followed, not avoided; oncoming ones are
                // passed on the preferred side like traffic, whichever side they are on.
                let mut oncoming = false;
                if let Some((hx, hy)) = self.heading_of(state, b.id) {
                    let h = length_ceil(hx, hy).max(1);
                    let alignment = hx * velocity.0 + hy * velocity.1;
                    if alignment * 10 > 3 * h * speed {
                        return true;
                    }
                    oncoming = alignment * 10 < -3 * h * speed;
                }
                // Stronger the closer and the more centred; away from the body's side, or to
                // the preferred side when it is near the centre line.
                let strength = scale(clearance - lateral.abs(), look - along.min(look), look);
                let toward = if oncoming || lateral <= clearance / 4 {
                    1
                } else {
                    -1
                };
                nudge.0 += toward * scale(side.0, strength, speed);
                nudge.1 += toward * scale(side.1, strength, speed);
                true
            });
        let l = length_ceil(nudge.0, nudge.1);
        if l > budget {
            (scale(nudge.0, budget, l), scale(nudge.1, budget, l))
        } else {
            nudge
        }
    }

    /// Whether an ally of `owner` that has stopped (no route, not fighting) touches the
    /// unit at `at`, give or take the tuning's gap.
    fn touching_standing_ally(
        &self,
        state: &State,
        id: EntityId,
        at: Fixed,
        body: &Body,
        owner: canopy_content::placement::Owner,
    ) -> bool {
        let radius = i64::from(body.radius_milli());
        let reach =
            radius + i64::from(self.spatial.profiles.max_radius) + 2 * self.spatial.tuning.gap;
        let (x, y) = (i64::from(at.x), i64::from(at.y));
        let mut touching = false;
        self.spatial
            .bodies
            .within(x - reach, y - reach, x + reach, y + reach, |b| {
                if b.id == id || !b.solid() || b.air != body.air || b.has_route {
                    return true;
                }
                let d = length_ceil(i64::from(b.x) - x, i64::from(b.y) - y);
                if d < radius + i64::from(b.radius) + 2 * self.spatial.tuning.gap
                    && self.standing_ally(state, b.id, owner)
                {
                    touching = true;
                    return false;
                }
                true
            });
        touching
    }

    /// Where a unit is heading: towards its next waypoint (milli-cells), if it has one.
    fn heading_of(&self, state: &State, id: EntityId) -> Option<(i64, i64)> {
        let e = state.entities.get(id)?;
        let next = *e.unit.as_ref()?.route.first()?;
        let p = self.spatial.point(next);
        Some((i64::from(p.x - e.position.x), i64::from(p.y - e.position.y)))
    }

    /// Whether `other` is an ally of `owner` standing still (arrived, or simply idle).
    fn standing_ally(
        &self,
        state: &State,
        other: EntityId,
        owner: canopy_content::placement::Owner,
    ) -> bool {
        state.entities.get(other).is_some_and(|b| {
            self.teams.allied(b.owner, owner) && b.unit.as_ref().is_some_and(|u| u.route.is_empty())
        })
    }

    /// Whether `other` may be shoved aside by an allied mover (StarCraft II's push
    /// priority): an idle, ready unit of the same side that can walk and is not fighting,
    /// working, held or casting.
    fn pushable(
        &self,
        state: &State,
        other: EntityId,
        owner: canopy_content::placement::Owner,
    ) -> bool {
        let tick = state.tick;
        state.entities.get(other).is_some_and(|b| {
            let Some(u) = b.unit.as_ref() else {
                return false;
            };
            let walks = self
                .spatial
                .profiles
                .get(b.definition)
                .motion
                .is_some_and(|m| m.speed > 0.0);
            self.teams.allied(b.owner, owner)
                && ready(b, tick)
                && alive(b)
                && walks
                && u.order.is_none()
                && u.order_queue.is_empty()
                && u.route.is_empty()
                && u.target.is_none()
                && u.combat.attack.is_none()
                && u.work.job.is_none()
                && u.work.employment.is_none()
                && u.contained.is_none()
                && u.release.is_none()
                && u.garrison.is_none()
                && b.caster.as_ref().is_none_or(|c| c.pending.is_none())
                && !crate::abilities::statuses::stunned(&self.catalog, b)
                && !crate::abilities::statuses::rooted(&self.catalog, b)
        })
    }
}
