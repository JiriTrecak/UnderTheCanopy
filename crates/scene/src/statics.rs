//! The static scene of a match: what never moves (old `PropField.place` and
//! `presentation/scenery`). Everything else is the live scene ([`crate::live`]).
//!
//! - **Stamps** (the map's stamps and scenery objects that are not resources) stand at
//!   the cell's centre `(x + 0.5, y + 0.5)` on the bilinear ground plus their elevation, or
//!   at an absolute height; rotation is `Rz(roll)·Rx(pitch)·Ry(yaw)` (or the authored
//!   quaternion), scale per local axis.
//! - **Standing trees** (resources with a scenery model) stand at their cell, turned by
//!   their rotation and scaled by their appearance. Every tree of the map gets an instance;
//!   the live scene hides it once it is struck or gone ([`StaticScene::tree_slots`]).
//!
//! Buildings, deposits, units and items are live: they are built, deplete, change owner
//! and move.

use canopy_content::{
    Content,
    assets::{self, Deck},
    game::Kind,
    map::{Stamp, UtcMap},
    placement::Placement,
};
use canopy_render::{frame::PointLight, statics::StaticInstance};
use canopy_sim::{
    entity::{EntityId, Milli},
    terrain::HeightField,
    world::World,
};
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

/// Scenery prototypes by id: model entry and light.
pub type SceneryModels = BTreeMap<String, canopy_assets::SceneryPrototype>;

/// A prototype's light placed with its instance (old `sceneryLights`: the light offset
/// turns and scales with the stamp).
pub fn placed_light(light: &canopy_assets::PointLight, transform: &Mat4) -> PointLight {
    let scale = transform.x_axis.truncate().length();
    PointLight {
        position: transform
            .transform_point3(Vec3::from_array(light.position))
            .to_array(),
        range: light.range * scale,
        color: (Vec3::from_array(light.color) * light.intensity).to_array(),
        pad: 0.0,
    }
}

/// The static scene of a match.
#[derive(Debug, Clone, Default)]
pub struct StaticScene {
    pub instances: Vec<StaticInstance>,
    pub lights: Vec<PointLight>,
    /// What could not be shown (missing models), for warnings.
    pub missing: Vec<String>,
    /// The instance index of each tree entity, for hiding felled ones.
    pub tree_slots: BTreeMap<EntityId, usize>,
}

/// Every static instance of a freshly started match.
pub fn build(
    world: &World,
    content: &Content,
    map: &UtcMap,
    scenery: &SceneryModels,
) -> StaticScene {
    let _span = tracing::info_span!("scene_statics").entered();
    let mut scene = StaticScene::default();
    let terrain = world.terrain();
    let field: &HeightField = terrain.field();
    let ground = |x: f64, z: f64| field.sample(x, z);
    let decks: BTreeMap<&str, &Deck> = assets::scenery(&content.manifest)
        .filter_map(|e| Some((e.id.as_str(), e.deck.as_ref()?)))
        .collect();
    for stamp in &terrain.scene().stamps {
        match scenery.get(&stamp.asset) {
            Some(prototype) => {
                let transform =
                    stamp_transform(stamp, ground, decks.get(stamp.asset.as_str()).copied());
                if let Some(light) = &prototype.light {
                    scene.lights.push(placed_light(light, &transform));
                }
                scene.instances.push(StaticInstance {
                    model: prototype.model.clone(),
                    transform,
                    owner_color: None,
                });
            }
            None => scene.missing.push(format!("scenery {}", stamp.asset)),
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
        if definition.kind != Kind::Resource || entity.resource.is_none() {
            continue;
        }
        let appearance = entity
            .placement
            .as_deref()
            .and_then(|id| placements.get(id))
            .and_then(|p| p.appearance.as_ref());
        let appearance_scale = appearance.and_then(|a| a.scale).unwrap_or(1.0);
        let asset_id = appearance
            .and_then(|a| a.asset.as_ref())
            .unwrap_or(&definition.asset);
        let Some(asset) = content.asset(asset_id.as_str()) else {
            scene.missing.push(format!("render asset {asset_id}"));
            continue;
        };
        let Some(scenery_id) = &asset.scenery_asset else {
            continue;
        };
        let degrees = f64::from(entity.rotation) / 1000.0;
        match scenery.get(scenery_id) {
            Some(prototype) => {
                let standing = entity.resource.as_ref().is_some_and(|r| {
                    r.amount > 0 && r.felling.as_ref().is_none_or(|f| f.last_hit_tick.is_none())
                });
                scene.tree_slots.insert(entity.id, scene.instances.len());
                if !standing {
                    scene
                        .missing
                        .push(format!("tree {} starts felled", entity.id.0));
                }
                scene.instances.push(StaticInstance {
                    model: prototype.model.clone(),
                    transform: standing_transform(
                        entity.position,
                        degrees,
                        appearance_scale,
                        ground,
                    ),
                    owner_color: None,
                });
            }
            None => scene.missing.push(format!("scenery {scenery_id}")),
        }
    }
    scene
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
