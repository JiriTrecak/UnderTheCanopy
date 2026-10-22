//! Camera shots of mission scenes (docs/campaign.md "Scenes"): the map's saved cameras
//! (`mission.cameras`) and the shot presets on actors (`mission.shot`, framed by
//! [`super::presets`]) as views, and the eased blend between two of them, or between one
//! and the game's own camera, computed from the shot's start tick and length at the tick
//! the picture shows, so saves and replays frame the same way.

use canopy_content::map::MapCamera;
use canopy_render::{CameraView, RtsCamera, camera::Ground};
use canopy_sim::mission::cinema::{self, Pose, PresetKind, Side};
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

    /// The camera at `eye` looking at `target`, with a vertical field of view and roll
    /// (radians).
    pub fn looking(eye: Vec3, target: Vec3, fov: f32, roll: f32, ground: &dyn Ground) -> Self {
        let offset = eye - target;
        let distance = offset.length().max(0.01);
        Self {
            target: [target.x, target.z],
            lift: target.y - ground.height(target.x, target.z).max(0.0),
            distance,
            pitch: (offset.y / distance).clamp(-1.0, 1.0).asin(),
            yaw: offset.x.atan2(offset.z),
            fov,
            roll,
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

/// One end of a shot: a saved camera, a preset on actors, or the game's own camera
/// looking at a point (or wherever the player's camera looks).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Framing {
    Saved(Saved),
    Preset(Preset),
    Game(Option<[f32; 2]>),
}

/// Where a placed unit stands for the picture: its entity id and position (cells).
pub type Find<'a> = &'a dyn Fn(&str) -> Option<(u32, [f32; 2])>;

/// Frames a preset as the picture draws its actors: `None` when they are not drawn.
pub type Place<'a> = &'a mut dyn FnMut(&Preset) -> Option<Saved>;

impl Framing {
    /// `None` when the pose names a camera the map does not have. `find` gives the
    /// entities a preset's placements made.
    pub fn of(pose: &Pose, cameras: &[MapCamera], find: Find) -> Option<Self> {
        if let Some(preset) = &pose.preset {
            return Some(Self::Preset(Preset::of(preset, find)));
        }
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

    /// The point it looks at on the ground (cells), when it names one.
    pub fn target(&self) -> Option<[f32; 2]> {
        match self {
            Self::Saved(s) => Some(s.target),
            Self::Preset(p) => Some(p.at),
            Self::Game(t) => *t,
        }
    }

    fn saved(&self, rts: &RtsCamera, aspect: f32, place: Place) -> Saved {
        match *self {
            Self::Saved(s) => s,
            Self::Preset(p) => place(&p).unwrap_or_else(|| Saved::of_game(rts, Some(p.at), aspect)),
            Self::Game(target) => Saved::of_game(rts, target, aspect),
        }
    }
}

/// A shot preset (`mission.shot`) resolved on the simulation thread: its actors as the
/// entities they are, its options in world units and radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub kind: PresetKind,
    /// The actors' entities, in the call's order (none: not on the map).
    pub actors: [Option<u32>; 2],
    /// Where it looks when its actors are not drawn: the first actor, or the region's
    /// centre (cells).
    pub at: [f32; 2],
    /// A wide shot's region radius (cells).
    pub radius: Option<f32>,
    /// How far round `at` the picture must show the world (cells): all of a region, or
    /// out to the second actor.
    pub reach: f32,
    pub side: Option<Side>,
    pub distance: Option<f32>,
    pub height: Option<f32>,
    pub fov: Option<f32>,
    pub roll: f32,
    /// The framings that share a chosen side: a single's or a wide shot's own, a pair's
    /// (over and two, either way round) so a conversation keeps to one side of the line.
    pub key: u64,
    /// Whether its first actor comes first in its pair's placement order (a side kept for
    /// the pair is seen from that one).
    pub first: bool,
}

impl Preset {
    pub fn of(preset: &cinema::Preset, find: Find) -> Self {
        let found: Vec<_> = preset.actors.iter().map(|p| find(p)).collect();
        let actor = |i: usize| found.get(i).copied().flatten();
        let milli = |v: Option<u32>| v.map(|v| v as f32 / 1000.0);
        let at = actor(0).map(|(_, p)| p).or_else(|| {
            preset
                .region
                .map(|(c, _)| [c.x as f32 / 1000.0, c.y as f32 / 1000.0])
        });
        let pair = matches!(preset.kind, PresetKind::Over | PresetKind::Two);
        let mut names: Vec<&str> = preset.actors.iter().map(String::as_str).collect();
        if pair {
            names.sort_unstable();
        }
        let family = match preset.kind {
            PresetKind::Over | PresetKind::Two => "pair",
            PresetKind::Single => "single",
            PresetKind::Wide => "wide",
        };
        // FNV-1a over the family and the placements: the same in every run.
        let mut key = 0xcbf2_9ce4_8422_2325u64;
        for byte in std::iter::once(family)
            .chain(names.iter().copied())
            .flat_map(|n| n.bytes().chain(std::iter::once(0)))
        {
            key = (key ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        if let Some((c, _)) = preset.region {
            key ^= u64::from(c.x as u32).rotate_left(17) ^ u64::from(c.y as u32).rotate_left(41);
        }
        Self {
            kind: preset.kind,
            actors: [actor(0).map(|(id, _)| id), actor(1).map(|(id, _)| id)],
            at: at.unwrap_or_default(),
            radius: preset.region.map(|(_, r)| r as f32 / 1000.0),
            reach: match (actor(0), actor(1), preset.region) {
                (Some((_, a)), Some((_, b)), _) => (a[0] - b[0]).hypot(a[1] - b[1]) + 12.0,
                (_, _, Some((_, r))) => r as f32 / 500.0,
                _ => 0.0,
            },
            side: preset.side,
            distance: milli(preset.distance),
            height: preset.height.map(|v| v as f32 / 1000.0),
            fov: preset.fov.map(|v| (v as f32 / 1000.0).to_radians()),
            roll: (preset.roll.unwrap_or(0) as f32 / 1000.0).to_radians(),
            key,
            first: !pair || preset.actors.first().map(String::as_str) == names.first().copied(),
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
    pub fn resolve(shot: &cinema::Shot, cameras: &[MapCamera], find: Find) -> Option<Self> {
        Some(Self {
            from: Framing::of(&shot.from, cameras, find)?,
            to: Framing::of(&shot.to, cameras, find)?,
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
    /// game's ends, and `place` frames presets.
    pub fn at(&self, tick: f32, rts: &RtsCamera, aspect: f32, place: Place) -> Saved {
        let t = self.progress(tick);
        let to = self.to.saved(rts, aspect, place);
        if t >= 1.0 {
            return to;
        }
        self.from.saved(rts, aspect, place).blend(&to, t)
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
                ..Pose::default()
            },
            to: Pose {
                camera: Some("b".into()),
                ..Pose::default()
            },
            start: 100,
            ticks: 40,
        };
        let frames = Frames::resolve(&shot, &cameras, &|_| None).unwrap();
        let rts = RtsCamera::new([0.0, 0.0], None);
        let half = frames.at(120.0, &rts, 1.5, &mut |_| None);
        assert_eq!(half.target, [50.0, 50.0]);
        // The short way round, through north.
        assert!(half.yaw.to_degrees().rem_euclid(360.0) < 1e-3 || half.yaw.abs() < 1e-4);
        assert_eq!(
            frames.at(99.0, &rts, 1.5, &mut |_| None).target,
            [40.0, 50.0]
        );
        assert_eq!(
            frames.at(500.0, &rts, 1.5, &mut |_| None).target,
            [60.0, 50.0]
        );
        // Eased: slower at the ends than in the middle.
        let early = frames.at(104.0, &rts, 1.5, &mut |_| None).target[0] - 40.0;
        let middle = frames.at(122.0, &rts, 1.5, &mut |_| None).target[0]
            - frames.at(118.0, &rts, 1.5, &mut |_| None).target[0];
        assert!(early < middle, "{early} {middle}");
        assert!(
            Frames::resolve(
                &cinema::Shot {
                    to: Pose {
                        camera: Some("gone".into()),
                        ..Pose::default()
                    },
                    ..shot
                },
                &cameras,
                &|_| None
            )
            .is_none()
        );
    }
}
