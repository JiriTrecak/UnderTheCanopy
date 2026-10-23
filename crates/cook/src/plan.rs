//! What to cook: every runtime output the published data declares, as jobs.
//!
//! The rules read the data's own declarations; no asset id is named here.
//! - Every `.glb` output is a model.
//! - Images of records without geometry (icons, interface art, effect and terrain
//!   textures) are textures. Images of model records are published copies of what the
//!   glb embeds, so only those a render binding shows (`image`, the icons) are cooked.
//! - Channel images of terrain materials whose catalogue entry declares packed runtime
//!   tiles are editor previews; the tiles (`.bin`, gzip RGBA) are cooked instead.
//! - Other `.bin` outputs are data; `.wav` outputs are sounds; `.mp3` and `.ogg` stay as
//!   they are (data the game streams, D97); JSON outputs shaped as
//!   daytime colour grades become volume textures.
//! - Map packages' preview images, and one `@scenery` table of every placeable prototype.

use crate::texture::{AlphaPolicy, ColorSpace, Encoding, TextureParams};
use canopy_assets::{Blocker, BlockerShape, PointLight, SceneryPrototype, SceneryTable};
use canopy_content::assets::{
    self, Manifest, Record,
    landscape::LandscapeAsset,
    models::{self, PublishedModel},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Serialize)]
pub enum Job {
    Model {
        path: String,
        asset: String,
        profile: String,
        faction: bool,
        published: Option<Box<PublishedModel>>,
    },
    Image {
        path: String,
        params: TextureParams,
    },
    /// A gzip-compressed square RGBA8 terrain tile.
    Tile {
        path: String,
        params: TextureParams,
    },
    Data {
        path: String,
    },
    Sound {
        path: String,
    },
    Grades {
        path: String,
    },
    Scenery {
        table: SceneryTable,
    },
    /// The runtime effect table, converted from the published effects when planning; the
    /// job checks the images it samples against the effect budgets.
    Effects {
        table: canopy_assets::EffectTable,
        images: Vec<String>,
        warnings: Vec<String>,
    },
}

impl Job {
    /// The data-relative files the job reads.
    pub fn sources(&self) -> Vec<&str> {
        match self {
            Self::Model { path, .. }
            | Self::Image { path, .. }
            | Self::Tile { path, .. }
            | Self::Data { path }
            | Self::Sound { path }
            | Self::Grades { path } => vec![path.as_str()],
            Self::Scenery { .. } => Vec::new(),
            Self::Effects { images, .. } => images.iter().map(String::as_str).collect(),
        }
    }

    /// A short label for messages.
    pub fn label(&self) -> &str {
        match self {
            Self::Scenery { .. } => canopy_assets::SCENERY_ENTRY,
            Self::Effects { .. } => canopy_assets::EFFECTS_ENTRY,
            _ => self.sources()[0],
        }
    }
}

/// Where each pack entry's file is: asset files by their folder, anything else (map
/// previews) by its path in the data folder.
#[derive(Debug, Clone, Default)]
pub struct Locator {
    pub data: std::path::PathBuf,
    pub sources: BTreeMap<String, std::path::PathBuf>,
}

impl Locator {
    pub fn path(&self, entry: &str) -> std::path::PathBuf {
        self.sources
            .get(entry)
            .cloned()
            .unwrap_or_else(|| self.data.join(entry))
    }
}

/// The data the cooker reads.
pub struct Inputs {
    pub locator: Locator,
    pub manifest: Manifest,
    pub models: Vec<PublishedModel>,
    pub catalogue: Vec<LandscapeAsset>,
    /// Pack entries of the map packages' preview images.
    pub previews: Vec<String>,
    pub effects: Vec<canopy_content::effects::VisualEffect>,
}

impl Inputs {
    pub fn load(data: &Path) -> Result<Self, String> {
        let mut errors = Vec::new();
        let base = canopy_content::package::Package::open(&data.join(canopy_content::BASE_DIR))
            .map_err(|e| e.to_string())?;
        // The campaigns' own art (backgrounds, loading and story art) cooks into the same
        // pack (docs/campaign.md).
        let mut packages = vec![base];
        match canopy_content::campaign::library(data) {
            Ok(campaigns) => packages.extend(campaigns.into_iter().map(|c| c.package)),
            Err(e) => errors.extend(e.0),
        }
        let mut index = assets::package::index(&packages, assets::Files::Check, &mut errors);
        let mut previews = Vec::new();
        match canopy_content::map::library(data) {
            Ok(maps) => {
                for map in maps {
                    let file = map.path.join(canopy_content::map::package::PREVIEW_FILE);
                    if file.is_file() {
                        let entry = format!(
                            "maps/{}/{}",
                            map.id,
                            canopy_content::map::package::PREVIEW_FILE
                        );
                        index.sources.insert(entry.clone(), file);
                        previews.push(entry);
                    }
                }
            }
            Err(e) => errors.extend(e.0),
        }
        if errors.is_empty() {
            let mut model_errors = Vec::new();
            models::validate(&index.models, &index.manifest, &mut model_errors);
            if model_errors.is_empty() {
                let effects =
                    canopy_content::effects::load_base(data).map_err(|e| e.to_string())?;
                return Ok(Self {
                    locator: Locator {
                        data: data.to_path_buf(),
                        sources: index.sources,
                    },
                    manifest: index.manifest,
                    models: index.models,
                    catalogue: index.catalogue,
                    previews,
                    effects,
                });
            }
            errors = model_errors;
        }
        Err(canopy_content::Errors(errors).to_string())
    }
}

