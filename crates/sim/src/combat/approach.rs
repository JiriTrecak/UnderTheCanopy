//! Attack approach (D101): how a unit gets to where its weapon reaches its target. Melee
//! units fill a ring round a unit they attack (`ring`); ranged ones walk at it and stop in
//! reach (`direct`, StarCraft II's); buildings, and the lab's old mode, keep the old game's
//! spot search (`attackApproach.ts`, `attackPositions.ts`: a free cell from which the
//! weapon reaches, nearest first, cells an ally already heads to costing four cells more,
//! preferring a straight clear approach, else one of the first eight valid cells).

use super::senses::{Senses, half_extents};
use crate::{
    entity::EntityId,
    spatial::{Fixed, Spatial, Spot},
    state::State,
};

/// Detour destinations remembered while looking for a direct approach.
const DETOURS: usize = 8;
/// Penalty in cells for a cell an ally already heads to.
const RESERVED_PENALTY: f64 = 4.0;

struct Candidate {
    score: f64,
    spot: Spot,
    index: usize,
}

impl Candidate {
    /// The order candidates are tried in: score, row, column, then generation order, so
    /// no two compare equal.
    fn order(&self, other: &Self) -> std::cmp::Ordering {
        self.score
            .total_cmp(&other.score)
            .then(self.spot.y.cmp(&other.spot.y))
            .then(self.spot.x.cmp(&other.spot.x))
            .then(self.index.cmp(&other.index))
    }
}

/// A candidate in a heap that pops the first one to try.
struct Next(Candidate);

