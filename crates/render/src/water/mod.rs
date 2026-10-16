//! Water (renderer §4.6): a stylised surface redesigned for this renderer, with the old
//! game's data model and its unit wakes carried over.
//!
//! - **Data**: the terrain's per-vertex bed and water heights, a per-cell flow grid
//!   (direction, foam, profile index; the map's `waterFlow`) and a profile table: slot 0
//!   is the biome's water, slots 1–255 the map's own profiles, so painting a river never
//!   recolours other lakes (old `waterSurface`).
//! - **Surface**: one vertex per world unit over the cells the old game drew (a cell is
//!   wet when its centre or a corner lies within 0.15 of the water), in 32-cell chunks
//!   culled on the CPU. Swells move the vertices with a depth-scaled amplitude that never
//!   changes phase (`water.wgsl`).
//! - **Look**: body colour from the water column through the profile's shallow and deep
//!   colours and clarity, caustics, the old reflection cube and glints, shore, crest and
//!   flow foam, ripples from moving bodies ([`ripples`]).

pub mod gpu;
pub mod ripples;

use crate::terrain::{CHUNK, TerrainDesc};

/// `config/water.json`: the textures every water shares (old `sourceReflection` and
/// `SourceCaustics`), as asset ids.
pub const WATER_FILE: &str = "water.json";

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaterConfig {
    /// The woodland sky the water reflects: a data asset of RGBA8 sRGB cube faces
    /// (+X, −X, +Y, −Y, +Z, −Z) for `levels` mips of `size`² down.
    pub reflection: ReflectionConfig,
    /// Texture asset whose albedo drives the caustics on the bed.
    pub caustics: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReflectionConfig {
    pub asset: String,
    pub size: u32,
    pub levels: u32,
}

impl WaterConfig {
    /// Reads and checks `config/water.json` under the data folder.
    pub fn load(data: &std::path::Path) -> Result<Self, crate::environment::ConfigError> {
        let path = data
            .join(crate::environment::CONFIG_FOLDER)
            .join(WATER_FILE);
        let file = path.display().to_string();
        let error = |message: String| crate::environment::ConfigError {
            file: file.clone(),
            message,
        };
        let bytes = std::fs::read(&path).map_err(|e| error(e.to_string()))?;
        let config: Self = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        config.validate().map_err(error)?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        let r = &self.reflection;
        if !r.size.is_power_of_two()
            || r.size > 1024
            || r.levels == 0
            || (r.size >> (r.levels - 1)) == 0
        {
            return Err("reflection: size must be a power of two up to 1024 with at most log2(size) + 1 levels".into());
        }
        if r.asset.is_empty() || self.caustics.is_empty() {
            return Err("reflection and caustics need asset ids".into());
        }
        Ok(())
    }

    /// Pack entries of the textures.
    pub fn textures(&self) -> gpu::WaterTextures {
        gpu::WaterTextures {
            reflection: Some(gpu::ReflectionCube {
                entry: format!("{}/data.bin", self.reflection.asset),
                size: self.reflection.size,
                levels: self.reflection.levels,
            }),
            caustics: Some(format!("{}/albedo.png", self.caustics)),
        }
    }
}

/// Swells of a profile (old `WaterProfile.waves`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterWaves {
    pub height: f32,
    pub length: f32,
    pub speed: f32,
    pub depth_start: f32,
    pub depth_end: f32,
    pub shallow_strength: f32,
    pub shore_width: f32,
    pub crest_strength: f32,
}

/// Swells of profiles that declare none (old `DEFAULT_WATER_WAVES`).
pub const DEFAULT_WAVES: WaterWaves = WaterWaves {
    height: 0.32,
    length: 12.0,
    speed: 0.65,
    depth_start: 0.45,
    depth_end: 2.5,
    shallow_strength: 0.025,
    shore_width: 0.65,
    crest_strength: 0.22,
};

/// How a body of water looks (a published water profile).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterProfile {
    /// Linear colours.
    pub shallow: [f32; 3],
    pub deep: [f32; 3],
    pub clarity: f32,
    pub ripple_scale: f32,
    pub ripple_strength: f32,
    pub foam_strength: f32,
    pub reflection_strength: f32,
    pub caustic_strength: f32,
    pub cloud_strength: f32,
    pub flow_speed: f32,
    pub waves: WaterWaves,
}

/// Most profiles a map can use (slot 0 is the biome's).
pub const MAX_PROFILES: usize = 256;

