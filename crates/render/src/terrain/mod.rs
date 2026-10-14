//! Terrain: what the caller describes, and the pure parts of drawing it.
//!
//! - **Geometry**: one vertex per world unit over the map and its halo, drawn as
//!   [`CHUNK`]-cell chunks with geometric mip-maps ([`LODS`] levels, steps 1–8). A chunk's
//!   level is the coarsest whose largest height error stays under a pixel budget on
//!   screen. Edges next to a coarser neighbour follow the neighbour's edge (vertex
//!   shader), so there are no cracks. Heights are pulled from a texture.
//! - **Layers** (old `authoredTerrain`): the base layer, the biome's tiled layers, atlas
//!   layers (corner tiles per texture cell) and cliffs, composited in order. Each one-unit
//!   cell lists the at most [`MAX_CELL_LAYERS`] layers that reach it, starting from the
//!   lowest one that covers it completely, so a pixel blends at most four (D24, Q7).

pub mod gpu;

use crate::camera::CameraView;
use glam::{Vec3, Vec4};

/// Cells per chunk side.
pub const CHUNK: u32 = 32;
/// Levels of detail: vertex steps 1, 2, 4, 8.
pub const LODS: usize = 4;
/// Layers blended per cell.
pub const MAX_CELL_LAYERS: usize = 4;
/// Marks an empty slot in a cell's layer list.
pub const NO_LAYER: u8 = 255;
/// Most layers a terrain may have.
pub const MAX_LAYERS: usize = 32;

/// How a layer covers the ground.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerKind {
    /// The base: everywhere, under everything.
    Base { tiling: f32 },
    /// A tiled texture blended by its vertex weight and height.
    Tiled {
        tiling: f32,
        blend: f32,
        edge: f32,
        breakup: f32,
    },
    /// Corner tiles of an atlas per texture cell; `corners[mask]` is the tile for a
    /// corner mask (NW 1, NE 2, SW 4, SE 8).
    Atlas {
        columns: u32,
        rows: u32,
        corners: [u8; 16],
    },
    /// A wall texture projected on two vertical planes where the slope is steep.
    Cliff {
        start_slope: f32,
        full_slope: f32,
        vertical_size: f32,
    },
}

/// One ground material.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainLayer {
    pub kind: LayerKind,
    /// Pack entries: albedo + roughness, normal + height (or opacity), occlusion + metal.
    pub albedo: String,
    pub normal: String,
    pub occlusion: Option<String>,
    /// Linear tint (× reflectance).
    pub tint: [f32; 3],
    /// Hand-painted: no specular.
    pub painted: bool,
    /// Vertex weights, 0–255, `vertices²`.
    pub mask: Vec<u8>,
    /// Atlas layers: per texture cell, the corner code (`| 16` when the cell is full)
    /// and the full tile.
    pub cells: Vec<[u8; 2]>,
}

impl TerrainLayer {
    /// Whether the layer covers texture cell `cell` completely.
    fn atlas_full(&self, cell: usize) -> bool {
        let code = self.cells.get(cell).map_or(0, |c| c[0]);
        code & 16 != 0 || code & 15 == 15
    }

    fn atlas_corners(&self, cell: usize) -> u8 {
        self.cells.get(cell).map_or(0, |c| c[0] & 15)
    }
}

/// The texture-cell lattice atlas layers use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureGrid {
    /// World position of the first cell's corner (x, z).
    pub start: [f32; 2],
    pub cell_size: f32,
    pub cells: [u32; 2],
}

impl TextureGrid {
    /// The cell containing world `(x, z)`, if any.
    pub fn cell(&self, x: f32, z: f32) -> Option<usize> {
        let cx = ((x - self.start[0]) / self.cell_size).floor();
        let cz = ((z - self.start[1]) / self.cell_size).floor();
        (cx >= 0.0 && cz >= 0.0 && cx < self.cells[0] as f32 && cz < self.cells[1] as f32)
            .then(|| cz as usize * self.cells[0] as usize + cx as usize)
    }
}

/// Render-only ground around the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Surroundings {
    /// Pack entry of the floor texture (albedo).
    pub floor_texture: String,
    /// Linear multiplier.
    pub floor_color: [f32; 3],
    /// Linear haze colour and its start and end distance.
    pub haze_color: [f32; 3],
    pub haze: [f32; 2],
}

