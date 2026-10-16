//! Unit cameras (old `shared/camera/modes.ts`, `render/camera/unitCamera.ts` and the
//! close pose of `camera.ts`): the RTS view, or a third- or first-person camera locked
//! to the focused unit, cycled by the content's camera command (J) and left with Escape.
//!
//! The pose is a pure function of the unit as the frame draws it (feet, facing, scale)
//! and its render asset's camera anchor; a pose change (mode or unit) blends from the
//! last shown view over 350 ms with smoothstep, then the camera tracks the unit hard.

use canopy_render::CameraView;
use glam::{Mat4, Quat, Vec3};

/// Which camera the player looks through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CameraMode {
    #[default]
    Rts,
    ThirdPerson,
    FirstPerson,
}

impl CameraMode {
    /// The next mode (RTS → third person → first person → RTS).
    pub fn next(self) -> Self {
        match self {
            Self::Rts => Self::ThirdPerson,
            Self::ThirdPerson => Self::FirstPerson,
            Self::FirstPerson => Self::Rts,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Rts => "RTS",
            Self::ThirdPerson => "Third person",
            Self::FirstPerson => "First person",
        }
    }
}

/// Upward aim added to the third-person view (old `THIRD_PERSON_LOOK_UP`).
const THIRD_PERSON_LOOK_UP: f32 = 10.0 * std::f32::consts::PI / 180.0;
/// Blend from the previous view when the pose changes (old default `transitionMs`).
pub const TRANSITION_SECONDS: f32 = 0.35;
/// Close cameras' planes (old near 0.08, far 180).
pub const NEAR: f32 = 0.08;
pub const FAR: f32 = 180.0;

/// The unit a close camera follows, as drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subject {
    pub position: Vec3,
    /// Facing about +Y; forward is `(sin yaw, 0, cos yaw)`.
    pub yaw: f32,
    pub eye_height: f32,
    pub eye_forward: f32,
    pub distance: f32,
}

impl Subject {
    /// From the model's transform, the asset's camera anchor and the definition's visual
    /// scale (old `cameraSubject`: anchor height, else 0.65 × the health bar height (at
    /// least 0.65); forward 0; distance 7; all × the definition's `modelScale`, not the
    /// model's own unit scale).
    pub fn of(
        transform: Mat4,
        anchor: Option<&canopy_content::game::CameraAnchor>,
        health_height: Option<f64>,
        visual_scale: f32,
    ) -> Self {
        let (_, rotation, position) = transform.to_scale_rotation_translation();
        let forward = rotation * Vec3::Z;
        let s = visual_scale.abs().max(1e-3);
        let height = anchor.map_or_else(
            || (health_height.unwrap_or(2.5) as f32 * 0.65).max(0.65),
            |a| a.height as f32,
        );
        Self {
            position,
            yaw: forward.x.atan2(forward.z),
            eye_height: height * s,
            eye_forward: anchor.map_or(0.0, |a| a.forward as f32) * s,
            distance: anchor.map_or(7.0, |a| a.distance as f32) * s,
        }
    }
}

/// A close pose: eye, aim, the followed focus, the vertical field of view and how long
/// a change to it blends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub eye: Vec3,
    pub target: Vec3,
    pub focus: Vec3,
    pub fov_degrees: f32,
    pub transition_seconds: f32,
}

/// What a mission's camera shot overrides (old `UnitShot`): boom length, eye height,
/// field of view, all in world units and degrees, and the blend time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub distance: Option<f32>,
    pub height: Option<f32>,
    pub fov_degrees: Option<f32>,
    pub transition_seconds: f32,
}

impl Default for Shot {
    /// The player's unit camera: the unit's own anchor, the 350 ms blend.
    fn default() -> Self {
        Self {
            distance: None,
            height: None,
            fov_degrees: None,
            transition_seconds: TRANSITION_SECONDS,
        }
    }
}