/// The water of a world, beside its terrain (whose bed and water heights it uses).
#[derive(Debug, Clone, PartialEq)]
pub struct WaterDesc {
    /// Slot 0: the biome's water; slot `n`: the map's profile `n`.
    pub profiles: Vec<WaterProfile>,
    /// Per cell (`span²`, row index z): flow x and z (128 still), foam, profile slot.
    pub flow: Vec<[u8; 4]>,
}

impl WaterDesc {
    /// Still water of the biome's profile everywhere.
    pub fn still(profile: WaterProfile, span: u32) -> Self {
        Self {
            profiles: vec![profile],
            flow: vec![[128, 128, 0, 0]; (span * span) as usize],
        }
    }

    pub fn validate(&self, terrain: &TerrainDesc) -> Result<(), String> {
        if self.profiles.is_empty() || self.profiles.len() > MAX_PROFILES {
            return Err(format!(
                "water needs 1–{MAX_PROFILES} profiles, got {}",
                self.profiles.len()
            ));
        }
        if self.flow.len() != (terrain.span * terrain.span) as usize {
            return Err("the water flow grid does not match the terrain".into());
        }
        if let Some(cell) = self
            .flow
            .iter()
            .position(|f| usize::from(f[3]) >= self.profiles.len())
        {
            return Err(format!("water cell {cell} names a missing profile"));
        }
        for (i, p) in self.profiles.iter().enumerate() {
            let w = &p.waves;
            if !(p.clarity > 0.0 && w.length > 0.0 && w.depth_end > w.depth_start) {
                return Err(format!(
                    "water profile {i}: clarity and wave length must be positive and the wave depths ordered"
                ));
            }
        }
        Ok(())
    }
}

/// Six rows per profile, as `water.wgsl` reads them.
pub fn profile_rows(p: &WaterProfile) -> [[f32; 4]; 6] {
    let w = &p.waves;
    [
        [p.shallow[0], p.shallow[1], p.shallow[2], p.clarity],
        [p.deep[0], p.deep[1], p.deep[2], p.reflection_strength],
        [
            p.ripple_scale,
            p.ripple_strength,
            p.caustic_strength,
            p.foam_strength,
        ],
        [p.cloud_strength, p.flow_speed, 0.0, 0.0],
        [w.height, w.length, w.speed, w.depth_end],
        [
            w.depth_start,
            w.shallow_strength,
            w.shore_width,
            w.crest_strength,
        ],
    ]
}

/// Water within this height above the ground still counts as water (old 0.15).
pub const WET_MARGIN: f32 = 0.15;

/// The drawn cells, chunk by chunk: index ranges into one buffer of absolute vertex
/// indices (two triangles per wet cell), and each chunk's height range.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WaterMesh {
    pub indices: Vec<u32>,
    /// Per chunk (row-major, `chunks²`): first index and count.
    pub ranges: Vec<(u32, u32)>,
    /// Per chunk: lowest and highest water level of its wet cells.
    pub levels: Vec<[f32; 2]>,
    pub chunks: u32,
}

impl WaterMesh {
    /// Cells the old water drew: wet at the centre (bilinear ground and water) or at any
    /// corner.
    pub fn build(terrain: &TerrainDesc) -> Self {
        let span = terrain.span as usize;
        let w = span + 1;
        let chunks = terrain.span / CHUNK;
        let wet_vertex = |i: usize| terrain.heights[i] <= terrain.water[i] + WET_MARGIN;
        let mut mesh = Self {
            chunks,
            ..Default::default()
        };
        for cz in 0..chunks as usize {
            for cx in 0..chunks as usize {
                let start = mesh.indices.len() as u32;
                let mut range = [f32::MAX, f32::MIN];
                for z in cz * CHUNK as usize..(cz + 1) * CHUNK as usize {
                    for x in cx * CHUNK as usize..(cx + 1) * CHUNK as usize {
                        let a = z * w + x;
                        let corners = [a, a + 1, a + w, a + w + 1];
                        let centre_ground =
                            corners.iter().map(|&i| terrain.heights[i]).sum::<f32>() / 4.0;
                        let centre_water =
                            corners.iter().map(|&i| terrain.water[i]).sum::<f32>() / 4.0;
                        let wet = centre_ground <= centre_water + WET_MARGIN
                            || corners.iter().any(|&i| wet_vertex(i));
                        if !wet {
                            continue;
                        }
                        for &i in &corners {
                            range[0] = range[0].min(terrain.water[i]);
                            range[1] = range[1].max(terrain.water[i]);
                        }
                        let [a, b, c, d] = corners.map(|i| i as u32);
                        mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
                    }
                }
                mesh.ranges.push((start, mesh.indices.len() as u32 - start));
                mesh.levels.push(range);
            }
        }
        mesh
    }

