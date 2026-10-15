//! Building foundations (D44): buildings stand at their centre's ground height, so on
//! sloping ground part of a footprint lies above the terrain. There the ground itself
//! rises to meet the building: a **pad** over the footprint just under the base and a
//! **bank** around it falling to just below the terrain, as wide as a natural fill slope
//! needs (within limits), rounded and irregular along the edge. Heights are pulled from
//! the terrain's height texture in the vertex shader, so where the ground is level or
//! higher than the base the bank stays buried and nothing changes.
//!
//! They are shaded as the terrain at the same place (`ground_at` in `terrain.wgsl`: the
//! same layers, world-space mapping, lighting, shadows and fog); steep faces take the
//! map's own cliff material, gated by slope as terrain cliffs are. One instanced draw
//! and one shadow draw for the footprints in view.
//!
//! [`Foundation::surface`] gives the bank's height on the CPU (the same formula) so ground
//! marks of the world UI drape over it. Visual only: the simulation knows nothing of it.

use glam::Vec3;

/// The pad and the bank's top lie this far under the base (units)…
pub const LIFT: f32 = 0.05;
/// …and the toe this far under the ground.
pub const SINK: f32 = 0.3;
/// The bank's width: at least, per unit of drop at the edge, and at most (units).
pub const MIN_BANK: f32 = 0.3;
pub const BATTER: f32 = 2.0;
pub const MAX_BANK: f32 = 2.5;
/// Lowest the bank's toe may lie under the base, for culling (units).
pub const MAX_DROP: f32 = 24.0;
/// Vertices of one foundation: the bank (spokes × bands × two triangles) and the pad.
pub const VERTICES: u32 = SPOKES * BANDS * 6 + 6;
/// Spokes around a footprint: per side, the edge samples and the corner's turn.
pub const SPOKES: u32 = 4 * (SIDE_SEGMENTS + CORNER_STEPS);
const SIDE_SEGMENTS: u32 = 16;
const CORNER_STEPS: u32 = 4;
/// Rings between a spoke's top and its toe, minus one.
const BANDS: u32 = 4;

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

/// How the bank's width varies along the edge (`edge`: a point on it), 0.75–1.25; the
/// shader's `ragged` is the same formula.
pub fn ragged(edge: [f32; 2], center: [f32; 2]) -> f32 {
    let s = edge[0] + edge[1];
    let phase = center[0] * 0.37 + center[1] * 0.61;
    1.0 + 0.15 * (0.9 * s + phase).sin() + 0.1 * (2.3 * s + 1.7 * phase).sin()
}

/// The bank's profile from its top (0) to its toe (1).
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

    /// The bank's width at an edge point.
    fn width(&self, edge: [f32; 2], ground: &dyn Fn(f32, f32) -> f32) -> f32 {
        let drop = (self.base - LIFT - ground(edge[0], edge[1])).max(0.0);
        ((MIN_BANK + drop * BATTER) * ragged(edge, self.center)).clamp(MIN_BANK, MAX_BANK)
    }

    /// Height of the pad or bank at `(x, z)` if it covers that point, whether or not the
    /// ground hides it there (`ground` gives the terrain's height).
    pub fn surface(&self, x: f32, z: f32, ground: &dyn Fn(f32, f32) -> f32) -> Option<f32> {
        let top = self.base - LIFT;
        let (ox, oz) = (x - self.center[0], z - self.center[1]);
        let (hx, hz) = (self.half[0], self.half[1]);
        let edge = [
            self.center[0] + ox.clamp(-hx, hx),
            self.center[1] + oz.clamp(-hz, hz),
        ];
        let d = ((x - edge[0]).powi(2) + (z - edge[1]).powi(2)).sqrt();
        if d == 0.0 {
            return Some(top);
        }
        let u = d / self.width(edge, ground);
        if u > 1.0 {
            return None;
        }
        let toe = ground(x, z) - SINK * u;
        Some(top + (toe - top) * profile(u))
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
        for (x, z) in [(10.0, 10.0), (14.1, 10.0), (14.1, 14.1), (6.0, 5.9)] {
            if let Some(h) = HOUSE.surface(x, z, &flat) {
                assert!(h < 1.0, "{x},{z}: {h}");
            }
        }
        // The bank is no wider than its minimum there.
        assert!(HOUSE.surface(14.0 + MIN_BANK * 1.3, 10.0, &flat).is_none());
    }

    #[test]
    fn the_bank_rises_where_the_ground_falls_away_and_stays_within_its_width() {
        // Ground falling 0.5 per unit to the east: the east edge is 2 below the base.
        let slope = |x: f32, _: f32| 1.0 - (x - 10.0) * 0.5;
        let near = HOUSE.surface(14.3, 10.0, &slope).expect("on the bank");
        assert!(near > slope(14.3, 10.0) + 1.0, "{near}");
        // It is gone beyond its widest extent.
        assert!(HOUSE.surface(14.0 + MAX_BANK * 1.3, 10.0, &slope).is_none());
        // Uphill (west) it stays under the ground.
        let west = HOUSE.surface(5.9, 10.0, &slope).expect("minimal bank");
        assert!(west < slope(5.9, 10.0));
        // The pad is flat at the base.
        assert_eq!(HOUSE.surface(12.0, 11.0, &slope), Some(1.0 - LIFT));
    }
}
