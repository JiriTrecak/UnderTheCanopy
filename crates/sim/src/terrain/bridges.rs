//! Walkable bridge decks placed by stamps (old `src/shared/map/bridgeSurface.ts`).
//!
//! A deck is either the catalogue's analytic arch (`Deck`: a box with an arched top and
//! an optional rise along its length) or an imported walk mesh carried by the stamp.

use crate::math;
use canopy_content::{
    assets::{Deck, SceneryEntry},
    map::Stamp,
};
use std::f64::consts::PI;

/// Deck thickness when the catalogue declares none, in local metres.
const DEFAULT_THICKNESS: f64 = 0.6;
/// Thickness of imported walk meshes.
const MESH_THICKNESS: f64 = 0.3;

/// One walkable deck in world space. Lengths are world units; `x`, `z` is the centre.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSurface {
    /// The placing stamp's id; also the surface name entities stand on.
    pub id: String,
    /// Navigation level, 1–31.
    pub level: u32,
    /// Levels the start (−z) and end (+z) ramps connect to.
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub x: f64,
    pub z: f64,
    /// Height of the deck's base.
    pub base: f64,
    /// Cosine and sine of the yaw.
    pub c: f64,
    pub s: f64,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub arch: f64,
    pub rise: f64,
    pub thickness: f64,
    /// Lengths of the start and end ramps of an imported mesh.
    pub ramps: [f64; 2],
    /// Imported mesh: world-space triangles. Empty for analytic decks.
    pub triangles: Vec<[[f64; 3]; 3]>,
}

impl BridgeSurface {
    /// Walkable height at a world position, if the position lies on the deck. Overlapping
    /// mesh triangles take the highest.
    pub fn height_at(&self, x: f64, z: f64) -> Option<f64> {
        if !self.triangles.is_empty() {
            return self.mesh_height(x, z);
        }
        let dx = x - self.x;
        let dz = z - self.z;
        let lx = self.c * dx - self.s * dz;
        let lz = self.s * dx + self.c * dz;
        if lx.abs() > self.width / 2.0 || lz.abs() > self.depth / 2.0 {
            return None;
        }
        Some(self.arched(lz))
    }

    /// Top of the analytic deck at local position `lz` along its length.
    pub(super) fn arched(&self, lz: f64) -> f64 {
        self.base
            + self.height
            + self.arch * math::cos(lz * PI / self.depth)
            + self.rise * (lz / self.depth + 0.5)
    }

    fn mesh_height(&self, x: f64, z: f64) -> Option<f64> {
        let mut height: Option<f64> = None;
        for [a, c, d] in &self.triangles {
            let det = (c[2] - d[2]) * (a[0] - d[0]) + (d[0] - c[0]) * (a[2] - d[2]);
            if det.abs() < 1e-10 {
                continue;
            }
            let u = ((c[2] - d[2]) * (x - d[0]) + (d[0] - c[0]) * (z - d[2])) / det;
            let v = ((d[2] - a[2]) * (x - d[0]) + (a[0] - d[0]) * (z - d[2])) / det;
            if u < -0.00001 || v < -0.00001 || u + v > 1.00001 {
                continue;
            }
            let h = u * a[1] + v * c[1] + (1.0 - u - v) * d[1];
            height = Some(height.map_or(h, |old| old.max(h)));
        }
        height
    }

    /// Radius of the circle around the centre that holds the whole deck.
    pub fn reach(&self) -> f64 {
        math::hypot(self.width, self.depth) / 2.0
    }
}

/// The decks of `stamps`: stamps carrying an imported walk mesh with a source transform,
/// and stamps of catalogue scenery with a deck. `deck` finds a scenery entry's deck;
/// `ground` samples the terrain for decks placed relative to it.
pub fn bridge_surfaces<'a>(
    stamps: &[Stamp],
    deck: impl Fn(&str) -> Option<&'a SceneryEntry>,
    ground: impl Fn(f64, f64) -> f64,
) -> Vec<BridgeSurface> {
    stamps
        .iter()
        .filter_map(|stamp| {
            if let (Some(walk), Some(_)) = (&stamp.walk, &stamp.source_transform)
                && walk.mesh.is_some()
            {
                return Some(mesh_surface(stamp));
            }
            let d = deck(&stamp.asset)?.deck.as_ref()?;
            let scale = stamp.scale.unwrap_or(1.0);
            let vertical = scale * stamp.height_scale.unwrap_or(1.0);
            let yaw = stamp.yaw.unwrap_or(0.0);
            let connections = stamp
                .walk
                .as_ref()
                .and_then(|w| w.connections.as_ref())
                .map(|c| (c.start, c.end));
            let (start, end) = connections.unwrap_or_else(|| {
                d.connections
                    .as_ref()
                    .map_or((None, None), |c| (c.start, c.end))
            });
            Some(BridgeSurface {
                id: stamp.id.clone(),
                level: stamp.walk.as_ref().map_or(d.level, |w| w.level),
                start,
                end,
                x: stamp.x + 0.5,
                z: stamp.y + 0.5,
                base: placement_height(stamp, Some(d), &ground),
                c: math::cos(yaw),
                s: math::sin(yaw),
                width: d.width * scale * stamp.width_scale.unwrap_or(1.0),
                depth: d.depth * scale * stamp.depth_scale.unwrap_or(1.0),
                height: d.height * vertical,
                arch: d.arch * vertical,
                rise: d.rise.unwrap_or(0.0) * vertical,
                thickness: d.thickness.unwrap_or(DEFAULT_THICKNESS) * vertical,
                ramps: [0.0, 0.0],
                triangles: Vec::new(),
            })
        })
        .collect()
}

