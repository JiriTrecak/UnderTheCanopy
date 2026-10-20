//! The world generated from a map's blocks (D80): what the biome's `blocks` recipe makes
//! of them. The simulation never sees any of it; it reads the heights, water and closed
//! cells the blocks made (`Blocks::apply`). Everything here is seeded and a pure function
//! of the map and the biome, so the same map always looks the same, and a better
//! generator improves every map.
//!
//! - **Zones** per cell: closed ground (frame) and its edge, open ground and its edge,
//!   cliff tops and feet, ramps, ice and its shore, from distance fields over the cells.
//! - **Ground**: the frame's cover in the biome's grass layer, rubble paint on ramps and
//!   along cliffs, ice paint on ice, frame shade deep in closed ground.
//! - **Cliffs**: a mesh along every steep drop, from contours of the heights between
//!   levels, broken into protruding stone blocks by cellular noise in world space (so
//!   faces join), with a lip at the top and a flared foot; rubble rocks at the foot, on
//!   ramps and a few along the tops.
//! - **Decals** and **scenery** scattered by zone.

use canopy_content::map::blocks::{BLOCK, BlockKind, Blocks, LEVEL_HEIGHT};
use canopy_content::map::{AuthoredObject, HeightMode, MAP_HALO, PaintLayer, UtcMap, grid::Grid};
use canopy_render::dressing::{CliffMesh, CliffVertex, Decal, DecalBatch, DressingDesc};
use canopy_render::environment::{BiomeConfig, BlockLook, Zone};
use glam::{Vec2, Vec3};

/// What the generator made: the map with its generated ground, the scenery it scattered
/// (decoration only: never part of the simulation) and the dressing the renderer draws.
pub struct Generated {
    pub map: UtcMap,
    pub objects: Vec<AuthoredObject>,
    pub dressing: DressingDesc,
}

impl Generated {
    /// The scattered scenery as the renderer's static models, on the map's ground.
    pub fn instances(
        &self,
        scenery: &crate::statics::SceneryModels,
    ) -> Vec<canopy_render::statics::StaticInstance> {
        let terrain = &self.map.authoring.terrain;
        let heights = Heights {
            bytes: &terrain.heights.0,
            side: terrain.vertices() as usize,
        };
        self.objects
            .iter()
            .filter_map(|o| {
                let prototype = scenery.get(&o.asset)?;
                let y = heights.at(o.x as f32, o.z as f32) + o.elevation as f32;
                Some(canopy_render::statics::StaticInstance {
                    model: prototype.model.clone(),
                    transform: glam::Mat4::from_scale_rotation_translation(
                        Vec3::splat(o.scale as f32),
                        glam::Quat::from_rotation_y(o.yaw as f32),
                        Vec3::new(o.x as f32, y, o.z as f32),
                    ),
                    owner_color: None,
                })
            })
            .collect()
    }
}

