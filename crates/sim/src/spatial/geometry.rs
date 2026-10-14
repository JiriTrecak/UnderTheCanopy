//! Fixed-point segment geometry on the navigation grid (old `src/sim/game/motion.ts`).
//!
//! Positions are milli-cells; cell `n` spans `[n·1000 − 500, n·1000 + 500)`. A ray visits
//! every cell its centre line crosses (an integer supercover DDA); an exact corner crossing
//! also checks both side cells. A body sweep traces a fixed pattern of parallel rays over
//! its square footprint.

use crate::terrain::DeckId;
use std::collections::BTreeMap;

/// A fixed-point position on the ground or on a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fixed {
    pub x: i32,
    pub y: i32,
    pub deck: Option<DeckId>,
}

impl Fixed {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y, deck: None }
    }

    pub const fn on(x: i32, y: i32, deck: Option<DeckId>) -> Self {
        Self { x, y, deck }
    }

    /// The centre of cell `(x, y)`.
    pub const fn cell(x: i32, y: i32, deck: Option<DeckId>) -> Self {
        Self {
            x: x * 1000,
            y: y * 1000,
            deck,
        }
    }

    /// The cell containing the point (half cells round up).
    pub fn grid(self) -> (i32, i32) {
        (grid(self.x), grid(self.y))
    }

    pub fn same_place(self, other: Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}

/// The cell coordinate of a milli-cell coordinate.
pub fn grid(v: i32) -> i32 {
    (v + 500).div_euclid(1000)
}

/// Integer length rounded up.
pub fn length_ceil(dx: i64, dy: i64) -> i64 {
    crate::math::length_ceil(dx, dy) as i64
}

/// Exact Euclidean length of an integer vector.
pub fn length(dx: i64, dy: i64) -> f64 {
    ((dx * dx + dy * dy) as f64).sqrt()
}

/// Integer supercover DDA between two fixed-point coordinates. `step(a, b)` is asked for
/// the start cell (`a == b`), every crossed edge, and both side cells of an exact corner;
/// `visit` receives every cell entered after the start.
#[inline]
pub fn ray(
    from: (i64, i64),
    to: (i64, i64),
    size: i64,
    mut step: impl FnMut(u32, u32) -> bool,
    mut visit: impl FnMut(u32),
) -> bool {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (ax, ay, sx, sy) = (dx.abs(), dy.abs(), dx.signum(), dy.signum());
    let mut x = (from.0 + 500).div_euclid(1000);
    let mut y = (from.1 + 500).div_euclid(1000);
    let tx = (to.0 + 500).div_euclid(1000);
    let ty = (to.1 + 500).div_euclid(1000);
    if x.min(y).min(tx).min(ty) < 0 || x.max(y).max(tx).max(ty) >= size {
        return false;
    }
    let start = (y * size + x) as u32;
    if !step(start, start) {
        return false;
    }
    let mut cross_x = if sx != 0 {
        (x * 1000 + sx * 500 - from.0).abs()
    } else {
        0
    };
    let mut cross_y = if sy != 0 {
        (y * 1000 + sy * 500 - from.1).abs()
    } else {
        0
    };
    while x != tx || y != ty {
        let prior = y * size + x;
        // Positive steps along y, negative along x, zero through an exact corner.
        let vertical = if x == tx {
            1
        } else if y == ty {
            -1
        } else if sx == 0 {
            1
        } else if sy == 0 {
            -1
        } else {
            cross_x * ay - cross_y * ax
        };
        if vertical <= 0 {
            x += sx;
            cross_x += 1000;
        }
        if vertical >= 0 {
            y += sy;
            cross_y += 1000;
        }
        let next = y * size + x;
        if !step(prior as u32, next as u32) {
            return false;
        }
        if vertical == 0 {
            let side_x = (prior + sx) as u32;
            let side_y = (prior + sy * size) as u32;
            let (p, n) = (prior as u32, next as u32);
            if !step(p, side_x) || !step(p, side_y) || !step(side_x, n) || !step(side_y, n) {
                return false;
            }
        }
        visit(next as u32);
    }
    true
}

