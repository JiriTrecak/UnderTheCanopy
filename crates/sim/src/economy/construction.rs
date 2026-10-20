//! Construction and repair (old `Game.canBuild`, the build/construct/cancel commands,
//! `constructionOrders`, the construct and repair branches of `advance`, and repair
//! assignment).
//!
//! A site is paid in full when placed. Builders raise its health with progress; when done
//! its inputs are consumed. Cancelling refunds the configured share.

use super::{
    Ctx, JobKind, active, alive, distance2,
    jobs::{self, Spec},
    sites, stock,
    supply::{self},
    work::{RETRY_TICKS, workers},
};
use crate::{
    action::FoundationPoint,
    catalog::Def,
    entity::{EntityId, Milli},
    orders::Order,
    terrain::CellBounds,
    terrain::footprint::{
        BUILDING_CELL, Extent, GRID_ORIGIN, body_overlaps_bounds, footprint_bounds,
        footprint_cells, placement_geometry_error,
    },
    vision::Sight,
};
use canopy_content::{
    game::{Creation, Kind},
    placement::Owner,
};

/// What the ground checks read: the match as it stands, without the right to change it.
#[derive(Clone, Copy)]
pub struct Ground<'a> {
    pub state: &'a crate::state::State,
    pub catalog: &'a crate::catalog::Catalog,
    pub spatial: &'a crate::spatial::Spatial,
    pub index: &'a super::Index,
}

impl Ctx<'_> {
    pub fn ground(&self) -> Ground<'_> {
        Ground {
            state: self.state,
            catalog: self.catalog,
            spatial: self.spatial,
            index: self.index,
        }
    }
}

/// A requested foundation: definition, centre in milli-cells and rotation in degrees.
#[derive(Debug, Clone, Copy)]
pub struct Foundation {
    pub def: Def,
    pub x: i32,
    pub y: i32,
    pub rotation: i32,
}

impl Foundation {
    pub fn of(def: Def, point: &FoundationPoint, rotation: i32) -> Self {
        Self {
            def,
            x: point.x.0 as i32 * 500,
            y: point.y.0 as i32 * 500,
            rotation,
        }
    }

    fn centre(&self) -> (f64, f64) {
        (f64::from(self.x) / 1000.0, f64::from(self.y) / 1000.0)
    }
}

