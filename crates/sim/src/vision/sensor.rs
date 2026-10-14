//! Sight sources and their footprints: the fog nodes a sensor sees (old `sightFootprint`,
//! `TacticalTerrain.visibleSpans`, `Spatial.visibleNodes`).

use crate::{
    entity::{EntityId, Milli},
    math,
    terrain::{DeckId, SurfacePoint, Terrain, UPHILL_VISIBILITY_CM},
};
use canopy_content::placement::Owner;

/// Sorted, disjoint half-open ranges `[start, end)` of fog nodes.
pub type Spans = Vec<(u32, u32)>;

/// Entities sort before spells, each by id: the order sensors are reconciled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SensorKey {
    Entity(EntityId),
    Spell(u32),
}

/// Everything a footprint depends on; a sensor whose spec is unchanged keeps its
/// footprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub owner: Owner,
    /// Centre of the sight circle in milli-cells: a unit's cell, a structure's placed
    /// position, a spell's point.
    pub origin: Milli,
    pub deck: Option<DeckId>,
    /// Eye height above the floor in centimetres (flight, garrison lookout).
    pub elevation: i32,
    /// Milli-cells.
    pub radius: u32,
    pub ignore_terrain: bool,
    /// Teams (by index) that share this sensor's sight.
    pub teams: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sensor {
    pub key: SensorKey,
    pub spec: Spec,
    pub footprint: Spans,
}

/// Bounds `[min x, max x, min y, max y]` in cells around which a deck may hide or carry
/// sight (the walk surfaces' own crossing test).
pub fn deck_bounds(terrain: &Terrain) -> Vec<[f64; 4]> {
    terrain.surfaces().map_or_else(Vec::new, |surfaces| {
        surfaces
            .decks()
            .iter()
            .map(|d| {
                let r = d.reach();
                [d.x - r, d.x + r, d.z - r, d.z + r]
            })
            .collect()
    })
}

/// The fog nodes `spec` sees.
pub fn footprint(terrain: &Terrain, ground: &Ground, decks: &[[f64; 4]], spec: &Spec) -> Spans {
    let radius = f64::from(spec.radius) / 1000.0;
    let (x, y) = (
        f64::from(spec.origin.x) / 1000.0,
        f64::from(spec.origin.y) / 1000.0,
    );
    let Some(surfaces) = terrain.surfaces() else {
        return if spec.ignore_terrain {
            circle(terrain, spec, |_, _, out| out(None))
        } else {
            ground.spans(terrain.heights(), spec)
        };
    };
    // Away from every deck, layered sight is ground sight.
    let integral = spec.origin.x % 1000 == 0 && spec.origin.y % 1000 == 0;
    let clear = decks
        .iter()
        .all(|b| x + radius < b[0] || x - radius > b[1] || y + radius < b[2] || y - radius > b[3]);
    if !spec.ignore_terrain && spec.deck.is_none() && integral && clear {
        return ground.spans(terrain.heights(), spec);
    }
    let eye = SurfacePoint {
        x,
        y,
        deck: spec.deck,
        elevation: f64::from(spec.elevation) / 100.0,
    };
    circle(terrain, spec, |x, y, out| {
        for id in surfaces.at(x, y) {
            let node = surfaces.node(id).expect("node");
            let target = SurfacePoint {
                x: x as f64,
                y: y as f64,
                deck: node.deck,
                elevation: 0.0,
            };
            if spec.ignore_terrain || terrain.visible(eye, target) {
                out(Some(id));
            }
        }
    })
}

/// Cells per side of a height block, and of a coarse block of blocks.
const BLOCK: usize = 4;
const COARSE: usize = 16;

/// Ground sight on the cell grid, the rule of `TacticalTerrain::visible_spans` (cells in
/// the circle around the sensor's cell no more than 2 m above its eye), with per-block
/// height ranges so whole runs of cells are decided at once. Derived from the terrain.
#[derive(Debug, Clone)]
pub struct Ground {
    size: usize,
    side: usize,
    /// Lowest and highest cell of each block, in centimetres.
    low: Vec<i16>,
    high: Vec<i16>,
    coarse_side: usize,
    /// Highest cell of each coarse block.
    coarse: Vec<i16>,
}

impl Ground {
    pub fn new(size: u32, heights: &[i16]) -> Self {
        let size = size as usize;
        let side = size.div_ceil(BLOCK);
        let coarse_side = size.div_ceil(COARSE);
        let mut low = vec![i16::MAX; side * side];
        let mut high = vec![i16::MIN; side * side];
        let mut coarse = vec![i16::MIN; coarse_side * coarse_side];
        for y in 0..size {
            for x in 0..size {
                let (h, b) = (heights[y * size + x], y / BLOCK * side + x / BLOCK);
                low[b] = low[b].min(h);
                high[b] = high[b].max(h);
                let c = &mut coarse[y / COARSE * coarse_side + x / COARSE];
                *c = (*c).max(h);
            }
        }
        Self {
            size,
            side,
            low,
            high,
            coarse_side,
            coarse,
        }
    }

