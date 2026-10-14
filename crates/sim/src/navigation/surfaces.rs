//! Paths and body sweeps on layered walk surfaces (bridge decks): the old
//! `WalkSurfaces.path` (`src/shared/map/walkSurfaces.ts`) and `clearLayeredSweep`
//! (`src/sim/game/layeredSweep.ts`).

use crate::{
    spatial::geometry::{Fixed, clear_ray, clear_sweep, grid},
    terrain::{DeckId, SurfaceNode, WalkSurfaces},
};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
};

/// Reusable search buffers sized to the node count.
#[derive(Debug, Default, Clone)]
pub struct SurfaceScratch {
    cost: Vec<i64>,
    prev: Vec<u32>,
    seen: Vec<u32>,
    closed: Vec<u32>,
    epoch: u32,
    neighbors: Vec<u32>,
    /// Nodes expanded by the latest search.
    pub expanded: u32,
}

/// Integer A* over walk-surface nodes with stable ties: the nodes after `start` up to
/// `goal`. `blocked` closes nodes (occupancy, traffic, corridor); `can_step` checks the
/// body between neighbours.
pub fn path(
    surfaces: &WalkSurfaces,
    s: &mut SurfaceScratch,
    start: u32,
    goal: u32,
    blocked: impl Fn(&SurfaceNode) -> bool,
    max_cost: i64,
    mut can_step: impl FnMut(u32, u32) -> bool,
) -> Option<Vec<u32>> {
    s.expanded = 0;
    let target = surfaces.node(goal)?;
    surfaces.node(start)?;
    if !surfaces.walkable(goal) || blocked(&target) {
        return None;
    }
    if start == goal {
        return Some(Vec::new());
    }
    if !surfaces.connected(start, goal) {
        return None;
    }
    let n = surfaces.node_count() as usize;
    if s.seen.len() != n {
        *s = SurfaceScratch {
            cost: vec![0; n],
            prev: vec![0; n],
            seen: vec![0; n],
            closed: vec![0; n],
            ..SurfaceScratch::default()
        };
    }
    s.epoch += 1;
    if s.epoch == u32::MAX {
        s.seen.fill(0);
        s.closed.fill(0);
        s.epoch = 1;
    }
    let epoch = s.epoch;
    let heuristic = |p: &SurfaceNode| {
        let dx = i64::from(p.x.abs_diff(target.x));
        let dy = i64::from(p.y.abs_diff(target.y));
        dx.max(dy) * 1000 + dx.min(dy) * 414
    };
    let mut heap = BinaryHeap::new();
    s.cost[start as usize] = 0;
    s.seen[start as usize] = epoch;
    let h = heuristic(&surfaces.node(start)?);
    heap.push(Reverse((h, h, start)));
    let mut out = std::mem::take(&mut s.neighbors);
    let result = loop {
        let Some(Reverse((f, h, id))) = heap.pop() else {
            break None;
        };
        let g = f - h;
        if s.closed[id as usize] == epoch || s.cost[id as usize] != g {
            continue;
        }
        if id == goal {
            let mut route = Vec::new();
            let mut at = goal;
            while at != start {
                route.push(at);
                at = s.prev[at as usize];
            }
            route.reverse();
            break Some(route);
        }
        s.closed[id as usize] = epoch;
        s.expanded += 1;
        let a = surfaces.node(id).expect("node");
        surfaces.neighbors(id, &blocked, &mut out);
        for &next in &out {
            if s.closed[next as usize] == epoch || !can_step(id, next) {
                continue;
            }
            let b = surfaces.node(next).expect("node");
            let g2 = g + if a.x != b.x && a.y != b.y { 1414 } else { 1000 };
            let h2 = heuristic(&b);
            if g2 + h2 > max_cost || (s.seen[next as usize] == epoch && g2 >= s.cost[next as usize]) {
                continue;
            }
            s.seen[next as usize] = epoch;
            s.cost[next as usize] = g2;
            s.prev[next as usize] = id;
            heap.push(Reverse((g2 + h2, h2, next)));
        }
    };
    s.neighbors = out;
    result
}