/// Why `owner` may not place `definition` at the foundation with `builder` (old
/// `canBuild`), checked in the old order; `None` when it may.
pub fn can_build(
    ctx: &Ctx,
    sight: &Sight,
    owner: Owner,
    definition: &str,
    point: &FoundationPoint,
    builder: Option<EntityId>,
    rotation: i32,
) -> Option<String> {
    let def = ctx.catalog.find(definition).filter(|&d| {
        let d = ctx.catalog.get(d);
        d.kind == Kind::Building && matches!(d.creation, Some(Creation::Construct { .. }))
    });
    let Some(def) = def else {
        return Some("Unknown building".into());
    };
    let d = ctx.catalog.get(def);
    let f = Foundation::of(def, point, rotation);
    let (x, y) = f.centre();
    if let Some(error) = placement_geometry_error(d, x, y, f64::from(rotation)) {
        return Some(error.into());
    }
    if point.surface.is_some() {
        return Some("Buildings require ground foundations".into());
    }
    let eligible = builder
        .and_then(|b| ctx.state.entities.get(b))
        .is_some_and(|w| {
            let wd = ctx.catalog.get(w.definition);
            w.owner == owner
                && alive(w)
                && wd.behaviors.player_control.is_some()
                && wd
                    .behaviors
                    .work
                    .as_ref()
                    .is_some_and(|work| work.builds.iter().any(|b| b.as_str() == definition))
                && w.unit
                    .as_ref()
                    .is_some_and(|u| u.contained.is_none() && u.release.is_none())
        });
    if !eligible {
        return Some("Select an eligible worker".into());
    }
    if ctx.state.outcome.is_some() {
        return Some("Match has ended".into());
    }
    if let Some(reason) = supply::prerequisite(ctx, owner, d.requires.as_deref()) {
        return Some(reason);
    }
    let buildings = ctx
        .index
        .buildings
        .iter()
        .filter_map(|&id| ctx.state.entities.get(id))
        .filter(|e| e.owner == owner)
        .count();
    if buildings >= ctx.catalog.content.rules.max_buildings as usize {
        return Some("Building limit reached".into());
    }
    let size = i64::from(ctx.spatial.size());
    let extent = Extent::of(d.footprint.as_ref());
    let bounds = footprint_cells(x, y, extent, f64::from(rotation));
    let cells: Vec<Option<u32>> = bounds
        .cells()
        .map(|(cx, cy)| {
            (cx >= 0 && cy >= 0 && cx < size && cy < size).then(|| (cy * size + cx) as u32)
        })
        .collect();
    let on_map: Vec<u32> = cells.iter().flatten().copied().collect();
    if cells.iter().any(Option::is_none) || !sight.explored(owner, &on_map) {
        return Some("Explore this location first".into());
    }
    if let Some(rule) = &d.placement_near {
        let source = ctx.catalog.find(rule.source.as_str());
        let r = i64::from(rule.radius) * 1000;
        let near = ctx
            .index
            .resources
            .near(i64::from(f.x), i64::from(f.y), r)
            .into_iter()
            .filter_map(|id| ctx.state.entities.get(id))
            .any(|e| {
                Some(e.definition) == source
                    && e.resource.as_ref().is_some_and(|r| r.amount > 0)
                    && distance2(super::coarse(e), (i64::from(f.x), i64::from(f.y))) <= r * r
                    && sight.seen(owner, e)
            });
        if !near {
            let name = source.map_or(rule.source.as_str(), |s| ctx.catalog.get(s).name.as_str());
            return Some(format!("Build within {} cells of {name}", rule.radius));
        }
    }
    if d.power.is_some() && !ctx.network.covers(owner, i64::from(f.x), i64::from(f.y)) {
        return Some(super::network::outside(
            ctx.tables.network.get(&owner).map(String::as_str),
        ));
    }
    if let Some(rule) = &d.placement_on {
        // On a resource (a Substance Well on its deposit, D62): exactly on a seen,
        // standing one nobody has built on yet. Its footprint is the resource's own, so the
        // ground rules were the map's to keep; only the doorway must be open.
        let source = ctx.catalog.find(rule.source.as_str());
        let name = source.map_or(rule.source.as_str(), |s| ctx.catalog.get(s).name.as_str());
        let at = Milli { x: f.x, y: f.y };
        let resource = ctx
            .index
            .resources
            .near(i64::from(f.x), i64::from(f.y), 1000)
            .into_iter()
            .filter_map(|id| ctx.state.entities.get(id))
            .find(|e| {
                Some(e.definition) == source
                    && e.position == at
                    && alive(e)
                    && e.resource.as_ref().is_some_and(|r| r.amount > 0)
                    && sight.seen(owner, e)
            });
        let Some(resource) = resource else {
            return Some(format!("Build on a {name}"));
        };
        if super::harvest::building_on(ctx, resource).is_some() {
            return Some(format!("This {name} is already built on"));
        }
        if !door_open(&ctx.ground(), def, f, rotation) {
            return Some("Entrance blocked".into());
        }
        return affordable(ctx, owner, def);
    }
    if let Some(reason) = ground(&ctx.ground(), def, f, rotation, &bounds, &on_map) {
        return Some(reason.into());
    }
    affordable(ctx, owner, def)
}

