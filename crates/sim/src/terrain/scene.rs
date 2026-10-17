//! Placed scenery objects projected into the match (old `projectMapObjects` and
//! `projectObject`, `src/shared/authoring/mapScene.ts`).
//!
//! A harvestable object standing on the terrain inside the playable area becomes a
//! resource placement on its cell; the first such object on a cell wins and later ones
//! vanish. Everything else (including harvestable objects in the halo) becomes a stamp
//! after the map's own stamps.

use crate::math;
use canopy_content::{
    Id,
    assets::landscape::LandscapeAsset,
    map::{AuthoredObject, HeightMode, SourceTransform, Stamp, UtcMap},
    placement::{Appearance, Owner, Placement, Point},
};
use std::{collections::BTreeMap, f64::consts::PI};

/// What the map's scenery contributes to the match.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    /// The map's stamps, then stamps projected from scenery objects in object order.
    pub stamps: Vec<Stamp>,
    /// Resource placements from harvestable objects, in object order.
    pub resources: Vec<Placement>,
}

/// Projects the map's visible scenery objects. Fails on an object whose asset has no
/// published scenery binding.
pub fn project(map: &UtcMap, landscape: &[LandscapeAsset]) -> Result<Scene, String> {
    let index: BTreeMap<&str, &LandscapeAsset> =
        landscape.iter().map(|a| (a.id.as_str(), a)).collect();
    let size = map.size as usize;
    let mut taken = vec![false; size * size];
    let mut scene = Scene {
        stamps: map.stamps.clone(),
        resources: Vec::new(),
    };
    for object in map.authoring.objects.iter().filter(|o| o.visible) {
        let asset = index
            .get(object.asset.as_str())
            .filter(|a| a.scenery.is_some())
            .ok_or_else(|| format!("Asset {} has no published scenery binding", object.asset))?;
        match resource_cell(object, asset, map) {
            Some(cell) => {
                if !std::mem::replace(&mut taken[cell], true) {
                    scene
                        .resources
                        .push(resource(object, asset, cell % size, cell / size)?);
                }
            }
            None => scene.stamps.push(stamp(object, asset)),
        }
    }
    Ok(scene)
}

/// The playable cell a harvestable, terrain-standing object occupies; outside the map's
/// playable area it is scenery only (a stamp), as nobody reaches it.
fn resource_cell(object: &AuthoredObject, asset: &LandscapeAsset, map: &UtcMap) -> Option<usize> {
    if asset.harvesting.is_none() || object.height_mode != HeightMode::Terrain {
        return None;
    }
    let x = (object.x + 0.5).floor();
    let y = (object.z + 0.5).floor();
    let size = f64::from(map.size);
    (x >= 0.0 && y >= 0.0 && x < size && y < size && map.playable_area().contains(x + 0.5, y + 0.5))
        .then_some((y * size + x) as usize)
}

fn resource(
    object: &AuthoredObject,
    asset: &LandscapeAsset,
    x: usize,
    y: usize,
) -> Result<Placement, String> {
    let harvesting = asset.harvesting.as_ref().expect("harvestable");
    let definition = Id::new(harvesting.definition.as_str())?;
    let appearance_asset = if harvesting.asset.is_empty() {
        None
    } else {
        Some(Id::new(harvesting.asset.as_str())?)
    };
    Ok(Placement {
        activation: None,
        id: object.id.clone(),
        map_knowledge: None,
        definition,
        position: Point {
            x: x as f64,
            y: y as f64,
            surface: None,
        },
        rotation: object.yaw * 180.0 / PI,
        owner: Owner::Nobody,
        initial_state: None,
        appearance: Some(Appearance {
            asset: appearance_asset,
            scale: Some(object.scale),
        }),
    })
}

fn stamp(object: &AuthoredObject, asset: &LandscapeAsset) -> Stamp {
    let absolute = object.height_mode == HeightMode::Absolute;
    let half = object.yaw / 2.0;
    Stamp {
        id: object.id.clone(),
        locked: None,
        asset: asset
            .scenery
            .as_ref()
            .expect("scenery binding")
            .as_str()
            .to_owned(),
        walk: None,
        source_transform: absolute.then(|| SourceTransform {
            packed_user_data: None,
            height: object.elevation,
            quaternion: [0.0, math::sin(half), 0.0, math::cos(half)],
        }),
        x: object.x,
        y: object.z,
        yaw: Some(object.yaw),
        pitch: None,
        roll: None,
        scale: Some(object.scale),
        height_scale: None,
        width_scale: None,
        depth_scale: None,
        elevation: (!absolute).then_some(object.elevation),
        variant: None,
    }
}
