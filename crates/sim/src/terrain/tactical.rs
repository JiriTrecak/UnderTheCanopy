//! Height rules for sight and weapons on the ground grid (old `TacticalTerrain`,
//! `src/shared/map/tacticalTerrain.ts`).
//!
//! Sight is height-filtered, not ray-occluded: a target is visible when it stands no more
//! than [`UPHILL_VISIBILITY_CM`] above the sensor. Shots need a clear straight corridor;
//! melee needs a gentle path. Coarse per-block maxima let most queries skip ray marching
//! without changing any answer.

use crate::math;

/// Eye and muzzle height above the floor. Model size never changes tactical sight.
pub const SIGHT_HEIGHT_CM: i32 = 120;
/// Higher ground stays visible up to this rise above the sensor.
pub const UPHILL_VISIBILITY_CM: i32 = 200;
/// The largest height step a ground body walks or strikes across.
pub const MAX_GROUND_STEP_CM: i32 = 90;

/// Cells per side of a fine and a coarse height block.
const FINE: usize = 4;
const COARSE: usize = 16;

/// A position in cells (fractional), with an elevation above its floor in world units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TerrainPoint {
    pub x: f64,
    pub y: f64,
    pub elevation: f64,
}

impl TerrainPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            elevation: 0.0,
        }
    }
}

/// Visible cells as sorted, disjoint half-open id ranges `[start, end)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VisibleSpans {
    pub spans: Vec<(u32, u32)>,
    pub cells: u32,
}

impl VisibleSpans {
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.spans.iter().flat_map(|&(start, end)| start..end)
    }
}

/// Immutable ground heights with block maxima for quick rejection.
#[derive(Debug, Clone)]
pub struct TacticalTerrain {
    size: usize,
    heights: Vec<i16>,
    flat: bool,
    fine_side: usize,
    fine: Vec<i16>,
    coarse_side: usize,
    coarse: Vec<i16>,
}

impl TacticalTerrain {
    /// `heights` holds `size²` cell heights in centimetres, row-major.
    pub fn new(size: u32, heights: Vec<i16>) -> Self {
        let size = size as usize;
        assert_eq!(heights.len(), size * size, "one height per cell");
        let fine_side = size.div_ceil(FINE);
        let coarse_side = size.div_ceil(COARSE);
        let mut fine = vec![i16::MIN; fine_side * fine_side];
        let mut coarse = vec![i16::MIN; coarse_side * coarse_side];
        for y in 0..size {
            for x in 0..size {
                let h = heights[y * size + x];
                let f = &mut fine[y / FINE * fine_side + x / FINE];
                *f = (*f).max(h);
                let c = &mut coarse[y / COARSE * coarse_side + x / COARSE];
                *c = (*c).max(h);
            }
        }
        let flat = heights.iter().all(|&h| h == heights[0]);
        Self {
            size,
            heights,
            flat,
            fine_side,
            fine,
            coarse_side,
            coarse,
        }
    }

    pub fn size(&self) -> u32 {
        self.size as u32
    }

    /// Cell heights in centimetres, row-major.
    pub fn heights(&self) -> &[i16] {
        &self.heights
    }

    /// Whether every cell has the same height.
    pub fn flat(&self) -> bool {
        self.flat
    }

    /// Ground height at a point in centimetres, bilinear between cell centres; positions
    /// off the map take the nearest edge.
    pub fn height(&self, x: f64, y: f64) -> f64 {
        let last = (self.size - 1) as f64;
        let x = x.clamp(0.0, last);
        let y = y.clamp(0.0, last);
        let (ix, iy) = (x.floor() as usize, y.floor() as usize);
        let jx = (ix + 1).min(self.size - 1);
        let jy = (iy + 1).min(self.size - 1);
        let (u, v) = (x - ix as f64, y - iy as f64);
        let n = self.size;
        let h = |x: usize, y: usize| f64::from(self.heights[y * n + x]);
        (h(ix, iy) * (1.0 - u) + h(jx, iy) * u) * (1.0 - v)
            + (h(ix, jy) * (1.0 - u) + h(jx, jy) * u) * v
    }

    /// RTS sight: `b` is visible from `a` unless it stands clearly higher.
    pub fn visible(&self, a: TerrainPoint, b: TerrainPoint) -> bool {
        let from = self.height(a.x, a.y) + a.elevation * 100.0;
        self.height(b.x, b.y) + b.elevation * 100.0 <= from + f64::from(UPHILL_VISIBILITY_CM)
    }

