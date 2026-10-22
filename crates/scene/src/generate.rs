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
use canopy_render::environment::{BiomeConfig, BlockLook, CliffLook, Roots, Shore, Spikes, Zone};
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

/// [`distance`], with the label of the nearest seed carried along.
fn distance_labeled(span: usize, seed: impl Fn(usize) -> Option<u8>) -> (Vec<f32>, Vec<u8>) {
    let far = 1e6f32;
    let mut label = vec![0u8; span * span];
    let mut d: Vec<f32> = (0..span * span)
        .map(|i| match seed(i) {
            Some(l) => {
                label[i] = l;
                0.0
            }
            None => far,
        })
        .collect();
    let (a, b) = (1.0f32, std::f32::consts::SQRT_2);
    let relax = |d: &mut Vec<f32>, label: &mut Vec<u8>, i: usize, j: usize, w: f32| {
        if d[j] + w < d[i] {
            d[i] = d[j] + w;
            label[i] = label[j];
        }
    };
    for z in 0..span {
        for x in 0..span {
            let i = z * span + x;
            if x > 0 {
                relax(&mut d, &mut label, i, i - 1, a);
            }
            if z > 0 {
                relax(&mut d, &mut label, i, i - span, a);
                if x > 0 {
                    relax(&mut d, &mut label, i, i - span - 1, b);
                }
                if x + 1 < span {
                    relax(&mut d, &mut label, i, i - span + 1, b);
                }
            }
        }
    }
    for z in (0..span).rev() {
        for x in (0..span).rev() {
            let i = z * span + x;
            if x + 1 < span {
                relax(&mut d, &mut label, i, i + 1, a);
            }
            if z + 1 < span {
                relax(&mut d, &mut label, i, i + span, a);
                if x + 1 < span {
                    relax(&mut d, &mut label, i, i + span + 1, b);
                }
                if x > 0 {
                    relax(&mut d, &mut label, i, i + span - 1, b);
                }
            }
        }
    }
    (d, label)
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
    /// Distance (cells) to the nearest water cell.
    to_water: Vec<f32>,
    /// The face style of the nearest cliff (its upper column's).
    face: Vec<u8>,
}

impl Cells {
    fn at(&self, wx: f32, wz: f32) -> Option<usize> {
        let halo = MAP_HALO as f32;
        let (x, z) = ((wx + halo).floor(), (wz + halo).floor());
        let span = self.span as f32;
        (x >= 0.0 && z >= 0.0 && x < span && z < span).then(|| z as usize * self.span + x as usize)
    }

    /// The way out of closed ground at cell `i` (down the distance to open ground).
    fn outward(&self, i: usize) -> Vec2 {
        let span = self.span;
        let (x, z) = (i % span, i / span);
        let at = |x: usize, z: usize| self.to_open[z.min(span - 1) * span + x.min(span - 1)];
        let gx = at(x + 1, z) - at(x.saturating_sub(1), z);
        let gz = at(x, z + 1) - at(x, z.saturating_sub(1));
        -Vec2::new(gx, gz).normalize_or(Vec2::X)
    }

    /// `zone` at cell `i`, and when `face` is given, near a cliff of that face.
    /// Whether vertex (`vx`, `vz`) is a corner of a ramp cell.
    fn by_ramp(&self, vx: usize, vz: usize) -> bool {
        let span = self.span;
        (vz.saturating_sub(1)..=vz.min(span - 1)).any(|z| {
            (vx.saturating_sub(1)..=vx.min(span - 1))
                .any(|x| self.kind[z * span + x] == BlockKind::Ramp)
        })
    }

    fn zone_of(&self, i: usize, zone: Zone, face: Option<u8>) -> bool {
        self.zone(i, zone) && face.is_none_or(|f| self.face[i] == f)
    }

