//! Building placement (old `placement.ts`, `settlementLayer` ghost and `Game.canBuild`).
//!
//! The ghost follows the cursor snapped to the building lattice, turns in quarter turns
//! (R, Shift+R) and is coloured by whether the site looks valid **from the player's
//! knowledge**: the simulation's foundation rules (`economy::construction::can_build`)
//! applied to the view (explored cells, known buildings and resources, units in sight)
//! and the static terrain. Nothing here reads simulation state, so it can never desync
//! or reveal hidden buildings; the click sends an ordinary `build` action and the
//! simulation, which knows everything, may still refuse it (an error fact then says
//! why). The old game validated against the authoritative world on its worker, which
//! leaked hidden blockers through the ghost colour (D14, decision in docs/decisions.md).

use super::know::Knowledge;
use canopy_content::game::{Creation, Definition, Kind};
use canopy_sim::{
    action::{FoundationPoint, HalfCells},
    ai::{geography::Pt, placement::snap},
    entity::Milli,
    terrain::{
        CellBounds, Terrain,
        footprint::{
            Extent, body_overlaps_bounds, footprint_bounds, footprint_cells,
            placement_geometry_error,
        },
    },
};

/// An armed placement.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub definition: String,
    pub actors: Vec<u32>,
    /// Degrees, a multiple of 90.
    pub rotation: i32,
}

impl Placement {
    /// R: a quarter turn; Shift+R: three quarters (old rotation keys).
    pub fn rotate(&mut self, reverse: bool) {
        self.rotation = (self.rotation + if reverse { 270 } else { 90 }) % 360;
    }
}

/// Where the ghost stands and what the site looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct Ghost {
    /// Footprint centre in world units (x, z).
    pub center: [f32; 2],
    pub rotation: i32,
    pub point: FoundationPoint,
    /// Why the site looks invalid; `None` when it looks buildable.
    pub problem: Option<String>,
    /// The footprint's cells (inclusive bounds).
    pub cells: CellBounds,
    /// The entrance cell centre in world units.
    pub entrance: [f32; 2],
}

/// The ghost for the cursor's ground point `(x, z)`.
pub fn ghost(
    k: &Knowledge,
    terrain: &Terrain,
    placement: &Placement,
    x: f32,
    z: f32,
) -> Option<Ghost> {
    let def = k.catalog.find(&placement.definition)?;
    let d = k.catalog.get(def);
    let mut p = snap(d, Pt::new(f64::from(x), f64::from(z)), placement.rotation);
    // A building placed on a resource snaps onto the nearest known one under the cursor
    // (a Substance Well onto its deposit, D62).
    if let Some(on) = &d.placement_on {
        let reach = d
            .footprint
            .as_ref()
            .map_or(4.0, |f| f64::from(f.width.max(f.depth)));
        let nearest = k
            .view
            .entities
            .iter()
            .filter(|e| e.public.resource.is_some() && k.def(e).id == on.source)
            .map(|e| {
                let [ex, ey] = k.position(e);
                let d = (f64::from(ex) - f64::from(x)).hypot(f64::from(ey) - f64::from(z));
                (d, ex, ey)
            })
            .filter(|&(d, _, _)| d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, ex, ey)) = nearest {
            p = Pt::new(f64::from(ex), f64::from(ey));
        }
    }
    let half = |v: f64| HalfCells((v * 2.0).round().max(0.0) as u32);
    let point = FoundationPoint {
        x: half(p.x),
        y: half(p.y),
        surface: None,
    };
    let (cx, cy) = (f64::from(point.x.0) / 2.0, f64::from(point.y.0) / 2.0);
    let extent = Extent::of(d.footprint.as_ref());
    let rotation = f64::from(placement.rotation);
    let cells = footprint_cells(cx, cy, extent, rotation);
    let door = canopy_sim::economy::sites::entrance(
        &k.catalog,
        def,
        Milli {
            x: (cx * 1000.0) as i32,
            y: (cy * 1000.0) as i32,
        },
        placement.rotation * 1000,
    );
    let problem = check(
        k,
        terrain,
        d,
        &placement.actors,
        cx,
        cy,
        placement.rotation,
        door,
    );
    Some(Ghost {
        center: [cx as f32, cy as f32],
        rotation: placement.rotation,
        point,
        problem,
        cells,
        entrance: [door.x as f32 / 1000.0, door.y as f32 / 1000.0],
    })
}

