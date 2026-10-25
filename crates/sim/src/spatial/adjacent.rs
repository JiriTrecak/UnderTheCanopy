//! A body sweep between neighbouring cell centres, compiled once per radius and direction
//! into the list of directed cell edges it checks (old `adjacentSweep`,
//! `src/sim/game/adjacentSweep.ts`). Evaluating the list against the live terrain gives
//! exactly the answer of the full sweep.

use super::geometry::{Fixed, clear_sweep, sweep_pattern};
use crate::terrain::SteepEdges;

#[derive(Debug, Clone)]
struct Pattern {
    /// Directed edges as cell offsets from the origin cell: `(ax, ay, bx, by)`.
    edges: Vec<(i32, i32, i32, i32)>,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    /// Row runs `(dy, x0, x1)` of the cells the edges enter, each of which must be open.
    entered: Vec<(i32, i32, i32)>,
    /// Row runs of the cells entered from another cell, where a steep step could stop it.
    crossed: Vec<(i32, i32, i32)>,
    /// The steps between different cells, each once: the step's direction as its steep
    /// plane `(dy + 1) * 3 + dx + 1` and the entered cell.
    steps: Vec<(usize, i32, i32)>,
}

/// Row runs `(dy, x0, x1)` covering exactly `cells`.
fn runs(mut cells: Vec<(i32, i32)>) -> Vec<(i32, i32, i32)> {
    cells.sort_unstable_by_key(|&(x, y)| (y, x));
    cells.dedup();
    let mut out: Vec<(i32, i32, i32)> = Vec::new();
    for (x, y) in cells {
        match out.last_mut() {
            Some(run) if run.0 == y && run.2 + 1 == x => run.2 = x,
            _ => out.push((y, x, x)),
        }
    }
    out
}

/// Whether any bit of `bits` (rows of `words` words) is set in `x0..=x1` of row `y`.
#[inline]
fn any_bit(bits: &[u32], words: usize, y: usize, x0: usize, x1: usize) -> bool {
    let row = y * words;
    let (first, last) = (x0 / 32, x1 / 32);
    (first..=last).any(|w| {
        let low = if w == first {
            u32::MAX << (x0 % 32)
        } else {
            u32::MAX
        };
        let high = if w == last {
            u32::MAX >> (31 - x1 % 32)
        } else {
            u32::MAX
        };
        bits[row + w] & low & high != 0
    })
}

/// Every cell's neighbour steps, answered at once by whole rows of bits: a step is
/// refused when the sweep would leave the map, enter a closed cell or cross a steep step
/// (what [`AdjacentSweep::clear`] asks of the ground step), and passes otherwise. Bit `x`
/// of row `y` of direction `(dy + 1) * 3 + dx + 1`.
pub struct StepTable {
    words: usize,
    refused: Vec<Vec<u64>>,
}

impl StepTable {
    /// Whether the sweep from `from` to the neighbouring (or same) cell `to` passes.
    pub fn passes(&self, size: u32, from: u32, to: u32) -> bool {
        let (x, y) = (from % size, from / size);
        let (dx, dy) = ((to % size) as i32 - x as i32, (to / size) as i32 - y as i32);
        let k = ((dy + 1) * 3 + dx + 1) as usize;
        self.refused[k][y as usize * self.words + x as usize / 64] & (1 << (x % 64)) == 0
    }
}

/// Row `y` of `bits` (rows of `words` 64-bit words) shifted so bit `x` reads bit `x + t`.
fn shifted(bits: &[u64], words: usize, y: usize, t: i32, out: &mut [u64]) {
    let row = &bits[y * words..(y + 1) * words];
    for (w, o) in out.iter_mut().enumerate() {
        let at = |i: i64| -> u64 {
            if i < 0 || i >= words as i64 {
                0
            } else {
                row[i as usize]
            }
        };
        let (q, r) = ((t.div_euclid(64)) as i64, t.rem_euclid(64) as u32);
        let low = at(w as i64 + q);
        *o = if r == 0 {
            low
        } else {
            (low >> r) | (at(w as i64 + q + 1) << (64 - r))
        };
    }
}