    fn zone(&self, i: usize, zone: Zone) -> bool {
        let kind = self.kind[i];
        let open = !kind.closed() && !kind.water();
        let plain = matches!(kind, BlockKind::Ground);
        match zone {
            Zone::Frame => kind.closed() && self.to_open[i] > 4.0,
            Zone::FrameEdge => kind.closed() && self.to_open[i] <= 4.0,
            Zone::Rim => kind.closed() && self.to_open[i] <= 1.5,
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

    /// The highest corner of the cell at (`wx`, `wz`): never under the drawn ground,
    /// whichever way its triangles split.
    fn cell_top(&self, wx: f32, wz: f32) -> f32 {
        let halo = MAP_HALO as f32;
        let (x, z) = ((wx + halo).max(0.0) as usize, (wz + halo).max(0.0) as usize);
        self.vertex(x, z)
            .max(self.vertex(x + 1, z))
            .max(self.vertex(x, z + 1))
            .max(self.vertex(x + 1, z + 1))
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
    let blocks = Blocks::of(terrain)?.ok()?;
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
    let objects = scatter(&cells, recipe, &Dressing::of(map, seed), seed);
    let decals = decals(&cells, &heights, recipe, seed);
    let cliffs = cliffs(&heights, &cells, recipe, seed);
    Some(Generated {
        map: out,
        objects,
        dressing: DressingDesc { cliffs, decals },
    })
}

fn classify(blocks: &Blocks, heights: &Heights, span: usize) -> Cells {
    let b = BLOCK as usize;
    let mut kind = Vec::with_capacity(span * span);
    let mut style = Vec::with_capacity(span * span);
    for cz in 0..span {
        for cx in 0..span {
            let u = ((cx % b) as f32 + 0.5) / b as f32;
            let v = ((cz % b) as f32 + 0.5) / b as f32;
            let block = blocks.sample((cx / b) as i32, (cz / b) as i32, u, v);
            kind.push(block.kind);
            style.push(block.style);
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
    // The face of each cliff cell: the style of the column above the drop.
    let mut face: Vec<Option<u8>> = vec![None; span * span];
    for cz in 1..span - 1 {
        for cx in 1..span - 1 {
            let i = cz * span + cx;
            let h = centre(cx, cz);
            for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                let n = centre((cx as i32 + dx) as usize, (cz as i32 + dz) as usize);
                if h - n > LEVEL_HEIGHT * 0.4 {
                    top[i] = true;
                }
                if n - h > LEVEL_HEIGHT * 0.4 {
                    foot[i] = true;
                }
            }
            // The face is the one of the highest cell within two: the column above the
            // drop, past the half-height cell the drop falls through.
            if top[i] || foot[i] {
                let mut highest = (h, style[i]);
                for dz in -2i32..=2 {
                    for dx in -2i32..=2 {
                        let (nx, nz) = (cx as i32 + dx, cz as i32 + dz);
                        if nx < 0 || nz < 0 || nx >= span as i32 || nz >= span as i32 {
                            continue;
                        }
                        let n = centre(nx as usize, nz as usize);
                        if n > highest.0 {
                            highest = (n, style[nz as usize * span + nx as usize]);
                        }
                    }
                }
                face[i] = Some(highest.1);
            }
        }
    }
    let closed = |i: usize| kind[i].closed();
    let (_, face) = distance_labeled(span, |i| face[i]);
    Cells {
        face,
        to_closed: distance(span, closed),
        to_open: distance(span, |i| !closed(i)),
        to_top: distance(span, |i| top[i]),
        to_foot: distance(span, |i| foot[i]),
        to_shore: distance(span, |i| kind[i] != BlockKind::Ice),
        to_water: distance(span, |i| kind[i].water()),
        kind,
        span,
    }
}

/// A box blur of a per-cell field, `r` cells each way, two passes per axis.
fn blur(field: &[f32], span: usize, r: usize) -> Vec<f32> {
    let mut a = field.to_vec();
    for _ in 0..2 {
        for axis in 0..2 {
            let mut b = vec![0.0; a.len()];
            for z in 0..span {
                for x in 0..span {
                    let mut sum = 0.0;
                    let mut n = 0.0;
                    for k in -(r as i32)..=r as i32 {
                        let (xx, zz) = if axis == 0 {
                            (x as i32 + k, z as i32)
                        } else {
                            (x as i32, z as i32 + k)
                        };
                        if xx >= 0 && zz >= 0 && (xx as usize) < span && (zz as usize) < span {
                            sum += a[zz as usize * span + xx as usize];
                            n += 1.0;
                        }
                    }
                    b[z * span + x] = sum / n;
                }
            }
            a = b;
        }
    }
    a
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
    // Rounded: the distance into closed ground, blurred over three cells and never more
    // than the true distance, so convex corners round off inside the frame and a diagonal
    // edge's staircase of cells runs straight along its inner corners (open ground is never
    // covered, D71); then wandering, eating bays into the frame.
    let blurred = blur(&into, span, 3);
    let into: Vec<f32> = into
        .iter()
        .zip(&blurred)
        .enumerate()
        .map(|(i, (&d, &b))| {
            let (x, z) = ((i % span) as f32, (i / span) as f32);
            let wander = (noise(x * 0.07, z * 0.07, seed ^ 41) - 0.5) * 7.0 - 1.0;
            if d <= 0.0 {
                d
            } else {
                d.min(b).min(d + wander.min(0.0)).max(-0.5)
            }
        })
        .collect();
    let ragged = |x: usize, z: usize, salt: u64| {
        let (wx, wz) = (x as f32 - halo, z as f32 - halo);
        (noise(wx * 0.35, wz * 0.35, seed ^ salt) - 0.5) * 2.2
            + (noise(wx * 1.3, wz * 1.3, seed ^ salt ^ 7) - 0.5) * 0.8
    };
    let heights_before: Vec<f32> = terrain
        .heights
        .0
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
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
    // A lake's bank slopes under its water over a few cells instead of stepping along the
    // cells (the look only, as above): the ground near the water relaxes towards the water
    // just under its surface, and the water spreads over what sinks below it, so its edge
    // runs where the slope crosses the surface.
    {
        let read = |b: &[u8], i: usize| {
            f32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])
        };
        let n = side * side;
        let ground: Vec<f32> = (0..n).map(|i| read(&terrain.heights.0, i)).collect();
        let water: Vec<f32> = (0..n).map(|i| read(&terrain.water_heights.0, i)).collect();
        let wet: Vec<bool> = (0..n).map(|i| water[i] > ground[i] + 0.05).collect();
        // Each vertex near water: the surface of the water nearest it.
        let mut surface = vec![f32::NAN; n];
        let band = 3.5f32;
        for vz in 0..side {
            for vx in 0..side {
                let i = vz * side + vx;
                if wet[i] || at_vertex(&cells.to_water, span, vx, vz) > band {
                    continue;
                }
                let r = 4i32;
                let mut best = (f32::MAX, f32::NAN);
                for dz in -r..=r {
                    for dx in -r..=r {
                        let (x, z) = (vx as i32 + dx, vz as i32 + dz);
                        if x < 0 || z < 0 || x >= side as i32 || z >= side as i32 {
                            continue;
                        }
                        let j = z as usize * side + x as usize;
                        let d = (dx * dx + dz * dz) as f32;
                        if wet[j] && d < best.0 {
                            best = (d, water[j]);
                        }
                    }
                }
                surface[i] = best.1;
            }
        }
        // Relax the band: water vertices held just under the surface, the band averaged.
        let mut h: Vec<f32> = (0..n)
            .map(|i| {
                if wet[i] {
                    ground[i].max(water[i] - 0.6)
                } else {
                    ground[i]
                }
            })
            .collect();
        for _ in 0..6 {
            let prev = h.clone();
            for vz in 1..side - 1 {
                for vx in 1..side - 1 {
                    let i = vz * side + vx;
                    if surface[i].is_nan() {
                        continue;
                    }
                    let mut sum = 0.0;
                    for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                        sum += prev[(vz as i32 + dz) as usize * side + (vx as i32 + dx) as usize];
                    }
                    h[i] = (sum / 4.0 * 0.7 + prev[i] * 0.3).min(ground[i]);
                }
            }
        }
        let mut heights = terrain.heights.0.clone();
        let mut waters = terrain.water_heights.0.clone();
        for i in 0..n {
            if surface[i].is_nan() || h[i] >= ground[i] {
                continue;
            }
            heights[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_le_bytes());
            if h[i] < surface[i] {
                waters[i * 4..i * 4 + 4].copy_from_slice(&surface[i].to_le_bytes());
            }
        }
        terrain.heights = Grid(heights);
        terrain.water_heights = Grid(waters);
    }
    // Beside a ramp, where the drop is too low for a wall, the ground slopes instead of
    // stepping cell by cell down a diagonal side (the look only, as above).
    {
        let read = |bytes: &[u8], i: usize| {
            f32::from_le_bytes([
                bytes[i * 4],
                bytes[i * 4 + 1],
                bytes[i * 4 + 2],
                bytes[i * 4 + 3],
            ])
        };
        let src: Vec<f32> = (0..side * side)
            .map(|i| read(&terrain.heights.0, i))
            .collect();
        let soft: Vec<bool> = (0..side * side)
            .map(|i| {
                let (vx, vz) = (i % side, i / side);
                if !cells.by_ramp(vx, vz)
                    && !(vx > 0 && cells.by_ramp(vx - 1, vz))
                    && !(vz > 0 && cells.by_ramp(vx, vz - 1))
                    && !cells.by_ramp(vx + 1, vz)
                    && !cells.by_ramp(vx, vz + 1)
                {
                    return false;
                }
                let mut most = 0.0f32;
                for z in vz.saturating_sub(1)..=(vz + 1).min(side - 1) {
                    for x in vx.saturating_sub(1)..=(vx + 1).min(side - 1) {
                        most = most.max((src[z * side + x] - src[i]).abs());
                    }
                }
                most > 0.01 && most < RAMP_STEP
            })
            .collect();
        let mut h = src;
        for _ in 0..2 {
            let prev = h.clone();
            for i in (0..side * side).filter(|&i| soft[i]) {
                let (vx, vz) = (i % side, i / side);
                let (mut sum, mut n) = (0.0, 0.0);
                for z in vz.saturating_sub(1)..=(vz + 1).min(side - 1) {
                    for x in vx.saturating_sub(1)..=(vx + 1).min(side - 1) {
                        sum += prev[z * side + x];
                        n += 1.0;
                    }
                }
                h[i] = sum / n;
            }
        }
        terrain.heights = Grid(h.iter().flat_map(|v| v.to_le_bytes()).collect());
    }
    // How far each cell is from the lower half of a ramp: rubble spills out of its foot.
    let to_ramp_foot = distance(span, |i| {
        let (x, z) = (i % span, i / span);
        if cells.kind[i] != BlockKind::Ramp {
            return false;
        }
        let h = heights_before[(z + 1).min(side - 1) * side + x + 1] / LEVEL_HEIGHT;
        h.fract() > 1e-3 && 1.0 - h.fract() > 0.55
    });
    let (mut cover, mut shade, mut rubble, mut ice) = (
        Vec::with_capacity(side * side),
        Vec::with_capacity(side * side),
        Vec::with_capacity(side * side),
        Vec::with_capacity(side * side),
    );
    for vz in 0..side {
        for vx in 0..side {
            let d = at_vertex(&into, span, vx, vz);
            cover.push(recipe.frame_cover * smoothstep(-0.2, 1.4, d + ragged(vx, vz, 11) * 1.1));
            shade.push(recipe.frame_shade * smoothstep(0.0, 6.0, d + ragged(vx, vz, 13)));
            let near_cliff =
                at_vertex(&cells.to_top, span, vx, vz).min(at_vertex(&cells.to_foot, span, vx, vz));
            // Rubble gathers down a ramp, towards its foot.
            let ramp = {
                let x = vx.saturating_sub(1).min(span - 1);
                let z = vz.saturating_sub(1).min(span - 1);
                if cells.kind[z * span + x] == BlockKind::Ramp {
                    let h = heights_before[vz * side + vx] / LEVEL_HEIGHT;
                    let fall = if h.fract() < 1e-3 {
                        0.0
                    } else {
                        1.0 - h.fract()
                    };
                    if recipe.ramp_rubble >= 1.0 {
                        0.0
                    } else {
                        smoothstep(recipe.ramp_rubble, 1.0, fall + ragged(vx, vz, 29) * 0.25)
                    }
                } else {
                    0.0
                }
            };
            let on_ramp = {
                let x = vx.saturating_sub(1).min(span - 1);
                let z = vz.saturating_sub(1).min(span - 1);
                cells.kind[z * span + x] == BlockKind::Ramp
            };
            // Spilled out over the ground beyond the foot, not on the ramp itself.
            let spill = if recipe.ramp_spill > 0.0 && !on_ramp {
                smoothstep(
                    recipe.ramp_spill + 1.0,
                    1.0,
                    at_vertex(&to_ramp_foot, span, vx, vz) + ragged(vx, vz, 31) * 1.8,
                )
            } else {
                0.0
            };
            let r = (recipe.edge_rubble
                * smoothstep(3.2, 0.8, near_cliff + ragged(vx, vz, 17) * 0.9))
            .max(ramp)
            .max(spill);
            rubble.push(r);
            let shore = at_vertex(&cells.to_shore, span, vx, vz);
            ice.push(smoothstep(0.0, 0.8, shore));
        }
    }
    terrain.grass = floats(cover.into_iter());
    terrain.shade = (recipe.frame_shade > 0.0).then(|| floats(shade.into_iter()));
    terrain.rock = floats(std::iter::repeat_n(0.0, side * side));
    let mut paint = Vec::new();
    // Each level's own ground, by the height the blocks made (ramps blend between).
    for level in &recipe.levels {
        let l = f32::from(level.level);
        paint.push(PaintLayer {
            material: level.material.clone(),
            weights: floats(heights_before.iter().map(|h| {
                let at = (h - canopy_content::map::blocks::BASE_HEIGHT) / LEVEL_HEIGHT;
                smoothstep(0.0, 1.0, (1.0 - (at - l).abs()) * 1.6 - 0.3)
            })),
            variants: None,
        });
    }
    // Soft patches over walkable ground: never under the frame's cover, which paint
    // would erase.
    for (k, patch) in recipe.patches.iter().enumerate() {
        let salt = seed ^ (k as u64 + 61).wrapping_mul(0x2127_599B_F432_5C37);
        let threshold = 1.0 - patch.cover.clamp(0.0, 1.0);
        let soft = patch.softness.max(0.01);
        let scale = patch.scale.max(1.0);
        let mut weights = Vec::with_capacity(side * side);
        for vz in 0..side {
            for vx in 0..side {
                let (wx, wz) = (vx as f32 - halo, vz as f32 - halo);
                let d = at_vertex(&into, span, vx, vz);
                let open = 1.0 - smoothstep(-0.2, 1.4, d + ragged(vx, vz, 11) * 1.1);
                if open <= 0.0 {
                    weights.push(0.0);
                    continue;
                }
                // Large shapes, their outline frayed by finer noise so no patch is a blob.
                let n = noise(wx / scale, wz / scale, salt) * 0.62
                    + noise(wx * 2.3 / scale, wz * 2.3 / scale, salt ^ 5) * 0.26
                    + noise(wx * 7.0 / scale, wz * 7.0 / scale, salt ^ 9) * 0.12;
                let mut w = smoothstep(threshold - soft, threshold + soft, n);
                if patch.edge > 0.0 {
                    let out = -d + ragged(vx, vz, 37) * patch.edge * 0.25;
                    w = w.max(smoothstep(patch.edge, patch.edge * 0.15, out));
                }
                if !patch.levels.is_empty() {
                    let at = (heights_before[vz * side + vx]
                        - canopy_content::map::blocks::BASE_HEIGHT)
                        / LEVEL_HEIGHT;
                    w *= patch
                        .levels
                        .iter()
                        .map(|&l| {
                            smoothstep(0.0, 1.0, (1.0 - (at - f32::from(l)).abs()) * 1.6 - 0.3)
                        })
                        .fold(0.0, f32::max);
                }
                weights.push(w * open * patch.strength);
            }
        }
        paint.push(PaintLayer {
            material: patch.material.clone(),
            weights: floats(weights.into_iter()),
            variants: None,
        });
    }
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

/// A map's own frame dressing (`landscape.scatter`) as the generator applies it: an area
/// with a wandering, dithered edge, the biome pieces it takes out there and the layers it
/// adds there.
struct Dressing {
    /// `None`: the whole map.
    area: Option<(Vec2, f32)>,
    fade: f32,
    remove: Option<canopy_content::map::ScatterRemoval>,
    layers: Vec<canopy_render::environment::Scatter>,
    seed: u64,
}

/// Metres a map's dressing blends in over at its edge when it does not say.
const DRESSING_FADE: f32 = 8.0;

impl Dressing {
    /// The map's dressing, in order; layers that do not read as the biome's format are
    /// left out with a warning.
    fn of(map: &UtcMap, seed: u64) -> Vec<Self> {
        let Some(entries) = map.landscape.as_ref().and_then(|l| l.scatter.as_ref()) else {
            return Vec::new();
        };
        let regions = map.mission.as_ref().map_or(&[][..], |m| &m.regions[..]);
        entries
            .iter()
            .enumerate()
            .map(|(o, entry)| {
                let area = match (&entry.area, &entry.region) {
                    (Some(a), _) => Some((Vec2::new(a.x as f32, a.y as f32), a.radius as f32)),
                    (None, Some(id)) => regions
                        .iter()
                        .find(|r| &r.id == id)
                        .map(|r| (Vec2::new(r.x as f32, r.y as f32), r.radius as f32)),
                    (None, None) => None,
                };
                let layers = entry
                    .layers
                    .iter()
                    .enumerate()
                    .filter_map(|(j, layer)| {
                        serde_json::from_value(layer.clone())
                            .map_err(|e| {
                                tracing::warn!("landscape.scatter[{o}].layers[{j}]: {e}");
                            })
                            .ok()
                    })
                    .collect();
                Self {
                    area,
                    fade: entry.fade.map_or(DRESSING_FADE, |f| f as f32),
                    remove: entry.remove.clone(),
                    layers,
                    seed: seed ^ (o as u64 + 1).wrapping_mul(0xA24B_AED4_963E_E407),
                }
            })
            .collect()
    }

    /// Whether `p` falls inside: within the fade the edge wanders and dithers, so the
    /// change reads as a drift rather than a circle.
    fn contains(&self, p: Vec2) -> bool {
        let Some((center, radius)) = self.area else {
            return true;
        };
        let d = p.distance(center);
        if self.fade <= 0.0 {
            return d <= radius;
        }
        let edge = radius + (noise(p.x * 0.045, p.y * 0.045, self.seed) - 0.5) * self.fade;
        let inside = 1.0 - smoothstep(edge - self.fade, edge, d);
        let dither = hash(
            (p.x * 8.0).floor() as i64,
            (p.y * 8.0).floor() as i64,
            7,
            self.seed,
        );
        dither < inside
    }

    /// Whether the biome's `piece` at `p` is taken out.
    fn removes(&self, piece: &str, p: Vec2) -> bool {
        self.remove.as_ref().is_some_and(|r| r.removes(piece)) && self.contains(p)
    }
}

/// The biome's scatter, less what the map's dressing takes out, then the dressing's own
/// layers inside their areas. Taking pieces out never moves the others: every candidate
/// draws its randomness before it is dropped.
fn scatter(
    cells: &Cells,
    recipe: &BlockLook,
    dressing: &[Dressing],
    seed: u64,
) -> Vec<AuthoredObject> {
    let mut out = Vec::new();
    for (k, rule) in recipe.scatter.iter().enumerate() {
        let keep = |p: Vec2, piece: &str| !dressing.iter().any(|d| d.removes(piece, p));
        scatter_layer(cells, rule, k, seed, &keep, &mut out);
    }
    for (o, d) in dressing.iter().enumerate() {
        for (j, rule) in d.layers.iter().enumerate() {
            let k = 1000 + o * canopy_content::map::MAX_MAP_SCATTER + j;
            scatter_layer(cells, rule, k, seed, &|p, _| d.contains(p), &mut out);
        }
    }
    out
}

/// One scatter layer (`k` its seed and id), keeping the pieces `keep` accepts.
fn scatter_layer(
    cells: &Cells,
    rule: &canopy_render::environment::Scatter,
    k: usize,
    seed: u64,
    keep: &dyn Fn(Vec2, &str) -> bool,
    out: &mut Vec<AuthoredObject>,
) {
    if rule.pieces.is_empty() || rule.density <= 0.0 {
        return;
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
        if !cells.zone_of(i, rule.zone, rule.face) {
            continue;
        }
        if let Some([near, far]) = rule.depth {
            let d = cells.to_open[i];
            if d < near || d > far {
                continue;
            }
        }
        if rule.clump > 0.0 {
            let gather = smoothstep(
                0.3,
                0.7,
                noise(p.x * 0.07, p.y * 0.07, seed ^ (k as u64 + 71)),
            );
            if rng.next() > 1.0 - rule.clump + rule.clump * gather {
                continue;
            }
        }
        // A group's piece: from the jittered group cell the point falls in.
        let pick = if rule.groups > 0.0 {
            let g = rule.groups;
            let jx = (noise(p.x / g * 1.7, p.y / g * 1.7, seed ^ 83) - 0.5) * 0.8;
            let jz = (noise(p.x / g * 1.7, p.y / g * 1.7, seed ^ 89) - 0.5) * 0.8;
            hash(
                (p.x / g + jx).floor() as i64,
                (p.y / g + jz).floor() as i64,
                k as i64,
                seed,
            )
        } else {
            rng.next()
        };
        let piece = &rule.pieces[(pick * rule.pieces.len() as f32) as usize % rule.pieces.len()];
        // Rim dressing faces out of the frame and reaches over its edge.
        let (at, yaw) = match rule.reach {
            Some(reach) => {
                let dir = Vec2::from_angle(rng.range(-0.5, 0.5)).rotate(cells.outward(i));
                (
                    p + dir * reach * rng.range(0.5, 1.0),
                    (-dir.x).atan2(-dir.y),
                )
            }
            None => (p, rng.range(0.0, std::f32::consts::TAU)),
        };
        let scale = {
            let random = rng.range(rule.scale[0], rule.scale[1]);
            match rule.depth {
                Some([near, far]) if rule.grow > 0.0 => {
                    let t = ((cells.to_open[i] - near) / (far - near).max(1.0)).clamp(0.0, 1.0);
                    let deep = (rule.scale[0] + (rule.scale[1] - rule.scale[0]) * t)
                        * rng.range(0.88, 1.12);
                    random + (deep - random) * rule.grow
                }
                _ => random,
            }
        };
        if !keep(p, piece) {
            continue;
        }
        out.push(AuthoredObject {
            id: format!("generated.{k}.{placed}"),
            asset: piece.clone(),
            x: f64::from(at.x),
            z: f64::from(at.y),
            elevation: f64::from(rule.elevation),
            yaw: f64::from(yaw),
            scale: f64::from(scale),
            height_mode: HeightMode::Terrain,
            visible: true,
            locked: false,
        });
        placed += 1;
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
            if !cells.zone_of(i, rule.zone, rule.face) {
                continue;
            }
            if let Some([near, far]) = rule.within {
                let d = cells.to_foot[i];
                if d < near || d > far || cells.to_top[i] < d {
                    continue;
                }
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
                frost: rule.frost,
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

/// Cellular noise in 3D on a jittered grid of `size`: the distance to the nearest feature
/// point minus the second nearest (a 0-to-1 edge measure), that feature's hash, and the
/// distance to its centre (0 at the centre, about 1 at its rim).
/// `grain` stretches the stones along each axis (1: as wide as tall).
fn stones_with_centre(p: Vec3, size: f32, grain: Vec3, seed: u64) -> (f32, f32, f32) {
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
                let d = (q - f) / grain;
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

    /// A spike: a tall frosted spire, scaly with rings of buds, from `at`, `height` tall,
    /// curving by `lean`.
    fn spike(&mut self, at: Vec3, height: f32, width: f32, lean: Vec2) {
        let rows = 18;
        let cols = 9;
        let phase = (at.x * 3.1 + at.z * 1.7).fract() * std::f32::consts::TAU;
        let mut points = Vec::with_capacity(rows * cols);
        let mut centres = Vec::with_capacity(rows);
        for r in 0..rows {
            let t = r as f32 / (rows - 1) as f32;
            // A slight bulb above the base, then a long taper to a sharp tip.
            let taper =
                (1.0 - t).powf(0.75) * (1.0 + 0.25 * (1.0 - (t * 6.0 - 0.6).abs()).max(0.0));
            let centre = at + Vec3::new(lean.x * t * t, height * t, lean.y * t * t);
            centres.push(centre);
            for c in 0..cols {
                let a = std::f32::consts::TAU * c as f32 / (cols - 1) as f32;
                // Rings of scales: bumps around and up the spire, offset ring by ring.
                let ring = (t * height * 5.5 + phase).sin() * 0.5 + 0.5;
                let around = ((a * 4.0 + t * height * 9.0 + phase).sin() * 0.5 + 0.5) * ring;
                let radius = width * 0.5 * taper * (0.72 + 0.55 * around);
                points.push(centre + Vec3::new(a.cos(), 0.0, a.sin()) * radius);
            }
        }
        let base = self.vertices.len();
        let first = self.indices.len();
        self.grid(&points, rows, cols);
        for (k, v) in self.vertices[base..].iter_mut().enumerate() {
            let p = Vec3::from_array(v.position) - centres[k / cols];
            // Tilted up, so the spires catch the light and the rime like the frost on them.
            v.normal = (p.normalize_or(Vec3::Y) + Vec3::Y * 0.9)
                .normalize()
                .to_array();
            // Pale with rime (above 1 lightens the material), darker at the foot.
            let r = k / cols;
            v.occlusion = 0.6 + 0.55 * (r as f32 / (rows - 1) as f32).sqrt();
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

    /// A tube through `points`, `radii` thick: a root. Its rings face out; it is open at
    /// both ends, which sink into what it lies on.
    fn tube(&mut self, points: &[Vec3], radii: &[f32]) {
        let n = points.len();
        if n < 2 {
            return;
        }
        let cols = 8;
        let mut grid = Vec::with_capacity(n * cols);
        let mut centres = Vec::with_capacity(n);
        for k in 0..n {
            let t =
                (points[(k + 1).min(n - 1)] - points[k.saturating_sub(1)]).normalize_or(Vec3::X);
            let a = t.cross(Vec3::Y).normalize_or(Vec3::Z);
            let b = a.cross(t).normalize_or(Vec3::Y);
            for c in 0..cols {
                let angle = std::f32::consts::TAU * c as f32 / (cols - 1) as f32;
                grid.push(points[k] + (a * angle.cos() + b * angle.sin()) * radii[k]);
            }
            centres.push(points[k]);
        }
        let base = self.vertices.len();
        let first = self.indices.len();
        self.grid(&grid, n, cols);
        for (i, v) in self.vertices[base..].iter_mut().enumerate() {
            let d = Vec3::from_array(v.position) - centres[i / cols];
            let normal = d.normalize_or(Vec3::Y);
            v.normal = normal.to_array();
            // Dark underneath, where it meets the face.
            let up = normal.y * 0.5 + 0.5;
            v.occlusion = 0.22 + 0.95 * up * up;
        }
        let p =
            |k: usize| Vec3::from_array(self.vertices[self.indices[first + k] as usize].position);
        let facing = (p(1) - p(0)).cross(p(2) - p(0));
        let mid = (p(0) + p(1) + p(2)) / 3.0;
        if facing.dot(mid - centres[0]) < 0.0 {
            for tri in self.indices[first..].chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
        }
    }

    /// A rubble rock at `at`, `size` across: a broken, faceted stone (a ball cut by a
    /// handful of random planes), flatter than tall, its facets shaded flat.
    fn rock(&mut self, at: Vec3, size: f32, yaw: f32, seed: u64) {
        let rows = 9;
        let cols = 13;
        let mut planes = Vec::with_capacity(9);
        for k in 0..9u64 {
            let (a, b) = (hash(k as i64, 1, 0, seed), hash(k as i64, 2, 0, seed));
            let theta = a * std::f32::consts::TAU;
            let y = b * 1.6 - 0.6;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let n = Vec3::new(r * theta.cos(), y, r * theta.sin()).normalize_or(Vec3::Y);
            planes.push((n, 0.36 + 0.14 * hash(k as i64, 3, 0, seed)));
        }
        let mut points = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            let phi = std::f32::consts::PI * r as f32 / (rows - 1) as f32;
            for c in 0..cols {
                let theta = std::f32::consts::TAU * c as f32 / (cols - 1) as f32 + yaw;
                let dir = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
                // The nearest cutting plane along this direction bounds the stone.
                let mut reach = 0.55f32;
                for (n, d) in &planes {
                    let along = dir.dot(*n);
                    if along > 1e-3 {
                        reach = reach.min(d / along);
                    }
                }
                let squash = Vec3::new(1.0, 0.6, 0.85);
                points.push(at + dir * reach * size * squash);
            }
        }
        // The poles and the seam share points, so the rock is closed; the winding is
        // turned so its faces look out.
        let base = self.vertices.len();
        let first = self.indices.len();
        self.grid(&points, rows, cols);
        let tri = first + (cols - 1) * 6 * (rows / 2);
        let p = |k: usize| Vec3::from_array(self.vertices[self.indices[tri + k] as usize].position);
        let facing = (p(1) - p(0)).cross(p(2) - p(0));
        let outward = facing.dot((p(0) + p(1) + p(2)) / 3.0 - at) >= 0.0;
        if !outward {
            for tri in self.indices[first..].chunks_exact_mut(3) {
                tri.swap(1, 2);
            }
        }
        for v in &mut self.vertices[base..] {
            let mut n = Vec3::from_array(v.normal);
            if (Vec3::from_array(v.position) - at).dot(n) < 0.0 {
                n = -n;
            }
            v.normal = n.to_array();
            // Dark sides and underneath, the frosted top pale.
            v.occlusion = 0.5 + 0.6 * n.y.max(0.0);
        }
    }
}

/// The least drop beside a ramp that makes a wall: steeper than a ramp ever falls (a
/// level over two blocks at the most). Lower drops beside it slope instead.
const RAMP_STEP: f32 = LEVEL_HEIGHT * 0.14;

/// One steep crossing of a level's middle height: a segment of contour, its two ends'
/// tops and bottoms and the way down.
struct Segment {
    a: Vec2,
    b: Vec2,
    top: [f32; 2],
    bottom: [f32; 2],
    down: Vec2,
    /// The way out at each end, shared with the segment joined there (mitred: longer
    /// where the face turns), so neighbouring stretches of face meet without a notch.
    downs: [Vec2; 2],
    /// How far each end is from the nearer end of the run of face it belongs to (metres;
    /// far for a closed loop).
    end: [f32; 2],
}

impl Segment {
    /// The way out `u` of the way from `a` to `b`.
    fn down_at(&self, u: f32) -> Vec2 {
        self.downs[0] + (self.downs[1] - self.downs[0]) * u
    }
}

/// Fills each segment's distances to the ends of its run: segments joined end to end with
/// the same face (`face[i]`) form a run.
fn runs(segments: &mut [Segment], face: &[usize]) {
    use std::collections::BTreeMap;
    let key = |p: Vec2| ((p.x * 64.0).round() as i64, (p.y * 64.0).round() as i64);
    let mut at: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, s) in segments.iter().enumerate() {
        at.entry(key(s.a)).or_default().push(i);
        at.entry(key(s.b)).or_default().push(i);
    }
    // Where segments meet, they share the way out: the mean of theirs, lengthened so the
    // face keeps its reach round the turn.
    for i in 0..segments.len() {
        let ends = [segments[i].a, segments[i].b];
        for (e, p) in ends.into_iter().enumerate() {
            let own = segments[i].down;
            let others = at.get(&key(p)).map_or(&[][..], |v| v.as_slice());
            // A face turning back on itself (a ramp's side leaving a cliff) keeps its own.
            let sum = others
                .iter()
                .map(|&j| segments[j].down)
                .filter(|d| d.dot(own) > -0.1)
                .fold(Vec2::ZERO, |a, d| a + d);
            let mean = sum.normalize_or(own);
            let cos = mean.dot(own).max(0.6);
            segments[i].downs[e] = mean / cos;
        }
    }
    let neighbour = |i: usize, p: Vec2, at: &BTreeMap<(i64, i64), Vec<usize>>| {
        at.get(&key(p))
            .and_then(|v| v.iter().copied().find(|&j| j != i && face[j] == face[i]))
    };
    let mut seen = vec![false; segments.len()];
    for start in 0..segments.len() {
        if seen[start] {
            continue;
        }
        // Walk back to the run's start (or round a loop), then forward along it.
        let mut first = start;
        let mut from = segments[start].a;
        let mut guard = 0;
        while let Some(j) = neighbour(first, from, &at) {
            let s = &segments[j];
            from = if key(s.a) == key(from) { s.b } else { s.a };
            first = j;
            guard += 1;
            if first == start || guard > segments.len() {
                break;
            }
        }
        let looped = first == start && guard > 0;
        let mut chain = Vec::new();
        let mut i = first;
        let mut p = from;
        loop {
            seen[i] = true;
            let s = &segments[i];
            let flip = key(s.a) != key(p);
            chain.push((i, flip));
            p = if flip { s.a } else { s.b };
            match neighbour(i, p, &at) {
                Some(j) if !seen[j] => i = j,
                _ => break,
            }
        }
        let total: f32 = chain
            .iter()
            .map(|&(i, _)| (segments[i].b - segments[i].a).length())
            .sum();
        let mut walked = 0.0;
        for &(i, flip) in &chain {
            let l = (segments[i].b - segments[i].a).length();
            let e = |d: f32| if looped { 1e3 } else { d.min(total - d) };
            let (ea, eb) = (e(walked), e(walked + l));
            segments[i].end = if flip { [eb, ea] } else { [ea, eb] };
            walked += l;
        }
    }
}

fn contours(heights: &Heights, cells: &Cells, side: usize) -> Vec<Segment> {
    // The least drop across a cell edge that makes a wall: deeper than a shallow water's
    // bed lies; beside a ramp, steeper than a ramp ever falls (a level over two blocks at
    // the most), so its sides keep their walls almost to its foot.
    const STEP: f32 = LEVEL_HEIGHT * 0.25;
    let ramp = |vx: usize, vz: usize| cells.by_ramp(vx, vz);
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
            let near_ramp = ramp(x, z) || ramp(x + 1, z + 1);
            let step = if near_ramp { RAMP_STEP } else { STEP };
            if hi - lo < step {
                continue;
            }
            let corners = [
                Vec2::new(x as f32, z as f32),
                Vec2::new(x as f32 + 1.0, z as f32),
                Vec2::new(x as f32 + 1.0, z as f32 + 1.0),
                Vec2::new(x as f32, z as f32 + 1.0),
            ];
            // A wall crosses each steep edge of the cell at its middle height, so the
            // cells either side of the edge agree where; a ramp's side, falling with the
            // ramp, keeps its wall down to where the drop gives out.
            let mut crossed: Vec<(usize, f32)> = (0..4)
                .filter_map(|e| {
                    let d = (h[e] - h[(e + 1) % 4]).abs();
                    (d >= step).then_some((e, d))
                })
                .collect();
            if crossed.len() % 2 == 1 {
                let least = (0..crossed.len())
                    .min_by(|&i, &j| crossed[i].1.total_cmp(&crossed[j].1))
                    .unwrap_or(0);
                crossed.remove(least);
            }
            let points: Vec<Vec2> = crossed
                .iter()
                .map(|&(e, _)| (corners[e] + corners[(e + 1) % 4]) * 0.5)
                .collect();
            if points.len() < 2 {
                continue;
            }
            for pair in points.chunks_exact(2) {
                let (mut a, mut b) = (pair[0] - Vec2::splat(halo), pair[1] - Vec2::splat(halo));
                // Ground falling into water has no cliff: the water hides its bank, and
                // the shore's crust (`shore`) rims it.
                let mid = (a + b) * 0.5;
                let into_water = [1.0f32, -1.0].iter().any(|s| {
                    let across = Vec2::new(-(b - a).y, (b - a).x).normalize_or(Vec2::X) * *s;
                    let q = mid + across * 0.8;
                    cells.at(q.x, q.y).is_some_and(|i| cells.kind[i].water())
                });
                if into_water {
                    continue;
                }
                // The way down: across the segment, to whichever side is lower (a cell's
                // own slope says nothing where two walls cross it).
                let across = Vec2::new(-(b - a).y, (b - a).x).normalize_or(Vec2::X);
                let mid = (a + b) * 0.5;
                let side = |d: Vec2| {
                    let p = mid + d * 0.3;
                    heights.at(p.x, p.y)
                };
                let down = if side(across) <= side(-across) {
                    across
                } else {
                    -across
                };
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
                    downs: [down; 2],
                    end: [1e3; 2],
                });
            }
        }
    }
    out
}

/// Where a face's surface lies `f` of the way down it (0: under its lip, 1: its foot):
/// its height and how far out from its contour, before stones and wander.
fn profile(look: &CliffLook, top: f32, bottom: f32, f: f32, reach: f32) -> (f32, f32) {
    let drop = (top - bottom).max(0.5);
    let run = look.slope * drop * reach;
    let curve = f.clamp(0.0, 1.0).powf(1.3);
    let hang = look.overhang * (1.0 - f.clamp(0.0, 1.0)).powi(2);
    (
        top + (bottom - top) * f.clamp(0.0, 1.0),
        // Its steep top starts a little out over the drop: the lip covers the ground to
        // there at the top's height, hiding the staircase a diagonal block edge leaves in
        // the ground.
        0.6 + run * 0.95 * curve + hang,
    )
}

/// How far a face wanders out at `at` (world noise, so faces that meet wander alike).
fn wander(look: &CliffLook, at: Vec2, seed: u64) -> f32 {
    (noise(at.x * 0.09, at.y * 0.09, seed ^ 0x3A1)
        + 0.5 * noise(at.x * 0.31, at.y * 0.31, seed ^ 0x3A2)
        - 0.75)
        * look.wander
}

/// How far out a face reaches at `at`, `end` metres from the end of its run: tapering to
/// a steep face at its ends, and longer and shorter along its length.
fn reach(at: Vec2, end: f32, seed: u64) -> f32 {
    let taper = 0.15 + 0.85 * smoothstep(0.0, 7.0, end);
    taper * (0.7 + 0.6 * noise(at.x * 0.07, at.y * 0.07, seed ^ 0x3A3))
}

/// Roots winding across the face along segment `s`, mostly sideways and a little down,
/// half sunk in it, and spilling out over the ground past its foot.
fn roots(
    mesh: &mut MeshBuilder,
    s: &Segment,
    look: &CliffLook,
    rule: &Roots,
    rng: &mut Rng,
    seed: u64,
) {
    let length = (s.b - s.a).length();
    // Few on a low face (a ramp's side towards its foot).
    let many = length * rule.density * smoothstep(0.6, 2.0, s.top[0] - s.bottom[0]);
    let n = many.floor() as usize + usize::from(rng.next() < many.fract());
    let along = (s.b - s.a) / length.max(1e-4);
    let (top, bottom) = (s.top[0], s.bottom[0]);
    let drop = (top - bottom).max(0.5);
    let run = look.slope * drop;
    let face = drop.hypot(run).max(0.5);
    for _ in 0..n {
        let mut u = rng.next() * length;
        let mut f = rng.range(-0.05, 0.9);
        let side = if rng.next() < 0.5 { -1.0 } else { 1.0 };
        let mut heading = rng.range(0.15, 0.7);
        let total = rng.range(rule.length[0], rule.length[1]);
        let r0 = rng.range(rule.radius[0], rule.radius[1]);
        let steps = ((total / 0.3).ceil() as usize).max(3);
        let mut points = Vec::with_capacity(steps);
        let mut radii = Vec::with_capacity(steps);
        for k in 0..steps {
            let t = k as f32 / (steps - 1) as f32;
            let r = r0 * (1.0 - 0.75 * t);
            let at = s.a + along * u;
            let e = s.end[0] + (s.end[1] - s.end[0]) * (u / length).clamp(0.0, 1.0);
            let k = reach(at, e - u.min(0.0).abs(), seed);
            let (y, out) = if f <= 1.0 {
                profile(look, top, bottom, f, k)
            } else {
                let (_, foot_out) = profile(look, top, bottom, 1.0, k);
                (bottom, foot_out + (f - 1.0) * face)
            };
            let out = out + wander(look, at, seed) + look.relief * 0.35;
            let p = Vec3::new(at.x + s.down.x * out, y + r * 0.4, at.y + s.down.y * out);
            points.push(p);
            radii.push(r);
            heading = (heading + rng.range(-0.3, 0.3)).clamp(-0.2, 1.3);
            u += side * heading.cos() * 0.3;
            f += heading.sin() * 0.3 / face;
        }
        mesh.tube(&points, &radii);
    }
}

/// One stretch of cliff face along segment `s`, as a talus of stones from a lip on the
/// plateau out over the lower ground, in `look`.
fn face_strip(mesh: &mut MeshBuilder, heights: &Heights, s: &Segment, look: &CliffLook, seed: u64) {
    let length = (s.b - s.a).length();
    let cols = ((length / 0.33).ceil() as usize).max(1) + 1;
    let drop = (s.top[0] - s.bottom[0]).max(0.5);
    let run = look.slope * drop;
    // Rows as close down the face as columns along it, however long its run.
    let rows = ((drop.hypot(run) / 0.33).ceil() as usize).max(2) + 3;
    let grain = Vec3::from_array(look.grain);
    let mut points = Vec::with_capacity(rows * cols);
    let mut open = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for c in 0..cols {
            let u = c as f32 / (cols - 1) as f32;
            let at = s.a + (s.b - s.a) * u;
            let bottom = s.bottom[0] + (s.bottom[1] - s.bottom[0]) * u;
            let top = s.top[0] + (s.top[1] - s.top[0]) * u;
            let down = s.down_at(u);
            // From a lip on the plateau out over the lower ground, steepest under the
            // lip, spreading at the foot; an overhang pushes the upper face out.
            let k = reach(at, s.end[0] + (s.end[1] - s.end[0]) * u, seed);
            let back = (run * k * 0.35).min(1.2);
            let w = wander(look, at, seed);
            let (y, out) = if r == 0 {
                // The lip wanders only out over the drop, so the block's straight edge
                // stays under the face.
                (
                    top + 0.06,
                    -0.5 - back + look.overhang * 0.6 + (w + look.wander * 0.75) * 0.8 - w,
                )
            } else if r == rows - 1 {
                (bottom - 0.3, 0.5 + run * k * 0.75)
            } else {
                profile(look, top, bottom, (r - 1) as f32 / (rows - 3) as f32, k)
            };
            // The profile wanders in and out along the face: near the top only out over
            // the drop (in front of the block's straight edge), freely towards the foot.
            let f = r as f32 / (rows - 1) as f32;
            let out = out + w + look.wander * 0.75 * (1.0 - f).powi(2);
            // Along a diagonal, the lip reaches back past the corners its staircase of cells
            // leaves (half a cell's diagonal behind the wall), however it wanders.
            let out = if r == 0 {
                let diagonal = smoothstep(
                    0.25,
                    0.7,
                    2.0 * (down.x * down.y).abs() / down.length_squared().max(1e-6),
                );
                out.min(-0.75 + (1.0 - diagonal) * 10.0)
            } else {
                out
            };
            let (px, pz) = (at.x + down.x * out, at.y + down.y * out);
            // The lip lies on the ground it covers: beside a ramp, the ramp's slope, so it
            // never floats over it (on the cell's highest corner, so never under it either).
            let y = if r == 0 {
                y.min(heights.cell_top(px, pz) + 0.06)
            } else if r == 1 {
                // Where the face turns down, a little over the ground's own edge, so the
                // teeth a diagonal leaves stay under it.
                y + 0.06
            } else {
                y
            };
            let p = Vec3::new(px, y, pz);
            // Domed stones of their own depth, deep grooves between them; the lip and
            // the foot stand out less.
            let (edge, id, centre) = stones_with_centre(p, look.stone, grain, seed);
            let face = if r == 0 || r == rows - 1 { 0.35 } else { 1.0 };
            let dome = 1.0 - 0.55 * (centre * centre).min(1.0);
            let bulge = look.relief * face * (0.2 + 0.8 * id) * dome * smoothstep(0.0, 0.3, edge);
            let lift = if r == 0 { bulge * 0.4 } else { 0.0 };
            let push = if r == 0 { 0.0 } else { bulge };
            points.push(p + Vec3::new(down.x * push, lift, down.y * push));
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
}

/// Frosted spikes in tufts over `zone` (near cliffs of face `face`, when given).
fn spikes(
    mesh: &mut MeshBuilder,
    heights: &Heights,
    cells: &Cells,
    spikes: &Spikes,
    face: Option<u8>,
    seed: u64,
) {
    let mut srng = Rng(seed ^ 0x5B1E ^ u64::from(face.unwrap_or(0)) << 8);
    let spacing = (100.0 / spikes.density.max(0.01)).sqrt();
    for p in candidates(cells.span, spacing, &mut srng) {
        let Some(i) = cells.at(p.x, p.y) else {
            continue;
        };
        if !cells.zone_of(i, spikes.zone, face) {
            continue;
        }
        // In tufts: a few spikes around each point that passes.
        if noise(p.x * 0.2, p.y * 0.2, seed ^ 0x77) < 0.45 {
            continue;
        }
        let tuft = 3 + (srng.next() * 5.0) as usize;
        for k in 0..tuft {
            let at = p + Vec2::new(srng.range(-0.9, 0.9), srng.range(-0.9, 0.9));
            // One or two tall ones in a tuft, the rest shorter.
            let tall = if k < 2 { 1.0 } else { srng.range(0.35, 0.75) };
            let height = srng.range(spikes.height[0], spikes.height[1]) * tall;
            let lean = Vec2::new(srng.range(-0.06, 0.06), srng.range(-0.06, 0.06)) * height;
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

/// A low bank of crusted stones along `a`–`b`, `rim.width` across towards `across`
/// (centred a little behind the line), on `ground`, shifted across by `shift`; loose
/// stones beside it.
#[allow(clippy::too_many_arguments)]
fn rim_strip(
    mesh: &mut MeshBuilder,
    a: Vec2,
    b: Vec2,
    across: [Vec2; 2],
    rim: &Shore,
    ground: impl Fn(Vec2) -> f32,
    shift: impl Fn(Vec2) -> f32,
    seed: u64,
    rng: &mut Rng,
) {
    let length = (b - a).length();
    let grain = Vec3::new(1.0, 0.7, 1.0);
    let cols = ((length / 0.3).ceil() as usize).max(1) + 1;
    let rows = 7;
    let mut grid = Vec::with_capacity(rows * cols);
    let mut open = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        let t = r as f32 / (rows - 1) as f32;
        for c in 0..cols {
            let u = c as f32 / (cols - 1) as f32;
            let along = a + (b - a) * u;
            let across = across[0] + (across[1] - across[0]) * u;
            let p = along + across * ((t - 0.45) * rim.width + shift(along));
            let g = ground(p);
            let profile = (std::f32::consts::PI * t).sin().max(0.0).powf(0.6);
            let p3 = Vec3::new(p.x, g, p.y);
            let (edge, id, centre) = stones_with_centre(p3, rim.stone, grain, seed);
            let dome = 1.0 - 0.6 * (centre * centre).min(1.0);
            let bump = rim.height * (0.3 + 0.9 * id) * dome * smoothstep(0.0, 0.25, edge);
            // Higher and lower along its length, broken here and there.
            let swell = noise(p.x * 0.18, p.y * 0.18, seed ^ 0x5408);
            let swell = 0.15 + 1.5 * swell * swell;
            let y = g - 0.06 + profile * swell * (rim.height * 0.45 + bump);
            grid.push(Vec3::new(p.x, y, p.y));
            open.push((0.3 + 0.7 * smoothstep(0.02, 0.3, edge)) * (0.7 + 0.3 * profile));
        }
    }
    // Wound so it faces up: across × along.
    let dir = b - a;
    let flip = dir.x * across[0].y - dir.y * across[0].x < 0.0;
    let base = mesh.vertices.len();
    let first = mesh.indices.len();
    mesh.grid(&grid, rows, cols);
    for (v, o) in mesh.vertices[base..].iter_mut().zip(open) {
        v.occlusion = o;
        if v.normal[1] < 0.0 {
            v.normal = [-v.normal[0], -v.normal[1], -v.normal[2]];
        }
    }
    if flip {
        for tri in mesh.indices[first..].chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
    }
    let n = (length * rim.rubble).floor() as usize
        + usize::from(rng.next() < (length * rim.rubble).fract());
    for _ in 0..n {
        let u = rng.next();
        let side = across[0] + (across[1] - across[0]) * u;
        let p = a + (b - a) * u + side * rng.range(-0.9, 0.9) * rim.width;
        let size = rng.range(0.25, 0.8) * rim.stone;
        let y = ground(p);
        mesh.rock(
            Vec3::new(p.x, y + size * 0.08, p.y),
            size,
            rng.range(0.0, std::f32::consts::TAU),
            seed ^ rng.0,
        );
    }
}

/// The shore's rim: a low bank of crusted stones wherever walkable ground meets ice at
/// about its level, along a smoothed outline of the ice.
fn shore(mesh: &mut MeshBuilder, heights: &Heights, cells: &Cells, shore: &Shore, seed: u64) {
    let span = cells.span;
    let halo = MAP_HALO as f32;
    let raw: Vec<f32> = cells
        .kind
        .iter()
        .map(|k| f32::from(u8::from(*k == BlockKind::Ice)))
        .collect();
    let field = blur(&raw, span, 1);
    let at = |x: usize, z: usize| {
        let (wx, wz) = (x as f32 - halo, z as f32 - halo);
        field[z.min(span - 1) * span + x.min(span - 1)]
            + (noise(wx * 0.22, wz * 0.22, seed ^ 0x5407) - 0.5) * 0.35
    };
    let mut rng = Rng(seed ^ 0x5407);
    for z in 0..span - 1 {
        for x in 0..span - 1 {
            // Samples at cell centres, corners of a square between them.
            let h = [at(x, z), at(x + 1, z), at(x + 1, z + 1), at(x, z + 1)];
            let corners = [
                Vec2::new(x as f32 + 0.5, z as f32 + 0.5),
                Vec2::new(x as f32 + 1.5, z as f32 + 0.5),
                Vec2::new(x as f32 + 1.5, z as f32 + 1.5),
                Vec2::new(x as f32 + 0.5, z as f32 + 1.5),
            ];
            let mut points = Vec::new();
            for e in 0..4 {
                let (p, q) = (e, (e + 1) % 4);
                if (h[p] > 0.5) != (h[q] > 0.5) {
                    let f = (0.5 - h[p]) / (h[q] - h[p]);
                    points.push(corners[p] + (corners[q] - corners[p]) * f - Vec2::splat(halo));
                }
            }
            if points.len() < 2 {
                continue;
            }
            let gx = (h[1] + h[2]) - (h[0] + h[3]);
            let gz = (h[2] + h[3]) - (h[0] + h[1]);
            let to_ice = Vec2::new(gx, gz).normalize_or(Vec2::X);
            for pair in points.chunks_exact(2) {
                let (a, b) = (pair[0], pair[1]);
                let length = (b - a).length();
                if length < 0.05 {
                    continue;
                }
                // Not under a cliff: the ground and the ice about level.
                let mid = (a + b) * 0.5;
                let (land, ice) = (mid - to_ice * 1.2, mid + to_ice * 1.2);
                if (heights.at(land.x, land.y) - heights.at(ice.x, ice.y)).abs() > 1.0 {
                    continue;
                }
                rim_strip(
                    mesh,
                    a,
                    b,
                    [to_ice; 2],
                    shore,
                    |p| heights.at(p.x, p.y),
                    |_| 0.0,
                    seed,
                    &mut rng,
                );
            }
        }
    }
}

/// The cliffs, one mesh per face material: the biome's cliff (face style 0) and its faces.
fn cliffs(heights: &Heights, cells: &Cells, recipe: &BlockLook, seed: u64) -> Vec<CliffMesh> {
    let _span = tracing::info_span!("blocks_cliffs").entered();
    let Some(cliff) = recipe.cliff.as_ref() else {
        return Vec::new();
    };
    let looks: Vec<&CliffLook> = std::iter::once(cliff)
        .chain(recipe.faces.iter().map(|f| &f.cliff))
        .collect();
    let mut meshes: Vec<MeshBuilder> = looks
        .iter()
        .map(|_| MeshBuilder {
            vertices: Vec::new(),
            indices: Vec::new(),
        })
        .collect();
    // Each face's roots, in their bark.
    let mut barks: Vec<MeshBuilder> = looks
        .iter()
        .map(|_| MeshBuilder {
            vertices: Vec::new(),
            indices: Vec::new(),
        })
        .collect();
    let mut rng = Rng(seed ^ 0xC11F);
    let side = heights.side;
    let mut segments = contours(heights, cells, side);
    // The face of the column above each drop.
    let faces: Vec<usize> = segments
        .iter()
        .map(|s| {
            let up = (s.a + s.b) * 0.5 - s.down * 1.2;
            let style = cells.at(up.x, up.y).map_or(0, |i| cells.face[i]) as usize;
            // A low wall (a ramp's side towards its foot) is a curb of the biome's own
            // stone, whatever face the cliff above it has.
            let low = s.top[0] - s.bottom[0] < LEVEL_HEIGHT * 0.4;
            if style < looks.len() && !low {
                style
            } else {
                0
            }
        })
        .collect();
    runs(&mut segments, &faces);
    for (s, &k) in segments.iter().zip(&faces) {
        let length = (s.b - s.a).length();
        if length < 0.05 {
            continue;
        }
        let look = looks[k];
        face_strip(&mut meshes[k], heights, s, look, seed ^ k as u64);
        // A crust of pale frost along the top, behind the lip.
        if let Some(crown) = &look.crown {
            let drop = (s.top[0] - s.bottom[0]).max(0.5);
            let back = 0.5 + (look.slope * drop * 0.35).min(1.2);
            let top = s.top[0];
            rim_strip(
                &mut meshes[0],
                s.a,
                s.b,
                [-s.downs[0], -s.downs[1]],
                crown,
                |p| heights.at(p.x, p.y).max(top - 0.25),
                |p| {
                    // On the lip, where the face's top turns over: a pale crust over the
                    // edge, following its wander.
                    let w = wander(look, p, seed ^ k as u64);
                    0.3 + back - look.overhang * 0.6 - (w + look.wander * 0.75) * 0.8 + w
                },
                seed ^ 0xC20,
                &mut rng,
            );
        }
        if let Some(rule) = &look.roots {
            roots(&mut barks[k], s, look, rule, &mut rng, seed ^ k as u64);
        }
        // Rubble at the foot.
        let drop = (s.top[0] - s.bottom[0]).max(0.5);
        let n = (length * look.rubble).floor() as usize
            + usize::from(rng.next() < (length * look.rubble).fract());
        for _ in 0..n {
            let at = s.a
                + (s.b - s.a) * rng.next()
                + s.down * (0.5 + look.slope * drop * 0.7 + rng.range(0.0, 1.6));
            let size = rng.range(0.45, 1.6) * (0.5 + 0.35 * look.stone);
            let y = heights.at(at.x, at.y);
            meshes[k].rock(
                Vec3::new(at.x, y + size * 0.12, at.y),
                size,
                rng.range(0.0, std::f32::consts::TAU),
                seed ^ rng.0,
            );
        }
    }
    // Rubble on ramps, a few stones along cliff tops, boulders heaped on closed rock.
    let span = cells.span;
    let halo = MAP_HALO as f32;
    let b = BLOCK as i32;
    for cz in 0..span {
        for cx in 0..span {
            let i = cz * span + cx;
            let (wx, wz) = (cx as f32 - halo, cz as f32 - halo);
            let rock = cells.kind[i] == BlockKind::Rock;
            let chance = if cells.kind[i] == BlockKind::Ramp {
                // Gathered towards the foot.
                let h = heights.at(wx + 0.5, wz + 0.5) / LEVEL_HEIGHT;
                let fall = if h.fract() < 1e-3 {
                    0.0
                } else {
                    1.0 - h.fract()
                };
                cliff.ramp_rubble as f32 / (b * b) as f32 * fall * fall * 2.0
            } else if cells.zone_of(i, Zone::CliffTop, Some(0)) && cells.to_top[i] <= 1.5 {
                cliff.top_rubble
            } else if rock {
                0.22
            } else if !cells.kind[i].water() && cells.to_water[i] <= 1.5 {
                // Frosted stones along a lake's shore, half in its water.
                0.16
            } else {
                0.0
            };
            if chance <= 0.0 || rng.next() > chance {
                continue;
            }
            let at = Vec2::new(wx + rng.next(), wz + rng.next());
            let size = if rock {
                rng.range(1.0, 2.8)
            } else if cells.kind[i] == BlockKind::Ramp {
                rng.range(cliff.ramp_stones[0], cliff.ramp_stones[1])
            } else if cells.to_water[i] <= 1.5 {
                rng.range(0.7, 2.0)
            } else {
                rng.range(0.35, 1.1)
            };
            let y = heights.at(at.x, at.y);
            let base = meshes[0].vertices.len();
            meshes[0].rock(
                Vec3::new(at.x, y + size * 0.1, at.y),
                size,
                rng.range(0.0, std::f32::consts::TAU),
                seed ^ rng.0,
            );
            // Heaped boulders shade each other: darker than loose stones.
            if rock {
                for v in &mut meshes[0].vertices[base..] {
                    v.occlusion *= cliff.boulder_shade;
                }
            }
        }
    }
    for (k, look) in looks.iter().enumerate() {
        if let Some(rule) = &look.spikes {
            let near_cliff = matches!(rule.zone, Zone::CliffTop | Zone::CliffFoot);
            let face = near_cliff.then_some(k as u8);
            // In the biome's cliff material, pale stone, whichever face they stand on.
            spikes(&mut meshes[0], heights, cells, rule, face, seed);
        }
    }
    if let Some(rim) = &recipe.shore {
        shore(&mut meshes[0], heights, cells, rim, seed);
    }
    let bark = barks.into_iter().zip(&looks).filter_map(|(mesh, look)| {
        let rule = look.roots.as_ref()?;
        Some(CliffMesh {
            vertices: mesh.vertices,
            indices: mesh.indices,
            albedo: rule.albedo.clone(),
            normal: rule.normal.clone(),
            tile: rule.tile,
            roughness: 0.85,
            frost: rule.frost,
        })
    });
    meshes
        .into_iter()
        .zip(&looks)
        .map(|(mesh, look)| CliffMesh {
            vertices: mesh.vertices,
            indices: mesh.indices,
            albedo: look.albedo.clone(),
            normal: look.normal.clone(),
            tile: look.tile,
            roughness: look.roughness,
            frost: look.frost,
        })
        .chain(bark)
        .collect()
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

    /// Closed ground everywhere, deep in.
    fn frame(span: usize) -> Cells {
        let n = span * span;
        Cells {
            span,
            kind: vec![BlockKind::Wall; n],
            to_closed: vec![0.0; n],
            to_open: vec![30.0; n],
            to_top: vec![99.0; n],
            to_foot: vec![99.0; n],
            to_shore: vec![99.0; n],
            to_water: vec![99.0; n],
            face: vec![0; n],
        }
    }

    #[test]
    fn a_map_s_dressing_changes_only_its_area() {
        let recipe: BlockLook = serde_json::from_value(serde_json::json!({
            "scatter": [{"zone": "frame", "pieces": ["lush-a", "lush-b"], "density": 3, "scale": [1, 1]}]
        }))
        .unwrap();
        let cells = frame(96);
        let (center, radius, fade) = (Vec2::new(32.0, 32.0), 24.0, 6.0);
        let dressing = [Dressing {
            area: Some((center, radius)),
            fade,
            remove: Some(canopy_content::map::ScatterRemoval::Pieces(vec![
                "lush-a".into(),
            ])),
            layers: vec![
                serde_json::from_value(serde_json::json!(
                    {"zone": "frame", "pieces": ["withered"], "density": 3, "scale": [1, 1]}
                ))
                .unwrap(),
            ],
            seed: 5,
        }];
        let plain = scatter(&cells, &recipe, &[], 11);
        let dressed = scatter(&cells, &recipe, &dressing, 11);
        let at = |o: &AuthoredObject| Vec2::new(o.x as f32, o.z as f32).distance(center);
        let same = |a: &AuthoredObject, b: &AuthoredObject| {
            a.asset == b.asset && a.x == b.x && a.z == b.z && a.yaw == b.yaw
        };
        // Beyond the wandering edge nothing moves; the other piece stays everywhere.
        for o in &plain {
            if at(o) > radius + fade || o.asset == "lush-b" {
                assert!(dressed.iter().any(|d| same(d, o)), "{o:?} moved");
            }
        }
        // Inside, the removed piece is gone and the map's own grows, only there.
        let inside = |asset: &str, within: f32| {
            dressed
                .iter()
                .filter(|o| o.asset == asset && at(o) < within)
                .count()
        };
        assert_eq!(inside("lush-a", radius - 1.5 * fade), 0);
        assert!(inside("lush-a", f32::MAX) > 0);
        assert!(inside("withered", radius - 1.5 * fade) > 10);
        assert_eq!(
            inside("withered", f32::MAX),
            inside("withered", radius + fade)
        );
    }

    #[test]
    fn the_hash_is_stable_and_spread() {
        assert_eq!(hash(1, 2, 3, 4), hash(1, 2, 3, 4));
        let mean: f32 = (0..1000).map(|i| hash(i, 0, 0, 9)).sum::<f32>() / 1000.0;
        assert!((0.4..0.6).contains(&mean));
    }
}
