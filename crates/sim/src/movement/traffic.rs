//! Deadlock recovery (old `trafficRequests` and `trafficEscape`,
//! `src/sim/game/trafficRequests.ts`, `trafficEscape.ts`).
//!
//! When friendly movers have stalled for [`STALL_TICKS`], their next attempted steps form
//! a directed "waits for" graph. Only genuine cycles (strongly connected components)
//! negotiate: priority propagates from the lowest stalled id along the waits, and each
//! lower-priority member is asked to yield to a parent ahead of it. At a terrain
//! constriction the request goes to a cycle member that can actually step aside.

use super::{Mover, local_path::{LocalScratch, local_path}};
use crate::{
    entity::EntityId,
    movement::facing::{heading, turn_difference},
    navigation::cells::{Adjusted, CellSet, Reserved, Single},
    spatial::{Fixed, Spatial, Spot},
    state::State,
};
use std::collections::{BTreeMap, BTreeSet};

/// Ticks without progress before a mover may negotiate.
pub const STALL_TICKS: u32 = 200;

/// Who a stalled unit should yield to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub leader: EntityId,
    pub parent: EntityId,
}

/// A yielding manoeuvre: the pocket cell and the escape path to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escape {
    pub waypoint: u32,
    pub points: Vec<Fixed>,
}

fn distance_sq(a: Fixed, b: Fixed) -> f64 {
    let (dx, dy) = (f64::from(a.x - b.x), f64::from(a.y - b.y));
    dx * dx + dy * dy
}

/// A pocket beside `e`, away from `parent`, reachable by a short local path (read-only).
pub fn escape(
    spatial: &Spatial,
    local: &mut LocalScratch,
    e: &Mover,
    parent: &Mover,
    occupied: &dyn Reserved,
    accept: Option<&dyn Fn(Spot) -> bool>,
) -> Option<Escape> {
    let (from, other) = (e.at, parent.at);
    let (dx, dy) = (f64::from(from.x - other.x), f64::from(from.y - other.y));
    let length = (dx * dx + dy * dy).sqrt();
    if length == 0.0 {
        return None;
    }
    let step = e.body.radius.max(parent.body.radius).ceil().max(1.0) as i32;
    let mut candidates: Vec<Spot> = Vec::new();
    for y in -3..=3 {
        for x in -3..=3 {
            let p = Spot::new(e.cell.x + x * step, e.cell.y + y * step, e.cell.deck);
            let (qx, qy) = (f64::from(p.x * 1000 - from.x), f64::from(p.y * 1000 - from.y));
            if (qx * qx + qy * qy).sqrt() > 3500.0 * f64::from(step)
                || (qx * dy - qy * dx).abs() / length < 1000.0 * f64::from(step)
                || qx * dx + qy * dy < 0.0
                || !spatial.free(p, Some(e.id), None, None)
            {
                continue;
            }
            let fp = p.fixed();
            if !spatial.clear_segment(fp, fp, Some(occupied), &e.body)
                || !spatial.unit_segment_clear(fp, fp, Some(e.id), None, None, None)
            {
                continue;
            }
            if accept.is_some_and(|a| !a(p)) {
                continue;
            }
            candidates.push(p);
        }
    }
    candidates.sort_by(|a, b| {
        distance_sq(a.fixed(), from)
            .total_cmp(&distance_sq(b.fixed(), from))
            .then(a.y.cmp(&b.y))
            .then(a.x.cmp(&b.x))
    });
    for target in candidates.into_iter().take(6) {
        let points = local_path(local, from, target.fixed(), 250 * step, |a, b| {
            spatial.unit_segment_clear(a, b, Some(e.id), None, None, None)
                && spatial.clear_local_segment(a, b, occupied, &e.body)
        });
        if let Some(points) = points {
            return Some(Escape {
                waypoint: spatial.node(target)?,
                points,
            });
        }
    }
    None
}

struct Candidate {
    mover: Mover,
    owner: canopy_content::placement::Owner,
    stalled: bool,
}

