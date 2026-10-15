//! Building foundations (D44): buildings stand at their centre's ground height, so on
//! sloping ground part of a footprint lies above the terrain. Every footprint gets an
//! earthen foundation drawn from the terrain's own foundation layer (the biome's dirt):
//!
//! - a **pad** over the footprint just under the building's base, and a **bank** around
//!   it falling from the base to just below the terrain, steeper where the drop is
//!   small and wider where it is large, rounded at the shoulder and the toe. Its heights
//!   are pulled from the terrain's height texture in the vertex shader; one instanced
//!   draw (and one shadow draw) for every footprint in view;
//! - a **ground mask** around each footprint (R8, [`MASK_TEXELS`] texels per unit) that
//!   the terrain shader turns into worn earth under and around the building, so the
//!   foundation blends into the ground. It is rasterised on the CPU only where
//!   footprints appeared or went.
//!
//! Visual only: the simulation does not know about any of it.

use glam::Vec3;

/// Mask texels per world unit.
pub const MASK_TEXELS: u32 = 2;
/// Earth reaches this far beyond a footprint's edge fully, in units…
pub const MASK_SOLID: f32 = 0.3;
/// …and fades out over this much more.
pub const MASK_FADE: f32 = 2.7;
/// Corner radius of the earth, as a share of the shorter half side.
pub const CORNER: f32 = 0.5;
/// The widest bank beyond the footprint (units); the vertex shader's limit.
pub const MAX_BANK: f32 = 4.0;
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
    /// Whether it has a pad and a bank, or only the earth around it (resource deposits,
    /// whose models carry their own base and which workers crowd around).
    pub bank: bool,
}

impl Foundation {
    /// The box its drawing may cover (for culling).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let reach = |h: f32| h + MAX_BANK + 0.5;
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