/// Cells a known building or resource closes (footprint, and a resource's collision
/// disc).
fn closed(d: &Definition, x: f64, y: f64, rotation: f64, out: &mut Vec<(i64, i64)>) {
    let bounds = footprint_cells(x, y, Extent::of(d.footprint.as_ref()), rotation);
    if d.footprint.is_some() {
        out.extend(bounds.cells());
    }
    if let Some(r) = d.collision_radius.filter(|&r| r > 0.0) {
        for cy in (y - r).ceil() as i64..=(y + r).floor() as i64 {
            for cx in (x - r).ceil() as i64..=(x + r).floor() as i64 {
                let (dx, dy) = (cx as f64 - x, cy as f64 - y);
                if dx * dx + dy * dy <= r * r {
                    out.push((cx, cy));
                }
            }
        }
    }
}

/// The knowledge version of `can_build`, in the simulation's order.
#[allow(clippy::too_many_arguments)]
fn check(
    k: &Knowledge,
    terrain: &Terrain,
    d: &Definition,
    actors: &[u32],
    x: f64,
    y: f64,
    rotation: i32,
    door: Milli,
) -> Option<String> {
    if d.kind != Kind::Building || !matches!(d.creation, Some(Creation::Construct { .. })) {
        return Some(tr!("message.unknown-building"));
    }
    if let Some(e) = placement_geometry_error(d, x, y, f64::from(rotation)) {
        return Some(e.into());
    }
    let eligible = actors.iter().any(|&a| {
        k.entity(a).is_some_and(|e| {
            k.controlled(e)
                && k.def(e)
                    .behaviors
                    .work
                    .as_ref()
                    .is_some_and(|w| w.builds.contains(&d.id))
        })
    });
    if !eligible {
        return Some(tr!("message.select-eligible-worker"));
    }
    if k.view.outcome.is_some() {
        return Some(tr!("message.match-ended"));
    }
    let done = super::card::fulfilled_buildings(k);
    if let Some(r) = super::card::prerequisite(k, d.requires.as_deref(), &done) {
        return Some(r);
    }
    let own_buildings = k
        .view
        .entities
        .iter()
        .filter(|e| k.own(e) && k.is_building(e))
        .count();
    if own_buildings >= k.content.rules.max_buildings as usize {
        return Some(tr!("message.building-limit"));
    }
    let size = i64::from(terrain.size());
    let extent = Extent::of(d.footprint.as_ref());
    let bounds = footprint_cells(x, y, extent, f64::from(rotation));
    let index = |cx: i64, cy: i64| {
        (cx >= 0 && cy >= 0 && cx < size && cy < size).then(|| (cy * size + cx) as usize)
    };
    let cells: Vec<Option<usize>> = bounds.cells().map(|(cx, cy)| index(cx, cy)).collect();
    let explored = |c: usize| {
        k.view
            .fog
            .as_ref()
            .is_none_or(|f| f.cells.get(c).copied().unwrap_or(0) > 0)
    };
    if cells.iter().any(|c| c.is_none_or(|c| !explored(c))) {
        return Some(tr!("message.explore-first"));
    }
    let on_map: Vec<usize> = cells.into_iter().flatten().collect();
    if let Some(rule) = &d.placement_near {
        let r = f64::from(rule.radius);
        let near = k.view.entities.iter().any(|e| {
            !e.remembered
                && k.def(e).id == rule.source
                && e.public.resource.as_ref().is_some_and(|res| res.amount > 0)
                && {
                    let [ex, ey] = k.position(e);
                    (f64::from(ex) - x).hypot(f64::from(ey) - y) <= r
                }
        });
        if !near {
            return Some(tr!(
                "message.build-within",
                cells = rule.radius,
                name = &k.name_of(rule.source.as_str())
            ));
        }
    }
    // Power (D66): the viewer knows its own network.
    if d.power.is_some() {
        let milli = |v: f64| (v * 1000.0).round() as i64;
        let network = &k.view.network;
        if !network.fields.iter().any(|f| f.covers(milli(x), milli(y))) {
            return Some(crate::text::message(
                &canopy_sim::economy::network::outside(network.hint.as_deref()),
            ));
        }
    }
    let (dx, dy) = door.cell();
    let (dx, dy) = (i64::from(dx), i64::from(dy));
    if let Some(on) = &d.placement_on {
        // Exactly on a standing resource in sight that nobody is known to have built on.
        let at = [x as f32, y as f32];
        let resource = k.view.entities.iter().find(|e| {
            !e.remembered
                && k.def(e).id == on.source
                && e.public.resource.as_ref().is_some_and(|r| r.amount > 0)
                && k.position(e) == at
        });
        let name = k.name_of(on.source.as_str());
        let Some(resource) = resource else {
            return Some(tr!("message.build-on", name = &name));
        };
        if k.covered.contains(&resource.id()) {
            return Some(tr!("message.already-built-on", name = &name));
        }
        if !index(dx, dy).is_some_and(|c| terrain.walkable(c)) {
            return Some(tr!("message.entrance-blocked"));
        }
        return affordable(k, d);
    }
    // Known blockers: buildings (seen or remembered), resources and scenery.
    let mut blocked: Vec<(i64, i64)> = Vec::new();
    for e in &k.view.entities {
        let ed = k.def(e);
        if ed.kind == Kind::Building || e.public.resource.is_some() {
            let [ex, ey] = k.position(e);
            closed(
                ed,
                f64::from(ex),
                f64::from(ey),
                f64::from(e.public.rotation) / 1000.0,
                &mut blocked,
            );
        }
    }
    for s in k.view.scenery.iter() {
        if s.resource.as_ref().is_some_and(|r| r.amount > 0) {
            let sd = k.catalog.get(s.definition);
            closed(
                sd,
                f64::from(s.position.x) / 1000.0,
                f64::from(s.position.y) / 1000.0,
                f64::from(s.rotation) / 1000.0,
                &mut blocked,
            );
        }
    }
    blocked.sort_unstable();
    blocked.dedup();
    let is_blocked = |cx: i64, cy: i64| blocked.binary_search(&(cx, cy)).is_ok();
    let decks = terrain.surfaces();
    let blocked_cell = bounds.cells().any(|(cx, cy)| {
        let c = (cy * size + cx) as usize;
        !terrain.walkable(c) || decks.is_some_and(|s| s.has_deck(c as u32)) || is_blocked(cx, cy)
    });
    let edges = footprint_bounds(x, y, extent, f64::from(rotation));
    let blocked_body = k.view.entities.iter().any(|e| {
        let ed = k.def(e);
        !e.remembered
            && ed.kind == Kind::Unit
            && k.alive(e)
            && !k.held(e.id().0)
            && e.magic.as_ref().is_none_or(|m| m.flight.is_none())
            && ed.dimensions.as_ref().is_some_and(|dim| {
                let [ex, ey] = k.position(e);
                body_overlaps_bounds(f64::from(ex), f64::from(ey), dim.radius, edges)
            })
    });
    if blocked_cell || blocked_body {
        return Some(tr!("message.placement-blocked"));
    }
    let clearance_blocked = k.view.entities.iter().any(|e| {
        let rd = k.def(e);
        let Some(c) = rd.construction_clearance else {
            return false;
        };
        if !e.public.resource.as_ref().is_some_and(|r| r.amount > 0) || !k.alive(e) {
            return false;
        }
        let [ex, ey] = k.position(e);
        let rb = footprint_cells(
            f64::from(ex),
            f64::from(ey),
            Extent::of(rd.footprint.as_ref()),
            f64::from(e.public.rotation) / 1000.0,
        );
        let c = i64::from(c);
        bounds.cells().any(|(cx, cy)| {
            cx >= rb.min_x - c && cx <= rb.max_x + c && cy >= rb.min_y - c && cy <= rb.max_y + c
        })
    });
    if clearance_blocked {
        return Some(tr!("message.deposit-access"));
    }
    let wet = on_map
        .iter()
        .any(|&c| i32::from(terrain.height_cm(c)) <= i32::from(terrain.water_cm(c)) + 10);
    if wet {
        return Some(tr!("message.dry-ground"));
    }
    if !terrain.foundation_fits(bounds) {
        return Some(tr!("message.flatter-ground"));
    }
    let door_open = index(dx, dy).is_some_and(|c| terrain.walkable(c) && !is_blocked(dx, dy));
    if !door_open {
        return Some(tr!("message.entrance-blocked"));
    }
    affordable(k, d)
}

/// Whether the colony's unreserved goods pay for `d`, as far as the view shows.
fn affordable(k: &Knowledge, d: &Definition) -> Option<String> {
    let items = d.creation.as_ref().map_or(&[][..], |c| c.items());
    let affordable = items.iter().all(|c| {
        k.colony()
            .and_then(|col| col.goods.iter().find(|g| g.item == c.item))
            .is_some_and(|g| g.available >= c.amount)
    });
    if !affordable {
        return Some(tr!("message.insufficient-materials"));
    }
    None
}
