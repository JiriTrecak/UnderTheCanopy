//! Lighting environment: the data in `config/` (lighting and one file per biome) and the
//! values the shaders use, derived from it for an hour of the day.
//!
//! The configuration reproduces the old game's outdoor look (its `biomes.ts`,
//! `environment/*` and `sky.ts`; D30). Every number lives in the JSON files; this module
//! only knows how they combine:
//!
//! - **Daylight**: two palettes (day, night) per profile, held between the phases and
//!   blended with a smoothstep at dawn and dusk. A biome's light tints and scales them.
//! - **Sun direction**: the stylised clock orbit (azimuth −45° at noon, elevation
//!   50° + 10°·cos 2φ), turned by the biome's `sunDirection` and with its elevation scaled
//!   by `sunHeight / 60`.
//! - **Ambient**: a flat ambient term, a sky/ground hemisphere, the ground probe that
//!   terrain uses (the old reflection cube's last mip) and the environment irradiance as
//!   9 spherical-harmonic coefficients.
//! - **Grade**: the biome finish, baked into one colour lookup volume.

use glam::Vec3;
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

/// Folder of the configuration, relative to the data folder.
pub const CONFIG_FOLDER: &str = "config";
/// Global lighting file inside [`CONFIG_FOLDER`].
pub const LIGHTING_FILE: &str = "lighting.json";
/// Biome files: `base/landscape/biomes/<biome id>.json` (the base package's).
pub const BIOMES_FOLDER: &str = "base/landscape/biomes";

/// A configuration problem: file and message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}: {message}")]
pub struct ConfigError {
    pub file: String,
    pub message: String,
}

/// `#rrggbb`, sRGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hex(pub [u8; 3]);

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex(&text).ok_or_else(|| serde::de::Error::custom(format!("{text:?} is not #rrggbb")))
    }
}

fn parse_hex(text: &str) -> Option<Hex> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok();
    Some(Hex([byte(0)?, byte(1)?, byte(2)?]))
}

impl Hex {
    /// Linear RGB.
    pub fn linear(self) -> Vec3 {
        Vec3::from_array(self.0.map(|c| srgb_to_linear(f32::from(c) / 255.0)))
    }

    /// Display (sRGB-encoded) RGB in 0–1.
    pub fn display(self) -> Vec3 {
        Vec3::from_array(self.0.map(|c| f32::from(c) / 255.0))
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// --- config/lighting.json ------------------------------------------------------------

/// Settings shared by every biome.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LightingConfig {
    /// Day and night palettes per daylight profile.
    pub daylight: BTreeMap<String, Daylight>,
    pub phases: Phases,
    /// Hemisphere ground colour = biome `bounceTint` × this.
    pub bounce_strength: f32,
    /// Ambient irradiance terrain receives by normal direction (up, horizon, down).
    pub ground_probe: GroundProbe,
    pub environment: EnvironmentProbe,
    pub shadow: ShadowConfig,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Daylight {
    pub day: DaytimeLook,
    pub night: DaytimeLook,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaytimeLook {
    pub ambient: DaytimeColor,
    pub sky: DaytimeColor,
    pub sun: DaytimeColor,
}

/// A colour as 0–255 sRGB components and an intensity.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaytimeColor {
    pub rgb: [f32; 3],
    pub multiplier: f32,
}

impl DaytimeColor {
    fn linear(&self) -> Vec3 {
        Vec3::from_array(self.rgb.map(|c| srgb_to_linear(c / 255.0)))
    }
}

/// Hours where dawn and dusk blend the palettes.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Phases {
    pub dawn_start: f32,
    pub dawn_end: f32,
    pub dusk_start: f32,
    pub dusk_end: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundProbe {
    pub up: [f32; 3],
    pub side: [f32; 3],
    pub down: [f32; 3],
}

/// Environment irradiance as order-2 spherical harmonics of radiance (y up), and the
/// strength it is applied with.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentProbe {
    pub intensity: f32,
    pub irradiance: [[f32; 3]; 9],
}

/// The sun's shadow map: resolution, the height band around the focus the view is
/// fitted to, padding and the size step (world units).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowConfig {
    pub resolution: u32,
    pub below: f32,
    pub above: f32,
    pub padding: f32,
    pub step: f32,
}

// --- base/landscape/biomes/<id>.json ---------------------------------------------------------

/// Everything a biome looks like.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BiomeConfig {
    pub name: String,
    /// A profile of [`LightingConfig::daylight`].
    pub daylight: String,
    pub light: BiomeLight,
    pub canopy: Canopy,
    pub grade: Grade,
    /// Bloom of bright highlights (post).
    pub bloom: Bloom,
    /// Screen-space contact shading (post).
    pub contact: Contact,
    /// Volumetric haze and light shafts in canopy openings (post).
    pub atmosphere: Atmosphere,
    pub terrain: TerrainLayers,
    /// Catalogue water profile of wet cells without their own.
    pub water: String,
    pub surroundings: Surroundings,
    /// Grass coverage added around foliage (and trees) when the map is compiled.
    pub ground_cover: GroundCover,
    /// Ambient dust in the light and falling leaves (motes.rs); none by default.
    #[serde(default)]
    pub motes: Option<Motes>,
    /// How a map's blocks are dressed (D80; `canopy_scene::generate`). Maps without
    /// blocks, or biomes without this, look as authored.
    #[serde(default)]
    pub blocks: Option<BlockLook>,
}