/// Why the ground itself refuses `def` at `f`, whoever places it: cells that are not
/// walkable or carry a deck, ground units in the way, a blocked deposit lane, water,
/// a slope or a closed doorway.
fn ground(
    ctx: &Ground,
    def: Def,
    f: Foundation,
    rotation: i32,
    bounds: &CellBounds,
    on_map: &[u32],
) -> Option<&'static str> {
    let (x, y) = f.centre();
    let extent = Extent::of(ctx.catalog.get(def).footprint.as_ref());
    let edges = footprint_bounds(x, y, extent, f64::from(rotation));
    let decks = ctx.spatial.terrain().surfaces();
    let blocked_cell = on_map
        .iter()
        .any(|&c| !ctx.spatial.walkable(c) || decks.is_some_and(|s| s.has_deck(c)));
    let tick = ctx.state.tick;
    let blocked_body = ctx.index.units.iter().any(|&id| {
        ctx.state.entities.get(id).is_some_and(|e| {
            let body = ctx.spatial.profiles.get(e.definition).body;
            active(e, tick)
                && !body.air
                && body_overlaps_bounds(
                    f64::from(e.position.x) / 1000.0,
                    f64::from(e.position.y) / 1000.0,
                    body.radius,
                    edges,
                )
        })
    });
    if blocked_cell || blocked_body {
        return Some("Placement blocked");
    }
    if access_blocked(ctx, bounds) {
        return Some("Leave access around the resource deposit");
    }
    let terrain = ctx.spatial.terrain();
    let wet = on_map.iter().any(|&c| {
        i32::from(terrain.height_cm(c as usize)) <= i32::from(terrain.water_cm(c as usize)) + 10
    });
    if wet {
        return Some("Build on dry ground");
    }
    if !terrain.foundation_fits(*bounds) {
        return Some("Choose flatter ground");
    }
    if !door_open(ctx, def, f, rotation) {
        return Some("Entrance blocked");
    }
    None
}

/// The site nearest `at` where `owner` may found `def` without a builder, a price or
/// prerequisites (an ant founding a tunnel mound where it lands): the building lattice
/// point under `at`, else the first of its eight neighbours whose ground allows it.
pub fn found_site(ctx: &Ground, owner: Owner, def: Def, at: Milli) -> Option<Foundation> {
    let d = ctx.catalog.get(def);
    if d.kind != Kind::Building || ctx.state.outcome.is_some() {
        return None;
    }
    let buildings = ctx
        .index
        .buildings
        .iter()
        .filter_map(|&id| ctx.state.entities.get(id))
        .filter(|e| e.owner == owner)
        .count();
    if buildings >= ctx.catalog.content.rules.max_buildings as usize {
        return None;
    }
    let extent = Extent::of(d.footprint.as_ref());
    let snap = |centre: f64, size: f64| {
        GRID_ORIGIN
            + ((centre - size / 2.0 - GRID_ORIGIN) / BUILDING_CELL).round() * BUILDING_CELL
            + size / 2.0
    };
    let (cx, cy) = (
        snap(f64::from(at.x) / 1000.0, extent.width),
        snap(f64::from(at.y) / 1000.0, extent.depth),
    );
    let size = i64::from(ctx.spatial.size());
    const STEPS: [(f64, f64); 9] = [
        (0.0, 0.0),
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (1.0, 1.0),
        (-1.0, 1.0),
        (1.0, -1.0),
        (-1.0, -1.0),
    ];
    STEPS.iter().find_map(|&(dx, dy)| {
        let (x, y) = (cx + dx * BUILDING_CELL, cy + dy * BUILDING_CELL);
        let point = FoundationPoint {
            x: crate::action::HalfCells((x * 2.0).round().max(0.0) as u32),
            y: crate::action::HalfCells((y * 2.0).round().max(0.0) as u32),
            surface: None,
        };
        let f = Foundation::of(def, &point, 0);
        let (x, y) = f.centre();
        if placement_geometry_error(d, x, y, 0.0).is_some() {
            return None;
        }
        let bounds = footprint_cells(x, y, extent, 0.0);
        let cells: Vec<Option<u32>> = bounds
            .cells()
            .map(|(cx, cy)| {
                (cx >= 0 && cy >= 0 && cx < size && cy < size).then(|| (cy * size + cx) as u32)
            })
            .collect();
        if cells.iter().any(Option::is_none) {
            return None;
        }
        let on_map: Vec<u32> = cells.into_iter().flatten().collect();
        ground(ctx, def, f, 0, &bounds, &on_map)
            .is_none()
            .then_some(f)
    })
}

