//! The block view's mesh (D80): every column as a block of flat colour, its top (a sloped
//! top for a ramp, two triangles for a cut), and walls wherever it stands above its
//! neighbour, split at every level so each cube reads as a cube. The heights are the ones
//! the blocks give the map (`Blocks::apply`), water drawn at its surface.

use canopy_content::map::MAP_HALO;
use canopy_content::map::blocks::{
    BASE_HEIGHT, BLOCK, Block, BlockKind, Blocks, Cut, LEVEL_HEIGHT, RampPlane,
};
use canopy_render::blockout::{BlockVertex, BlockoutMesh};
use glam::Vec3;

/// No diagonal outline on this face.
const NO_DIAGONAL: f32 = 9.0;

fn linear(hex: u32) -> [f32; 3] {
    let c = |v: u32| ((v & 0xff) as f32 / 255.0).powf(2.2);
    [c(hex >> 16), c(hex >> 8), c(hex)]
}

/// A block's colour: ground by level (low greens to high sand), the other kinds their own.
pub fn color(block: Block) -> [f32; 3] {
    const LEVELS: [u32; 8] = [
        0x56683f, 0x6e874c, 0x91a75b, 0xb5bd6c, 0xcdc27f, 0xdbc697, 0xe6d3b1, 0xefe2cb,
    ];
    linear(match block.kind {
        BlockKind::Ground => LEVELS[(block.level as usize).min(7)],
        BlockKind::Ramp => 0xc8955a,
        BlockKind::Wall => 0x3f3a2b,
        BlockKind::Rock => 0x8c8b8e,
        BlockKind::Shallow => 0x71c1d6,
        BlockKind::Deep => 0x2e6fa4,
        BlockKind::Ice => 0xbfe3ee,
    })
}

/// The height a block's top is drawn at (water at its surface).
pub fn top(block: Block) -> f32 {
    BASE_HEIGHT
        + block.level as f32 * LEVEL_HEIGHT
        + match block.kind {
            BlockKind::Shallow => -0.2,
            BlockKind::Deep => -0.4,
            BlockKind::Ice => -0.15,
            _ => 0.0,
        }
}

struct Builder {
    mesh: BlockoutMesh,
}

impl Builder {
    fn vertex(&mut self, p: Vec3, n: Vec3, color: [f32; 3], edge: [f32; 3]) -> u32 {
        self.mesh.vertices.push(BlockVertex {
            position: p.to_array(),
            normal: n.to_array(),
            color,
            edge,
        });
        self.mesh.vertices.len() as u32 - 1
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.mesh.indices.extend_from_slice(&[a, b, c]);
    }

    /// A quad a–b–c–d (counter-clockwise from outside) with face coordinates.
    fn quad(&mut self, p: [Vec3; 4], n: Vec3, color: [f32; 3]) {
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        // In order, without a heap list: this runs for every face of a large map.
        let i =
            [0, 1, 2, 3].map(|k| self.vertex(p[k], n, color, [uv[k][0], uv[k][1], NO_DIAGONAL]));
        self.triangle(i[0], i[1], i[2]);
        self.triangle(i[0], i[2], i[3]);
    }

    /// A wall from `low` up to `high` along a→b (both heights per end), facing `n`, split
    /// at every level when it is level-flat so each cube shows its own face.
    fn wall(&mut self, a: Vec3, b: Vec3, low: [f32; 2], high: [f32; 2], n: Vec3, color: [f32; 3]) {
        if high[0] <= low[0] + 0.01 && high[1] <= low[1] + 0.01 {
            return;
        }
        let flat = (high[0] - high[1]).abs() < 0.01 && (low[0] - low[1]).abs() < 0.01;
        if !flat {
            self.quad(
                [
                    Vec3::new(a.x, low[0], a.z),
                    Vec3::new(b.x, low[1], b.z),
                    Vec3::new(b.x, high[1], b.z),
                    Vec3::new(a.x, high[0], a.z),
                ],
                n,
                color,
            );
            return;
        }
        let (mut y, top) = (low[0], high[0]);
        while y < top - 0.01 {
            let next =
                (((y - BASE_HEIGHT) / LEVEL_HEIGHT).floor() + 1.0) * LEVEL_HEIGHT + BASE_HEIGHT;
            let next = next.min(top);
            self.quad(
                [
                    Vec3::new(a.x, y, a.z),
                    Vec3::new(b.x, y, b.z),
                    Vec3::new(b.x, next, b.z),
                    Vec3::new(a.x, next, a.z),
                ],
                n,
                color,
            );
            y = next;
        }
    }
}