/// What the generator makes of a map's blocks in this biome: ground paint, the generated
/// cliffs, decals and scattered scenery. Every id is data: terrain-material assets for
/// paint, pack texture entries for the cliff and decals, scenery ids for props.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlockLook {
    /// The generator's seed, mixed with each map's.
    #[serde(default)]
    pub seed: u32,
    /// Painted on ice blocks.
    pub ice: Option<String>,
    /// Painted on ramps and along cliff tops and feet.
    pub rubble: Option<String>,
    /// Closed ground's cover in the biome's grass layer (0 none, 1 full).
    #[serde(default = "one")]
    pub frame_cover: f32,
    /// Frame shade (0–1) deep in closed ground.
    #[serde(default)]
    pub frame_shade: f32,
    /// How high closed ground mounds up inside its edge (metres; the look only: nothing
    /// walks there).
    #[serde(default)]
    pub frame_rise: f32,
    pub cliff: Option<CliffLook>,
    /// Flat cut-outs pressed onto the ground.
    #[serde(default)]
    pub decals: Vec<DecalScatter>,
    /// Scenery scattered by zone.
    #[serde(default)]
    pub scatter: Vec<Scatter>,
}

/// The generated cliff faces and rubble.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CliffLook {
    /// Pack texture entries.
    pub albedo: String,
    pub normal: Option<String>,
    /// Metres one texture tile covers.
    pub tile: f32,
    pub roughness: f32,
    /// Size of the stone blocks the face is broken into (metres).
    pub stone: f32,
    /// How far stones stand out of the face (metres).
    pub relief: f32,
    /// Horizontal run of the face per metre of drop (0.85: about 50°).
    #[serde(default = "talus")]
    pub slope: f32,
    /// Rubble stones per metre of cliff at its foot, and on each ramp column.
    #[serde(default)]
    pub rubble: f32,
    #[serde(default)]
    pub ramp_rubble: u32,
    /// Frosted spikes in tufts out of closed ground, in the cliff's material.
    pub spikes: Option<Spikes>,
}

/// Tufts of spikes (frozen stalks, ice) in closed ground.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Spikes {
    /// Tufts tried per 100 m² (about half grow).
    pub density: f32,
    /// Height range (metres) and width at the base.
    pub height: [f32; 2],
    pub width: f32,
}

/// Where on the blocks something goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Zone {
    /// Closed ground, away from its edge.
    Frame,
    /// Closed ground near walkable ground.
    FrameEdge,
    /// Walkable ground near closed ground.
    GroundEdge,
    /// Open walkable ground.
    Ground,
    /// Ground at a cliff's top.
    CliffTop,
    /// Ground at a cliff's foot.
    CliffFoot,
    Ramp,
    Ice,
    /// Ice near its shore.
    IceEdge,
}

/// Decals of one atlas scattered over a zone.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecalScatter {
    pub zone: Zone,
    /// Pack texture entry of the atlas (RGBA, cells × cells cut-outs).
    pub atlas: String,
    pub cells: u32,
    /// How many of the atlas's cells are used (from the first).
    pub count: u32,
    /// Per 100 m².
    pub density: f32,
    /// Size range (metres).
    pub size: [f32; 2],
    /// Linear tint multiplier.
    #[serde(default = "no_tint")]
    pub tint: [f32; 3],
    /// 0–1: how much ice covers them (faded towards the ice).
    #[serde(default)]
    pub under: f32,
    /// 0–1: how much they gather in clumps instead of spreading evenly.
    #[serde(default)]
    pub clump: f32,
}

fn talus() -> f32 {
    0.85
}

fn no_tint() -> [f32; 3] {
    [1.0; 3]
}

/// Scenery scattered over a zone.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scatter {
    pub zone: Zone,
    /// Scenery ids, picked at random.
    pub pieces: Vec<String>,
    /// Per 100 m².
    pub density: f32,
    pub scale: [f32; 2],
    /// Metres up or down from the ground.
    #[serde(default)]
    pub elevation: f32,
    /// At most this many on a map.
    pub max: Option<u32>,
}

/// A biome's ambient motes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Motes {
    /// Dust motes glinting where light falls through the canopy (close cameras).
    pub dust: u32,
    /// Brightness of the dust in the light.
    pub dust_brightness: f32,
    /// Leaves drifting down around the view.
    pub leaves: u32,
    /// Pack texture of one falling leaf, alpha-masked (needed with leaves).
    pub leaf_texture: Option<String>,
    /// Leaf size in metres.
    pub leaf_size: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BiomeLight {
    pub sun_tint: Hex,
    pub sun_strength: f32,
    /// Degrees the clock orbit is turned about +y.
    pub sun_direction: f32,
    /// Elevation scale: the orbit's elevation × `sun_height / 60`.
    pub sun_height: f32,
    pub ambient_tint: Hex,
    pub ambient_strength: f32,
    pub sky_tint: Hex,
    pub bounce_tint: Hex,
    pub fill_strength: f32,
    /// Shadow filter radius in shadow-map texels.
    pub shadow_softness: f32,
    /// Rime on the scenery (0 none, 1 white): upward faces of props, trees and generated
    /// cliffs turn frosty; units and buildings never do (D80).
    #[serde(default)]
    pub frost: f32,
}

/// The dappled light of the overhead forest.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Canopy {
    pub height: f32,
    pub scale: f32,
    pub coverage: f32,
    pub softness: f32,
    pub strength: f32,
    pub sway: f32,
    pub speed: f32,
    pub cloud_shadow: f32,
    pub seed: u32,
}

