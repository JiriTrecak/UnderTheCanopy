//! Saved-camera shots of mission scenes (docs/campaign.md "Scenes"): the map's cameras
//! (`mission.cameras`) as views, and the eased blend between two of them, or between one
//! and the game's own camera, computed from the shot's start tick and length at the tick
//! the picture shows, so saves and replays frame the same way.

use canopy_content::map::MapCamera;
use canopy_render::{CameraView, RtsCamera, camera::Ground};
use canopy_sim::mission::cinema::{self, Pose};
use glam::{Mat4, Quat, Vec3};

/// Near and far planes of a saved camera's view (close shots and long ones alike).
const NEAR: f32 = 0.3;
const FAR: f32 = 900.0;
/// How far a saved camera's eye keeps above the ground under it.
const CLEARANCE: f32 = 0.6;

/// A camera's parameters in world units and radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Saved {
    /// The ground point looked at (x, z).
    pub target: [f32; 2],
    /// The target's lift above the ground.
    pub lift: f32,
    pub distance: f32,
    /// Below the horizon.
    pub pitch: f32,
    /// 0 looks north from the south.
    pub yaw: f32,
    /// Vertical.
    pub fov: f32,
    pub roll: f32,
}

impl Saved {
    /// A map camera, looking at `target` instead when panned (milli-cells).
    pub fn of(camera: &MapCamera, target: Option<canopy_sim::entity::Milli>) -> Self {
        let target = target.map_or([camera.x as f32, camera.y as f32], |t| {
            [t.x as f32 / 1000.0, t.y as f32 / 1000.0]
        });
        Self {
            target,
            lift: camera.height.unwrap_or(0.0) as f32,
            distance: camera.distance as f32,
            pitch: (camera.pitch as f32).to_radians(),
            yaw: (camera.yaw as f32).to_radians(),
            fov: (camera.fov as f32).to_radians(),
            roll: (camera.roll.unwrap_or(0.0) as f32).to_radians(),
        }
    }

    /// The game's camera as these parameters, looking at `target` (or its own focus).
    pub fn of_game(rts: &RtsCamera, target: Option<[f32; 2]>, aspect: f32) -> Self {
        Self {
            target: target.unwrap_or(rts.focus),
            lift: 0.0,
            distance: rts.distance,
            pitch: rts.pitch,
            yaw: rts.yaw,
            fov: rts.fov_y(aspect),
            roll: 0.0,
        }
    }

    /// The view, the eye kept above the ground.
    pub fn view(&self, aspect: f32, ground: &dyn Ground) -> CameraView {
        let [x, z] = self.target;
        let target = Vec3::new(x, ground.height(x, z).max(0.0) + self.lift, z);
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let mut eye = target + Vec3::new(sy * cp, sp, cy * cp) * self.distance;
        let floor = ground.height(eye.x, eye.z).max(0.0) + CLEARANCE;
        let floor = ground
            .water(eye.x, eye.z)
            .map_or(floor, |w| floor.max(w + CLEARANCE));
        eye.y = eye.y.max(floor);
        let mut view = CameraView::look_at(eye, target, self.fov, aspect, NEAR, FAR);
        if self.roll != 0.0 {
            let forward = (target - eye).normalize_or_zero();
            let up = Quat::from_axis_angle(forward, -self.roll) * Vec3::Y;
            view.view = Mat4::look_at_rh(eye, target, up);
            view.view_projection = view.projection * view.view;
        }
        view
    }

    /// Part way (`t`, 0 to 1) from `self` to `other`: positions and angles along a line,
    /// the distance in proportion, the yaw the short way round.
    pub fn blend(&self, other: &Self, t: f32) -> Self {
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        let turn = std::f32::consts::TAU;
        let mut yaw = (other.yaw - self.yaw).rem_euclid(turn);
        if yaw > turn / 2.0 {
            yaw -= turn;
        }
        Self {
            target: [
                lerp(self.target[0], other.target[0]),
                lerp(self.target[1], other.target[1]),
            ],
            lift: lerp(self.lift, other.lift),
            distance: (lerp(self.distance.max(0.01).ln(), other.distance.max(0.01).ln())).exp(),
            pitch: lerp(self.pitch, other.pitch),
            yaw: self.yaw + yaw * t,
            fov: lerp(self.fov, other.fov),
            roll: lerp(self.roll, other.roll),
        }
    }
}

/// One end of a shot: a saved camera, or the game's own camera looking at a point (or
/// wherever the player's camera looks).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Framing {
    Saved(Saved),
    Game(Option<[f32; 2]>),
}