/// Raises a finished `def` for `owner` at `site` (see [`found_site`]).
pub fn found(ctx: &mut Ctx, owner: Owner, site: Foundation) -> EntityId {
    let tick = ctx.state.tick;
    let id = crate::spawn::insert(
        ctx.state,
        ctx.catalog,
        crate::spawn::Spawn {
            definition: site.def,
            owner,
            position: Milli {
                x: site.x,
                y: site.y,
            },
            surface: None,
            rotation: 0,
            placement: None,
            initial: Default::default(),
            complete: true,
            ready_tick: tick + 1,
        },
    );
    ctx.index.sync(ctx.state, ctx.catalog);
    ctx.spatial.sync_entity(ctx.state, id);
    id
}

/// Whether the foundation's doorway cell is walkable.
fn door_open(ctx: &Ground, def: Def, f: Foundation, rotation: i32) -> bool {
    let size = i64::from(ctx.spatial.size());
    let door = sites::entrance(ctx.catalog, def, Milli { x: f.x, y: f.y }, rotation * 1000);
    let (dx, dy) = door.cell();
    dx >= 0
        && dy >= 0
        && i64::from(dx) < size
        && i64::from(dy) < size
        && ctx
            .spatial
            .walkable((i64::from(dy) * size + i64::from(dx)) as u32)
}

/// Why the owner cannot pay for `def` now, if it cannot.
fn affordable(ctx: &Ctx, owner: Owner, def: Def) -> Option<String> {
    if stock::reserve(
        ctx.state,
        ctx.catalog,
        ctx.index,
        owner,
        stock::price(ctx.catalog, def),
    )
    .is_none()
    {
        return Some("Insufficient unreserved materials".into());
    }
    None
}

/// Whether a standing resource with a construction clearance keeps this footprint out
/// (old `resourceBlocksCell` over every resource; only resources near enough can).
fn access_blocked(ctx: &Ground, bounds: &crate::terrain::CellBounds) -> bool {
    let reach = ctx
        .catalog
        .content
        .definitions
        .iter()
        .filter_map(|d| {
            let clearance = i64::from(d.construction_clearance?);
            let f = Extent::of(d.footprint.as_ref());
            Some(clearance + f.width.max(f.depth).ceil() as i64)
        })
        .max();
    let Some(reach) = reach else {
        return false;
    };
    let (cx, cy) = (
        (bounds.min_x + bounds.max_x) * 500,
        (bounds.min_y + bounds.max_y) * 500,
    );
    let half = (bounds.max_x - bounds.min_x).max(bounds.max_y - bounds.min_y) * 500 + 1000;
    ctx.index
        .resources
        .near(cx, cy, half + reach * 1000)
        .into_iter()
        .filter_map(|id| ctx.state.entities.get(id))
        .any(|r| {
            let d = ctx.catalog.get(r.definition);
            let Some(clearance) = d.construction_clearance else {
                return false;
            };
            if !alive(r) || !r.resource.as_ref().is_some_and(|res| res.amount > 0) {
                return false;
            }
            let rb = footprint_cells(
                f64::from(r.position.x) / 1000.0,
                f64::from(r.position.y) / 1000.0,
                Extent::of(d.footprint.as_ref()),
                f64::from(r.rotation) / 1000.0,
            );
            let c = i64::from(clearance);
            bounds.cells().any(|(x, y)| {
                x >= rb.min_x - c && x <= rb.max_x + c && y >= rb.min_y - c && y <= rb.max_y + c
            })
        })
}

