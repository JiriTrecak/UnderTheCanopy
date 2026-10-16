//! The committed authoring scene (`authoring`): terrain grids and placed scenery objects.
//!
//! Grids are stored as canonical base64 of little-endian data:
//!
//! | grid | element | count |
//! |---|---|---|
//! | `heights`, `waterHeights`, `grass`, `rock`, `paint[].weights` | `f32` | `vertices²` |
//! | `waterFlow` | `[u8; 4]` (flow x, flow z, foam, profile) | `span²` |
//! | `paint[].variants` | `u8` | texture cells ([`TextureLayout::count`]) |
//!
//! with `span = size + 2·MAP_HALO` cells per side and `vertices = span + 1`, both starting
//! at `-MAP_HALO` in world units. Rows run along x; row index is z.

use super::grid::Grid;
use super::{MAP_HALO, Version};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::fmt;

/// Lowest editable ground height, in world units.
pub const HEIGHT_MIN: f64 = -16.0;
/// Highest editable ground or water height, in world units.
pub const HEIGHT_MAX: f64 = 24.0;
/// Water height of a dry vertex; below every editable ground sample.
pub const DRY_WATER_HEIGHT: f64 = HEIGHT_MIN - 1.0;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum HeightMode {
    /// Stands on the terrain, `elevation` above it.
    #[default]
    Terrain,
    /// `elevation` is the absolute height.
    Absolute,
}

fn one() -> f64 {
    1.0
}

fn yes() -> bool {
    true
}

/// A placed scenery object (trees, rocks, props), in world units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthoredObject {
    pub id: String,
    /// Landscape catalogue asset id.
    pub asset: String,
    pub x: f64,
    pub z: f64,
    #[serde(default)]
    pub elevation: f64,
    /// Radians.
    #[serde(default)]
    pub yaw: f64,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub height_mode: HeightMode,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
}

/// Texture tiles, independent of the navigation cells: `cellSize` world units, anchored at
/// `origin` (x, z).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextureGrid {
    pub cell_size: f64,
    pub origin: [f64; 2],
}

/// One ground material painted over the terrain.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaintLayer {
    pub material: String,
    /// `f32` blend weight per vertex, 0 to 1.
    pub weights: Grid,
    /// `u8` atlas variant per texture cell.
    pub variants: Option<Grid>,
}

/// Committed terrain samples, as stored.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerrainData {
    pub version: Version<1>,
    /// Playable cells per side; equals the map size.
    pub size: u32,
    pub texture_grid: Option<TextureGrid>,
    /// `f32` ground height per vertex, [`HEIGHT_MIN`] to [`HEIGHT_MAX`].
    pub heights: Grid,
    /// `f32` water surface per vertex, [`DRY_WATER_HEIGHT`] to [`HEIGHT_MAX`].
    pub water_heights: Grid,
    /// [`WaterFlow`] per cell.
    pub water_flow: Grid,
    /// Published water profiles that [`WaterFlow::profile`] refers to (1-based).
    pub water_profiles: Vec<String>,
    /// `f32` grass coverage per vertex, 0 to 1.
    pub grass: Grid,
    /// `f32` rock coverage per vertex, 0 to 1.
    pub rock: Grid,
    pub paint: Vec<PaintLayer>,
}

/// The saved scene: committed terrain plus placed objects. Procedural layers are editor-only
/// and never part of a saved map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedScene {
    pub terrain: TerrainData,
    pub version: Version<1>,
    pub objects: Vec<AuthoredObject>,
}

/// Water flow of one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct WaterFlow {
    /// Flow direction and speed along x; 128 is still.
    pub x: u8,
    /// Flow direction and speed along z; 128 is still.
    pub z: u8,
    pub foam: u8,
    /// 0: the biome's water; `n`: `water_profiles[n - 1]`.
    pub profile: u8,
}

/// Where the texture cells lie, as the old `textureGridLayout` computes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureLayout {
    pub cell_size: f64,
    pub origin: [f64; 2],
    /// World position of the first cell's corner.
    pub start: [f64; 2],
    /// Cells along x and z.
    pub cells: [u32; 2],
    pub count: usize,
}

/// A grid that does not decode to its declared dimensions or values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridError {
    /// Grid path within the terrain, such as `heights` or `paint.2.variants`.
    pub grid: String,
    pub message: String,
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.grid, self.message)
    }
}

impl std::error::Error for GridError {}

/// A paint layer with its grids decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedPaint {
    pub material: String,
    pub weights: Vec<f32>,
    pub variants: Option<Vec<u8>>,
}

