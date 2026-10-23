//! Authored maps: map packages (`<id>.utcm`, [`package`]) in the base package's `maps/`.
//!
//! In memory a map is one [`UtcMap`]: the old single-document format (version 3), which
//! tests and tools also build as JSON ([`UtcMap::parse`], grids as base64). Reading checks
//! exactly what the old game's `readUtcMap` checked and returns the same normalised value;
//! [`validate`] applies the map rules that need the content. Rules that need navigation
//! geometry or the terrain projection belong to the simulation.
//!
//! Terrain grids hold raw samples ([`grid::Grid`]); [`TerrainData::decode`] and the
//! per-grid helpers turn them into typed buffers.

mod authoring;
pub mod blocks;
mod camera;
mod check;
pub mod grid;
mod library;
mod mission;
pub mod package;
pub mod players;
mod stamp;
mod validate;

pub use authoring::{
    AuthoredObject, DRY_WATER_HEIGHT, DecodedPaint, DecodedTerrain, GridError, HEIGHT_MAX,
    HEIGHT_MIN, HeightMode, PaintLayer, SavedScene, TerrainData, TextureGrid, TextureLayout,
    WaterFlow, decode_base64, decode_floats,
};
pub use camera::{
    CAMERA_DISTANCE, CAMERA_FOV, CAMERA_HEIGHT, CAMERA_PITCH, CAMERA_ROLL, MapCamera,
};
pub use library::{MAP_GROUPS, MAPS_FOLDER, MapFile, folder, library};
pub use mission::{Mission, Objective, Region};
pub use players::{
    Behaviour, COLOR_NAMES, Grow, MissionPlayer, PlayerAi, PlayerColor, SkirmishLimits, Team,
    WaveLine, WaveSource, WaveTarget, WaveUnit,
};
pub use stamp::{SourceTransform, Stamp, StampVariant, Walk, WalkConnections, WalkMesh};
pub use validate::validate;

use crate::{
    error::{Error, Errors},
    placement::{Camp, Placement},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// The `.utcmap` format version this reader accepts.
pub const UTCMAP_VERSION: u32 = 3;
/// Name of a map whose document has no (or a blank) name.
pub const DEFAULT_MAP_NAME: &str = "Untitled";
/// The smallest and largest cells per side a map may have.
pub const MAP_SIZE_RANGE: std::ops::RangeInclusive<u32> = 128..=1024;

/// Whether a map may have `size` cells per side: a multiple of the renderer's 32-cell
/// terrain chunks (so of the 4-cell blocks too), 128 to 1024.
pub fn valid_size(size: u32) -> bool {
    size.is_multiple_of(32) && MAP_SIZE_RANGE.contains(&size)
}
/// Visible, non-playable border around the playable cells, in cells per side.
pub const MAP_HALO: u32 = 16;
/// Biome ids: the biomes whose recipe generates a map's look from its blocks (D80, D82).
/// A map naming any other biome is rejected.
pub const BIOMES: [&str; 2] = ["frozen-dream", "underleaf"];

/// A number that must equal `N` (format and section versions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Version<const N: u32>;

impl<const N: u32> Serialize for Version<N> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(N)
    }
}

impl<'de, const N: u32> Deserialize<'de> for Version<N> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.as_f64() == Some(f64::from(N)) {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected {N}, got {value}"
            )))
        }
    }
}

/// A value the format still accepts in any form but no longer reads; dropped on load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Retired;

impl<'de> Deserialize<'de> for Retired {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer).map(|_| Self)
    }
}

/// Terrain knowledge at match start; never grants current sight of enemies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Exploration {
    Explored,
    Hidden,
}

/// Where a player's fort stands. `x`/`z` are navigation cells on half-cell steps.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerStart {
    /// 1 to 8.
    pub player: u8,
    pub x: f64,
    pub z: f64,
    /// Degrees: 0, 90, 180 or 270.
    pub rotation: Option<u32>,
    /// Starting setup id (`rules.startingSetup` or a race's setup).
    pub setup: String,
    /// Placement id of the fort whose loss defeats the player.
    pub main_fort: String,
}