/// A terrain to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainDesc {
    /// Cells per side including the halo; a multiple of [`CHUNK`].
    pub span: u32,
    /// World x and z of vertex 0.
    pub origin: f32,
    /// Ground height per vertex, `(span + 1)²`, row index z.
    pub heights: Vec<f32>,
    /// Water surface per vertex; below the ground where dry.
    pub water: Vec<f32>,
    /// In compositing order; the first is the base.
    pub layers: Vec<TerrainLayer>,
    pub grid: TextureGrid,
    pub surroundings: Option<Surroundings>,
}

impl TerrainDesc {
    pub fn vertices(&self) -> u32 {
        self.span + 1
    }

    /// Ground height at vertex `(x, z)` (clamped to the grid).
    pub fn height_at(&self, x: i64, z: i64) -> f32 {
        let last = i64::from(self.span);
        let w = self.vertices() as usize;
        self.heights[z.clamp(0, last) as usize * w + x.clamp(0, last) as usize]
    }

    /// Bilinear ground height at a world position.
    pub fn sample(&self, x: f32, z: f32) -> f32 {
        let fx = (x - self.origin).clamp(0.0, self.span as f32);
        let fz = (z - self.origin).clamp(0.0, self.span as f32);
        let (x0, z0) = (fx.floor(), fz.floor());
        let (tx, tz) = (fx - x0, fz - z0);
        let (x0, z0) = (x0 as i64, z0 as i64);
        let a = self.height_at(x0, z0);
        let b = self.height_at(x0 + 1, z0);
        let c = self.height_at(x0, z0 + 1);
        let d = self.height_at(x0 + 1, z0 + 1);
        (a + (b - a) * tx) + ((c + (d - c) * tx) - (a + (b - a) * tx)) * tz
    }

    /// Checks sizes and limits.
    pub fn validate(&self) -> Result<(), String> {
        let n = (self.vertices() as usize).pow(2);
        if self.span == 0 || !self.span.is_multiple_of(CHUNK) {
            return Err(format!(
                "terrain span {} is not a multiple of {CHUNK}",
                self.span
            ));
        }
        if self.heights.len() != n || self.water.len() != n {
            return Err("height grids have the wrong size".into());
        }
        if self.layers.is_empty() || !matches!(self.layers[0].kind, LayerKind::Base { .. }) {
            return Err("the first layer must be the base".into());
        }
        if self.layers.len() > MAX_LAYERS {
            return Err(format!("more than {MAX_LAYERS} terrain layers"));
        }
        let cells = (self.grid.cells[0] * self.grid.cells[1]) as usize;
        for (i, layer) in self.layers.iter().enumerate() {
            if layer.mask.len() != n {
                return Err(format!("layer {i}: mask has the wrong size"));
            }
            if matches!(layer.kind, LayerKind::Atlas { .. }) && layer.cells.len() != cells {
                return Err(format!("layer {i}: atlas cells have the wrong size"));
            }
        }
        Ok(())
    }
}

impl crate::camera::Ground for TerrainDesc {
    fn height(&self, x: f32, z: f32) -> f32 {
        self.sample(x, z)
    }

    fn water(&self, x: f32, z: f32) -> Option<f32> {
        let (fx, fz) = (
            (x - self.origin).round() as i64,
            (z - self.origin).round() as i64,
        );
        let last = i64::from(self.span);
        let i = fz.clamp(0, last) as usize * self.vertices() as usize + fx.clamp(0, last) as usize;
        (self.water[i] > self.heights[i]).then_some(self.water[i])
    }
}

/// Offset of the layer-list cells from whole units: the texture grid's fraction, so a
/// list cell never straddles a texture cell (the shipped grids start at −0.5).
pub fn selection_offset(desc: &TerrainDesc) -> f32 {
    desc.grid.start[0].rem_euclid(1.0)
}

