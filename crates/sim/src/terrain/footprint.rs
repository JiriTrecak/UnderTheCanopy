//! Footprint geometry on the navigation grid (old `src/shared/spatial/footprint.ts`,
//! `placement.ts`, `foundation.ts`, and the footprint helpers of
//! `src/shared/map/resourceClearance.ts`).
//!
//! Navigation cells are centred on integers with edges at `n ± 0.5`; buildings snap to a
//! lattice of four navigation cells anchored at `-0.5`.

use super::round_half_up;
use canopy_content::game::{Definition, Footprint, Kind};

/// Navigation cells per building cell.
pub const BUILDING_CELL: f64 = 4.0;
/// Corner of navigation cell 0.
pub const GRID_ORIGIN: f64 = -0.5;
/// Largest step between neighbouring foundation cells, in centimetres (D44): cliff faces
/// and ramps rise more, ordinary rolling ground less. There is no limit on the relief
/// across a footprint; buildings sit at their centre's height on a drawn foundation.
pub const MAX_FOUNDATION_STEP_CM: i32 = 80;

/// Width (x) and depth (y) in navigation cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub width: f64,
    pub depth: f64,
}

impl Extent {
    pub const CELL: Self = Self {
        width: 1.0,
        depth: 1.0,
    };

    pub fn of(footprint: Option<&Footprint>) -> Self {
        footprint.map_or(Self::CELL, |f| Self {
            width: f64::from(f.width),
            depth: f64::from(f.depth),
        })
    }

    /// Swapped on odd quarter turns (`rotation` in degrees).
    pub fn rotated(self, rotation: f64) -> Self {
        if (round_half_up(rotation / 90.0) as i64) % 2 != 0 {
            Self {
                width: self.depth,
                depth: self.width,
            }
        } else {
            self
        }
    }
}

/// An axis-aligned rectangle; the meaning of the edges depends on the producer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// Inclusive integer cell bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellBounds {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}

impl CellBounds {
    pub fn contains(&self, x: i64, y: i64) -> bool {
        (self.min_x..=self.max_x).contains(&x) && (self.min_y..=self.max_y).contains(&y)
    }

    /// Cells in row order.
    pub fn cells(self) -> impl Iterator<Item = (i64, i64)> {
        (self.min_y..=self.max_y).flat_map(move |y| (self.min_x..=self.max_x).map(move |x| (x, y)))
    }

    /// Whether every cell lies on a `size`-cell map.
    pub fn inside(&self, size: u32) -> bool {
        let size = i64::from(size);
        self.min_x >= 0 && self.min_y >= 0 && self.max_x < size && self.max_y < size
    }
}

/// Physical edges of a footprint centred at `(x, y)`.
pub fn footprint_bounds(x: f64, y: f64, extent: Extent, rotation: f64) -> Bounds {
    let Extent { width, depth } = extent.rotated(rotation);
    Bounds {
        min_x: x - width / 2.0,
        min_y: y - depth / 2.0,
        max_x: x + width / 2.0,
        max_y: y + depth / 2.0,
    }
}

/// Cells whose area overlaps the footprint; arbitrary centres are covered conservatively.
pub fn footprint_cells(x: f64, y: f64, extent: Extent, rotation: f64) -> CellBounds {
    let b = footprint_bounds(x, y, extent, rotation);
    CellBounds {
        min_x: (b.min_x - 0.5).floor() as i64 + 1,
        min_y: (b.min_y - 0.5).floor() as i64 + 1,
        max_x: (b.max_x + 0.5).ceil() as i64 - 1,
        max_y: (b.max_y + 0.5).ceil() as i64 - 1,
    }
}

/// Whether every footprint edge lies on the `step` lattice.
pub fn footprint_aligned(x: f64, y: f64, extent: Extent, rotation: f64, step: f64) -> bool {
    let b = footprint_bounds(x, y, extent, rotation);
    [b.min_x, b.min_y, b.max_x, b.max_y]
        .into_iter()
        .all(|edge| ((edge - GRID_ORIGIN) / step).fract() == 0.0)
}

/// Distance between the outer cell centres and the footprint centre.
pub fn half_extents(extent: Extent, rotation: f64) -> (f64, f64) {
    let Extent { width, depth } = extent.rotated(rotation);
    ((width - 1.0) / 2.0, (depth - 1.0) / 2.0)
}

/// Smallest centre separation between a building and a resource that keeps `clearance`
/// free cells between them (old `resourceCenterSeparation`).
pub fn resource_center_separation(
    building: Extent,
    resource: Extent,
    clearance: f64,
    building_rotation: f64,
    resource_rotation: f64,
) -> (f64, f64) {
    let a = half_extents(building, building_rotation);
    let b = half_extents(resource, resource_rotation);
    (a.0 + b.0 + clearance + 1.0, a.1 + b.1 + clearance + 1.0)
}

/// Whether the square navigation body of `radius` around `(x, y)` touches `bounds`. A
/// cell's lower edge belongs to it, its upper edge does not.
pub fn body_overlaps_bounds(x: f64, y: f64, radius: f64, bounds: Bounds) -> bool {
    x + radius >= bounds.min_x
        && x - radius < bounds.max_x
        && y + radius >= bounds.min_y
        && y - radius < bounds.max_y
}