impl PlayerStart {
    /// `player.N`, the owner of this start's placements.
    pub fn owner(&self) -> crate::placement::Owner {
        crate::placement::Owner::Player(self.player.saturating_sub(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WeatherKind {
    Clear,
    Rain,
    Snow,
    Spores,
    /// Sunlight in shafts through the air.
    GodRays,
    /// An enchanted evening: a soft violet-blue cast and fireflies drifting low over the
    /// ground (D92).
    EnchantedForest,
}

/// Unknown keys are dropped, as the old parser's non-strict schema does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WeatherConditions {
    pub kind: WeatherKind,
}

/// The only environment values a map owns; its look comes from the biome. Unknown keys are
/// dropped, as the old parser's non-strict schema does.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentConditions {
    /// 0 to 24.
    pub hour: f64,
    pub playing: bool,
    pub weather: Option<WeatherConditions>,
    /// Minutes of match time a whole day takes while time passes ([`DAY_MINUTES`] when
    /// absent): a mission that turns slowly from dusk to night sets a long day.
    pub day_minutes: Option<f64>,
}

/// Minutes of match time per day when a map does not say (the old game's ten).
pub const DAY_MINUTES: f64 = 10.0;
/// The longest day a map may set: a whole day lasts at most a full real day.
pub const MAX_DAY_MINUTES: f64 = 1440.0;

impl EnvironmentConditions {
    /// Minutes of match time per day.
    pub fn day_minutes(&self) -> f64 {
        self.day_minutes.unwrap_or(DAY_MINUTES)
    }
}

/// A map's own frame dressing over its biome's (`blocks.scatter`, D80): inside its area
/// (the whole map when it names none), the biome's scenery can be taken out and the
/// map's own layers added, so a map can wither one corner of a lush biome.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapScatter {
    /// A circle of cells.
    pub area: Option<ScatterArea>,
    /// A mission region's circle (`mission.regions`) instead of an area.
    pub region: Option<String>,
    /// Metres over which the change blends in at the edge, along a wandering line (8 when
    /// absent; 0 a hard edge).
    pub fade: Option<f64>,
    /// The biome's scenery taken out inside: `true` all of it, or the scenery ids listed.
    pub remove: Option<ScatterRemoval>,
    /// Scatter layers in the biome recipe's own format (`blocks.scatter`), placed only
    /// inside.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Value>,
}

/// A circle of cells (as mission regions are).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScatterArea {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
}

/// Which of the biome's scenery a [`MapScatter`] takes out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ScatterRemoval {
    /// `true`: every piece; `false`: none.
    All(bool),
    /// These scenery ids.
    Pieces(Vec<String>),
}

impl ScatterRemoval {
    /// Whether the biome's `piece` is taken out.
    pub fn removes(&self, piece: &str) -> bool {
        match self {
            Self::All(all) => *all,
            Self::Pieces(pieces) => pieces.iter().any(|p| p == piece),
        }
    }
}

/// Most entries in `landscape.scatter`, and most layers in one.
pub const MAX_MAP_SCATTER: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DecalKind {
    LeafLitter,
    TinyFlowers,
    Pebbles,
    RootRot,
    MyceliumBed,
}

/// A painted ground detail; world units, rotation in degrees. Unknown keys are accepted and
/// dropped (the old parser passes them through, but nothing reads them).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Decal {
    pub locked: Option<bool>,
    pub id: String,
    pub kind: DecalKind,
    pub x: f64,
    pub z: f64,
    pub size: f64,
    pub rotation: f64,
    pub opacity: f64,
}

/// Saved landscape appearance. Generator inputs (`strokes`, `cover`, rivers, imported
/// terrain) never belong in a saved map.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Landscape {
    pub environment: EnvironmentConditions,
    pub decals: Option<Vec<Decal>>,
    /// The map's own frame dressing over its biome's, in order.
    pub scatter: Option<Vec<MapScatter>>,
    /// Retired per-map water appearance: accepted in any form, always `None` after load.
    #[serde(default, skip_serializing)]
    pub water: Option<Retired>,
}

fn default_name() -> String {
    DEFAULT_MAP_NAME.into()
}

fn is_false(value: &bool) -> bool {
    !value
}