/// For every one-unit cell (`span²`, row index z, starting at `origin` +
/// [`selection_offset`]), the layers a pixel there blends, in compositing order, padded
/// with [`NO_LAYER`]. The first slot is drawn opaque: it is the base or an atlas layer
/// that covers the cell completely. With more candidates than slots, the first is kept
/// and then those that weigh most. Returns the lists and how many cells had to drop a
/// layer.
pub fn select_layers(desc: &TerrainDesc) -> (Vec<[u8; MAX_CELL_LAYERS]>, usize) {
    let span = desc.span as usize;
    let w = span + 1;
    let offset = selection_offset(desc);
    let mut out = Vec::with_capacity(span * span);
    let mut dropped = 0;
    let mut candidates: Vec<(usize, u8)> = Vec::with_capacity(desc.layers.len());
    for z in 0..span {
        for x in 0..span {
            // The cell's corner, and the texture cells it overlaps.
            let (wx, wz) = (
                desc.origin + offset + x as f32,
                desc.origin + offset + z as f32,
            );
            let mut texture_cells = [usize::MAX; 4];
            for (i, (dx, dz)) in [(0.01, 0.01), (0.99, 0.01), (0.01, 0.99), (0.99, 0.99)]
                .into_iter()
                .enumerate()
            {
                texture_cells[i] = desc.grid.cell(wx + dx, wz + dz).unwrap_or(usize::MAX);
            }
            // Vertices whose weights reach into the cell.
            let reach = |v: usize| (v, (v + 1 + usize::from(offset > 0.0)).min(w - 1));
            let (x0, x1) = reach(x);
            let (z0, z1) = reach(z);
            candidates.clear();
            for (index, layer) in desc.layers.iter().enumerate().rev() {
                let mut weight = 0u8;
                for vz in z0..=z1 {
                    for vx in x0..=x1 {
                        weight = weight.max(layer.mask[vz * w + vx]);
                    }
                }
                let (relevant, opaque) = match layer.kind {
                    LayerKind::Base { .. } => (true, true),
                    LayerKind::Atlas { .. } => {
                        let touched = texture_cells
                            .iter()
                            .any(|&c| c != usize::MAX && layer.atlas_corners(c) != 0);
                        let full = texture_cells
                            .iter()
                            .all(|&c| c != usize::MAX && layer.atlas_full(c));
                        (weight > 0 || touched, full)
                    }
                    LayerKind::Tiled { .. } | LayerKind::Cliff { .. } => (weight > 0, false),
                };
                if relevant {
                    candidates.push((index, weight.max(if opaque { 255 } else { 0 })));
                    if opaque {
                        break;
                    }
                }
            }
            // `candidates` runs top-down and ends with the opaque floor.
            candidates.reverse();
            if candidates.len() > MAX_CELL_LAYERS {
                dropped += 1;
                let floor = candidates[0];
                let mut rest: Vec<(usize, u8)> = candidates[1..].to_vec();
                rest.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                rest.truncate(MAX_CELL_LAYERS - 1);
                rest.sort_by_key(|c| c.0);
                candidates.clear();
                candidates.push(floor);
                candidates.extend(rest);
            }
            let mut slots = [NO_LAYER; MAX_CELL_LAYERS];
            for (slot, (index, _)) in slots.iter_mut().zip(&candidates) {
                *slot = *index as u8;
            }
            out.push(slots);
        }
    }
    (out, dropped)
}

/// Largest height error of each level per chunk: how far the fine vertices are from
/// the coarser level's surface (bilinear between its vertices). Level 0 is exact.
pub fn chunk_errors(desc: &TerrainDesc) -> Vec<[f32; LODS]> {
    let chunks = desc.span / CHUNK;
    let mut out = Vec::with_capacity((chunks * chunks) as usize);
    for cz in 0..chunks {
        for cx in 0..chunks {
            let mut errors = [0.0f32; LODS];
            for (lod, error) in errors.iter_mut().enumerate().skip(1) {
                let step = 1i64 << lod;
                let (x0, z0) = (i64::from(cx * CHUNK), i64::from(cz * CHUNK));
                let mut worst = 0.0f32;
                for z in 0..=i64::from(CHUNK) {
                    for x in 0..=i64::from(CHUNK) {
                        let (gx, gz) = (x0 + x, z0 + z);
                        let (ax, az) = ((x / step) * step, (z / step) * step);
                        let (bx, bz) = (
                            (ax + step).min(i64::from(CHUNK)),
                            (az + step).min(i64::from(CHUNK)),
                        );
                        let tx = if bx > ax {
                            (x - ax) as f32 / (bx - ax) as f32
                        } else {
                            0.0
                        };
                        let tz = if bz > az {
                            (z - az) as f32 / (bz - az) as f32
                        } else {
                            0.0
                        };
                        let h = |px: i64, pz: i64| desc.height_at(x0 + px, z0 + pz);
                        let top = h(ax, az) + (h(bx, az) - h(ax, az)) * tx;
                        let bottom = h(ax, bz) + (h(bx, bz) - h(ax, bz)) * tx;
                        let coarse = top + (bottom - top) * tz;
                        worst = worst.max((desc.height_at(gx, gz) - coarse).abs());
                    }
                }
                *error = worst.max(errors_floor(lod));
            }
            // Coarser levels never claim to be better than finer ones.
            for lod in 1..LODS {
                errors[lod] = errors[lod].max(errors[lod - 1]);
            }
            out.push(errors);
        }
    }
    out
}

