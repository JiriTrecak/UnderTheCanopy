//! The spatial model (old `Spatial`, `src/sim/game/spatial.ts`): static occupancy, the
//! moving-body broad phase, terrain and body sweeps, free-position queries and routing.
//!
//! Everything here is derived from state and never saved. Static occupancy is
//! synchronised incrementally ([`Spatial::sync_static`]); unit bodies are refreshed at each
//! planning or movement boundary ([`Spatial::sync_bodies`]) and after each mover
//! ([`Spatial::update_body`]). A cold rebuild after restore answers identically.

pub mod adjacent;
pub mod bodies;
pub mod clearance;
pub mod geometry;
pub mod occupancy;
pub mod profile;
mod routing;

pub use bodies::{BodyRecord, Bodies};
pub use geometry::Fixed;
pub use profile::{Body, Motion, Profile, Profiles};
pub use routing::Stats;

use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId},
    navigation::{
        cells::{CellSet, Reserved},
        corridors::RouteCorridors,
        grid::{GridNavigation, Scratch},
        mesh::GroundMesh,
        sectors::{Sectors, Topology},
        surfaces::{SurfaceScratch, clear_layered_sweep},
    },
    state::State,
    terrain::{
        DeckId, MAX_GROUND_STEP_CM, SurfaceNode, SurfacePoint, Terrain, WalkSurfaces,
        footprint::footprint_cells,
        round_half_up,
    },
};
use adjacent::AdjacentSweep;
use canopy_content::game::Kind;
use clearance::Clearance;
use geometry::{clear_ray, clear_sweep, sweep_pattern};
use occupancy::{Cover, Occupancy};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::Arc,
};

/// A cell on the ground or on a deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Spot {
    pub x: i32,
    pub y: i32,
    pub deck: Option<DeckId>,
}

impl Spot {
    pub const fn new(x: i32, y: i32, deck: Option<DeckId>) -> Self {
        Self { x, y, deck }
    }

    pub fn fixed(self) -> Fixed {
        Fixed::cell(self.x, self.y, self.deck)
    }
}

/// Why a movement step cannot be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Obstruction {
    /// Terrain or a static blocker: the route is broken.
    Terrain,
    /// A cell claimed by moving traffic.
    Reservation,
}

/// Whether a unit ghosts through other bodies. Harvest and delivery traffic and garrisoned
/// archers do in the old game (`GameContext` `ignoresUnits`); the jobs and garrisons that
/// decide it arrive with the economy and combat steps.
pub fn ignores_units(_state: &State, _e: &Entity) -> bool {
    false
}

/// Whether a unit is alive and physically present on the map.
pub fn present(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
        && e.unit
            .as_ref()
            .is_some_and(|u| u.contained.is_none() && u.release.is_none())
}

struct Ground<'a> {
    walk: &'a [bool],
    heights: &'a [i16],
}

impl Ground<'_> {
    #[inline]
    fn step(&self, a: u32, b: u32) -> bool {
        self.walk[b as usize]
            && (i32::from(self.heights[a as usize]) - i32::from(self.heights[b as usize])).abs()
                <= MAX_GROUND_STEP_CM
    }
}

struct Topo<'a> {
    walk: &'a [bool],
    heights: &'a [i16],
    surfaces: Option<&'a WalkSurfaces>,
    occupancy: &'a Occupancy,
}

impl Topology for Topo<'_> {
    fn count(&self) -> u32 {
        self.walk.len() as u32
    }
    fn cell(&self, node: u32) -> u32 {
        match self.surfaces {
            Some(s) => s.node(node).map_or(node, |n| n.cell),
            None => node,
        }
    }
    fn walkable(&self, node: u32) -> bool {
        self.walk[node as usize]
    }
    fn neighbors(&self, node: u32, out: &mut Vec<u32>) {
        if let Some(s) = self.surfaces {
            s.neighbors(node, |n: &SurfaceNode| self.occupancy.blocked(n.id), out);
        } else {
            out.clear();
        }
    }
    fn step(&self, from: u32, to: u32) -> bool {
        Ground {
            walk: self.walk,
            heights: self.heights,
        }
        .step(from, to)
    }
    fn layered(&self) -> bool {
        self.surfaces.is_some()
    }
}