    /// Wet cells drawn.
    pub fn cells(&self) -> usize {
        self.indices.len() / 6
    }

    /// Index ranges to draw for the chunks `visible` says are in view, merged where
    /// they are contiguous.
    pub fn draw_ranges(&self, mut visible: impl FnMut(usize) -> bool, out: &mut Vec<(u32, u32)>) {
        out.clear();
        for (c, &(start, count)) in self.ranges.iter().enumerate() {
            if count == 0 || !visible(c) {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.0 + last.1 == start => last.1 += count,
                _ => out.push((start, count)),
            }
        }
    }
}

/// Per-vertex water level and depth above the bed (negative where dry).
pub fn vertex_fields(terrain: &TerrainDesc) -> (Vec<f32>, Vec<f32>) {
    let depth = terrain
        .water
        .iter()
        .zip(&terrain.heights)
        .map(|(w, h)| w - h)
        .collect();
    (terrain.water.clone(), depth)
}

/// The 256² hash lattice of the water noise: R = hash(i), G = hash(i + 31.7) (old
/// `waterHash`, `fract(sin(dot(p, (127.1, 311.7))) · 43758.5453)`), as RG8.
pub fn hash_lattice() -> Vec<u8> {
    let hash = |x: f64, y: f64| ((x * 127.1 + y * 311.7).sin() * 43758.5453).rem_euclid(1.0);
    let mut out = Vec::with_capacity(256 * 256 * 2);
    for y in 0..256 {
        for x in 0..256 {
            let (x, y) = (f64::from(x), f64::from(y));
            out.push((hash(x, y) * 255.0).round() as u8);
            out.push((hash(x + 31.7, y + 31.7) * 255.0).round() as u8);
        }
    }
    out
}

/// Half-precision bits of `v` (round to nearest even; no denormal output below 2⁻¹⁴).
pub fn f16_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if exponent <= 0 {
        return sign;
    }
    if exponent >= 31 {
        return sign | 0x7c00;
    }
    let mut half = (exponent as u32) << 10 | (mantissa >> 13);
    let rest = mantissa & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && half & 1 == 1) {
        half += 1;
    }
    sign | half as u16
}

impl ripples::WaterField for TerrainDesc {
    /// Bilinear water surface (old `cellWaterAt`).
    fn water(&self, x: f32, z: f32) -> f32 {
        let fx = (x - self.origin).clamp(0.0, self.span as f32);
        let fz = (z - self.origin).clamp(0.0, self.span as f32);
        let (x0, z0) = (fx.floor(), fz.floor());
        let (tx, tz) = (fx - x0, fz - z0);
        let last = self.span as usize;
        let (x0, z0) = (x0 as usize, z0 as usize);
        let (x1, z1) = ((x0 + 1).min(last), (z0 + 1).min(last));
        let w = self.vertices() as usize;
        let at = |x: usize, z: usize| self.water[z * w + x];
        (at(x0, z0) * (1.0 - tx) + at(x1, z0) * tx) * (1.0 - tz)
            + (at(x0, z1) * (1.0 - tx) + at(x1, z1) * tx) * tz
    }