/// A small error every coarser level has even on flat ground (silhouettes, shading of
/// steps), so far chunks still prefer finer levels when affordable.
fn errors_floor(lod: usize) -> f32 {
    0.02 * (1u32 << lod) as f32
}

/// Chunk bounds and errors, built once per terrain.
#[derive(Debug, Clone)]
pub struct ChunkGrid {
    pub chunks: u32,
    pub origin: f32,
    /// Per chunk: min and max height.
    pub heights: Vec<[f32; 2]>,
    pub errors: Vec<[f32; LODS]>,
}

/// A chunk chosen for drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkDraw {
    pub x: u32,
    pub z: u32,
    pub lod: u8,
    /// Levels of the neighbours: −x, +x, −z, +z (own level where there is none).
    pub neighbours: [u8; 4],
}

impl ChunkGrid {
    pub fn new(desc: &TerrainDesc) -> Self {
        let chunks = desc.span / CHUNK;
        let mut heights = Vec::with_capacity((chunks * chunks) as usize);
        for cz in 0..chunks {
            for cx in 0..chunks {
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for z in 0..=CHUNK {
                    for x in 0..=CHUNK {
                        let h =
                            desc.height_at(i64::from(cx * CHUNK + x), i64::from(cz * CHUNK + z));
                        lo = lo.min(h);
                        hi = hi.max(h);
                    }
                }
                heights.push([lo, hi]);
            }
        }
        Self {
            chunks,
            origin: desc.origin,
            heights,
            errors: chunk_errors(desc),
        }
    }

    fn bounds(&self, cx: u32, cz: u32) -> (Vec3, Vec3) {
        let [lo, hi] = self.heights[(cz * self.chunks + cx) as usize];
        let x0 = self.origin + (cx * CHUNK) as f32;
        let z0 = self.origin + (cz * CHUNK) as f32;
        (
            Vec3::new(x0, lo, z0),
            Vec3::new(x0 + CHUNK as f32, hi, z0 + CHUNK as f32),
        )
    }

    /// Visible chunks with their levels, for a view of `screen_height` pixels. A level is
    /// accepted while its error projects to at most `max_pixels`.
    pub fn select(
        &self,
        camera: &CameraView,
        screen_height: f32,
        max_pixels: f32,
        out: &mut Vec<ChunkDraw>,
        levels: &mut Vec<u8>,
    ) {
        out.clear();
        levels.clear();
        levels.resize((self.chunks * self.chunks) as usize, 0);
        let planes = camera.frustum();
        let pixels_per_unit = screen_height / (2.0 * (camera.fov_y / 2.0).tan());
        for cz in 0..self.chunks {
            for cx in 0..self.chunks {
                let (min, max) = self.bounds(cx, cz);
                let i = (cz * self.chunks + cx) as usize;
                let distance = (camera.eye.clamp(min, max) - camera.eye).length().max(1.0);
                levels[i] = select_lod(&self.errors[i], distance, pixels_per_unit, max_pixels);
                if !box_visible(&planes, min, max) {
                    levels[i] |= 0x80;
                }
            }
        }
        for cz in 0..self.chunks {
            for cx in 0..self.chunks {
                let i = (cz * self.chunks + cx) as usize;
                if levels[i] & 0x80 != 0 {
                    continue;
                }
                let lod = levels[i];
                let level = |x: i64, z: i64| -> u8 {
                    let n = i64::from(self.chunks);
                    if x < 0 || z < 0 || x >= n || z >= n {
                        lod
                    } else {
                        levels[(z * n + x) as usize] & 0x7f
                    }
                };
                let (x, z) = (i64::from(cx), i64::from(cz));
                out.push(ChunkDraw {
                    x: cx,
                    z: cz,
                    lod,
                    neighbours: [
                        level(x - 1, z),
                        level(x + 1, z),
                        level(x, z - 1),
                        level(x, z + 1),
                    ],
                });
            }
        }
        out.sort_by_key(|c| c.lod);
    }