/// The four sides of a column: direction, and its two corners (u, v) in order around the
/// column seen from above (counter-clockwise looking down +y with x right, z down).
/// A side of a column: its outward direction and its two corners (u, v).
type Side = ((i32, i32), [(f32, f32); 2]);

const SIDES: [Side; 4] = [
    ((0, -1), [(1.0, 0.0), (0.0, 0.0)]),
    ((1, 0), [(1.0, 1.0), (1.0, 0.0)]),
    ((0, 1), [(0.0, 1.0), (1.0, 1.0)]),
    ((-1, 0), [(0.0, 0.0), (0.0, 1.0)]),
];

/// The height of column (`x`, `z`)'s top at its corner (`u`, `v`), on the side `side`
/// (the half that owns that side when the column is cut).
fn edge_height(
    blocks: &Blocks,
    planes: &[Option<RampPlane>],
    x: i32,
    z: i32,
    side: (i32, i32),
    u: f32,
    v: f32,
) -> (f32, Block) {
    let size = blocks.side as i32;
    if x < 0 || z < 0 || x >= size || z >= size {
        return (BASE_HEIGHT - LEVEL_HEIGHT, Block::ground(0));
    }
    let block = blocks.at(x, z);
    if block.kind == BlockKind::Ramp {
        return (blocks.ramp_height(planes, x, z, u, v), block);
    }
    // A point just inside the side decides which half of a cut column owns it.
    let (su, sv) = (0.5 + side.0 as f32 * 0.49, 0.5 + side.1 as f32 * 0.49);
    let half = blocks.sample(x, z, su, sv);
    (top(half), half)
}

/// The block view's mesh of `blocks`, built whole: what [`mesh_cached`] must equal.
#[cfg(test)]
fn mesh(blocks: &Blocks) -> BlockoutMesh {
    let _span = tracing::info_span!("blockout_mesh").entered();
    let mut b = Builder {
        mesh: BlockoutMesh::default(),
    };
    let planes = blocks.ramp_planes();
    for z in 0..blocks.side as i32 {
        for x in 0..blocks.side as i32 {
            column(&mut b, blocks, &planes, x, z);
        }
    }
    // Painted cells: a tile of their kind's colour over the top they lie on.
    if blocks.paint.is_some() {
        let n = blocks.cells() as i32;
        for cz in 0..n {
            for cx in 0..n {
                painted_cell(&mut b, blocks, cx, cz);
            }
        }
    }
    b.mesh
}

/// Rows of columns a cached strip of the mesh holds.
const STRIP: i32 = 8;
/// Rows around a strip its faces depend on (a wall looks at its neighbour, whose cut
/// looks at its own neighbours).
const REACH: i32 = 2;

/// The block view's mesh in strips of rows, each kept until the blocks it depends on
/// change: a stroke on a large map builds only the strips it touches
/// ([`mesh_cached`]).
#[derive(Default, Clone)]
pub struct MeshCache {
    strips: Vec<Strip>,
}

impl std::fmt::Debug for MeshCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshCache")
            .field("strips", &self.strips.len())
            .finish()
    }
}

/// One strip: what it was built from, its columns' faces and its painted tiles.
#[derive(Clone)]
struct Strip {
    inputs: (Vec<Block>, Vec<Option<RampPlane>>, Option<Vec<u8>>),
    columns: BlockoutMesh,
    cells: BlockoutMesh,
}

