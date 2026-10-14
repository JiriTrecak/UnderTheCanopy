//! `assets/authoring/catalogue.json`: the landscape index (`LandscapeAsset[]`,
//! `src/shared/authoring/catalogue.ts`), read by `src/shared/authoring/project.ts` and
//! `groundTextures.ts`. Map scenes are generated from it: landscape recipes
//! (`landscapeRecipeSchema`), water profiles (`waterProfileSchema`) and ground textures
//! (`groundTextureSchema`).
//!
//! Publication writes the zod *output* of those schemas, so fields with zod defaults are
//! always present and are required here.

use super::{AuthoringId, Manifest, ResourceRef, output_paths, read, unique};
use crate::{
    error::Error,
    game::{Definition, Kind},
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const CATALOGUE_FILE: &str = "assets/authoring/catalogue.json";

/// One published asset that takes part in landscape generation.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LandscapeAsset {
    pub id: AuthoringId,
    pub name: String,
    pub kind: AssetKind,
    /// The first scenery binding of the asset.
    pub scenery: Option<AuthoringId>,
    pub scale: f64,
    /// Vegetation clearance; 0 when the asset declares none.
    pub clearance: f64,
    pub harvesting: Option<Harvesting>,
    pub recipe: Option<LandscapeRecipe>,
    pub water: Option<WaterProfile>,
    pub terrain: Option<GroundTexture>,
    pub terrain_images: Option<TerrainImages>,
}

/// `ASSET_KINDS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetKind {
    Unit,
    Creature,
    Building,
    Tree,
    Foliage,
    Prop,
    Bridge,
    TerrainMaterial,
    WaterProfile,
    LandscapeRecipe,
    Icon,
    Interface,
    Texture,
    Audio,
    Map,
    Data,
}

/// A harvestable model: generated instances become `definition` with render asset `asset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Harvesting {
    pub definition: AuthoringId,
    /// The record's render asset with a `sceneryAsset`; empty when it has none.
    pub asset: String,
}

/// Published image paths of a ground texture (`Partial<Record<…, string>>`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerrainImages {
    pub albedo: Option<String>,
    pub normal: Option<String>,
    pub occlusion: Option<String>,
    pub roughness: Option<String>,
    pub metalness: Option<String>,
    pub thumbnail: Option<String>,
}

impl TerrainImages {
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        [
            &self.albedo,
            &self.normal,
            &self.occlusion,
            &self.roughness,
            &self.metalness,
            &self.thumbnail,
        ]
        .into_iter()
        .flatten()
    }
}

/// Appearance of a body of water; course and elevation belong to map instances.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaterProfile {
    /// `#rrggbb`.
    pub shallow_color: String,
    pub deep_color: String,
    pub clarity: f64,
    pub ripple_scale: f64,
    pub ripple_strength: f64,
    pub foam_strength: f64,
    pub reflection_strength: f64,
    pub caustic_strength: f64,
    pub cloud_strength: f64,
    pub flow_speed: f64,
    pub waves: Option<WaterWaves>,
}

/// Visual swells that fade with water depth; they never alter navigation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaterWaves {
    pub height: f64,
    pub length: f64,
    pub speed: f64,
    pub depth_start: f64,
    pub depth_end: f64,
    pub shallow_strength: f64,
    pub shore_width: f64,
    pub crest_strength: f64,
}

/// One generation layer, discriminated by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LandscapeRecipe {
    Terrain(TerrainRecipe),
    River(RiverRecipe),
    Path(PathRecipe),
    Forest(ForestRecipe),
    Grass(Scatter),
    GroundCover(Scatter),
    Meadow(MeadowRecipe),
}