pub struct Spatial {
    terrain: Arc<Terrain>,
    size: u32,
    pub profiles: Profiles,
    /// Per node: walkable terrain (or deck) not covered by a building or resource.
    walk: Vec<bool>,
    occupancy: Occupancy,
    /// Static blocker signatures by entity, to detect changes.
    statics: BTreeMap<EntityId, (crate::catalog::Def, i32, i32, i32, u64, Option<String>)>,
    clearance: Option<Clearance>,
    patterns: BTreeMap<i32, Vec<(i32, i32)>>,
    adjacent: BTreeMap<i32, AdjacentSweep>,
    grids: BTreeMap<i32, GridNavigation>,
    air: GridNavigation,
    meshes: BTreeMap<i32, GroundMesh>,
    sectors: Sectors,
    scratch: Scratch,
    surface_scratch: SurfaceScratch,
    pub bodies: Bodies,
    corridors: Option<RouteCorridors>,
    /// Appearance scale of resource placements, by placement id.
    scales: BTreeMap<String, f64>,
    blockers: CellSet,
    pub stats: Stats,
}

impl Spatial {
    /// Builds the spatial model for `state` on `terrain`. `scales` gives the appearance
    /// scale of placements that declare one (resource collision discs scale with it).
    pub fn new(terrain: Arc<Terrain>, catalog: &Catalog, state: &State, scales: BTreeMap<String, f64>) -> Self {
        let size = terrain.size();
        let profiles = Profiles::new(catalog);
        let layered = terrain.surfaces().is_some();
        let nodes = terrain.surfaces().map_or((size * size) as usize, |s| s.node_count() as usize);
        let walk: Vec<bool> = (0..nodes as u32)
            .map(|n| match terrain.surfaces() {
                Some(s) => s.walkable(n),
                None => terrain.walkable(n as usize),
            })
            .collect();
        let mut radii: Vec<i32> = definitions_radii(&profiles, catalog);
        radii.sort_unstable();
        radii.dedup();
        let patterns = radii.iter().map(|&r| (r, sweep_pattern(r))).collect();
        let adjacent = radii
            .iter()
            .filter(|&&r| r >= 500)
            .map(|&r| (r, AdjacentSweep::new(r)))
            .collect();
        let occupancy = Occupancy::new(nodes);
        let sectors = {
            let topo = Topo {
                walk: &walk,
                heights: terrain.heights(),
                surfaces: terrain.surfaces(),
                occupancy: &occupancy,
            };
            Sectors::new(size, &topo)
        };
        let mut this = Self {
            clearance: (!layered).then(|| Clearance::new(size, |c| walk[c as usize])),
            terrain,
            size,
            profiles,
            walk,
            occupancy,
            statics: BTreeMap::new(),
            patterns,
            adjacent,
            grids: BTreeMap::new(),
            air: GridNavigation::new(size),
            meshes: BTreeMap::new(),
            sectors,
            scratch: Scratch::default(),
            surface_scratch: SurfaceScratch::default(),
            bodies: Bodies::new(size),
            corridors: None,
            scales,
            blockers: CellSet::new(nodes),
            stats: Stats::default(),
        };
        this.sync_static(state);
        this.sync_bodies(state);
        this.prepare_meshes(state);
        this
    }