/// Why a definition cannot stand at a position, if it cannot: buildings turn in quarter
/// turns on the building lattice, everything else stands on cell centres.
pub fn placement_geometry_error(
    definition: &Definition,
    x: f64,
    y: f64,
    rotation: f64,
) -> Option<&'static str> {
    if definition.kind == Kind::Building {
        if !rotation.is_finite() || rotation % 90.0 != 0.0 {
            return Some("Buildings rotate in 90-degree steps");
        }
        let aligned = definition
            .footprint
            .as_ref()
            .is_some_and(|f| footprint_aligned(x, y, Extent::of(Some(f)), rotation, BUILDING_CELL));
        if !aligned {
            return Some("Align the building footprint to the building grid");
        }
    } else if x.fract() != 0.0 || y.fract() != 0.0 {
        return Some("Place units and resources on navigation cell centers");
    }
    None
}

/// Whether ground within `bounds` is buildable (D44, as Warcraft III's buildability): no
/// cell rises more than [`MAX_FOUNDATION_STEP_CM`] into a 4-neighbour inside the bounds,
/// which rules out cliff faces and ramps but not hills. `height_cm` gives a cell's height.
pub fn foundation_fits(bounds: CellBounds, height_cm: impl Fn(i64, i64) -> i32) -> bool {
    let mut any = false;
    for (x, y) in bounds.cells() {
        let h = height_cm(x, y);
        any = true;
        if (x > bounds.min_x && (h - height_cm(x - 1, y)).abs() > MAX_FOUNDATION_STEP_CM)
            || (y > bounds.min_y && (h - height_cm(x, y - 1)).abs() > MAX_FOUNDATION_STEP_CM)
        {
            return false;
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprints_cover_their_declared_cells() {
        let four = Extent {
            width: 4.0,
            depth: 2.0,
        };
        // A 4×2 footprint centred between cells.
        assert_eq!(
            footprint_cells(10.5, 20.5, four, 0.0),
            CellBounds {
                min_x: 9,
                min_y: 20,
                max_x: 12,
                max_y: 21
            }
        );
        assert_eq!(
            footprint_cells(10.5, 20.5, four, 90.0),
            CellBounds {
                min_x: 10,
                min_y: 19,
                max_x: 11,
                max_y: 22
            }
        );
        // An odd footprint on a cell centre; an unaligned centre is covered conservatively.
        assert_eq!(
            footprint_cells(5.0, 5.0, Extent::CELL, 0.0),
            CellBounds {
                min_x: 5,
                min_y: 5,
                max_x: 5,
                max_y: 5
            }
        );
        assert_eq!(
            footprint_cells(5.25, 5.0, Extent::CELL, 0.0),
            CellBounds {
                min_x: 5,
                min_y: 5,
                max_x: 6,
                max_y: 5
            }
        );
    }

    #[test]
    fn buildings_align_to_the_building_lattice() {
        let hall = Extent {
            width: 4.0,
            depth: 4.0,
        };
        assert!(footprint_aligned(1.5, 1.5, hall, 0.0, BUILDING_CELL));
        assert!(footprint_aligned(5.5, 9.5, hall, 270.0, BUILDING_CELL));
        assert!(!footprint_aligned(2.5, 1.5, hall, 0.0, BUILDING_CELL));
        let long = Extent {
            width: 8.0,
            depth: 4.0,
        };
        assert!(footprint_aligned(3.5, 1.5, long, 0.0, BUILDING_CELL));
        assert!(!footprint_aligned(3.5, 1.5, long, 90.0, BUILDING_CELL));
        assert!(footprint_aligned(1.5, 3.5, long, 90.0, BUILDING_CELL));
    }

    #[test]
    fn resource_lanes_count_from_outer_cells() {
        let hall = Extent {
            width: 4.0,
            depth: 4.0,
        };
        assert_eq!(
            resource_center_separation(hall, Extent::CELL, 2.0, 0.0, 0.0),
            (4.5, 4.5)
        );
        let long = Extent {
            width: 6.0,
            depth: 2.0,
        };
        assert_eq!(
            resource_center_separation(long, Extent::CELL, 1.0, 90.0, 0.0),
            (2.5, 4.5)
        );
    }

    #[test]
    fn foundations_reject_steps_but_not_relief() {
        let bounds = CellBounds {
            min_x: 0,
            min_y: 0,
            max_x: 19,
            max_y: 19,
        };
        // Rolling ground: 15 m of relief across the footprint in steps of 80 cm.
        assert!(foundation_fits(bounds, |x, y| (x * 80 - y * 80) as i32));
        // A ramp and a cliff face.
        assert!(!foundation_fits(bounds, |x, _| (x * 81) as i32));
        assert!(!foundation_fits(bounds, |_, y| if y >= 12 {
            300
        } else {
            0
        }));
        // Only steps inside the bounds count.
        let inner = CellBounds {
            min_x: 0,
            min_y: 0,
            max_x: 11,
            max_y: 11,
        };
        assert!(foundation_fits(inner, |_, y| if y >= 12 { 300 } else { 0 }));
        assert!(!foundation_fits(
            CellBounds {
                min_x: 1,
                min_y: 0,
                max_x: 0,
                max_y: 0
            },
            |_, _| 0
        ));
    }

    #[test]
    fn bodies_touch_lower_edges_only() {
        let cell = Bounds {
            min_x: 4.5,
            min_y: 4.5,
            max_x: 5.5,
            max_y: 5.5,
        };
        assert!(body_overlaps_bounds(4.0, 5.0, 0.5, cell));
        assert!(!body_overlaps_bounds(6.0, 5.0, 0.5, cell));
        assert!(!body_overlaps_bounds(3.9, 5.0, 0.5, cell));
    }
}
