//! The sun's shadow map: one orthographic map fitted to what the camera sees
//! (old `Sky.fitView`), snapped to whole texels so it does not shimmer while panning.
//!
//! The camera's four corner rays are cut by two horizontal planes, `below` under and
//! `above` over the ground at the focus; the eight points, padded and rounded up to
//! `step`, bound the map in light space. The light space is anchored at the world origin,
//! so snapping the bounds to texels is the same wherever the camera is.

use crate::camera::{CameraView, frustum_planes};
use crate::environment::ShadowConfig;
use glam::{Mat4, Vec3, Vec4, Vec4Swizzles};

/// How far towards the sun beyond the fitted slice casters are kept (tall trees, hills).
pub const CASTER_REACH: f32 = 120.0;
/// Depth kept past the slice away from the sun.
pub const RECEIVER_REACH: f32 = 24.0;
/// Half extent of the fallback square when the view sees the horizon.
pub const FALLBACK_EXTENT: f32 = 110.0;

/// A fitted shadow view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowView {
    /// World → clip (x, y in −1…1, depth 0 at the sun side … 1).
    pub view_projection: Mat4,
    /// Width and height of the covered area, world units.
    pub size: [f32; 2],
    /// World units per texel.
    pub texel: f32,
}

/// The light's rotation: looks along −`towards_sun`, anchored at the origin.
fn light_view(towards_sun: Vec3) -> Mat4 {
    let dir = towards_sun.normalize();
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    Mat4::look_at_rh(dir, Vec3::ZERO, up)
}

/// Fits the map to `camera` with the focus ground at `ground` height.
pub fn fit(
    camera: &CameraView,
    ground: f32,
    towards_sun: Vec3,
    config: &ShadowConfig,
) -> ShadowView {
    let light = light_view(towards_sun);
    let inverse = camera.view_projection.inverse();
    let mut points = Vec::with_capacity(8);
    let mut horizon = false;
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        // Reverse-Z: depth 0 is the far plane.
        let far = inverse.project_point3(Vec3::new(x, y, 0.0));
        let dir = far - camera.eye;
        if dir.y > -1e-3 {
            horizon = true;
            break;
        }
        for plane in [ground - config.below, ground + config.above] {
            let t = (plane - camera.eye.y) / dir.y;
            points.push(camera.eye + dir * t.max(0.0));
        }
    }
    if horizon {
        points.clear();
        let t = camera.target;
        for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            for y in [ground - config.below, ground + config.above] {
                points.push(Vec3::new(
                    t.x + dx * FALLBACK_EXTENT,
                    y,
                    t.z + dz * FALLBACK_EXTENT,
                ));
            }
        }
    }
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for p in &points {
        let q = (light * p.extend(1.0)).xyz();
        min = min.min(q);
        max = max.max(q);
    }
    let pad = config.padding;
    let width = ((max.x - min.x + 2.0 * pad) / config.step).ceil() * config.step;
    let height = ((max.y - min.y + 2.0 * pad) / config.step).ceil() * config.step;
    let size = width.max(height);
    let texel = size / config.resolution as f32;
    let center = (min + max) * 0.5;
    let left = ((center.x - size / 2.0) / texel).floor() * texel;
    let bottom = ((center.y - size / 2.0) / texel).floor() * texel;
    // View space looks down −z: the sun side has the largest z.
    let near = -(max.z + CASTER_REACH);
    let far = -(min.z - RECEIVER_REACH);
    let projection = Mat4::orthographic_rh(left, left + size, bottom, bottom + size, near, far);
    ShadowView {
        view_projection: projection * light,
        size: [size, size],
        texel,
    }
}

