//! `assets/maps/previews/index.json`: menu overviews of the shipped maps, keyed by map id
//! (the lowercase `.utcmap` file name), read by `src/shared/map/library.ts`.
//!
//! The game casts entries to `MapOverview` (`src/shared/map/overview.ts`); the writer,
//! `scripts/maps/previews.ts`, adds `generationHash`, so an entry is
//! `MapOverview & {generationHash?: string}`.

use super::read;
use crate::error::Error;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const MAP_PREVIEWS_FILE: &str = "assets/maps/previews/index.json";
/// Folder of the preview images named by [`MapOverview::image`].
pub const MAP_PREVIEWS_FOLDER: &str = "assets/maps/previews";

/// Map sizes a `.utcmap` may have.
pub const MAP_SIZES: [u32; 5] = [256, 512, 640, 1024, 2048];

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapOverview {
    pub name: String,
    pub size: u32,
    pub description: Option<String>,
    pub starts: Vec<PlayerStart>,
    pub camps: u32,
    pub sandbox: bool,
    pub mission: Option<MissionSummary>,
    /// `null` until validated; the menu then decides from the starts.
    #[serde(deserialize_with = "Option::deserialize")]
    #[serialize_always]
    pub playable: Option<bool>,
    /// Why the map is not playable.
    pub error: Option<String>,
    /// FNV-1a of the `.utcmap` text, hex; a stale hash makes the menu re-read the map.
    pub source_hash: String,
    /// File name in [`MAP_PREVIEWS_FOLDER`].
    pub image: Option<String>,
    pub custom: Option<bool>,
    pub generation_hash: Option<String>,
}

/// `PlayerStart` of a `.utcmap`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerStart {
    /// 1–8.
    pub player: u32,
    /// Cells, in halves.
    pub x: f64,
    pub z: f64,
    /// 0, 90, 180 or 270 degrees.
    pub rotation: Option<u32>,
    pub setup: String,
    pub main_fort: String,
}

/// The mission fields the menus show.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissionSummary {
    pub campaign: String,
    pub order: u32,
    pub title: String,
    pub presentation: Option<MissionPresentation>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissionPresentation {
    /// `asset.interface.*` image ids.
    pub loading_background: Option<String>,
    pub selection_background: Option<String>,
    pub chapter: Option<String>,
    pub briefing: Option<String>,
    pub tips: Option<Vec<String>>,
    pub features: Option<Vec<String>>,
}

/// Reads `assets/maps/previews/index.json` and checks sizes, starts and preview images.
pub fn load(data: &Path, errors: &mut Vec<Error>) -> Option<BTreeMap<String, MapOverview>> {
    let overviews: BTreeMap<String, MapOverview> = read(data, MAP_PREVIEWS_FILE, errors)?;
    for (id, overview) in &overviews {
        let mut fail = |message: String| errors.push(Error::new(MAP_PREVIEWS_FILE, id, message));
        if !MAP_SIZES.contains(&overview.size) {
            fail(format!("unsupported map size {}", overview.size));
        }
        let mut players = BTreeSet::new();
        for start in &overview.starts {
            if !players.insert(start.player) {
                fail(format!("duplicate player {}", start.player));
            }
            if !(1..=8).contains(&start.player) {
                fail(format!("player {} is outside 1–8", start.player));
            }
            if start.rotation.is_some_and(|r| r % 90 != 0 || r >= 360) {
                fail(format!(
                    "player {}: rotation must be a right angle",
                    start.player
                ));
            }
        }
        if let Some(image) = &overview.image {
            let path = format!("{MAP_PREVIEWS_FOLDER}/{image}");
            if image.contains('/') || !data.join(&path).is_file() {
                fail(format!("missing preview image {path}"));
            }
        }
    }
    Some(overviews)
}