    /// Chunks inside `planes` (the shadow view), all at one level so no edge needs
    /// stitching.
    pub fn visible_in(&self, planes: &[Vec4; 6], lod: u8, out: &mut Vec<ChunkDraw>) {
        out.clear();
        let lod = lod.min(LODS as u8 - 1);
        for cz in 0..self.chunks {
            for cx in 0..self.chunks {
                let (min, max) = self.bounds(cx, cz);
                if box_visible(planes, min, max) {
                    out.push(ChunkDraw {
                        x: cx,
                        z: cz,
                        lod,
                        neighbours: [lod; 4],
                    });
                }
            }
        }
    }
}

/// The coarsest level whose error stays within `max_pixels` at `distance`.
pub fn select_lod(
    errors: &[f32; LODS],
    distance: f32,
    pixels_per_unit: f32,
    max_pixels: f32,
) -> u8 {
    let mut lod = 0;
    for (level, &error) in errors.iter().enumerate() {
        if error / distance * pixels_per_unit <= max_pixels {
            lod = level as u8;
        } else {
            break;
        }
    }
    lod
}

/// Whether an axis-aligned box intersects the frustum (conservative).
pub fn box_visible(planes: &[Vec4; 6], min: Vec3, max: Vec3) -> bool {
    planes.iter().all(|p| {
        let corner = Vec3::new(
            if p.x >= 0.0 { max.x } else { min.x },
            if p.y >= 0.0 { max.y } else { min.y },
            if p.z >= 0.0 { max.z } else { min.z },
        );
        p.truncate().dot(corner) + p.w >= 0.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{FlatGround, RtsCamera};

    fn flat(span: u32) -> TerrainDesc {
        let n = ((span + 1) * (span + 1)) as usize;
        TerrainDesc {
            span,
            origin: -16.0,
            heights: vec![0.0; n],
            water: vec![-17.0; n],
            layers: vec![TerrainLayer {
                kind: LayerKind::Base { tiling: 1.0 },
                albedo: "a".into(),
                normal: "n".into(),
                occlusion: None,
                tint: [1.0; 3],
                painted: false,
                mask: vec![255; n],
                cells: Vec::new(),
            }],
            grid: TextureGrid {
                start: [-20.5, -20.5],
                cell_size: 5.0,
                cells: [(span + 8) / 5, (span + 8) / 5],
            },
            surroundings: None,
        }
    }

    fn tiled(n: usize, weight: impl Fn(usize) -> u8) -> TerrainLayer {
        TerrainLayer {
            kind: LayerKind::Tiled {
                tiling: 1.0,
                blend: 0.5,
                edge: 0.0,
                breakup: 0.0,
            },
            mask: (0..n).map(weight).collect(),
            ..flat(32).layers[0].clone()
        }
    }

    #[test]
    fn flat_ground_takes_the_coarsest_level_far_away() {
        let mut desc = flat(64);
        let errors = chunk_errors(&desc);
        assert_eq!(errors.len(), 4);
        assert_eq!(select_lod(&errors[0], 1000.0, 1000.0, 1.0), 3);
        assert_eq!(select_lod(&errors[0], 1.0, 1000.0, 1.0), 0);
        // A spike makes coarse levels costly near it.
        let w = 65;
        desc.heights[10 * w + 11] = 5.0;
        let errors = chunk_errors(&desc);
        assert!(errors[0][1] >= 5.0 - 1e-3);
        assert_eq!(select_lod(&errors[0], 100.0, 1000.0, 1.0), 0);
        assert!(errors[0].windows(2).all(|p| p[0] <= p[1]));
    }

    #[test]
    fn chunks_outside_the_view_are_culled_and_levels_vary_with_distance() {
        // Gentle bumps: fine levels pay off near the camera only.
        let mut desc = flat(1056);
        for (i, h) in desc.heights.iter_mut().enumerate() {
            let (x, z) = ((i % 1057) as f32, (i / 1057) as f32);
            *h = ((x * 0.7).sin() + (z * 1.3).sin()) * 0.1;
        }
        let grid = ChunkGrid::new(&desc);
        let mut camera = RtsCamera::new([500.0, 500.0], None);
        camera.zoom_range = (0.5, 8.0);
        camera.set_zoom(6.0);
        let view = camera.view(16.0 / 9.0, &FlatGround);
        let (mut draws, mut levels) = (Vec::new(), Vec::new());
        grid.select(&view, 1080.0, 1.0, &mut draws, &mut levels);
        assert!(!draws.is_empty() && draws.len() < 33 * 33);
        assert!(draws.windows(2).all(|p| p[0].lod <= p[1].lod));
        let near = draws.iter().map(|d| d.lod).min().unwrap();
        let far = draws.iter().map(|d| d.lod).max().unwrap();
        assert!(far > near, "levels {near}..{far}");
        for d in &draws {
            assert!(d.neighbours.iter().all(|&n| n < LODS as u8));
        }
    }

    #[test]
    fn cells_list_layers_from_the_opaque_floor_up() {
        let mut desc = flat(32);
        let n = 33 * 33;
        // Two tiled layers: one everywhere, one only at vertex (5, 5).
        desc.layers.push(tiled(n, |_| 255));
        desc.layers
            .push(tiled(n, |i| if i == 5 * 33 + 5 { 128 } else { 0 }));
        let (cells, dropped) = select_layers(&desc);
        assert_eq!(dropped, 0);
        assert_eq!(cells[0], [0, 1, NO_LAYER, NO_LAYER]);
        assert_eq!(cells[4 * 32 + 4], [0, 1, 2, NO_LAYER]);
        assert_eq!(cells[5 * 32 + 5], [0, 1, 2, NO_LAYER]);
        assert_eq!(cells[6 * 32 + 6], [0, 1, NO_LAYER, NO_LAYER]);
    }

    #[test]
    fn a_full_atlas_cell_hides_what_lies_below() {
        let mut desc = flat(32);
        let n = 33 * 33;
        let cells = (desc.grid.cells[0] * desc.grid.cells[1]) as usize;
        desc.layers.push(tiled(n, |_| 255));
        desc.layers.push(TerrainLayer {
            kind: LayerKind::Atlas {
                columns: 8,
                rows: 4,
                corners: [0; 16],
            },
            cells: vec![[16, 3]; cells],
            ..tiled(n, |_| 255)
        });
        desc.layers.push(tiled(n, |_| 40));
        let (lists, _) = select_layers(&desc);
        assert!(lists.iter().all(|l| *l == [2, 3, NO_LAYER, NO_LAYER]));
    }

    #[test]
    fn crowded_cells_keep_the_floor_and_the_heaviest() {
        let mut desc = flat(32);
        let n = 33 * 33;
        for w in [10u8, 200, 30, 220, 250] {
            desc.layers.push(tiled(n, move |_| w));
        }
        let (lists, dropped) = select_layers(&desc);
        assert_eq!(dropped, 32 * 32);
        assert_eq!(lists[0], [0, 2, 4, 5]);
    }

    #[test]
    fn boxes_are_tested_against_the_frustum() {
        let view = RtsCamera::new([100.0, 100.0], None).view(16.0 / 9.0, &FlatGround);
        let planes = view.frustum();
        assert!(box_visible(
            &planes,
            Vec3::new(90.0, 0.0, 90.0),
            Vec3::new(110.0, 1.0, 110.0)
        ));
        assert!(!box_visible(
            &planes,
            Vec3::new(900.0, 0.0, 900.0),
            Vec3::new(910.0, 1.0, 910.0)
        ));
        let desc = flat(64);
        assert_eq!(desc.sample(3.5, 7.25), 0.0);
        assert!(desc.validate().is_ok());
    }
}