/// The committed terrain as typed buffers. Vertex grids hold `vertices²` samples, cell grids
/// `span²`; index `z * side + x`, origin at `-MAP_HALO`.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedTerrain {
    pub size: u32,
    pub span: u32,
    pub vertices: u32,
    pub heights: Vec<f32>,
    pub water_heights: Vec<f32>,
    pub water_flow: Vec<WaterFlow>,
    pub water_profiles: Vec<String>,
    pub grass: Vec<f32>,
    pub rock: Vec<f32>,
    pub paint: Vec<DecodedPaint>,
    pub texture: TextureLayout,
}

impl TerrainData {
    /// Cells per side including the halo.
    pub fn span(&self) -> u32 {
        self.size + 2 * MAP_HALO
    }

    /// Vertices per side.
    pub fn vertices(&self) -> u32 {
        self.span() + 1
    }

    pub fn vertex_count(&self) -> usize {
        (self.vertices() as usize).pow(2)
    }

    pub fn cell_count(&self) -> usize {
        (self.span() as usize).pow(2)
    }

    /// Texture cells covering the halo-inclusive terrain.
    pub fn texture_layout(&self) -> TextureLayout {
        let origin = -f64::from(MAP_HALO);
        let span = f64::from(self.span());
        match &self.texture_grid {
            // The legacy four-unit raster.
            None => {
                let first = origin / 4.0;
                let start = -0.5 + first * 4.0;
                let cells = (span / 4.0) as u32;
                TextureLayout {
                    cell_size: 4.0,
                    origin: [-0.5, -0.5],
                    start: [start, start],
                    cells: [cells, cells],
                    count: cells as usize * cells as usize,
                }
            }
            Some(grid) => {
                let size = grid.cell_size;
                let axis = |anchor: f64| {
                    let first = ((origin - anchor) / size).floor();
                    let start = anchor + first * size;
                    let cells = ((origin + span - start) / size).ceil();
                    (start, cells.max(0.0) as u32)
                };
                let (x0, cx) = axis(grid.origin[0]);
                let (z0, cz) = axis(grid.origin[1]);
                TextureLayout {
                    cell_size: size,
                    origin: grid.origin,
                    start: [x0, z0],
                    cells: [cx, cz],
                    count: cx as usize * cz as usize,
                }
            }
        }
    }

    pub fn heights(&self) -> Result<Vec<f32>, GridError> {
        decode_floats(&self.heights, self.vertex_count()).map_err(|m| grid_error("heights", m))
    }

    pub fn water_heights(&self) -> Result<Vec<f32>, GridError> {
        decode_floats(&self.water_heights, self.vertex_count())
            .map_err(|m| grid_error("waterHeights", m))
    }

    pub fn grass(&self) -> Result<Vec<f32>, GridError> {
        decode_floats(&self.grass, self.vertex_count()).map_err(|m| grid_error("grass", m))
    }

    pub fn rock(&self) -> Result<Vec<f32>, GridError> {
        decode_floats(&self.rock, self.vertex_count()).map_err(|m| grid_error("rock", m))
    }

    pub fn water_flow(&self) -> Result<Vec<WaterFlow>, GridError> {
        let bytes = self
            .water_flow
            .exactly(self.cell_count() * 4)
            .map_err(|m| grid_error("waterFlow", m))?;
        Ok(bytes
            .chunks_exact(4)
            .map(|c| WaterFlow {
                x: c[0],
                z: c[1],
                foam: c[2],
                profile: c[3],
            })
            .collect())
    }

    /// Paint layer `index` decoded.
    pub fn paint(&self, index: usize) -> Result<DecodedPaint, GridError> {
        let layer = &self.paint[index];
        let weights = decode_floats(&layer.weights, self.vertex_count())
            .map_err(|m| grid_error(&format!("paint.{index}"), m))?;
        let variants = layer
            .variants
            .as_ref()
            .map(|grid| {
                grid.exactly(self.texture_layout().count)
                    .map(<[u8]>::to_vec)
            })
            .transpose()
            .map_err(|m| grid_error(&format!("paint.{index}.variants"), m))?;
        Ok(DecodedPaint {
            material: layer.material.clone(),
            weights,
            variants,
        })
    }

    /// Every grid decoded, dimensions checked.
    pub fn decode(&self) -> Result<DecodedTerrain, GridError> {
        Ok(DecodedTerrain {
            size: self.size,
            span: self.span(),
            vertices: self.vertices(),
            heights: self.heights()?,
            water_heights: self.water_heights()?,
            water_flow: self.water_flow()?,
            water_profiles: self.water_profiles.clone(),
            grass: self.grass()?,
            rock: self.rock()?,
            paint: (0..self.paint.len())
                .map(|i| self.paint(i))
                .collect::<Result<_, _>>()?,
            texture: self.texture_layout(),
        })
    }

