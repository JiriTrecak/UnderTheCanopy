//! Two-snapshot interpolation (D24, Q15): the render thread shows the live scene one tick
//! behind the simulation, between the two presentations around its render tick.
//!
//! Models present in both are blended: positions and scales linearly, rotations along the
//! shorter arc, opacity linearly, and poses clip by clip (looping clips wrap; a one-shot
//! that restarted takes the newer time). Models that appear, jump (`snap`) or change model
//! show the newer tick as is; models that disappear are gone.

use crate::{animation::Poses, live::Presentation};
use canopy_render::actors::{ActorInstance, MAX_POSES, Pose};
use glam::{Mat4, Quat};

/// The poses between `a` and `b`.
pub fn blend_poses(a: &Poses, b: &Poses, t: f32) -> Poses {
    let mut out = Poses::default();
    let mut n = 0;
    for (i, pb) in b.used().iter().enumerate() {
        let length = b.loops[i];
        let pose = match a.used().iter().position(|pa| pa.clip == pb.clip) {
            Some(j) => {
                let pa = a.poses[j];
                let mut tb = pb.time;
                let time = if length > 0.0 {
                    if tb < pa.time {
                        tb += length;
                    }
                    (pa.time + (tb - pa.time) * t) % length
                } else if tb < pa.time {
                    tb
                } else {
                    pa.time + (tb - pa.time) * t
                };
                Pose {
                    clip: pb.clip,
                    time,
                    weight: pa.weight + (pb.weight - pa.weight) * t,
                }
            }
            None => Pose {
                weight: pb.weight * t.max(1e-3),
                ..*pb
            },
        };
        out.poses[n] = pose;
        out.loops[n] = length;
        n += 1;
    }
    // Poses that ended between the ticks fade out over the interval.
    for (j, pa) in a.used().iter().enumerate() {
        if n == MAX_POSES {
            break;
        }
        if !b.used().iter().any(|pb| pb.clip == pa.clip) {
            out.poses[n] = Pose {
                weight: pa.weight * (1.0 - t),
                ..*pa
            };
            out.loops[n] = a.loops[j];
            n += 1;
        }
    }
    out.count = n as u8;
    let total: f32 = out.used().iter().map(|p| p.weight).sum();
    if total > 0.0 {
        for p in &mut out.poses[..n] {
            p.weight /= total;
        }
    }
    out
}

/// The instances to draw at `t` (0 = `a`, 1 = `b`) between two presentations, `a` older.
/// Without `a` the newer one is shown as is.
pub fn interpolate(
    a: Option<&Presentation>,
    b: &Presentation,
    t: f32,
    out: &mut Vec<ActorInstance>,
) {
    let _span = tracing::trace_span!("interpolate").entered();
    out.clear();
    out.reserve(b.actors.len());
    let t = t.clamp(0.0, 1.0);
    let older = a.map_or(&[][..], |a| &a.actors[..]);
    let mut j = 0;
    for actor in &b.actors {
        while j < older.len() && older[j].key < actor.key {
            j += 1;
        }
        let previous = older
            .get(j)
            .filter(|p| p.key == actor.key && p.model == actor.model && !actor.snap);
        let (position, rotation, scale, opacity, poses) = match previous {
            Some(p) => {
                let mut ra = p.rotation;
                if ra.dot(actor.rotation) < 0.0 {
                    ra = -ra;
                }
                (
                    p.position.lerp(actor.position, t),
                    Quat::from_xyzw(
                        ra.x + (actor.rotation.x - ra.x) * t,
                        ra.y + (actor.rotation.y - ra.y) * t,
                        ra.z + (actor.rotation.z - ra.z) * t,
                        ra.w + (actor.rotation.w - ra.w) * t,
                    )
                    .normalize(),
                    p.scale.lerp(actor.scale, t),
                    p.opacity + (actor.opacity - p.opacity) * t,
                    blend_poses(&p.poses, &actor.poses, t),
                )
            }
            None => (
                actor.position,
                actor.rotation,
                actor.scale,
                actor.opacity,
                actor.poses,
            ),
        };
        out.push(ActorInstance {
            model: actor.model,
            transform: Mat4::from_scale_rotation_translation(scale, rotation, position),
            owner_color: actor.color,
            opacity,
            poses: poses.poses,
            pose_count: poses.count,
        });
    }
}