/// Bit planes over the ground grid for [`AdjacentSweep::clear_planes`]: closed cells and
/// cells entered across a steep step, rows of `words` words.
#[derive(Clone, Copy)]
pub struct Planes<'a> {
    pub closed: &'a [u32],
    pub steep: &'a [u32],
    pub words: usize,
    /// A rectangle of cells `(x0, x1, y0, y1)` closed as well (a proposed foundation).
    pub avoid: Option<(i32, i32, i32, i32)>,
    /// `closed | steep`, when kept: one test over the entered cells clears open flat
    /// ground at once.
    pub rough: Option<&'a [u32]>,
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
                let entered = runs(edges.iter().map(|&(_, _, bx, by)| (bx, by)).collect());
                let crossed = runs(
                    edges
                        .iter()
                        .filter(|&&(ax, ay, bx, by)| (ax, ay) != (bx, by))
                        .map(|&(_, _, bx, by)| (bx, by))
                        .collect(),
                );
                let mut steps: Vec<(usize, i32, i32)> = edges
                    .iter()
                    .filter(|&&(ax, ay, bx, by)| (ax, ay) != (bx, by))
                    .map(|&(ax, ay, bx, by)| (((by - ay + 1) * 3 + bx - ax + 1) as usize, bx, by))
                    .collect();
                steps.sort_unstable();
                steps.dedup();
                Pattern {
                    edges,
                    min_x,
                    max_x,
                    min_y,
                    max_y,
                    entered,
                    crossed,
                    steps,
                }
            })
            .collect();
        Self { patterns }
    }

    /// Whether the sweep from cell `from` to the neighbouring (or same) cell `to` passes.
    pub fn clear(
        &self,
        size: u32,
        from: u32,
        to: u32,
        mut step: impl FnMut(u32, u32) -> bool,
    ) -> bool {
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

    /// Every cell's neighbour steps over ground with `closed` cells (rows of
    /// `steep.words_per_row()` words) and `steep` steps, at once ([`StepTable`]).
    pub fn table(&self, size: u32, closed: &[u32], steep: &SteepEdges) -> StepTable {
        let size = size as usize;
        let words = size.div_ceil(64);
        let words32 = steep.words_per_row();
        let widen = |plane: &[u32]| {
            let mut out = vec![0u64; words * size];
            for y in 0..size {
                for w in 0..words32 {
                    let v = u64::from(plane[y * words32 + w]);
                    out[y * words + w / 2] |= v << (32 * (w % 2));
                }
            }
            out
        };
        let closed = widen(closed);
        let planes: Vec<Vec<u64>> = (0..9)
            .map(|k| widen(steep.plane(k % 3 - 1, k / 3 - 1)))
            .collect();
        let mut shift = vec![0u64; words];
        let refused = self
            .patterns
            .iter()
            .map(|p| {
                let mut refuse = vec![u64::MAX; words * size];
                for y in 0..size as i32 {
                    if y + p.min_y < 0 || y + p.max_y >= size as i32 {
                        continue;
                    }
                    let no = &mut refuse[y as usize * words..(y as usize + 1) * words];
                    no.iter_mut().for_each(|w| *w = 0);
                    for x in 0..size as i32 {
                        if x + p.min_x < 0 || x + p.max_x >= size as i32 {
                            no[x as usize / 64] |= 1 << (x % 64);
                        }
                    }
                    // Every edge enters an open cell, on the level when it comes from another.
                    for &(ry, x0, x1) in &p.entered {
                        for t in x0..=x1 {
                            shifted(&closed, words, (y + ry) as usize, t, &mut shift);
                            no.iter_mut().zip(&shift).for_each(|(a, s)| *a |= s);
                        }
                    }
                    for &(k, bx, by) in &p.steps {
                        shifted(&planes[k], words, (y + by) as usize, bx, &mut shift);
                        no.iter_mut().zip(&shift).for_each(|(a, s)| *a |= s);
                    }
                }
                refuse
            })
            .collect();
        StepTable { words, refused }
    }

    /// Whether the sweep from `from` to the neighbouring `to` enters a cell of the rectangle
    /// `(x0, x1, y0, y1)` (false when it would leave the map).
    pub fn enters(
        &self,
        size: u32,
        from: u32,
        to: u32,
        (ax0, ax1, ay0, ay1): (i32, i32, i32, i32),
    ) -> bool {
        let (x, y) = ((from % size) as i32, (from / size) as i32);
        let (dx, dy) = ((to % size) as i32 - x, (to / size) as i32 - y);
        if dx.abs() > 1 || dy.abs() > 1 {
            return false;
        }
        let p = &self.patterns[((dy + 1) * 3 + dx + 1) as usize];
        p.entered
            .iter()
            .any(|&(ry, x0, x1)| (ay0..=ay1).contains(&(y + ry)) && x + x0 <= ax1 && x + x1 >= ax0)
    }

    /// [`AdjacentSweep::clear`] for the ground step (the entered cell open, the height
    /// change at most a ground step), read from bit planes of the same grid: every entered
    /// cell open and no steep step near decides at once; a steep step near takes the edges
    /// one by one.
    pub fn clear_planes(
        &self,
        size: u32,
        from: u32,
        to: u32,
        planes: Planes,
        step: impl FnMut(u32, u32) -> bool,
    ) -> bool {
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
        let at = |&(ry, x0, x1): &(i32, i32, i32)| {
            ((y + ry) as usize, (x + x0) as usize, (x + x1) as usize)
        };
        let avoided = |ry: usize, x0: usize, x1: usize| {
            planes.avoid.is_some_and(|(ax0, ax1, ay0, ay1)| {
                (ay0..=ay1).contains(&(ry as i32)) && x0 as i32 <= ax1 && x1 as i32 >= ax0
            })
        };
        // Crossed cells are entered cells: nothing rough among the entered decides.
        if let Some(rough) = planes.rough
            && p.entered.iter().all(|run| {
                let (ry, x0, x1) = at(run);
                !any_bit(rough, planes.words, ry, x0, x1) && !avoided(ry, x0, x1)
            })
        {
            return true;
        }
        for run in &p.entered {
            let (ry, x0, x1) = at(run);
            if any_bit(planes.closed, planes.words, ry, x0, x1) {
                return false;
            }
            if let Some((ax0, ax1, ay0, ay1)) = planes.avoid
                && (ay0..=ay1).contains(&(ry as i32))
                && x0 as i32 <= ax1
                && x1 as i32 >= ax0
            {
                return false;
            }
        }
        let steep = p.crossed.iter().any(|run| {
            let (ry, x0, x1) = at(run);
            any_bit(planes.steep, planes.words, ry, x0, x1)
        });
        !steep || self.clear(size, from, to, step)
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

    #[test]
    fn bit_planes_answer_as_the_edges() {
        // 40 and 70 cells: rows of two or three 32-bit words, one or two 64-bit ones.
        // Closed cells and a terraced patch with steep steps.
        for size in [40u32, 70] {
            planes_answer_as_the_edges(size);
        }
    }

    fn planes_answer_as_the_edges(size: u32) {
        use crate::{
            spatial::clearance::Clearance,
            terrain::{MAX_GROUND_STEP_CM, SteepEdges},
        };
        let n = (size * size) as usize;
        let walk: Vec<bool> = (0..n as u32)
            .map(|c| !(c % 13 == 4 || c % 29 == 7))
            .collect();
        let heights: Vec<i16> = (0..n)
            .map(|c| {
                let (x, y) = (c as u32 % size, c as u32 / size);
                if (20..30).contains(&x) && y > 10 {
                    ((x + y) % 3) as i16 * 60
                } else {
                    0
                }
            })
            .collect();
        let steep = SteepEdges::new(size, &heights);
        let clearance = Clearance::new(size, |c| walk[c as usize]).with_steep(steep.any());
        let planes = Planes {
            closed: clearance.closed(),
            steep: steep.any(),
            words: steep.words_per_row(),
            avoid: None,
            rough: None,
        };
        let step = |a: u32, b: u32| {
            walk[b as usize]
                && (i32::from(heights[a as usize]) - i32::from(heights[b as usize])).abs()
                    <= MAX_GROUND_STEP_CM
        };
        for radius in [500, 1250, 2500] {
            let compiled = AdjacentSweep::new(radius);
            let table = compiled.table(size, clearance.closed(), &steep);
            for from in 0..size * size {
                for (dx, dy) in [(0, 0), (1, 0), (-1, 1), (1, 1), (0, -1), (-1, -1)] {
                    let (x, y) = ((from % size) as i32 + dx, (from / size) as i32 + dy);
                    if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                        continue;
                    }
                    let to = y as u32 * size + x as u32;
                    let exact = compiled.clear(size, from, to, step);
                    assert_eq!(
                        compiled.clear_planes(size, from, to, planes, step),
                        exact,
                        "radius {radius} from {from} to {to}"
                    );
                    assert_eq!(
                        table.passes(size, from, to),
                        exact,
                        "radius {radius} from {from} to {to}, table"
                    );
                    let rough = Planes {
                        rough: Some(clearance.rough()),
                        ..planes
                    };
                    assert_eq!(
                        compiled.clear_planes(size, from, to, rough, step),
                        exact,
                        "radius {radius} from {from} to {to}, rough cells"
                    );
                }
            }
        }
    }
}
