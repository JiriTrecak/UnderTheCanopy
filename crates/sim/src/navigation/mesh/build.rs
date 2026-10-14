//! Ground mesh geometry from simulation collision (old `buildGroundMesh`,
//! `src/shared/navigation/groundMesh.ts`).
//!
//! Each 32-cell tile is sampled on a lattice whose cuts include every square-footprint
//! obstacle edge for the body radius, so a sample is clear exactly when a body centred
//! anywhere in it fits: walkable cells, no steep step inside the footprint. Clear samples
//! become contours, contours become triangles (earcut), as in the old game.

use super::earcut::triangulate_shape;
use crate::terrain::MAX_GROUND_STEP_CM;
use std::collections::BTreeMap;

/// Navigation cells per tile side.
pub const TILE: i32 = 32;
/// Milli-cells added to the body radius so paths keep off exact contacts.
pub const CLEARANCE_PADDING: i32 = 2;
const SCALE: i32 = 1000;

/// The collision input: ground cells (walkable incl. static occupancy) and heights.
#[derive(Clone, Copy)]
pub struct MeshInput<'a> {
    pub size: u32,
    pub walkable: &'a [bool],
    pub heights: &'a [i16],
}

/// One tile's triangles in milli-cells.
#[derive(Debug, Clone, PartialEq)]
pub struct TileGeometry {
    pub x: i32,
    pub z: i32,
    pub min_x: i32,
    pub min_z: i32,
    pub max_x: i32,
    pub max_z: i32,
    pub vertices: Vec<(i32, i32)>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Debug, Clone)]
struct SampleAxis {
    cuts: Vec<i32>,
    low: Vec<i32>,
    high: Vec<i32>,
}

