//! Placed models of a match as static render instances (old `PropField.place`,
//! `presentation/scenery` and `settlementLayer`).
//!
//! - **Stamps** (the map's stamps and scenery objects that are not resources) stand at
//!   the cell's centre `(x + 0.5, y + 0.5)` on the bilinear ground plus their elevation, or
//!   at an absolute height; rotation is `Rz(roll)·Rx(pitch)·Ry(yaw)` (or the authored
//!   quaternion), scale per local axis.
//! - **Standing trees** (resources with a scenery model) stand at their cell, turned by
//!   their rotation and scaled by their appearance.
//! - **Buildings and deposits** stand at their footprint centre on the ground, turned by
//!   their rotation, scaled by the render asset × appearance × definition scale. Deposits
//!   show their depletion stage.

use canopy_content::{
    Content,
    assets::{self, Deck},
    game::Kind,
    map::{Stamp, UtcMap},
    placement::{Owner, Placement},
};
use canopy_render::statics::StaticInstance;
use canopy_sim::{entity::Milli, terrain::HeightField, world::World};
use glam::{EulerRot, Mat4, Quat, Vec3};
use std::collections::BTreeMap;

/// Where a stamp's model goes.
pub fn stamp_transform(
    stamp: &Stamp,
    ground: impl Fn(f64, f64) -> f64,
    deck: Option<&Deck>,
) -> Mat4 {
    let s = stamp.scale.unwrap_or(1.0);
    let (ws, hs, ds) = (
        stamp.width_scale.unwrap_or(1.0),
        stamp.height_scale.unwrap_or(1.0),
        stamp.depth_scale.unwrap_or(1.0),
    );
    let (x, z) = (stamp.x + 0.5, stamp.y + 0.5);
    let y = if let Some(source) = &stamp.source_transform {
        source.height
    } else if let (Some(height), Some(deck)) = (stamp.walk.as_ref().and_then(|w| w.height), deck) {
        height - (deck.height + deck.arch + deck.rise.unwrap_or(0.0) / 2.0) * s * hs
    } else {
        ground(x, z) + stamp.elevation.unwrap_or(0.0)
    };
    let rotation = match &stamp.source_transform {
        Some(source) => {
            let [qx, qy, qz, qw] = source.quaternion.map(|v| v as f32);
            Quat::from_xyzw(qx, qy, qz, qw).normalize()
        }
        None => Quat::from_euler(
            EulerRot::ZXY,
            stamp.roll.unwrap_or(0.0) as f32,
            stamp.pitch.unwrap_or(0.0) as f32,
            stamp.yaw.unwrap_or(0.0) as f32,
        ),
    };
    Mat4::from_scale_rotation_translation(
        Vec3::new((s * ws) as f32, (s * hs) as f32, (s * ds) as f32),
        rotation,
        Vec3::new(x as f32, y as f32, z as f32),
    )
}

/// A model standing at `position` (cells) turned by `degrees` about y.
pub fn standing_transform(
    position: Milli,
    degrees: f64,
    scale: f64,
    ground: impl Fn(f64, f64) -> f64,
) -> Mat4 {
    let x = f64::from(position.x) / f64::from(Milli::CELL);
    let z = f64::from(position.y) / f64::from(Milli::CELL);
    Mat4::from_scale_rotation_translation(
        Vec3::splat(scale as f32),
        Quat::from_rotation_y(degrees.to_radians() as f32),
        Vec3::new(x as f32, ground(x, z) as f32, z as f32),
    )
}

/// Pack entry of a scenery prototype's model.
pub type SceneryModels = BTreeMap<String, String>;