    /// Every rule of the old `terrainDataSchema` refinement: grid dimensions, finite samples
    /// within their ranges, known water profiles, unique profiles and materials.
    pub fn grid_errors(&self) -> Vec<GridError> {
        let n = self.vertex_count();
        let mut errors = Vec::new();
        let mut floats = |name: String, grid: &Grid, min: f64, max: f64| {
            if let Err(message) = check_floats(grid, n, min, max) {
                errors.push(grid_error(&name, message));
            }
        };
        floats("heights".into(), &self.heights, HEIGHT_MIN, HEIGHT_MAX);
        floats(
            "waterHeights".into(),
            &self.water_heights,
            DRY_WATER_HEIGHT,
            HEIGHT_MAX,
        );
        floats("grass".into(), &self.grass, 0.0, 1.0);
        floats("rock".into(), &self.rock, 0.0, 1.0);
        for (i, layer) in self.paint.iter().enumerate() {
            floats(format!("paint.{i}"), &layer.weights, 0.0, 1.0);
        }
        let profiles = self.water_profiles.len();
        match self.water_flow.exactly(self.cell_count() * 4) {
            Ok(bytes) => {
                if bytes.chunks_exact(4).any(|c| usize::from(c[3]) > profiles) {
                    errors.push(grid_error("waterFlow", "Unknown water profile".into()));
                }
            }
            Err(message) => errors.push(grid_error("waterFlow", message)),
        }
        let texture_cells = self.texture_layout().count;
        for (i, layer) in self.paint.iter().enumerate() {
            if let Some(grid) = &layer.variants
                && let Err(message) = grid.exactly(texture_cells)
            {
                errors.push(grid_error(&format!("paint.{i}.variants"), message));
            }
        }
        if !all_unique(self.water_profiles.iter().map(String::as_str)) {
            errors.push(grid_error(
                "waterProfiles",
                "Duplicate water profile".into(),
            ));
        }
        if !all_unique(self.paint.iter().map(|p| p.material.as_str())) {
            errors.push(grid_error("paint", "Duplicate ground material".into()));
        }
        errors
    }
}

fn grid_error(grid: &str, message: String) -> GridError {
    GridError {
        grid: grid.into(),
        message,
    }
}

fn all_unique<'a>(items: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    items.into_iter().all(|item| seen.insert(item))
}

fn check_floats(grid: &Grid, count: usize, min: f64, max: f64) -> Result<(), String> {
    let bytes = grid.exactly(count * 4)?;
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        let sample = f64::from(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        if !sample.is_finite() {
            return Err("Non-finite sample".into());
        }
        if sample < min || sample > max {
            return Err(format!("Sample {i} outside {min}..{max}"));
        }
    }
    Ok(())
}

/// `count` little-endian `f32`s.
pub fn decode_floats(grid: &Grid, count: usize) -> Result<Vec<f32>, String> {
    let bytes = grid.exactly(count * 4)?;
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

const INVALID: u8 = 0xff;

const fn alphabet() -> [u8; 256] {
    let mut table = [INVALID; 256];
    let symbols = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i = 0;
    while i < 64 {
        table[symbols[i] as usize] = i as u8;
        i += 1;
    }
    table
}

static ALPHABET: [u8; 256] = alphabet();

/// Base64 as the web platform's `atob` decodes it ("forgiving base64"): ASCII whitespace is
/// ignored, padding is optional, and unused trailing bits are discarded.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let is_space = |b: &u8| matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
    let filtered: Vec<u8>;
    let mut data = text.as_bytes();
    if data.iter().any(is_space) {
        filtered = data.iter().copied().filter(|b| !is_space(b)).collect();
        data = &filtered;
    }
    if data.len().is_multiple_of(4) {
        if let Some(rest) = data.strip_suffix(b"==") {
            data = rest;
        } else if let Some(rest) = data.strip_suffix(b"=") {
            data = rest;
        }
    }
    let invalid = || "The string to be decoded is not correctly encoded.".to_string();
    if data.len() % 4 == 1 {
        return Err(invalid());
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3 + 2);
    let quads = data.chunks_exact(4);
    let tail = quads.remainder();
    for quad in quads {
        let [a, b, c, d] = [
            ALPHABET[usize::from(quad[0])],
            ALPHABET[usize::from(quad[1])],
            ALPHABET[usize::from(quad[2])],
            ALPHABET[usize::from(quad[3])],
        ];
        if (a | b | c | d) & 0xc0 != 0 {
            return Err(invalid());
        }
        let word = u32::from(a) << 18 | u32::from(b) << 12 | u32::from(c) << 6 | u32::from(d);
        out.extend_from_slice(&[(word >> 16) as u8, (word >> 8) as u8, word as u8]);
    }
    let mut word = 0u32;
    for &symbol in tail {
        let value = ALPHABET[usize::from(symbol)];
        if value == INVALID {
            return Err(invalid());
        }
        word = word << 6 | u32::from(value);
    }
    match tail.len() {
        2 => out.push((word >> 4) as u8),
        3 => out.extend_from_slice(&[(word >> 10) as u8, (word >> 2) as u8]),
        _ => {}
    }
    Ok(out)
}
