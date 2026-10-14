//! Bounded local escapes (old `localPath`, `src/sim/game/localPath.ts`): A* on a
//! world-anchored lattice of `spacing` milli-cells around the mover, at most 16 steps out
//! and 256 expansions, using the caller's swept clearance for every edge. The mover's exact
//! start is a special node; it is never snapped.

use crate::spatial::{Fixed, geometry::length_ceil};
use std::{cmp::Reverse, collections::BinaryHeap};

const RADIUS: i32 = 16;
const WIDTH: i32 = RADIUS * 2 + 1;
const MAX_VISITS: u32 = 256;
const OFFSETS: [(i32, i32); 8] = [
    (-1, 0),
    (0, -1),
    (1, 0),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Reusable search state.
#[derive(Debug, Clone)]
pub struct LocalScratch {
    cost: Vec<i64>,
    previous: Vec<u32>,
    seen: Vec<u32>,
    closed: Vec<u32>,
    epoch: u32,
    heap: BinaryHeap<Reverse<(i64, i64, u32)>>,
}

impl Default for LocalScratch {
    fn default() -> Self {
        let n = (WIDTH * WIDTH) as usize;
        Self {
            cost: vec![0; n],
            previous: vec![0; n],
            seen: vec![0; n],
            closed: vec![0; n],
            epoch: 0,
            heap: BinaryHeap::new(),
        }
    }
}

/// Waypoints from `from` to `to` (ending at `to`), straightened, or `None`.
pub fn local_path(
    scratch: &mut LocalScratch,
    from: Fixed,
    to: Fixed,
    spacing: i32,
    mut clear: impl FnMut(Fixed, Fixed) -> bool,
) -> Option<Vec<Fixed>> {
    let (dx, dy) = (i64::from(to.x - from.x), i64::from(to.y - from.y));
    let reach = i64::from(RADIUS) * i64::from(spacing);
    if dx * dx + dy * dy > reach * reach {
        return None;
    }
    if clear(from, to) {
        return Some(vec![to]);
    }
    let start = (RADIUS * WIDTH + RADIUS) as u32;
    // Anchor the lattice to the world so a clear centre between bodies is not missed.
    let anchor = |v: i32| {
        let (v, s) = (i64::from(v), i64::from(spacing));
        ((2 * v + s).div_euclid(2 * s) * s) as i32
    };
    let (origin_x, origin_y) = (anchor(from.x), anchor(from.y));
    let position = |id: u32| {
        if id == start {
            return from;
        }
        let (gx, gy) = (id as i32 % WIDTH, id as i32 / WIDTH);
        Fixed::on(
            origin_x + (gx - RADIUS) * spacing,
            origin_y + (gy - RADIUS) * spacing,
            from.deck,
        )
    };
    scratch.epoch = scratch.epoch.wrapping_add(1);
    if scratch.epoch == 0 {
        scratch.seen.fill(0);
        scratch.closed.fill(0);
        scratch.epoch = 1;
    }
    let epoch = scratch.epoch;
    scratch.heap.clear();
    scratch.seen[start as usize] = epoch;
    scratch.cost[start as usize] = 0;
    let h0 = length_ceil(dx, dy);
    scratch.heap.push(Reverse((h0, h0, start)));
    let mut visits = 0;
    while visits < MAX_VISITS {
        let Some(Reverse((f, h, current))) = scratch.heap.pop() else {
            break;
        };
        let g = f - h;
        let c = current as usize;
        if scratch.closed[c] == epoch || scratch.cost[c] != g {
            continue;
        }
        scratch.closed[c] = epoch;
        visits += 1;
        let p = position(current);
        if current != start && clear(p, to) {
            let mut path = vec![to];
            let mut at = current;
            while at != start {
                path.push(position(at));
                at = scratch.previous[at as usize];
            }
            path.reverse();
            // Straighten only this escape.
            let mut result = Vec::with_capacity(path.len());
            let mut anchor = from;
            let mut i = 0;
            while i < path.len() {
                let mut far = i;
                while far + 1 < path.len() && clear(anchor, path[far + 1]) {
                    far += 1;
                }
                result.push(path[far]);
                anchor = path[far];
                i = far + 1;
            }
            return Some(result);
        }
        let (cx, cy) = (current as i32 % WIDTH, current as i32 / WIDTH);
        for (ox, oy) in OFFSETS {
            let (x, y) = (cx + ox, cy + oy);
            if x < 0 || y < 0 || x >= WIDTH || y >= WIDTH {
                continue;
            }
            let id = (y * WIDTH + x) as u32;
            let i = id as usize;
            if scratch.closed[i] == epoch {
                continue;
            }
            let q = position(id);
            let g2 = g + length_ceil(i64::from(q.x - p.x), i64::from(q.y - p.y));
            if scratch.seen[i] == epoch && g2 >= scratch.cost[i] {
                continue;
            }
            if !clear(p, q) {
                continue;
            }
            scratch.seen[i] = epoch;
            scratch.cost[i] = g2;
            scratch.previous[i] = current;
            let h2 = length_ceil(i64::from(to.x - q.x), i64::from(to.y - q.y));
            scratch.heap.push(Reverse((g2 + h2, h2, id)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_around_a_wall_within_the_bound() {
        let mut scratch = LocalScratch::default();
        // A wall along x = 1000 for y in [-1500, 1500].
        let blocked = |a: Fixed, b: Fixed| {
            let crosses = (a.x < 1000) != (b.x < 1000) || a.x == 1000 || b.x == 1000;
            let y = (a.y + b.y) / 2;
            crosses && (-1500..=1500).contains(&y)
        };
        let from = Fixed::new(0, 0);
        let to = Fixed::new(2000, 0);
        let path = local_path(&mut scratch, from, to, 250, |a, b| !blocked(a, b)).expect("escape");
        assert_eq!(path.last(), Some(&to));
        let mut anchor = from;
        for p in &path {
            assert!(!blocked(anchor, *p));
            anchor = *p;
        }
        // Too far for the bounded search.
        assert!(local_path(&mut scratch, from, Fixed::new(9000, 0), 250, |_, _| false).is_none());
    }
}
