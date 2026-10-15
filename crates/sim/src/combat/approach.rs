//! Attack approach (old `attackApproach.ts`, `attackPositions.ts`): instead of walking
//! into its target, a unit picks a free cell from which its weapon reaches it, nearest
//! first (cells another unit of its owner is already heading to for the same target cost
//! four cells more), preferring a straight clear approach; otherwise it routes to one of
//! the first eight valid cells.

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
    !spatial.free(goal, Some(actor), Some(&body), None)
        || body_range_from(spatial, e, t, x, y) > weapon.range * weapon.range
        || !senses.attack_clear(state, senses.eye_at(e, x, y, goal.deck), t, weapon.ranged())
}

/// Routes `actor` to a position from which it can hit `target` (old `routeToAttack`).
pub fn route_to_attack(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    actor: EntityId,
    target: EntityId,
) -> bool {
    let _span = tracing::trace_span!("attack approach").entered();
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
