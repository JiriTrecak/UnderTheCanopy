//! What the game loads once at boot and keeps for every screen and match: the content,
//! the cooked pack, the map library with what the menus show of each map, and the identity of the
//! content for saves and multiplayer.
//!
//! A packaged game ships the base package's documents and the cooked pack instead of the
//! asset files (D26), so content is read with `Content::load_packaged` there.

use crate::{paths::Paths, saves};
use anyhow::{Context, Result, anyhow};
use canopy_assets::Pack;
use canopy_content::{
    Content,
    map::{self, MapFile, package::MissionSummary},
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
    pub mission: Option<MissionSummary>,
    /// Pack entry of the preview image.
    pub preview: Option<String>,
    pub playable: bool,
}

impl MapEntry {
    pub fn id(&self) -> &str {
        &self.file.id
    }

    /// The map's identity in saves and matches: its package's digest.
    pub fn identity(&self) -> Result<Vec<u8>> {
        self.file
            .digest()
            .map(|d| d.to_vec())
            .map_err(|e| anyhow!("{}: {e}", self.file.file))
    }

    /// The pack entry of map `id`'s preview image.
    pub fn preview_entry(id: &str) -> String {
        format!("maps/{id}/{}", map::package::PREVIEW_FILE)
    }

    /// The menus' view of a map package, read without its terrain.
    fn from_package(file: MapFile, pack: &Pack) -> Result<Self> {
        let s = map::package::summary(&file.path).map_err(|e| anyhow!("{}: {e}", file.file))?;
        let preview =
            Some(Self::preview_entry(&file.id)).filter(|e| s.preview && pack.entry(e).is_some());
        // A skirmish needs two starts; a mission or testbed one.
        let playable = s.starts.len()
            >= if s.mission.is_some() || s.sandbox {
                1
            } else {
                2
            };
        Ok(Self {
            name: s.name,
            size: s.size,
            description: s.description,
            starts: s.starts.iter().map(|s| s.0).collect(),
            start_cells: s.starts.iter().map(|s| [s.1 as f32, s.2 as f32]).collect(),
            camps: s.camps,
            sandbox: s.sandbox,
            mission: s.mission,
            preview,
            playable,
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
        let (identity_parts, content_digest) = content_identity(paths)?;
        progress(BootStage::Maps, 0.8);
        let maps = load_maps(paths, &pack)?;
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

    /// This data with other content: the editor's documents for Test in game.
    pub fn testing(&self, content: Arc<Content>) -> Self {
        Self {
            content,
            pack: self.pack.clone(),
            maps: self.maps.clone(),
            content_digest: self.content_digest.clone(),
            identity_parts: self.identity_parts.clone(),
        }
    }

    /// Puts the map package at `path` in place of map `id` (the editor's unsaved copy).
    pub fn add_test_map(&mut self, id: &str, path: std::path::PathBuf) -> Result<()> {
        let group = self.map(id).map_or("skirmish", |m| m.file.group);
        let file = MapFile {
            id: id.to_string(),
            group,
            file: format!("editor/{id}.utcm"),
            path,
        };
        let entry = MapEntry::from_package(file, &self.pack)?;
        self.maps.retain(|m| m.id() != id);
        self.maps.push(entry);
        Ok(())
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

    /// The identity a save of this map must match; `map_identity` is the map package's
    /// digest (`MapFile::digest`).
    pub fn match_identity(&self, map: &MapEntry, map_identity: &[u8]) -> saves::MatchIdentity {
        saves::MatchIdentity {
            map_id: map.id().into(),
            map_digest: saves::digest(map_identity),
            content_digest: self.content_digest.clone(),
        }
    }

    /// What a multiplayer client reports in `ready`: the content and the map package's
    /// digest.
    pub fn network_identity(&self, map_identity: &[u8]) -> ContentIdentity {
        ContentIdentity::digest(
            canopy_net::wire::ABI,
            self.identity_parts
                .iter()
                .map(Vec::as_slice)
                .chain(std::iter::once(map_identity)),
        )
    }
}

/// The parts of the content's identity (the base package's digest) and their SHA-256,
/// the content digest saves and replays carry.
fn content_identity(paths: &Paths) -> Result<(Vec<Vec<u8>>, String)> {
    let base = canopy_content::package::Package::open(&paths.data.join(canopy_content::BASE_DIR))
        .map_err(|e| anyhow!("content: {e}"))?;
    let base_digest = base
        .digest()
        .with_context(|| format!("hashing {}", base.root.display()))?;
    let parts: Vec<Vec<u8>> = vec![base_digest.to_vec()];
    let digest =
        ContentIdentity::digest(canopy_net::wire::ABI, parts.iter().map(Vec::as_slice)).sha256;
    Ok((parts, digest))
}

/// The content digest of the game's data (`GameData::content_digest` without loading).
pub fn content_digest(paths: &Paths) -> Result<String> {
    Ok(content_identity(paths)?.1)
}

/// Every map the game offers: the base package's, then the player's own (`maps/` in the
/// user folder) and downloaded ones. A player's map with the id of an earlier one is left
/// out with a warning.
pub fn map_files(paths: &Paths) -> Result<Vec<MapFile>> {
    let mut files = map::library(&paths.data).map_err(|e| anyhow!("maps: {e}"))?;
    for (folder, group) in [
        (crate::paths::USER_MAPS, "custom"),
        (crate::paths::DOWNLOADED_MAPS, "downloaded"),
    ] {
        match map::folder(&paths.user.join(folder), group) {
            Ok(found) => {
                for file in found {
                    if files.iter().any(|f| f.id == file.id) {
                        tracing::warn!(map = %file.file, "a map with this id already exists; skipped");
                    } else {
                        files.push(file);
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "player maps"),
        }
    }
    Ok(files)
}

fn load_maps(paths: &Paths, pack: &Pack) -> Result<Vec<MapEntry>> {
    let files = map_files(paths)?;
    let mut maps = Vec::new();
    for file in files {
        match MapEntry::from_package(file, pack) {
            Ok(entry) => maps.push(entry),
            Err(error) => tracing::warn!(%error, "map skipped"),
        }
    }
    maps.sort_by(|a, b| a.name.cmp(&b.name).then(a.id().cmp(b.id())));
    Ok(maps)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copies the map package `from` to `to`, giving it package id `id`.
    fn copy_map(from: &Path, to: &Path, id: &str) {
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                std::fs::create_dir_all(&target).unwrap();
                copy_map(&entry.path(), &target, id);
            } else {
                std::fs::create_dir_all(to).unwrap();
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
        let manifest = to.join("package.json");
        if manifest.is_file() {
            let mut value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
            value["id"] = serde_json::json!(id);
            std::fs::write(&manifest, value.to_string()).unwrap();
        }
    }

    #[test]
    fn players_maps_join_the_base_maps() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let base = map::library(&data).unwrap();
        let user = std::env::temp_dir().join(format!("canopy-user-maps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&user);
        let maps = user.join(crate::paths::USER_MAPS);
        copy_map(&base[0].path, &maps.join("mine.utcm"), "mine");
        copy_map(&base[0].path, &maps.join("again.utcm"), base[0].id.as_str());
        let paths = Paths {
            data: data.clone(),
            user,
            cache: std::env::temp_dir(),
            replays: std::env::temp_dir(),
        };
        let files = map_files(&paths).unwrap();
        let mine = files
            .iter()
            .find(|f| f.id == "mine")
            .expect("the player's map");
        assert_eq!(mine.group, "custom");
        mine.load().unwrap_or_else(|e| panic!("{e}"));
        // The copy that reuses a base map's id is left out.
        assert_eq!(files.len(), base.len() + 1);
    }
}
