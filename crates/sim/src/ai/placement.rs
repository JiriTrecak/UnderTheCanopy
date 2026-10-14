//! Where the AI may build (old `Frame.placeable`, `doorReachable`, `resourceClear` and
//! `ai/placementSearch.ts`): the simulation's foundation rules applied to what the
//! player knows. Blockers it has not seen are not consulted (the simulation still refuses
//! such a site, and the AI then avoids it for a while), so placement never reveals
//! hidden buildings or units.

use super::{
    frame::Frame,
    geography::{Pt, dist, round},
    navigation::ObservedNavigation,
};
use crate::{
    catalog::Def,
    entity::Milli,
    navigation::surfaces::clear_layered_sweep,
    spatial::{Fixed, adjacent::AdjacentSweep, geometry::sweep_pattern},
    terrain::{
        CellBounds, MAX_GROUND_STEP_CM,
        footprint::{
            BUILDING_CELL, Extent, GRID_ORIGIN, body_overlaps_bounds, footprint_bounds,
            footprint_cells, foundation_fits, placement_geometry_error,
        },
        round_half_up,
    },
};
use canopy_content::game::{Definition, Kind};

/// Snaps a cursor to where a definition may stand: the building lattice for buildings,
/// cell centres otherwise (old `snapPlacement`). `rotation` in degrees.
pub fn snap(d: &Definition, p: Pt, rotation: i32) -> Pt {
    match (&d.kind, &d.footprint) {
        (Kind::Building, Some(f)) => {
            let e = Extent::of(Some(f)).rotated(f64::from(rotation));
            let snap = |centre: f64, extent: f64| {
                GRID_ORIGIN
                    + round((centre - extent / 2.0 - GRID_ORIGIN) / BUILDING_CELL) * BUILDING_CELL
                    + extent / 2.0
            };
            Pt {
                x: snap(p.x, e.width),
                y: snap(p.y, e.depth),
                deck: p.deck,
            }
        }
        _ => Pt {
            x: round(p.x),
            y: round(p.y),
            deck: p.deck,
        },
    }
}

/// The doorway of `def` standing at `p` turned by `rotation` degrees.
pub fn entrance(f: &Frame, def: Def, p: Pt, rotation: i32) -> Pt {
    let door = crate::economy::sites::entrance(
        f.catalog(),
        def,
        Milli {
            x: round_half_up(p.x * 1000.0) as i32,
            y: round_half_up(p.y * 1000.0) as i32,
        },
        rotation * 1000,
    );
    Pt::of(door, p.deck)
}

fn overlaps(a: &CellBounds, b: &CellBounds, pad: i64) -> bool {
    a.min_x <= b.max_x + pad
        && a.max_x >= b.min_x - pad
        && a.min_y <= b.max_y + pad
        && a.max_y >= b.min_y - pad
}

/// Whether the player may place `def` at `p` turned by `rotation` degrees, as far as it
/// knows: prerequisites, the lattice, explored dry land without known blockers or seen
/// units, flat ground, resource access lanes, a lane around its own buildings, and a
/// doorway its workers can walk to.
pub fn placeable(f: &Frame, def: Def, p: Pt, rotation: i32) -> bool {
    let _span = tracing::trace_span!("ai site check").entered();
    let d = f.definition(def);
    let geo = f.geo;
    if !f.available(d.requires.as_deref())
        || p.deck.is_some()
        || placement_geometry_error(d, p.x, p.y, f64::from(rotation)).is_some()
    {
        return false;
    }
    if let Some(near) = &d.placement_near {
        let source = f.find(near.source.as_str());
        let found = f.resources.iter().any(|&i| {
            let e = f.at(i);
            !e.remembered && Some(e.def) == source && dist(e.p, p) <= f64::from(near.radius)
        });
        if !found {
            return false;
        }
    }
    let extent = Extent::of(d.footprint.as_ref());
    let cells = footprint_cells(p.x, p.y, extent, f64::from(rotation));
    let size = i64::from(geo.size);
    for (x, y) in cells.cells() {
        if x < 0 || y < 0 || x >= size || y >= size {
            return false;
        }
        let i = (y * size + x) as u32;
        if !f.land(i)
            || geo.terrain.surfaces().is_some_and(|s| s.has_deck(i))
            || f.blocked(i)
            || f.explored(i) == 0
        {
            return false;
        }
    }
    let terrain = &geo.terrain;
    if !foundation_fits(cells, |x, y| {
        i32::from(terrain.height_cm((y * size + x) as usize))
    }) {
        return false;
    }
    let bounds = footprint_bounds(p.x, p.y, extent, f64::from(rotation));
    let profiles = f.input.profiles();
    if f.all.iter().any(|e| {
        e.unit && !e.contained && !e.remembered && {
            let body = profiles.get(e.def).body;
            !body.air && body_overlaps_bounds(e.p.x, e.p.y, body.radius, bounds)
        }
    }) {
        return false;
    }
    if !resource_clear(f, &cells) {
        return false;
    }
    // Keep a lane for the largest trainable ground body around own buildings.
    let (radius, _) = f.base_body();
    let lane = (radius * 2.0).ceil() as i64;
    for &i in &f.buildings {
        let b = f.at(i);
        let bd = f.def(b);
        let occupied = footprint_cells(
            b.p.x,
            b.p.y,
            Extent::of(bd.footprint.as_ref()),
            f64::from(b.rotation) / 1000.0,
        );
        if overlaps(&cells, &occupied, lane) {
            return false;
        }
    }
    let door = entrance(f, def, p, rotation);
    if !geo.connected(f.home, door) || geo.index(door).is_some_and(|i| f.blocked(i)) {
        return false;
    }
    // Outposts check a local approach: a home-centred flood would reject every remote
    // resource site.
    let approach = if d.placement_near.is_some() {
        f.nearest_safe(Pt::new(p.x, p.y + 12.0), &[])
    } else {
        f.home
    };
    door_reachable(f, &cells, approach, door, d.placement_near.is_some())
}

