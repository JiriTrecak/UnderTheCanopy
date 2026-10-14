//! Published asset data the game reads at runtime.
//!
//! - `assets/manifest.json`: the runtime asset records ([`Manifest`]), read by
//!   `src/shared/assets/manifest.ts`. The type is `Manifest` with `RuntimeRecord`s from
//!   `tooling/asset-studio/shared/schema.ts`; scenery entries are `CatalogEntry`
//!   (`src/shared/asset/catalog.ts`) and render entries are [`RenderAsset`].
//! - `assets/authoring/catalogue.json`: the landscape index ([`landscape`]).
//! - `assets/authoring/models.json`: the published model index ([`models`]).
//! - `assets/maps/previews/index.json`: map overviews for the menus ([`previews`]).
//! - Texture data files the renderer imports by path ([`textures`]).
//!
//! The asset studio writes all of these and the game only casts them, so the rules here
//! are the ones publication enforces.

pub mod landscape;
pub mod models;
pub mod previews;
mod sha256;
pub mod textures;

use crate::{
    Content,
    error::{Error, parse, read_json},
    game::RenderAsset,
};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_with::skip_serializing_none;
use std::{collections::BTreeSet, fmt, path::Path};

const MANIFEST: &str = crate::MANIFEST_FILE;

/// `assets/manifest.json`: every published asset record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub version: VersionOne,
    pub records: Vec<Record>,
}

/// A published asset (`RuntimeRecord`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub id: AuthoringId,
    pub name: String,
    pub kind: RecordKind,
    pub faction: Option<String>,
    pub tags: Vec<String>,
    /// Positive.
    pub revision: u32,
    pub profile: String,
    /// At least one.
    pub outputs: Vec<Output>,
    pub render: Vec<RenderAsset>,
    pub scenery: Vec<SceneryEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordKind {
    Icon,
    Interface,
    Model,
    Texture,
    Audio,
    Map,
    Data,
}

/// A published file.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Output {
    pub role: OutputRole,
    /// Relative to the data folder; always below `assets/`.
    pub path: String,
    /// Hex SHA-256 of the file.
    pub sha256: String,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub triangles: Option<u32>,
    pub primitives: Option<u32>,
}

/// The schema allows any string; publication writes only these three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputRole {
    Model,
    Image,
    Data,
}

/// A placeable scenery model (`CatalogEntry`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneryEntry {
    pub id: AuthoringId,
    pub name: String,
    pub category: SceneryCategory,
    #[serde(rename = "type")]
    pub kind: SceneryType,
    /// Relative to `assets/`.
    pub file: String,
    /// Retired references keep gameplay geometry but cannot be placed as new art.
    pub editor_hidden: Option<bool>,
    pub deck: Option<Deck>,
    pub light: Option<Light>,
    pub blocker: Option<Blocker>,
    pub blockers: Option<Vec<Blocker>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneryCategory {
    Units,
    Foliage,
    Terrain,
    Water,
    Landmark,
    Resource,
    Other,
}

/// `prop` sits on dry land, `water` on wet cells, `span` (bridges) and `ground` anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneryType {
    Prop,
    Water,
    Span,
    Ground,
}

/// Walkable deck of a bridge, in asset-local metres.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Deck {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub arch: f64,
    pub thickness: Option<f64>,
    pub rise: Option<f64>,
    /// Navigation level, 1–31.
    pub level: u32,
    pub connections: Option<DeckConnections>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckConnections {
    pub start: Option<u32>,
    pub end: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Light {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// `#rrggbb`.
    pub color: String,
    pub intensity: f64,
    pub range: f64,
}

/// Ground-plane collision in asset-local metres, independent of the visual mesh.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Blocker {
    pub width: f64,
    pub depth: f64,
    pub shape: Option<BlockerShape>,
    pub x: Option<f64>,
    pub z: Option<f64>,
    pub yaw: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockerShape {
    Ellipse,
}

/// A file of an asset by role and 1-based index (`ResourceRef`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceRef {
    pub role: FileRole,
    /// 1–1024.
    pub index: u32,
}

/// `FILE_ROLES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileRole {
    Geometry,
    Source,
    Recipe,
    Build,
    Image,
    Albedo,
    Normal,
    Roughness,
    Metalness,
    Occlusion,
    Height,
    Opacity,
    TeamMask,
    Emissive,
    Wind,
    Animation,
    Collision,
    Walkable,
    Preview,
    Reference,
    Data,
    Grading,
    Audio,
    Generation,
}