/// [`ray`] between two fixed points.
#[inline]
pub fn clear_ray(from: Fixed, to: Fixed, size: u32, step: impl FnMut(u32, u32) -> bool) -> bool {
    ray(
        (i64::from(from.x), i64::from(from.y)),
        (i64::from(to.x), i64::from(to.y)),
        i64::from(size),
        step,
        |_| {},
    )
}

/// Rasterizes the polyline segment, appending every entered cell to `path`.
pub fn append_ray_cells(
    from: Fixed,
    to: Fixed,
    size: u32,
    step: impl FnMut(u32, u32) -> bool,
    path: &mut Vec<u32>,
) -> bool {
    ray(
        (i64::from(from.x), i64::from(from.y)),
        (i64::from(to.x), i64::from(to.y)),
        i64::from(size),
        step,
        |cell| path.push(cell),
    )
}

/// [`append_ray_cells`] between cell centres, checking the destination end first. Edges
/// keep their forward direction; the emitted cells keep forward order.
pub fn append_ray_cells_from_end(
    from: Fixed,
    to: Fixed,
    size: u32,
    mut step: impl FnMut(u32, u32) -> bool,
    path: &mut Vec<u32>,
) -> bool {
    if from.x % 1000 != 0 || from.y % 1000 != 0 || to.x % 1000 != 0 || to.y % 1000 != 0 {
        return append_ray_cells(from, to, size, step, path);
    }
    let (x, y, tx, ty) = (from.x / 1000, from.y / 1000, to.x / 1000, to.y / 1000);
    let size_i = size as i32;
    if x.min(y).min(tx).min(ty) < 0 || x.max(y).max(tx).max(ty) >= size_i {
        return false;
    }
    let origin = (y * size_i + x) as u32;
    if !step(origin, origin) {
        return false;
    }
    let offset = path.len();
    let reverse = ray(
        (i64::from(to.x), i64::from(to.y)),
        (i64::from(from.x), i64::from(from.y)),
        i64::from(size),
        |a, b| a == b || step(b, a),
        |cell| path.push(cell),
    );
    if !reverse {
        return false;
    }
    if path.len() > offset {
        path.pop();
        path[offset..].reverse();
        path.push((ty * size_i + tx) as u32);
    }
    true
}

/// Ray offsets of a body sweep: the centre and four corners, plus an interior lattice with
/// sub-cell gaps for bodies of half a cell or more. Each ray appears once, in order.
pub fn sweep_pattern(radius: i32) -> Vec<(i32, i32)> {
    let r = radius;
    let mut offsets = vec![(0, 0), (-r, -r), (r, -r), (-r, r), (r, r)];
    if r >= 500 {
        let divisions = (2 * i64::from(r) + 899) / 900;
        let at = |i: i64| {
            let v = -f64::from(r) + (2.0 * f64::from(r) * i as f64) / divisions as f64;
            crate::terrain::round_half_up(v) as i32
        };
        for x in 0..=divisions {
            for y in 0..=divisions {
                offsets.push((at(x), at(y)));
            }
        }
    }
    let mut unique: Vec<(i32, i32)> = Vec::with_capacity(offsets.len());
    for offset in offsets {
        if !unique.contains(&offset) {
            unique.push(offset);
        }
    }
    unique
}

/// Sweep patterns by radius, computed once.
#[derive(Debug, Default, Clone)]
pub struct Patterns {
    patterns: BTreeMap<i32, Vec<(i32, i32)>>,
}

impl Patterns {
    pub fn get(&mut self, radius: i32) -> &[(i32, i32)] {
        self.patterns
            .entry(radius)
            .or_insert_with(|| sweep_pattern(radius))
    }
}