/// Which pose a view blends to: a new key starts a blend (old `ClosePose.key`: the mode,
/// the unit, the unit looked at and the shot's lens).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub mode: CameraMode,
    pub entity: u32,
    pub look_at: Option<u32>,
    pub lens: [Option<f32>; 3],
}

impl Key {
    /// The player's unit camera on `entity`.
    pub fn unit(mode: CameraMode, entity: u32) -> Self {
        Self {
            mode,
            entity,
            look_at: None,
            lens: [None; 3],
        }
    }
}

/// The close pose of `mode` (old `unitCameraPose`); `None` for the RTS view.
#[cfg(test)]
pub fn pose(subject: &Subject, mode: CameraMode) -> Option<Pose> {
    shot_pose(subject, mode, &Shot::default(), None)
}

/// The close pose of `mode` with a shot's overrides, aimed at `look_at` (the eye point
/// of the unit looked at) if given (old `unitCameraPose`).
pub fn shot_pose(
    subject: &Subject,
    mode: CameraMode,
    shot: &Shot,
    look_at: Option<Vec3>,
) -> Option<Pose> {
    let forward = Vec3::new(subject.yaw.sin(), 0.0, subject.yaw.cos());
    let focus = subject.position + Vec3::Y * shot.height.unwrap_or(subject.eye_height);
    let first = mode == CameraMode::FirstPerson;
    let eye = match mode {
        CameraMode::Rts => return None,
        CameraMode::FirstPerson => focus + forward * subject.eye_forward,
        CameraMode::ThirdPerson => {
            let d = shot.distance.unwrap_or(subject.distance);
            focus - forward * d + Vec3::Y * (d * 0.4)
        }
    };
    let mut target = look_at.unwrap_or(focus + forward * if first { 8.0 } else { 2.0 });
    if !first && look_at.is_none() {
        // Shows more of the canopy without moving the boom or changing authored shots.
        let horizontal = (target.x - eye.x).hypot(target.z - eye.z);
        let pitch = (target.y - eye.y).atan2(horizontal) + THIRD_PERSON_LOOK_UP;
        target.y = eye.y + pitch.tan() * horizontal;
    }
    if target.distance_squared(eye) < 0.001 {
        target += forward;
    }
    Some(Pose {
        eye,
        target,
        focus,
        fov_degrees: shot.fov_degrees.unwrap_or(if first { 68.0 } else { 55.0 }),
        transition_seconds: shot.transition_seconds,
    })
}

/// How far a ray from `origin` along a unit direction runs before scenery or a building
/// stops it, up to `far` ([`crate::live::LiveScene::camera_obstruction`]).
pub type Obstruction<'a> = &'a dyn Fn(Vec3, Vec3, f32) -> f32;

/// Nothing stops a ray.
#[cfg(test)]
pub fn open_air(_: Vec3, _: Vec3, far: f32) -> f32 {
    far
}

/// Pulls the third-person boom in front of scenery, buildings (`obstruction`, from 5 cm
/// off the focus) and the ground (old renderer: samples every 0.15 along the boom, 0.2
/// clearance, the eye 0.25 short of the obstruction and at least 0.1 from the focus).
pub fn clear_boom(pose: &mut Pose, ground: impl Fn(f32, f32) -> f32, obstruction: Obstruction) {
    let offset = pose.eye - pose.focus;
    let distance = offset.length();
    if distance <= 0.0 {
        return;
    }
    let direction = offset / distance;
    let near = 0.05;
    let mut allowed = near + obstruction(pose.focus + direction * near, direction, distance - near);
    let mut d = 0.15;
    while d < allowed {
        let p = pose.focus + direction * d;
        if p.y < ground(p.x, p.z) + 0.2 {
            allowed = d;
            break;
        }
        d += 0.15;
    }
    pose.eye = pose.focus + direction * (allowed - 0.25).max(0.1);
}