/// Whether the footprint keeps out of every known resource's construction clearance.
fn resource_clear(f: &Frame, candidate: &CellBounds) -> bool {
    !f.resources.iter().any(|&i| {
        let r = f.at(i);
        let d = f.def(r);
        let Some(padding) = d.construction_clearance else {
            return false;
        };
        let b = footprint_cells(
            r.p.x,
            r.p.y,
            Extent::of(d.footprint.as_ref()),
            f64::from(r.rotation) / 1000.0,
        );
        overlaps(candidate, &b, i64::from(padding))
    })
}

/// Points within `radius` cells (square) of `p` on the map, nearest first, then by row
/// and column.
fn nearby(f: &Frame, p: Pt, radius: i32) -> Vec<(i32, i32)> {
    let c = p.rounded();
    let size = f.geo.size as i32;
    let mut points = Vec::new();
    for y in -radius..=radius {
        for x in -radius..=radius {
            let (qx, qy) = (c.x as i32 + x, c.y as i32 + y);
            if qx >= 0 && qy >= 0 && qx < size && qy < size {
                points.push((qx, qy));
            }
        }
    }
    let d = |q: &(i32, i32)| dist(Pt::new(f64::from(q.0), f64::from(q.1)), p);
    points.sort_by(|a, b| {
        d(a).total_cmp(&d(b))
            .then(a.1.cmp(&b.1))
            .then(a.0.cmp(&b.0))
    });
    points
}

/// Whether a worker of the widest ground body could still walk from `approach` to the
/// doorway with the foundation in place (old `doorReachable`).
fn door_reachable(f: &Frame, cells: &CellBounds, approach: Pt, door: Pt, outpost: bool) -> bool {
    let geo = f.geo;
    let size = geo.size;
    let (radius_cells, height) = f.base_body();
    let radius = round_half_up(radius_cells * 1000.0) as i32;
    let proposed = |c: u32| {
        let (x, y) = (i64::from(c % size), i64::from(c / size));
        c < size * size && cells.contains(x, y)
    };
    let (min_x, max_x) = (cells.min_x as f64 - 0.5, cells.max_x as f64 + 0.5);
    let (min_y, max_y) = (cells.min_y as f64 - 0.5, cells.max_y as f64 + 0.5);
    let heights = geo.terrain.heights();
    let base_step = |a: u32, b: u32| {
        f.land(b)
            && !f.blocked(b)
            && (i32::from(heights[a as usize]) - i32::from(heights[b as usize])).abs()
                <= MAX_GROUND_STEP_CM
    };
    let check = |nav: Option<&mut ObservedNavigation>| -> bool {
        let adjacent = AdjacentSweep::new(radius);
        let pattern = sweep_pattern(radius);
        let point = |c: u32| Fixed::cell((c % size) as i32, (c / size) as i32, None);
        let exact = |a: u32, b: u32, overlay: bool| -> bool {
            match geo.terrain.surfaces() {
                Some(s) => {
                    let walkable =
                        |id: u32| s.walkable(id) && !f.blocked(id) && !(overlay && proposed(id));
                    clear_layered_sweep(
                        s,
                        point(a),
                        point(b),
                        height,
                        radius,
                        &pattern,
                        &walkable,
                        None,
                    )
                }
                None => adjacent.clear(size, a, b, |x, y| {
                    base_step(x, y) && !(overlay && proposed(y))
                }),
            }
        };
        let certified = |a: u32, b: u32, overlay: bool| -> bool {
            if let Some(nav) = nav.as_deref() {
                let (ax, ay) = (f64::from(a % size), f64::from(a / size));
                let (bx, by) = (f64::from(b % size), f64::from(b / size));
                let r = f64::from(radius) / 1000.0;
                let outside = !overlay
                    || ax.max(bx) + r < min_x
                    || ax.min(bx) - r > max_x
                    || ay.max(by) + r < min_y
                    || ay.min(by) - r > max_y;
                if outside && nav.proves_ground_sweep(geo, point(a), point(b), radius) {
                    return true;
                }
            }
            exact(a, b, overlay)
        };
        let index = |q: (i32, i32)| q.1 as u32 * size + q.0 as u32;
        let Some(origin) = nearby(f, approach, 8)
            .into_iter()
            .find(|&q| certified(index(q), index(q), false))
        else {
            return false;
        };
        if !certified(index(origin), index(origin), true) {
            return false;
        }
        let mut goals: Vec<u32> = nearby(f, door, radius_cells.ceil() as i32 + 2)
            .into_iter()
            .map(index)
            .filter(|&i| certified(i, i, true))
            .collect();
        if goals.is_empty() {
            return false;
        }
        goals.sort_unstable();
        flood(
            f,
            origin,
            if outpost { 32.0 } else { 60.0 },
            proposed,
            |a, b| certified(a, b, true),
            |c| goals.binary_search(&c).is_ok(),
        )
    };
    if geo.layered() {
        check(None)
    } else {
        f.routes(|nav| check(Some(nav)))
    }
}

