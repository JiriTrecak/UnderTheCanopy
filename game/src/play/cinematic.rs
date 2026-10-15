//! Mission cinematics' camera (old `session.tick`, `Renderer.updateUnitCamera` and
//! `Camera.cinematic`): while a scripted scene or a cinematic line plays, the view
//! narrows its lens by 8 % (blending in and out with a 220 ms time constant) and looks at
//! the scene; a scene's unit shot (`mission.camera`) puts the camera behind or in the eyes
//! of a unit, optionally aimed at another, with the shot's boom, height, lens and blend.
//! An RTS-mode shot (or one whose unit is not drawn) looks at its unit, else at the
//! scene. The simulation thread resolves the shot's placements to entities
//! ([`crate::sim::SceneTargets`]) and reveals the scene's set ([`crate::sim::stage`]).

use super::follow::{self, CameraMode, Key, Pose, Shot, Subject};
use crate::sim::{SceneTarget, SceneTargets};
use canopy_render::CameraView;
use canopy_sim::mission::state::{Camera, CameraMode as ShotMode};
use glam::Vec3;

/// How much a cinematic narrows the RTS lens (old `1 − .08 · cinematicBlend`).
pub const LENS_NARROWING: f32 = 0.08;
/// Time constant of the lens blend, ms (old 220).
pub const LENS_TIME_MS: f32 = 220.0;

/// The cinematic lens between frames.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Lens {
    /// 0 outside cinematics, 1 inside.
    pub blend: f32,
}

impl Lens {
    /// Moves toward the cinematic lens (`on`) or back for a frame of `dt_ms`.
    pub fn update(&mut self, on: bool, dt_ms: f32) {
        let target = if on { 1.0 } else { 0.0 };
        self.blend += (target - self.blend) * (1.0 - (-dt_ms.max(0.0) / LENS_TIME_MS).exp());
        if (self.blend - target).abs() < 1e-4 {
            self.blend = target;
        }
    }

    /// The RTS view through the lens.
    pub fn apply(&self, view: CameraView) -> CameraView {
        if self.blend <= 0.0 {
            return view;
        }
        CameraView::look_at(
            view.eye,
            view.target,
            view.fov_y * (1.0 - LENS_NARROWING * self.blend),
            view.aspect,
            view.near,
            view.far,
        )
    }
}

/// The unit camera a shot asks for; `None` for an RTS shot.
pub fn mode(camera: &Camera) -> Option<CameraMode> {
    match camera.mode {
        ShotMode::Rts => None,
        ShotMode::ThirdPerson => Some(CameraMode::ThirdPerson),
        ShotMode::FirstPerson => Some(CameraMode::FirstPerson),
    }
}

/// A shot's overrides in world units, degrees and seconds (the simulation keeps
/// milli-units).
pub fn shot(camera: &Camera) -> Shot {
    let milli = |v: Option<u32>| v.map(|v| v as f32 / 1000.0);
    Shot {
        distance: milli(camera.distance),
        height: milli(camera.height),
        fov_degrees: milli(camera.fov),
        transition_seconds: camera.transition_ms as f32 / 1000.0,
    }
}

/// Where the RTS view looks during a scene: an RTS shot's unit, else the scene.
pub fn focus(scene: [f32; 2], camera: Option<&Camera>, targets: Option<&SceneTargets>) -> [f32; 2] {
    camera
        .and(targets)
        .and_then(|t| t.subject)
        .map_or(scene, |s| s.position)
}

/// The unit shot's key and pose: the subject as drawn (`subject` gives it for a target,
/// `None` when it is not drawn), aimed at the eye point of the unit looked at, the
/// third-person boom kept in front of the ground. `None` for RTS shots and undrawn units.
pub fn pose(
    camera: &Camera,
    targets: &SceneTargets,
    subject: impl Fn(&SceneTarget) -> Option<Subject>,
    ground: &dyn canopy_render::camera::Ground,
    obstruction: follow::Obstruction,
) -> Option<(Key, Pose)> {
    let mode = mode(camera)?;
    let target = targets.subject?;
    let s = subject(&target)?;
    let aim = targets.look_at.as_ref();
    let look_at = aim
        .and_then(&subject)
        .map(|a| a.position + Vec3::Y * a.eye_height);
    let shot = shot(camera);
    let pose = follow::ground_shot_pose(&s, mode, &shot, look_at, ground, obstruction)?;
    let key = Key {
        mode,
        entity: target.id,
        look_at: aim.map(|a| a.id),
        lens: [shot.fov_degrees, shot.height, shot.distance],
    };
    Some((key, pose))
}

