//! Places around buildings and resources where work happens (old `Spatial.entrance`,
//! `perimeter`, `deployment`, `attackClear` and `Economy.workPoint`).

use super::{Ctx, precise};
use crate::{
    catalog::{Catalog, Def},
    entity::{Entity, EntityId, Milli},
    spatial::{Fixed, Spatial, Spot},
    terrain::{
        SurfacePoint,
        footprint::{Extent, footprint_cells},
        round_half_up,
    },
};

/// Quarter turns of a rotation in millidegrees, 0–3.
fn quarter(rotation: i32) -> i64 {
    (round_half_up(f64::from(rotation) / 90_000.0) as i64).rem_euclid(4)
}

/// The doorway of a building, in milli-cells (old `entrance`): the authored offset turned
/// with the building.
pub fn entrance(catalog: &Catalog, def: Def, position: Milli, rotation: i32) -> Milli {
    let (ox, oy) = catalog
        .get(def)
        .entrance
        .as_ref()
        .map_or((0.0, 0.0), |e| (e.x, e.y));
    let (x, y) = match quarter(rotation) {
        0 => (ox, oy),
        1 => (oy, -ox),
        2 => (-ox, -oy),
        _ => (-oy, ox),
    };
    Milli {
        x: position.x + round_half_up(x * 1000.0) as i32,
        y: position.y + round_half_up(y * 1000.0) as i32,
    }
}

pub fn entrance_of(catalog: &Catalog, e: &Entity) -> Milli {
    entrance(catalog, e.definition, e.position, e.rotation)
}

/// The cell a point lies in (old `cell`: halves round up).
pub fn cell_of(p: Milli) -> (i32, i32) {
    p.cell()
}

/// Cells exactly `ring` cells outside the footprint (Chebyshev), in row order, on the map
/// (old `perimeter`).
pub fn perimeter(
    catalog: &Catalog,
    size: u32,
    def: Def,
    position: Milli,
    rotation: i32,
    ring: i64,
) -> Vec<(i32, i32)> {
    let b = footprint_cells(
        f64::from(position.x) / 1000.0,
        f64::from(position.y) / 1000.0,
        Extent::of(catalog.get(def).footprint.as_ref()),
        f64::from(rotation) / 1000.0,
    );
    let (x0, x1, y0, y1) = (
        b.min_x - ring,
        b.max_x + ring,
        b.min_y - ring,
        b.max_y + ring,
    );
    let size = i64::from(size);
    let mut out = Vec::new();
    for y in y0..=y1 {
        let edge = y == y0 || y == y1;
        let mut x = x0;
        while x <= x1 {
            if x >= 0 && y >= 0 && x < size && y < size {
                out.push((x as i32, y as i32));
            }
            x += if edge { 1 } else { (x1 - x0).max(1) };
        }
    }
    out
}

/// Whether a melee blow from `from` reaches the target's footprint across the terrain
/// (old `attackClear(..., false)`).
pub fn melee_clear(
    catalog: &Catalog,
    spatial: &Spatial,
    from: (i32, i32),
    target: &Entity,
) -> bool {
    let (cx, cy) = (
        f64::from(target.position.x) / 1000.0,
        f64::from(target.position.y) / 1000.0,
    );
    let (half_x, half_y) = match &catalog.get(target.definition).footprint {
        Some(f) => {
            let rotated = quarter(target.rotation) % 2 != 0;
            let (w, d) = (f64::from(f.width), f64::from(f.depth));
            if rotated {
                (d / 2.0, w / 2.0)
            } else {
                (w / 2.0, d / 2.0)
            }
        }
        None => (0.0, 0.0),
    };
    let (ox, oy) = (f64::from(from.0), f64::from(from.1));
    let end = SurfacePoint {
        x: ox.clamp(cx - half_x, cx + half_x),
        y: oy.clamp(cy - half_y, cy + half_y),
        deck: spatial.deck_of(target),
        elevation: 0.0,
    };
    let origin = SurfacePoint {
        x: ox,
        y: oy,
        deck: None,
        elevation: 0.0,
    };
    spatial.terrain().melee_clear(origin, end)
}