    /// Whether terrain leaves a straight weapon corridor between the two eye heights.
    pub fn shot_clear(&self, a: TerrainPoint, b: TerrainPoint) -> bool {
        if self.flat {
            return true;
        }
        let sight = f64::from(SIGHT_HEIGHT_CM);
        let from = self.height(a.x, a.y) + a.elevation * 100.0 + sight;
        let to = self.height(b.x, b.y) + b.elevation * 100.0 + sight;
        self.corridor_clear(a, b, from, to)
    }

    fn corridor_clear(&self, a: TerrainPoint, b: TerrainPoint, from: f64, to: f64) -> bool {
        let (ax, ay, dx, dy) = (a.x, a.y, b.x - a.x, b.y - a.y);
        // A level shelf: the highest block on the way is below both ends.
        let block = |v: f64| v.floor() as i64;
        let coarse_last = self.coarse_side as i64 - 1;
        let min_y = block(ay.min(b.y) / COARSE as f64).max(0);
        let max_y = block((ay.max(b.y) + 1.0) / COARSE as f64).min(coarse_last);
        let min_x = block(ax.min(b.x) / COARSE as f64).max(0);
        let max_x = block((ax.max(b.x) + 1.0) / COARSE as f64).min(coarse_last);
        let mut maximum = i32::from(i16::MIN);
        // Half-open ranges: the inclusive ones cost a check each step in this hot loop.
        for y in min_y..max_y + 1 {
            for x in min_x..max_x + 1 {
                maximum = maximum.max(i32::from(
                    self.coarse[y as usize * self.coarse_side + x as usize],
                ));
            }
        }
        if f64::from(maximum) <= from.min(to) {
            return true;
        }
        let steps = (dx.abs().max(dy.abs()) * 4.0).ceil().max(1.0) as i64;
        let last = (self.size - 1) as f64;
        let fine_last = self.fine_side as i64 - 1;
        let total = steps as f64;
        let mut start = 1;
        while start < steps {
            let end = (start + 7).min(steps - 1);
            let (t0, t1) = (start as f64 / total, end as f64 / total);
            let x0 = (ax + dx * t0).clamp(0.0, last);
            let x1 = (ax + dx * t1).clamp(0.0, last);
            let y0 = (ay + dy * t0).clamp(0.0, last);
            let y1 = (ay + dy * t1).clamp(0.0, last);
            let fx0 = block(x0.min(x1) / FINE as f64);
            let fx1 = block((x0.max(x1) + 1.0) / FINE as f64).min(fine_last);
            let fy0 = block(y0.min(y1) / FINE as f64);
            let fy1 = block((y0.max(y1) + 1.0) / FINE as f64).min(fine_last);
            let mut high = i32::from(i16::MIN);
            for y in fy0..fy1 + 1 {
                for x in fx0..fx1 + 1 {
                    high = high.max(i32::from(
                        self.fine[y as usize * self.fine_side + x as usize],
                    ));
                }
            }
            // A bilinear surface never exceeds its corners; the margin absorbs roundoff.
            let line0 = from + (to - from) * t0;
            let line1 = from + (to - from) * t1;
            if f64::from(high) >= line0.min(line1) - 1e-9 {
                for i in start..end + 1 {
                    let t = i as f64 / total;
                    if self.height(ax + dx * t, ay + dy * t) > from + (to - from) * t {
                        return false;
                    }
                }
            }
            start += 8;
        }
        true
    }

    /// Physical reach: no striking up or down a cliff.
    pub fn melee_clear(&self, a: TerrainPoint, b: TerrainPoint) -> bool {
        if self.flat {
            return true;
        }
        let step = f64::from(MAX_GROUND_STEP_CM);
        let from = self.height(a.x, a.y);
        if (from - self.height(b.x, b.y)).abs() > step {
            return false;
        }
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let steps = (dx.abs().max(dy.abs()) * 4.0).ceil().max(1.0);
        let segment = math::hypot(dx, dy) / steps;
        let mut previous = from;
        for i in 1..=steps as i64 {
            let t = i as f64 / steps;
            let h = self.height(a.x + dx * t, a.y + dy * t);
            if (h - previous).abs() > step * segment + 1.0 {
                return false;
            }
            previous = h;
        }
        true
    }

