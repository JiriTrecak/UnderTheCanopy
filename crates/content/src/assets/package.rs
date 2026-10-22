//! Assets as packages store them (docs/packages.md, D53): one folder per asset holding
//! `asset.json` and its files (`assets/`), placeable scenery (`scenery/`), and landscape
//! water profiles and biomes (`landscape/`).
//!
//! From these the loader builds the registries the game reads — the asset records
//! ([`Manifest`]), the model index ([`PublishedModel`]) and the landscape catalogue
//! ([`LandscapeAsset`]) — and names every runtime file by its pack entry,
//! `<asset id>/<file>`, wherever the asset's folder sits.

use super::{
    AuthoringId, Blocker, Deck, FileRole, Files, Light, Manifest, Output, OutputRole, Record,
    RecordKind, SceneryCategory, SceneryEntry, SceneryType, VersionOne,
    landscape::{
        AssetKind, GroundTexture, Harvesting, LandscapeAsset, TerrainImages, WaterProfile,
    },
    models::{Capabilities, ModelTransform, PublishedModel},
};
use crate::{
    error::Error,
    game::RenderAsset,
    load::elements,
    package::{Collection, Form, Package},
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

pub const ASSETS: Collection = Collection {
    dir: "assets",
    form: Form::Folders(ASSET_FILE),
};
pub const SCENERY: Collection = Collection::files("scenery");
pub const WATER: Collection = Collection::files("landscape/water");

/// The file that makes a folder an asset.
pub const ASSET_FILE: &str = "asset.json";

/// `asset.json`: what an asset is and how to read its files.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetDefinition {
    pub id: AuthoringId,
    pub name: String,
    pub kind: AssetKind,
    #[serde(default)]
    pub tags: Vec<String>,
    /// The race a model belongs to (team colour expected).
    pub race: Option<String>,
    /// The asset's files, each `<role>[_<index>].<format>` in its folder.
    pub resources: Vec<AssetFile>,
    /// How a model's geometry is oriented and scaled when cooked.
    pub transform: Option<ModelTransform>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<serde_json::Value>,
    pub capabilities: Option<Capabilities>,
    /// How a model is shown wherever it is used.
    pub display: Option<Display>,
    /// Building shading for structures; props otherwise.
    pub shading: Option<AssetShading>,
    /// A ground texture's projection, atlas and channels.
    pub terrain: Option<GroundTexture>,
    pub model_class: Option<String>,
    pub license: Option<String>,
}

/// A file of an asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetFile {
    pub role: FileRole,
    /// 1-based.
    pub index: u32,
    pub format: String,
    /// How an image's colour channels are encoded, when not by its role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_space: Option<String>,
}

impl AssetFile {
    /// The file name: `<role>.<format>`, or `<role>_<index>.<format>` after the first.
    pub fn name(&self) -> String {
        let role = serde_json::to_value(self.role)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        match self.index {
            1 => format!("{role}.{}", self.format),
            index => format!("{role}_{index}.{}", self.format),
        }
    }

    /// Working sources and authoring records: kept with the asset, never cooked or shipped.
    pub fn is_source(&self) -> bool {
        matches!(
            self.role,
            FileRole::Source
                | FileRole::Recipe
                | FileRole::Build
                | FileRole::Generation
                | FileRole::Reference
        )
    }
}

/// How a model is shown: scale, animation profile, health bar height and, when the
/// automatic framing misses its face, what its close dialogue portrait shows. A
/// definition may override the health bar height and profile for its own use (the
/// placeholder model stands in for many).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Display {
    pub scale: Option<f64>,
    pub character: Option<crate::Id>,
    pub health_height: Option<f64>,
    pub portrait: Option<crate::game::PortraitFrame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AssetShading {
    Building,
}

/// A placeable piece of scenery (`scenery/`): a model with its placement rules.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneryDefinition {
    /// `scenery.<name>`.
    pub id: AuthoringId,
    pub name: String,
    pub category: SceneryCategory,
    #[serde(rename = "type")]
    pub kind: SceneryType,
    /// The model asset.
    pub model: AuthoringId,
    /// Retired pieces keep gameplay geometry but cannot be placed as new art.
    pub editor_hidden: Option<bool>,
    pub deck: Option<Deck>,
    pub light: Option<Light>,
    pub blocker: Option<Blocker>,
    pub blockers: Option<Vec<Blocker>>,
    /// Placed on playable ground, it becomes this resource.
    pub harvesting: Option<SceneryHarvesting>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneryHarvesting {
    pub definition: AuthoringId,
}

/// `landscape/water/`: the look of a body of water.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaterFile {
    pub id: AuthoringId,
    pub name: String,
    pub water: WaterProfile,
}

/// Everything the game reads about assets, built from the packages.
#[derive(Debug, Clone, Default)]
pub struct AssetIndex {
    pub assets: Vec<AssetDefinition>,
    pub scenery: Vec<SceneryDefinition>,
    pub manifest: Manifest,
    pub models: Vec<PublishedModel>,
    pub catalogue: Vec<LandscapeAsset>,
    /// Pack entry → the file on disk.
    pub sources: BTreeMap<String, std::path::PathBuf>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: VersionOne,
            records: Vec::new(),
        }
    }
}

