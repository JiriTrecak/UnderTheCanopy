//! Picking on the CPU (old `unitPicking.ts`, `renderer.pickGameEntity`, `pickGround`):
//! a screen-space capsule from each unit's feet to its head, the ground under the
//! cursor by marching the camera ray over the terrain, the camera ray against standing
//! trees ([`Trees`]) and the box test of box selection. CPU picking needs no GPU
//! read-back (no frame of latency) and works on the interpolated positions the frame
//! shows.

use canopy_render::camera::{CameraView, Ground};
use glam::{Vec2, Vec3, Vec4};
use std::collections::BTreeMap;

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

impl Body {
    /// Where the ray `origin + t · dir` enters the body's upright cylinder, before `far`.
    pub fn enter(&self, origin: Vec3, dir: Vec3, far: f32) -> Option<f32> {
        cylinder(
            origin,
            dir,
            self.foot,
            self.radius,
            self.foot.y,
            self.foot.y + self.height,
            far,
        )
    }
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

/// Height bands of a tree's pick profile.
pub const TREE_BANDS: usize = 8;
/// Smallest pick radius of a tree at any height (its trunk), world units.
pub const MIN_TRUNK_RADIUS: f32 = 0.45;
/// Share of a band's widest vertex that picks: needle and leaf cards are mostly
/// transparent towards their tips.
pub const CROWN_SHARE: f32 = 0.85;
/// How far outside a tree's widest reach its ring runs, world units.
pub const RING_MARGIN: f32 = 0.25;
/// Grid buckets of the tree index, world units.
const TREE_BUCKET: f32 = 8.0;

/// A tree model's pick shape (model units): upright cylinders stacked around its trunk
/// axis, one per equal height band from the soil line to the top, each as wide as the
/// model's widest vertex in that band. It follows the drawn silhouette (a pine's cone,
/// a broadleaf's ball on a trunk) and is cheap to test exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeProfile {
    pub height: f32,
    pub radii: [f32; TREE_BANDS],
}

impl TreeProfile {
    /// The profile of a model's rest-pose vertices (model space, y up, trunk on the
    /// y axis). Bands without vertices (a bare trunk between its rings) take the
    /// narrower of their neighbours. `None` without vertices above the soil.
    pub fn of(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let points: Vec<Vec3> = points.into_iter().filter(|p| p.y >= 0.0).collect();
        let height = points.iter().map(|p| p.y).fold(0.0f32, f32::max);
        if height <= 1e-3 {
            return None;
        }
        let mut radii = [None::<f32>; TREE_BANDS];
        for p in &points {
            let band = ((p.y / height * TREE_BANDS as f32) as usize).min(TREE_BANDS - 1);
            let r = Vec2::new(p.x, p.z).length();
            radii[band] = Some(radii[band].map_or(r, |old| old.max(r)));
        }
        let filled: [f32; TREE_BANDS] = std::array::from_fn(|i| {
            radii[i].unwrap_or_else(|| {
                let below = radii[..i].iter().rev().find_map(|r| *r);
                let above = radii[i + 1..].iter().find_map(|r| *r);
                match (below, above) {
                    (Some(a), Some(b)) => a.min(b),
                    (a, b) => a.or(b).unwrap_or(0.0),
                }
            })
        });
        Some(Self {
            height,
            radii: filled.map(|r| r * CROWN_SHARE),
        })
    }

    /// The widest band (model units).
    pub fn widest(&self) -> f32 {
        self.radii.iter().copied().fold(0.0, f32::max)
    }
}

/// A standing tree as the static scene draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tree {
    /// Its scenery entity.
    pub id: u32,
    /// Where its trunk meets the soil, world units.
    pub root: Vec3,
    /// Uniform model scale.
    pub scale: f32,
    /// Index into the profiles.
    pub profile: usize,
}

/// Every tree of the map with its pick shape, bucketed on the ground so a camera ray
/// tests only the trees near the stretch of it that passes through the forest's height.
#[derive(Debug, Clone, Default)]
pub struct Trees {
    profiles: Vec<TreeProfile>,
    /// Sorted by id.
    trees: Vec<Tree>,
    buckets: Vec<Vec<u32>>,
    origin: Vec2,
    columns: usize,
    rows: usize,
    /// Widest reach and the lowest root and highest top of any tree.
    reach: f32,
    low: f32,
    top: f32,
}

