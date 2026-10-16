//! What the game loads once at boot and keeps for every screen and match: the content,
//! the cooked pack, the map library with its menu overviews, and the identity of the
//! content for saves and multiplayer.
//!
//! A packaged game ships the base package's documents and the cooked pack instead of the
//! asset files (D26), so content is read with `Content::load_packaged` there.

use crate::{paths::Paths, saves};
use anyhow::{Context, Result, anyhow};
use canopy_assets::Pack;
use canopy_content::{
    Content,
    assets::previews::{self, MapOverview},
    map::{self, MapFile},
};
use canopy_net::ContentIdentity;
use std::path::Path;
use std::sync::Arc;

/// A map as the menus show it.
#[derive(Debug, Clone)]
pub struct MapEntry {
    pub file: MapFile,
    pub name: String,
    pub size: u32,
    pub description: Option<String>,
    /// Player numbers of the starts (1–8), in map order.
    pub starts: Vec<u8>,
    /// Start positions in cells (x, z), for preview markers.
    pub start_cells: Vec<[f32; 2]>,
    pub camps: u32,
    pub sandbox: bool,
    pub mission: Option<previews::MissionSummary>,
    /// Pack entry of the preview image.
    pub preview: Option<String>,
    pub playable: bool,
}

impl MapEntry {
    pub fn id(&self) -> &str {
        &self.file.id
    }

    fn from_overview(file: MapFile, o: &MapOverview, pack: &Pack) -> Self {
        let preview = o
            .image
            .as_ref()
            .map(|image| format!("{}/{image}", previews::MAP_PREVIEWS_FOLDER))
            .filter(|entry| pack.entry(entry).is_some());
        Self {
            name: o.name.clone(),
            size: o.size,
            description: o.description.clone(),
            starts: o.starts.iter().map(|s| s.player as u8).collect(),
            start_cells: o.starts.iter().map(|s| [s.x as f32, s.z as f32]).collect(),
            camps: o.camps,
            sandbox: o.sandbox,
            mission: o.mission.clone(),
            preview,
            playable: o.playable.unwrap_or(!o.starts.is_empty()) && o.error.is_none(),
            file,
        }
    }

    /// Reads the map itself (maps the preview index does not list).
    fn from_map(file: MapFile) -> Result<Self> {
        let map = file.load().map_err(|e| anyhow!("{}: {e}", file.file))?;
        Ok(Self {
            name: map.name.clone(),
            size: map.size,
            description: map.description.clone(),
            starts: map.player_starts.iter().map(|s| s.player).collect(),
            start_cells: map
                .player_starts
                .iter()
                .map(|s| [s.x as f32, s.z as f32])
                .collect(),
            camps: map.camps.len() as u32,
            sandbox: map.sandbox,
            mission: map.mission.as_ref().map(|m| previews::MissionSummary {
                campaign: m.campaign.clone(),
                order: m.order,
                title: m.title.clone(),
                presentation: None,
            }),
            preview: None,
            playable: !map.player_starts.is_empty(),
            file,
        })
    }
}

pub struct GameData {
    pub content: Arc<Content>,
    pub pack: Arc<Pack>,
    /// Sorted by name.
    pub maps: Vec<MapEntry>,
    /// SHA-256 over the base package's digest (`Package::digest`).
    pub content_digest: String,
    /// The bytes behind the digest, kept for per-match identities.
    identity_parts: Vec<Vec<u8>>,
}

/// Loading steps, for the boot screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootStage {
    Pack,
    Content,
    Maps,
}

impl BootStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pack => "Opening the asset pack",
            Self::Content => "Reading the game's content",
            Self::Maps => "Surveying the battlefields",
        }
    }
}

/// Whether `data` is a packaged game's data folder.
pub fn is_packaged(data: &Path) -> bool {
    data.join(canopy_assets::PACK_LOCATIONS[0]).is_file()
}