/// A planned cook: jobs, and outputs deliberately left out with the reason.
pub struct Plan {
    pub jobs: Vec<Job>,
    pub skipped: Vec<(String, &'static str)>,
    pub unknown: Vec<String>,
}

pub fn plan(inputs: &Inputs) -> Plan {
    let mut jobs = Vec::new();
    let mut skipped = Vec::new();
    let mut unknown = Vec::new();
    let render_images: BTreeSet<&str> = inputs
        .manifest
        .records
        .iter()
        .flat_map(|r| &r.render)
        .filter_map(|a| a.image.as_deref())
        .collect();
    let catalogue: BTreeMap<&str, &LandscapeAsset> = inputs
        .catalogue
        .iter()
        .map(|a| (a.id.as_str(), a))
        .collect();
    let mut cooked = BTreeSet::new();
    for record in &inputs.manifest.records {
        let is_model = record.outputs.iter().any(|o| extension(&o.path) == "glb");
        let entry = catalogue.get(record.id.as_str());
        let packed_tiles = entry
            .and_then(|a| a.terrain.as_ref())
            .and_then(|t| t.runtime.as_ref());
        let previews_of_tiles: BTreeSet<&str> =
            match (packed_tiles, entry.and_then(|a| a.terrain_images.as_ref())) {
                (Some(_), Some(images)) => images.paths().map(String::as_str).collect(),
                _ => BTreeSet::new(),
            };
        for output in &record.outputs {
            let path = output.path.clone();
            if !cooked.insert(path.clone()) {
                continue;
            }
            match extension(&path).as_str() {
                "glb" => jobs.push(Job::Model {
                    asset: record.id.to_string(),
                    profile: record.profile.clone(),
                    faction: record.faction.is_some(),
                    published: published_model(&inputs.models, &path).map(Box::new),
                    path,
                }),
                "png" | "jpg" | "jpeg" | "webp" => {
                    if is_model && !render_images.contains(path.as_str()) {
                        skipped.push((path, "copy of a texture the model's glb embeds"));
                    } else if previews_of_tiles.contains(path.as_str()) {
                        skipped.push((path, "editor preview of packed terrain tiles"));
                    } else {
                        let params = image_params(&path);
                        jobs.push(Job::Image { path, params });
                    }
                }
                "bin" if record.profile == "terrain-material" => {
                    let params = tile_params(record, entry.copied(), &path);
                    jobs.push(Job::Tile { path, params });
                }
                "bin" => jobs.push(Job::Data { path }),
                "wav" => jobs.push(Job::Sound { path }),
                // Music and voice stay compressed and stream (D97).
                "mp3" | "ogg" => jobs.push(Job::Data { path }),
                "json" => {
                    let mut errors = Vec::new();
                    let file = inputs.locator.path(&path);
                    if assets::textures::load_daytime_grades(
                        Path::new("/"),
                        &file.to_string_lossy(),
                        &mut errors,
                    )
                    .is_some()
                    {
                        jobs.push(Job::Grades { path });
                    } else {
                        skipped.push((path, "data the game reads from the catalogue"));
                    }
                }
                _ => unknown.push(path),
            }
        }
    }
    for path in &inputs.previews {
        if cooked.insert(path.clone()) {
            jobs.push(Job::Image {
                path: path.clone(),
                params: TextureParams::color(ColorSpace::Srgb),
            });
        }
    }
    jobs.push(Job::Scenery {
        table: scenery(&inputs.manifest),
    });
    let effects = crate::effects::convert(&inputs.effects, &inputs.manifest);
    jobs.push(Job::Effects {
        table: effects.table,
        images: effects.images,
        warnings: effects.warnings,
    });
    Plan {
        jobs,
        skipped,
        unknown,
    }
}

/// The model index entry of a geometry. Capabilities describe the first geometry; later
/// ones (carried cargo, alternates) share only the authored transform.
fn published_model(models: &[PublishedModel], path: &str) -> Option<PublishedModel> {
    let model = models
        .iter()
        .find(|m| m.geometry.iter().any(|g| g == path))?;
    if model.geometry.first().is_some_and(|g| g == path) {
        return Some(model.clone());
    }
    Some(PublishedModel {
        capabilities: None,
        ground_contact: None,
        canopy: None,
        scenery: Vec::new(),
        ..model.clone()
    })
}

fn extension(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The role and 1-based index of an output from its file name (`<role>[_<index>].<ext>`).
fn role_index(path: &str) -> (&str, u32) {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = file.split('.').next().unwrap_or(file);
    match stem.rsplit_once('_') {
        Some((role, index)) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => {
            (role, index.parse().unwrap_or(1))
        }
        _ => (stem, 1),
    }
}

fn role(path: &str) -> &str {
    role_index(path).0
}

/// Colour roles are sRGB; normal maps renormalise; team masks stay lossless.
fn image_params(path: &str) -> TextureParams {
    match role(path) {
        "albedo" | "image" | "emissive" | "preview" => TextureParams::color(ColorSpace::Srgb),
        "normal" => TextureParams {
            normal_map: true,
            ..TextureParams::color(ColorSpace::Linear)
        },
        "team_mask" => TextureParams {
            color: ColorSpace::Linear,
            alpha: AlphaPolicy::Opaque,
            normal_map: false,
            encoding: Encoding::R8,
        },
        _ => TextureParams::color(ColorSpace::Linear),
    }
}

/// Packed tiles carry data in alpha (roughness, height or metalness). The catalogue says
/// which tile is albedo, normal or occlusion; materials outside it publish an albedo image
/// next to their albedo tile.
fn tile_params(record: &Record, entry: Option<&LandscapeAsset>, path: &str) -> TextureParams {
    let index = role_index(path).1;
    let data = |color, normal_map| TextureParams {
        color,
        alpha: AlphaPolicy::Data,
        normal_map,
        encoding: Encoding::Bc7,
    };
    if let Some(runtime) = entry
        .and_then(|a| a.terrain.as_ref())
        .and_then(|t| t.runtime.as_ref())
    {
        if runtime.albedo_roughness.index == index {
            return data(ColorSpace::Srgb, false);
        }
        if runtime.normal_opacity.index == index {
            return data(ColorSpace::Linear, true);
        }
        return data(ColorSpace::Linear, false);
    }
    let has_albedo = record.outputs.iter().any(|o| role(&o.path) == "albedo");
    data(
        if has_albedo {
            ColorSpace::Srgb
        } else {
            ColorSpace::Linear
        },
        false,
    )
}

fn scenery(manifest: &Manifest) -> SceneryTable {
    let blocker = |b: &assets::Blocker| Blocker {
        shape: match b.shape {
            Some(assets::BlockerShape::Ellipse) => BlockerShape::Ellipse,
            None => BlockerShape::Box,
        },
        width: b.width as f32,
        depth: b.depth as f32,
        x: b.x.unwrap_or(0.0) as f32,
        z: b.z.unwrap_or(0.0) as f32,
        yaw: b.yaw.unwrap_or(0.0) as f32,
    };
    let prototypes = assets::scenery(manifest)
        .map(|s| SceneryPrototype {
            id: s.id.to_string(),
            model: s.file.clone(),
            light: s.light.as_ref().map(|l| PointLight {
                position: [l.x as f32, l.y as f32, l.z as f32],
                color: hex_color(&l.color),
                intensity: l.intensity as f32,
                range: l.range as f32,
            }),
            blockers: s
                .blocker
                .iter()
                .chain(s.blockers.iter().flatten())
                .map(blocker)
                .collect(),
            hidden: s.editor_hidden == Some(true),
        })
        .collect();
    SceneryTable { prototypes }
}

/// `#rrggbb` as linear RGB.
fn hex_color(text: &str) -> [f32; 3] {
    let hex = text.trim_start_matches('#');
    std::array::from_fn(|i| {
        let c = hex
            .get(i * 2..i * 2 + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(255);
        let c = f64::from(c) / 255.0;
        (if c <= 0.04045 {
            c / 12.92
        } else {
            libm::pow((c + 0.055) / 1.055, 2.4)
        }) as f32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_come_from_file_names() {
        assert_eq!(role("assets/library/a/albedo.png"), "albedo");
        assert_eq!(role("assets/library/a/albedo_2.png"), "albedo");
        assert_eq!(role("assets/library/a/team_mask.png"), "team_mask");
        assert_eq!(role_index("assets/library/a/data_3.bin"), ("data", 3));
        assert_eq!(image_params("x/team_mask.png").encoding, Encoding::R8);
        assert_eq!(image_params("x/image.webp").color, ColorSpace::Srgb);
        assert!(image_params("x/normal.png").normal_map);
    }

    #[test]
    fn hex_colours_are_linear() {
        assert_eq!(hex_color("#ffffff"), [1.0; 3]);
        assert_eq!(hex_color("#000000"), [0.0; 3]);
    }
}