/// The seeded hash of integer coordinates, 0 to 1.
fn hash(a: i64, b: i64, c: i64, seed: u64) -> f32 {
    let mut h = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (c as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ seed.wrapping_mul(0x27D4_EB2F_1656_67C5);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise in 2D, 0 to 1.
fn noise(x: f32, z: f32, seed: u64) -> f32 {
    let (ix, iz) = (x.floor(), z.floor());
    let (fx, fz) = (x - ix, z - iz);
    let (ux, uz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let (i, j) = (ix as i64, iz as i64);
    let a = hash(i, j, 0, seed);
    let b = hash(i + 1, j, 0, seed);
    let c = hash(i, j + 1, 0, seed);
    let d = hash(i + 1, j + 1, 0, seed);
    a + (b - a) * ux + (c - a) * uz + (a - b - c + d) * ux * uz
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A small seeded generator for scatter.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

/// Distance in cells to the nearest cell where `seed` holds (two-pass chamfer).
fn distance(span: usize, seed: impl Fn(usize) -> bool) -> Vec<f32> {
    let far = 1e6f32;
    let mut d: Vec<f32> = (0..span * span)
        .map(|i| if seed(i) { 0.0 } else { far })
        .collect();
    let (a, b) = (1.0f32, std::f32::consts::SQRT_2);
    for z in 0..span {
        for x in 0..span {
            let i = z * span + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + a);
            }
            if z > 0 {
                v = v.min(d[i - span] + a);
                if x > 0 {
                    v = v.min(d[i - span - 1] + b);
                }
                if x + 1 < span {
                    v = v.min(d[i - span + 1] + b);
                }
            }
            d[i] = v;
        }
    }
    for z in (0..span).rev() {
        for x in (0..span).rev() {
            let i = z * span + x;
            let mut v = d[i];
            if x + 1 < span {
                v = v.min(d[i + 1] + a);
            }
            if z + 1 < span {
                v = v.min(d[i + span] + a);
                if x + 1 < span {
                    v = v.min(d[i + span + 1] + b);
                }
                if x > 0 {
                    v = v.min(d[i + span - 1] + b);
                }
            }
            d[i] = v;
        }
    }
    d
}

/// Per-cell facts the zones are made from.
struct Cells {
    span: usize,
    kind: Vec<BlockKind>,
    /// Distances (cells) to the nearest closed, open, cliff-top, cliff-foot and non-ice cell.
    to_closed: Vec<f32>,
    to_open: Vec<f32>,
    to_top: Vec<f32>,
    to_foot: Vec<f32>,
    to_shore: Vec<f32>,
}

impl Cells {
    fn at(&self, wx: f32, wz: f32) -> Option<usize> {
        let halo = MAP_HALO as f32;
        let (x, z) = ((wx + halo).floor(), (wz + halo).floor());
        let span = self.span as f32;
        (x >= 0.0 && z >= 0.0 && x < span && z < span).then(|| z as usize * self.span + x as usize)
    }

    fn zone(&self, i: usize, zone: Zone) -> bool {
        let kind = self.kind[i];
        let open = !kind.closed() && !kind.water();
        let plain = matches!(kind, BlockKind::Ground);
        match zone {
            Zone::Frame => kind.closed() && self.to_open[i] > 4.0,
            Zone::FrameEdge => kind.closed() && self.to_open[i] <= 4.0,
            Zone::GroundEdge => plain && self.to_closed[i] <= 3.0,
            Zone::Ground => {
                plain && self.to_closed[i] > 3.0 && self.to_top[i] > 3.0 && self.to_foot[i] > 3.0
            }
            Zone::CliffTop => open && self.to_top[i] <= 2.5,
            Zone::CliffFoot => open && self.to_foot[i] <= 3.0,
            Zone::Ramp => kind == BlockKind::Ramp,
            Zone::Ice => kind == BlockKind::Ice && self.to_shore[i] > 2.0,
            Zone::IceEdge => kind == BlockKind::Ice && self.to_shore[i] <= 2.0,
        }
    }
}

/// The map's heights, sampled.
struct Heights<'a> {
    bytes: &'a [u8],
    side: usize,
}

impl Heights<'_> {
    fn vertex(&self, x: usize, z: usize) -> f32 {
        let i = (z.min(self.side - 1) * self.side + x.min(self.side - 1)) * 4;
        f32::from_le_bytes([
            self.bytes[i],
            self.bytes[i + 1],
            self.bytes[i + 2],
            self.bytes[i + 3],
        ])
    }

    fn at(&self, wx: f32, wz: f32) -> f32 {
        let halo = MAP_HALO as f32;
        let (x, z) = ((wx + halo).max(0.0), (wz + halo).max(0.0));
        let (ix, iz) = (x.floor() as usize, z.floor() as usize);
        let (fx, fz) = (x - x.floor(), z - z.floor());
        let a = self.vertex(ix, iz);
        let b = self.vertex(ix + 1, iz);
        let c = self.vertex(ix, iz + 1);
        let d = self.vertex(ix + 1, iz + 1);
        a + (b - a) * fx + (c - a) * fz + (a - b - c + d) * fx * fz
    }
}

/// The world generated from `map`'s blocks for `biome`, when the map has blocks and the
/// biome a `blocks` recipe.
pub fn generate(map: &UtcMap, biome: &BiomeConfig) -> Option<Generated> {
    let recipe = biome.blocks.as_ref()?;
    let terrain = &map.authoring.terrain;
    let blocks = Blocks::decode(terrain.blocks.as_ref()?, terrain.size).ok()?;
    let _span = tracing::info_span!("blocks_generate").entered();
    let seed = u64::from(recipe.seed) ^ u64::from(map.size).wrapping_mul(0x9E37);
    let span = terrain.span() as usize;
    let vertices = terrain.vertices() as usize;
    let cells = classify(
        &blocks,
        &Heights {
            bytes: &terrain.heights.0,
            side: vertices,
        },
        span,
    );
    let mut out = map.clone();
    ground(&mut out, &cells, recipe, biome, seed);
    let heights = Heights {
        bytes: &out.authoring.terrain.heights.0,
        side: vertices,
    };
    let mut objects = Vec::new();
    scatter(&cells, &heights, recipe, seed, &mut objects);
    let decals = decals(&cells, &heights, recipe, seed);
    let cliff = recipe
        .cliff
        .as_ref()
        .map(|look| cliffs(&heights, &cells, &blocks, look, seed));
    Some(Generated {
        map: out,
        objects,
        dressing: DressingDesc { cliff, decals },
    })
}