/// What a `build` command asks for.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub definition: &'a str,
    pub point: &'a FoundationPoint,
    /// Degrees, a multiple of 90.
    pub rotation: i32,
    pub append: bool,
}

/// The checks of the `build` command after the common actor checks (old `Game.command`
/// build branch): picks the builder among `eligible` (in the order given), preferring one
/// not already set on construction.
pub fn check_build(
    ctx: &Ctx,
    sight: &Sight,
    owner: Owner,
    eligible: &[EntityId],
    request: &Request,
) -> Result<EntityId, String> {
    let Request {
        definition,
        point,
        rotation,
        append,
    } = *request;
    let builders: Vec<EntityId> = eligible
        .iter()
        .copied()
        .filter(|&id| {
            ctx.state.entities.get(id).is_some_and(|e| {
                ctx.catalog
                    .get(e.definition)
                    .behaviors
                    .work
                    .as_ref()
                    .is_some_and(|w| w.builds.iter().any(|b| b.as_str() == definition))
            })
        })
        .collect();
    let builder = builders
        .iter()
        .copied()
        .find(|&b| !constructing(ctx, b))
        .or(builders.first().copied())
        .ok_or("Select an eligible worker")?;
    if !crate::orders::can_issue(ctx.state.entities.get(builder).expect("builder"), append) {
        return Err("Order queue is full".into());
    }
    if let Some(reason) = can_build(
        ctx,
        sight,
        owner,
        definition,
        point,
        Some(builder),
        rotation,
    ) {
        return Err(reason);
    }
    Ok(builder)
}

/// Places a checked construction site, moves its bill in and sends the builder (old build
/// branch after `canBuild`).
pub fn place_site(ctx: &mut Ctx, owner: Owner, builder: EntityId, request: &Request) -> EntityId {
    let Request {
        definition,
        point,
        rotation,
        append,
    } = *request;
    let def = ctx.catalog.find(definition).expect("building");
    let reservation = stock::reserve(
        ctx.state,
        ctx.catalog,
        ctx.index,
        owner,
        stock::price(ctx.catalog, def),
    )
    .expect("checked");
    let f = Foundation::of(def, point, rotation);
    let tick = ctx.state.tick;
    let site = crate::spawn::insert(
        ctx.state,
        ctx.catalog,
        crate::spawn::Spawn {
            definition: def,
            owner,
            position: Milli { x: f.x, y: f.y },
            surface: None,
            rotation: rotation * 1000,
            placement: None,
            initial: Default::default(),
            complete: false,
            ready_tick: tick + 1,
        },
    );
    ctx.index.sync(ctx.state, ctx.catalog);
    stock::admit(ctx.state, site, &reservation);
    crate::orders::issue(
        ctx.state,
        builder,
        Order::Construct { target: site },
        append,
    );
    ctx.spatial.sync_entity(ctx.state, site);
    site
}

/// Whether the worker is set on construction (old `isConstructing`).
fn constructing(ctx: &Ctx, worker: EntityId) -> bool {
    let Some(u) = ctx.state.entities.get(worker).and_then(|e| e.unit.as_ref()) else {
        return false;
    };
    matches!(u.order, Some(Order::Construct { .. }))
        || u.order_queue
            .iter()
            .any(|o| matches!(o, Order::Construct { .. }))
        || u.work
            .job
            .and_then(|j| ctx.state.economy.job(j))
            .is_some_and(|j| j.kind == JobKind::Construct)
}

/// Whether a builder's goods must be delivered before it builds (old
/// `cargoBlocksConstruction`): only while some completed drop-off could take them.
pub fn cargo_blocks_construction(ctx: &Ctx, worker: EntityId) -> bool {
    let Some(e) = ctx.state.entities.get(worker) else {
        return false;
    };
    let Some(cargo) = e.unit.as_ref().and_then(|u| u.work.cargo.as_ref()) else {
        return false;
    };
    !super::work::stores(ctx, e.owner, cargo.item.as_str(), 0, super::coarse(e)).is_empty()
}