    /// How much earth the ground at `(x, z)` shows for this footprint, 0–1.
    /// The earth is a rounded rectangle (corner radius [`CORNER`] of the shorter half
    /// side) reaching [`MASK_SOLID`] beyond the edges, then fading.
    pub fn mask(&self, x: f32, z: f32) -> f32 {
        let r = CORNER * self.half[0].min(self.half[1]);
        let dx = ((x - self.center[0]).abs() - self.half[0] + r).max(0.0);
        let dz = ((z - self.center[1]).abs() - self.half[1] + r).max(0.0);
        let d = (dx * dx + dz * dz).sqrt() - r - MASK_SOLID;
        if d <= 0.0 {
            return 1.0;
        }
        let t = (1.0 - d / MASK_FADE).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// A rectangle of mask texels, `[x0, z0, x1, z1)`.
pub type TexelRect = [u32; 4];

/// The mask texels a foundation's earth may touch, on a `size`² mask whose texel 0
/// starts at world `origin`.
pub fn texel_rect(f: &Foundation, origin: f32, size: u32) -> Option<TexelRect> {
    let reach = MASK_SOLID + MASK_FADE;
    let k = MASK_TEXELS as f32;
    let lo = |c: f32, h: f32| (((c - h - reach - origin) * k).floor().max(0.0)) as u32;
    let hi = |c: f32, h: f32| ((((c + h + reach - origin) * k).ceil()).max(0.0) as u32).min(size);
    let rect = [
        lo(f.center[0], f.half[0]),
        lo(f.center[1], f.half[1]),
        hi(f.center[0], f.half[0]),
        hi(f.center[1], f.half[1]),
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
}

/// Fills `out` (the rectangle's texels, row by row) with the earth of every foundation:
/// the strongest one at each texel.
pub fn rasterise(foundations: &[Foundation], origin: f32, rect: TexelRect, out: &mut Vec<u8>) {
    let [x0, z0, x1, z1] = rect;
    let w = (x1 - x0) as usize;
    out.clear();
    out.resize(w * (z1 - z0) as usize, 0);
    let k = MASK_TEXELS as f32;
    let world = |t: u32| origin + (t as f32 + 0.5) / k;
    for f in foundations {
        let Some([fx0, fz0, fx1, fz1]) = texel_rect(f, origin, u32::MAX) else {
            continue;
        };
        let (ax, az, bx, bz) = (fx0.max(x0), fz0.max(z0), fx1.min(x1), fz1.min(z1));
        for z in az..bz {
            let row = (z - z0) as usize * w;
            for x in ax..bx {
                let v = (f.mask(world(x), world(z)) * 255.0).round() as u8;
                let texel = &mut out[row + (x - x0) as usize];
                *texel = (*texel).max(v);
            }
        }
    }
}

/// What changed between two lists, as mask rectangles to redraw.
pub fn changed_rects(
    before: &[Foundation],
    after: &[Foundation],
    origin: f32,
    size: u32,
) -> Vec<TexelRect> {
    let gone = before.iter().filter(|f| !after.contains(f));
    let new = after.iter().filter(|f| !before.contains(f));
    gone.chain(new)
        .filter_map(|f| texel_rect(f, origin, size))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUSE: Foundation = Foundation {
        center: [10.0, 10.0],
        half: [4.0, 4.0],
        base: 1.0,
        bank: true,
    };

    #[test]
    fn earth_covers_the_footprint_and_fades_beyond() {
        assert_eq!(HOUSE.mask(10.0, 10.0), 1.0);
        assert_eq!(HOUSE.mask(14.0 + MASK_SOLID, 10.0), 1.0);
        let fading = HOUSE.mask(14.0 + MASK_SOLID + MASK_FADE / 2.0, 10.0);
        assert!(fading > 0.3 && fading < 0.7, "{fading}");
        assert_eq!(HOUSE.mask(14.0 + MASK_SOLID + MASK_FADE, 10.0), 0.0);
        // Corners round off: the footprint's own corner is already fading, and the earth
        // ends sooner along the diagonal than along a side.
        let corner = HOUSE.mask(14.0, 14.0);
        assert!(corner > 0.0 && corner < 1.0, "{corner}");
        let d = (MASK_SOLID + MASK_FADE) * 0.8;
        assert!(HOUSE.mask(14.0 + d, 10.0) > 0.0);
        assert_eq!(HOUSE.mask(14.0 + d, 14.0 + d), 0.0);
    }

    #[test]
    fn the_mask_keeps_the_strongest_earth_and_only_changed_rects_redraw() {
        let rect = texel_rect(&HOUSE, 0.0, 64).expect("on the mask");
        let mut out = Vec::new();
        rasterise(&[HOUSE], 0.0, rect, &mut out);
        let w = (rect[2] - rect[0]) as usize;
        let at = |x: u32, z: u32| out[(z - rect[1]) as usize * w + (x - rect[0]) as usize];
        // Texel 20 is centred on 10.25.
        assert_eq!(at(20, 20), 255);
        assert_eq!(at(rect[0], rect[1]), 0);
        let near = Foundation {
            center: [16.0, 10.0],
            ..HOUSE
        };
        let mut both = Vec::new();
        rasterise(&[HOUSE, near], 0.0, rect, &mut both);
        assert!(both.iter().zip(&out).all(|(b, a)| b >= a));
        assert!(both.iter().zip(&out).any(|(b, a)| b > a));
        // A footprint off the mask draws nothing.
        assert!(
            texel_rect(
                &Foundation {
                    center: [-50.0, 5.0],
                    ..HOUSE
                },
                0.0,
                64
            )
            .is_none()
        );
        let rects = changed_rects(&[HOUSE], &[HOUSE, near], 0.0, 64);
        assert_eq!(rects, vec![texel_rect(&near, 0.0, 64).unwrap()]);
        assert!(changed_rects(&[HOUSE, near], &[HOUSE, near], 0.0, 64).is_empty());
    }
}