impl Trees {
    pub fn new(profiles: Vec<TreeProfile>, mut trees: Vec<Tree>) -> Self {
        trees.retain(|t| t.profile < profiles.len());
        trees.sort_by_key(|t| t.id);
        trees.dedup_by_key(|t| t.id);
        if trees.is_empty() {
            return Self::default();
        }
        let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let (mut reach, mut low, mut top) = (0.0f32, f32::MAX, f32::MIN);
        for t in &trees {
            let p = &profiles[t.profile];
            let at = Vec2::new(t.root.x, t.root.z);
            min = min.min(at);
            max = max.max(at);
            reach = reach.max((p.widest() * t.scale).max(MIN_TRUNK_RADIUS));
            low = low.min(t.root.y);
            top = top.max(t.root.y + p.height * t.scale);
        }
        let columns = ((max.x - min.x) / TREE_BUCKET) as usize + 1;
        let rows = ((max.y - min.y) / TREE_BUCKET) as usize + 1;
        let mut buckets = vec![Vec::new(); columns * rows];
        for (i, t) in trees.iter().enumerate() {
            let c = ((t.root.x - min.x) / TREE_BUCKET) as usize;
            let r = ((t.root.z - min.y) / TREE_BUCKET) as usize;
            buckets[r * columns + c].push(i as u32);
        }
        Self {
            profiles,
            trees,
            buckets,
            origin: min,
            columns,
            rows,
            reach,
            low,
            top,
        }
    }

    /// The trees of the static scene: the instance of each tree entity (`tree_slots`)
    /// with the profile of its model, read once per model from the pack.
    pub fn of_scene(
        pack: &canopy_assets::Pack,
        statics: &[canopy_render::statics::StaticInstance],
        slots: &BTreeMap<canopy_sim::entity::EntityId, usize>,
    ) -> Self {
        let _span = tracing::info_span!("tree_picking").entered();
        let mut by_model: BTreeMap<&str, Option<usize>> = BTreeMap::new();
        let mut profiles = Vec::new();
        let mut trees = Vec::new();
        for (id, &slot) in slots {
            let Some(instance) = statics.get(slot) else {
                continue;
            };
            let profile = *by_model.entry(instance.model.as_str()).or_insert_with(|| {
                let profile = model_points(pack, &instance.model).and_then(TreeProfile::of)?;
                profiles.push(profile);
                Some(profiles.len() - 1)
            });
            let Some(profile) = profile else {
                continue;
            };
            let (scale, _, root) = instance.transform.to_scale_rotation_translation();
            trees.push(Tree {
                id: id.0,
                root,
                scale: scale.max_element(),
                profile,
            });
        }
        Self::new(profiles, trees)
    }

    pub fn get(&self, id: u32) -> Option<&Tree> {
        self.trees
            .binary_search_by_key(&id, |t| t.id)
            .ok()
            .map(|i| &self.trees[i])
    }

    /// The ground ring that marks a tree: around its root, just outside its crown's
    /// widest reach, so it shows around the crown from the overhead camera (a ring the
    /// size of the trunk would hide under the branches).
    pub fn ring(&self, id: u32) -> Option<([f32; 2], f32)> {
        let t = self.get(id)?;
        let radius = (self.profiles[t.profile].widest() / CROWN_SHARE * t.scale)
            .max(MIN_TRUNK_RADIUS)
            + RING_MARGIN;
        Some(([t.root.x, t.root.z], radius))
    }