/// The literal `1` of versioned documents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct VersionOne;

impl TryFrom<u32> for VersionOne {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, String> {
        match value {
            1 => Ok(Self),
            _ => Err(format!("unsupported version {value}, expected 1")),
        }
    }
}

impl From<VersionOne> for u32 {
    fn from(_: VersionOne) -> u32 {
        1
    }
}

/// The literal `true`, for flags that are either present and true or absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "bool", into = "bool")]
pub struct True;

impl TryFrom<bool> for True {
    type Error = &'static str;
    fn try_from(value: bool) -> Result<Self, &'static str> {
        if value {
            Ok(Self)
        } else {
            Err("expected true")
        }
    }
}

impl From<True> for bool {
    fn from(_: True) -> bool {
        true
    }
}

/// An authoring asset id (`authoringId`): `[a-zA-Z0-9][a-zA-Z0-9._-]*`, 1–160 characters.
/// Looser than [`crate::Id`]: asset, scenery and recipe ids may use capitals and `_`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AuthoringId(String);

impl AuthoringId {
    pub fn new(text: impl Into<String>) -> Result<Self, String> {
        let text = text.into();
        let bytes = text.as_bytes();
        let valid = !bytes.is_empty()
            && bytes.len() <= 160
            && bytes[0].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if valid {
            Ok(Self(text))
        } else {
            Err(format!("invalid asset id {text:?}"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AuthoringId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for AuthoringId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::borrow::Borrow<str> for AuthoringId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for AuthoringId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

/// Reads `assets/manifest.json` and checks it: unique record, scenery and output paths;
/// every output below `assets/` and present under `data`; render and scenery files
/// declared as outputs.
pub fn load(data: &Path, errors: &mut Vec<Error>) -> Option<Manifest> {
    let manifest: Manifest = read(data, MANIFEST, errors)?;
    check_manifest(data, &manifest, errors);
    Some(manifest)
}

/// Every render asset the published records declare, unsorted.
pub fn render_assets(manifest: &Manifest) -> Vec<RenderAsset> {
    manifest
        .records
        .iter()
        .flat_map(|r| r.render.iter().cloned())
        .collect()
}

/// Every placeable scenery model in record order (the game's `sceneryCatalogue`).
pub fn scenery(manifest: &Manifest) -> impl Iterator<Item = &SceneryEntry> {
    manifest.records.iter().flat_map(|r| &r.scenery)
}

/// Every output path the manifest publishes.
pub fn output_paths(manifest: &Manifest) -> BTreeSet<&str> {
    manifest
        .records
        .iter()
        .flat_map(|r| &r.outputs)
        .map(|o| o.path.as_str())
        .collect()
}

/// Asset rules that need the whole content: render assets placed as scenery name a
/// published scenery entry.
pub fn validate(content: &Content, errors: &mut Vec<Error>) {
    let scenery: BTreeSet<&str> = scenery(&content.manifest).map(|s| s.id.as_str()).collect();
    for asset in &content.render_assets {
        if let Some(id) = &asset.scenery_asset
            && !scenery.contains(id.as_str())
        {
            errors.push(Error::new(
                MANIFEST,
                format!("assets/{}", asset.id),
                format!("unknown sceneryAsset {id}"),
            ));
        }
    }
}

/// Checks every published output against its file under `data`: size and SHA-256. Reads
/// every byte, so [`Content::load`] does not run it. Git LFS pointer files count as
/// missing content.
pub fn verify_outputs(data: &Path) -> Vec<Error> {
    let mut errors = Vec::new();
    let Some(manifest) = read::<Manifest>(data, MANIFEST, &mut errors) else {
        return errors;
    };
    for output in manifest.records.iter().flat_map(|r| &r.outputs) {
        let mut fail = |message: String| errors.push(Error::new(&output.path, "", message));
        let bytes = match std::fs::read(data.join(&output.path)) {
            Ok(bytes) => bytes,
            Err(error) => {
                fail(error.to_string());
                continue;
            }
        };
        if bytes.starts_with(b"version https://git-lfs.github.com/spec/") {
            fail("Git LFS pointer, not the file (run `git lfs pull`)".into());
            continue;
        }
        if bytes.len() as u64 != output.bytes {
            fail(format!(
                "{} bytes, manifest says {}",
                bytes.len(),
                output.bytes
            ));
        }
        let hash = sha256::hex(&bytes);
        if !hash.eq_ignore_ascii_case(&output.sha256) {
            fail(format!("SHA-256 {hash}, manifest says {}", output.sha256));
        }
    }
    errors
}

/// Reads and parses one JSON file below `data`.
pub(crate) fn read<T: DeserializeOwned>(
    data: &Path,
    file: &str,
    errors: &mut Vec<Error>,
) -> Option<T> {
    read_json(data, file)
        .and_then(|value| parse(value, file, ""))
        .map_err(|error| errors.push(error))
        .ok()
}

/// Reports every id that occurs more than once.
pub(crate) fn unique<'a>(
    file: &str,
    location: &str,
    what: &str,
    ids: impl IntoIterator<Item = &'a str>,
    errors: &mut Vec<Error>,
) {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            errors.push(Error::new(file, location, format!("duplicate {what} {id}")));
        }
    }
}

/// `safePath`: `[a-zA-Z0-9_./-]+`, relative, without `..` segments.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'))
        && !path.starts_with('/')
        && !path.split('/').any(|segment| segment == "..")
}

fn check_manifest(data: &Path, manifest: &Manifest, errors: &mut Vec<Error>) {
    unique(
        MANIFEST,
        "records",
        "asset",
        manifest.records.iter().map(|r| r.id.as_str()),
        errors,
    );
    unique(
        MANIFEST,
        "scenery",
        "scenery",
        scenery(manifest).map(|s| s.id.as_str()),
        errors,
    );
    // Unique ignoring case, as publication requires: case-insensitive file systems.
    let mut lowercase = BTreeSet::new();
    let outputs = output_paths(manifest);
    for record in &manifest.records {
        let location = format!("records/{}", record.id);
        let mut fail = |message: String| errors.push(Error::new(MANIFEST, &location, message));
        if record.name.is_empty() {
            fail("name must not be empty".into());
        }
        if record.revision == 0 {
            fail("revision must be positive".into());
        }
        if record.outputs.is_empty() {
            fail("needs at least one output".into());
        }
        for output in &record.outputs {
            let path = output.path.as_str();
            if !safe_path(path) || !path.starts_with("assets/") {
                fail(format!(
                    "output {path:?} must be a plain path below assets/"
                ));
            } else if !data.join(path).is_file() {
                fail(format!("missing output {path}"));
            }
            if !lowercase.insert(path.to_lowercase()) {
                fail(format!("duplicate output {path}"));
            }
            if output.sha256.len() != 64 || !output.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                fail(format!("output {path}: sha256 must be 64 hex digits"));
            }
        }
        // A render binding may resolve to another record's output, as publication allows.
        for asset in &record.render {
            for file in [&asset.file, &asset.image, &asset.harvest_animation]
                .into_iter()
                .flatten()
            {
                if !outputs.contains(file.as_str()) {
                    fail(format!(
                        "render {}: {file} is not a published output",
                        asset.id
                    ));
                }
            }
        }
        for scenery in &record.scenery {
            let file = format!("assets/{}", scenery.file);
            if !record.outputs.iter().any(|o| o.path == file) {
                fail(format!(
                    "scenery {}: {file} is not an output of its record",
                    scenery.id
                ));
            }
            if scenery.name.trim().is_empty() {
                fail(format!("scenery {}: name must not be empty", scenery.id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoring_ids_follow_the_authoring_grammar() {
        for text in [
            "asset.models.a",
            "Synty_Rock-01",
            "9lives",
            "recipe.river.glacial.water",
        ] {
            assert!(AuthoringId::new(text).is_ok(), "{text}");
        }
        for text in ["", ".a", "-a", "a b", "a/b", &"a".repeat(161)] {
            assert!(AuthoringId::new(text).is_err(), "{text}");
        }
    }

    #[test]
    fn safe_paths_stay_relative() {
        assert!(safe_path("assets/library/a/geometry.glb"));
        for path in [
            "",
            "/assets/a.png",
            "assets/../a.png",
            "assets/a b.png",
            "assets\\a.png",
        ] {
            assert!(!safe_path(path), "{path}");
        }
    }

    #[test]
    fn literals_reject_other_values() {
        assert!(serde_json::from_str::<VersionOne>("1").is_ok());
        assert!(serde_json::from_str::<VersionOne>("2").is_err());
        assert!(serde_json::from_str::<True>("true").is_ok());
        assert!(serde_json::from_str::<True>("false").is_err());
    }
}
