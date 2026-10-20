//! The block view's mesh (D80): every column as a block of flat colour, its top (a sloped
//! top for a ramp, two triangles for a cut), and walls wherever it stands above its
//! neighbour, split at every level so each cube reads as a cube. The heights are the ones
//! the blocks give the map (`Blocks::apply`), water drawn at its surface.

use canopy_content::map::MAP_HALO;
use canopy_content::map::blocks::{
    BASE_HEIGHT, BLOCK, Block, BlockKind, Blocks, Cut, LEVEL_HEIGHT,
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
    })
}

/// The height a block's top is drawn at (water at its surface).
pub fn top(block: Block) -> f32 {
    BASE_HEIGHT
        + block.level as f32 * LEVEL_HEIGHT
        + match block.kind {
            BlockKind::Shallow => -0.2,
            BlockKind::Deep => -0.4,
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
        let i: Vec<u32> = (0..4)
            .map(|k| self.vertex(p[k], n, color, [uv[k][0], uv[k][1], NO_DIAGONAL]))
            .collect();
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
fn edge_height(blocks: &Blocks, x: i32, z: i32, side: (i32, i32), u: f32, v: f32) -> (f32, Block) {
    let size = blocks.side as i32;
    if x < 0 || z < 0 || x >= size || z >= size {
        return (BASE_HEIGHT - LEVEL_HEIGHT, Block::ground(0));
    }
    let block = blocks.at(x, z);
    if block.kind == BlockKind::Ramp {
        let (dx, dz) = blocks.ramp_down(x, z);
        let t = if dx != 0 {
            if dx > 0 { u } else { 1.0 - u }
        } else if dz > 0 {
            v
        } else {
            1.0 - v
        };
        return (top(block) - t * LEVEL_HEIGHT, block);
    }
    // A point just inside the side decides which half of a cut column owns it.
    let (su, sv) = (0.5 + side.0 as f32 * 0.49, 0.5 + side.1 as f32 * 0.49);
    let half = blocks.sample(x, z, su, sv);
    (top(half), half)
}

/// The block view's mesh of `blocks`.
pub fn mesh(blocks: &Blocks) -> BlockoutMesh {
    let _span = tracing::info_span!("blockout_mesh").entered();
    let mut b = Builder {
        mesh: BlockoutMesh::default(),
    };
    let s = BLOCK as f32;
    let halo = MAP_HALO as f32;
    let up = Vec3::Y;
    for z in 0..blocks.side as i32 {
        for x in 0..blocks.side as i32 {
            let block = blocks.at(x, z);
            let (x0, z0) = (x as f32 * s - halo, z as f32 * s - halo);
            let at = |u: f32, v: f32, y: f32| Vec3::new(x0 + u * s, y, z0 + v * s);
            let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            // The top.
            if block.kind == BlockKind::Ramp {
                let h: Vec<f32> = corners
                    .iter()
                    .map(|&(u, v)| edge_height(blocks, x, z, (0, 0), u, v).0)
                    .collect();
                let p: Vec<Vec3> = corners
                    .iter()
                    .zip(&h)
                    .map(|(&(u, v), &y)| at(u, v, y))
                    .collect();
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
                    let ids: Vec<u32> = idx
                        .iter()
                        .map(|&k| {
                            let (u, v) = corners[k];
                            b.vertex(at(u, v, y), up, color(half), [u, v, diag(u, v)])
                        })
                        .collect();
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
                let p: Vec<Vec3> = corners.iter().map(|&(u, v)| at(u, v, y)).collect();
                b.quad([p[0], p[3], p[2], p[1]], up, color(block));
            }
            // Walls where this column stands above its neighbour.
            for ((dx, dz), [ca, cb]) in SIDES {
                let (ha, mine) = edge_height(blocks, x, z, (dx, dz), ca.0, ca.1);
                let (hb, _) = edge_height(blocks, x, z, (dx, dz), cb.0, cb.1);
                // The neighbour's matching corners on its far side.
                let (na, nb) = (
                    (ca.0 - dx as f32, ca.1 - dz as f32),
                    (cb.0 - dx as f32, cb.1 - dz as f32),
                );
                let (la, _) = edge_height(blocks, x + dx, z + dz, (-dx, -dz), na.0, na.1);
                let (lb, _) = edge_height(blocks, x + dx, z + dz, (-dx, -dz), nb.0, nb.1);
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
    }
    b.mesh
}