/// Absolute height of a deck stamp's base: its source transform, its authored walk height
/// less the deck's centre rise, or the ground under its centre plus its elevation.
fn placement_height(stamp: &Stamp, deck: Option<&Deck>, ground: impl Fn(f64, f64) -> f64) -> f64 {
    if let Some(transform) = &stamp.source_transform {
        return transform.height;
    }
    if let (Some(d), Some(height)) = (deck, stamp.walk.as_ref().and_then(|w| w.height)) {
        return height
            - (d.height + d.arch + d.rise.unwrap_or(0.0) / 2.0)
                * stamp.scale.unwrap_or(1.0)
                * stamp.height_scale.unwrap_or(1.0);
    }
    ground(stamp.x + 0.5, stamp.y + 0.5) + stamp.elevation.unwrap_or(0.0)
}

/// An imported walk mesh, transformed like the visible model (scale, then the source
/// quaternion), with an oriented bounding box for the deck frame.
fn mesh_surface(stamp: &Stamp) -> BridgeSurface {
    let walk = stamp.walk.as_ref().expect("walk");
    let mesh = walk.mesh.as_ref().expect("mesh");
    let transform = stamp.source_transform.as_ref().expect("source transform");
    let [qx, qy, qz, qw] = transform.quaternion;
    let s = stamp.scale.unwrap_or(1.0);
    let (sx, sy, sz) = (
        s * stamp.width_scale.unwrap_or(1.0),
        s * stamp.height_scale.unwrap_or(1.0),
        s * stamp.depth_scale.unwrap_or(1.0),
    );
    let positions: Vec<[f64; 3]> = mesh
        .positions
        .iter()
        .map(|&[px, py, pz]| {
            let (x, y, z) = (px * sx, py * sy, pz * sz);
            let tx = 2.0 * (qy * z - qz * y);
            let ty = 2.0 * (qz * x - qx * z);
            let tz = 2.0 * (qx * y - qy * x);
            [
                stamp.x + 0.5 + x + qw * tx + qy * tz - qz * ty,
                transform.height + y + qw * ty + qz * tx - qx * tz,
                stamp.y + 0.5 + z + qw * tz + qx * ty - qy * tx,
            ]
        })
        .collect();
    let yaw = math::atan2(2.0 * (qw * qy + qx * qz), 1.0 - 2.0 * (qy * qy + qz * qz));
    let (c, sn) = (math::cos(yaw), math::sin(yaw));
    let (mut lo_x, mut hi_x, mut lo_z, mut hi_z) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for p in &positions {
        let x = c * p[0] - sn * p[2];
        let z = sn * p[0] + c * p[2];
        lo_x = lo_x.min(x);
        hi_x = hi_x.max(x);
        lo_z = lo_z.min(z);
        hi_z = hi_z.max(z);
    }
    let (cx, cz) = ((lo_x + hi_x) / 2.0, (lo_z + hi_z) / 2.0);
    // Ramps: from each end of the local mesh to the flat top.
    let top = mesh
        .positions
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let flat = || {
        mesh.positions
            .iter()
            .filter(move |p| (p[1] - top).abs() < 0.001)
            .map(|p| p[2])
    };
    let min = |values: &mut dyn Iterator<Item = f64>| values.fold(f64::INFINITY, f64::min);
    let max = |values: &mut dyn Iterator<Item = f64>| values.fold(f64::NEG_INFINITY, f64::max);
    let min_z = min(&mut mesh.positions.iter().map(|p| p[2]));
    let max_z = max(&mut mesh.positions.iter().map(|p| p[2]));
    let ramps = [
        (min(&mut flat()) - min_z) * sz,
        (max_z - max(&mut flat())) * sz,
    ];
    let triangles = mesh
        .indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]].map(|i| positions[i as usize]))
        .collect();
    let connections = walk.connections.as_ref();
    BridgeSurface {
        id: stamp.id.clone(),
        level: walk.level,
        start: connections.and_then(|c| c.start),
        end: connections.and_then(|c| c.end),
        x: c * cx + sn * cz,
        z: -sn * cx + c * cz,
        base: transform.height,
        c,
        s: sn,
        width: hi_x - lo_x,
        depth: hi_z - lo_z,
        height: 0.0,
        arch: 0.0,
        rise: 0.0,
        thickness: MESH_THICKNESS,
        ramps,
        triangles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck_entry() -> SceneryEntry {
        serde_json::from_value(serde_json::json!({
            "id": "bridge", "name": "Bridge", "category": "landmark", "type": "span",
            "file": "library/bridge.glb",
            "deck": {"width": 4, "depth": 10, "height": 0.5, "arch": 1, "rise": 2, "level": 1,
                "connections": {"start": 0, "end": 0}},
        }))
        .unwrap()
    }

    fn stamp(extra: serde_json::Value) -> Stamp {
        let mut value = serde_json::json!({"id": "s1", "asset": "bridge", "x": 10, "y": 20});
        for (k, v) in extra.as_object().unwrap() {
            value[k] = v.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn analytic_decks_arch_and_rise_along_their_length() {
        let entry = deck_entry();
        let surfaces = bridge_surfaces(
            &[stamp(serde_json::json!({"elevation": 1}))],
            |id| (id == "bridge").then_some(&entry),
            |_, _| 3.0,
        );
        let [b] = surfaces.as_slice() else {
            panic!("one deck")
        };
        assert_eq!((b.x, b.z, b.base), (10.5, 20.5, 4.0));
        assert_eq!((b.start, b.end, b.thickness), (Some(0), Some(0), 0.6));
        // Centre: base + height + arch + half the rise.
        assert_eq!(b.height_at(10.5, 20.5), Some(4.0 + 0.5 + 1.0 + 1.0));
        // Ends: arch at cos(±π/2) ≈ 0, full or no rise.
        let end = b.height_at(10.5, 25.5).unwrap();
        assert!((end - (4.0 + 0.5 + 2.0)).abs() < 1e-9, "{end}");
        assert_eq!(b.height_at(12.6, 20.5), None);
        assert_eq!(b.height_at(10.5, 25.6), None);
    }

    #[test]
    fn walk_heights_and_yaw_place_the_deck() {
        let entry = deck_entry();
        let turned = stamp(serde_json::json!({"yaw": std::f64::consts::FRAC_PI_2,
            "walk": {"level": 2, "height": 9}}));
        let surfaces = bridge_surfaces(&[turned], |_| Some(&entry), |_, _| 0.0);
        let b = &surfaces[0];
        assert_eq!(b.level, 2);
        // Walk height names the centre top: base = 9 − (0.5 + 1 + 2/2).
        assert_eq!(b.base, 6.5);
        // Turned a quarter: the long axis now runs along x.
        assert!(b.height_at(15.0, 20.5).is_some());
        assert!(b.height_at(10.5, 25.0).is_none());
    }

    #[test]
    fn imported_meshes_follow_their_source_transform() {
        let mesh = stamp(serde_json::json!({
            "asset": "custom",
            "walk": {"level": 3, "mesh": {
                "positions": [[-1, 0, -2], [1, 0, -2], [1, 1, 2], [-1, 1, 2]],
                "indices": [0, 1, 2, 0, 2, 3]}},
            "sourceTransform": {"height": 5, "quaternion": [0, 0, 0, 1]},
        }));
        let surfaces = bridge_surfaces(&[mesh], |_| None, |_, _| 0.0);
        let b = &surfaces[0];
        assert_eq!((b.level, b.base, b.width, b.depth), (3, 5.0, 2.0, 4.0));
        assert_eq!((b.x, b.z), (10.5, 20.5));
        assert_eq!(b.ramps, [4.0, 0.0]);
        let near = |h: Option<f64>, expected: f64| h.is_some_and(|h| (h - expected).abs() < 1e-9);
        assert!(near(b.height_at(10.5, 20.5), 5.5));
        assert!(near(b.height_at(10.5, 22.5), 6.0));
        assert_eq!(b.height_at(12.0, 20.5), None);
    }

    #[test]
    fn stamps_without_a_deck_are_not_surfaces() {
        let surfaces = bridge_surfaces(&[stamp(serde_json::json!({}))], |_| None, |_, _| 0.0);
        assert!(surfaces.is_empty());
    }
}
