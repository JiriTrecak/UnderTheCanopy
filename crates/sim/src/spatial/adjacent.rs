//! A body sweep between neighbouring cell centres, compiled once per radius and direction
//! into the list of directed cell edges it checks (old `adjacentSweep`,
//! `src/sim/game/adjacentSweep.ts`). Evaluating the list against the live terrain gives
//! exactly the answer of the full sweep.

use super::geometry::{Fixed, clear_sweep, sweep_pattern};

#[derive(Debug, Clone)]
struct Pattern {
    /// Directed edges as cell offsets from the origin cell: `(ax, ay, bx, by)`.
    edges: Vec<(i32, i32, i32, i32)>,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
}

#[derive(Debug, Clone)]
pub struct AdjacentSweep {
    patterns: Vec<Pattern>,
}

impl AdjacentSweep {
    pub fn new(radius: i32) -> Self {
        let rays = sweep_pattern(radius);
        let origin = (radius + 999) / 1000 + 2;
        let width = (origin * 2 + 3) as u32;
        let patterns = (0..9)
            .map(|k| {
                let (dx, dy) = (k % 3 - 1, k / 3 - 1);
                let mut edges = Vec::new();
                let (mut min_x, mut max_x, mut min_y, mut max_y) = (0, 0, 0, 0);
                let from = Fixed::new(origin * 1000, origin * 1000);
                let to = Fixed::new((origin + dx) * 1000, (origin + dy) * 1000);
                clear_sweep(from, to, width, &rays, |a, b| {
                    let w = width as i32;
                    let (ax, ay) = (a as i32 % w - origin, a as i32 / w - origin);
                    let (bx, by) = (b as i32 % w - origin, b as i32 / w - origin);
                    min_x = min_x.min(ax).min(bx);
                    max_x = max_x.max(ax).max(bx);
                    min_y = min_y.min(ay).min(by);
                    max_y = max_y.max(ay).max(by);
                    if !edges.contains(&(ax, ay, bx, by)) {
                        edges.push((ax, ay, bx, by));
                    }
                    true
                });
                Pattern {
                    edges,
                    min_x,
                    max_x,
                    min_y,
                    max_y,
                }
            })
            .collect();
        Self { patterns }
    }

    /// Whether the sweep from cell `from` to the neighbouring (or same) cell `to` passes.
    pub fn clear(&self, size: u32, from: u32, to: u32, mut step: impl FnMut(u32, u32) -> bool) -> bool {
        let n = size * size;
        if from >= n || to >= n {
            return false;
        }
        let s = size as i32;
        let (x, y) = ((from % size) as i32, (from / size) as i32);
        let (dx, dy) = ((to % size) as i32 - x, (to / size) as i32 - y);
        if dx.abs() > 1 || dy.abs() > 1 {
            return false;
        }
        let p = &self.patterns[((dy + 1) * 3 + dx + 1) as usize];
        if x + p.min_x < 0 || x + p.max_x >= s || y + p.min_y < 0 || y + p.max_y >= s {
            return false;
        }
        let base = from as i32;
        p.edges
            .iter()
            .all(|&(ax, ay, bx, by)| step((base + ay * s + ax) as u32, (base + by * s + bx) as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_sweeps_match_full_sweeps() {
        let size = 24u32;
        // A scatter of closed cells.
        let closed = |c: u32| c % 7 == 3 || c % 11 == 5;
        let step = |_: u32, b: u32| !closed(b);
        for radius in [400, 900, 1500, 2300] {
            let compiled = AdjacentSweep::new(radius);
            let rays = sweep_pattern(radius);
            for from in 0..size * size {
                for (dx, dy) in [(0, 0), (1, 0), (-1, 1), (1, 1), (0, -1)] {
                    let (x, y) = ((from % size) as i32 + dx, (from / size) as i32 + dy);
                    if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                        continue;
                    }
                    let to = y as u32 * size + x as u32;
                    let a = Fixed::new((from % size) as i32 * 1000, (from / size) as i32 * 1000);
                    let b = Fixed::new(x * 1000, y * 1000);
                    assert_eq!(
                        compiled.clear(size, from, to, step),
                        clear_sweep(a, b, size, &rays, step),
                        "radius {radius} from {from} to {to}"
                    );
                }
            }
        }
    }
}