/// Who should yield to whom this tick, among `units` (the movement pass's eligible units).
pub fn requests(
    spatial: &Spatial,
    state: &State,
    units: &[EntityId],
    local: &mut LocalScratch,
    claims: &mut CellSet,
) -> BTreeMap<EntityId, Request> {
    let tick = state.tick;
    let mut moving: Vec<Candidate> = Vec::new();
    for &id in units {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        let Some(u) = e.unit.as_ref() else {
            continue;
        };
        let profile = spatial.profiles.get(e.definition);
        let speed = profile.motion.map_or(0.0, |m| m.speed);
        let ignores = spatial.bodies.get(id).is_some_and(|b| b.ignores);
        if u.route.is_empty() || u.detour.is_some() || e.ready_tick > tick || speed <= 0.0 || ignores {
            continue;
        }
        moving.push(Candidate {
            mover: Mover::of(spatial, e),
            owner: e.owner,
            stalled: u.last_moved_tick.is_some_and(|t| tick.saturating_sub(t) >= STALL_TICKS),
        });
    }
    let mut out = BTreeMap::new();
    if !moving.iter().any(|c| c.stalled) {
        return out;
    }
    let index: BTreeMap<EntityId, usize> = moving.iter().enumerate().map(|(i, c)| (c.mover.id, i)).collect();
    // Waits-for edges along each mover's next attempted step.
    let mut edges: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
    for c in &moving {
        let e = state.entities.get(c.mover.id).expect("mover");
        let u = e.unit.as_ref().expect("unit");
        let from = c.mover.at;
        let waypoint = u
            .route
            .iter()
            .map(|&n| spatial.point(n))
            .find(|q| q.x != from.x || q.y != from.y);
        let Some(waypoint) = waypoint else {
            continue;
        };
        let profile = spatial.profiles.get(e.definition);
        let motion = profile.motion.expect("moving units move");
        let position = (f64::from(from.x) / 1000.0, f64::from(from.y) / 1000.0);
        let target = (f64::from(waypoint.x) / 1000.0, f64::from(waypoint.y) / 1000.0);
        let rotation = f64::from(e.rotation) / 1000.0;
        // A unit still turning cannot attempt this leg this tick.
        if turn_difference(rotation, heading(position, target, rotation)).abs() > f64::from(motion.turn_rate) / 40.0 + 1.0 {
            continue;
        }
        let (dx, dy) = (f64::from(waypoint.x - from.x), f64::from(waypoint.y - from.y));
        let length = (dx * dx + dy * dy).sqrt();
        if length == 0.0 {
            continue;
        }
        let step = length.min(motion.speed * 1000.0 / 40.0);
        let round = |v: f64| crate::terrain::round_half_up(v) as i32;
        let mut to = Fixed::on(
            round(f64::from(from.x) + dx / length * step),
            round(f64::from(from.y) + dy / length * step),
            from.deck,
        );
        spatial.adopt_surface(from, &mut to, waypoint);
        if !spatial.clear_segment(from, to, None, &c.mover.body) {
            continue;
        }
        let mut blocked: Vec<EntityId> = Vec::new();
        for other in &moving {
            let b = &other.mover;
            if b.id == c.mover.id || other.owner != c.owner || b.body.air != c.mover.body.air {
                continue;
            }
            // The broad phase of the old body index: cells within 1.5 cells of the start.
            let (bx, by) = (b.cell.x, b.cell.y);
            let lo = |v: i32| crate::spatial::geometry::grid(v - 1500);
            let hi = |v: i32| crate::spatial::geometry::grid(v + 1500);
            if bx < lo(from.x).max(0) || bx > hi(from.x) || by < lo(from.y).max(0) || by > hi(from.y) {
                continue;
            }
            if b.node != c.mover.node && !spatial.clear_segment(from, to, Some(&Single(b.node)), &c.mover.body) {
                blocked.push(b.id);
            }
        }
        let mut physical = Vec::new();
        spatial.unit_segment_clear(from, to, Some(c.mover.id), None, None, Some(&mut physical));
        for id in physical {
            if let Some(&i) = index.get(&id)
                && moving[i].owner == c.owner
                && !blocked.contains(&id)
            {
                blocked.push(id);
            }
        }
        blocked.sort_unstable();
        edges.insert(c.mover.id, blocked);
    }
    let components = cycles(&moving.iter().map(|c| c.mover.id).collect::<Vec<_>>(), &edges);
    // Priority inheritance from the lowest stalled id along the waits.
    let mut ranks: BTreeMap<EntityId, (EntityId, u32)> = moving.iter().map(|c| (c.mover.id, (c.mover.id, 0))).collect();
    for c in &moving {
        let id = c.mover.id;
        if ranks[&id].0 < id || !c.stalled {
            continue;
        }
        let mut queue = vec![id];
        let mut cursor = 0;
        while cursor < queue.len() {
            let current = queue[cursor];
            cursor += 1;
            let rank = ranks[&current];
            for &next in edges.get(&current).into_iter().flatten() {
                let old = ranks[&next];
                if old.0 < rank.0 || (old.0 == rank.0 && old.1 <= rank.1 + 1) {
                    continue;
                }
                ranks.insert(next, (rank.0, rank.1 + 1));
                queue.push(next);
            }
        }
    }
    let mut incoming: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
    for (&from, list) in &edges {
        for &to in list {
            incoming.entry(to).or_default().push(from);
        }
    }
    let stalled = |id: EntityId| index.get(&id).is_some_and(|&i| moving[i].stalled);
    for c in &moving {
        let id = c.mover.id;
        let rank = ranks[&id];
        if rank.0 == id || !c.stalled {
            continue;
        }
        let Some(&component) = components.get(&id) else {
            continue;
        };
        let mut parents: Vec<EntityId> = incoming
            .get(&id)
            .into_iter()
            .flatten()
            .copied()
            .filter(|p| {
                components.get(p) == Some(&component)
                    && ranks[p].0 == rank.0
                    && ranks[p].1 < rank.1
                    && stalled(*p)
            })
            .collect();
        parents.sort_by_key(|p| (ranks[p].1, *p));
        if let Some(&parent) = parents.first() {
            out.insert(
                id,
                Request {
                    leader: rank.0,
                    parent,
                },
            );
        }
    }
    // At a constriction, ask a cycle member that can actually step aside.
    let mover = |id: EntityId| &moving[index[&id]].mover;
    claims.reset(spatial.node_count() as usize);
    for &id in units {
        if let Some(b) = spatial.bodies.get(id)
            && !b.air
            && !b.ignores
        {
            claims.insert(b.node);
            if let Some(w) = b.reservation {
                claims.insert(w);
            }
        }
    }
    let mut air_claims: Option<CellSet> = None;
    let mut can_escape = |e: &Mover, parent: &Mover, local: &mut LocalScratch| {
        if e.body.air {
            let set = air_claims.get_or_insert_with(|| {
                let mut set = spatial.cell_set();
                for &id in units {
                    if let Some(b) = spatial.bodies.get(id)
                        && b.air
                    {
                        set.insert(b.node);
                    }
                }
                set
            });
            let adjusted = Adjusted {
                base: &*set,
                without: Some(e.node),
                with: None,
            };
            escape(spatial, local, e, parent, &adjusted, None).is_some()
        } else {
            let adjusted = Adjusted {
                base: &*claims,
                without: Some(e.node),
                with: None,
            };
            escape(spatial, local, e, parent, &adjusted, None).is_some()
        }
    };
    let distinct: BTreeSet<EntityId> = components.values().copied().collect();
    for component in distinct {
        let candidates: Vec<(EntityId, Request)> = out
            .iter()
            .filter(|(id, _)| components.get(id) == Some(&component))
            .map(|(&id, &r)| (id, r))
            .collect();
        let constrained = candidates.iter().any(|&(id, r)| {
            let (p, q) = (mover(id).at, mover(r.parent).at);
            let (dx, dy) = (f64::from(p.x - q.x), f64::from(p.y - q.y));
            let length = (dx * dx + dy * dy).sqrt();
            if length == 0.0 {
                return false;
            }
            [-1.0, 1.0].iter().all(|&sign| {
                let side = Fixed::on(
                    p.x + crate::terrain::round_half_up(-dy / length * 1000.0 * sign) as i32,
                    p.y + crate::terrain::round_half_up(dx / length * 1000.0 * sign) as i32,
                    p.deck,
                );
                !spatial.clear_segment(p, side, None, &mover(id).body)
            })
        });
        if !constrained
            || candidates
                .iter()
                .any(|&(id, r)| can_escape(mover(id), mover(r.parent), local))
        {
            continue;
        }
        let alternatives: Vec<EntityId> = moving
            .iter()
            .filter(|c| components.get(&c.mover.id) == Some(&component) && !out.contains_key(&c.mover.id) && c.stalled)
            .map(|c| c.mover.id)
            .collect();
        for id in alternatives {
            let parent = edges
                .get(&id)
                .into_iter()
                .flatten()
                .copied()
                .find(|&b| components.get(&b) == Some(&component) && can_escape(mover(id), mover(b), local));
            let Some(parent) = parent else {
                continue;
            };
            for (candidate, _) in &candidates {
                out.remove(candidate);
            }
            out.insert(id, Request { leader: parent, parent });
            break;
        }
    }
    out
}