/// A small mission on `base` (a skirmish map) that stages one scene at the first start:
/// the content's default starting hero (the first race's when the content has no default
/// hero) facing a rival of the same kind twelve cells north, a third-person shot from
/// behind the hero aimed at the rival, and a cinematic line. Every id comes from the
/// content and the map, so the same builder serves test-owned content and captures of
/// the shipped game.
#[cfg(test)]
pub fn scene_map(
    content: &canopy_content::Content,
    base: &canopy_content::map::UtcMap,
) -> anyhow::Result<canopy_content::map::UtcMap> {
    use anyhow::Context;
    use serde_json::json;
    let rules = &content.rules;
    let setup = rules
        .race(&rules.default_race)
        .map_or(&rules.starting_setup, |r| r.starting_setup);
    let hero = setup
        .hero
        .as_ref()
        .map(|h| h.default.clone())
        .context("a starting hero")?;
    let companion = setup
        .units
        .first()
        .map(|u| u.definition.clone())
        .context("a starting unit")?;
    let start = base.player_starts.first().context("a player start")?;
    let (x, y) = (start.x.round(), start.z.round());
    let mut document = serde_json::to_value(base)?;
    let mut entities = document["entities"].as_array().cloned().unwrap_or_default();
    let place = |id: &str, definition: &str, owner: &str, px: f64, py: f64, rotation: f64| {
        json!({"id": id, "definition": definition, "position": {"x": px, "y": py},
               "rotation": rotation, "owner": owner})
    };
    entities.push(place("scene.hero", hero.as_str(), "player.1", x, y, 180.0));
    entities.push(place(
        "scene.companion",
        companion.as_str(),
        "player.1",
        x + 2.0,
        y + 1.0,
        180.0,
    ));
    entities.push(place(
        "scene.rival",
        hero.as_str(),
        "player.2",
        x,
        y - 12.0,
        0.0,
    ));
    let script = format!(
        r#"function on_start()
  mission.objective("Meet the rival at the ford")
  mission.begin_scene({x}, {sy})
  mission.face("scene.hero", {x}, {ry})
  mission.camera("third-person", "scene.hero", "scene.rival", 8, nil, 50, 0)
  mission.say("Marshal", "{hero}", "There they are. Hold the line.", 8, true, "scene.hero")
end
"#,
        sy = y - 6.0,
        ry = y - 12.0,
    );
    document["entities"] = json!(entities);
    document["mission"] = json!({
        "campaign": rules.campaigns.keys().next().map_or("test", String::as_str),
        "title": "Scene test",
        "order": 1,
        "regions": [],
        "script": script,
    });
    canopy_content::map::UtcMap::from_value(document, "scene.utcmap")
        .map_err(|e| anyhow::anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{self, Seat, ai_fixture};
    use canopy_render::camera::FlatGround;
    use canopy_sim::{MatchSetup, Slot, SlotKind, World};

    fn humans() -> MatchSetup {
        MatchSetup {
            seed: 5,
            slots: (0..2)
                .map(|player| Slot {
                    player,
                    kind: SlotKind::Human,
                    name: None,
                    team: None,
                    hero: None,
                    race: None,
                })
                .collect(),
        }
    }

    /// The subject as a test draws it: at its position, facing the way it faces.
    fn drawn(world: &World, t: &SceneTarget) -> Option<Subject> {
        let e = world
            .state()
            .entities
            .get(canopy_sim::entity::EntityId(t.id))?;
        let yaw = (e.rotation as f32 / 1000.0).to_radians();
        Some(Subject {
            position: Vec3::new(t.position[0], 0.0, t.position[1]),
            yaw,
            eye_height: 1.5,
            eye_forward: 0.2,
            distance: 7.0,
        })
    }

    /// A test-owned mission stages a scene with `begin_scene`, `face`, `camera` and `say`:
    /// the simulation resolves the shot to entities, the picture reveals the scene's set
    /// without telling the player's view, and the camera stands behind the hero looking
    /// at the rival through the shot's lens.
    #[test]
    fn a_scripted_scene_frames_its_units() {
        let content = ai_fixture::content();
        let map = scene_map(&content, &ai_fixture::skirmish(vec![], vec![])).unwrap();
        let mut world = World::new(content, &map, humans()).unwrap();
        for _ in 0..3 {
            world.step();
        }
        let view = Seat::Player(0).view(&world);
        let m = view.mission.as_ref().expect("a mission");
        assert_eq!(m.error, None);
        let scene = m.scene.as_ref().expect("a scene");
        let camera = scene.camera.as_ref().expect("a shot");
        assert_eq!(mode(camera), Some(CameraMode::ThirdPerson));
        assert!(
            m.dialogue
                .as_ref()
                .is_some_and(|d| d.cinematic && d.remaining > 0)
        );

        let targets = sim::scene_targets(&world, &view).expect("targets");
        let (hero, rival) = (targets.subject.unwrap(), targets.look_at.unwrap());
        assert_ne!(hero.id, rival.id);
        assert!((hero.position[1] - rival.position[1] - 12.0).abs() < 1.0);

        // The set is revealed for the picture only.
        let staged = sim::stage(&world, &view).expect("staged");
        let fog = view.fog.as_ref().expect("the player's fog");
        let staged_fog = staged.fog.as_ref().unwrap();
        let [sx, sy] = [
            scene.position.x as f32 / 1000.0,
            scene.position.y as f32 / 1000.0,
        ];
        let cell = |x: f32, y: f32| (y as u32 * fog.size + x as u32) as usize;
        let far = cell(sx + 30.0, sy);
        assert_ne!(fog.cells[far], canopy_sim::vision::VISIBLE, "out of sight");
        assert_eq!(staged_fog.cells[far], canopy_sim::vision::VISIBLE);
        let outside = cell(sx + 40.0, sy);
        assert_eq!(staged_fog.cells[outside], fog.cells[outside]);
        assert_ne!(staged_fog.revision, fog.revision);
        assert!(staged.entities.len() >= view.entities.len());
        assert!(staged.entities.windows(2).all(|w| w[0].id() < w[1].id()));

        // Behind the hero, the boom and lens of the shot, aimed at the rival's eyes.
        let (key, pose) = pose(
            camera,
            &targets,
            |t| drawn(&world, t),
            &FlatGround,
            &follow::open_air,
        )
        .unwrap();
        assert_eq!(key.entity, hero.id);
        assert_eq!(key.look_at, Some(rival.id));
        assert_eq!(pose.fov_degrees, 50.0);
        assert_eq!(pose.transition_seconds, 0.0);
        let rival_eye = Vec3::new(rival.position[0], 1.5, rival.position[1]);
        assert!((pose.target - rival_eye).length() < 1e-4);
        let boom = pose.eye - pose.focus;
        assert!(
            (boom.length() - 8.0 * 1.16f32.sqrt()).abs() < 0.3,
            "{boom:?}"
        );
        // The hero faces the rival (north, −z), so the eye is south of it.
        assert!(pose.eye.z > pose.focus.z, "{:?} {:?}", pose.eye, pose.focus);

        // An RTS view during the scene looks at the shot's unit through a narrower lens.
        assert_eq!(focus([sx, sy], Some(camera), Some(&targets)), hero.position);
        assert_eq!(focus([sx, sy], None, Some(&targets)), [sx, sy]);
        let mut lens = Lens::default();
        let rts = canopy_render::RtsCamera::new([sx, sy], None).view(16.0 / 9.0, &FlatGround);
        lens.update(true, LENS_TIME_MS);
        assert!((lens.blend - (1.0 - (-1.0f32).exp())).abs() < 1e-5);
        for _ in 0..60 {
            lens.update(true, 50.0);
        }
        assert_eq!(lens.blend, 1.0);
        assert!((lens.apply(rts).fov_y - rts.fov_y * 0.92).abs() < 1e-6);
        for _ in 0..60 {
            lens.update(false, 50.0);
        }
        assert_eq!(lens.apply(rts), rts);
    }

    /// Writes the scene mission on a shipped map for `canopy shot --map <file>`:
    /// `cargo test -p canopy -- --ignored write_the_scene_capture_map`.
    #[test]
    #[ignore = "reads the shipped data; writes target/scene-capture.utcmap"]
    fn write_the_scene_capture_map() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let content = canopy_content::Content::load(&root).unwrap();
        let base =
            canopy_content::map::UtcMap::load(&root.join("assets/maps/skirmish/echo-isles.utcmap"))
                .unwrap();
        let map = scene_map(&content, &base).unwrap();
        let out = root.join("target/scene-capture.utcmap");
        std::fs::write(&out, serde_json::to_vec(&map).unwrap()).unwrap();
        canopy_content::map::UtcMap::load(&out).unwrap();
    }
}