/// Whether the worker may build this site.
fn builds(ctx: &Ctx, worker: EntityId, site: EntityId) -> bool {
    let (Some(w), Some(b)) = (ctx.state.entities.get(worker), ctx.state.entities.get(site)) else {
        return false;
    };
    b.construction.is_some()
        && alive(b)
        && b.owner == w.owner
        && ctx
            .catalog
            .get(w.definition)
            .behaviors
            .work
            .as_ref()
            .is_some_and(|work| work.builds.contains(ctx.catalog.id(b.definition)))
}

/// Step 4: workers ordered to construct take up free sites whose bill is in place (old
/// `constructionOrders`). The order keeps the builder even while a last harvest goes home.
pub fn construction_orders(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    for id in ctx.index.units.clone() {
        let Some(e) = ctx.state.entities.get(id) else {
            continue;
        };
        if !active(e, tick) {
            continue;
        }
        let u = e.unit.as_ref().expect("unit");
        if u.taunt.is_some() {
            continue;
        }
        let Some(Order::Construct { target }) = u.order else {
            continue;
        };
        if !builds(ctx, id, target) {
            ctx.state
                .entities
                .get_mut(id)
                .and_then(|e| e.unit.as_mut())
                .expect("unit")
                .order = None;
            continue;
        }
        if ctx.state.economy.jobs.occupied(JobKind::Construct, target) {
            continue;
        }
        let u = ctx
            .state
            .entities
            .get(id)
            .and_then(|e| e.unit.as_ref())
            .expect("unit");
        if u.work.job.is_some()
            || cargo_blocks_construction(ctx, id)
            || u.retry_at > tick
            || crate::abilities::statuses::stunned(
                ctx.catalog,
                ctx.state.entities.get(id).expect("unit"),
            )
        {
            continue;
        }
        let site = ctx.state.entities.get(target).expect("site");
        if stock::missing(&site.inventory, stock::price(ctx.catalog, site.definition)) {
            continue;
        }
        if sites::work_point(ctx, target, id).is_some() {
            jobs::create(ctx, JobKind::Construct, id, target, Spec::default());
        } else if let Some(u) = ctx.state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
            u.retry_at = tick + RETRY_TICKS;
        }
    }
}

/// One tick of building (old construct branch of `advance`): health grows with progress;
/// when complete the inputs are consumed and the building opens next tick.
pub fn advance_construct(ctx: &mut Ctx, id: u32) {
    let tick = ctx.state.tick;
    let job = ctx.state.economy.job(id).expect("job").clone();
    let (worker, site) = (job.worker, job.target);
    if !builds(ctx, worker, site) {
        jobs::erase(ctx, id);
        return;
    }
    let b = ctx.state.entities.get(site).expect("site");
    let def = ctx.catalog.get(b.definition);
    if stock::missing(&b.inventory, stock::price(ctx.catalog, b.definition)) {
        jobs::erase(ctx, id);
        return;
    }
    let work_ticks = match &def.creation {
        Some(Creation::Construct { work_ticks, .. }) => *work_ticks,
        _ => 1,
    };
    let max_hp = def.body.as_ref().map_or(1, |b| b.max_hp);
    let permille = ctx.catalog.content.rules.construction_hp_permille;
    let start = (u64::from(max_hp) * u64::from(permille) / 1000).max(1) as u32;
    let b = ctx.state.entities.get_mut(site).expect("site");
    let c = b.construction.as_mut().expect("construction");
    c.progress += 1;
    let supported = max_hp.min(
        start
            + (u64::from(max_hp - start.min(max_hp)) * u64::from(c.progress)
                / u64::from(work_ticks.max(1))) as u32,
    );
    let gained = supported.saturating_sub(c.supported_hp);
    c.supported_hp = supported;
    let progress = c.progress;
    if let Some(hp) = b.hp.as_mut() {
        *hp += gained;
    }
    if progress < work_ticks {
        return;
    }
    let owner = b.owner;
    let definition = b.definition;
    for cost in stock::price(ctx.catalog, definition) {
        if let Some(b) = ctx.state.entities.get_mut(site) {
            stock::take(&mut b.inventory, cost.item.as_str(), cost.amount);
        }
        stock::record(
            ctx.state,
            owner,
            crate::state::FactKind::Consumed,
            "Production input consumed",
            Some((&cost.item, cost.amount)),
        );
    }
    let b = ctx.state.entities.get_mut(site).expect("site");
    b.construction = None;
    b.ready_tick = tick + 1;
    if let Some(u) = ctx
        .state
        .entities
        .get_mut(worker)
        .and_then(|e| e.unit.as_mut())
        && u.order == Some(Order::Construct { target: site })
    {
        u.order = None;
    }
    jobs::erase(ctx, id);
    ctx.spatial.sync_entity(ctx.state, site);
}