/// Every static instance of a freshly started match. `colors` are the player colours
/// (linear), indexed by player slot.
pub fn build(
    world: &World,
    content: &Content,
    map: &UtcMap,
    scenery: &SceneryModels,
    colors: &[[f32; 3]],
) -> (Vec<StaticInstance>, Vec<String>) {
    let _span = tracing::info_span!("scene_statics").entered();
    let mut out = Vec::new();
    let mut missing = Vec::new();
    let terrain = world.terrain();
    let field: &HeightField = terrain.field();
    let ground = |x: f64, z: f64| field.sample(x, z);
    let decks: BTreeMap<&str, &Deck> = assets::scenery(&content.manifest)
        .filter_map(|e| Some((e.id.as_str(), e.deck.as_ref()?)))
        .collect();
    for stamp in &terrain.scene().stamps {
        match scenery.get(&stamp.asset) {
            Some(model) => out.push(StaticInstance {
                model: model.clone(),
                transform: stamp_transform(stamp, ground, decks.get(stamp.asset.as_str()).copied()),
                owner_color: None,
            }),
            None => missing.push(format!("scenery {}", stamp.asset)),
        }
    }

    let placements: BTreeMap<&str, &Placement> = map
        .entities
        .iter()
        .chain(&terrain.scene().resources)
        .map(|p| (p.id.as_str(), p))
        .collect();
    let catalog = world.catalog();
    for entity in world.state().entities.iter() {
        let definition = catalog.get(entity.definition);
        if matches!(definition.kind, Kind::Unit | Kind::Item) || entity.fallen {
            continue;
        }
        let appearance = entity
            .placement
            .as_deref()
            .and_then(|id| placements.get(id))
            .and_then(|p| p.appearance.as_ref());
        let appearance_scale = appearance.and_then(|a| a.scale).unwrap_or(1.0);
        let full = appearance
            .and_then(|a| a.asset.as_ref())
            .unwrap_or(&definition.asset);
        // Depletion stages replace the full model (old `resourceDepositVisual`).
        let mut asset_id = full.as_str();
        if let (Some(visuals), Some(resource)) = (&definition.resource_visuals, &entity.resource) {
            let initial = resource
                .initial_amount
                .or(definition.resource_yield)
                .unwrap_or(0);
            let fraction = if resource.amount == 0 {
                0.0
            } else if initial > 0 {
                f64::from(resource.amount) / f64::from(initial)
            } else {
                1.0
            };
            if let Some(stage) = visuals.stages.iter().find(|s| fraction <= s.remaining) {
                asset_id = stage.asset.as_str();
            }
        }
        let Some(asset) = content.asset(asset_id) else {
            missing.push(format!("render asset {asset_id}"));
            continue;
        };
        let degrees = f64::from(entity.rotation) / 1000.0;
        let owner_color = match entity.owner {
            Owner::Player(slot) => colors.get(usize::from(slot) % colors.len().max(1)).copied(),
            Owner::Nobody => None,
        };
        if let Some(scenery_id) = &asset.scenery_asset {
            // A tree: standing ones only (felled ones animate, next step).
            let standing = entity.resource.as_ref().is_some_and(|r| {
                r.amount > 0 && r.felling.as_ref().is_none_or(|f| f.last_hit_tick.is_none())
            });
            if !standing {
                continue;
            }
            match scenery.get(scenery_id) {
                Some(model) => out.push(StaticInstance {
                    model: model.clone(),
                    transform: standing_transform(
                        entity.position,
                        degrees,
                        appearance_scale,
                        ground,
                    ),
                    owner_color,
                }),
                None => missing.push(format!("scenery {scenery_id}")),
            }
            continue;
        }
        let Some(file) = &asset.file else {
            missing.push(format!("render asset {asset_id} has no model"));
            continue;
        };
        let scale =
            asset.scale.unwrap_or(1.0) * appearance_scale * definition.model_scale.unwrap_or(1.0);
        out.push(StaticInstance {
            model: file.clone(),
            transform: standing_transform(entity.position, degrees, scale, ground),
            owner_color,
        });
    }
    (out, missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_content::map::SourceTransform;

    fn stamp() -> Stamp {
        Stamp {
            id: "s".into(),
            locked: None,
            asset: "a".into(),
            walk: None,
            source_transform: None,
            x: 10.0,
            y: 20.0,
            yaw: None,
            pitch: None,
            roll: None,
            scale: None,
            height_scale: None,
            width_scale: None,
            depth_scale: None,
            elevation: None,
            variant: None,
        }
    }

    #[test]
    fn stamps_stand_at_cell_centres_on_the_ground() {
        let mut s = stamp();
        s.elevation = Some(1.5);
        let m = stamp_transform(&s, |x, z| x + z, None);
        assert_eq!(m.transform_point3(Vec3::ZERO), Vec3::new(10.5, 32.5, 20.5));
        // Yaw turns local +z towards +x, scales apply on local axes.
        s.yaw = Some(std::f64::consts::FRAC_PI_2);
        s.width_scale = Some(2.0);
        s.scale = Some(3.0);
        let m = stamp_transform(&s, |_, _| 0.0, None);
        let forward = m.transform_vector3(Vec3::Z);
        assert!((forward - Vec3::new(3.0, 0.0, 0.0)).length() < 1e-5);
        let side = m.transform_vector3(Vec3::X);
        assert!((side - Vec3::new(0.0, 0.0, -6.0)).length() < 1e-5);
    }

    #[test]
    fn absolute_heights_and_quaternions_override_the_ground() {
        let mut s = stamp();
        let half = 0.3f64;
        s.source_transform = Some(SourceTransform {
            packed_user_data: None,
            height: 7.0,
            quaternion: [0.0, half.sin(), 0.0, half.cos()],
        });
        s.elevation = Some(100.0);
        let m = stamp_transform(&s, |_, _| 50.0, None);
        assert_eq!(m.transform_point3(Vec3::ZERO).y, 7.0);
        let forward = m.transform_vector3(Vec3::Z);
        assert!((forward.x - (0.6f32).sin()).abs() < 1e-5);
    }

    #[test]
    fn pitch_follows_yaw_and_roll_comes_last() {
        let mut s = stamp();
        s.yaw = Some(std::f64::consts::FRAC_PI_2);
        s.pitch = Some(std::f64::consts::FRAC_PI_2);
        let m = stamp_transform(&s, |_, _| 0.0, None);
        // Ry first: +z → +x; then Rx leaves +x alone.
        assert!((m.transform_vector3(Vec3::Z) - Vec3::X).length() < 1e-5);
        // Up: Ry keeps +y; Rx(90°) turns it to +z.
        assert!((m.transform_vector3(Vec3::Y) - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn bridges_sit_their_deck_on_the_walk_height() {
        let mut s = stamp();
        s.walk = Some(canopy_content::map::Walk {
            mesh: None,
            level: 1,
            height: Some(10.0),
            connections: None,
        });
        s.scale = Some(2.0);
        let deck = Deck {
            width: 4.0,
            depth: 10.0,
            height: 1.0,
            arch: 0.5,
            thickness: None,
            rise: Some(1.0),
            level: 1,
            connections: None,
        };
        let m = stamp_transform(&s, |_, _| 0.0, Some(&deck));
        assert_eq!(m.transform_point3(Vec3::ZERO).y, 10.0 - 2.0 * 2.0);
    }

    #[test]
    fn standing_models_use_cell_positions_and_degrees() {
        let m = standing_transform(
            Milli {
                x: 12_500,
                y: 3_000,
            },
            90.0,
            2.0,
            |_, _| 1.0,
        );
        assert_eq!(m.transform_point3(Vec3::ZERO), Vec3::new(12.5, 1.0, 3.0));
        assert!((m.transform_vector3(Vec3::Z) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5);
    }
}