fn classify(blocks: &Blocks, heights: &Heights, span: usize) -> Cells {
    let b = BLOCK as usize;
    let mut kind = Vec::with_capacity(span * span);
    for cz in 0..span {
        for cx in 0..span {
            let u = ((cx % b) as f32 + 0.5) / b as f32;
            let v = ((cz % b) as f32 + 0.5) / b as f32;
            kind.push(blocks.sample((cx / b) as i32, (cz / b) as i32, u, v).kind);
        }
    }
    // A cell's height: its centre's; a drop to a neighbour of more than half a level is a
    // cliff (ramps fall a quarter level per cell).
    let centre = |cx: usize, cz: usize| {
        (heights.vertex(cx, cz)
            + heights.vertex(cx + 1, cz)
            + heights.vertex(cx, cz + 1)
            + heights.vertex(cx + 1, cz + 1))
            * 0.25
    };
    let mut top = vec![false; span * span];
    let mut foot = vec![false; span * span];
    for cz in 1..span - 1 {
        for cx in 1..span - 1 {
            let h = centre(cx, cz);
            for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let n = centre((cx as i32 + dx) as usize, (cz as i32 + dz) as usize);
                if h - n > LEVEL_HEIGHT * 0.4 {
                    top[cz * span + cx] = true;
                }
                if n - h > LEVEL_HEIGHT * 0.4 {
                    foot[cz * span + cx] = true;
                }
            }
        }
    }
    let closed = |i: usize| kind[i].closed();
    Cells {
        to_closed: distance(span, closed),
        to_open: distance(span, |i| !closed(i)),
        to_top: distance(span, |i| top[i]),
        to_foot: distance(span, |i| foot[i]),
        to_shore: distance(span, |i| kind[i] != BlockKind::Ice),
        kind,
        span,
    }
}

/// Bilinear sample of a per-cell field at a vertex (the mean of the four cells around it).
fn at_vertex(field: &[f32], span: usize, vx: usize, vz: usize) -> f32 {
    let mut sum = 0.0;
    for (dx, dz) in [(0usize, 0usize), (1, 0), (0, 1), (1, 1)] {
        let x = (vx + dx).saturating_sub(1).min(span - 1);
        let z = (vz + dz).saturating_sub(1).min(span - 1);
        sum += field[z * span + x];
    }
    sum * 0.25
}

fn floats(values: impl Iterator<Item = f32>) -> Grid {
    Grid(values.flat_map(f32::to_le_bytes).collect())
}