/// Sweeps a square footprint of `radius` from `from` to `to` with parallel rays.
pub fn clear_sweep(
    from: Fixed,
    to: Fixed,
    size: u32,
    pattern: &[(i32, i32)],
    mut step: impl FnMut(u32, u32) -> bool,
) -> bool {
    pattern.iter().all(|&(ox, oy)| {
        ray(
            (i64::from(from.x + ox), i64::from(from.y + oy)),
            (i64::from(to.x + ox), i64::from(to.y + oy)),
            i64::from(size),
            &mut step,
            |_| {},
        )
    })
}

/// Whether moving along `(dx, dy)` from `from` brings a body at `(x, y)` within
/// `separation` (squared). Motion that separates an existing overlap is allowed.
#[inline]
pub fn body_blocks_segment(
    from: Fixed,
    dx: i64,
    dy: i64,
    square: i64,
    x: i64,
    y: i64,
    separation_squared: i64,
) -> bool {
    let sx = x - i64::from(from.x);
    let sy = y - i64::from(from.y);
    if square != 0 && sx * sx + sy * sy < separation_squared && sx * dx + sy * dy <= 0 {
        return false;
    }
    let t = if square != 0 {
        ((sx * dx + sy * dy) as f64 / square as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let ex = sx as f64 - t * dx as f64;
    let ey = sy as f64 - t * dy as f64;
    ex * ex + ey * ey < separation_squared as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(from: Fixed, to: Fixed) -> Vec<u32> {
        let mut out = Vec::new();
        assert!(append_ray_cells(from, to, 16, |_, _| true, &mut out));
        out
    }

    #[test]
    fn rays_visit_every_crossed_cell_and_both_corner_sides() {
        assert_eq!(cells(Fixed::new(0, 0), Fixed::new(3000, 0)), [1, 2, 3]);
        assert_eq!(cells(Fixed::new(0, 0), Fixed::new(2000, 1000)), [1, 17, 18]);
        // An exact diagonal checks both side cells of each corner.
        let mut edges = Vec::new();
        assert!(clear_ray(Fixed::new(0, 0), Fixed::new(1000, 1000), 16, |a, b| {
            edges.push((a, b));
            true
        }));
        assert_eq!(edges, [(0, 0), (0, 17), (0, 1), (0, 16), (1, 17), (16, 17)]);
        // Off the map fails; a blocked side cell blocks the diagonal.
        assert!(!clear_ray(Fixed::new(0, 0), Fixed::new(-600, 0), 16, |_, _| true));
        assert!(!clear_ray(Fixed::new(0, 0), Fixed::new(1000, 1000), 16, |_, b| b != 1));
    }

    #[test]
    fn rays_from_the_end_match_forward_rays() {
        let (from, to) = (Fixed::new(1000, 2000), Fixed::new(9000, 5000));
        let mut forward = Vec::new();
        let mut backward = Vec::new();
        assert!(append_ray_cells(from, to, 16, |_, _| true, &mut forward));
        assert!(append_ray_cells_from_end(from, to, 16, |_, _| true, &mut backward));
        assert_eq!(forward, backward);
    }

    #[test]
    fn wide_bodies_sweep_an_interior_lattice() {
        assert_eq!(sweep_pattern(400).len(), 5);
        // 2·1500/900 → 4 divisions: a 5×5 lattice whose corners and centre repeat.
        assert_eq!(sweep_pattern(1500).len(), 25);
        assert!(sweep_pattern(1500).contains(&(-750, 750)));
    }

    #[test]
    fn separating_motion_escapes_an_overlap() {
        let from = Fixed::new(0, 0);
        // A body 300 to the east, separation 800: moving east is blocked, west is not.
        assert!(body_blocks_segment(from, 100, 0, 10_000, 300, 0, 640_000));
        assert!(!body_blocks_segment(from, -100, 0, 10_000, 300, 0, 640_000));
        // Passing alongside at a distance is clear.
        assert!(!body_blocks_segment(
            from, 5000, 0, 25_000_000, 2500, 900, 640_000
        ));
    }
}