/// The biome finish, in display (sRGB-encoded) space.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grade {
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// Knee of the hue-preserving highlight shoulder; 0 bypasses it (values clip at 1).
    pub highlight_shoulder: f32,
    pub shadow_tint: Hex,
    pub highlight_tint: Hex,
    pub split_strength: f32,
    pub shadow_lift: f32,
    pub vignette: f32,
    /// The pack's daytime colour grade volume (`…/data.json#day`) and its weight; 0
    /// leaves it out (every current biome).
    pub daytime_lut: Option<String>,
    pub daytime_lut_strength: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bloom {
    pub strength: f32,
    pub threshold: f32,
    pub knee: f32,
    pub radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub strength: f32,
    pub radius: f32,
    pub bias: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Atmosphere {
    pub enabled: bool,
    pub color: Hex,
    pub sun_tint: Option<Hex>,
    pub density: f32,
    pub shaft_density: f32,
    pub base_height: f32,
    pub height_falloff: f32,
    pub sun_strength: f32,
    pub noise_scale: f32,
    pub noise_strength: f32,
    pub drift_speed: f32,
    /// Share of the haze and shafts kept at RTS pitches (1 by default): a forest can keep
    /// its beams for close cameras, where they are the scene, and leave the army clear.
    #[serde(default = "one")]
    pub overhead_strength: f32,
    /// Elevation of the light shafts in degrees, when they fall from the canopy's gaps more
    /// steeply than the sun (light from above under a low sun); they then follow the gaps
    /// alone, not the sun's shadows. `None`: along the sun.
    #[serde(default)]
    pub shaft_elevation: Option<f32>,
    /// Drift of the mist noise (x, z); the old game's calm-weather wind by default.
    #[serde(default = "calm_wind")]
    pub wind: [f32; 2],
}

fn one() -> f32 {
    1.0
}

fn calm_wind() -> [f32; 2] {
    [0.4, 0.2]
}

/// The old game's warm shaft tint when a biome names none.
pub const DEFAULT_SHAFT_TINT: Hex = Hex([0xff, 0xe3, 0xb8]);

/// The six built-in ground layers. Which grid drives each is the renderer's rule (old
/// `authoredTerrain`): soil is the base, grass follows the grass coverage, dirt and stones
/// paint channels, waterbed the water depth and rock the rock coverage and slope.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainLayers {
    pub soil: GroundLayer,
    pub grass: GroundLayer,
    pub dirt: GroundLayer,
    pub waterbed: GroundLayer,
    pub stones: GroundLayer,
    pub rock: GroundLayer,
}

impl TerrainLayers {
    /// In layer order.
    pub fn ordered(&self) -> [&GroundLayer; 6] {
        [
            &self.soil,
            &self.grass,
            &self.dirt,
            &self.waterbed,
            &self.stones,
            &self.rock,
        ]
    }
}

fn white() -> Hex {
    Hex([255; 3])
}

/// A tiled ground texture pair and how it blends.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroundLayer {
    /// Terrain-material asset whose `data.bin` is albedo + roughness.
    pub albedo: String,
    /// Terrain-material asset whose `data.bin` is normal + height.
    pub normal: String,
    /// Texture repeats per 12.5 world units (UV = world · tiling · 0.08).
    pub tiling: f32,
    /// Height-blend sharpness; negative values blend by weight alone.
    pub blend: f32,
    /// Darkens the layer towards its fringe, 0–1.
    #[serde(default)]
    pub edge: f32,
    /// Noise that frays the coverage into patches, 0–1.
    #[serde(default)]
    pub breakup: f32,
    #[serde(default = "white")]
    pub tint: Hex,
}

/// Render-only ground and forest beyond the map's halo.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Surroundings {
    pub floor_texture: String,
    pub floor_color: Hex,
    pub haze_color: Hex,
    pub haze_start: f32,
    pub haze_end: f32,
    /// Giant trunks, limbs and leaves around the map (old `ForestSurroundings`).
    pub forest: Option<ForestConfig>,
}

/// The forest around the map: scenery ids of its models, the bark of the generated
/// limbs (a texture asset), the trunk rings and the overhead leaves.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForestConfig {
    pub trunk: String,
    pub mushroom: Option<String>,
    pub log: Option<String>,
    pub limb_texture: Option<String>,
    pub rings: Vec<TrunkRingConfig>,
    pub leaf_colors: Vec<Hex>,
    pub leaf_spacing: f32,
    pub leaf_length: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrunkRingConfig {
    /// World units beyond the map border.
    pub distance: f32,
    /// World units between trunks along the ring (at most 32 per side).
    pub spacing: f32,
    pub height: f32,
}

/// Old `groundCover`: foliage within `radius` (× its scale) adds grass coverage up to
/// `strength`; trees always reach [`TREE_COVER_RADIUS`].
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundCover {
    pub radius: f32,
    pub strength: f32,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let file = path.display().to_string();
    let error = |message: String| ConfigError {
        file: file.clone(),
        message,
    };
    let bytes = std::fs::read(path).map_err(|e| error(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))
}

impl LightingConfig {
    /// Reads and checks `config/lighting.json` under the data folder.
    pub fn load(data: &Path) -> Result<Self, ConfigError> {
        let path = data.join(CONFIG_FOLDER).join(LIGHTING_FILE);
        let config: Self = read(&path)?;
        config.validate().map_err(|message| ConfigError {
            file: path.display().to_string(),
            message,
        })?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        let p = &self.phases;
        if !(0.0 <= p.dawn_start
            && p.dawn_start < p.dawn_end
            && p.dawn_end < p.dusk_start
            && p.dusk_start < p.dusk_end
            && p.dusk_end <= 24.0)
        {
            return Err("phases must be increasing hours within 0–24".into());
        }
        for (name, set) in &self.daylight {
            for look in [&set.day, &set.night] {
                for color in [look.ambient, look.sky, look.sun] {
                    if color.rgb.iter().any(|c| !(0.0..=255.0).contains(c))
                        || !(0.0..=10.0).contains(&color.multiplier)
                    {
                        return Err(format!(
                            "daylight {name}: colours are 0–255, multipliers 0–10"
                        ));
                    }
                }
            }
        }
        let s = &self.shadow;
        if !s.resolution.is_power_of_two() || !(256..=4096).contains(&s.resolution) {
            return Err("shadow resolution must be a power of two in 256–4096".into());
        }
        if s.step <= 0.0 || s.padding < 0.0 || s.below < 0.0 || s.above < 0.0 {
            return Err("shadow fitting distances must be positive".into());
        }
        if !(0.0..=4.0).contains(&self.environment.intensity) {
            return Err("environment intensity must be 0–4".into());
        }
        Ok(())
    }
}

impl BiomeConfig {
    /// Reads and checks `base/landscape/biomes/<biome>.json` under the data folder.
    pub fn load(data: &Path, biome: &str, lighting: &LightingConfig) -> Result<Self, ConfigError> {
        let path = data.join(BIOMES_FOLDER).join(format!("{biome}.json"));
        let config: Self = read(&path)?;
        config.validate(lighting).map_err(|message| ConfigError {
            file: path.display().to_string(),
            message,
        })?;
        Ok(config)
    }