fn ground(map: &mut UtcMap, cells: &Cells, recipe: &BlockLook, biome: &BiomeConfig, seed: u64) {
    let terrain = &mut map.authoring.terrain;
    let span = cells.span;
    let side = terrain.vertices() as usize;
    let halo = MAP_HALO as f32;
    // Signed distance into closed ground (positive inside), per cell.
    let into: Vec<f32> = (0..span * span)
        .map(|i| {
            if cells.kind[i].closed() {
                cells.to_open[i]
            } else {
                -cells.to_closed[i]
            }
        })
        .collect();
    let ragged = |x: usize, z: usize, salt: u64| {
        let (wx, wz) = (x as f32 - halo, z as f32 - halo);
        (noise(wx * 0.35, wz * 0.35, seed ^ salt) - 0.5) * 2.2
            + (noise(wx * 1.3, wz * 1.3, seed ^ salt ^ 7) - 0.5) * 0.8
    };
    // Closed ground mounds up inside its edge (the look only: the simulation keeps the
    // heights the blocks made).
    if recipe.frame_rise > 0.0 {
        let mut bytes = terrain.heights.0.clone();
        for vz in 0..side {
            for vx in 0..side {
                let d = at_vertex(&into, span, vx, vz);
                let (wx, wz) = (vx as f32 - halo, vz as f32 - halo);
                let rise = recipe.frame_rise
                    * smoothstep(0.5, 4.0, d + ragged(vx, vz, 19))
                    * (0.55 + 0.45 * noise(wx * 0.12, wz * 0.12, seed ^ 23));
                if rise > 0.0 {
                    let i = (vz * side + vx) * 4;
                    let h =
                        f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]])
                            + rise;
                    bytes[i..i + 4].copy_from_slice(&h.to_le_bytes());
                }
            }
        }
        terrain.heights = Grid(bytes);
    }
    let (mut cover, mut shade, mut rubble, mut ice) = (
        Vec::with_capacity(side * side),
        Vec::with_capacity(side * side),
        Vec::with_capacity(side * side),
        Vec::with_capacity(side * side),
    );
    for vz in 0..side {
        for vx in 0..side {
            let d = at_vertex(&into, span, vx, vz);
            // The cover's edge wanders inside the closed ground (never out over open ground,
            // D71): it eats into the frame by up to three cells in bays and tongues.
            let wander = (noise(vx as f32 * 0.09, vz as f32 * 0.09, seed ^ 41) - 0.5) * 6.0 - 1.4;
            cover.push(
                recipe.frame_cover
                    * smoothstep(0.0, 1.6, d + wander.min(0.4) + ragged(vx, vz, 11) * 0.4),
            );
            shade.push(recipe.frame_shade * smoothstep(0.0, 6.0, d + ragged(vx, vz, 13)));
            let near_cliff =
                at_vertex(&cells.to_top, span, vx, vz).min(at_vertex(&cells.to_foot, span, vx, vz));
            let ramp = {
                let x = vx.saturating_sub(1).min(span - 1);
                let z = vz.saturating_sub(1).min(span - 1);
                if cells.kind[z * span + x] == BlockKind::Ramp {
                    1.0
                } else {
                    0.0
                }
            };
            let r = smoothstep(3.2, 0.8, near_cliff + ragged(vx, vz, 17) * 0.9).max(ramp);
            rubble.push(r);
            let shore = at_vertex(&cells.to_shore, span, vx, vz);
            ice.push(smoothstep(0.0, 0.8, shore));
        }
    }
    terrain.grass = floats(cover.into_iter());
    terrain.shade = (recipe.frame_shade > 0.0).then(|| floats(shade.into_iter()));
    terrain.rock = floats(std::iter::repeat_n(0.0, side * side));
    let mut paint = Vec::new();
    // Paint that a biome layer takes feeds that layer; anything else is the map's own.
    let _ = &biome.terrain;
    if let Some(material) = &recipe.rubble {
        paint.push(PaintLayer {
            material: material.clone(),
            weights: floats(rubble.into_iter()),
            variants: None,
        });
    }
    if let Some(material) = &recipe.ice {
        paint.push(PaintLayer {
            material: material.clone(),
            weights: floats(ice.into_iter()),
            variants: None,
        });
    }
    terrain.paint = paint;
}

/// Candidate points `spacing` apart, jittered, over the map and its halo.
fn candidates(span: usize, spacing: f32, rng: &mut Rng) -> Vec<Vec2> {
    let halo = MAP_HALO as f32;
    let n = (span as f32 / spacing).ceil() as usize;
    let mut out = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let x = (i as f32 + rng.next()) * spacing - halo;
            let z = (j as f32 + rng.next()) * spacing - halo;
            out.push(Vec2::new(x, z));
        }
    }
    out
}

fn scatter(
    cells: &Cells,
    heights: &Heights,
    recipe: &BlockLook,
    seed: u64,
    out: &mut Vec<AuthoredObject>,
) {
    let _ = heights;
    for (k, rule) in recipe.scatter.iter().enumerate() {
        if rule.pieces.is_empty() || rule.density <= 0.0 {
            continue;
        }
        let mut rng = Rng(seed ^ (k as u64 + 1).wrapping_mul(0x5851_F42D_4C95_7F2D));
        let spacing = (100.0 / rule.density).sqrt();
        let mut placed = 0;
        for p in candidates(cells.span, spacing, &mut rng) {
            if rule.max.is_some_and(|m| placed >= m) {
                break;
            }
            let Some(i) = cells.at(p.x, p.y) else {
                continue;
            };
            if !cells.zone(i, rule.zone) {
                continue;
            }
            let piece =
                &rule.pieces[(rng.next() * rule.pieces.len() as f32) as usize % rule.pieces.len()];
            out.push(AuthoredObject {
                id: format!("generated.{k}.{placed}"),
                asset: piece.clone(),
                x: f64::from(p.x),
                z: f64::from(p.y),
                elevation: f64::from(rule.elevation),
                yaw: f64::from(rng.range(0.0, std::f32::consts::TAU)),
                scale: f64::from(rng.range(rule.scale[0], rule.scale[1])),
                height_mode: HeightMode::Terrain,
                visible: true,
                locked: false,
            });
            placed += 1;
        }
    }
}