/// The bounded four-neighbour flood from `origin` (old `PlacementSearch.reachable`),
/// including its unmarked first node and 4096-node budget.
fn flood(
    f: &Frame,
    origin: (i32, i32),
    radius: f64,
    proposed: impl Fn(u32) -> bool,
    step: impl Fn(u32, u32) -> bool,
    goal: impl Fn(u32) -> bool,
) -> bool {
    let geo = f.geo;
    let size = geo.size as i32;
    let (ox, oy) = (f64::from(origin.0), f64::from(origin.1));
    let min_x = (ox - radius).floor() as i32;
    let min_y = (oy - radius).floor() as i32;
    let width = (ox + radius).ceil() as i32 - min_x + 1;
    let height = (oy + radius).ceil() as i32 - min_y + 1;
    let mut seen = vec![false; (width * height) as usize];
    let mut queue = vec![origin];
    let heights = geo.terrain.heights();
    let mut head = 0;
    while head < queue.len() && head < 4096 {
        let (x, y) = queue[head];
        head += 1;
        let from = (y * size + x) as u32;
        if goal(from) {
            return true;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= size || ny >= size {
                continue;
            }
            let (fx, fy) = (f64::from(nx) - ox, f64::from(ny) - oy);
            if fx * fx + fy * fy > radius * radius {
                continue;
            }
            let local = ((ny - min_y) * width + nx - min_x) as usize;
            let index = (ny * size + nx) as u32;
            if seen[local]
                || proposed(index)
                || f.blocked(index)
                || !f.land(index)
                || (i32::from(heights[index as usize]) - i32::from(heights[from as usize])).abs()
                    > MAX_GROUND_STEP_CM
                || !step(from, index)
            {
                continue;
            }
            seen[local] = true;
            queue.push((nx, ny));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn building(width: u32, depth: u32) -> Definition {
        serde_json::from_value(json!({
            "id": "building.test.snap", "kind": "building", "name": "", "description": "",
            "asset": "asset.model", "icon": "asset.icon",
            "footprint": {"width": width, "depth": depth}, "behaviors": {},
        }))
        .unwrap()
    }

    #[test]
    fn sites_snap_to_the_building_lattice() {
        // Edges sit on −0.5 + 4k, so an 8-cell footprint is centred on 4k + 3.5 and a
        // 4-cell one on 4k + 1.5.
        let hall = building(8, 8);
        let p = snap(&hall, Pt::new(40.2, 37.9), 0);
        assert_eq!((p.x, p.y), (39.5, 39.5));
        let house = building(4, 4);
        let p = snap(&house, Pt::new(40.2, 37.9), 0);
        assert_eq!((p.x, p.y), (41.5, 37.5));
        assert!(placement_geometry_error(&house, p.x, p.y, 0.0).is_none());
        // A quarter turn swaps width and depth before snapping.
        let long = building(8, 4);
        let p = snap(&long, Pt::new(40.2, 37.9), 90);
        assert_eq!((p.x, p.y), (41.5, 39.5));
        assert!(placement_geometry_error(&long, p.x, p.y, 90.0).is_none());
    }
}