/// Which shadow casters can darken anything the camera sees.
///
/// Every surface that reads the sun's map (lit surfaces, water, the haze march) lies
/// inside the camera's view, and reads it only where its depth is inside the map. A
/// caster can only shade a point that lies behind it, away from the sun; so a caster
/// matters only when its bounding sphere, swept away from the sun until the map's far
/// plane or below the lowest ground (nothing under all of the ground can be seen),
/// touches the camera's view. The sphere grows by every offset a read takes:
/// the receivers' normal offset, the filter disk and the hardware filter's texels, and
/// a little sway. Skipping the others leaves every texel any visible pixel reads as it
/// was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CasterCull {
    /// The camera's side planes and its near plane (n·p + d ≥ 0 inside).
    pub planes: [Vec4; 5],
    /// Unit vector towards the sun; w: the margin added to every radius (world units).
    pub sweep: Vec4,
    /// The map's depth row: `depth · (p, 1)` is a point's depth in the map (0 at the sun).
    pub depth: Vec4,
    /// Height under which nothing can be seen (the lowest ground), world units.
    pub floor: f32,
}

/// Wind sway beyond a model's rest bounds, world units (scenery's flutter is centimetres).
const SWAY_MARGIN: f32 = 0.5;

impl CasterCull {
    /// `texel`: the map's world units per texel; `filter`: the filter disk's radius in
    /// texels (`shadow_softness`); `normal_offset`: the receivers' offset, world units;
    /// `floor`: the lowest ground (`f32::MIN` when unknown).
    pub fn new(
        camera: Mat4,
        shadow: Mat4,
        towards_sun: Vec3,
        texel: f32,
        filter: f32,
        normal_offset: f32,
        floor: f32,
    ) -> Self {
        let p = frustum_planes(camera);
        // The disk's radius (at least half a texel), the 2×2 comparison filter and the
        // diagonal of a texel.
        let margin = normal_offset
            + (filter.max(0.5) + 2.0) * texel * std::f32::consts::SQRT_2
            + SWAY_MARGIN;
        Self {
            planes: [p[0], p[1], p[2], p[3], p[5]],
            sweep: towards_sun.normalize().extend(margin),
            depth: shadow.row(2),
            floor,
        }
    }