    /// The nearest tree the ray `origin + t · dir` (unit `dir`) enters before `far`,
    /// among those `standing` accepts, and the distance; equal distances go to the
    /// lower id.
    pub fn hit(
        &self,
        origin: Vec3,
        dir: Vec3,
        far: f32,
        standing: impl Fn(u32) -> bool,
    ) -> Option<(u32, f32)> {
        if self.trees.is_empty() {
            return None;
        }
        // The stretch of the ray between the forest's top and its lowest root.
        let (mut t0, mut t1) = (0.0f32, far);
        if dir.y.abs() > 1e-6 {
            let (a, b) = ((self.top - origin.y) / dir.y, (self.low - origin.y) / dir.y);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        } else if origin.y < self.low || origin.y > self.top {
            return None;
        }
        if t0 > t1 {
            return None;
        }
        let (a, b) = (origin + dir * t0, origin + dir * t1);
        let lo = Vec2::new(a.x.min(b.x), a.z.min(b.z)) - Vec2::splat(self.reach) - self.origin;
        let hi = Vec2::new(a.x.max(b.x), a.z.max(b.z)) + Vec2::splat(self.reach) - self.origin;
        let span = |lo: f32, hi: f32, n: usize| {
            let first = (lo / TREE_BUCKET).floor().max(0.0) as usize;
            let last = (hi / TREE_BUCKET).floor().min(n as f32 - 1.0);
            (
                first,
                if last < 0.0 {
                    None
                } else {
                    Some(last as usize)
                },
            )
        };
        let (c0, c1) = span(lo.x, hi.x, self.columns);
        let (r0, r1) = span(lo.y, hi.y, self.rows);
        let (Some(c1), Some(r1)) = (c1, r1) else {
            return None;
        };
        let mut best: Option<(f32, u32)> = None;
        for r in r0..=r1 {
            for c in c0..=c1 {
                for &i in &self.buckets[r * self.columns + c] {
                    let tree = &self.trees[i as usize];
                    let Some(t) = self.enter(tree, origin, dir, far) else {
                        continue;
                    };
                    if best.is_some_and(|(bt, bid)| (bt, bid) <= (t, tree.id)) || !standing(tree.id)
                    {
                        continue;
                    }
                    best = Some((t, tree.id));
                }
            }
        }
        best.map(|(t, id)| (id, t))
    }

    /// Where the ray enters a tree's bands.
    fn enter(&self, tree: &Tree, origin: Vec3, dir: Vec3, far: f32) -> Option<f32> {
        let p = &self.profiles[tree.profile];
        let band = p.height * tree.scale / TREE_BANDS as f32;
        let mut best: Option<f32> = None;
        for (i, r) in p.radii.iter().enumerate() {
            let radius = (r * tree.scale).max(MIN_TRUNK_RADIUS);
            let y0 = tree.root.y + band * i as f32;
            if let Some(t) = cylinder(origin, dir, tree.root, radius, y0, y0 + band, far)
                && best.is_none_or(|b| t < b)
            {
                best = Some(t);
            }
        }
        best
    }
}

/// Where the ray `origin + t · dir` enters the upright cylinder of `radius` around
/// `axis` (x, z) between heights `y0` and `y1`, with `0 ≤ t ≤ far`.
pub fn cylinder(
    origin: Vec3,
    dir: Vec3,
    axis: Vec3,
    radius: f32,
    y0: f32,
    y1: f32,
    far: f32,
) -> Option<f32> {
    let (mut lo, mut hi) = (0.0f32, far);
    if dir.y.abs() > 1e-9 {
        let (a, b) = ((y0 - origin.y) / dir.y, (y1 - origin.y) / dir.y);
        lo = lo.max(a.min(b));
        hi = hi.min(a.max(b));
    } else if origin.y < y0 || origin.y > y1 {
        return None;
    }
    let (ox, oz) = (origin.x - axis.x, origin.z - axis.z);
    let a = dir.x * dir.x + dir.z * dir.z;
    let c = ox * ox + oz * oz - radius * radius;
    if a < 1e-12 {
        if c > 0.0 {
            return None;
        }
    } else {
        let b = 2.0 * (ox * dir.x + oz * dir.z);
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return None;
        }
        let root = disc.sqrt();
        lo = lo.max((-b - root) / (2.0 * a));
        hi = hi.min((-b + root) / (2.0 * a));
    }
    (lo <= hi).then_some(lo)
}