    pub fn spans(&self, heights: &[i16], spec: &Spec) -> Spans {
        let size = self.size as i64;
        let (x, y) = (
            math::div_floor(i64::from(spec.origin.x) + 500, 1000),
            math::div_floor(i64::from(spec.origin.y) + 500, 1000),
        );
        let radius = f64::from(spec.radius) / 1000.0;
        let r = radius.ceil() as i64;
        let centre = (x.clamp(0, size - 1) + y.clamp(0, size - 1) * size) as usize;
        // Heights are whole centimetres, so `h <= from` is `h <= floor(from)`.
        let from = f64::from(heights[centre])
            + f64::from(spec.elevation)
            + f64::from(UPHILL_VISIBILITY_CM);
        let limit = from.floor().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
        // On low or gentle ground one look at the coarse blocks admits the whole circle.
        let coarse = |v: i64| (v.clamp(0, size - 1) as usize) / COARSE;
        let (cx0, cx1, cy0, cy1) = (coarse(x - r), coarse(x + r), coarse(y - r), coarse(y + r));
        let all_clear = (cy0..=cy1).all(|cy| {
            (cx0..=cx1).all(|cx| i32::from(self.coarse[cy * self.coarse_side + cx]) <= limit)
        });
        let mut spans: Spans = Vec::with_capacity(2 * r as usize + 1);
        for dy in -r..=r {
            let row_y = y + dy;
            if row_y < 0 || row_y >= size || (dy * dy) as f64 > radius * radius {
                continue;
            }
            let half = (radius * radius - (dy * dy) as f64).sqrt().floor() as i64;
            let (left, right) = ((x - half).max(0), (x + half).min(size - 1));
            if left > right {
                continue;
            }
            let row = row_y as usize * self.size;
            if all_clear {
                spans.push((
                    (row + left as usize) as u32,
                    (row + right as usize + 1) as u32,
                ));
                continue;
            }
            let blocks = row_y as usize / BLOCK * self.side;
            let coarse_row = row_y as usize / COARSE * self.coarse_side;
            let mut open: Option<usize> = None;
            let mut cx = left as usize;
            let right = right as usize;
            while cx <= right {
                // A clear coarse block decides up to 16 cells; a block, up to 4.
                let coarse_end = ((cx / COARSE + 1) * COARSE - 1).min(right);
                if i32::from(self.coarse[coarse_row + cx / COARSE]) <= limit {
                    toggle(&mut spans, &mut open, row + cx, true);
                    cx = coarse_end + 1;
                    continue;
                }
                let b = blocks + cx / BLOCK;
                let run_end = ((cx / BLOCK + 1) * BLOCK - 1).min(right);
                if i32::from(self.high[b]) <= limit {
                    toggle(&mut spans, &mut open, row + cx, true);
                } else if i32::from(self.low[b]) > limit {
                    toggle(&mut spans, &mut open, row + cx, false);
                } else {
                    for c in cx..=run_end {
                        let clear = i32::from(heights[row + c]) <= limit;
                        toggle(&mut spans, &mut open, row + c, clear);
                    }
                }
                cx = run_end + 1;
            }
            if let Some(start) = open {
                spans.push((start as u32, (row + right + 1) as u32));
            }
        }
        spans
    }
}

/// Opens a span at a clear cell or closes the open one at a blocked cell.
fn toggle(spans: &mut Spans, open: &mut Option<usize>, cell: usize, clear: bool) {
    match (clear, *open) {
        (true, None) => *open = Some(cell),
        (false, Some(start)) => {
            spans.push((start as u32, cell as u32));
            *open = None;
        }
        _ => {}
    }
}

/// Nodes of the cells within the radius of the (possibly fractional) origin, as chosen by
/// `nodes`, which reports `None` for the ground cell itself.
fn circle(
    terrain: &Terrain,
    spec: &Spec,
    mut nodes: impl FnMut(i64, i64, &mut dyn FnMut(Option<u32>)),
) -> Spans {
    let size = i64::from(terrain.size());
    let (ox, oy, r) = (
        i64::from(spec.origin.x),
        i64::from(spec.origin.y),
        i64::from(spec.radius),
    );
    let ceil = |v: i64| -math::div_floor(-v, 1000);
    let floor = |v: i64| math::div_floor(v, 1000);
    let mut found = Vec::new();
    for y in ceil(oy - r).max(0)..=floor(oy + r).min(size - 1) {
        for x in ceil(ox - r).max(0)..=floor(ox + r).min(size - 1) {
            let (dx, dy) = (x * 1000 - ox, y * 1000 - oy);
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let cell = (y * size + x) as u32;
            nodes(x, y, &mut |node| found.push(node.unwrap_or(cell)));
        }
    }
    found.sort_unstable();
    spans(found)
}

