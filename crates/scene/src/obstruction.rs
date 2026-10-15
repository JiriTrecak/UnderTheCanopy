//! What stops a close camera's boom (old `cameraObstruction`): rays against the solid
//! geometry of the map's scenery and of live buildings, so the third-person camera stays
//! in front of trees and walls. The geometry is each model's full-detail mesh, leaves
//! included as the old raycast had them, kept on the CPU per model. Scenery is tested only where the ray passes its
//! bounding sphere, so a short boom costs a few triangle tests a frame.

use canopy_assets::Pack;
use canopy_render::statics::StaticInstance;
use glam::{Mat4, Vec3};
use std::collections::BTreeMap;

/// A model's solid triangles in model space.
#[derive(Debug, Clone, Default)]
pub struct Hull {
    positions: Vec<Vec3>,
    indices: Vec<u32>,
    min: Vec3,
    max: Vec3,
}

impl Hull {
    /// Every surface of pack model `name` at full detail (the old raycast hit leaves
    /// too); `None` when it has none.
    pub fn of(pack: &Pack, name: &str) -> Option<Self> {
        let model = pack.model(name).ok()?;
        let meta = &model.meta;
        let (min, extent) = (
            Vec3::from(meta.position_min),
            Vec3::from(meta.position_extent),
        );
        let positions: Vec<Vec3> = model
            .vertices()
            .iter()
            .map(|v| {
                let q = v.position;
                min + Vec3::new(f32::from(q[0]), f32::from(q[1]), f32::from(q[2])) / 65535.0
                    * extent
            })
            .collect();
        let all = model.indices();
        let mut indices = Vec::new();
        for section in &meta.sections {
            let first = section.first_index as usize;
            let end = (first + section.index_count as usize).min(all.len());
            indices.extend((first..end).map(|k| section.base_vertex + all.get(k)));
        }
        indices.retain(|&i| (i as usize) < positions.len());
        indices.truncate(indices.len() / 3 * 3);
        if indices.is_empty() {
            return None;
        }
        Some(Self::from_triangles(positions, indices))
    }

    /// A hull from triangles (tests).
    pub fn from_triangles(positions: Vec<Vec3>, indices: Vec<u32>) -> Self {
        let (min, max) = positions.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        Self {
            positions,
            indices,
            min,
            max,
        }
    }

    /// Centre and radius of the model-space bounds.
    fn sphere(&self) -> (Vec3, f32) {
        (
            (self.min + self.max) * 0.5,
            (self.max - self.min).length() * 0.5,
        )
    }

    /// The nearest hit of the ray `origin + t · dir` (world space, `t` in metres along a
    /// unit `dir`) on this hull placed by `transform`, below `far`.
    pub fn hit(&self, transform: &Mat4, origin: Vec3, dir: Vec3, far: f32) -> Option<f32> {
        // In model space the same `t` holds when the direction is transformed unnormalised.
        let inverse = transform.inverse();
        let o = inverse.transform_point3(origin);
        let d = inverse.transform_vector3(dir);
        let (mut t0, mut t1) = (0.0f32, far);
        for axis in 0..3 {
            let (oa, da) = (o[axis], d[axis]);
            if da.abs() < 1e-9 {
                if oa < self.min[axis] || oa > self.max[axis] {
                    return None;
                }
                continue;
            }
            let (a, b) = ((self.min[axis] - oa) / da, (self.max[axis] - oa) / da);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
            if t0 > t1 {
                return None;
            }
        }
        let mut best: Option<f32> = None;
        for tri in self.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| self.positions[tri[k] as usize]);
            if let Some(t) = triangle(o, d, a, b, c)
                && t < best.unwrap_or(far)
            {
                best = Some(t);
            }
        }
        best
    }
}

/// Möller–Trumbore, both faces: `t` of the hit, if ahead.
fn triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// The map's scenery and the live models' hulls, for camera rays.
#[derive(Default)]
pub struct Obstacles {
    hulls: Vec<Option<Hull>>,
    by_name: BTreeMap<String, usize>,
    /// Each static instance: hull, transform, world bounding sphere.
    statics: Vec<(usize, Mat4, Vec3, f32)>,
    /// Hull of each live model (by its index in the world's actor models), on first use.
    actors: Vec<Option<usize>>,
    actor_names: Vec<String>,
}