/// Whether a body of `radius` (milli-cells) and `height` (world units) can move from
/// `from` to `to` on the layered grid: the centre line follows real surface edges (a
/// change of floor is a single portal step), and the footprint sweep may be supported
/// by portal-reachable nodes near a ramp. `walkable` is the live static walkability.
#[allow(clippy::too_many_arguments)]
pub fn clear_layered_sweep(
    graph: &WalkSurfaces,
    from: Fixed,
    to: Fixed,
    height: f64,
    radius: i32,
    pattern: &[(i32, i32)],
    walkable: &dyn Fn(u32) -> bool,
    blocked: Option<&dyn Fn(u32) -> bool>,
) -> bool {
    let size = graph.size();
    let node_of = |p: Fixed| {
        let (x, y) = (grid(p.x), grid(p.y));
        graph.node_at(i64::from(x), i64::from(y), p.deck)
    };
    let (Some(start), Some(goal)) = (node_of(from), node_of(to)) else {
        return false;
    };
    let clearance = crate::terrain::round_half_up(height * 100.0) as i32;
    let is_blocked = |id: u32| blocked.is_some_and(|b| b(id));
    let blocked_node =
        |n: &SurfaceNode| !walkable(n.id) || !graph.walkable_with(n.id, clearance) || is_blocked(n.id);
    let node = |cell: u32, deck: Option<DeckId>| {
        graph.node_at(i64::from(cell % size), i64::from(cell / size), deck)
    };
    if from.deck != to.deck {
        // Never smooth past a portal: the crossing is one cardinal edge.
        if !graph.step(start, goal, blocked_node) {
            return false;
        }
    } else if !clear_ray(from, to, size, |a, b| {
        let (Some(na), Some(nb)) = (node(a, from.deck), node(b, from.deck)) else {
            return false;
        };
        graph.step(na, nb, blocked_node)
    }) {
        return false;
    }
    let usable = |id: u32| walkable(id) && graph.walkable_with(id, clearance);
    // Nodes near a ramp reachable through real portals support a wide body's corners.
    let mut support: Option<BTreeMap<u32, Vec<u32>>> = None;
    let mut support_of = |cell: u32| -> Vec<u32> {
        let map = support.get_or_insert_with(|| {
            let limit = (radius + 999) / 1000 + 1;
            let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
            let mut seen = vec![start, goal];
            let mut queue = vec![(start, 0), (goal, 0)];
            let mut out = Vec::new();
            let mut i = 0;
            while i < queue.len() {
                let (id, depth) = queue[i];
                i += 1;
                if usable(id) {
                    map.entry(graph.node(id).expect("node").cell).or_default().push(id);
                }
                if depth >= limit {
                    continue;
                }
                graph.neighbors(id, blocked_node, &mut out);
                for &next in &out {
                    if !seen.contains(&next) {
                        seen.push(next);
                        queue.push((next, depth + 1));
                    }
                }
            }
            map
        });
        map.get(&cell).cloned().unwrap_or_default()
    };
    let mut candidates = |cell: u32| -> Vec<u32> {
        let mut result = Vec::with_capacity(2);
        for id in [node(cell, from.deck), node(cell, to.deck)].into_iter().flatten() {
            if usable(id) && !result.contains(&id) {
                result.push(id);
            }
        }
        if result.is_empty() {
            result.extend(support_of(cell).into_iter().filter(|&id| !is_blocked(id)));
        }
        result
    };
    clear_sweep(from, to, size, pattern, |a, b| {
        let (ca, cb) = (candidates(a), candidates(b));
        ca.iter()
            .any(|&na| cb.iter().any(|&nb| graph.step(na, nb, blocked_node)))
    })
}