/// The rotation looking from `eye` at `target` (camera −Z towards the target).
fn look_rotation(eye: Vec3, target: Vec3) -> Quat {
    let d = (target - eye).normalize_or(Vec3::NEG_Z);
    let yaw = (-d.x).atan2(-d.z);
    let pitch = d.y.clamp(-1.0, 1.0).asin();
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)
}

/// The subject of a close camera: the focused unit as the live scene draws it, with its
/// render asset's anchor.
pub fn subject(
    live: &crate::live::LiveScene,
    know: &super::know::Knowledge,
    id: u32,
) -> Option<Subject> {
    subject_of(live, &know.content, know.def_of(id)?, id)
}

/// The subject of a close camera: entity `id` of definition `d` as the live scene draws
/// it (old `cameraSubject`).
pub fn subject_of(
    live: &crate::live::LiveScene,
    content: &canopy_content::Content,
    d: &canopy_content::game::Definition,
    id: u32,
) -> Option<Subject> {
    let transform = live.entity_transform(id)?;
    let asset = content.look(d.id.as_str());
    Some(Subject::of(
        transform,
        asset.and_then(|a| a.camera_anchor.as_ref()),
        asset.and_then(|a| a.health_height),
        d.model_scale.unwrap_or(1.0) as f32,
    ))
}

/// The close pose of the focused unit in `mode`, the third-person boom kept in front
/// of the ground, scenery and buildings.
pub fn ground_pose(
    subject: &Subject,
    mode: CameraMode,
    ground: &dyn canopy_render::camera::Ground,
    obstruction: Obstruction,
) -> Option<Pose> {
    ground_shot_pose(subject, mode, &Shot::default(), None, ground, obstruction)
}

/// [`shot_pose`] with the third-person boom kept in front of the ground.
pub fn ground_shot_pose(
    subject: &Subject,
    mode: CameraMode,
    shot: &Shot,
    look_at: Option<Vec3>,
    ground: &dyn canopy_render::camera::Ground,
    obstruction: Obstruction,
) -> Option<Pose> {
    let mut pose = shot_pose(subject, mode, shot, look_at)?;
    if mode == CameraMode::ThirdPerson {
        clear_boom(&mut pose, |x, z| ground.height(x, z), obstruction);
    }
    Some(pose)
}

/// What the close camera shows over time.
#[derive(Debug, Clone, Default)]
pub struct Follow {
    /// The pose being blended to.
    key: Option<Key>,
    from: Option<(Vec3, Quat, f32)>,
    elapsed: f32,
    /// The last view shown (eye, rotation, vertical fov in radians).
    last: Option<(Vec3, Quat, f32)>,
}

impl Follow {
    /// Remembers the RTS view shown (the start of the next blend).
    pub fn shown(&mut self, view: &CameraView) {
        self.key = None;
        self.last = Some((view.eye, look_rotation(view.eye, view.target), view.fov_y));
    }