/// Members of non-trivial strongly connected components, mapped to their lowest id.
fn cycles(nodes: &[EntityId], edges: &BTreeMap<EntityId, Vec<EntityId>>) -> BTreeMap<EntityId, EntityId> {
    // Iterative Tarjan.
    let mut seen: BTreeMap<EntityId, u32> = BTreeMap::new();
    let mut low: BTreeMap<EntityId, u32> = BTreeMap::new();
    let mut stack: Vec<EntityId> = Vec::new();
    let mut active: BTreeSet<EntityId> = BTreeSet::new();
    let mut components = BTreeMap::new();
    let mut sequence = 0;
    let empty = Vec::new();
    for &root in nodes {
        if seen.contains_key(&root) {
            continue;
        }
        let mut frames: Vec<(EntityId, usize)> = vec![(root, 0)];
        seen.insert(root, sequence);
        low.insert(root, sequence);
        sequence += 1;
        stack.push(root);
        active.insert(root);
        while let Some(&mut (id, ref mut next)) = frames.last_mut() {
            let out = edges.get(&id).unwrap_or(&empty);
            if *next < out.len() {
                let to = out[*next];
                *next += 1;
                if !seen.contains_key(&to) {
                    seen.insert(to, sequence);
                    low.insert(to, sequence);
                    sequence += 1;
                    stack.push(to);
                    active.insert(to);
                    frames.push((to, 0));
                } else if active.contains(&to) {
                    let l = low[&id].min(seen[&to]);
                    low.insert(id, l);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                let l = low[&parent].min(low[&id]);
                low.insert(parent, l);
            }
            if low[&id] != seen[&id] {
                continue;
            }
            let mut members = Vec::new();
            loop {
                let member = stack.pop().expect("member");
                active.remove(&member);
                members.push(member);
                if member == id {
                    break;
                }
            }
            if members.len() > 1 {
                let min = *members.iter().min().expect("members");
                for member in members {
                    components.insert(member, min);
                }
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_cycles_form_components() {
        let id = EntityId;
        let mut edges = BTreeMap::new();
        edges.insert(id(1), vec![id(2)]);
        edges.insert(id(2), vec![id(3)]);
        edges.insert(id(3), vec![id(1)]);
        edges.insert(id(4), vec![id(1)]);
        let components = cycles(&[id(1), id(2), id(3), id(4)], &edges);
        assert_eq!(components.len(), 3);
        assert!(components.values().all(|&c| c == id(1)));
        assert!(!components.contains_key(&id(4)));
    }
}