impl GameData {
    /// Opens the cooked pack (its index only; entries are read on demand).
    pub fn open_pack(data: &Path) -> Result<Arc<Pack>> {
        let pack_path =
            canopy_assets::find_pack(data).context("no cooked asset pack (run `just cook`)")?;
        Ok(Arc::new(
            Pack::open(&pack_path).with_context(|| pack_path.display().to_string())?,
        ))
    }

    pub fn load(paths: &Paths, pack: Arc<Pack>, progress: &dyn Fn(BootStage, f32)) -> Result<Self> {
        let _span = tracing::info_span!("boot_data").entered();
        let started = std::time::Instant::now();
        progress(BootStage::Content, 0.2);
        let packaged = is_packaged(&paths.data);
        let content = if packaged {
            Content::load_packaged(&paths.data)
        } else {
            Content::load(&paths.data)
        }
        .map_err(|e| anyhow!("content: {e}"))?;
        let base =
            canopy_content::package::Package::open(&paths.data.join(canopy_content::BASE_DIR))
                .map_err(|e| anyhow!("content: {e}"))?;
        let base_digest = base
            .digest()
            .with_context(|| format!("hashing {}", base.root.display()))?;
        let identity_parts: Vec<Vec<u8>> = vec![base_digest.to_vec()];
        let content_digest = ContentIdentity::digest(
            canopy_net::wire::ABI,
            identity_parts.iter().map(Vec::as_slice),
        )
        .sha256;
        progress(BootStage::Maps, 0.8);
        let maps = load_maps(&paths.data, &pack)?;
        progress(BootStage::Maps, 1.0);
        tracing::info!(
            packaged,
            maps = maps.len(),
            ms = started.elapsed().as_millis() as u64,
            "game data loaded"
        );
        Ok(Self {
            content: Arc::new(content),
            pack,
            maps,
            content_digest,
            identity_parts,
        })
    }

    pub fn map(&self, id: &str) -> Option<&MapEntry> {
        self.maps.iter().find(|m| m.id() == id)
    }

    /// Skirmish and multiplayer maps (no missions), playable ones.
    pub fn skirmish_maps(&self) -> impl Iterator<Item = &MapEntry> {
        self.maps
            .iter()
            .filter(|m| m.mission.is_none() && m.playable)
    }

    /// Missions of a campaign, in order.
    pub fn missions(&self, campaign: &str) -> Vec<&MapEntry> {
        let mut out: Vec<&MapEntry> = self
            .maps
            .iter()
            .filter(|m| m.mission.as_ref().is_some_and(|x| x.campaign == campaign))
            .collect();
        out.sort_by_key(|m| m.mission.as_ref().map_or(0, |x| x.order));
        out
    }

    /// The identity a save of this map must match.
    pub fn match_identity(&self, map: &MapEntry, map_bytes: &[u8]) -> saves::MatchIdentity {
        saves::MatchIdentity {
            map_id: map.id().into(),
            map_digest: saves::digest(map_bytes),
            content_digest: self.content_digest.clone(),
        }
    }

    /// What a multiplayer client reports in `ready`: the content files and the map.
    pub fn network_identity(&self, map_bytes: &[u8]) -> ContentIdentity {
        ContentIdentity::digest(
            canopy_net::wire::ABI,
            self.identity_parts
                .iter()
                .map(Vec::as_slice)
                .chain(std::iter::once(map_bytes)),
        )
    }
}

fn load_maps(data: &Path, pack: &Pack) -> Result<Vec<MapEntry>> {
    let files = map::library(data).map_err(|e| anyhow!("maps: {e}"))?;
    let mut errors = Vec::new();
    let overviews = previews::load(data, &mut errors).unwrap_or_default();
    for e in &errors {
        tracing::warn!(error = %e, "map previews");
    }
    let mut maps = Vec::new();
    for file in files {
        let entry = match overviews.get(&file.id) {
            Some(o) => MapEntry::from_overview(file, o, pack),
            None => match MapEntry::from_map(file) {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(%error, "map skipped");
                    continue;
                }
            },
        };
        maps.push(entry);
    }
    maps.sort_by(|a, b| a.name.cmp(&b.name).then(a.id().cmp(b.id())));
    Ok(maps)
}
