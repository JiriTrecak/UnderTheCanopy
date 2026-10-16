//! The landscape catalogue the map scene is generated from, built from packages
//! ([`super::package`]): placeable models and scenery, ground textures and water profiles.

use super::{AuthoringId, Manifest, ResourceRef, output_paths};
use crate::{
    error::Error,
    game::{Definition, Kind},
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeSet;

/// One published asset that takes part in landscape generation.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    pub water: Option<WaterProfile>,
    pub terrain: Option<GroundTexture>,
    pub terrain_images: Option<TerrainImages>,
}

/// `ASSET_KINDS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
    Icon,
    Interface,
    Texture,
    Audio,
    Map,
    Data,
}

/// A harvestable model: generated instances become `definition` with render asset `asset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Harvesting {
    pub definition: AuthoringId,
    /// The record's render asset with a `sceneryAsset`; empty when it has none.
    pub asset: String,
}

/// Published image paths of a ground texture (`Partial<Record<…, string>>`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

/// A ground material. Atlas tiles count row-major from the top-left; corner bits are
/// NW=1, NE=2, SW=4, SE=8.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Projection {
    #[serde(rename_all = "camelCase")]
    Cliff {
        vertical_world_size: f64,
        start_slope: f64,
        full_slope: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Shading {
    Physical,
    Painted,
}

/// Packed runtime textures (`data` resources).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeTextures {
    pub albedo_roughness: ResourceRef,
    pub normal_opacity: ResourceRef,
    pub occlusion_metalness: ResourceRef,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Atlas {
    pub columns: u32,
    pub rows: u32,
    pub full_tiles: Vec<u32>,
    /// 16 entries, one per corner mask; `None` has no tile.
    pub corners: Option<Vec<Option<u32>>>,
}

/// Landscape rules against the published records and the resolved definitions
/// ([`crate::Content`]'s `manifest` and `definitions`): scenery and terrain images resolve;
/// harvestable models name a resource definition and its render asset.
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
    let scenery: BTreeSet<&str> = super::scenery(manifest).map(|s| s.id.as_str()).collect();
    let outputs = output_paths(manifest);
    for asset in assets {
        let location = asset.id.to_string();
        let mut fail = |message: String| errors.push(Error::new("landscape", &location, message));
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
    }
}