    /// The old editor's ranges, and references to the lighting configuration.
    pub fn validate(&self, lighting: &LightingConfig) -> Result<(), String> {
        let range = |name: &str, value: f32, min: f32, max: f32| {
            if value.is_finite() && (min..=max).contains(&value) {
                Ok(())
            } else {
                Err(format!("{name} must be within {min}–{max}, got {value}"))
            }
        };
        if !lighting.daylight.contains_key(&self.daylight) {
            return Err(format!("unknown daylight profile {}", self.daylight));
        }
        let l = &self.light;
        range("light.sunStrength", l.sun_strength, 0.0, 3.0)?;
        range("light.sunDirection", l.sun_direction, -180.0, 180.0)?;
        range("light.sunHeight", l.sun_height, 15.0, 85.0)?;
        range("light.ambientStrength", l.ambient_strength, 0.0, 3.0)?;
        range("light.fillStrength", l.fill_strength, 0.0, 3.0)?;
        range("light.shadowSoftness", l.shadow_softness, 0.0, 8.0)?;
        let c = &self.canopy;
        range("canopy.height", c.height, 12.0, 256.0)?;
        range("canopy.scale", c.scale, 16.0, 1024.0)?;
        range("canopy.softness", c.softness, 0.01, 0.45)?;
        range("canopy.strength", c.strength, 0.0, 1.0)?;
        range("canopy.coverage", c.coverage, 0.1, 0.95)?;
        range("canopy.sway", c.sway, 0.0, 3.0)?;
        range("canopy.speed", c.speed, 0.0, 2.0)?;
        range("canopy.cloudShadow", c.cloud_shadow, 0.0, 1.0)?;
        let g = &self.grade;
        range("grade.exposure", g.exposure, 0.25, 4.0)?;
        range("grade.contrast", g.contrast, 0.5, 2.0)?;
        range("grade.saturation", g.saturation, 0.0, 2.0)?;
        range("grade.highlightShoulder", g.highlight_shoulder, 0.0, 0.99)?;
        range("grade.splitStrength", g.split_strength, 0.0, 1.0)?;
        range("grade.shadowLift", g.shadow_lift, 0.0, 1.0)?;
        range("grade.vignette", g.vignette, 0.0, 1.0)?;
        range("grade.daytimeLutStrength", g.daytime_lut_strength, 0.0, 1.0)?;
        for (name, layer) in ["soil", "grass", "dirt", "waterbed", "stones", "rock"]
            .iter()
            .zip(self.terrain.ordered())
        {
            range(&format!("terrain.{name}.tiling"), layer.tiling, 0.001, 16.0)?;
            range(&format!("terrain.{name}.blend"), layer.blend, -4.0, 4.0)?;
            range(&format!("terrain.{name}.edge"), layer.edge, 0.0, 1.0)?;
            range(&format!("terrain.{name}.breakup"), layer.breakup, 0.0, 1.0)?;
            if layer.albedo.is_empty() || layer.normal.is_empty() {
                return Err(format!("terrain.{name} needs albedo and normal assets"));
            }
        }
        let s = &self.surroundings;
        if !(s.haze_start >= 0.0 && s.haze_end > s.haze_start) {
            return Err("surroundings haze must end after it starts".into());
        }
        if let Some(f) = &s.forest {
            if f.trunk.is_empty() || f.rings.is_empty() || f.rings.len() > 8 {
                return Err("surroundings.forest needs a trunk and 1–8 rings".into());
            }
            if f.leaf_colors.is_empty() {
                return Err("surroundings.forest needs leaf colours".into());
            }
            for (i, r) in f.rings.iter().enumerate() {
                range(
                    &format!("surroundings.forest.rings.{i}.distance"),
                    r.distance,
                    0.0,
                    1024.0,
                )?;
                range(
                    &format!("surroundings.forest.rings.{i}.spacing"),
                    r.spacing,
                    8.0,
                    1024.0,
                )?;
                range(
                    &format!("surroundings.forest.rings.{i}.height"),
                    r.height,
                    1.0,
                    512.0,
                )?;
            }
            range(
                "surroundings.forest.leafSpacing",
                f.leaf_spacing,
                1.0,
                256.0,
            )?;
            range("surroundings.forest.leafLength", f.leaf_length, 0.5, 128.0)?;
        }
        let b = &self.bloom;
        range("bloom.strength", b.strength, 0.0, 2.0)?;
        range("bloom.threshold", b.threshold, 0.0, 8.0)?;
        range("bloom.knee", b.knee, 0.0, 2.0)?;
        range("bloom.radius", b.radius, 0.0, 8.0)?;
        let c = &self.contact;
        range("contact.strength", c.strength, 0.0, 1.0)?;
        range("contact.radius", c.radius, 0.1, 8.0)?;
        range("contact.bias", c.bias, 0.0, 1.0)?;
        // The old editor's atmosphere schema.
        let a = &self.atmosphere;
        range("atmosphere.density", a.density, 0.0, 0.08)?;
        range("atmosphere.shaftDensity", a.shaft_density, 0.0, 0.01)?;
        range("atmosphere.baseHeight", a.base_height, -32.0, 64.0)?;
        range("atmosphere.heightFalloff", a.height_falloff, 0.5, 24.0)?;
        range("atmosphere.sunStrength", a.sun_strength, 0.0, 3.0)?;
        range("atmosphere.noiseScale", a.noise_scale, 0.01, 0.5)?;
        range("atmosphere.noiseStrength", a.noise_strength, 0.0, 1.0)?;
        range("atmosphere.driftSpeed", a.drift_speed, 0.0, 3.0)?;
        range("atmosphere.overheadStrength", a.overhead_strength, 0.0, 1.0)?;
        if let Some(e) = a.shaft_elevation {
            range("atmosphere.shaftElevation", e, 10.0, 90.0)?;
        }
        if let Some(m) = &self.motes {
            range("motes.dust", m.dust as f32, 0.0, 8192.0)?;
            range("motes.dustBrightness", m.dust_brightness, 0.0, 20.0)?;
            range("motes.leaves", m.leaves as f32, 0.0, 2048.0)?;
            range("motes.leafSize", m.leaf_size, 0.05, 10.0)?;
            if m.leaves > 0 && m.leaf_texture.is_none() {
                return Err("motes.leaves needs motes.leafTexture".into());
            }
        }
        range(
            "atmosphere.wind",
            a.wind[0].abs().max(a.wind[1].abs()),
            0.0,
            10.0,
        )?;
        range("groundCover.radius", self.ground_cover.radius, 0.0, 32.0)?;
        range("groundCover.strength", self.ground_cover.strength, 0.0, 1.0)?;
        Ok(())
    }
}

