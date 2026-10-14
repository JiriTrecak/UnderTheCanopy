//! Authored maps (`assets/maps/**/*.utcmap`).
//!
//! A `.utcmap` is one JSON document (format version 3). [`UtcMap::load`] accepts exactly what
//! the old game's `readUtcMap` accepts and returns the same normalised value; [`validate`]
//! applies the map rules that need the content. Rules that need navigation geometry or the
//! terrain projection belong to the simulation.
//!
//! The committed terrain stays base64 text as stored; [`TerrainData::decode`] and the
//! per-grid helpers turn it into typed buffers.

mod authoring;
mod check;
mod library;
mod mission;
mod stamp;
mod validate;

pub use authoring::{
    AuthoredObject, DRY_WATER_HEIGHT, DecodedPaint, DecodedTerrain, GridError, HEIGHT_MAX,
    HEIGHT_MIN, HeightMode, PaintLayer, SavedScene, TerrainData, TextureGrid, TextureLayout,
    WaterFlow, decode_base64, decode_bytes, decode_floats,
};
pub use library::{MAP_GROUPS, MAPS_FOLDER, MapFile, library};
pub use mission::{Mission, Objective, Presentation, Region};
pub use stamp::{SourceTransform, Stamp, StampVariant, Walk, WalkConnections, WalkMesh};
pub use validate::validate;

use crate::{
    error::{Error, Errors},
    placement::{Camp, Placement},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use std::path::Path;

/// The `.utcmap` format version this reader accepts.
pub const UTCMAP_VERSION: u32 = 3;
/// File extension of authored maps.
pub const UTCMAP_EXTENSION: &str = "utcmap";
/// Name of a map whose document has no (or a blank) name.
pub const DEFAULT_MAP_NAME: &str = "Untitled";
/// Playable cells per side a map may have.
pub const MAP_SIZES: [u32; 5] = [256, 512, 640, 1024, 2048];
/// Visible, non-playable border around the playable cells, in cells per side.
pub const MAP_HALO: u32 = 16;
/// Biome ids. The old game defines biomes in code (`src/content/biomes.ts`), not data, and
/// rejects a map naming any other biome.
pub const BIOMES: [&str; 4] = [
    "vibrant-forest",
    "frozen-forest",
    "autumn-forest",
    "deep-forest",
];

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Exploration {
    Explored,
    Hidden,
}

/// Where a player's fort stands. `x`/`z` are navigation cells on half-cell steps.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WeatherKind {
    Clear,
    Rain,
    Snow,
    Spores,
}

/// Unknown keys are dropped, as the old parser's non-strict schema does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherConditions {
    pub kind: WeatherKind,
}

/// The only environment values a map owns; its look comes from the biome. Unknown keys are
/// dropped, as the old parser's non-strict schema does.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentConditions {
    /// 0 to 24.
    pub hour: f64,
    pub playing: bool,
    pub weather: Option<WeatherConditions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Landscape {
    pub environment: EnvironmentConditions,
    pub decals: Option<Vec<Decal>>,
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

/// One `.utcmap` document, normalised as the old parser returns it.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UtcMap {
    pub v: Version<UTCMAP_VERSION>,
    /// Playable cells per side, one of [`MAP_SIZES`].
    pub size: u32,
    /// Trimmed; [`DEFAULT_MAP_NAME`] when absent or blank.
    #[serde(default = "default_name")]
    pub name: String,
    /// One of [`BIOMES`]; see [`UtcMap::biome`].
    pub biome: Option<String>,
    /// Trimmed; `None` when blank.
    pub description: Option<String>,
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
    /// Committed terrain and placed objects.
    pub authoring: SavedScene,
}

impl UtcMap {
    /// Reads and checks a `.utcmap` file. Errors name the file as given.
    pub fn load(path: &Path) -> Result<Self, Errors> {
        let file = path.display().to_string();
        let bytes = std::fs::read(path)
            .map_err(|error| Errors(vec![Error::new(&file, "", error.to_string())]))?;
        Self::parse(&bytes, &file)
    }

    /// Parses and checks a `.utcmap` document; `file` labels the errors.
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
    fn normalised(mut self) -> Self {
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

    pub fn player_start(&self, player: u8) -> Option<&PlayerStart> {
        self.player_starts.iter().find(|s| s.player == player)
    }
}