/// The same mesh as [`mesh`] (the same vertices in the same order), building again only
/// the strips whose blocks, ramp planes or painted cells changed since `cache` last saw
/// them.
pub fn mesh_cached(blocks: &Blocks, cache: &mut MeshCache) -> BlockoutMesh {
    let _span = tracing::info_span!("blockout_mesh").entered();
    let planes = blocks.ramp_planes();
    let side = blocks.side as i32;
    let strips = (side + STRIP - 1) / STRIP;
    if cache.strips.len() != strips as usize {
        cache.strips.clear();
    }
    let row = blocks.side as usize;
    let cells = blocks.cells() as usize;
    let b = BLOCK as usize;
    for k in 0..strips {
        let (z0, z1) = (k * STRIP, ((k + 1) * STRIP).min(side));
        let (r0, r1) = (
            (z0 - REACH).max(0) as usize,
            (z1 + REACH).min(side) as usize,
        );
        let inputs = (
            blocks.columns[r0 * row..r1 * row].to_vec(),
            planes[r0 * row..r1 * row].to_vec(),
            blocks
                .paint
                .as_ref()
                .map(|p| p[r0 * b * cells..r1 * b * cells].to_vec()),
        );
        if cache
            .strips
            .get(k as usize)
            .is_some_and(|strip| strip.inputs == inputs)
        {
            continue;
        }
        let mut columns = Builder {
            mesh: BlockoutMesh::default(),
        };
        for z in z0..z1 {
            for x in 0..side {
                column(&mut columns, blocks, &planes, x, z);
            }
        }
        let mut tiles = Builder {
            mesh: BlockoutMesh::default(),
        };
        if blocks.paint.is_some() {
            let n = blocks.cells() as i32;
            for cz in z0 * BLOCK as i32..z1 * BLOCK as i32 {
                for cx in 0..n {
                    painted_cell(&mut tiles, blocks, cx, cz);
                }
            }
        }
        let strip = Strip {
            inputs,
            columns: columns.mesh,
            cells: tiles.mesh,
        };
        match cache.strips.get_mut(k as usize) {
            Some(slot) => *slot = strip,
            None => cache.strips.push(strip),
        }
    }
    // The columns' faces strip by strip, then the painted tiles: the order `mesh` makes.
    let mut out = BlockoutMesh::default();
    let parts = cache
        .strips
        .iter()
        .map(|s| &s.columns)
        .chain(cache.strips.iter().map(|s| &s.cells));
    for part in parts {
        let base = out.vertices.len() as u32;
        out.vertices.extend_from_slice(&part.vertices);
        out.indices.extend(part.indices.iter().map(|i| i + base));
    }
    out
}

/// Column (`x`, `z`)'s top and the walls where it stands above its neighbours.
fn column(b: &mut Builder, blocks: &Blocks, planes: &[Option<RampPlane>], x: i32, z: i32) {
    let s = BLOCK as f32;
    let halo = MAP_HALO as f32;
    let up = Vec3::Y;
    let block = blocks.at(x, z);
    let (x0, z0) = (x as f32 * s - halo, z as f32 * s - halo);
    let at = |u: f32, v: f32, y: f32| Vec3::new(x0 + u * s, y, z0 + v * s);
    let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    // The top.
    if block.kind == BlockKind::Ramp {
        let h = corners.map(|(u, v)| edge_height(blocks, planes, x, z, (0, 0), u, v).0);
        let p: [Vec3; 4] = std::array::from_fn(|k| at(corners[k].0, corners[k].1, h[k]));
        let n = (p[2] - p[0]).cross(p[1] - p[3]).normalize_or(up);
        let n = if n.y < 0.0 { -n } else { n };
        b.quad([p[0], p[3], p[2], p[1]], n, color(block));
    } else if let Some((cx, cz)) = block.cut.corner() {
        let fill = blocks.sample(x, z, 0.5 + cx as f32 * 0.45, 0.5 + cz as f32 * 0.45);
        let own = Block {
            cut: Cut::None,
            ..block
        };
        // Triangles: the cut corner and its two neighbours belong to the fill.
        let corner = corners
            .iter()
            .position(|&(u, v)| (u > 0.5) == (cx > 0) && (v > 0.5) == (cz > 0))
            .unwrap_or(0);
        let (a, c) = ((corner + 1) % 4, (corner + 3) % 4);
        let o = (corner + 2) % 4;
        let diag = |u: f32, v: f32| {
            let p = if cx < 0 { 1.0 - u } else { u };
            let q = if cz < 0 { 1.0 - v } else { v };
            p + q - 1.0
        };
        for (half, idx) in [(own, [a, o, c]), (fill, [corner, a, c])] {
            let y = top(half);
            let ids = idx.map(|k| {
                let (u, v) = corners[k];
                b.vertex(at(u, v, y), up, color(half), [u, v, diag(u, v)])
            });
            b.triangle(ids[0], ids[2], ids[1]);
        }
        // The diagonal wall between the halves.
        let (ya, yb) = (top(own), top(fill));
        if (ya - yb).abs() > 0.01 {
            let (pa, pb) = (corners[a], corners[c]);
            let (hi, lo, hi_block) = if ya > yb {
                (ya, yb, own)
            } else {
                (yb, ya, fill)
            };
            let towards_fill = Vec3::new(cx as f32, 0.0, cz as f32).normalize();
            let n = if ya > yb { towards_fill } else { -towards_fill };
            b.wall(
                at(pa.0, pa.1, 0.0),
                at(pb.0, pb.1, 0.0),
                [lo, lo],
                [hi, hi],
                n,
                color(hi_block),
            );
        }
    } else {
        let y = top(block);
        let p = corners.map(|(u, v)| at(u, v, y));
        b.quad([p[0], p[3], p[2], p[1]], up, color(block));
    }
    // Walls where this column stands above its neighbour.
    for ((dx, dz), [ca, cb]) in SIDES {
        let (ha, mine) = edge_height(blocks, planes, x, z, (dx, dz), ca.0, ca.1);
        let (hb, _) = edge_height(blocks, planes, x, z, (dx, dz), cb.0, cb.1);
        // The neighbour's matching corners on its far side.
        let (na, nb) = (
            (ca.0 - dx as f32, ca.1 - dz as f32),
            (cb.0 - dx as f32, cb.1 - dz as f32),
        );
        let (la, _) = edge_height(blocks, planes, x + dx, z + dz, (-dx, -dz), na.0, na.1);
        let (lb, _) = edge_height(blocks, planes, x + dx, z + dz, (-dx, -dz), nb.0, nb.1);
        b.wall(
            at(ca.0, ca.1, 0.0),
            at(cb.0, cb.1, 0.0),
            [la.min(ha), lb.min(hb)],
            [ha, hb],
            Vec3::new(dx as f32, 0.0, dz as f32),
            color(mine),
        );
    }
}