/// Reach of a tree's ground cover, world units × its scale (old `mapScene`).
pub const TREE_COVER_RADIUS: f32 = 3.0;

// --- Resolved values -----------------------------------------------------------------

/// What the shaders light with, for one biome at one hour.
#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    /// Unit vector towards the sun.
    pub sun_direction: Vec3,
    /// Linear irradiance of the sun (colour × intensity) before canopy clouds.
    pub sun: Vec3,
    /// The sun's colour and intensity separately (`sun = sun_color · sun_intensity`).
    pub sun_color: Vec3,
    pub sun_intensity: f32,
    /// The daylight palette's ambient irradiance, before the biome's ambient tint and
    /// strength (what the old water was lit with).
    pub daylight_ambient: Vec3,
    /// Flat ambient irradiance (`ambientLightColor`).
    pub ambient: Vec3,
    /// Hemisphere irradiance from above and below.
    pub sky: Vec3,
    pub ground: Vec3,
    /// Background colour behind the world (linear, may exceed 1).
    pub background: Vec3,
    /// Terrain's ambient probe (up, horizon, down).
    pub ground_probe: [Vec3; 3],
    /// Environment radiance SH, already scaled by its intensity.
    pub environment: [Vec3; 9],
    pub shadow_softness: f32,
    /// Rime on the scenery ([`BiomeLight::frost`]).
    pub frost: f32,
    pub canopy: Canopy,
    pub grade: Grade,
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Hours wrapped to 0–24.
pub fn wrap_hour(hour: f32) -> f32 {
    if hour.is_finite() {
        hour.rem_euclid(24.0)
    } else {
        0.0
    }
}

/// Weight of the night palette at `hour` (0 by day, 1 by night).
pub fn night_weight(phases: &Phases, hour: f32) -> f32 {
    let h = wrap_hour(hour);
    if h >= phases.dawn_end && h <= phases.dusk_start {
        0.0
    } else if h >= phases.dusk_end || h <= phases.dawn_start {
        1.0
    } else if h < phases.dawn_end {
        1.0 - smoothstep(
            0.0,
            1.0,
            (h - phases.dawn_start) / (phases.dawn_end - phases.dawn_start),
        )
    } else {
        smoothstep(
            0.0,
            1.0,
            (h - phases.dusk_start) / (phases.dusk_end - phases.dusk_start),
        )
    }
}

/// The clock orbit turned and scaled by the biome (old `Sky.apply` with
/// `daytimeDirection`): a unit vector towards the sun.
pub fn sun_direction(hour: f32, turn_degrees: f32, height: f32) -> Vec3 {
    let phase = (wrap_hour(hour) - 12.0) * std::f32::consts::PI / 12.0;
    let azimuth = -std::f32::consts::FRAC_PI_4 + phase;
    let elevation = (50.0 + 10.0 * (phase * 2.0).cos()).to_radians();
    let d = Vec3::new(
        elevation.cos() * azimuth.sin(),
        elevation.sin(),
        elevation.cos() * azimuth.cos(),
    );
    let d = glam::Quat::from_rotation_y(turn_degrees.to_radians()) * d;
    let azimuth = d.x.atan2(d.z);
    let elevation = (d.y.clamp(-1.0, 1.0).asin() * height / 60.0).min(1.48);
    Vec3::new(
        elevation.cos() * azimuth.sin(),
        elevation.sin(),
        elevation.cos() * azimuth.cos(),
    )
}

impl Lighting {
    /// The biome's light at `hour`.
    pub fn resolve(lighting: &LightingConfig, biome: &BiomeConfig, hour: f32) -> Self {
        let set = &lighting.daylight[&biome.daylight];
        let t = night_weight(&lighting.phases, hour);
        let mix = |a: &DaytimeColor, b: &DaytimeColor| {
            (
                a.linear().lerp(b.linear(), t),
                a.multiplier + (b.multiplier - a.multiplier) * t,
            )
        };
        let (sun_rgb, sun_m) = mix(&set.day.sun, &set.night.sun);
        let (ambient_rgb, ambient_m) = mix(&set.day.ambient, &set.night.ambient);
        let (sky_rgb, sky_m) = mix(&set.day.sky, &set.night.sky);
        let l = &biome.light;
        let hemisphere = sky_m * l.fill_strength;
        let probe = |c: [f32; 3]| Vec3::from_array(c);
        let env = &lighting.environment;
        Self {
            sun_direction: sun_direction(hour, l.sun_direction, l.sun_height),
            sun: sun_rgb * l.sun_tint.linear() * sun_m * l.sun_strength,
            sun_color: sun_rgb * l.sun_tint.linear(),
            sun_intensity: sun_m * l.sun_strength,
            daylight_ambient: ambient_rgb * ambient_m,
            ambient: ambient_rgb * l.ambient_tint.linear() * ambient_m * l.ambient_strength,
            sky: sky_rgb * l.sky_tint.linear() * hemisphere,
            ground: l.bounce_tint.linear() * lighting.bounce_strength * hemisphere,
            background: sky_rgb * sky_m,
            ground_probe: [
                probe(lighting.ground_probe.up),
                probe(lighting.ground_probe.side),
                probe(lighting.ground_probe.down),
            ],
            environment: env.irradiance.map(|c| Vec3::from_array(c) * env.intensity),
            shadow_softness: l.shadow_softness,
            frost: l.frost,
            canopy: biome.canopy,
            grade: biome.grade.clone(),
        }
    }
}

