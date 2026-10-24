//! The lab's grid as a blockout mesh: open cells flat on the zinc stage, each outlined so
//! the grid reads, and closed cells as raised blocks.

use super::FLOOR;
use canopy_content::lab::Lab;
use canopy_render::blockout::{BlockVertex, BlockoutMesh};
use glam::Vec3;

/// How far the plain border reaches round the grid, covering the map's halo.
const BORDER: f32 = 160.0;
/// How tall a closed cell stands.
const WALL: f32 = 1.2;
/// No diagonal outline on a face.
const NO_DIAGONAL: f32 = 9.0;

fn linear(hex: u32) -> [f32; 3] {
    let c = |v: u32| ((v & 0xff) as f32 / 255.0).powf(2.2);
    [c(hex >> 16), c(hex >> 8), c(hex)]
}

#[derive(Default)]
struct Builder {
    mesh: BlockoutMesh,
}

impl Builder {
    fn quad(&mut self, p: [Vec3; 4], n: Vec3, color: [f32; 3]) {
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let base = self.mesh.vertices.len() as u32;
        for (k, point) in p.iter().enumerate() {
            self.mesh.vertices.push(BlockVertex {
                position: point.to_array(),
                normal: n.to_array(),
                color,
                edge: [uv[k][0], uv[k][1], NO_DIAGONAL],
            });
        }
        self.mesh
            .indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The grid of `lab`: open cells (in tiles of two on big grids, to keep the mesh small)
/// and closed blocks.
pub fn cells(lab: &Lab) -> BlockoutMesh {
    let size = lab.size as usize;
    let closed = lab.closed_cells();
    let open = linear(0x27272a);
    let top = linear(0x71717a);
    let side = linear(0x52525b);
    let tile = if size > 256 { 2 } else { 1 };
    let mut b = Builder::default();
    let y = FLOOR + 0.02;
    // A plain border round the grid, so the stage beyond it reads as nothing.
    let s = size as f32;
    let border = linear(0x18181b);
    for (x0, z0, x1, z1) in [
        (-BORDER, -BORDER, s + BORDER, 0.0),
        (-BORDER, s, s + BORDER, s + BORDER),
        (-BORDER, 0.0, 0.0, s),
        (s, 0.0, s + BORDER, s),
    ] {
        b.quad(
            [
                Vec3::new(x0, y, z1),
                Vec3::new(x1, y, z1),
                Vec3::new(x1, y, z0),
                Vec3::new(x0, y, z0),
            ],
            Vec3::Y,
            border,
        );
    }
    for tz in (0..size).step_by(tile) {
        for tx in (0..size).step_by(tile) {
            let all_closed = (tz..(tz + tile).min(size))
                .all(|z| (tx..(tx + tile).min(size)).all(|x| closed[z * size + x]));
            if all_closed {
                continue;
            }
            let (x0, z0, w) = (tx as f32, tz as f32, tile as f32);
            b.quad(
                [
                    Vec3::new(x0, y, z0 + w),
                    Vec3::new(x0 + w, y, z0 + w),
                    Vec3::new(x0 + w, y, z0),
                    Vec3::new(x0, y, z0),
                ],
                Vec3::Y,
                open,
            );
        }
    }
    let at = |x: i64, z: i64| {
        (0..size as i64).contains(&x)
            && (0..size as i64).contains(&z)
            && closed[z as usize * size + x as usize]
    };
    let h = y + WALL;
    for z in 0..size as i64 {
        for x in 0..size as i64 {
            if !at(x, z) {
                continue;
            }
            let (x0, z0) = (x as f32, z as f32);
            b.quad(
                [
                    Vec3::new(x0, h, z0 + 1.0),
                    Vec3::new(x0 + 1.0, h, z0 + 1.0),
                    Vec3::new(x0 + 1.0, h, z0),
                    Vec3::new(x0, h, z0),
                ],
                Vec3::Y,
                top,
            );
            for (dx, dz) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                if at(x + dx, z + dz) {
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
                        Vec3::new(a.x, y, a.z),
                        Vec3::new(c.x, y, c.z),
                        Vec3::new(c.x, h, c.z),
                        Vec3::new(a.x, h, a.z),
                    ],
                    n,
                    side,
                );
            }
        }
    }
    b.mesh
}