fn axis(lo: i32, hi: i32, radius: i32) -> Vec<i32> {
    let mut cuts = vec![lo, hi];
    let first = (lo - radius).div_euclid(SCALE) - 1;
    let last = -(-(hi + radius)).div_euclid(SCALE) + 1;
    for cell in first..=last {
        for v in [cell * SCALE - 500 - radius, cell * SCALE - 500 + radius] {
            if v > lo && v < hi {
                cuts.push(v);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

fn sample_axis(lo: i32, hi: i32, radius: i32) -> SampleAxis {
    let cuts = axis(lo, hi, radius);
    let (mut low, mut high) = (Vec::new(), Vec::new());
    for pair in cuts.windows(2) {
        // floor((centre ± radius + 500) / 1000) with a half-integer centre.
        let twice = i64::from(pair[0]) + i64::from(pair[1]);
        let r = i64::from(radius);
        low.push((twice - 2 * r + 1000).div_euclid(2000) as i32);
        high.push((twice + 2 * r + 1000).div_euclid(2000) as i32);
    }
    SampleAxis { cuts, low, high }
}

/// Builds and caches tile masks and geometry for one body radius.
#[derive(Debug, Clone)]
pub struct Builder {
    size: u32,
    width: u32,
    /// Body radius in milli-cells including the clearance padding.
    radius: i32,
    halo: i32,
    axes: BTreeMap<u32, SampleAxis>,
    masks: Vec<Option<Vec<u8>>>,
}

impl Builder {
    pub fn new(size: u32, radius: f64) -> Self {
        let width = size.div_ceil(TILE as u32);
        Self {
            size,
            width,
            radius: crate::terrain::round_half_up(radius * f64::from(SCALE)) as i32
                + CLEARANCE_PADDING,
            halo: radius.ceil() as i32 + 2,
            axes: BTreeMap::new(),
            masks: vec![None; (width * width) as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    fn bounds(&self, tx: i32, tz: i32) -> (i32, i32, i32, i32) {
        let size = self.size as i32;
        (
            tx * TILE * SCALE - 500,
            tz * TILE * SCALE - 500,
            (size.min((tx + 1) * TILE)) * SCALE - 500,
            (size.min((tz + 1) * TILE)) * SCALE - 500,
        )
    }

    fn axis(&mut self, index: i32, lo: i32, hi: i32) -> SampleAxis {
        let radius = self.radius;
        self.axes
            .entry(index as u32)
            .or_insert_with(|| sample_axis(lo, hi, radius))
            .clone()
    }

    /// Recomputes tile `id`; returns its geometry when its clearance mask changed (or on
    /// the first build). `Some(None)` is a tile left without walkable area.
    pub fn rebuild(&mut self, input: MeshInput, id: u32) -> Option<Option<TileGeometry>> {
        let (tx, tz) = ((id % self.width) as i32, (id / self.width) as i32);
        let (min_x, min_z, max_x, max_z) = self.bounds(tx, tz);
        let sx = self.axis(tx, min_x, max_x);
        let sz = self.axis(tz, min_z, max_z);
        let mask = self.mask(input, tx, tz, &sx, &sz);
        let slot = &mut self.masks[id as usize];
        if slot.as_ref() == Some(&mask) {
            return None;
        }
        let geometry =
            geometry(&mask, &sx.cuts, &sz.cuts).map(|(vertices, triangles)| TileGeometry {
                x: tx,
                z: tz,
                min_x,
                min_z,
                max_x,
                max_z,
                vertices,
                triangles,
            });
        *slot = Some(mask);
        Some(geometry)
    }

    fn mask(
        &self,
        input: MeshInput,
        tx: i32,
        tz: i32,
        sx: &SampleAxis,
        sz: &SampleAxis,
    ) -> Vec<u8> {
        let size = input.size as i32;
        let (ox, oz) = (tx * TILE - self.halo, tz * TILE - self.halo);
        let span = TILE + 2 * self.halo;
        let stride = (span + 1) as usize;
        let mut blocked = vec![0u32; stride * stride];
        let mut bad_x = vec![0u32; stride * stride];
        let mut bad_z = vec![0u32; stride * stride];
        for z in 0..span {
            let (mut b, mut bx, mut bz) = (0, 0, 0);
            for x in 0..span {
                let (wx, wz) = (x + ox, z + oz);
                let (mut cell_blocked, mut cell_x, mut cell_z) = (1, 0, 0);
                if wx >= 0 && wz >= 0 && wx < size && wz < size {
                    let i = (wz * size + wx) as usize;
                    cell_blocked = u32::from(!input.walkable[i]);
                    let h = i32::from(input.heights[i]);
                    if wx < size - 1 {
                        cell_x = u32::from(
                            (h - i32::from(input.heights[i + 1])).abs() > MAX_GROUND_STEP_CM,
                        );
                    }
                    if wz < size - 1 {
                        cell_z = u32::from(
                            (h - i32::from(input.heights[i + size as usize])).abs()
                                > MAX_GROUND_STEP_CM,
                        );
                    }
                }
                b += cell_blocked;
                bx += cell_x;
                bz += cell_z;
                let at = (z as usize + 1) * stride + x as usize + 1;
                let above = z as usize * stride + x as usize + 1;
                blocked[at] = blocked[above] + b;
                bad_x[at] = bad_x[above] + bx;
                bad_z[at] = bad_z[above] + bz;
            }
        }
        let query = |table: &[u32], x0: i32, z0: i32, x1: i32, z1: i32| -> u32 {
            if x1 < x0 || z1 < z0 {
                return 0;
            }
            let (x0, z0) = ((x0 - ox) as usize, (z0 - oz) as usize);
            let (x1, z1) = ((x1 - ox) as usize, (z1 - oz) as usize);
            table[(z1 + 1) * stride + x1 + 1] + table[z0 * stride + x0]
                - table[z0 * stride + x1 + 1]
                - table[(z1 + 1) * stride + x0]
        };
        let (w, h) = (sx.low.len(), sz.low.len());
        let mut mask = vec![0u8; w * h];
        for z in 0..h {
            let (z0, z1) = (sz.low[z], sz.high[z]);
            for x in 0..w {
                let (x0, x1) = (sx.low[x], sx.high[x]);
                let clear = x0 >= 0
                    && z0 >= 0
                    && x1 < size
                    && z1 < size
                    && query(&blocked, x0, z0, x1, z1) == 0
                    && query(&bad_x, x0, z0, x1 - 1, z1) == 0
                    && query(&bad_z, x0, z0, x1, z1 - 1) == 0;
                mask[z * w + x] = u8::from(clear);
            }
        }
        mask
    }
}

type Loop = Vec<(i32, i32)>;

/// Boundary loops of the clear samples, filled samples on the right (old `contours`).
fn contours(mask: &[u8], xs: &[i32], zs: &[i32]) -> Vec<Loop> {
    let (w, h) = (xs.len() - 1, zs.len() - 1);
    let stride = w + 1;
    struct Edge {
        from: usize,
        to: usize,
        dir: usize,
        used: bool,
    }
    let mut edges: Vec<Edge> = Vec::new();
    let mut outgoing: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut edge = |edges: &mut Vec<Edge>, from: usize, to: usize, dir: usize| {
        outgoing.entry(from).or_default().push(edges.len());
        edges.push(Edge {
            from,
            to,
            dir,
            used: false,
        });
    };
    for z in 0..h {
        for x in 0..w {
            if mask[z * w + x] == 0 {
                continue;
            }
            let a = z * stride + x;
            let (b, c) = (a + 1, a + stride);
            let d = c + 1;
            if z == 0 || mask[(z - 1) * w + x] == 0 {
                edge(&mut edges, a, b, 0);
            }
            if x == w - 1 || mask[z * w + x + 1] == 0 {
                edge(&mut edges, b, d, 1);
            }
            if z == h - 1 || mask[(z + 1) * w + x] == 0 {
                edge(&mut edges, d, c, 2);
            }
            if x == 0 || mask[z * w + x - 1] == 0 {
                edge(&mut edges, c, a, 3);
            }
        }
    }
    let rank = |q: usize, e: usize| {
        [1, 0, 3, 2]
            .iter()
            .position(|&r| r == (q + 4 - e) % 4)
            .unwrap_or(4)
    };
    let mut loops = Vec::new();
    for first in 0..edges.len() {
        if edges[first].used {
            continue;
        }
        let mut points: Loop = Vec::new();
        let mut e = first;
        for _ in 0..=edges.len() {
            edges[e].used = true;
            let from = edges[e].from;
            points.push((xs[from % stride], zs[from / stride]));
            if edges[e].to == edges[first].from {
                break;
            }
            let dir = edges[e].dir;
            let mut best: Option<(usize, usize)> = None;
            for &candidate in outgoing.get(&edges[e].to).into_iter().flatten() {
                if edges[candidate].used {
                    continue;
                }
                let r = rank(edges[candidate].dir, dir);
                if best.is_none_or(|(_, b)| r < b) {
                    best = Some((candidate, r));
                }
            }
            let Some((next, _)) = best else {
                panic!("open navigation contour");
            };
            e = next;
        }
        let n = points.len();
        let simplified: Loop = (0..n)
            .filter(|&i| {
                let (a, p, b) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
                i64::from(p.0 - a.0) * i64::from(b.1 - p.1)
                    != i64::from(p.1 - a.1) * i64::from(b.0 - p.0)
            })
            .map(|i| points[i])
            .collect();
        if simplified.len() >= 3 {
            loops.push(simplified);
        }
    }
    loops
}

fn twice_area(ring: &[(i32, i32)]) -> i64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            i64::from(a.0) * i64::from(b.1) - i64::from(b.0) * i64::from(a.1)
        })
        .sum()
}

fn contains(ring: &[(i32, i32)], p: (i32, i32)) -> bool {
    let mut inside = false;
    let (px, py) = (f64::from(p.0), f64::from(p.1));
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        let (ax, ay, bx, by) = (
            f64::from(a.0),
            f64::from(a.1),
            f64::from(b.0),
            f64::from(b.1),
        );
        if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax {
            inside = !inside;
        }
        j = i;
    }
    inside
}

type Triangles = (Vec<(i32, i32)>, Vec<[u32; 3]>);

/// Triangles of the clear area of a mask, oriented clockwise in (x, z).
fn geometry(mask: &[u8], xs: &[i32], zs: &[i32]) -> Option<Triangles> {
    let loops = contours(mask, xs, zs);
    let (mut outers, mut holes) = (Vec::new(), Vec::new());
    for ring in loops {
        let area = twice_area(&ring);
        if area > 0 {
            outers.push(ring);
        } else if area < 0 {
            holes.push(ring);
        }
    }
    let areas: Vec<i64> = outers.iter().map(|r| twice_area(r)).collect();
    let mut assigned: Vec<Vec<Loop>> = vec![Vec::new(); outers.len()];
    for hole in holes {
        let mut owner: Option<usize> = None;
        for (i, outer) in outers.iter().enumerate() {
            if owner.is_none_or(|o| areas[i] < areas[o]) && contains(outer, hole[0]) {
                owner = Some(i);
            }
        }
        if let Some(owner) = owner {
            assigned[owner].push(hole);
        }
    }
    let mut vertices: Vec<(i32, i32)> = Vec::new();
    let mut ids: BTreeMap<(i32, i32), u32> = BTreeMap::new();
    let mut triangles = Vec::new();
    let as_f64 = |ring: &[(i32, i32)]| -> Vec<(f64, f64)> {
        ring.iter()
            .map(|&(x, y)| (f64::from(x), f64::from(y)))
            .collect()
    };
    for (outer, own) in outers.iter().zip(&assigned) {
        let flat: Vec<(i32, i32)> = outer.iter().chain(own.iter().flatten()).copied().collect();
        let holes: Vec<Vec<(f64, f64)>> = own.iter().map(|h| as_f64(h)).collect();
        let faces = triangulate_shape(&as_f64(outer), &holes);
        let index: Vec<u32> = flat
            .iter()
            .map(|&p| {
                *ids.entry(p).or_insert_with(|| {
                    vertices.push(p);
                    (vertices.len() - 1) as u32
                })
            })
            .collect();
        for face in faces.chunks(3) {
            let (a, b, c) = (index[face[0]], index[face[1]], index[face[2]]);
            let (va, vb, vc) = (
                vertices[a as usize],
                vertices[b as usize],
                vertices[c as usize],
            );
            let cross = i64::from(vb.0 - va.0) * i64::from(vc.1 - va.1)
                - i64::from(vb.1 - va.1) * i64::from(vc.0 - va.0);
            if cross < 0 {
                triangles.push([a, b, c]);
            } else if cross > 0 {
                triangles.push([a, c, b]);
            }
        }
    }
    (!triangles.is_empty()).then_some((vertices, triangles))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_cuts_follow_footprint_edges() {
        // Radius 0.4 (+ padding) around cell edges at ±500.
        let a = sample_axis(-500, 3500, 402);
        assert_eq!(a.cuts.first(), Some(&-500));
        assert!(a.cuts.contains(&(500 - 402)) && a.cuts.contains(&(500 + 402)));
        // A sample just right of an edge cut reaches the next cell.
        let i = a.cuts.iter().position(|&c| c == 500 - 402).unwrap();
        assert_eq!((a.low[i], a.high[i]), (0, 1));
    }

    #[test]
    fn a_blocked_cell_becomes_a_hole() {
        let size = 32u32;
        let mut walkable = vec![true; 1024];
        walkable[16 * 32 + 16] = false;
        let heights = vec![0i16; 1024];
        let input = MeshInput {
            size,
            walkable: &walkable,
            heights: &heights,
        };
        let mut builder = Builder::new(size, 0.4);
        let tile = builder
            .rebuild(input, 0)
            .expect("first build")
            .expect("area");
        let area: i64 = tile
            .triangles
            .iter()
            .map(|t| {
                let (a, b, c) = (
                    tile.vertices[t[0] as usize],
                    tile.vertices[t[1] as usize],
                    tile.vertices[t[2] as usize],
                );
                (i64::from(b.0 - a.0) * i64::from(c.1 - a.1)
                    - i64::from(b.1 - a.1) * i64::from(c.0 - a.0))
                .abs()
            })
            .sum();
        // Clear area: the inset map minus the blocked cell grown by the body.
        let side = i64::from(32 * 1000 - 2 * 402);
        let hole = i64::from(1000 + 2 * 402);
        assert_eq!(area, 2 * (side * side - hole * hole));
        // Unchanged input: nothing to rebuild.
        assert!(builder.rebuild(input, 0).is_none());
    }
}