    /// The ground; the terrain knows no other surfaces.
    fn walk(&self, x: f32, z: f32, _surface: u32) -> f32 {
        self.sample(x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{LayerKind, TerrainLayer, TextureGrid};

    /// A 64-cell terrain: a lake (bed −1, water 0) in the west half, dry land at 1 in
    /// the east half.
    fn terrain() -> TerrainDesc {
        let span = 64u32;
        let w = (span + 1) as usize;
        let mut heights = vec![1.0; w * w];
        for z in 0..w {
            for x in 0..w {
                if x <= 20 {
                    heights[z * w + x] = -1.0;
                }
            }
        }
        TerrainDesc {
            span,
            origin: -16.0,
            water: vec![0.0; w * w],
            heights,
            layers: vec![TerrainLayer {
                kind: LayerKind::Base { tiling: 1.0 },
                albedo: String::new(),
                normal: String::new(),
                occlusion: None,
                tint: [1.0; 3],
                painted: false,
                mask: vec![255; w * w],
                cells: Vec::new(),
            }],
            grid: TextureGrid {
                start: [-16.5, -16.5],
                cell_size: 4.0,
                cells: [17, 17],
            },
            surroundings: None,
        }
    }

    fn profile() -> WaterProfile {
        WaterProfile {
            shallow: [0.1, 0.2, 0.2],
            deep: [0.0, 0.05, 0.1],
            clarity: 0.85,
            ripple_scale: 0.15,
            ripple_strength: 0.055,
            foam_strength: 0.02,
            reflection_strength: 0.5,
            caustic_strength: 0.18,
            cloud_strength: 0.03,
            flow_speed: 1.0,
            waves: DEFAULT_WAVES,
        }
    }

    #[test]
    fn the_mesh_covers_wet_cells_and_the_shore_row() {
        let t = terrain();
        let mesh = WaterMesh::build(&t);
        assert_eq!(mesh.chunks, 2);
        // Columns 0–19 are wet everywhere; column 20 has wet west corners (the shore
        // row); column 21 and beyond are dry.
        assert_eq!(mesh.cells(), 21 * 64);
        let w = t.vertices();
        assert!(mesh.indices.iter().all(|&i| i % w <= 21));
        // Only the western chunks have water.
        assert_eq!(mesh.ranges[1].1, 0);
        assert_eq!(mesh.ranges[3].1, 0);
        assert_eq!(mesh.levels[0], [0.0, 0.0]);
        // Contiguous visible chunks merge into one draw; hidden ones split it.
        let mut out = Vec::new();
        mesh.draw_ranges(|_| true, &mut out);
        assert_eq!(out, vec![(0, mesh.indices.len() as u32)]);
        mesh.draw_ranges(|c| c != 0, &mut out);
        assert_eq!(out, vec![mesh.ranges[2]]);
    }

    #[test]
    fn descriptions_are_checked_against_the_terrain() {
        let t = terrain();
        let mut water = WaterDesc::still(profile(), t.span);
        assert!(water.validate(&t).is_ok());
        water.flow[5][3] = 1;
        assert!(water.validate(&t).is_err());
        water.profiles.push(profile());
        assert!(water.validate(&t).is_ok());
        water.flow.pop();
        assert!(water.validate(&t).is_err());
        let mut bad = WaterDesc::still(profile(), t.span);
        bad.profiles[0].waves.depth_end = 0.1;
        assert!(bad.validate(&t).is_err());
    }

    #[test]
    fn the_water_config_is_checked() {
        let parse = |text: &str| serde_json::from_str::<WaterConfig>(text);
        let good = parse(
            r#"{ "reflection": { "asset": "r", "size": 64, "levels": 7 }, "caustics": "c" }"#,
        )
        .unwrap();
        assert!(good.validate().is_ok());
        assert_eq!(good.textures().reflection.unwrap().entry, "r/data.bin");
        let mut bad = good.clone();
        bad.reflection.levels = 8;
        assert!(bad.validate().is_err());
        bad.reflection = ReflectionConfig {
            asset: "r".into(),
            size: 48,
            levels: 1,
        };
        assert!(bad.validate().is_err());
        assert!(parse(r#"{ "reflection": { "asset": "r", "size": 64, "levels": 7 }, "caustics": "c", "x": 1 }"#).is_err());
    }

    #[test]
    fn half_floats_round_trip() {
        for (v, bits) in [
            (0.0f32, 0u16),
            (1.0, 0x3c00),
            (-2.0, 0xc000),
            (0.5, 0x3800),
            (65504.0, 0x7bff),
        ] {
            assert_eq!(f16_bits(v), bits, "{v}");
        }
        // 1 + 2⁻¹¹ is exactly between two halves and rounds to even.
        assert_eq!(f16_bits(1.0 + 2f32.powi(-11)), 0x3c00);
        assert_eq!(f16_bits(1.0 + 3.0 * 2f32.powi(-11)), 0x3c02);
    }

    #[test]
    fn the_terrain_answers_water_queries() {
        use ripples::WaterField;
        let t = terrain();
        assert_eq!(t.water(0.0, 0.0), 0.0);
        assert_eq!(t.walk(-10.0, 5.0, 0), -1.0);
        assert_eq!(t.walk(40.0, 5.0, 3), 1.0);
        let lattice = hash_lattice();
        assert_eq!(lattice.len(), 256 * 256 * 2);
        assert!(lattice.iter().any(|&v| v > 200) && lattice.iter().any(|&v| v < 50));
    }
}