fn decals(cells: &Cells, heights: &Heights, recipe: &BlockLook, seed: u64) -> Vec<DecalBatch> {
    let mut batches: Vec<DecalBatch> = Vec::new();
    for (k, rule) in recipe.decals.iter().enumerate() {
        if rule.density <= 0.0 || rule.count == 0 {
            continue;
        }
        let mut rng = Rng(seed ^ (k as u64 + 101).wrapping_mul(0x2545_F491_4F6C_DD1D));
        let spacing = (100.0 / rule.density).sqrt();
        let mut decals = Vec::new();
        for p in candidates(cells.span, spacing, &mut rng) {
            let Some(i) = cells.at(p.x, p.y) else {
                continue;
            };
            if !cells.zone(i, rule.zone) {
                continue;
            }
            if rule.clump > 0.0 {
                let gather = smoothstep(
                    0.3,
                    0.7,
                    noise(p.x * 0.18, p.y * 0.18, seed ^ (k as u64 + 31)),
                );
                if rng.next() > 1.0 - rule.clump + rule.clump * gather {
                    continue;
                }
            }
            let shade = rng.range(0.85, 1.1);
            decals.push(Decal {
                position: [p.x, heights.at(p.x, p.y), p.y],
                yaw: rng.range(0.0, std::f32::consts::TAU),
                size: rng.range(rule.size[0], rule.size[1]),
                cell: ((rng.next() * rule.count as f32) as u32).min(rule.count - 1) as f32,
                under: rule.under * rng.range(0.6, 1.0),
                pad: 0.0,
                tint: [
                    rule.tint[0] * shade,
                    rule.tint[1] * shade,
                    rule.tint[2] * shade,
                    1.0,
                ],
            });
        }
        match batches.iter_mut().find(|b| b.atlas == rule.atlas) {
            Some(batch) => batch.decals.extend(decals),
            None => batches.push(DecalBatch {
                atlas: rule.atlas.clone(),
                cells: rule.cells,
                decals,
            }),
        }
    }
    batches
}

/// Cellular noise in 3D: (distance to the nearest feature point minus the second nearest,
/// as a 0-to-1 edge measure, and that feature's hash), on a jittered grid of `size`.
fn stones(p: Vec3, size: f32, seed: u64) -> (f32, f32) {
    let (edge, id, _) = stones_with_centre(p, size, seed);
    (edge, id)
}

/// [`stones`] and the distance to the stone's centre (0 at the centre, about 1 at its rim).
fn stones_with_centre(p: Vec3, size: f32, seed: u64) -> (f32, f32, f32) {
    let q = p / size;
    let base = q.floor();
    let (mut f1, mut f2, mut id) = (9.0f32, 9.0f32, 0.0f32);
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = base + Vec3::new(dx as f32, dy as f32, dz as f32);
                let (i, j, k) = (c.x as i64, c.y as i64, c.z as i64);
                // Stones are wider than tall, as stacked blocks.
                let jitter = Vec3::new(
                    hash(i, j, k, seed),
                    hash(i, j, k, seed ^ 1),
                    hash(i, j, k, seed ^ 2),
                );
                let f = c + jitter * 0.8 + 0.1;
                let d = (q - f) * Vec3::new(1.0, 1.6, 1.0);
                let d = d.length();
                if d < f1 {
                    f2 = f1;
                    f1 = d;
                    id = hash(i, j, k, seed ^ 3);
                } else if d < f2 {
                    f2 = d;
                }
            }
        }
    }
    ((f2 - f1).min(1.0), id, f1)
}

struct MeshBuilder {
    vertices: Vec<CliffVertex>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// A grid of `rows` × `cols` points (row-major) as triangles, normals from neighbours.
    fn grid(&mut self, points: &[Vec3], rows: usize, cols: usize) {
        let base = self.vertices.len() as u32;
        for r in 0..rows {
            for c in 0..cols {
                let p = |rr: usize, cc: usize| points[rr.min(rows - 1) * cols + cc.min(cols - 1)];
                let du = p(r, c + 1) - p(r, c.saturating_sub(1));
                let dv = p(r + 1, c) - p(r.saturating_sub(1), c);
                let n = dv.cross(du).normalize_or(Vec3::Y);
                self.vertices.push(CliffVertex {
                    position: points[r * cols + c].to_array(),
                    normal: n.to_array(),
                    occlusion: 1.0,
                });
            }
        }
        for r in 0..rows - 1 {
            for c in 0..cols - 1 {
                let i = base + (r * cols + c) as u32;
                let j = i + cols as u32;
                self.indices
                    .extend_from_slice(&[i, j, i + 1, i + 1, j, j + 1]);
            }
        }
    }