/// A map, normalised as the old parser returns it.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UtcMap {
    pub v: Version<UTCMAP_VERSION>,
    /// Cells per side ([`valid_size`]).
    pub size: u32,
    /// Trimmed; [`DEFAULT_MAP_NAME`] when absent or blank.
    #[serde(default = "default_name")]
    pub name: String,
    /// One of [`BIOMES`]; see [`UtcMap::biome`].
    pub biome: Option<String>,
    /// Trimmed; `None` when blank.
    pub description: Option<String>,
    /// Music events, shuffled, played in a match on the map over the player's race's
    /// (D97).
    pub music: Option<Vec<crate::Id>>,
    pub mission: Option<Mission>,
    /// Local, fully revealed testbed: authored entities only, no opponents or victory rules.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sandbox: bool,
    pub initial_exploration: Option<Exploration>,
    /// Placed catalogue assets.
    #[serde(default)]
    pub stamps: Vec<Stamp>,
    pub player_starts: Vec<PlayerStart>,
    pub entities: Vec<Placement>,
    pub camps: Vec<Camp>,
    pub landscape: Option<Landscape>,
    /// The part of the map play happens in, when it is not the whole map: the cells
    /// around it are scenery (Warcraft III's map margins), seen but never entered.
    pub playable: Option<Playable>,
    /// Committed terrain and placed objects.
    pub authoring: SavedScene,
}

/// A rectangle of cells: `x`, `z` its first cell, `width` and `height` cells across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Playable {
    pub x: u32,
    pub z: u32,
    pub width: u32,
    pub height: u32,
}

impl Playable {
    /// Whether world (`x`, `z`) lies inside.
    pub fn contains(&self, x: f64, z: f64) -> bool {
        x >= f64::from(self.x)
            && z >= f64::from(self.z)
            && x < f64::from(self.x + self.width)
            && z < f64::from(self.z + self.height)
    }
}

impl UtcMap {
    /// Parses and checks a map document (the JSON form); `file` labels the errors.
    pub fn parse(json: &[u8], file: &str) -> Result<Self, Errors> {
        let value: Value = serde_json::from_slice(json)
            .map_err(|error| Errors(vec![Error::new(file, "", error.to_string())]))?;
        Self::from_value(value, file)
    }

    /// Checks an already parsed JSON document.
    pub fn from_value(mut value: Value, file: &str) -> Result<Self, Errors> {
        let mut errors = Vec::new();
        check::raw(&mut value, file, &mut errors);
        let map = match crate::error::parse::<Self>(value, file, "") {
            Ok(map) => map,
            Err(error) => {
                errors.push(error);
                return Err(Errors(errors));
            }
        };
        check::document(&map, file, &mut errors);
        if errors.is_empty() {
            Ok(map.normalised())
        } else {
            Err(Errors(errors))
        }
    }

    /// The old parser's output: trimmed texts, no default stamp scale, no retired fields.
    pub(crate) fn normalised(mut self) -> Self {
        let name = check::js_trim(&self.name);
        self.name = if name.is_empty() {
            DEFAULT_MAP_NAME.into()
        } else {
            name.into()
        };
        self.description = self
            .description
            .as_deref()
            .map(check::js_trim)
            .filter(|d| !d.is_empty())
            .map(Into::into);
        for stamp in &mut self.stamps {
            if stamp.scale == Some(1.0) {
                stamp.scale = None;
            }
        }
        if let Some(landscape) = &mut self.landscape {
            landscape.water = None;
        }
        self
    }

    /// Skirmishes start surveyed; campaign missions keep discovery unless overridden.
    pub fn exploration(&self) -> Exploration {
        self.initial_exploration
            .unwrap_or(if self.mission.is_some() {
                Exploration::Hidden
            } else {
                Exploration::Explored
            })
    }

    /// The biome id; the first biome when the map names none.
    pub fn biome(&self) -> &str {
        self.biome.as_deref().unwrap_or(BIOMES[0])
    }

    /// The playable rectangle: [`UtcMap::playable`], or the whole map.
    pub fn playable_area(&self) -> Playable {
        self.playable.unwrap_or(Playable {
            x: 0,
            z: 0,
            width: self.size,
            height: self.size,
        })
    }

    pub fn player_start(&self, player: u8) -> Option<&PlayerStart> {
        self.player_starts.iter().find(|s| s.player == player)
    }
}

impl schemars::JsonSchema for Retired {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Retired".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let _ = &generator;
        schemars::json_schema!({"description": "no longer read"})
    }
}

impl<const N: u32> schemars::JsonSchema for Version<N> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("Version{N}").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let _ = &generator;
        schemars::json_schema!({"const": N})
    }
}