impl Obstacles {
    /// The scenery of `statics` and the live models `actor_models` (pack entries).
    pub fn new(pack: &Pack, statics: &[StaticInstance], actor_models: &[String]) -> Self {
        let mut this = Self {
            actor_names: actor_models.to_vec(),
            actors: vec![None; actor_models.len()],
            ..Default::default()
        };
        for s in statics {
            let hull = this.hull(pack, &s.model);
            let Some(h) = &this.hulls[hull] else {
                continue;
            };
            let (centre, radius) = h.sphere();
            let scale = s
                .transform
                .x_axis
                .length()
                .max(s.transform.y_axis.length())
                .max(s.transform.z_axis.length());
            this.statics.push((
                hull,
                s.transform,
                s.transform.transform_point3(centre),
                radius * scale,
            ));
        }
        for i in 0..actor_models.len() {
            let name = this.actor_names[i].clone();
            this.actors[i] = Some(this.hull(pack, &name));
        }
        this
    }

    fn hull(&mut self, pack: &Pack, name: &str) -> usize {
        if let Some(&i) = self.by_name.get(name) {
            return i;
        }
        self.hulls.push(Hull::of(pack, name));
        self.by_name.insert(name.to_owned(), self.hulls.len() - 1);
        self.hulls.len() - 1
    }

    /// How far the ray `origin + t · dir` (unit `dir`) runs before scenery stops it, up to
    /// `far`; `hidden(i)` skips static instance `i` (felled trees).
    pub fn scenery(
        &self,
        origin: Vec3,
        dir: Vec3,
        far: f32,
        hidden: &dyn Fn(usize) -> bool,
    ) -> f32 {
        let mut nearest = far;
        for (i, (hull, transform, centre, radius)) in self.statics.iter().enumerate() {
            if !segment_near_sphere(origin, dir, nearest, *centre, *radius) || hidden(i) {
                continue;
            }
            if let Some(Some(h)) = self.hulls.get(*hull)
                && let Some(t) = h.hit(transform, origin, dir, nearest)
            {
                nearest = t;
            }
        }
        nearest
    }

    /// How far the ray runs before live model `model` placed by `transform` stops it.
    pub fn live(&self, model: u32, transform: &Mat4, origin: Vec3, dir: Vec3, far: f32) -> f32 {
        let Some(Some(hull)) = self.actors.get(model as usize) else {
            return far;
        };
        let Some(Some(h)) = self.hulls.get(*hull) else {
            return far;
        };
        h.hit(transform, origin, dir, far).unwrap_or(far)
    }
}

/// Whether the segment `origin + t · dir`, `t` in `0..far`, passes within `radius` of
/// `centre`.
fn segment_near_sphere(origin: Vec3, dir: Vec3, far: f32, centre: Vec3, radius: f32) -> bool {
    let t = (centre - origin).dot(dir).clamp(0.0, far);
    (origin + dir * t).distance_squared(centre) <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube's two faces facing ±x.
    fn wall() -> Hull {
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        Hull::from_triangles(
            vec![
                p(-0.5, 0.0, -0.5),
                p(-0.5, 1.0, -0.5),
                p(-0.5, 1.0, 0.5),
                p(-0.5, 0.0, 0.5),
                p(0.5, 0.0, -0.5),
                p(0.5, 1.0, -0.5),
                p(0.5, 1.0, 0.5),
                p(0.5, 0.0, 0.5),
            ],
            vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        )
    }

    #[test]
    fn rays_stop_at_the_nearest_face_of_a_placed_hull() {
        let h = wall();
        // Placed 5 m east, scaled ×2.
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            glam::Quat::IDENTITY,
            Vec3::new(5.0, 0.0, 0.0),
        );
        let origin = Vec3::new(0.0, 0.5, 0.0);
        let t = h.hit(&transform, origin, Vec3::X, 10.0).expect("a hit");
        assert!((t - 4.0).abs() < 1e-4, "{t}");
        // Too short, or passing above.
        assert_eq!(h.hit(&transform, origin, Vec3::X, 3.0), None);
        assert_eq!(
            h.hit(&transform, Vec3::new(0.0, 3.0, 0.0), Vec3::X, 10.0),
            None
        );
    }

    #[test]
    fn spheres_gate_the_segment() {
        assert!(segment_near_sphere(
            Vec3::ZERO,
            Vec3::X,
            10.0,
            Vec3::new(5.0, 1.0, 0.0),
            1.5
        ));
        assert!(!segment_near_sphere(
            Vec3::ZERO,
            Vec3::X,
            3.0,
            Vec3::new(5.0, 0.0, 0.0),
            1.5
        ));
        assert!(!segment_near_sphere(
            Vec3::ZERO,
            Vec3::X,
            10.0,
            Vec3::new(5.0, 3.0, 0.0),
            1.5
        ));
    }
}