    /// A spike: a tapered, slightly knobbly cone from `at`, `height` tall, leaning by `lean`.
    fn spike(&mut self, at: Vec3, height: f32, width: f32, lean: Vec2) {
        let rows = 6;
        let cols = 6;
        let mut points = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            let t = r as f32 / (rows - 1) as f32;
            let radius = width * 0.5 * (1.0 - t).powf(0.8) * (1.0 + 0.25 * (t * 17.0).sin().abs());
            let centre = at + Vec3::new(lean.x * t * t, height * t, lean.y * t * t);
            for c in 0..cols {
                let a = std::f32::consts::TAU * c as f32 / (cols - 1) as f32;
                points.push(centre + Vec3::new(a.cos(), 0.0, a.sin()) * radius);
            }
        }
        let base = self.vertices.len();
        let first = self.indices.len();
        self.grid(&points, rows, cols);
        for (k, v) in self.vertices[base..].iter_mut().enumerate() {
            let r = k / cols;
            let centre = at + Vec3::new(0.0, height * r as f32 / (rows - 1) as f32, 0.0);
            let p = Vec3::from_array(v.position) - centre;
            v.normal = (p.normalize_or(Vec3::Y) + Vec3::Y * 0.3)
                .normalize()
                .to_array();
        }
        let p =
            |k: usize| Vec3::from_array(self.vertices[self.indices[first + k] as usize].position);
        let facing = (p(1) - p(0)).cross(p(2) - p(0));
        let mid = (p(0) + p(1) + p(2)) / 3.0;
        if facing.dot(Vec3::new(mid.x - at.x, 0.0, mid.z - at.z)) < 0.0 {
            for tri in self.indices[first..].chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
        }
    }

    /// A rubble rock: a displaced octahedron sphere at `at`, `size` across.
    fn rock(&mut self, at: Vec3, size: f32, yaw: f32, seed: u64) {
        let rows = 7;
        let cols = 9;
        let mut points = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            let phi = std::f32::consts::PI * r as f32 / (rows - 1) as f32;
            for c in 0..cols {
                let theta = std::f32::consts::TAU * c as f32 / (cols - 1) as f32 + yaw;
                let dir = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
                // Angular: a few flat facets from a coarse cellular field.
                let (edge, _) = stones(dir * 1.7 + at * 0.37, 1.0, seed);
                let r_ = 0.5 * (0.8 + 0.25 * edge.min(0.6));
                let squash = Vec3::new(1.0, 0.62, 1.0);
                points.push(at + dir * r_ * size * squash);
            }
        }
        // The poles and the seam share points, so the rock is closed; normals point out,
        // and the winding is turned to face them.
        let base = self.vertices.len();
        let first = self.indices.len();
        self.grid(&points, rows, cols);
        for v in &mut self.vertices[base..] {
            let p = Vec3::from_array(v.position) - at;
            v.normal = p.normalize_or(Vec3::Y).to_array();
        }
        // A triangle off the poles tells which way the grid winds.
        let tri = first + (cols - 1) * 6 * (rows / 2);
        let p = |k: usize| Vec3::from_array(self.vertices[self.indices[tri + k] as usize].position);
        let facing = (p(1) - p(0)).cross(p(2) - p(0));
        if facing.dot((p(0) + p(1) + p(2)) / 3.0 - at) < 0.0 {
            for tri in self.indices[first..].chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
        }
    }
}

/// One steep crossing of a level's middle height: a segment of contour, its two ends'
/// tops and bottoms and the way down.
struct Segment {
    a: Vec2,
    b: Vec2,
    top: [f32; 2],
    bottom: [f32; 2],
    down: Vec2,
}