/// Where a unit of `def` leaving building `b` appears (old `deployment`): the free cell on
/// the nearest ring around the footprint closest to `toward`, in the doorway's walk region.
pub fn deployment(
    ctx: &mut Ctx,
    b: EntityId,
    toward: Milli,
    def: Def,
    max_ring: i64,
) -> Option<Spot> {
    let building = ctx.state.entities.get(b)?;
    let (bdef, position, rotation) = (building.definition, building.position, building.rotation);
    let door = cell_of(entrance(ctx.catalog, bdef, position, rotation));
    let door = ctx.spatial.node(Spot::new(door.0, door.1, None));
    let body = ctx.spatial.profiles.get(def).body;
    let size = ctx.spatial.size();
    for ring in 1..=max_ring {
        let mut cells = perimeter(ctx.catalog, size, bdef, position, rotation, ring);
        cells.sort_by_key(|&(x, y)| {
            let d = super::distance2(
                (i64::from(x) * 1000, i64::from(y) * 1000),
                (i64::from(toward.x), i64::from(toward.y)),
            );
            (d, y, x)
        });
        for (x, y) in cells {
            let spot = Spot::new(x, y, None);
            if !ctx.spatial.free(spot, None, Some(&body), None) {
                continue;
            }
            let reachable = body.air
                || match (door, ctx.spatial.node(spot)) {
                    (Some(door), Some(node)) => ctx.spatial.connected(door, node),
                    _ => true,
                };
            if reachable {
                return Some(spot);
            }
        }
    }
    None
}

/// Plans the worker's walk to a place it can work `target` from and returns it, or `None`
/// (old `workPoint`). Buildings are worked from any side, preferring a directly reachable
/// edge cell; resources from the ring around their collision disc.
pub fn work_point(ctx: &mut Ctx, target: EntityId, worker: EntityId) -> Option<Spot> {
    let t = ctx.state.entities.get(target)?;
    let w = ctx.state.entities.get(worker)?;
    let origin = precise(w);
    let body = ctx.spatial.body_of(w);
    let radius = ctx
        .catalog
        .get(w.definition)
        .dimensions
        .as_ref()
        .map_or(0.0, |d| d.radius);
    if ctx.catalog.kind(t.definition) == canopy_content::game::Kind::Building {
        let size = ctx.spatial.size();
        let mut candidates = Vec::new();
        for ring in 1..=(radius.ceil() as i64 + 2) {
            let mut cells = perimeter(
                ctx.catalog,
                size,
                t.definition,
                t.position,
                t.rotation,
                ring,
            );
            cells.sort_by_key(|&(x, y)| {
                (
                    super::distance2((i64::from(x) * 1000, i64::from(y) * 1000), origin),
                    y,
                    x,
                )
            });
            candidates.extend(cells);
        }
        let from = Fixed::on(w.position.x, w.position.y, ctx.spatial.deck_of(w));
        for direct in [true, false] {
            for &(x, y) in &candidates {
                let spot = Spot::new(x, y, None);
                let t = ctx.state.entities.get(target).expect("target");
                let usable = ctx.spatial.free(spot, Some(worker), Some(&body), None)
                    && melee_clear(ctx.catalog, ctx.spatial, (x, y), t)
                    && (!direct
                        || ctx.spatial.unit_segment_clear(
                            from,
                            spot.fixed(),
                            Some(worker),
                            None,
                            None,
                            None,
                        ) && ctx.spatial.clear_segment(from, spot.fixed(), None, &body));
                if usable && route(ctx, worker, spot) {
                    return Some(spot);
                }
            }
        }
        return None;
    }
    let mut footprint = ctx.spatial.collision_cells(t);
    let cells = footprint.clone();
    footprint.sort_unstable();
    let size = ctx.spatial.size() as i64;
    let reach = radius.ceil() as i64 + 1;
    let mut candidates: Vec<u32> = Vec::new();
    for &cell in &cells {
        let (x, y) = (i64::from(cell) % size, i64::from(cell) / size);
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let (nx, ny) = (x + dx, y + dy);
                if dx * dx + dy * dy > reach * reach || nx < 0 || nx >= size || ny < 0 || ny >= size
                {
                    continue;
                }
                let n = (ny * size + nx) as u32;
                if footprint.binary_search(&n).is_err() && ctx.spatial.walkable(n) {
                    candidates.push(n);
                }
            }
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    candidates.sort_by_key(|&n| {
        let (x, y) = (i64::from(n) % size, i64::from(n) / size);
        (super::distance2((x * 1000, y * 1000), origin), n)
    });
    for n in candidates {
        let (x, y) = ((i64::from(n) % size) as i32, (i64::from(n) / size) as i32);
        let t = ctx.state.entities.get(target).expect("target");
        if melee_clear(ctx.catalog, ctx.spatial, (x, y), t) {
            let spot = Spot::new(x, y, None);
            if route(ctx, worker, spot) {
                return Some(spot);
            }
        }
    }
    None
}

/// Plans a route with the old default unit avoidance (old `spatial.route(e, p)`).
pub fn route(ctx: &mut Ctx, id: EntityId, spot: Spot) -> bool {
    match ctx.state.entities.get_mut(id) {
        Some(e) => ctx.spatial.route_default(e, spot),
        None => false,
    }
}