/// The canopy pattern's drift and the sun's transmission through moving cover at
/// `seconds` (old `canopyLayer`).
pub fn canopy_motion(canopy: &Canopy, seconds: f32) -> ([f32; 2], f32) {
    let t = seconds * canopy.speed;
    let a = canopy.sway / canopy.scale;
    let offset = [
        (0.31 * t).sin() * a + (0.073 * t).sin() * a * 0.5,
        (0.23 * t).cos() * a,
    ];
    let cloud =
        0.5 + 0.3 * (0.075 * t + canopy.seed as f32 * 0.013).sin() + 0.2 * (0.031 * t + 1.7).sin();
    (offset, (1.0 - canopy.cloud_shadow * cloud).clamp(0.0, 1.0))
}

// --- Canopy pattern ------------------------------------------------------------------

/// The canopy's shade pattern (old `canopyLayer`): a tileable `size`² R8 mask, 1 where the
/// crowns shade the ground. Coverage picks the threshold from the sorted noise so the
/// shaded fraction is exact.
pub fn canopy_pattern(canopy: &Canopy, size: u32) -> Vec<u8> {
    let n = (size * size) as usize;
    let mut values = Vec::with_capacity(n);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 / size as f32, y as f32 / size as f32);
            values.push(
                canopy_field(u * 4.0, v * 4.0, 4, canopy.seed) * 0.82
                    + canopy_field(u * 12.0, v * 12.0, 12, canopy.seed.wrapping_add(31)) * 0.18,
            );
        }
    }
    let mut sorted = values.clone();
    sorted.sort_by(f32::total_cmp);
    let threshold = sorted[((1.0 - canopy.coverage) * (n - 1) as f32) as usize];
    values
        .iter()
        .map(|&value| {
            let t = (0.5 + (value - threshold) / canopy.softness).clamp(0.0, 1.0);
            (t * t * (3.0 - 2.0 * t) * 255.0).round() as u8
        })
        .collect()
}

/// Periodic value noise of the canopy (old `canopyLayer.field`).
fn canopy_field(u: f32, v: f32, period: i64, seed: u32) -> f32 {
    let hash = |ix: i64, iy: i64| -> f32 {
        let x = ix.rem_euclid(period) as u32;
        let y = iy.rem_euclid(period) as u32;
        let mut n = x.wrapping_add(seed).wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
        n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
        (n ^ (n >> 16)) as f32 / u32::MAX as f32
    };
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i64, y0 as i64);
    let a = hash(ix, iy);
    let b = hash(ix + 1, iy);
    let c = hash(ix, iy + 1);
    let d = hash(ix + 1, iy + 1);
    (a + (b - a) * sx) + ((c + (d - c) * sx) - (a + (b - a) * sx)) * sy
}

/// Where sunlight scatters within the canopy's openings (old `canopyMask` green): the
/// unshaded part of [`canopy_pattern`] feathered by a finer noise, so shafts stay sparse
/// even in a large clearing. R8, `size`².
pub fn canopy_openings(canopy: &Canopy, size: u32) -> Vec<u8> {
    let shade = canopy_pattern(canopy, size);
    shade
        .iter()
        .enumerate()
        .map(|(i, &alpha)| {
            let (u, v) = (
                (i as u32 % size) as f32 / size as f32,
                (i as u32 / size) as f32 / size as f32,
            );
            let gap = ((canopy_field(u * 16.0, v * 16.0, 16, canopy.seed.wrapping_add(91)) - 0.58)
                / 0.2)
                .clamp(0.0, 1.0);
            (f32::from(255 - alpha) * gap * gap * (3.0 - 2.0 * gap)).round() as u8
        })
        .collect()
}

// --- Colour grade --------------------------------------------------------------------

/// Edge length of the baked grade volume.
pub const GRADE_SIZE: u32 = 32;

/// The biome finish (old `gradeBiome`, without the vignette, which depends on the screen
/// position) applied to a display-space colour.
pub fn grade_color(g: &Grade, c: Vec3) -> Vec3 {
    let luminance = |c: Vec3| c.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    let mut c = c;
    let peak = c.max_element();
    let w = if g.exposure > 1.0 {
        1.0 - smoothstep(0.45, 0.9, peak)
    } else {
        1.0
    };
    c *= 1.0 + (g.exposure - 1.0) * w;
    let l = luminance(c);
    let shadow = g.shadow_tint.display();
    let mut tint = shadow.lerp(g.highlight_tint.display(), smoothstep(0.2, 0.85, l));
    tint /= luminance(tint).max(0.01);
    c *= Vec3::ONE.lerp(tint, g.split_strength);
    let sw = (1.0 - smoothstep(0.04, 0.72, l)).powi(2) * smoothstep(0.01, 0.16, l);
    c = c.lerp(c.max(shadow), g.shadow_lift * sw);
    let chroma = c.max_element() - c.min_element();
    let sat = if g.saturation > 1.0 {
        1.0 + (g.saturation - 1.0) * (1.0 - smoothstep(0.15, 0.65, chroma))
    } else {
        g.saturation
    };
    c = Vec3::splat(luminance(c)).lerp(c, sat);
    c = 0.18 * (c.max(Vec3::ZERO) / 0.18).powf(g.contrast);
    let peak = c.max_element();
    if g.highlight_shoulder > 0.0 && peak > 0.9 {
        c *= (0.9 + 0.1 * (1.0 - (-(peak - 0.9) / 0.1).exp())) / peak;
    }
    c
}

/// A daytime lookup volume from the pack and the weight it is mixed in with.
pub struct DaytimeLut<'a> {
    pub size: u32,
    /// RGBA8, red fastest.
    pub rgba: &'a [u8],
    pub strength: f32,
}

