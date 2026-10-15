//! Picking on the CPU (old `unitPicking.ts`, `renderer.pickGameEntity`, `pickGround`):
//! a screen-space capsule from each unit's feet to its head, the ground under the
//! cursor by marching the camera ray over the terrain, and the box test of box
//! selection. CPU picking needs no GPU read-back (no frame of latency) and works on the
//! interpolated positions the frame shows.

use canopy_render::camera::{CameraView, Ground};
use glam::{Vec2, Vec3, Vec4};

/// Smallest pick half-width of a unit, logical pixels (old 14, 28 px wide).
pub const MIN_PICK_RADIUS: f32 = 14.0;
/// Extra pick padding around the projected body, logical pixels.
pub const PICK_PADDING: f32 = 7.0;

/// A pickable body as drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub id: u32,
    /// Feet, world units.
    pub foot: Vec3,
    /// Head height above the feet.
    pub height: f32,
    /// Body radius, world units.
    pub radius: f32,
    /// Units pick by capsule; buildings and the rest by the ground point.
    pub unit: bool,
}

/// The view a frame was drawn with: camera and target size in physical pixels.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    pub camera: CameraView,
    pub size: [f32; 2],
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

impl Screen {
    /// Physical pixel position of a world point, and its clip depth; `None` behind the
    /// camera.
    pub fn project(&self, p: Vec3) -> Option<(Vec2, f32)> {
        let c = self.camera.view_projection * Vec4::new(p.x, p.y, p.z, 1.0);
        if c.w <= 1e-4 {
            return None;
        }
        let ndc = c.truncate() / c.w;
        Some((
            Vec2::new(
                (ndc.x * 0.5 + 0.5) * self.size[0],
                (0.5 - ndc.y * 0.5) * self.size[1],
            ),
            ndc.z,
        ))
    }

    /// The camera ray through a physical pixel: origin and unit direction.
    pub fn ray(&self, px: Vec2) -> (Vec3, Vec3) {
        let ndc = Vec2::new(
            px.x / self.size[0].max(1.0) * 2.0 - 1.0,
            1.0 - px.y / self.size[1].max(1.0) * 2.0,
        );
        let inverse = self.camera.view_projection.inverse();
        // Reverse-Z: depth 1 is the near plane, 0 the far one.
        let unproject = |z: f32| {
            let p = inverse * Vec4::new(ndc.x, ndc.y, z, 1.0);
            p.truncate() / p.w
        };
        let near = unproject(1.0);
        let far = unproject(0.0);
        (near, (far - near).normalize_or_zero())
    }

    /// Where the ray through `px` meets the ground (the first crossing, refined).
    pub fn ground(&self, px: Vec2, ground: &dyn Ground) -> Option<Vec3> {
        let (origin, dir) = self.ray(px);
        if dir.y >= -1e-4 {
            return None;
        }
        let step = 0.5;
        let max = self.camera.far.max(64.0) * 1.5;
        let above = |t: f32| {
            let p = origin + dir * t;
            p.y - ground.height(p.x, p.z).max(0.0)
        };
        let mut t0 = 0.0;
        let mut t = step;
        while t < max {
            if above(t) <= 0.0 {
                let (mut lo, mut hi) = (t0, t);
                for _ in 0..20 {
                    let mid = 0.5 * (lo + hi);
                    if above(mid) > 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                return Some(origin + dir * hi);
            }
            t0 = t;
            t += step;
        }
        None
    }

    /// The unit whose padded capsule contains `px`, nearest by perpendicular distance,
    /// then depth, then id.
    pub fn pick_unit(&self, bodies: &[Body], px: Vec2) -> Option<u32> {
        let mut best: Option<(f32, f32, u32)> = None;
        for b in bodies.iter().filter(|b| b.unit) {
            let Some((foot, depth)) = self.project(b.foot) else {
                continue;
            };
            let Some((head, _)) = self.project(b.foot + Vec3::Y * b.height) else {
                continue;
            };
            // Projected body radius at the feet.
            let side = self
                .project(b.foot + self.camera_right() * b.radius)
                .map_or(0.0, |(p, _)| p.distance(foot));
            let radius = (MIN_PICK_RADIUS * self.scale).max(side + PICK_PADDING * self.scale);
            let d = segment_distance(px, foot, head);
            if d > radius {
                continue;
            }
            // Reverse-Z: a larger depth is nearer.
            let key = (d, -depth, b.id);
            if best.is_none_or(|(bd, bz, bid)| key < (bd, bz, bid)) {
                best = Some(key);
            }
        }
        best.map(|(_, _, id)| id)
    }

    fn camera_right(&self) -> Vec3 {
        let forward = (self.camera.target - self.camera.eye).normalize_or_zero();
        forward.cross(Vec3::Y).normalize_or_zero()
    }

    /// Whether a world point projects inside the rectangle `a`–`b` (physical pixels).
    pub fn inside(&self, p: Vec3, a: Vec2, b: Vec2) -> bool {
        let (min, max) = (a.min(b), a.max(b));
        self.project(p)
            .is_some_and(|(s, _)| s.x >= min.x && s.x <= max.x && s.y >= min.y && s.y <= max.y)
    }

    /// Whether a world point is on screen.
    pub fn on_screen(&self, p: Vec3) -> bool {
        self.inside(p, Vec2::ZERO, Vec2::new(self.size[0], self.size[1]))
    }
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-6 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_render::{RtsCamera, camera::FlatGround};

    fn screen() -> Screen {
        let camera = RtsCamera::new([50.0, 50.0], None).view(16.0 / 9.0, &FlatGround);
        Screen {
            camera,
            size: [1600.0, 900.0],
            scale: 1.0,
        }
    }

    #[test]
    fn the_ground_under_a_projected_point_is_that_point() {
        let s = screen();
        for p in [Vec3::new(50.0, 0.0, 50.0), Vec3::new(40.0, 0.0, 58.0)] {
            let (px, _) = s.project(p).unwrap();
            let hit = s.ground(px, &FlatGround).unwrap();
            assert!(hit.distance(p) < 0.01, "{hit} vs {p}");
        }
        // The focus is the screen centre.
        let (centre, _) = s.project(Vec3::new(50.0, 0.0, 50.0)).unwrap();
        assert!(centre.distance(Vec2::new(800.0, 450.0)) < 1.0);
    }

    #[test]
    fn capsules_are_at_least_28_pixels_wide_and_the_nearest_body_wins() {
        let s = screen();
        let body = |id, x: f32| Body {
            id,
            foot: Vec3::new(x, 0.0, 50.0),
            height: 2.0,
            radius: 0.3,
            unit: true,
        };
        let bodies = [body(1, 50.0), body(2, 51.0)];
        let (foot, _) = s.project(bodies[0].foot).unwrap();
        assert_eq!(s.pick_unit(&bodies, foot + Vec2::new(-12.0, 0.0)), Some(1));
        assert_eq!(s.pick_unit(&bodies, foot + Vec2::new(-40.0, 0.0)), None);
        let (other, _) = s.project(bodies[1].foot).unwrap();
        assert_eq!(s.pick_unit(&bodies, other), Some(2));
        // Above the feet, along the body.
        assert_eq!(s.pick_unit(&bodies, foot + Vec2::new(0.0, -20.0)), Some(1));
        let building = Body {
            unit: false,
            ..body(3, 50.0)
        };
        assert_eq!(s.pick_unit(&[building], foot), None);
    }
}