    /// Cells within `radius` of the sensor's cell that are not clearly above it.
    pub fn visible_spans(&self, sensor: TerrainPoint, radius: f64) -> VisibleSpans {
        let size = self.size as i64;
        let x = super::round_half_up(sensor.x) as i64;
        let y = super::round_half_up(sensor.y) as i64;
        let r = radius.ceil() as i64;
        let from = self.height(x as f64, y as f64)
            + sensor.elevation * 100.0
            + f64::from(UPHILL_VISIBILITY_CM);
        // One block maximum admits a whole circle on low or gentle ground.
        let coarse_last = self.coarse_side as i64 - 1;
        let mut high = i32::from(i16::MIN);
        let by0 = ((y - r) as f64 / COARSE as f64).floor() as i64;
        let by1 = ((y + r + 1) as f64 / COARSE as f64).floor() as i64;
        let bx0 = ((x - r) as f64 / COARSE as f64).floor() as i64;
        let bx1 = ((x + r + 1) as f64 / COARSE as f64).floor() as i64;
        for by in by0.max(0)..=by1.min(coarse_last) {
            for bx in bx0.max(0)..=bx1.min(coarse_last) {
                high = high.max(i32::from(
                    self.coarse[by as usize * self.coarse_side + bx as usize],
                ));
            }
        }
        let all_clear = f64::from(high) <= from;
        let mut spans = Vec::new();
        for dy in -r..=r {
            let row_y = y + dy;
            if row_y < 0 || row_y >= size || (dy * dy) as f64 > radius * radius {
                continue;
            }
            let span = (radius * radius - (dy * dy) as f64).sqrt().floor() as i64;
            let (left, right) = ((x - span).max(0), (x + span).min(size - 1));
            if left > right {
                continue;
            }
            let row = row_y * size;
            if all_clear {
                spans.push(((row + left) as u32, (row + right + 1) as u32));
                continue;
            }
            let mut open: Option<i64> = None;
            for cell in row + left..=row + right {
                let clear = f64::from(self.heights[cell as usize]) <= from;
                match (clear, open) {
                    (true, None) => open = Some(cell),
                    (false, Some(start)) => {
                        spans.push((start as u32, cell as u32));
                        open = None;
                    }
                    _ => {}
                }
            }
            if let Some(start) = open {
                spans.push((start as u32, (row + right + 1) as u32));
            }
        }
        let cells = spans.iter().map(|(a, b)| b - a).sum();
        VisibleSpans { spans, cells }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 32-cell map: flat at 0 with a 5 m wall along x = 16.
    fn wall() -> TacticalTerrain {
        let heights = (0..32 * 32)
            .map(|i| if i % 32 == 16 { 500 } else { 0 })
            .collect();
        TacticalTerrain::new(32, heights)
    }

    #[test]
    fn heights_interpolate_and_clamp() {
        let t = wall();
        assert_eq!(t.height(16.0, 3.0), 500.0);
        assert_eq!(t.height(15.5, 3.0), 250.0);
        assert_eq!(t.height(-4.0, -4.0), 0.0);
        assert_eq!(t.height(40.0, 3.0), 0.0);
    }

    #[test]
    fn walls_block_shots_and_melee_but_not_sight_downhill() {
        let t = wall();
        let (a, b) = (TerrainPoint::new(10.0, 5.0), TerrainPoint::new(22.0, 5.0));
        assert!(!t.shot_clear(a, b));
        assert!(!t.melee_clear(a, b));
        assert!(t.visible(a, b));
        let on_wall = TerrainPoint::new(16.0, 5.0);
        assert!(!t.visible(a, on_wall));
        assert!(t.visible(on_wall, a));
        // Along the wall's side, the corridor is clear.
        let (c, d) = (TerrainPoint::new(10.0, 2.0), TerrainPoint::new(10.0, 30.0));
        assert!(t.shot_clear(c, d) && t.melee_clear(c, d));
        // High enough above the wall, a shot passes.
        let high = TerrainPoint {
            elevation: 6.0,
            ..a
        };
        assert!(t.shot_clear(
            high,
            TerrainPoint {
                elevation: 6.0,
                ..b
            }
        ));
    }

    #[test]
    fn flat_ground_is_always_clear() {
        let t = TacticalTerrain::new(16, vec![30; 256]);
        assert!(t.flat());
        assert!(t.shot_clear(TerrainPoint::new(0.0, 0.0), TerrainPoint::new(15.0, 15.0)));
    }

    #[test]
    fn sight_circles_skip_higher_ground() {
        let t = wall();
        let open = t.visible_spans(TerrainPoint::new(5.0, 5.0), 3.0);
        assert_eq!(open.cells, 29);
        assert_eq!(open.spans.first(), Some(&(2 * 32 + 5, 2 * 32 + 6)));
        let walled = t.visible_spans(TerrainPoint::new(14.0, 5.0), 3.0);
        assert!(walled.iter().all(|cell| cell % 32 != 16));
        assert_eq!(walled.cells, 29 - 5);
        // Clipped at the map edge.
        let corner = t.visible_spans(TerrainPoint::new(0.0, 0.0), 1.0);
        assert_eq!(corner.iter().collect::<Vec<_>>(), [0, 1, 32]);
    }
}
