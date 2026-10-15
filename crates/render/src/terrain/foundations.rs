//! Building foundations (D44): buildings stand at their centre's ground height, so on
//! sloping ground part of a footprint lies above the terrain. There the ground itself
//! rises to meet the building, as if the terrain were sculpted under it: a **pad** over
//! the footprint (its corners rounded) just under the base, and a **mound** around it
//! falling with a rounded shoulder and easing tangentially into the ground at its toe, as
//! wide as a gentle fill slope needs (about 32° at its steepest) up to [`MAX_BANK`], with
//! an organic outline. Heights are pulled from the terrain's height texture in the
//! vertex shader, so where the ground is level with the base or higher nothing changes.
//!
//! The pad and mound are shaded as the terrain at the same place (`ground_at` in
//! `terrain.wgsl`: the same layers, world-space mapping, lighting, shadows and fog), lit
//! with their own normal turning into the terrain's at the toe. One instanced draw and
//! one shadow draw for the footprints in view.
//!
//! [`Foundation::surface`] gives the mound's height on the CPU (the same formula) so ground
//! marks of the world UI drape over it. Visual only: the simulation knows nothing of it.

use glam::Vec3;

// Must match `terrain.wgsl`.
/// The pad and the mound's rim lie this far under the base (units)…
pub const LIFT: f32 = 0.05;
/// …and the toe this far under the ground.
pub const TOE_SINK: f32 = 0.02;
/// The mound's width: at least, per unit of drop at the rim, and at most (units).
pub const MIN_BANK: f32 = 0.4;
pub const BATTER: f32 = 2.4;
pub const MAX_BANK: f32 = 5.0;
/// The rim's corner radius: a share of the shorter half side, at most this (units).
pub const CORNER_SHARE: f32 = 0.4;
pub const MAX_CORNER: f32 = 3.0;
/// Lowest the mound's toe may lie under the base, for culling (units).
pub const MAX_DROP: f32 = 24.0;
/// Vertices of one foundation: the mound (spokes × bands × two triangles) and the pad (a
/// fan of one triangle per spoke).
pub const VERTICES: u32 = SPOKES * BANDS * 6 + SPOKES * 3;
/// Spokes around the rim: per side, the straight samples and the corner's arc.
pub const SPOKES: u32 = 4 * (SIDE_SEGMENTS + CORNER_STEPS);
const SIDE_SEGMENTS: u32 = 14;
const CORNER_STEPS: u32 = 6;
/// Rings between the rim and the toe.
const BANDS: u32 = 8;

/// One building's footprint as drawn: axis-aligned (buildings turn in quarter turns).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Foundation {
    /// Centre (x, z).
    pub center: [f32; 2],
    /// Half the width (x) and depth (z).
    pub half: [f32; 2],
    /// The building's base height (its centre's ground).
    pub base: f32,
}

/// How the mound's width varies along the rim (`edge`: a point on it), 0.7–1.3; the
/// shader's `ragged` is the same formula.
pub fn ragged(edge: [f32; 2], center: [f32; 2]) -> f32 {
    let s = edge[0] + edge[1];
    let phase = center[0] * 0.37 + center[1] * 0.61;
    1.0 + 0.2 * (0.45 * s + phase).sin() + 0.1 * (1.1 * s + 1.7 * phase).sin()
}

/// The mound's profile from its rim (0) to its toe (1).
fn profile(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

impl Foundation {
    /// The box its drawing may cover (for culling).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let reach = |h: f32| h + MAX_BANK * 1.3;
        (
            Vec3::new(
                self.center[0] - reach(self.half[0]),
                self.base - MAX_DROP,
                self.center[1] - reach(self.half[1]),
            ),
            Vec3::new(
                self.center[0] + reach(self.half[0]),
                self.base + 0.5,
                self.center[1] + reach(self.half[1]),
            ),
        )
    }

    fn corner_radius(&self) -> f32 {
        (CORNER_SHARE * self.half[0].min(self.half[1])).min(MAX_CORNER)
    }

    /// The mound's width at a rim point.
    fn width(&self, edge: [f32; 2], ground: &dyn Fn(f32, f32) -> f32) -> f32 {
        let drop = (self.base - LIFT - ground(edge[0], edge[1])).max(0.0);
        ((drop * BATTER).max(MIN_BANK) * ragged(edge, self.center)).clamp(MIN_BANK, MAX_BANK)
    }

    /// Height of the pad or mound at `(x, z)` if it covers that point, whether or not the
    /// ground hides it there (`ground` gives the terrain's height).
    pub fn surface(&self, x: f32, z: f32, ground: &dyn Fn(f32, f32) -> f32) -> Option<f32> {
        let top = self.base - LIFT;
        let r = self.corner_radius();
        let (ix, iz) = (self.half[0] - r, self.half[1] - r);
        let (ox, oz) = (x - self.center[0], z - self.center[1]);
        // The nearest point of the inner rectangle, and the way out from it.
        let (px, pz) = (ox.clamp(-ix, ix), oz.clamp(-iz, iz));
        let (vx, vz) = (ox - px, oz - pz);
        let v = (vx * vx + vz * vz).sqrt();
        if v <= r {
            return Some(top);
        }
        let edge = [
            self.center[0] + px + vx / v * r,
            self.center[1] + pz + vz / v * r,
        ];
        let u = (v - r) / self.width(edge, ground);
        if u > 1.0 {
            return None;
        }
        Some(top + (ground(x, z) - TOE_SINK - top) * profile(u))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUSE: Foundation = Foundation {
        center: [10.0, 10.0],
        half: [4.0, 4.0],
        base: 1.0,
    };

    #[test]
    fn level_ground_hides_the_foundation() {
        let flat = |_: f32, _: f32| 1.0;
        for (x, z) in [
            (10.0, 10.0),
            (14.1, 10.0),
            (14.1, 14.1),
            (6.0, 5.9),
            (13.9, 13.9),
        ] {
            if let Some(h) = HOUSE.surface(x, z, &flat) {
                assert!(h < 1.0, "{x},{z}: {h}");
            }
        }
        // The mound is no wider than its minimum there.
        assert!(HOUSE.surface(14.0 + MIN_BANK * 1.35, 10.0, &flat).is_none());
    }

    #[test]
    fn the_mound_rises_where_the_ground_falls_away_and_eases_into_it() {
        // Ground falling 0.5 per unit to the east: the east edge is 2 below the base.
        let slope = |x: f32, _: f32| 1.0 - (x - 10.0) * 0.5;
        let near = HOUSE.surface(14.3, 10.0, &slope).expect("on the mound");
        assert!(near > slope(14.3, 10.0) + 1.0, "{near}");
        // It eases into the ground at its toe.
        let toe = 14.0 + (1.95 * BATTER * ragged([14.0, 10.0], HOUSE.center)).min(MAX_BANK);
        let h = HOUSE.surface(toe - 0.01, 10.0, &slope).expect("at the toe");
        assert!((h - slope(toe - 0.01, 10.0)).abs() < 0.05, "{h}");
        // It is gone beyond its widest extent.
        assert!(HOUSE.surface(14.0 + MAX_BANK * 1.3, 10.0, &slope).is_none());
        // Uphill (west) it stays under the ground.
        let west = HOUSE.surface(5.9, 10.0, &slope).expect("minimal mound");
        assert!(west < slope(5.9, 10.0));
        // The pad is flat at the base.
        assert_eq!(HOUSE.surface(12.0, 11.0, &slope), Some(1.0 - LIFT));
    }
}
