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
use crate::navigation::grid::GridNavigation;
use crate::{
    catalog::Def,
    entity::Milli,
    navigation::surfaces::clear_layered_sweep,
    spatial::{
        Fixed,
        adjacent::{AdjacentSweep, Planes},
        geometry::sweep_pattern,
    },
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
use std::cell::RefCell;

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
    // Power (D66): the player knows its own network.
    if d.power.is_some() && !f.powered(p) {
        return false;
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
    // Outposts and sites far from home (a hall at a further field) check a local approach
    // from the side facing home: a home-centred flood would reject every remote site.
    let remote = d.placement_near.is_some() || dist(p, f.home) > REMOTE_SITE;
    let approach = if remote {
        let (dx, dy) = (f.home.x - p.x, f.home.y - p.y);
        let d = (dx * dx + dy * dy).sqrt().max(1.0);
        let reach = extent.width.max(extent.depth) / 2.0 + 4.0;
        f.nearest_safe(Pt::new(p.x + dx / d * reach, p.y + dy / d * reach), &[])
    } else {
        f.home
    };
    door_reachable(f, &cells, approach, door, remote)
}

/// Sites farther than this from home check their doorway from nearby: the home-centred
/// flood reaches about this far.
const REMOTE_SITE: f64 = 40.0;

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

/// Doorway checks of one frame, where the knowledge, the navigation and the widest body
/// stay the same: what they share across sites.
#[derive(Debug, Default)]
pub struct DoorMemo {
    /// The flood origin found near each approach (it does not depend on the site).
    origins: Vec<(Approach, Option<(i32, i32)>)>,
    /// Floods that ran out without a goal and never came near their foundation.
    floods: Vec<Flood>,
}

/// An approach point by its coordinates' bits.
type Approach = (u64, u64);

/// A flood from `origin` within `radius` cells that met no goal: the cells it took, in
/// order, and as a set. A foundation that keeps away from them changes none of its steps,
/// so for any such site the flood takes the same cells, and reaches a doorway exactly
/// when one of them is a goal.
#[derive(Debug)]
struct Flood {
    origin: (i32, i32),
    radius: u64,
    taken: Vec<u32>,
    /// One bit per map cell.
    set: Vec<u64>,
}

impl Flood {
    /// Whether the foundation `cells` keeps out of reach of every step the flood takes:
    /// a step reads its two neighbouring cells and the sweep of a body of `reach` cells
    /// around them, so no taken cell within that sweep and a cell more of the foundation
    /// leaves it untouched.
    fn clear_of(&self, cells: &CellBounds, reach: i64, size: u32) -> bool {
        let margin = reach + 2;
        let last = i64::from(size) - 1;
        for y in (cells.min_y - margin).max(0)..=(cells.max_y + margin).min(last) {
            for x in (cells.min_x - margin).max(0)..=(cells.max_x + margin).min(last) {
                let c = (y * i64::from(size) + x) as usize;
                if self.set[c / 64] >> (c % 64) & 1 != 0 {
                    return false;
                }
            }
        }
        true
    }
}

/// Whether a worker of the widest ground body could still walk from `approach` to the
/// doorway with the foundation in place (old `doorReachable`).
fn door_reachable(f: &Frame, cells: &CellBounds, approach: Pt, door: Pt, outpost: bool) -> bool {
    let _span = tracing::trace_span!("ai door reach").entered();
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
    // `edges` remembers the foundation-free neighbour steps between knowledge changes.
    let check = |nav: Option<&ObservedNavigation>,
                 edges: Option<&RefCell<GridNavigation>>|
     -> bool {
        // Compiled once per radius by the navigation; without it (maps with decks) only
        // the layered sweep below is used.
        let compiled;
        let adjacent: &AdjacentSweep = match nav {
            Some(nav) => nav.compiled_sweep(radius).expect("compiled"),
            None => {
                compiled = AdjacentSweep::new(radius);
                &compiled
            }
        };
        let pattern = sweep_pattern(radius);
        let planes = nav.and_then(|nav| nav.planes(geo));
        let foundation = (
            cells.min_x as i32,
            cells.max_x as i32,
            cells.min_y as i32,
            cells.max_y as i32,
        );
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
                // The navigation's planes hold the same open cells as `base_step`.
                None => match planes {
                    Some(planes) => adjacent.clear_planes(
                        size,
                        a,
                        b,
                        Planes {
                            avoid: overlay.then_some(foundation),
                            ..planes
                        },
                        |x, y| base_step(x, y) && !(overlay && proposed(y)),
                    ),
                    None => adjacent.clear(size, a, b, |x, y| {
                        base_step(x, y) && !(overlay && proposed(y))
                    }),
                },
            }
        };
        // The exact test first: with the planes it is the cheaper one.
        let certified = |a: u32, b: u32, overlay: bool| -> bool {
            if exact(a, b, overlay) {
                return true;
            }
            if let Some(nav) = nav {
                let (ax, ay) = (f64::from(a % size), f64::from(a / size));
                let (bx, by) = (f64::from(b % size), f64::from(b / size));
                let r = f64::from(radius) / 1000.0;
                let outside = !overlay
                    || ax.max(bx) + r < min_x
                    || ax.min(bx) - r > max_x
                    || ay.max(by) + r < min_y
                    || ay.min(by) - r > max_y;
                return outside && nav.proves_ground_sweep(geo, point(a), point(b), radius);
            }
            false
        };
        let index = |q: (i32, i32)| q.1 as u32 * size + q.0 as u32;
        // Without the foundation's overlay the search for an origin is the same at every
        // site: it is made once a frame for each approach.
        let key = (approach.x.to_bits(), approach.y.to_bits());
        let known = f
            .doors
            .borrow()
            .origins
            .iter()
            .find(|(k, _)| *k == key)
            .map(|&(_, o)| o);
        let origin = known.unwrap_or_else(|| {
            let origin = nearby(f, approach, 8)
                .into_iter()
                .find(|&q| certified(index(q), index(q), false));
            f.doors.borrow_mut().origins.push((key, origin));
            origin
        });
        let Some(origin) = origin else {
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
        let flood_radius: f64 = if outpost { 32.0 } else { 60.0 };
        let reach = radius_cells.ceil() as i64;
        let is_goal = |c: u32| goals.binary_search(&c).is_ok();
        if let Some(found) = f.doors.borrow().floods.iter().find(|m| {
            m.origin == origin
                && m.radius == flood_radius.to_bits()
                && m.clear_of(cells, reach, size)
        }) {
            return found.taken.iter().any(|&c| is_goal(c));
        }
        let _span = tracing::trace_span!("ai door flood").entered();
        // The planes' closed cells are the known blockers and the water (`base_step`).
        let closed = |c: u32| match planes {
            Some(p) => {
                let (x, y) = ((c % size) as usize, (c / size) as usize);
                p.closed[y * p.words + x / 32] & (1 << (x % 32)) != 0
            }
            None => f.blocked(c) || !f.land(c),
        };
        // A step with the foundation in place is the step without it that enters none of
        // the foundation's cells; the step without it is remembered.
        let step = |a: u32, b: u32| match (edges, planes) {
            (Some(edges), Some(_)) => {
                let open = edges
                    .borrow_mut()
                    .cardinal(a, b, &mut |x, y| exact(x, y, false))
                    && !adjacent.enters(size, a, b, foundation);
                open || certified(a, b, true)
            }
            _ => certified(a, b, true),
        };
        let mut taken = Vec::new();
        let reached = flood(
            f,
            origin,
            flood_radius,
            proposed,
            closed,
            step,
            is_goal,
            &mut taken,
        );
        if !reached {
            let mut set = vec![0u64; (size * size).div_ceil(64) as usize];
            for &c in &taken {
                set[c as usize / 64] |= 1 << (c % 64);
            }
            let flood = Flood {
                origin,
                radius: flood_radius.to_bits(),
                taken,
                set,
            };
            // Only a flood its own foundation left untouched is every far site's flood.
            if flood.clear_of(cells, reach, size) {
                f.doors.borrow_mut().floods.push(flood);
            }
        }
        reached
    };
    if geo.layered() {
        check(None, None)
    } else {
        f.routes(|nav| {
            nav.sweep(radius);
            let edges = RefCell::new(nav.take_door_edges(radius));
            let reached = check(Some(&*nav), Some(&edges));
            nav.put_door_edges(radius, edges.into_inner());
            reached
        })
    }
}

/// The bounded four-neighbour flood from `origin` (old `PlacementSearch.reachable`),
/// including its unmarked first node and 4096-node budget. The cells it takes are added
/// to `taken` in order.
#[allow(clippy::too_many_arguments)]
fn flood(
    f: &Frame,
    origin: (i32, i32),
    radius: f64,
    proposed: impl Fn(u32) -> bool,
    closed: impl Fn(u32) -> bool,
    step: impl Fn(u32, u32) -> bool,
    goal: impl Fn(u32) -> bool,
    taken: &mut Vec<u32>,
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
        taken.push(from);
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
                || closed(index)
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
            "model": "asset.model", "icon": "asset.icon",
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
