//! Ground cells closed by scenery collision (old `applySceneryBlockers`,
//! `src/shared/map/sceneryCollision.ts`).

use crate::math;
use canopy_content::{
    assets::{Blocker, BlockerShape},
    map::Stamp,
};

/// Padding around every blocker, in world units before the stamp's scale.
const MARGIN: f64 = 0.35;

/// Marks the cells under each stamp's blockers as not walkable. Stamp positions are cell
/// centres, yaws radians; blockers are rectangles or ellipses in asset-local metres.
pub fn apply<'a>(
    size: u32,
    stamps: &[Stamp],
    blockers: impl Fn(&str) -> &'a [Blocker],
    walkable: &mut [bool],
) {
    let n = size as usize;
    for stamp in stamps {
        let list = blockers(&stamp.asset);
        if list.is_empty() {
            continue;
        }
        let scale = stamp.scale.unwrap_or(1.0);
        let sx = scale * stamp.width_scale.unwrap_or(1.0);
        let sz = scale * stamp.depth_scale.unwrap_or(1.0);
        let yaw = stamp.yaw.unwrap_or(0.0);
        let (c, s) = (math::cos(yaw), math::sin(yaw));
        for b in list {
            let byaw = b.yaw.unwrap_or(0.0);
            let (bc, bs) = (math::cos(byaw), math::sin(byaw));
            let (bx, bz) = (b.x.unwrap_or(0.0), b.z.unwrap_or(0.0));
            let margin = MARGIN / sx.abs().min(sz.abs());
            let (hw, hd) = (b.width / 2.0 + margin, b.depth / 2.0 + margin);
            let (mut lo_x, mut hi_x, mut lo_z, mut hi_z) = (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            );
            for x in [-hw, hw] {
                for z in [-hd, hd] {
                    let lx = (bx + bc * x + bs * z) * sx;
                    let lz = (bz - bs * x + bc * z) * sz;
                    let wx = stamp.x + c * lx + s * lz;
                    let wz = stamp.y - s * lx + c * lz;
                    lo_x = lo_x.min(wx);
                    hi_x = hi_x.max(wx);
                    lo_z = lo_z.min(wz);
                    hi_z = hi_z.max(wz);
                }
            }
            let last = f64::from(size) - 1.0;
            let (y0, y1) = (lo_z.floor().max(0.0), hi_z.ceil().min(last));
            let (x0, x1) = (lo_x.floor().max(0.0), hi_x.ceil().min(last));
            if !(y0 <= y1 && x0 <= x1) {
                continue;
            }
            for y in y0 as usize..=y1 as usize {
                for x in x0 as usize..=x1 as usize {
                    // Invert the whole stamp transform, then the blocker's own.
                    let dx = x as f64 - stamp.x;
                    let dz = y as f64 - stamp.y;
                    let ax = (c * dx - s * dz) / sx - bx;
                    let az = (s * dx + c * dz) / sz - bz;
                    let lx = bc * ax - bs * az;
                    let lz = bs * ax + bc * az;
                    let inside = match b.shape {
                        Some(BlockerShape::Ellipse) => {
                            let (ex, ez) = (lx / hw, lz / hd);
                            ex * ex + ez * ez <= 1.0
                        }
                        None => lx.abs() <= hw && lz.abs() <= hd,
                    };
                    if inside {
                        walkable[y * n + x] = false;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocker(value: serde_json::Value) -> Blocker {
        serde_json::from_value(value).unwrap()
    }

    fn stamp(value: serde_json::Value) -> Stamp {
        let mut base = serde_json::json!({"id": "s", "asset": "rock", "x": 10, "y": 10});
        for (k, v) in value.as_object().unwrap() {
            base[k] = v.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    fn closed(stamps: &[Stamp], blockers: &[Blocker]) -> Vec<(usize, usize)> {
        let size = 32;
        let mut walkable = vec![true; size * size];
        apply(size as u32, stamps, |_| blockers, &mut walkable);
        (0..size * size)
            .filter(|&i| !walkable[i])
            .map(|i| (i % size, i / size))
            .collect()
    }

    #[test]
    fn rectangles_close_their_cells_with_a_margin() {
        let cells = closed(
            &[stamp(serde_json::json!({}))],
            &[blocker(serde_json::json!({"width": 2, "depth": 1}))],
        );
        // Half extents 1.35 × 0.85 around (10, 10).
        assert_eq!(cells, [(9, 10), (10, 10), (11, 10)]);
    }

    #[test]
    fn stamps_turn_and_scale_their_blockers() {
        let cells = closed(
            &[stamp(
                serde_json::json!({"yaw": std::f64::consts::FRAC_PI_2, "scale": 2}),
            )],
            &[blocker(
                serde_json::json!({"width": 2, "depth": 0.3, "x": 1}),
            )],
        );
        // Local x runs along world −z after a quarter turn; offset 1 × scale 2.
        assert!(cells.iter().all(|&(x, _)| x == 10), "{cells:?}");
        let ys: Vec<usize> = cells.iter().map(|&(_, y)| y).collect();
        assert_eq!(ys, [6, 7, 8, 9, 10]);
    }

    #[test]
    fn ellipses_leave_their_corners_open() {
        let cells = closed(
            &[stamp(serde_json::json!({}))],
            &[blocker(
                serde_json::json!({"width": 4, "depth": 4, "shape": "ellipse"}),
            )],
        );
        assert!(cells.contains(&(12, 10)) && cells.contains(&(10, 8)));
        assert!(!cells.contains(&(12, 12)));
    }

    #[test]
    fn blockers_off_the_map_are_clipped() {
        let cells = closed(
            &[stamp(serde_json::json!({"x": -5, "y": 40}))],
            &[blocker(serde_json::json!({"width": 2, "depth": 2}))],
        );
        assert!(cells.is_empty());
    }
}
