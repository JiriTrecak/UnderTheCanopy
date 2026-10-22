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
    campaign::CampaignPackage,
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
    /// A mission map (it has a `mission` section).
    pub mission: bool,
    /// The campaign whose `missions/` hold the map.
    pub campaign: Option<String>,
    /// A mission with an autoplay plan (Play with AI).
    pub autoplay: bool,
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
        // A skirmish needs two starts, a testbed one; a mission may have none (its
        // players begin with what the map places).
        let playable = s.starts.len()
            >= if s.mission {
                0
            } else if s.sandbox {
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
            campaign: None,
            autoplay: s.autoplay,
            preview,
            playable,
            file,
        })
    }
}

pub struct GameData {
    pub content: Arc<Content>,
    pub pack: Arc<Pack>,
    /// Sorted by name; campaign missions included.
    pub maps: Vec<MapEntry>,
    /// The campaigns (`.utcc` packages), sorted by id.
    pub campaigns: Vec<Arc<CampaignPackage>>,
    /// Image pack entries of the campaigns' own art, by asset id.
    pub campaign_images: std::collections::BTreeMap<String, String>,
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
        let campaigns = load_campaigns(paths, &content);
        let mut maps = load_maps(paths, &pack)?;
        add_missions(&mut maps, &campaigns, &pack);
        let campaign_images = campaign_images(&campaigns);
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
            campaigns,
            campaign_images,
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
            campaigns: self.campaigns.clone(),
            campaign_images: self.campaign_images.clone(),
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
        let mut entry = MapEntry::from_package(file, &self.pack)?;
        entry.campaign = self.map(id).and_then(|m| m.campaign.clone());
        self.maps.retain(|m| m.id() != id);
        self.maps.push(entry);
        Ok(())
    }

    /// Puts the campaign package at `path` in place of its campaign, with its mission
    /// maps and art (the editor's copy with its unsaved edits). Returns its id.
    pub fn add_test_campaign(&mut self, path: &Path) -> Result<String> {
        let campaign = canopy_content::campaign::open(path).map_err(|e| anyhow!("{e}"))?;
        let id = campaign.id().to_string();
        self.maps
            .retain(|m| m.campaign.as_deref() != Some(id.as_str()));
        self.campaigns.retain(|c| c.id() != id);
        let campaign = Arc::new(campaign);
        let one = std::slice::from_ref(&campaign);
        add_missions(&mut self.maps, one, &self.pack);
        self.campaign_images.extend(campaign_images(one));
        self.campaigns.push(campaign);
        self.campaigns.sort_by(|a, b| a.id().cmp(b.id()));
        Ok(id)
    }

    pub fn map(&self, id: &str) -> Option<&MapEntry> {
        self.maps.iter().find(|m| m.id() == id)
    }

    /// Skirmish and multiplayer maps (no missions), playable ones.
    pub fn skirmish_maps(&self) -> impl Iterator<Item = &MapEntry> {
        self.maps.iter().filter(|m| !m.mission && m.playable)
    }

    pub fn campaign(&self, id: &str) -> Option<&Arc<CampaignPackage>> {
        self.campaigns.iter().find(|c| c.id() == id)
    }

    /// The packages a map's match layers over the base, when its campaign or the map
    /// itself changes game data (docs/campaign.md: base, campaign, map). `None`: the
    /// base content plays it.
    fn layers(&self, map: &MapEntry) -> Option<Vec<canopy_content::package::Package>> {
        let campaign = map.campaign.as_deref().and_then(|c| self.campaign(c))?;
        let package = canopy_content::package::Package::open(&map.file.path).ok()?;
        (campaign.has_content() || canopy_content::campaign::has_content(&package))
            .then(|| vec![campaign.package.clone(), package])
    }

    /// The content a match on `map` plays: the base's, or the base with its campaign and
    /// the map layered on top.
    pub fn content_for(&self, paths: &Paths, map: &str) -> Result<Arc<Content>> {
        let Some(layers) = self.map(map).and_then(|m| self.layers(m)) else {
            return Ok(self.content.clone());
        };
        let base =
            canopy_content::package::Package::open(&paths.data.join(canopy_content::BASE_DIR))
                .map_err(|e| anyhow!("content: {e}"))?;
        let mut packages = vec![base];
        packages.extend(layers);
        let files = if is_packaged(&paths.data) {
            canopy_content::assets::Files::Skip
        } else {
            canopy_content::assets::Files::Check
        };
        Content::from_packages(&packages, files)
            .map(Arc::new)
            .map_err(|e| anyhow!("content: {e}"))
    }

    /// The identity a save of this map must match; `map_identity` is the map package's
    /// digest (`MapFile::digest`). A map played with its campaign's layer also names the
    /// campaign package.
    pub fn match_identity(&self, map: &MapEntry, map_identity: &[u8]) -> saves::MatchIdentity {
        let content_digest = match self.layers(map).and_then(|l| l[0].digest().ok()) {
            Some(campaign) => {
                ContentIdentity::digest(
                    canopy_net::wire::ABI,
                    self.identity_parts
                        .iter()
                        .map(Vec::as_slice)
                        .chain(std::iter::once(campaign.as_slice())),
                )
                .sha256
            }
            None => self.content_digest.clone(),
        };
        saves::MatchIdentity {
            map_id: map.id().into(),
            map_digest: saves::digest(map_identity),
            content_digest,
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

/// The shipped campaigns that pass every rule; a broken one is left out with a warning.
fn load_campaigns(paths: &Paths, content: &Content) -> Vec<Arc<CampaignPackage>> {
    let found = match canopy_content::campaign::library(&paths.data) {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(%error, "campaigns");
            return Vec::new();
        }
    };
    found
        .into_iter()
        .filter(|c| {
            let errors = canopy_content::campaign::validate(c, content);
            for error in &errors {
                tracing::warn!(campaign = c.id(), %error, "campaign skipped");
            }
            errors.is_empty()
        })
        .map(Arc::new)
        .collect()
}

/// The campaigns' mission maps join the maps (as their campaign's); a map whose id is
/// already taken is left out with a warning.
fn add_missions(maps: &mut Vec<MapEntry>, campaigns: &[Arc<CampaignPackage>], pack: &Pack) {
    for campaign in campaigns {
        for file in &campaign.missions {
            if maps.iter().any(|m| m.id() == file.id) {
                tracing::warn!(map = %file.file, "a map with this id already exists; skipped");
                continue;
            }
            match MapEntry::from_package(file.clone(), pack) {
                Ok(mut entry) => {
                    entry.campaign = Some(campaign.id().to_string());
                    maps.push(entry);
                }
                Err(error) => tracing::warn!(%error, "mission skipped"),
            }
        }
    }
    maps.sort_by(|a, b| a.name.cmp(&b.name).then(a.id().cmp(b.id())));
}

/// The image pack entries of the campaigns' own assets, by asset id.
fn campaign_images(
    campaigns: &[Arc<CampaignPackage>],
) -> std::collections::BTreeMap<String, String> {
    let mut images = std::collections::BTreeMap::new();
    for campaign in campaigns {
        let mut errors = Vec::new();
        let index = canopy_content::assets::package::index(
            std::slice::from_ref(&campaign.package),
            canopy_content::assets::Files::Skip,
            &mut errors,
        );
        for record in &index.manifest.records {
            if let Some(output) = record
                .outputs
                .iter()
                .find(|o| o.role == canopy_content::assets::OutputRole::Image)
            {
                images.insert(record.id.to_string(), output.path.clone());
            }
        }
    }
    images
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