    /// Whether a caster with this world bounding sphere can shade a visible point.
    pub fn casts_into_view(&self, sphere: Vec4) -> bool {
        let towards_sun = self.sweep.truncate();
        let radius = sphere.w + self.sweep.w;
        let start = sphere.truncate();
        // Depth grows by `rate` per unit moved away from the sun.
        let rate = -self.depth.truncate().dot(towards_sun);
        if rate <= 1e-6 {
            return true;
        }
        let depth = self.depth.dot(start.extend(1.0));
        let mut length = ((1.0 - depth) / rate).max(0.0) + radius;
        if towards_sun.y > 1e-3 {
            length = length.min(((start.y + radius - self.floor) / towards_sun.y).max(0.0));
        }
        let end = start - towards_sun * length;
        self.planes.iter().all(|p| {
            p.truncate().dot(start) + p.w >= -radius || p.truncate().dot(end) + p.w >= -radius
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{FlatGround, RtsCamera};

    fn config() -> ShadowConfig {
        ShadowConfig {
            resolution: 2048,
            below: 6.0,
            above: 12.0,
            padding: 2.0,
            step: 4.0,
        }
    }

    fn sun() -> Vec3 {
        Vec3::new(-0.40558, std::f32::consts::FRAC_1_SQRT_2, -0.57923)
    }

    #[test]
    fn the_map_covers_the_view_on_the_ground() {
        let camera = RtsCamera::new([300.0, 300.0], None).view(16.0 / 9.0, &FlatGround);
        let shadow = fit(&camera, 0.0, sun(), &config());
        let rts = RtsCamera::new([300.0, 300.0], None);
        // The far corners span the footprint's full width.
        let [x0, x1, z0, z1] = rts.footprint(16.0 / 9.0, 0.0).unwrap();
        for p in [
            Vec3::new(x0, 0.0, z0),
            Vec3::new(x1, 0.0, z0),
            Vec3::new(300.0, 0.0, z1),
            Vec3::new(300.0, 0.0, 300.0),
        ] {
            let c = shadow.view_projection.project_point3(p);
            assert!(c.x.abs() <= 1.0 && c.y.abs() <= 1.0, "{p} → {c}");
            assert!((0.0..=1.0).contains(&c.z));
        }
        // A tree top 20 m up still lands in the depth range.
        let top = shadow
            .view_projection
            .project_point3(Vec3::new(300.0, 20.0, 300.0));
        assert!(top.z > 0.0);
        assert!(shadow.size[0] % 4.0 == 0.0);
    }

    #[test]
    fn only_casters_whose_shadow_reaches_the_view_are_kept() {
        let camera = RtsCamera::new([300.0, 300.0], None).view(16.0 / 9.0, &FlatGround);
        let shadow = fit(&camera, 0.0, sun(), &config());
        let cull = CasterCull::new(
            camera.view_projection,
            shadow.view_projection,
            sun(),
            shadow.texel,
            1.5,
            0.025,
            f32::MIN,
        );
        let planes = frustum_planes(camera.view_projection);
        let visible = |p: Vec3| planes.iter().all(|q| q.truncate().dot(p) + q.w >= 0.0);
        // A caster standing in view casts into it.
        assert!(cull.casts_into_view(Vec3::new(300.0, 2.0, 300.0).extend(1.0)));
        // Casters just outside the view on the sun's side shade it; on the far side
        // they do not. Find, along the sun's ground direction, the last ground point in
        // view.
        let ground = Vec3::new(sun().x, 0.0, sun().z).normalize();
        let edge = (0..4000)
            .map(|i| Vec3::new(300.0, 0.0, 300.0) + ground * (i as f32 * 0.1))
            .take_while(|&p| visible(p))
            .last()
            .unwrap();
        // A tall caster a little beyond the edge, towards the sun: its shadow falls back
        // into view.
        let tall = (edge + ground * 4.0 + Vec3::Y * 6.0).extend(1.0);
        assert!(!visible(tall.truncate()));
        assert!(cull.casts_into_view(tall));
        // Away from the sun, beyond the opposite edge: nothing it shades is in view.
        let back = (0..4000)
            .map(|i| Vec3::new(300.0, 0.0, 300.0) - ground * (i as f32 * 0.1))
            .take_while(|&p| visible(p))
            .last()
            .unwrap();
        let behind = (back - ground * 20.0 + Vec3::Y).extend(1.0);
        assert!(!cull.casts_into_view(behind));
    }

    #[test]
    fn panning_moves_the_map_by_whole_texels() {
        let a = RtsCamera::new([300.0, 300.0], None).view(16.0 / 9.0, &FlatGround);
        let b = RtsCamera::new([300.37, 301.11], None).view(16.0 / 9.0, &FlatGround);
        let (sa, sb) = (
            fit(&a, 0.0, sun(), &config()),
            fit(&b, 0.0, sun(), &config()),
        );
        assert_eq!(sa.size, sb.size);
        // The same world point maps to texel positions differing by whole texels.
        let p = Vec3::new(310.0, 0.0, 290.0);
        let res = config().resolution as f32;
        let ta = (sa.view_projection.project_point3(p).x + 1.0) * 0.5 * res;
        let tb = (sb.view_projection.project_point3(p).x + 1.0) * 0.5 * res;
        let shift = ta - tb;
        assert!((shift - shift.round()).abs() < 1e-2, "{shift}");
    }

    #[test]
    fn a_view_of_the_horizon_falls_back_to_a_square() {
        let camera = CameraView::look_at(
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 10.0, -100.0),
            1.0,
            1.5,
            1.0,
            500.0,
        );
        let shadow = fit(&camera, 0.0, sun(), &config());
        assert!(shadow.size[0] >= 2.0 * FALLBACK_EXTENT);
    }
}