/// One tick of repair (old repair branch of `advance`): a point of health every
/// `repairTicks` until whole.
pub fn advance_repair(ctx: &mut Ctx, id: u32) {
    let job = ctx.state.economy.job(id).expect("job").clone();
    let repair_ticks = ctx.catalog.content.rules.repair_ticks;
    let b = ctx.state.entities.get(job.target).expect("building");
    let max_hp = ctx
        .catalog
        .get(b.definition)
        .body
        .as_ref()
        .map_or(0, |b| b.max_hp);
    let mut progress = job.progress + 1;
    let mut hp = b.hp.unwrap_or(0);
    if progress >= repair_ticks {
        progress = 0;
        hp = max_hp.min(hp + 1);
    }
    ctx.state.economy.job_mut(id).expect("job").progress = progress;
    if let Some(b) = ctx.state.entities.get_mut(job.target) {
        b.hp = Some(hp);
    }
    if hp == max_hp {
        jobs::erase(ctx, id);
    }
}

/// Ticks before idle workers look again for a way to a damaged building none of them
/// could reach. The old game retried every tick, each worker running a full path search.
pub const REPAIR_RETRY_TICKS: u32 = 40;

/// Step 4: each damaged completed building without a repairer gets the first idle worker
/// that can reach it (old repair loop of `assign`). A building nobody could reach waits
/// [`REPAIR_RETRY_TICKS`] before the next attempt.
pub fn assign_repairs(ctx: &mut Ctx, buildings: &[EntityId]) {
    let tick = ctx.state.tick;
    let retry = &mut ctx.state.economy.repair_retry;
    retry.retain(|b, until| *until > tick && buildings.contains(b));
    for &b in buildings {
        let Some(e) = ctx.state.entities.get(b) else {
            continue;
        };
        let max_hp = ctx
            .catalog
            .get(e.definition)
            .body
            .as_ref()
            .map(|b| b.max_hp);
        let damaged = e.construction.is_none()
            && match (e.hp, max_hp) {
                (Some(hp), Some(max)) => hp < max,
                _ => false,
            };
        if !damaged
            || ctx.state.economy.jobs.occupied(JobKind::Repair, b)
            || ctx.state.economy.repair_retry.contains_key(&b)
        {
            continue;
        }
        let owner = e.owner;
        let mut tried = false;
        let mut assigned = false;
        for w in workers(ctx, owner, true) {
            tried = true;
            if sites::work_point(ctx, b, w).is_some() {
                jobs::create(ctx, JobKind::Repair, w, b, Spec::default());
                assigned = true;
                break;
            }
        }
        if tried && !assigned {
            ctx.state
                .economy
                .repair_retry
                .insert(b, tick + REPAIR_RETRY_TICKS);
        }
    }
}