impl DaytimeLut<'_> {
    /// Trilinear lookup of a display colour (no half-texel remap, as the old composite).
    fn sample(&self, c: Vec3) -> Vec3 {
        let n = self.size as usize;
        let max = (n - 1) as f32;
        let p = (c.clamp(Vec3::ZERO, Vec3::ONE) * max).to_array();
        let i0 = p.map(|v| (v.floor() as usize).min(n - 1));
        let i1 = i0.map(|v| (v + 1).min(n - 1));
        let f = [
            p[0] - i0[0] as f32,
            p[1] - i0[1] as f32,
            p[2] - i0[2] as f32,
        ];
        let at = |x: usize, y: usize, z: usize| {
            let o = ((z * n + y) * n + x) * 4;
            Vec3::new(
                f32::from(self.rgba[o]),
                f32::from(self.rgba[o + 1]),
                f32::from(self.rgba[o + 2]),
            ) / 255.0
        };
        let mut out = Vec3::ZERO;
        for corner in 0..8 {
            let (bx, by, bz) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let w = [f[0], f[1], f[2]]
                .iter()
                .zip([bx, by, bz])
                .map(|(&t, b)| if b == 1 { t } else { 1.0 - t })
                .product::<f32>();
            let pick = |b: usize, a: [usize; 3], c: [usize; 3], axis: usize| {
                if b == 1 { c[axis] } else { a[axis] }
            };
            out += at(
                pick(bx, i0, i1, 0),
                pick(by, i0, i1, 1),
                pick(bz, i0, i1, 2),
            ) * w;
        }
        out
    }
}