/// Painted cell (`cx`, `cz`)'s tile, when it is painted.
fn painted_cell(b: &mut Builder, blocks: &Blocks, cx: i32, cz: i32) {
    let bi = BLOCK as i32;
    let halo = MAP_HALO as f32;
    let up = Vec3::Y;
    let Some(kind) = blocks.painted(cx, cz) else {
        return;
    };
    let (x, z) = (cx / bi, cz / bi);
    let (u, v) = (
        ((cx % bi) as f32 + 0.5) / BLOCK as f32,
        ((cz % bi) as f32 + 0.5) / BLOCK as f32,
    );
    let cell = blocks.sample(x, z, u, v);
    if cell.kind == BlockKind::Ramp {
        return;
    }
    let block = Block { kind, ..cell };
    let y = top(block) + 0.03;
    let (x0, z0) = (cx as f32 - halo, cz as f32 - halo);
    let p = [
        Vec3::new(x0, y, z0),
        Vec3::new(x0, y, z0 + 1.0),
        Vec3::new(x0 + 1.0, y, z0 + 1.0),
        Vec3::new(x0 + 1.0, y, z0),
    ];
    b.quad(p, up, color(block));
}

/// The pathing view's mesh: every cell of the playable map flat at its height, green where
/// ground units can walk (open, with a neighbour within a climbable step, as the
/// simulation moves them) and grey where they cannot, each outlined; walls where a cell
/// stands above its neighbour, grey where the step is too high to climb.
pub fn pathing(terrain: &canopy_sim::terrain::Terrain) -> BlockoutMesh {
    use canopy_sim::terrain::MAX_GROUND_STEP_CM;
    let size = terrain.size() as i64;
    let heights = terrain.heights();
    let open = terrain.walkable_cells();
    let at = |x: i64, z: i64| terrain.cell(x, z);
    let step = |a: usize, b: usize| {
        open[b] && (i32::from(heights[a]) - i32::from(heights[b])).abs() <= MAX_GROUND_STEP_CM
    };
    let pathable: Vec<bool> = (0..size * size)
        .map(|i| {
            let (x, z) = (i % size, i / size);
            let i = i as usize;
            open[i]
                && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .filter_map(|(dx, dz)| at(x + dx, z + dz))
                    .any(|j| step(i, j))
        })
        .collect();
    let (green, grey) = (linear(0x6cc35f), linear(0x9a9aa2));
    let shade = |c: [f32; 3]| c.map(|v| v * 0.62);
    let mut b = Builder {
        mesh: BlockoutMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
        },
    };
    let y = |i: usize| f32::from(heights[i]) / 100.0;
    for z in 0..size {
        for x in 0..size {
            let i = (z * size + x) as usize;
            let color = if pathable[i] { green } else { grey };
            let (x0, z0, h) = (x as f32, z as f32, y(i));
            b.quad(
                [
                    Vec3::new(x0, h, z0 + 1.0),
                    Vec3::new(x0 + 1.0, h, z0 + 1.0),
                    Vec3::new(x0 + 1.0, h, z0),
                    Vec3::new(x0, h, z0),
                ],
                Vec3::Y,
                color,
            );
            // Walls down to each lower neighbour (and at the map's edges): grey where the
            // step is too high to climb.
            for (dx, dz) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let next = at(x + dx, z + dz);
                let low = next.map_or(h - 0.5, y);
                let barrier = next.is_none_or(|j| {
                    (i32::from(heights[i]) - i32::from(heights[j])).abs() > MAX_GROUND_STEP_CM
                });
                let wall = if barrier { grey } else { color };
                if low >= h - 0.01 {
                    continue;
                }
                let (a, c, n) = match (dx, dz) {
                    (1, _) => (
                        Vec3::new(x0 + 1.0, 0.0, z0 + 1.0),
                        Vec3::new(x0 + 1.0, 0.0, z0),
                        Vec3::X,
                    ),
                    (-1, _) => (
                        Vec3::new(x0, 0.0, z0),
                        Vec3::new(x0, 0.0, z0 + 1.0),
                        -Vec3::X,
                    ),
                    (_, 1) => (
                        Vec3::new(x0, 0.0, z0 + 1.0),
                        Vec3::new(x0 + 1.0, 0.0, z0 + 1.0),
                        Vec3::Z,
                    ),
                    _ => (
                        Vec3::new(x0 + 1.0, 0.0, z0),
                        Vec3::new(x0, 0.0, z0),
                        -Vec3::Z,
                    ),
                };
                b.quad(
                    [
                        Vec3::new(a.x, low, a.z),
                        Vec3::new(c.x, low, c.z),
                        Vec3::new(c.x, h, c.z),
                        Vec3::new(a.x, h, a.z),
                    ],
                    n,
                    shade(wall),
                );
            }
        }
    }
    b.mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strokes of every kind, cut, ramp and painted cell: the strips kept between them make
    /// the same mesh as building it whole.
    #[test]
    fn the_cached_mesh_is_the_whole_mesh() {
        let mut seed = 0x2545_f491_u32;
        let mut next = |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        let kinds = [
            BlockKind::Ground,
            BlockKind::Ramp,
            BlockKind::Wall,
            BlockKind::Rock,
            BlockKind::Shallow,
            BlockKind::Deep,
            BlockKind::Ice,
        ];
        let mut blocks = Blocks::flat(128);
        let mut cache = MeshCache::default();
        assert!(mesh_cached(&blocks, &mut cache) == mesh(&blocks));
        for stroke in 0..60 {
            let (x, z) = (next(blocks.side) as i32, next(blocks.side) as i32);
            if stroke % 4 == 3 {
                let n = blocks.cells();
                let (cx, cz) = (next(n) as i32, next(n) as i32);
                for d in 0..6 {
                    let kind = (next(3) > 0).then(|| kinds[next(7) as usize]);
                    blocks.paint_cell(cx + d, cz + d / 2, kind);
                }
            } else {
                for dz in 0..1 + next(3) as i32 {
                    for dx in 0..1 + next(3) as i32 {
                        let block = Block {
                            level: next(8) as u8,
                            kind: kinds[next(7) as usize],
                            cut: Cut::from_u8(next(5) as u8),
                            ramp: next(9) as u8,
                            style: next(3) as u8,
                        };
                        blocks.set(x + dx, z + dz, block);
                    }
                }
            }
            assert!(
                mesh_cached(&blocks, &mut cache) == mesh(&blocks),
                "after stroke {stroke}"
            );
        }
    }
}