impl Framing {
    /// `None` when the pose names a camera the map does not have.
    pub fn of(pose: &Pose, cameras: &[MapCamera]) -> Option<Self> {
        match &pose.camera {
            Some(id) => {
                let camera = cameras.iter().find(|c| &c.id == id)?;
                Some(Self::Saved(Saved::of(camera, pose.target)))
            }
            None => Some(Self::Game(
                pose.target
                    .map(|t| [t.x as f32 / 1000.0, t.y as f32 / 1000.0]),
            )),
        }
    }

    fn saved(&self, rts: &RtsCamera, aspect: f32) -> Saved {
        match *self {
            Self::Saved(s) => s,
            Self::Game(target) => Saved::of_game(rts, target, aspect),
        }
    }
}

/// A mission's shot resolved against its map's cameras (on the simulation thread).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frames {
    pub from: Framing,
    pub to: Framing,
    pub start: u32,
    pub ticks: u32,
}

impl Frames {
    pub fn resolve(shot: &cinema::Shot, cameras: &[MapCamera]) -> Option<Self> {
        Some(Self {
            from: Framing::of(&shot.from, cameras)?,
            to: Framing::of(&shot.to, cameras)?,
            start: shot.start,
            ticks: shot.ticks,
        })
    }

    /// The blend's eased progress at `tick`.
    pub fn progress(&self, tick: f32) -> f32 {
        let shot = cinema::Shot {
            from: Pose::default(),
            to: Pose::default(),
            start: self.start,
            ticks: self.ticks,
        };
        cinema::ease(shot.progress(tick))
    }

    /// Where the shot has the camera at `tick`; the game camera `rts` stands in for the
    /// game's ends.
    pub fn at(&self, tick: f32, rts: &RtsCamera, aspect: f32) -> Saved {
        let t = self.progress(tick);
        let to = self.to.saved(rts, aspect);
        if t >= 1.0 {
            return to;
        }
        self.from.saved(rts, aspect).blend(&to, t)
    }

    /// The blend back to the game's camera is over: the player has it again, looking at
    /// the returned point.
    pub fn returned(&self, tick: f32) -> Option<Option<[f32; 2]>> {
        match self.to {
            Framing::Game(target) if self.progress(tick) >= 1.0 => Some(target),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_render::camera::FlatGround;

    fn camera(id: &str, x: f64, yaw: f64) -> MapCamera {
        MapCamera {
            id: id.into(),
            x,
            y: 50.0,
            height: Some(1.0),
            distance: 20.0,
            pitch: 30.0,
            yaw,
            fov: 40.0,
            roll: None,
        }
    }

    #[test]
    fn saved_cameras_frame_their_target_and_blend_smoothly() {
        let cameras = [camera("a", 40.0, 350.0), camera("b", 60.0, 10.0)];
        let view = Saved::of(&cameras[0], None).view(16.0 / 9.0, &FlatGround);
        assert_eq!(view.target, Vec3::new(40.0, 1.0, 50.0));
        assert!(((view.eye - view.target).length() - 20.0).abs() < 1e-3);
        // Yaw 350° stands the eye a little west of due south.
        assert!(view.eye.z > 50.0 && view.eye.x < 40.0);
        let shot = cinema::Shot {
            from: Pose {
                camera: Some("a".into()),
                target: None,
            },
            to: Pose {
                camera: Some("b".into()),
                target: None,
            },
            start: 100,
            ticks: 40,
        };
        let frames = Frames::resolve(&shot, &cameras).unwrap();
        let rts = RtsCamera::new([0.0, 0.0], None);
        let half = frames.at(120.0, &rts, 1.5);
        assert_eq!(half.target, [50.0, 50.0]);
        // The short way round, through north.
        assert!(half.yaw.to_degrees().rem_euclid(360.0) < 1e-3 || half.yaw.abs() < 1e-4);
        assert_eq!(frames.at(99.0, &rts, 1.5).target, [40.0, 50.0]);
        assert_eq!(frames.at(500.0, &rts, 1.5).target, [60.0, 50.0]);
        // Eased: slower at the ends than in the middle.
        let early = frames.at(104.0, &rts, 1.5).target[0] - 40.0;
        let middle = frames.at(122.0, &rts, 1.5).target[0] - frames.at(118.0, &rts, 1.5).target[0];
        assert!(early < middle, "{early} {middle}");
        assert!(
            Frames::resolve(
                &cinema::Shot {
                    to: Pose {
                        camera: Some("gone".into()),
                        target: None
                    },
                    ..shot
                },
                &cameras
            )
            .is_none()
        );
    }
}