/// Bakes the display-space grade into a [`GRADE_SIZE`]³ RGBA8 volume (red fastest):
/// the optional daytime lookup, then the biome finish.
pub fn bake_grade(grade: &Grade, daytime: Option<&DaytimeLut<'_>>) -> Vec<u8> {
    let n = GRADE_SIZE;
    let mut out = Vec::with_capacity((n * n * n * 4) as usize);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let mut c = Vec3::new(r as f32, g as f32, b as f32) / (n - 1) as f32;
                if let Some(lut) = daytime.filter(|l| l.strength > 0.0) {
                    c = c.lerp(lut.sample(c), lut.strength);
                }
                let c = grade_color(grade, c).clamp(Vec3::ZERO, Vec3::ONE);
                out.extend(c.to_array().map(|v| (v * 255.0).round() as u8));
                out.push(255);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a - b).abs().max_element() <= eps
    }

    fn lighting() -> LightingConfig {
        let day = DaytimeLook {
            ambient: DaytimeColor {
                rgb: [245.0, 248.0, 245.0],
                multiplier: 3.0,
            },
            sky: DaytimeColor {
                rgb: [222.0, 233.0, 249.0],
                multiplier: 1.5,
            },
            sun: DaytimeColor {
                rgb: [255.0; 3],
                multiplier: 3.2,
            },
        };
        let night = DaytimeLook {
            sun: DaytimeColor {
                rgb: [213.0, 225.0, 250.0],
                multiplier: 2.2,
            },
            ..day
        };
        LightingConfig {
            daylight: BTreeMap::from([("test".to_owned(), Daylight { day, night })]),
            phases: Phases {
                dawn_start: 5.8,
                dawn_end: 6.2,
                dusk_start: 17.8,
                dusk_end: 18.2,
            },
            bounce_strength: 0.15,
            ground_probe: GroundProbe {
                up: [1.0; 3],
                side: [0.5; 3],
                down: [0.25; 3],
            },
            environment: EnvironmentProbe {
                intensity: 0.5,
                irradiance: [[1.0, 0.0, 0.0]; 9],
            },
            shadow: ShadowConfig {
                resolution: 2048,
                below: 6.0,
                above: 12.0,
                padding: 2.0,
                step: 4.0,
            },
        }
    }

    fn layer() -> GroundLayer {
        GroundLayer {
            albedo: "a".into(),
            normal: "n".into(),
            tiling: 1.0,
            blend: 0.5,
            edge: 0.0,
            breakup: 0.0,
            tint: white(),
        }
    }

    fn biome() -> BiomeConfig {
        BiomeConfig {
            name: "Test".into(),
            daylight: "test".into(),
            light: BiomeLight {
                sun_tint: Hex([255; 3]),
                sun_strength: 1.0,
                sun_direction: -100.0,
                sun_height: 45.0,
                ambient_tint: Hex([255; 3]),
                ambient_strength: 0.3,
                sky_tint: Hex([0xe0, 0xee, 0xff]),
                bounce_tint: Hex([255; 3]),
                fill_strength: 0.8,
                shadow_softness: 3.0,
                frost: 0.0,
            },
            canopy: Canopy {
                height: 140.0,
                scale: 240.0,
                coverage: 0.55,
                softness: 0.22,
                strength: 0.14,
                sway: 1.2,
                speed: 0.35,
                cloud_shadow: 0.0,
                seed: 731,
            },
            grade: Grade {
                exposure: 1.0,
                contrast: 1.0,
                saturation: 1.0,
                highlight_shoulder: 0.0,
                shadow_tint: white(),
                highlight_tint: white(),
                split_strength: 0.0,
                shadow_lift: 0.0,
                vignette: 0.0,
                daytime_lut: None,
                daytime_lut_strength: 0.0,
            },
            bloom: Bloom {
                strength: 0.0,
                threshold: 1.6,
                knee: 0.15,
                radius: 1.5,
            },
            contact: Contact {
                strength: 0.4,
                radius: 1.2,
                bias: 0.07,
            },
            atmosphere: Atmosphere {
                enabled: false,
                color: white(),
                sun_tint: None,
                density: 0.0,
                shaft_density: 0.0,
                base_height: 1.0,
                height_falloff: 4.0,
                sun_strength: 1.0,
                noise_scale: 0.09,
                noise_strength: 0.65,
                drift_speed: 0.35,
                overhead_strength: 1.0,
                shaft_elevation: None,
                wind: calm_wind(),
            },
            terrain: TerrainLayers {
                soil: layer(),
                grass: layer(),
                dirt: layer(),
                waterbed: layer(),
                stones: layer(),
                rock: layer(),
            },
            water: "w".into(),
            surroundings: Surroundings {
                floor_texture: "f".into(),
                floor_color: white(),
                haze_color: white(),
                haze_start: 170.0,
                haze_end: 850.0,
                forest: None,
            },
            ground_cover: GroundCover {
                radius: 1.3,
                strength: 0.8,
            },
            motes: None,
            blocks: None,
        }
    }

    #[test]
    fn noon_light_matches_the_old_numbers() {
        let lit = Lighting::resolve(&lighting(), &biome(), 12.0);
        // Old game, vibrant forest at noon: azimuth −145°, elevation 45°.
        assert!(close(
            lit.sun_direction,
            Vec3::new(-0.40558, std::f32::consts::FRAC_1_SQRT_2, -0.57923),
            1e-4
        ));
        assert!(close(lit.sun, Vec3::splat(3.2), 1e-5));
        assert!(close(lit.ambient, Vec3::new(0.8218, 0.8448, 0.8218), 1e-3));
        assert!(close(lit.sky, Vec3::new(0.6534, 0.8360, 1.1368), 1e-3));
        assert!(close(lit.ground, Vec3::splat(0.18), 1e-5));
        assert!(close(lit.environment[0], Vec3::new(0.5, 0.0, 0.0), 1e-6));
    }

    #[test]
    fn dawn_and_dusk_blend_the_palettes() {
        let phases = lighting().phases;
        assert_eq!(night_weight(&phases, 12.0), 0.0);
        assert_eq!(night_weight(&phases, 2.0), 1.0);
        assert_eq!(night_weight(&phases, 26.0), 1.0);
        assert!((night_weight(&phases, 6.0) - 0.5).abs() < 1e-5);
        assert!((night_weight(&phases, 18.0) - 0.5).abs() < 1e-5);
        let dusk = Lighting::resolve(&lighting(), &biome(), 18.0);
        assert!(dusk.sun.x < 3.2 && dusk.sun.x > 2.2 * 0.6);
        // The default orbit: elevation 60° at noon, 40° at six.
        let noon = sun_direction(12.0, 0.0, 60.0);
        assert!((noon.y - 60f32.to_radians().sin()).abs() < 1e-5);
        assert!((sun_direction(6.0, 0.0, 60.0).y - 40f32.to_radians().sin()).abs() < 1e-5);
    }

    #[test]
    fn validation_follows_the_old_ranges() {
        let lighting = lighting();
        assert_eq!(biome().validate(&lighting), Ok(()));
        let mut bad = biome();
        bad.light.sun_height = 5.0;
        assert!(bad.validate(&lighting).unwrap_err().contains("sunHeight"));
        let mut bad = biome();
        bad.daylight = "missing".into();
        assert!(bad.validate(&lighting).is_err());
        let mut bad = biome();
        bad.canopy.coverage = 1.0;
        assert!(bad.validate(&lighting).is_err());
        let mut bad_lighting = lighting.clone();
        bad_lighting.shadow.resolution = 1000;
        assert!(bad_lighting.validate().is_err());
        assert!(parse_hex("#12345g").is_none());
        assert_eq!(parse_hex("#ff8000"), Some(Hex([255, 128, 0])));
        let error = serde_json::from_str::<BiomeLight>(r##"{"sunTint":"#fff"}"##);
        assert!(error.is_err());
    }

    #[test]
    fn a_neutral_grade_is_the_identity() {
        let grade = biome().grade;
        let volume = bake_grade(&grade, None);
        assert_eq!(volume.len(), (GRADE_SIZE.pow(3) * 4) as usize);
        for (i, texel) in volume.chunks_exact(4).enumerate() {
            let i = i as u32;
            let expect =
                [i % 32, (i / 32) % 32, i / 1024].map(|v| (v as f32 / 31.0 * 255.0).round() as u8);
            assert_eq!(&texel[..3], &expect);
        }
    }

    #[test]
    fn grading_moves_colours_as_configured() {
        let mut grade = biome().grade;
        grade.saturation = 0.0;
        let grey = grade_color(&grade, Vec3::new(1.0, 0.0, 0.0));
        assert!((grey.x - grey.y).abs() < 1e-6 && (grey.y - grey.z).abs() < 1e-6);
        let mut grade = biome().grade;
        grade.shadow_tint = Hex([0x40, 0x40, 0xff]);
        grade.split_strength = 1.0;
        let tinted = grade_color(&grade, Vec3::splat(0.1));
        assert!(tinted.z > tinted.x);
        // A daytime volume at full strength replaces the input.
        let size = 2;
        let inverted: Vec<u8> = (0..8u32)
            .flat_map(|i| {
                [i & 1, (i >> 1) & 1, (i >> 2) & 1]
                    .map(|b| if b == 1 { 0 } else { 255 })
                    .into_iter()
                    .chain([255])
            })
            .map(|v| v as u8)
            .collect();
        let lut = DaytimeLut {
            size,
            rgba: &inverted,
            strength: 1.0,
        };
        let volume = bake_grade(&biome().grade, Some(&lut));
        assert_eq!(&volume[..3], &[255, 255, 255]);
    }

    #[test]
    fn the_canopy_pattern_covers_the_configured_fraction() {
        let canopy = biome().canopy;
        let pattern = canopy_pattern(&canopy, 64);
        let shaded = pattern.iter().filter(|&&v| v >= 128).count() as f32 / pattern.len() as f32;
        assert!((shaded - canopy.coverage).abs() < 0.05, "{shaded}");
        let (offset, transmission) = canopy_motion(&canopy, 10.0);
        assert!(offset[0].abs() < canopy.sway / canopy.scale * 1.5);
        assert_eq!(transmission, 1.0);
    }
}