fn record_kind(kind: AssetKind) -> RecordKind {
    match kind {
        AssetKind::Icon => RecordKind::Icon,
        AssetKind::Interface => RecordKind::Interface,
        AssetKind::Texture => RecordKind::Texture,
        AssetKind::Audio => RecordKind::Audio,
        AssetKind::TerrainMaterial | AssetKind::WaterProfile | AssetKind::Data => RecordKind::Data,
        AssetKind::Map => RecordKind::Map,
        _ => RecordKind::Model,
    }
}

fn kind_name(kind: AssetKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Reads the assets, scenery and landscape files of `packages` (layered) and builds the
/// registries. `files` decides whether every file an asset lists must exist on disk.
pub fn index(packages: &[Package], files: Files, errors: &mut Vec<Error>) -> AssetIndex {
    let mut paths = BTreeMap::new();
    let mut out = AssetIndex::default();
    // Asset folders: the latest package's folder that has a file provides it.
    let layers = crate::package::Layers::build(&ASSETS, packages, errors);
    let mut folders: BTreeMap<String, Vec<std::path::PathBuf>> = BTreeMap::new();
    for layered in layers.elements.values() {
        let top = layered.top();
        let file = packages[top.package].label(&format!("{}/{ASSET_FILE}", top.path));
        match crate::error::parse::<AssetDefinition>(layered.value.clone(), &file, "") {
            Ok(asset) => {
                folders.insert(
                    asset.id.to_string(),
                    layered
                        .contributions
                        .iter()
                        .rev()
                        .map(|c| packages[c.package].root.join(&c.path))
                        .collect(),
                );
                paths.insert(asset.id.to_string(), file);
                out.assets.push(asset);
            }
            Err(e) => errors.push(e),
        }
    }
    for element in elements(&SCENERY, packages, &mut paths, errors) {
        if let Some(scenery) = element.parse::<SceneryDefinition>(errors) {
            out.scenery.push(scenery);
        }
    }
    let water: Vec<WaterFile> = elements(&WATER, packages, &mut paths, errors)
        .into_iter()
        .filter_map(|e| e.parse(errors))
        .collect();
    let assets: BTreeMap<&str, &AssetDefinition> =
        out.assets.iter().map(|a| (a.id.as_str(), a)).collect();
    let entry = |id: &str, file: &AssetFile| format!("{id}/{}", file.name());
    for asset in &out.assets {
        let id = asset.id.as_str();
        let label = paths.get(id).cloned().unwrap_or_default();
        if let Some(p) = asset.display.as_ref().and_then(|d| d.portrait) {
            let finite = p.focus.iter().all(|v| v.is_finite());
            let turn = p.turn.is_none_or(|t| (-180.0..=180.0).contains(&t));
            if !finite || !turn || !p.size.is_finite() || p.size <= 0.0 {
                errors.push(Error::new(
                    &label,
                    "display.portrait",
                    "a portrait frames a finite focus with a positive size, turned -180 to 180 degrees",
                ));
            }
        }
        for file in &asset.resources {
            let name = file.name();
            let found = folders[id]
                .iter()
                .map(|f| f.join(&name))
                .find(|p| p.is_file());
            match found {
                Some(path) => {
                    if !file.is_source() {
                        out.sources.insert(entry(id, file), path);
                    }
                }
                None if files == Files::Check => {
                    errors.push(Error::new(&label, "resources", format!("missing {name}")));
                }
                None => {}
            }
        }
    }

    let geometry = |a: &AssetDefinition| {
        a.resources
            .iter()
            .filter(|f| f.format == "glb" && !f.is_source())
            .map(|f| entry(a.id.as_str(), f))
            .collect::<Vec<_>>()
    };
    let mut scenery_of: BTreeMap<&str, Vec<&SceneryDefinition>> = BTreeMap::new();
    for s in &out.scenery {
        match assets.get(s.model.as_str()) {
            Some(model) if !geometry(model).is_empty() => {
                scenery_of.entry(s.model.as_str()).or_default().push(s);
            }
            _ => errors.push(Error::new(
                paths.get(s.id.as_str()).map_or("scenery", String::as_str),
                "model",
                format!("{} is not a model asset", s.model),
            )),
        }
    }

    for asset in &out.assets {
        let id = asset.id.as_str();
        let runtime: Vec<&AssetFile> = asset.resources.iter().filter(|f| !f.is_source()).collect();
        let geometry = geometry(asset);
        let image = runtime
            .iter()
            .find(|f| f.role == FileRole::Image)
            .map(|f| entry(id, f));
        let display = asset.display.clone().unwrap_or_default();
        let bound = scenery_of.get(id).cloned().unwrap_or_default();
        let mut render = Vec::new();
        if let Some(file) = geometry.first() {
            render.push(RenderAsset {
                id: crate::Id::new(id).unwrap_or_else(|_| crate::Id::new("invalid").unwrap()),
                file: Some(file.clone()),
                scale: display.scale,
                character: display.character.clone(),
                health_height: display.health_height,
                portrait: display.portrait,
                ..RenderAsset::empty()
            });
            // A placed piece of scenery is drawn as its prototype; felled, as its model.
            for s in &bound {
                render.push(RenderAsset {
                    id: crate::Id::new(s.id.as_str())
                        .unwrap_or_else(|_| crate::Id::new("invalid").unwrap()),
                    file: Some(file.clone()),
                    scale: display.scale,
                    harvest_animation: Some(file.clone()),
                    scenery_asset: Some(s.id.to_string()),
                    ..RenderAsset::empty()
                });
            }
        } else if let Some(image) = image.as_ref().filter(|_| asset.kind == AssetKind::Icon) {
            // Icons are what definitions, commands and spells show.
            render.push(RenderAsset {
                id: crate::Id::new(id).unwrap_or_else(|_| crate::Id::new("invalid").unwrap()),
                image: Some(image.clone()),
                ..RenderAsset::empty()
            });
        }
        let profile = match (asset.shading, asset.kind) {
            (Some(AssetShading::Building), _) => "building".to_string(),
            (_, kind) => kind_name(kind),
        };
        out.manifest.records.push(Record {
            id: asset.id.clone(),
            name: asset.name.clone(),
            kind: record_kind(asset.kind),
            faction: asset.race.clone(),
            tags: asset.tags.clone(),
            revision: 1,
            profile,
            outputs: runtime
                .iter()
                .map(|f| Output {
                    role: match f.format.as_str() {
                        "glb" => OutputRole::Model,
                        "png" | "jpg" | "jpeg" | "webp" => OutputRole::Image,
                        _ => OutputRole::Data,
                    },
                    path: entry(id, f),
                    sha256: String::new(),
                    bytes: 0,
                    width: None,
                    height: None,
                    triangles: None,
                    primitives: None,
                })
                .collect(),
            render,
            scenery: bound
                .iter()
                .map(|s| SceneryEntry {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    category: s.category,
                    kind: s.kind,
                    file: geometry.first().cloned().unwrap_or_default(),
                    editor_hidden: s.editor_hidden,
                    deck: s.deck.clone(),
                    light: s.light.clone(),
                    blocker: s.blocker.clone(),
                    blockers: s.blockers.clone(),
                })
                .collect(),
        });
        if !geometry.is_empty() {
            let capabilities = asset.capabilities.clone();
            out.models.push(PublishedModel {
                id: asset.id.clone(),
                transform: asset
                    .transform
                    .clone()
                    .unwrap_or_else(ModelTransform::identity),
                ground_contact: capabilities
                    .as_ref()
                    .and_then(|c| c.ground_contact.as_ref())
                    .map(|g| g.mode),
                canopy: capabilities.as_ref().and_then(|c| c.canopy),
                capabilities,
                geometry: geometry.clone(),
                scenery: bound.iter().map(|s| s.id.clone()).collect(),
            });
        }
        // The landscape catalogue: models (by asset and by scenery id), ground textures.
        let clearance = asset
            .capabilities
            .as_ref()
            .and_then(|c| c.vegetation_clearance)
            .unwrap_or(0.0);
        let scale = asset.transform.as_ref().map_or(1.0, |t| t.scale);
        let landscape = |key: &AuthoringId, scenery: Option<&SceneryDefinition>| LandscapeAsset {
            id: key.clone(),
            name: asset.name.clone(),
            kind: asset.kind,
            scenery: scenery.map(|s| s.id.clone()),
            scale,
            clearance,
            harvesting: scenery.and_then(|s| {
                s.harvesting.as_ref().map(|h| Harvesting {
                    definition: h.definition.clone(),
                    asset: s.id.to_string(),
                })
            }),
            water: None,
            terrain: None,
            terrain_images: None,
        };
        if !geometry.is_empty() {
            out.catalogue
                .push(landscape(&asset.id, bound.first().copied()));
            for s in &bound {
                out.catalogue.push(landscape(&s.id, Some(s)));
            }
        }
        if let Some(terrain) = &asset.terrain {
            let image = |role: FileRole| {
                runtime
                    .iter()
                    .find(|f| f.role == role && f.index == 1)
                    .map(|f| entry(id, f))
            };
            out.catalogue.push(LandscapeAsset {
                terrain: Some(terrain.clone()),
                terrain_images: Some(TerrainImages {
                    albedo: image(FileRole::Albedo),
                    normal: image(FileRole::Normal),
                    occlusion: image(FileRole::Occlusion),
                    roughness: image(FileRole::Roughness),
                    metalness: image(FileRole::Metalness),
                    thumbnail: image(FileRole::Image),
                }),
                ..landscape(&asset.id, None)
            });
        }
    }
    for w in water {
        out.catalogue.push(LandscapeAsset {
            id: w.id,
            name: w.name,
            kind: AssetKind::WaterProfile,
            scenery: None,
            scale: 1.0,
            clearance: 0.0,
            harvesting: None,
            water: Some(w.water),
            terrain: None,
            terrain_images: None,
        });
    }
    out.catalogue.sort_by(|a, b| a.id.cmp(&b.id));
    out
}