impl PartialEq for Next {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Next {}
impl PartialOrd for Next {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Next {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.0.order(&self.0)
    }
}

/// Squared gap in cells between `actor`'s body standing at `(x, y)` and `target` (old
/// `bodyRange` from a point).
fn body_range_from(
    spatial: &Spatial,
    actor: &crate::entity::Entity,
    target: &crate::entity::Entity,
    x: f64,
    y: f64,
) -> f64 {
    let reach = spatial.body_of(actor).radius
        + if target.unit.is_some() {
            spatial.body_of(target).radius
        } else {
            0.0
        };
    let gap = (spatial.point_range(x, y, target).sqrt() - reach).max(0.0);
    gap * gap
}

/// Whether a route's goal no longer serves the attack: taken, out of reach, or blocked
/// for the weapon.
pub fn stale_goal(
    senses: &Senses,
    spatial: &Spatial,
    state: &State,
    actor: EntityId,
    target: EntityId,
    goal: Spot,
) -> bool {
    let (e, t) = (
        state.entities.get(actor).expect("actor"),
        state.entities.get(target).expect("target"),
    );
    let weapon = senses.weapon(e).expect("weapon");
    let body = spatial.body_of(e);
    let (x, y) = (f64::from(goal.x), f64::from(goal.y));
    if spatial.tuning.approach == crate::movement::tuning::Approach::Ring
        && t.unit.is_some()
        && !weapon.ranged()
    {
        // A ring spot (D101) lasts while it stays on the ring round the target and no ally
        // fights from it; the soft crowd on the way does not make it stale.
        let distance = ring_aim(spatial.body_of(t).radius, body.radius);
        let (tx, ty) = (
            f64::from(t.position.x) / 1000.0,
            f64::from(t.position.y) / 1000.0,
        );
        let off = (((x - tx) * (x - tx) + (y - ty) * (y - ty)).sqrt() - distance).abs() > 1.0;
        let spacing = 2.0 * body.radius * spatial.tuning.ring_spacing as f64 / 100.0;
        let fought = state.entities.iter().any(|o| {
            o.id != actor
                && o.owner == e.owner
                && o.unit
                    .as_ref()
                    .is_some_and(|u| u.target == Some(target) && u.combat.attack.is_some())
                && {
                    let (ox, oy) = (
                        f64::from(o.position.x) / 1000.0 - x,
                        f64::from(o.position.y) / 1000.0 - y,
                    );
                    (ox * ox + oy * oy).sqrt() < spacing * 0.5
                }
        });
        return off || fought;
    }
    !spatial.free(goal, Some(actor), Some(&body), None)
        || body_range_from(spatial, e, t, x, y) > weapon.range * weapon.range
        || !senses.attack_clear(state, senses.eye_at(e, x, y, goal.deck), t, weapon.ranged())
}

/// Routes `actor` to where it can hit `target`, as the tuning's approach says (D101).
pub fn route_to_attack(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    actor: EntityId,
    target: EntityId,
) -> bool {
    let _span = tracing::trace_span!("attack approach").entered();
    use crate::movement::tuning::Approach;
    let (e, t) = (
        state.entities.get(actor).expect("actor"),
        state.entities.get(target).expect("target"),
    );
    let ranged = senses.weapon(e).is_some_and(|w| w.ranged());
    // Rings are round units; buildings keep the spot search.
    let round = t.unit.is_some();
    match spatial.tuning.approach {
        Approach::Spots => route_to_spot(senses, spatial, state, actor, target),
        Approach::Ring if round && !ranged => {
            ring(senses, spatial, state, actor, target)
                || route_to_spot(senses, spatial, state, actor, target)
        }
        Approach::Direct | Approach::Ring if round => {
            direct(spatial, state, actor, target)
                || route_to_spot(senses, spatial, state, actor, target)
        }
        _ => route_to_spot(senses, spatial, state, actor, target),
    }
}

/// StarCraft II's approach: walk at the target; the unit stops once in reach.
fn direct(spatial: &mut Spatial, state: &mut State, actor: EntityId, target: EntityId) -> bool {
    let Some(t) = state.entities.get(target) else {
        return false;
    };
    let deck = spatial.deck_of(t);
    let spot = Spot::new(
        (f64::from(t.position.x) / 1000.0).round() as i32,
        (f64::from(t.position.y) / 1000.0).round() as i32,
        deck,
    );
    let e = state.entities.get_mut(actor).expect("actor");
    spatial.route(e, spot, false)
}

/// How far from a target of radius `tr` a ring goal stands for an attacker of radius
/// `ar`: half the attacker's body inside touching.
fn ring_aim(tr: f64, ar: f64) -> f64 {
    tr + ar * 0.5
}

/// The angle `a` brought into (-π, π].
fn wrap(a: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let mut a = a % tau;
    if a > std::f64::consts::PI {
        a -= tau;
    } else if a <= -std::f64::consts::PI {
        a += tau;
    }
    a
}

/// Where a unit on the move will be in `seconds`, cells: along its route at its speed.
fn ahead(spatial: &Spatial, e: &crate::entity::Entity, seconds: f64) -> (f64, f64) {
    let here = (
        f64::from(e.position.x) / 1000.0,
        f64::from(e.position.y) / 1000.0,
    );
    let Some(&next) = e.unit.as_ref().and_then(|u| u.route.first()) else {
        return here;
    };
    let speed = spatial
        .profiles
        .get(e.definition)
        .motion
        .map_or(0.0, |m| m.speed);
    let p = spatial.point(next);
    let (dx, dy) = (
        f64::from(p.x) / 1000.0 - here.0,
        f64::from(p.y) / 1000.0 - here.1,
    );
    let l = (dx * dx + dy * dy).sqrt();
    if l < 1e-6 {
        return here;
    }
    let go = (speed * seconds).min(l * 4.0);
    (here.0 + dx / l * go, here.1 + dy / l * go)
}

/// Ring fill (D101): the melee attackers of one target share the ring round it. The ring
/// holds as many as fit; those nearest the target take it, each the spot on its own side
/// in the group's order round the target (so nobody crosses), the group's spots a block
/// centred on where it comes from; those bound past the target's flank walk round it; the
/// rest wait a row further out. With the lab's `lead_fleeing`, a faster group aims where a
/// fleeing target is going and closes in front of it.
fn ring(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    actor: EntityId,
    target: EntityId,
) -> bool {
    use crate::math::{atan2, cos, sin};
    let (e, t) = (
        state.entities.get(actor).expect("actor"),
        state.entities.get(target).expect("target"),
    );
    let Some(weapon) = senses.weapon(e) else {
        return false;
    };
    let body = spatial.body_of(e);
    let tr = spatial.body_of(t).radius;
    let ar = body.radius;
    // Where the ring stands (bodies touching), and the goals: half a body inside it, so a
    // unit walks in from its spot's side until the target's body stops it, in reach
    // whatever cell the goal rounds to.
    let distance = tr + ar;
    let aim = ring_aim(tr, ar);
    let spacing = (2.0 * ar * spatial.tuning.ring_spacing as f64 / 100.0).max(0.5);
    let slots = ((std::f64::consts::TAU * distance / spacing).floor() as usize).max(1);
    let mut centre = (
        f64::from(t.position.x) / 1000.0,
        f64::from(t.position.y) / 1000.0,
    );
    let here = (
        f64::from(e.position.x) / 1000.0,
        f64::from(e.position.y) / 1000.0,
    );
    if spatial.tuning.lead_fleeing && t.unit.as_ref().is_some_and(|u| !u.route.is_empty()) {
        let speed = |x: &crate::entity::Entity| {
            spatial
                .profiles
                .get(x.definition)
                .motion
                .map_or(0.0, |m| m.speed)
        };
        let (mine, theirs) = (speed(e), speed(t));
        let gap = ((here.0 - centre.0) * (here.0 - centre.0)
            + (here.1 - centre.1) * (here.1 - centre.1))
            .sqrt();
        if mine > theirs * 1.1 {
            let seconds = (gap / (mine - theirs)).min(3.0);
            centre = ahead(spatial, t, seconds);
        }
    }
    // The target's melee attackers of the actor's owner and layer, the actor among them.
    let tick = state.tick;
    let mut attackers: Vec<(f64, EntityId, f64, Option<f64>)> = state
        .entities
        .iter()
        .filter(|o| {
            (o.id == actor
                || (o.owner == e.owner
                    && o.hp.is_none_or(|hp| hp > 0)
                    && o.ready_tick <= tick
                    && crate::spatial::present(o)
                    && o.unit.as_ref().is_some_and(|u| u.target == Some(target))))
                && spatial.body_of(o).air == body.air
                && senses.weapon(o).is_some_and(|w| !w.ranged())
        })
        .map(|o| {
            let (x, y) = (
                f64::from(o.position.x) / 1000.0,
                f64::from(o.position.y) / 1000.0,
            );
            let (dx, dy) = (x - centre.0, y - centre.1);
            // A spot it already heads to on this ring (its goal at the ring's distance).
            let claim = o
                .unit
                .as_ref()
                .filter(|u| !u.route.is_empty())
                .and_then(|u| u.goal)
                .and_then(|g| {
                    let p = spatial.point(g);
                    let (gx, gy) = (
                        f64::from(p.x) / 1000.0 - centre.0,
                        f64::from(p.y) / 1000.0 - centre.1,
                    );
                    ((gx * gx + gy * gy).sqrt() - aim)
                        .abs()
                        .le(&1.0)
                        .then(|| atan2(gy, gx))
                });
            ((dx * dx + dy * dy).sqrt(), o.id, atan2(dy, dx), claim)
        })
        .collect();
    // Spots stand at fixed angles round the target, so the ring never turns under the
    // attackers. One already in reach keeps the spot it stands at; the rest, nearest first,
    // each take the free spot closest to the side they come from (so nobody crosses
    // another's way more than it must); beyond what the ring holds, a row further out.
    attackers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let step = std::f64::consts::TAU / slots as f64;
    let slot_of = |bearing: f64| {
        (((bearing + std::f64::consts::TAU) % std::f64::consts::TAU / step).round() as usize)
            % slots
    };
    let reach = distance + weapon.range + 0.25;
    let mut taken = vec![false; slots];
    let mut placed: Option<usize> = None;
    let mut queued: Vec<&(f64, EntityId, f64, Option<f64>)> = Vec::new();
    for a in &attackers {
        if a.0 <= reach && !taken[slot_of(a.2)] {
            taken[slot_of(a.2)] = true;
            if a.1 == actor {
                placed = Some(slot_of(a.2));
            }
        } else {
            queued.push(a);
        }
    }
    // Those on their way keep the spot they head to while it is free: a spot is changed
    // only when someone took it first, so nobody turns back and forth.
    let mut unclaimed = Vec::new();
    for a in queued {
        match a.3.map(slot_of) {
            Some(k) if !taken[k] => {
                taken[k] = true;
                if a.1 == actor {
                    placed = Some(k);
                }
            }
            _ => unclaimed.push(a),
        }
    }
    let queued = unclaimed;
    let own_bearing = attackers.iter().find(|a| a.1 == actor).map_or(0.0, |a| a.2);
    if placed.is_none() {
        for a in queued {
            let best = (0..slots).filter(|&k| !taken[k]).min_by(|&x, &y| {
                let gap = |k: usize| wrap(k as f64 * step - a.2).abs();
                gap(x).total_cmp(&gap(y)).then(x.cmp(&y))
            });
            let Some(k) = best else {
                break;
            };
            taken[k] = true;
            if a.1 == actor {
                placed = Some(k);
                break;
            }
        }
    }
    let (angle, radius) = match placed {
        Some(k) => (k as f64 * step, aim),
        // A row further out, on its own side: it steps in when a spot frees.
        None => (own_bearing, distance + 2.0 * ar + 0.5),
    };
    let point = (
        centre.0 + radius * cos(angle),
        centre.1 + radius * sin(angle),
    );
    let deck = spatial.deck_of(t);
    let spot = |p: (f64, f64)| Spot::new(p.0.round() as i32, p.1.round() as i32, deck);
    let goal = spot(point);
    if !spatial.unit_walkable(goal, &body) {
        return false;
    }
    // Round the flank when the spot is well round the target from the unit: along an arc
    // outside the ring, an eighth of a turn a waypoint.
    let mut waypoints = Vec::new();
    let turn = wrap(angle - own_bearing);
    let gap = ((here.0 - centre.0) * (here.0 - centre.0)
        + (here.1 - centre.1) * (here.1 - centre.1))
        .sqrt();
    if turn.abs() > std::f64::consts::FRAC_PI_3 && gap < distance * 4.0 + 8.0 {
        let outer = distance + 2.0 * ar + 1.0;
        let steps = (turn.abs() / std::f64::consts::FRAC_PI_4).ceil() as usize;
        for k in 1..steps {
            let a = own_bearing + turn * k as f64 / steps as f64;
            waypoints.push(spot((centre.0 + outer * cos(a), centre.1 + outer * sin(a))));
        }
    }
    waypoints.push(goal);
    let from = crate::spatial::Fixed::on(e.position.x, e.position.y, spatial.deck_of(e));
    let mut previous = from;
    let mut nodes = Vec::new();
    for w in &waypoints {
        let p = w.fixed();
        let Some(node) = spatial.node(*w) else {
            nodes.clear();
            break;
        };
        if !spatial.clear_segment(previous, p, None, &body) {
            nodes.clear();
            break;
        }
        nodes.push(node);
        previous = p;
    }
    let e = state.entities.get_mut(actor).expect("actor");
    if nodes.is_empty() {
        return spatial.route(e, goal, false);
    }
    let u = e.unit.as_mut().expect("unit");
    u.goal = nodes.last().copied();
    u.route = nodes;
    u.segment = None;
    true
}

/// Routes `actor` to a free cell from which it can hit `target` (old `routeToAttack`).
fn route_to_spot(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    actor: EntityId,
    target: EntityId,
) -> bool {
    let (e, t) = (
        state.entities.get(actor).expect("actor"),
        state.entities.get(target).expect("target"),
    );
    let weapon = senses.weapon(e).expect("weapon");
    let body = spatial.body_of(e);
    let range = weapon.range
        + body.radius
        + if t.unit.is_some() {
            spatial.body_of(t).radius
        } else {
            0.0
        };
    let (ox, oy) = (
        f64::from(e.position.x) / 1000.0,
        f64::from(e.position.y) / 1000.0,
    );
    let (cx, cy) = (
        f64::from(t.position.x) / 1000.0,
        f64::from(t.position.y) / 1000.0,
    );
    let (hx, hy) = half_extents(senses.catalog, t);
    let tick = state.tick;
    // Cells allies of the same owner and locomotion already head to for this target.
    let mut reserved: Vec<u32> = state
        .entities
        .iter()
        .filter(|o| {
            o.id != actor
                && o.owner == e.owner
                && o.hp.is_none_or(|hp| hp > 0)
                && o.ready_tick <= tick
                && crate::spatial::present(o)
                && spatial.body_of(o).air == body.air
                && o.unit
                    .as_ref()
                    .is_some_and(|u| u.target == Some(target) && !u.route.is_empty())
        })
        .filter_map(|o| o.unit.as_ref().expect("unit").goal)
        .collect();
    reserved.sort_unstable();
    reserved.dedup();
    let size = spatial.size() as i64;
    let y0 = ((cy - hy - range).ceil() as i64).max(0);
    let y1 = ((cy + hy + range).floor() as i64).min(size - 1);
    let x0 = ((cx - hx - range).ceil() as i64).max(0);
    let x1 = ((cx + hx + range).floor() as i64).min(size - 1);
    let mut candidates = Vec::new();
    let from = Fixed::on(e.position.x, e.position.y, spatial.deck_of(e));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = ((x as f64 - cx).abs() - hx).max(0.0);
            let dy = ((y as f64 - cy).abs() - hy).max(0.0);
            if dx * dx + dy * dy > range * range {
                continue;
            }
            let mut push = |spot: Spot| {
                let node = spatial.node(spot);
                let penalty = node.is_some_and(|n| reserved.binary_search(&n).is_ok());
                let (px, py) = (f64::from(spot.x) - ox, f64::from(spot.y) - oy);
                let score =
                    (px * px + py * py).sqrt() + if penalty { RESERVED_PENALTY } else { 0.0 };
                candidates.push(Candidate {
                    score,
                    spot,
                    index: candidates.len(),
                });
            };
            match spatial.terrain().surfaces() {
                Some(surfaces) => {
                    let nodes: Vec<u32> = surfaces.at(x, y).collect();
                    for node in nodes {
                        push(spatial.spot(node));
                    }
                }
                None => push(Spot::new(x as i32, y as i32, None)),
            }
        }
    }
    // Candidates are tried in `Candidate::order`. Until eight detours are collected each
    // one is tried in turn (popped from a heap: usually only a few are needed). After
    // that a candidate only wins by a direct approach; that test is a pure query of the
    // other bodies, so it runs over the rest in any order, and only the few that pass are
    // sorted and tried in turn.
    let ranged = weapon.ranged();
    let mut detours: Vec<Spot> = Vec::new();
    let mut heap =
        std::collections::BinaryHeap::from(candidates.into_iter().map(Next).collect::<Vec<_>>());
    // Whether `c` is valid and free; with a clear straight approach that routes, the
    // actor is on its way.
    let attempt =
        |spatial: &mut Spatial, state: &mut State, c: &Candidate, direct: Option<bool>| {
            let destination = c.spot.fixed();
            let e = state.entities.get(actor).expect("actor");
            let t = state.entities.get(target).expect("target");
            let valid = spatial.unit_walkable(c.spot, &body)
                && senses.attack_clear(
                    state,
                    senses.eye_at(e, f64::from(c.spot.x), f64::from(c.spot.y), c.spot.deck),
                    t,
                    ranged,
                );
            if !valid || !spatial.free(c.spot, Some(actor), Some(&body), None) {
                return None;
            }
            let straight = direct.unwrap_or_else(|| {
                spatial.unit_segment_clear(from, destination, Some(actor), None, None, None)
            }) && spatial.clear_segment(from, destination, None, &body);
            if straight {
                let e = state.entities.get_mut(actor).expect("actor");
                if spatial.route(e, c.spot, false) {
                    return Some(true);
                }
            }
            Some(false)
        };
    while detours.len() < DETOURS {
        let Some(Next(c)) = heap.pop() else {
            break;
        };
        match attempt(spatial, state, &c, None) {
            Some(true) => return true,
            Some(false) => detours.push(c.spot),
            None => {}
        }
    }
    // In a crowd most directions are blocked by a body touching the actor.
    let near = spatial.near_bodies(from, Some(actor), None);
    let mut direct: Vec<Candidate> = heap
        .into_vec()
        .into_iter()
        .map(|Next(c)| c)
        .filter(|c| {
            let to = c.spot.fixed();
            !near.blocks(to) && spatial.unit_segment_clear(from, to, Some(actor), None, None, None)
        })
        .collect();
    direct.sort_unstable_by(Candidate::order);
    for c in &direct {
        if attempt(spatial, state, c, Some(true)) == Some(true) {
            return true;
        }
    }
    for spot in detours {
        let e = state.entities.get_mut(actor).expect("actor");
        if spatial.route(e, spot, false) {
            return true;
        }
    }
    false
}