/// Where the render clock stands between presentations: the render tick (simulation time
/// in ticks, one tick behind) picks the pair around it and the fraction between them.
/// `ticks` are the presentations' ticks, oldest first. Returns `(older, newer, t)` as
/// indices, clamped to the newest (no extrapolation) and the oldest.
pub fn bracket(ticks: &[u32], render_tick: f64) -> Option<(usize, usize, f32)> {
    let last = ticks.len().checked_sub(1)?;
    if render_tick >= f64::from(ticks[last]) || last == 0 {
        return Some((last, last, 1.0));
    }
    if render_tick <= f64::from(ticks[0]) {
        return Some((0, 0, 1.0));
    }
    let i = ticks
        .windows(2)
        .position(|w| render_tick < f64::from(w[1]))?;
    let (a, b) = (f64::from(ticks[i]), f64::from(ticks[i + 1]));
    Some((i, i + 1, ((render_tick - a) / (b - a)) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{Actor, Part, key};
    use glam::Vec3;

    fn actor(id: u32, x: f32, yaw: f32) -> Actor {
        let mut a = Actor {
            key: key(Part::Entity, 0, id),
            model: 0,
            position: Vec3::new(x, 0.0, 0.0),
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::ONE,
            color: None,
            opacity: 1.0,
            poses: Poses::single(0, 0.0),
            snap: false,
            body: None,
        };
        a.poses.loops[0] = 1.0;
        a
    }

    fn presentation(tick: u32, actors: Vec<Actor>) -> Presentation {
        Presentation {
            tick,
            actors,
            ..Default::default()
        }
    }

    #[test]
    fn positions_and_rotations_blend_along_the_short_way() {
        let a = presentation(1, vec![actor(1, 0.0, 3.0), actor(2, 5.0, 0.0)]);
        let b = presentation(2, vec![actor(1, 2.0, -3.0), actor(3, 9.0, 0.0)]);
        let mut out = Vec::new();
        interpolate(Some(&a), &b, 0.25, &mut out);
        // Unit 2 vanished; unit 3 appears at its new place.
        assert_eq!(out.len(), 2);
        let p = out[0].transform.transform_point3(Vec3::ZERO);
        assert!((p.x - 0.5).abs() < 1e-5);
        // 3 rad → −3 rad crosses ±π: a quarter of the 0.28 rad short arc, not of 6 rad.
        let forward = out[0].transform.transform_vector3(Vec3::Z);
        let angle = forward.x.atan2(forward.z);
        let expected = 3.0 + (2.0 * std::f32::consts::PI - 6.0) * 0.25;
        let wrapped = (angle - expected + std::f32::consts::PI)
            .rem_euclid(2.0 * std::f32::consts::PI)
            - std::f32::consts::PI;
        assert!(wrapped.abs() < 1e-3, "{angle} {expected}");
        assert_eq!(out[1].transform.transform_point3(Vec3::ZERO).x, 9.0);
    }

    #[test]
    fn snaps_and_model_changes_do_not_blend() {
        let a = presentation(1, vec![actor(1, 0.0, 0.0)]);
        let mut jumped = actor(1, 50.0, 0.0);
        jumped.snap = true;
        let b = presentation(2, vec![jumped]);
        let mut out = Vec::new();
        interpolate(Some(&a), &b, 0.5, &mut out);
        assert_eq!(out[0].transform.transform_point3(Vec3::ZERO).x, 50.0);
        let mut other = actor(1, 2.0, 0.0);
        other.model = 7;
        interpolate(Some(&a), &presentation(2, vec![other]), 0.5, &mut out);
        assert_eq!(out[0].transform.transform_point3(Vec3::ZERO).x, 2.0);
        interpolate(None, &a, 0.5, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn poses_wrap_looping_clips_and_restart_one_shots() {
        let mut a = Poses::single(0, 0.9);
        a.loops[0] = 1.0;
        let mut b = Poses::single(0, 0.1);
        b.loops[0] = 1.0;
        let p = blend_poses(&a, &b, 0.5);
        assert!((p.poses[0].time - 0.0).abs() < 1e-5 || (p.poses[0].time - 1.0).abs() < 1e-5);
        let p = blend_poses(&a, &b, 0.25);
        assert!((p.poses[0].time - 0.95).abs() < 1e-5);
        // A one-shot that started again takes the new time.
        let one = Poses::single(0, 0.9);
        let again = Poses::single(0, 0.1);
        assert_eq!(blend_poses(&one, &again, 0.5).poses[0].time, 0.1);
        // A fade-in blends with the fade-out of the clip that ended between the ticks.
        let mut fading = Poses::single(1, 0.3);
        fading.poses[0].weight = 1.0;
        let p = blend_poses(&fading, &Poses::single(2, 0.0), 0.5);
        assert_eq!(p.count, 2);
        let total: f32 = p.used().iter().map(|p| p.weight).sum();
        assert!((total - 1.0).abs() < 1e-5);
        assert_eq!(p.poses[1].clip, 1);
    }

    #[test]
    fn the_render_tick_brackets_two_presentations() {
        let ticks = [10, 11, 12];
        assert_eq!(bracket(&ticks, 10.5), Some((0, 1, 0.5)));
        assert_eq!(bracket(&ticks, 11.0), Some((1, 2, 0.0)));
        // Ahead of the newest: hold it (no extrapolation); behind the oldest: the oldest.
        assert_eq!(bracket(&ticks, 14.0), Some((2, 2, 1.0)));
        assert_eq!(bracket(&ticks, 3.0), Some((0, 0, 1.0)));
        // A gap of several ticks still interpolates linearly.
        assert_eq!(bracket(&[10, 14], 11.0), Some((0, 1, 0.25)));
        assert_eq!(bracket(&[], 1.0), None);
        assert_eq!(bracket(&[5], 1.0), Some((0, 0, 1.0)));
    }
}