/// A pack model's rest-pose vertex positions (model space).
fn model_points(pack: &canopy_assets::Pack, name: &str) -> Option<Vec<Vec3>> {
    let model = pack.model(name).ok()?;
    let meta = &model.meta;
    let (min, extent) = (
        Vec3::from(meta.position_min),
        Vec3::from(meta.position_extent),
    );
    Some(
        model
            .vertices()
            .iter()
            .map(|v| {
                let q = v.position;
                min + Vec3::new(f32::from(q[0]), f32::from(q[1]), f32::from(q[2])) / 65535.0
                    * extent
            })
            .collect(),
    )
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

    /// A fixture pine: a trunk ring at the soil and at 3 m, and a cone of branch tips
    /// from 2.5 m wide at 3 m up to its top at 10 m.
    fn pine() -> TreeProfile {
        let mut points = Vec::new();
        for k in 0..16 {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            for (y, r) in [(0.0, 0.3), (3.0, 0.3), (3.0, 2.5), (6.5, 1.25), (10.0, 0.0)] {
                points.push(Vec3::new(c * r, y, s * r));
            }
        }
        TreeProfile::of(points).expect("a profile")
    }

    #[test]
    fn profiles_follow_the_silhouette_and_fill_bare_trunk_bands() {
        let p = pine();
        assert_eq!(p.height, 10.0);
        // Bands of 1.25 m: the soil ring, bare trunk, the cone's base ring, tips up to
        // the top.
        assert!((p.radii[0] - 0.3 * CROWN_SHARE).abs() < 1e-4, "{p:?}");
        assert!(
            (p.radii[1] - 0.3 * CROWN_SHARE).abs() < 1e-4,
            "bare trunk {p:?}"
        );
        assert!((p.radii[2] - 2.5 * CROWN_SHARE).abs() < 1e-4, "{p:?}");
        assert!(p.radii[2] > p.radii[5] && p.radii[5] > p.radii[7], "{p:?}");
        assert_eq!(TreeProfile::of([Vec3::new(1.0, 0.0, 0.0)]), None);
    }

    #[test]
    fn rays_pick_the_nearest_standing_tree_by_its_drawn_shape() {
        let s = screen();
        let tree = |id, x: f32, z: f32| Tree {
            id,
            root: Vec3::new(x, 0.0, z),
            scale: 1.0,
            profile: 0,
        };
        // Tree 2 stands behind tree 1 as the camera looks; tree 3 far away.
        let trees = Trees::new(
            vec![pine()],
            vec![
                tree(1, 50.0, 50.0),
                tree(2, 50.0, 46.0),
                tree(3, 200.0, 10.0),
            ],
        );
        let pick = |p: Vec3, standing: &dyn Fn(u32) -> bool| {
            let (px, _) = s.project(p).unwrap();
            let (origin, dir) = s.ray(px);
            let far = s.ground(px, &FlatGround).unwrap().distance(origin) + 0.5;
            trees.hit(origin, dir, far, standing).map(|(id, _)| id)
        };
        let all = |_| true;
        // The crown well off the root, and the trunk.
        assert_eq!(pick(Vec3::new(51.5, 4.0, 50.0), &all), Some(1));
        assert_eq!(pick(Vec3::new(50.0, 1.0, 50.0), &all), Some(1));
        // Beside the crown, and open ground.
        assert_eq!(pick(Vec3::new(53.0, 4.0, 50.0), &all), None);
        assert_eq!(pick(Vec3::new(60.0, 0.0, 50.0), &all), None);
        // The front tree's crown covers the back one's trunk: the nearest wins, the
        // back one when the front one has fallen.
        let behind = Vec3::new(50.0, 4.0, 46.0);
        assert_eq!(pick(behind, &all), Some(1));
        assert_eq!(pick(behind, &|id| id != 1), Some(2));
        assert_eq!(pick(Vec3::new(200.0, 3.5, 10.0), &all), Some(3));
        let (centre, radius) = trees.ring(1).unwrap();
        assert_eq!(centre, [50.0, 50.0]);
        assert!((radius - (2.5 + RING_MARGIN)).abs() < 1e-4);
        assert_eq!(trees.ring(4), None);
    }
}