fn contours(heights: &Heights, side: usize) -> Vec<Segment> {
    let halo = MAP_HALO as f32;
    let mut out = Vec::new();
    for z in 0..side - 1 {
        for x in 0..side - 1 {
            let h = [
                heights.vertex(x, z),
                heights.vertex(x + 1, z),
                heights.vertex(x + 1, z + 1),
                heights.vertex(x, z + 1),
            ];
            let (lo, hi) = (
                h.iter().copied().fold(f32::MAX, f32::min),
                h.iter().copied().fold(f32::MIN, f32::max),
            );
            // Only steep cells: a ramp falls a quarter level per cell.
            if hi - lo < LEVEL_HEIGHT * 0.6 {
                continue;
            }
            let corners = [
                Vec2::new(x as f32, z as f32),
                Vec2::new(x as f32 + 1.0, z as f32),
                Vec2::new(x as f32 + 1.0, z as f32 + 1.0),
                Vec2::new(x as f32, z as f32 + 1.0),
            ];
            // The level middle just above this cell's foot (rounded: water and ice lie a
            // little under their level): one wall per drop.
            let level = (lo / LEVEL_HEIGHT).round();
            let t = (level + 0.5) * LEVEL_HEIGHT;
            if t <= lo || t > hi {
                continue;
            }
            let mut points = Vec::new();
            for e in 0..4 {
                let (p, q) = (e, (e + 1) % 4);
                if (h[p] > t) != (h[q] > t) {
                    let f = (t - h[p]) / (h[q] - h[p]);
                    points.push(corners[p] + (corners[q] - corners[p]) * f);
                }
            }
            if points.len() < 2 {
                continue;
            }
            // The way down: against the cell's gradient.
            let gx = (h[1] + h[2]) - (h[0] + h[3]);
            let gz = (h[2] + h[3]) - (h[0] + h[1]);
            let down = -Vec2::new(gx, gz).normalize_or(Vec2::X);
            for pair in points.chunks_exact(2) {
                let (mut a, mut b) = (pair[0] - Vec2::splat(halo), pair[1] - Vec2::splat(halo));
                // Wound so the wall's front (dv × du) faces down the slope.
                let dir = b - a;
                if -dir.y * down.x + dir.x * down.y < 0.0 {
                    std::mem::swap(&mut a, &mut b);
                }
                out.push(Segment {
                    a,
                    b,
                    top: [hi; 2],
                    bottom: [lo; 2],
                    down,
                });
            }
        }
    }
    out
}