impl LandscapeRecipe {
    /// Every species this recipe scatters, in every pass.
    pub fn species(&self) -> Vec<&Species> {
        let mut out: Vec<&Species> = Vec::new();
        match self {
            Self::Terrain(r) => out.extend(r.chunks.iter().flat_map(|c| &c.species)),
            Self::River(r) => {
                if let Some(details) = &r.details {
                    out.extend(details.banks.iter().flat_map(|s| &s.species));
                    out.extend(details.water.iter().flat_map(|s| &s.species));
                    out.extend(details.shore.iter().flatten().flat_map(|s| &s.species));
                }
            }
            Self::Path(_) => {}
            Self::Forest(r) => {
                out.extend(&r.species);
                out.extend(r.details.iter().flatten().flat_map(|d| &d.species));
                out.extend(r.edge.iter().flat_map(|e| &e.species));
            }
            Self::Grass(r) | Self::GroundCover(r) => out.extend(&r.species),
            Self::Meadow(r) => out.extend(r.accents.iter().flatten().flat_map(|a| &a.species)),
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerrainOperation {
    Raise,
    Lower,
    Flatten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerrainSurface {
    Bank,
    Mountain,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerrainRecipe {
    pub operation: TerrainOperation,
    pub height: f64,
    pub falloff: f64,
    pub surface: Option<TerrainSurface>,
    pub roughness: Option<f64>,
    pub noise_scale: Option<f64>,
    pub grass_strength: Option<f64>,
    pub rock_strength: Option<f64>,
    pub chunks: Option<Scatter>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RiverRecipe {
    /// A water profile asset.
    pub water: AuthoringId,
    pub width: f64,
    pub depth: f64,
    pub bank_width: f64,
    /// Terrain material assets.
    pub bank_material: Option<AuthoringId>,
    pub bed_material: Option<AuthoringId>,
    pub flow: f64,
    pub max_uphill_grade: f64,
    pub details: Option<RiverDetails>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RiverDetails {
    pub banks: Option<Scatter>,
    pub water: Option<Scatter>,
    pub shore: Option<Vec<ShoreDetail>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathRecipe {
    /// A terrain material asset.
    pub material: AuthoringId,
    pub width: f64,
    pub shoulder: f64,
    pub flatten: f64,
    pub vegetation_clearance: f64,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MeadowRecipe {
    /// A terrain material asset.
    pub material: AuthoringId,
    pub canopy: f64,
    pub reach: f64,
    pub falloff: f64,
    pub under: f64,
    pub peak: f64,
    pub open: f64,
    pub edge_fade: f64,
    pub noise: Option<Noise>,
    pub ragged: Option<Ragged>,
    pub accents: Option<Vec<MeadowAccent>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Noise {
    pub scale: f64,
    pub strength: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ragged {
    pub width: f64,
    pub scale: f64,
    pub strength: f64,
}

/// A `min`–`max` band.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Band {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScatterPattern {
    Scattered,
    Patches,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Species {
    /// A model asset with a scenery binding.
    pub asset: AuthoringId,
    pub weight: f64,
}

/// Declares a scatter pass: the shared scatter fields plus the pass's own. (serde cannot
/// combine `flatten` with `deny_unknown_fields`.)
macro_rules! scatter {
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty,)* }) => {
        $(#[$meta])*
        #[skip_serializing_none]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            $($(#[$field_meta])* pub $field: $ty,)*
            pub density: Option<f64>,
            pub pattern: Option<ScatterPattern>,
            pub species: Vec<Species>,
            pub spacing: f64,
            pub probability: f64,
            pub jitter: f64,
            pub scale_min: f64,
            pub scale_max: f64,
            pub max_slope: f64,
            pub water_clearance: f64,
            pub object_clearance: f64,
            pub edge_fade: f64,
            pub min_spacing: Option<f64>,
            pub river_bank: Option<Band>,
            pub patchiness: Option<Noise>,
        }
    };
}

scatter!(
    /// Weighted species scattered over a region.
    Scatter {}
);

scatter!(
    /// A forest's main scatter plus its interior margin, detail passes and edge.
    ForestRecipe {
        interior_margin: f64,
        details: Option<Vec<ScatterPass>>,
        edge: Option<ForestEdge>,
    }
);

scatter!(
    /// An extra named scatter pass.
    ScatterPass { id: AuthoringId, }
);

scatter!(
    /// The scatter along a forest's border, `width` metres deep.
    ForestEdge { width: f64, }
);

scatter!(
    /// A scatter along a river's waterline: `bank` is metres from the edge, negative
    /// inside the water.
    ShoreDetail { id: AuthoringId, bank: Band, }
);

scatter!(
    /// A meadow scatter restricted to coverage and tree-distance bands.
    MeadowAccent {
        id: AuthoringId,
        coverage: Option<Band>,
        trees: Option<Band>,
    }
);

/// A ground material. Atlas tiles count row-major from the top-left; corner bits are
/// NW=1, NE=2, SW=4, SE=8.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroundTexture {
    pub tile_world_size: f64,
    pub projection: Option<Projection>,
    pub reflectance: f64,
    pub shading: Option<Shading>,
    pub source_ids: Vec<String>,
    pub albedo: ResourceRef,
    pub normal: ResourceRef,
    pub occlusion: ResourceRef,
    pub roughness: ResourceRef,
    pub metalness: ResourceRef,
    pub thumbnail: ResourceRef,
    pub runtime: Option<RuntimeTextures>,
    pub atlas: Atlas,
}

/// Cliff overlays project vertically and never replace top paint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Projection {
    #[serde(rename_all = "camelCase")]
    Cliff {
        vertical_world_size: f64,
        start_slope: f64,
        full_slope: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shading {
    Physical,
    Painted,
}

/// Packed runtime textures (`data` resources).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTextures {
    pub albedo_roughness: ResourceRef,
    pub normal_opacity: ResourceRef,
    pub occlusion_metalness: ResourceRef,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Atlas {
    pub columns: u32,
    pub rows: u32,
    pub full_tiles: Vec<u32>,
    /// 16 entries, one per corner mask; `None` has no tile.
    pub corners: Option<Vec<Option<u32>>>,
}

/// Reads `assets/authoring/catalogue.json`; ids are unique.
pub fn load(data: &Path, errors: &mut Vec<Error>) -> Option<Vec<LandscapeAsset>> {
    let assets: Vec<LandscapeAsset> = read(data, CATALOGUE_FILE, errors)?;
    unique(
        CATALOGUE_FILE,
        "",
        "asset",
        assets.iter().map(|a| a.id.as_str()),
        errors,
    );
    Some(assets)
}

/// Landscape rules against the published records and the resolved definitions
/// ([`crate::Content`]'s `manifest` and `definitions`): every entry is a
/// published record; scenery, species, materials, water profiles and terrain images
/// resolve; harvestable models name a resource definition and its render asset.
pub fn validate(
    assets: &[LandscapeAsset],
    manifest: &Manifest,
    definitions: &[Definition],
    errors: &mut Vec<Error>,
) {
    let render: BTreeSet<String> = manifest
        .records
        .iter()
        .flat_map(|r| &r.render)
        .map(|a| a.id.to_string())
        .collect();
    let resources: BTreeSet<&str> = definitions
        .iter()
        .filter(|d| d.kind == Kind::Resource)
        .map(|d| d.id.as_str())
        .collect();
    let records: BTreeSet<&str> = manifest.records.iter().map(|r| r.id.as_str()).collect();
    let scenery: BTreeSet<&str> = super::scenery(manifest).map(|s| s.id.as_str()).collect();
    let outputs = output_paths(manifest);
    let kinds: BTreeMap<&str, &LandscapeAsset> =
        assets.iter().map(|a| (a.id.as_str(), a)).collect();
    for asset in assets {
        let location = asset.id.to_string();
        let mut fail =
            |message: String| errors.push(Error::new(CATALOGUE_FILE, &location, message));
        if !records.contains(asset.id.as_str()) {
            fail("not a published asset record".into());
        }
        if let Some(id) = &asset.scenery
            && !scenery.contains(id.as_str())
        {
            fail(format!("unknown scenery {id}"));
        }
        for path in asset.terrain_images.iter().flat_map(TerrainImages::paths) {
            if !outputs.contains(path.as_str()) {
                fail(format!("terrain image {path} is not a published output"));
            }
        }
        if asset.terrain.is_some() != asset.terrain_images.is_some() {
            fail("terrain and terrainImages come together".into());
        }
        if let Some(harvesting) = &asset.harvesting {
            if !resources.contains(harvesting.definition.as_str()) {
                fail(format!(
                    "harvesting needs a resource definition ({})",
                    harvesting.definition
                ));
            }
            if !render.contains(&harvesting.asset) {
                fail(format!(
                    "harvesting needs a render asset ({:?})",
                    harvesting.asset
                ));
            }
        }
        let Some(recipe) = &asset.recipe else {
            continue;
        };
        // Materials without a ground texture are published records outside this index.
        let materials: Vec<&AuthoringId> = match recipe {
            LandscapeRecipe::River(river) => {
                if kinds
                    .get(river.water.as_str())
                    .is_none_or(|a| a.water.is_none())
                {
                    fail(format!("water {} must be a water profile", river.water));
                }
                [&river.bank_material, &river.bed_material]
                    .into_iter()
                    .flatten()
                    .collect()
            }
            LandscapeRecipe::Path(path) => vec![&path.material],
            _ => Vec::new(),
        };
        for material in materials {
            if !records.contains(material.as_str()) {
                fail(format!("material {material} is not a published asset"));
            }
        }
        // Meadow accents are not checked by publication either.
        if matches!(recipe, LandscapeRecipe::Meadow(_)) {
            continue;
        }
        for species in recipe.species() {
            if kinds
                .get(species.asset.as_str())
                .is_none_or(|a| a.scenery.is_none())
            {
                fail(format!("species {} needs a scenery model", species.asset));
            }
        }
    }
}