/// Runs of consecutive values in sorted, unique `nodes`.
pub fn spans(nodes: impl IntoIterator<Item = u32>) -> Spans {
    let mut out: Spans = Vec::new();
    for node in nodes {
        match out.last_mut() {
            Some((_, end)) if *end == node => *end += 1,
            _ => out.push((node, node + 1)),
        }
    }
    out
}

/// Calls `change(node, delta)` for every node leaving (`-1`) or entering (`+1`) the
/// footprint when it changes from `old` to `new`.
pub fn diff(old: &[(u32, u32)], new: &[(u32, u32)], mut change: impl FnMut(u32, i32)) {
    let (mut a, mut b) = (0, 0);
    let start = |s: &[(u32, u32)], i: usize| s.get(i).map_or(u32::MAX, |r| r.0);
    let end = |s: &[(u32, u32)], i: usize| s.get(i).map_or(u32::MAX, |r| r.1);
    let (mut sa, mut sb) = (start(old, 0), start(new, 0));
    while a < old.len() || b < new.len() {
        let (ea, eb) = (end(old, a), end(new, b));
        if sa < sb {
            let stop = ea.min(sb);
            (sa..stop).for_each(|n| change(n, -1));
            sa = stop;
        } else if sb < sa {
            let stop = eb.min(sa);
            (sb..stop).for_each(|n| change(n, 1));
            sb = stop;
        } else {
            let stop = ea.min(eb);
            sa = stop;
            sb = stop;
        }
        if sa == ea {
            a += 1;
            sa = start(old, a);
        }
        if sb == eb {
            b += 1;
            sb = start(new, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &[(u32, u32)], new: &[(u32, u32)]) -> Vec<(u32, i32)> {
        let mut out = Vec::new();
        diff(old, new, |n, d| out.push((n, d)));
        out
    }

    #[test]
    fn diffs_report_only_changed_nodes() {
        assert_eq!(apply(&[(2, 5)], &[(2, 5)]), []);
        assert_eq!(apply(&[(2, 5)], &[(3, 6)]), [(2, -1), (5, 1)]);
        assert_eq!(apply(&[], &[(0, 2)]), [(0, 1), (1, 1)]);
        assert_eq!(apply(&[(0, 2), (7, 8)], &[]), [(0, -1), (1, -1), (7, -1)]);
        assert_eq!(
            apply(&[(0, 4), (10, 12)], &[(2, 3), (5, 6), (11, 13)]),
            [(0, -1), (1, -1), (3, -1), (5, 1), (10, -1), (12, 1)]
        );
    }

    #[test]
    fn block_footprints_equal_the_tactical_rule() {
        use crate::terrain::{TacticalTerrain, TerrainPoint};
        let size = 70u32;
        // Ridges, pits and plateaus of assorted heights.
        let heights: Vec<i16> = (0..size * size)
            .map(|i| {
                let (x, y) = ((i % size) as i32, (i / size) as i32);
                ((x * 37 + y * 11) % 23 * 25 + if (20..30).contains(&x) { 400 } else { 0 }
                    - if y > 50 { 300 } else { 0 }) as i16
            })
            .collect();
        let tactical = TacticalTerrain::new(size, heights.clone());
        let ground = Ground::new(size, &heights);
        for (i, (ox, oy)) in [(0, 0), (5, 9), (24, 33), (69, 69), (35, 52), (12, 66)]
            .into_iter()
            .enumerate()
        {
            for radius in [1000, 2500, 6000, 13_000, 31_000] {
                for elevation in [0, 150, 700] {
                    let spec = Spec {
                        owner: canopy_content::placement::Owner::Nobody,
                        origin: Milli {
                            x: ox * 1000 + if i % 2 == 0 { 0 } else { 500 },
                            y: oy * 1000,
                        },
                        deck: None,
                        elevation,
                        radius,
                        ignore_terrain: false,
                        teams: 0,
                    };
                    let point = TerrainPoint {
                        x: f64::from(spec.origin.x) / 1000.0,
                        y: f64::from(spec.origin.y) / 1000.0,
                        elevation: f64::from(elevation) / 100.0,
                    };
                    let expected = tactical
                        .visible_spans(point, f64::from(radius) / 1000.0)
                        .spans;
                    assert_eq!(
                        ground.spans(&heights, &spec),
                        expected,
                        "{ox},{oy} r{radius} e{elevation}"
                    );
                }
            }
        }
    }

    #[test]
    fn runs_compress_sorted_nodes() {
        assert_eq!(spans([1, 2, 3, 7, 9, 10]), [(1, 4), (7, 8), (9, 11)]);
        assert!(spans([]).is_empty());
    }
}