    /// The close view for `pose` (keyed by `key`) after `dt` seconds.
    pub fn view(&mut self, key: Key, pose: &Pose, dt: f32, aspect: f32) -> CameraView {
        if self.key != Some(key) {
            self.key = Some(key);
            self.from = self.last;
            self.elapsed = 0.0;
        }
        self.elapsed += dt.max(0.0);
        let to_rotation = look_rotation(pose.eye, pose.target);
        let to_fov = pose.fov_degrees.to_radians();
        let t = if pose.transition_seconds > 0.0 {
            (self.elapsed / pose.transition_seconds).min(1.0)
        } else {
            1.0
        };
        let blend = t * t * (3.0 - 2.0 * t);
        let (eye, rotation, fov) = match self.from {
            Some((eye, rotation, fov)) if blend < 1.0 => (
                eye.lerp(pose.eye, blend),
                rotation.slerp(to_rotation, blend),
                fov + (to_fov - fov) * blend,
            ),
            _ => (pose.eye, to_rotation, to_fov),
        };
        self.last = Some((eye, rotation, fov));
        let ahead = eye + rotation * Vec3::NEG_Z * pose.eye.distance(pose.target).max(1.0);
        CameraView::look_at(eye, ahead, fov, aspect, NEAR, FAR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject() -> Subject {
        Subject {
            position: Vec3::new(10.0, 0.0, 20.0),
            yaw: 0.0,
            eye_height: 1.5,
            eye_forward: 0.2,
            distance: 7.0,
        }
    }

    #[test]
    fn modes_cycle_and_poses_follow_the_old_rules() {
        assert_eq!(CameraMode::Rts.next(), CameraMode::ThirdPerson);
        assert_eq!(CameraMode::FirstPerson.next(), CameraMode::Rts);
        let s = subject();
        assert!(pose(&s, CameraMode::Rts).is_none());
        let first = pose(&s, CameraMode::FirstPerson).unwrap();
        assert!((first.eye - Vec3::new(10.0, 1.5, 20.2)).length() < 1e-5);
        assert!((first.target - Vec3::new(10.0, 1.5, 28.0)).length() < 1e-5);
        assert_eq!(first.fov_degrees, 68.0);
        let third = pose(&s, CameraMode::ThirdPerson).unwrap();
        // Behind (−forward × 7) and above (0.4 × 7) the eye point.
        assert!((third.eye - Vec3::new(10.0, 1.5 + 2.8, 13.0)).length() < 1e-5);
        // Aimed 10° above the old trailing view.
        let old = (1.5 - 4.3f32).atan2(9.0);
        let now = (third.target.y - third.eye.y).atan2(third.target.z - third.eye.z);
        assert!((now - old - THIRD_PERSON_LOOK_UP).abs() < 1e-4);
        assert_eq!(third.fov_degrees, 55.0);
    }

    #[test]
    fn the_boom_stops_in_front_of_rising_ground() {
        let s = subject();
        let mut p = pose(&s, CameraMode::ThirdPerson).unwrap();
        let free = p.eye;
        clear_boom(&mut p, |_, _| -10.0, &open_air);
        assert!((p.eye - p.focus).length() < (free - p.focus).length());
        let mut p = pose(&s, CameraMode::ThirdPerson).unwrap();
        // A wall of ground behind the unit.
        clear_boom(&mut p, |_, z| if z < 18.0 { 10.0 } else { 0.0 }, &open_air);
        assert!(p.eye.z >= 17.5, "{:?}", p.eye);
        // Scenery 1 m behind the focus stops it 0.25 short.
        let mut p = pose(&s, CameraMode::ThirdPerson).unwrap();
        clear_boom(&mut p, |_, _| -10.0, &|_, _, far: f32| far.min(0.95));
        assert!(
            ((p.eye - p.focus).length() - 0.75).abs() < 1e-4,
            "{:?}",
            p.eye
        );
    }

    #[test]
    fn a_new_pose_blends_from_the_last_view_then_tracks() {
        let s = subject();
        let p = pose(&s, CameraMode::FirstPerson).unwrap();
        let mut f = Follow::default();
        let rts = CameraView::look_at(
            Vec3::new(10.0, 60.0, -20.0),
            Vec3::new(10.0, 0.0, 20.0),
            0.6,
            1.6,
            0.5,
            500.0,
        );
        f.shown(&rts);
        let key = Key::unit(CameraMode::FirstPerson, 1);
        let start = f.view(key, &p, 0.0, 1.6);
        assert!((start.eye - rts.eye).length() < 1e-3);
        let mid = f.view(key, &p, 0.175, 1.6);
        assert!(mid.eye.distance(p.eye) < rts.eye.distance(p.eye));
        let end = f.view(key, &p, 1.0, 1.6);
        assert!((end.eye - p.eye).length() < 1e-4);
        assert!((end.fov_y - 68f32.to_radians()).abs() < 1e-5);
    }
}