    /// Builds the navigation meshes of the smallest ground body and of every ground unit
    /// present, so the first long order does not stall (the old `rebuild` did the same).
    fn prepare_meshes(&mut self, state: &State) {
        if self.layered() {
            return;
        }
        let mut bodies = vec![self.profiles.navigation_body];
        for e in state.entities.iter() {
            if e.unit.is_some() && e.hp.is_none_or(|hp| hp > 0) {
                bodies.push(self.body_of(e));
            }
        }
        bodies.retain(|b| !b.air && b.radius <= 16.0);
        bodies.sort_by(|a, b| a.radius.total_cmp(&b.radius));
        bodies.dedup_by_key(|b| b.radius_milli());
        for body in bodies {
            self.mesh(&body);
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn terrain(&self) -> &Arc<Terrain> {
        &self.terrain
    }

    pub fn layered(&self) -> bool {
        self.terrain.surfaces().is_some()
    }

    /// Nodes: ground cells, plus deck nodes on layered maps.
    pub fn node_count(&self) -> u32 {
        self.walk.len() as u32
    }

    pub fn deck_id(&self, name: &str) -> Option<DeckId> {
        self.terrain.deck_id(name)
    }

    pub fn deck_name(&self, deck: DeckId) -> String {
        self.terrain
            .surfaces()
            .map(|s| s.deck(deck).id.clone())
            .unwrap_or_default()
    }

    /// The deck an entity stands on.
    pub fn deck_of(&self, e: &Entity) -> Option<DeckId> {
        e.surface.as_deref().and_then(|name| self.deck_id(name))
    }

    /// The node of a cell (old `Spatial.cell`). Off-map ground cells have none.
    pub fn node(&self, spot: Spot) -> Option<u32> {
        match self.terrain.surfaces() {
            Some(s) => s.node_at(i64::from(spot.x), i64::from(spot.y), spot.deck),
            None => (spot.x >= 0 && spot.y >= 0 && spot.x < self.size as i32 && spot.y < self.size as i32)
                .then(|| spot.y as u32 * self.size + spot.x as u32),
        }
    }

    /// The node under a fixed-point position.
    pub fn node_at(&self, p: Fixed) -> Option<u32> {
        let (x, y) = p.grid();
        self.node(Spot::new(x, y, p.deck))
    }

    /// The node an entity stands on.
    pub fn entity_node(&self, e: &Entity) -> Option<u32> {
        let (x, y) = e.position.cell();
        self.node(Spot::new(x, y, self.deck_of(e)))
    }

    /// The cell of a node.
    pub fn spot(&self, node: u32) -> Spot {
        match self.terrain.surfaces().and_then(|s| s.node(node)) {
            Some(n) => Spot::new(n.x as i32, n.y as i32, n.deck),
            None => Spot::new((node % self.size) as i32, (node / self.size) as i32, None),
        }
    }

    /// The centre of a node.
    pub fn point(&self, node: u32) -> Fixed {
        self.spot(node).fixed()
    }

    /// A position on the map (and on its deck, if any).
    pub fn valid_spot(&self, spot: Spot) -> bool {
        spot.x >= 0
            && spot.y >= 0
            && spot.x < self.size as i32
            && spot.y < self.size as i32
            && (spot.deck.is_none() || self.node(spot).is_some())
    }

    pub fn valid_node(&self, node: u32) -> bool {
        node < self.node_count()
    }

    /// Walkable terrain without a building or standing resource.
    pub fn walkable(&self, node: u32) -> bool {
        self.walk.get(node as usize).copied().unwrap_or(false)
    }

    /// Whether a building covers the node.
    pub fn building_at(&self, node: u32) -> bool {
        self.occupancy.building(node)
    }

    /// Floor height in world units at a point in cells (old `Spatial.height`).
    pub fn height(&self, x: f64, y: f64, deck: Option<DeckId>) -> f64 {
        if let Some(s) = self.terrain.surfaces()
            && let Some(h) = s.height(SurfacePoint {
                x,
                y,
                deck,
                elevation: 0.0,
            })
        {
            return h;
        }
        let last = self.size as i64 - 1;
        let cx = (round_half_up(x) as i64).clamp(0, last);
        let cy = (round_half_up(y) as i64).clamp(0, last);
        f64::from(self.terrain.height_cm((cy * self.size as i64 + cx) as usize)) / 100.0
    }

    fn pattern(&self, radius: i32) -> Cow<'_, [(i32, i32)]> {
        match self.patterns.get(&radius) {
            Some(p) => Cow::Borrowed(p),
            None => Cow::Owned(sweep_pattern(radius)),
        }
    }

    // ----- static occupancy -------------------------------------------------------------

    /// The cells a building or standing resource covers (old `footprint`/`collision`).
    fn cover(&self, e: &Entity) -> Cover {
        let profile = self.profiles.get(e.definition);
        let alive = e.hp.is_none_or(|hp| hp > 0);
        let building = alive && profile.kind == Kind::Building;
        let resource = alive && e.resource.as_ref().is_some_and(|r| r.amount > 0);
        let mut cover = Cover::default();
        if !building && !resource {
            return cover;
        }
        let (x, y) = (f64::from(e.position.x) / 1000.0, f64::from(e.position.y) / 1000.0);
        let rotation = f64::from(e.rotation) / 1000.0;
        let size = i64::from(self.size);
        let bounds = footprint_cells(x, y, profile.footprint, rotation);
        let in_map = |cx: i64, cy: i64| (cx >= 0 && cy >= 0 && cx < size && cy < size).then(|| (cy * size + cx) as u32);
        if building {
            cover.building = bounds.cells().filter_map(|(cx, cy)| in_map(cx, cy)).collect();
        }
        if resource {
            cover.resource = match profile.collision_radius {
                None => bounds.cells().filter_map(|(cx, cy)| in_map(cx, cy)).collect(),
                Some(radius) => {
                    let scale = e
                        .placement
                        .as_ref()
                        .and_then(|p| self.scales.get(p))
                        .copied()
                        .unwrap_or(1.0);
                    let r = radius * scale;
                    let mut cells = Vec::new();
                    let (y0, y1) = (bounds.min_y.min((y - r).ceil() as i64), bounds.max_y.max((y + r).floor() as i64));
                    let (x0, x1) = (bounds.min_x.min((x - r).ceil() as i64), bounds.max_x.max((x + r).floor() as i64));
                    for cy in y0..=y1 {
                        for cx in x0..=x1 {
                            let (fx, fy) = (cx as f64 - x, cy as f64 - y);
                            if (bounds.contains(cx, cy) || fx * fx + fy * fy <= r * r)
                                && let Some(cell) = in_map(cx, cy)
                            {
                                cells.push(cell);
                            }
                        }
                    }
                    cells
                }
            };
        }
        cover
    }

    /// Brings static occupancy up to date with the buildings and resources in `state`,
    /// touching only entities whose blocking footprint changed.
    pub fn sync_static(&mut self, state: &State) {
        let mut changed = Vec::new();
        let mut present: Vec<EntityId> = Vec::new();
        for e in state.entities.iter() {
            let kind = self.profiles.get(e.definition).kind;
            if kind != Kind::Building && e.resource.is_none() {
                continue;
            }
            present.push(e.id);
            let alive = e.hp.is_none_or(|hp| hp > 0);
            let standing = e.resource.as_ref().is_some_and(|r| r.amount > 0);
            let signature = (
                e.definition,
                e.position.x,
                e.position.y,
                e.rotation,
                u64::from(alive) | u64::from(standing) << 1,
                e.surface.clone(),
            );
            if self.statics.get(&e.id) == Some(&signature) {
                continue;
            }
            let cover = self.cover(e);
            self.occupancy.set(e.id, cover, &mut changed);
            self.statics.insert(e.id, signature);
        }
        let gone: Vec<EntityId> = self
            .statics
            .keys()
            .filter(|id| present.binary_search(id).is_err())
            .copied()
            .collect();
        for id in gone {
            self.statics.remove(&id);
            self.occupancy.set(id, Cover::default(), &mut changed);
        }
        if !changed.is_empty() {
            changed.sort_unstable();
            changed.dedup();
            self.invalidate(&changed);
        }
    }

    fn invalidate(&mut self, changed: &[u32]) {
        let surfaces = self.terrain.surfaces();
        for &node in changed {
            let base = match surfaces {
                Some(s) => s.walkable(node),
                None => self.terrain.walkable(node as usize),
            };
            self.walk[node as usize] = base && !self.occupancy.blocked(node);
            if let Some(clearance) = &mut self.clearance {
                clearance.refresh(node, self.walk[node as usize]);
            }
        }
        let padding = 1 + (self.profiles.max_radius + 500) / 1000;
        for grid in self.grids.values_mut() {
            grid.invalidate(Some(changed), padding);
        }
        let ground: Vec<u32> = changed
            .iter()
            .copied()
            .filter(|&n| n < self.size * self.size)
            .collect();
        for mesh in self.meshes.values_mut() {
            mesh.invalidate(&ground);
        }
        let topo = Topo {
            walk: &self.walk,
            heights: self.terrain.heights(),
            surfaces: self.terrain.surfaces(),
            occupancy: &self.occupancy,
        };
        self.sectors.invalidate(&topo, changed);
        self.sectors.prepare(&topo);
    }

    // ----- bodies ------------------------------------------------------------------------

    fn record(&self, e: &Entity, ignores: bool) -> Option<BodyRecord> {
        let u = e.unit.as_ref()?;
        let profile = self.profiles.get(e.definition);
        let deck = self.deck_of(e);
        let (cx, cy) = e.position.cell();
        Some(BodyRecord {
            id: e.id,
            owner: e.owner,
            x: e.position.x,
            y: e.position.y,
            deck,
            node: self.node(Spot::new(cx, cy, deck)).unwrap_or(u32::MAX),
            radius: profile.body.radius_milli(),
            height: profile.body.height,
            floor: if self.layered() {
                self.height(f64::from(e.position.x) / 1000.0, f64::from(e.position.y) / 1000.0, deck)
            } else {
                0.0
            },
            air: profile.body.air,
            ignores,
            active: present(e),
            ready_tick: e.ready_tick,
            has_route: !u.route.is_empty(),
            goal: u.goal,
            reservation: u
                .detour
                .as_ref()
                .filter(|d| d.yielding.is_some())
                .map(|d| d.waypoint),
        })
    }

    /// Refreshes every unit body from state.
    pub fn sync_bodies(&mut self, state: &State) {
        self.bodies.begin_sync();
        for e in state.entities.iter() {
            if let Some(record) = self.record(e, ignores_units(state, e)) {
                self.bodies.update(record);
            }
        }
        self.bodies.end_sync();
    }

    /// Refreshes one unit's body after it moved or changed its plan. Whether it ghosts
    /// through bodies is kept from the last [`Spatial::sync_bodies`].
    pub fn update_body(&mut self, e: &Entity) {
        let ignores = self.bodies.get(e.id).is_some_and(|b| b.ignores);
        if let Some(record) = self.record(e, ignores) {
            self.bodies.update(record);
        }
    }

    /// The collision body of a unit.
    pub fn body_of(&self, e: &Entity) -> Body {
        self.profiles.get(e.definition).body
    }

    // ----- sweeps --------------------------------------------------------------------------

    /// Whether `body` can sweep from `from` to `to` over terrain and static blockers, its
    /// centre line also avoiding `blocked` nodes (old `clearSegment`).
    pub fn clear_segment(&self, from: Fixed, to: Fixed, blocked: Option<&dyn Reserved>, body: &Body) -> bool {
        let radius = body.radius_milli();
        let pattern = self.pattern(radius);
        if body.air {
            return from.deck.is_none()
                && to.deck.is_none()
                && clear_sweep(from, to, self.size, &pattern, |_, _| true)
                && blocked.is_none_or(|b| clear_ray(from, to, self.size, |_, c| !b.has(c)));
        }
        if let Some(surfaces) = self.terrain.surfaces() {
            let walk = |n: u32| self.walkable(n);
            let blocked_fn = blocked.map(|b| move |n: u32| b.has(n));
            return clear_layered_sweep(
                surfaces,
                from,
                to,
                body.height,
                radius,
                &pattern,
                &walk,
                blocked_fn.as_ref().map(|f| f as &dyn Fn(u32) -> bool),
            );
        }
        let ground = Ground {
            walk: &self.walk,
            heights: self.terrain.heights(),
        };
        if let Some(b) = blocked
            && !clear_ray(from, to, self.size, |a, c| ground.step(a, c) && !b.has(c))
        {
            return false;
        }
        self.ground_sweep(from, to, radius, &pattern, &ground)
    }

    fn ground_sweep(&self, from: Fixed, to: Fixed, radius: i32, pattern: &[(i32, i32)], ground: &Ground) -> bool {
        let span = (to.x - from.x).abs().max((to.y - from.y).abs());
        if (span >= 16000 || (span >= 2000 && radius >= 500))
            && let Some(clearance) = &self.clearance
            && clearance.proves_clear(self.terrain.steep_edges(), from, to, radius)
        {
            return true;
        }
        clear_sweep(from, to, self.size, pattern, |a, b| ground.step(a, b))
    }

    /// Classifies what stops a movement step (old `movementSegmentBlocker`).
    pub fn movement_blocker(
        &self,
        from: Fixed,
        to: Fixed,
        blocked: Option<&dyn Reserved>,
        body: &Body,
    ) -> Option<Obstruction> {
        if !self.clear_segment(from, to, None, body) {
            return Some(Obstruction::Terrain);
        }
        if let Some(b) = blocked {
            let clear = if self.layered() {
                self.clear_segment(from, to, Some(b), body)
            } else {
                clear_ray(from, to, self.size, |_, c| !b.has(c))
            };
            if !clear {
                return Some(Obstruction::Reservation);
            }
        }
        None
    }

    /// A local-search edge: live reservations along the centre line, exact terrain sweep
    /// (old `clearLocalSegment`).
    pub fn clear_local_segment(&self, from: Fixed, to: Fixed, blocked: &dyn Reserved, body: &Body) -> bool {
        if self.layered() || body.air {
            return self.clear_segment(from, to, Some(blocked), body);
        }
        if !clear_ray(from, to, self.size, |_, c| !blocked.has(c)) {
            return false;
        }
        let ground = Ground {
            walk: &self.walk,
            heights: self.terrain.heights(),
        };
        let pattern = self.pattern(body.radius_milli());
        clear_sweep(from, to, self.size, &pattern, |a, b| ground.step(a, b))
    }

    /// Whether a body fits at a cell centre (old `unitWalkable`).
    pub fn unit_walkable(&self, spot: Spot, body: &Body) -> bool {
        let p = spot.fixed();
        self.clear_segment(p, p, None, body)
    }

    /// The body a query without an explicit actor uses: the mover's, else the smallest
    /// ground body.
    fn query_body(&self, actor: Option<&Body>, mover: Option<&BodyRecord>) -> (i32, f64, bool) {
        match (actor, mover) {
            (Some(b), _) => (b.radius_milli(), b.height, b.air),
            (None, Some(m)) => (m.radius, m.height, m.air),
            (None, None) => {
                let b = self.profiles.navigation_body;
                (b.radius_milli(), b.height, b.air)
            }
        }
    }

    /// Whether moving `from → to` stays clear of other bodies (old `unitSegmentClear`).
    /// With `blockers`, every blocking body is collected instead of stopping at the first.
    pub fn unit_segment_clear(
        &self,
        from: Fixed,
        to: Fixed,
        except: Option<EntityId>,
        actor: Option<&Body>,
        known: Option<&dyn Fn(&BodyRecord) -> bool>,
        mut blockers: Option<&mut Vec<EntityId>>,
    ) -> bool {
        let mover = except.and_then(|id| self.bodies.get(id));
        if mover.is_some_and(|m| m.ignores) {
            return true;
        }
        let (radius, height, air) = self.query_body(actor, mover);
        let (dx, dy) = (i64::from(to.x - from.x), i64::from(to.y - from.y));
        let square = dx * dx + dy * dy;
        let reach = i64::from(radius + self.profiles.max_radius);
        let (lo_x, hi_x) = (i64::from(from.x.min(to.x)) - reach, i64::from(from.x.max(to.x)) + reach);
        let (lo_y, hi_y) = (i64::from(from.y.min(to.y)) - reach, i64::from(from.y.max(to.y)) + reach);
        let floor = if self.layered() {
            self.height(f64::from(from.x) / 1000.0, f64::from(from.y) / 1000.0, from.deck)
        } else {
            0.0
        };
        let layered = self.layered();
        let mut clear = true;
        self.bodies.within(lo_x, lo_y, hi_x, hi_y, |unit| {
            if Some(unit.id) == except || !unit.solid() || unit.air != air {
                return true;
            }
            if layered && (floor >= unit.floor + unit.height || unit.floor >= floor + height) {
                return true;
            }
            let (x, y) = (i64::from(unit.x), i64::from(unit.y));
            if x < lo_x || x > hi_x || y < lo_y || y > hi_y {
                return true;
            }
            let separation = i64::from(radius + unit.radius);
            if !geometry::body_blocks_segment(from, dx, dy, square, x, y, separation * separation) {
                return true;
            }
            if known.is_some_and(|k| !k(unit)) {
                return true;
            }
            clear = false;
            match blockers.as_deref_mut() {
                Some(list) => {
                    list.push(unit.id);
                    true
                }
                None => false,
            }
        });
        clear
    }

    /// Whether a body may stop at `spot`: it fits, touches no body, and no other body
    /// stands in or has reserved the cell (old `free`).
    pub fn free(
        &self,
        spot: Spot,
        except: Option<EntityId>,
        actor: Option<&Body>,
        known: Option<&dyn Fn(&BodyRecord) -> bool>,
    ) -> bool {
        if spot.x < 0 || spot.y < 0 || spot.x >= self.size as i32 || spot.y >= self.size as i32 {
            return false;
        }
        let mover = except.and_then(|id| self.bodies.get(id));
        let body = actor
            .copied()
            .or_else(|| mover.map(|m| self.body_by_record(m)))
            .unwrap_or(self.profiles.navigation_body);
        if !self.unit_walkable(spot, &body) {
            return false;
        }
        let p = spot.fixed();
        if !self.unit_segment_clear(p, p, except, actor, known, None) {
            return false;
        }
        if mover.is_some_and(|m| m.ignores) {
            return true;
        }
        let node = self.node(spot);
        let layered = self.layered();
        let spot_floor = if layered {
            self.height(f64::from(spot.x), f64::from(spot.y), spot.deck)
        } else {
            0.0
        };
        let occupies = |e: &BodyRecord| {
            Some(e.id) != except
                && e.solid()
                && e.air == body.air
                && (e.deck == spot.deck || (layered && (e.floor - spot_floor).abs() < e.height.max(body.height)))
                && known.is_none_or(|k| k(e))
        };
        let standing = self.bodies.in_cell(spot.x, spot.y).any(|e| {
            geometry::grid(e.x) == spot.x && geometry::grid(e.y) == spot.y && occupies(e)
        });
        let reserved = node.is_some_and(|n| self.bodies.reserved_at(n).any(&occupies));
        !standing && !reserved
    }

    /// The exact body of a recorded unit.
    fn body_by_record(&self, record: &BodyRecord) -> Body {
        Body {
            radius: f64::from(record.radius) / 1000.0,
            height: record.height,
            spacing: 0.0,
            air: record.air,
        }
    }

    /// The first free cell in growing diamonds around `origin`, up to `max` steps away
    /// (old `nearest`).
    pub fn nearest(
        &self,
        origin: Spot,
        max: i32,
        except: Option<EntityId>,
        actor: Option<&Body>,
        accept: Option<&dyn Fn(Spot) -> bool>,
        known: Option<&dyn Fn(&BodyRecord) -> bool>,
    ) -> Option<Spot> {
        for r in 0..=max {
            for dy in -r..=r {
                let rest = r - dy.abs();
                let xs: &[i32] = if rest == 0 { &[0] } else { &[-rest, rest] };
                for &dx in xs {
                    let p = Spot::new(origin.x + dx, origin.y + dy, origin.deck);
                    if self.free(p, except, actor, known) && accept.is_none_or(|a| a(p)) {
                        return Some(p);
                    }
                }
            }
        }
        None
    }

    /// The floor a fixed-point mover takes: it changes layer only when it crosses the
    /// portal cell of its goal (old `adoptSurface`).
    pub fn adopt_surface(&self, from: Fixed, to: &mut Fixed, goal: Fixed) {
        if !self.layered() {
            return;
        }
        to.deck = if from.deck != goal.deck && to.grid() == goal.grid() {
            goal.deck
        } else {
            from.deck
        };
    }

    /// Scratch cell sets for traffic claims, sized for this map.
    pub fn cell_set(&self) -> CellSet {
        CellSet::new(self.node_count() as usize)
    }

    /// Point-to-footprint squared distance in cells (old `pointRange`).
    pub fn point_range(&self, x: f64, y: f64, target: &Entity) -> f64 {
        let profile = self.profiles.get(target.definition);
        let (px, py) = (f64::from(target.position.x) / 1000.0, f64::from(target.position.y) / 1000.0);
        let (mut dx, mut dy) = ((x - px).abs(), (y - py).abs());
        if profile.has_footprint {
            let f = profile.footprint.rotated(f64::from(target.rotation) / 1000.0);
            dx = (dx - f.width / 2.0).max(0.0);
            dy = (dy - f.depth / 2.0).max(0.0);
        }
        dx * dx + dy * dy
    }

    /// Squared free space between two bodies in cells (old `bodyRange`): a building's
    /// footprint counts from its edge, a unit's from its body.
    pub fn body_range(&self, a: &Entity, b: &Entity) -> f64 {
        let (x, y) = (f64::from(a.position.x) / 1000.0, f64::from(a.position.y) / 1000.0);
        let reach = self.profiles.get(a.definition).body.radius
            + if b.unit.is_some() { self.profiles.get(b.definition).body.radius } else { 0.0 };
        let gap = (self.point_range(x, y, b).sqrt() - reach).max(0.0);
        gap * gap
    }
}

fn definitions_radii(profiles: &Profiles, catalog: &Catalog) -> Vec<i32> {
    let mut radii: Vec<i32> = (0..catalog.content.definitions.len())
        .map(|i| profiles.get(crate::catalog::Def(i as u16)).body.radius_milli())
        .collect();
    radii.push(profiles.navigation_body.radius_milli());
    radii
}