fn cliffs(
    heights: &Heights,
    cells: &Cells,
    blocks: &Blocks,
    look: &canopy_render::environment::CliffLook,
    seed: u64,
) -> CliffMesh {
    let _span = tracing::info_span!("blocks_cliffs").entered();
    let mut mesh = MeshBuilder {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    let mut rng = Rng(seed ^ 0xC11F);
    let side = heights.side;
    let segments = contours(heights, side);
    let relief = look.relief;
    for s in &segments {
        let length = (s.b - s.a).length();
        if length < 0.05 {
            continue;
        }
        let cols = ((length / 0.33).ceil() as usize).max(1) + 1;
        let drop = (s.top[0] - s.bottom[0]).max(0.5);
        let rows = ((drop / 0.3).ceil() as usize).max(2) + 3;
        let mut points = Vec::with_capacity(rows * cols);
        let mut open = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            // Rows: a lip over the top, the face, a flared foot.
            let t = r as f32 / (rows - 1) as f32;
            for c in 0..cols {
                let u = c as f32 / (cols - 1) as f32;
                let at = s.a + (s.b - s.a) * u;
                let top = s.top[0] + (s.top[1] - s.top[0]) * u;
                let bottom = s.bottom[0] + (s.bottom[1] - s.bottom[0]) * u;
                // A talus of stones: from a lip on the plateau out over the lower ground at
                // about 50°, steepest under the lip, spreading at the foot.
                let run = look.slope * drop;
                let (y, out) = if r == 0 {
                    (top + 0.02, -0.5 - run * 0.35)
                } else if r == rows - 1 {
                    (bottom - 0.3, 0.5 + run * 0.75)
                } else {
                    let f = (r - 1) as f32 / (rows - 3) as f32;
                    let curve = f.powf(1.3);
                    (
                        top + (bottom - top) * f,
                        -0.5 - run * 0.25 + (run * 0.95) * curve,
                    )
                };
                let p = Vec3::new(at.x + s.down.x * out, y, at.y + s.down.y * out);
                // Stones stand out of the face; the lip and the foot less so.
                // Domed stones of their own depth, deep grooves between them.
                let (edge, id, centre) = stones_with_centre(p, look.stone, seed);
                let face = if r == 0 || r == rows - 1 { 0.35 } else { 1.0 };
                let dome = 1.0 - 0.55 * (centre * centre).min(1.0);
                let bulge = relief * face * (0.2 + 0.8 * id) * dome * smoothstep(0.0, 0.3, edge);
                let lift = if r == 0 { bulge * 0.4 } else { 0.0 };
                let push = if r == 0 { 0.0 } else { bulge };
                let _ = t;
                points.push(p + Vec3::new(s.down.x * push, lift, s.down.y * push));
                // Dark in the grooves, and towards the foot under the overhang of stones.
                let groove = smoothstep(0.02, 0.32, edge);
                let foot = if r == rows - 1 {
                    0.55
                } else {
                    1.0 - 0.3 * (r as f32 / rows as f32)
                };
                open.push((0.25 + 0.75 * groove) * foot * (0.8 + 0.2 * id));
            }
        }
        let base = mesh.vertices.len();
        mesh.grid(&points, rows, cols);
        for (v, o) in mesh.vertices[base..].iter_mut().zip(open) {
            v.occlusion = o;
        }
        // Rubble at the foot.
        let n = (length * look.rubble).floor() as usize
            + usize::from(rng.next() < (length * look.rubble).fract());
        for _ in 0..n {
            let u = rng.next();
            let at = s.a
                + (s.b - s.a) * u
                + s.down * (0.5 + look.slope * drop * 0.7 + rng.range(0.0, 1.6));
            let size = rng.range(0.45, 1.6) * (0.5 + 0.35 * look.stone);
            let y = heights.at(at.x, at.y);
            mesh.rock(
                Vec3::new(at.x, y + size * 0.12, at.y),
                size,
                rng.range(0.0, std::f32::consts::TAU),
                seed ^ rng.0,
            );
        }
    }
    // Rubble on ramps and a few stones along cliff tops.
    let span = cells.span;
    let halo = MAP_HALO as f32;
    let b = BLOCK as i32;
    let _ = blocks;
    for cz in 0..span {
        for cx in 0..span {
            let i = cz * span + cx;
            let (wx, wz) = (cx as f32 - halo, cz as f32 - halo);
            let rock = cells.kind[i] == BlockKind::Rock;
            let chance = if cells.kind[i] == BlockKind::Ramp {
                look.ramp_rubble as f32 / (b * b) as f32
            } else if cells.zone(i, Zone::CliffTop) && cells.to_top[i] <= 1.5 {
                0.3
            } else if rock {
                // Closed rock is heaped with boulders.
                0.22
            } else {
                0.0
            };
            if chance <= 0.0 || rng.next() > chance {
                continue;
            }
            let at = Vec2::new(wx + rng.next(), wz + rng.next());
            let size = if rock {
                rng.range(1.0, 2.8)
            } else {
                rng.range(0.35, 1.1)
            };
            let y = heights.at(at.x, at.y);
            mesh.rock(
                Vec3::new(at.x, y + size * 0.1, at.y),
                size,
                rng.range(0.0, std::f32::consts::TAU),
                seed ^ rng.0,
            );
        }
    }
    // Frosted spikes standing out of closed ground near its edge.
    if let Some(spikes) = &look.spikes {
        let mut srng = Rng(seed ^ 0x5B1E);
        let spacing = (100.0 / spikes.density.max(0.01)).sqrt();
        for p in candidates(span, spacing, &mut srng) {
            let Some(i) = cells.at(p.x, p.y) else {
                continue;
            };
            if !cells.zone(i, Zone::FrameEdge) && !cells.zone(i, Zone::Frame) {
                continue;
            }
            // In tufts: a few spikes around each point that passes.
            if noise(p.x * 0.2, p.y * 0.2, seed ^ 0x77) < 0.45 {
                continue;
            }
            let tuft = 3 + (srng.next() * 4.0) as usize;
            for _ in 0..tuft {
                let at = p + Vec2::new(srng.range(-0.6, 0.6), srng.range(-0.6, 0.6));
                let height = srng.range(spikes.height[0], spikes.height[1]);
                let lean = Vec2::new(srng.range(-0.15, 0.15), srng.range(-0.15, 0.15)) * height;
                let y = heights.at(at.x, at.y) - 0.1;
                mesh.spike(
                    Vec3::new(at.x, y, at.y),
                    height,
                    spikes.width * srng.range(0.7, 1.3),
                    lean,
                );
            }
        }
    }
    CliffMesh {
        vertices: mesh.vertices,
        indices: mesh.indices,
        albedo: look.albedo.clone(),
        normal: look.normal.clone(),
        tile: look.tile,
        roughness: look.roughness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_count_cells_to_the_nearest_seed() {
        let span = 5;
        let d = distance(span, |i| i == 0);
        assert_eq!(d[0], 0.0);
        assert_eq!(d[1], 1.0);
        assert!((d[span + 1] - std::f32::consts::SQRT_2).abs() < 1e-5);
    }

    #[test]
    fn the_hash_is_stable_and_spread() {
        assert_eq!(hash(1, 2, 3, 4), hash(1, 2, 3, 4));
        let mean: f32 = (0..1000).map(|i| hash(i, 0, 0, 9)).sum::<f32>() / 1000.0;
        assert!((0.4..0.6).contains(&mean));
    }
}
