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

pub use bodies::{Bodies, BodyRecord};
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
        footprint::footprint_cells, round_half_up,
    },
};
use adjacent::AdjacentSweep;
use canopy_content::game::Kind;
use clearance::Clearance;
use geometry::{clear_ray, clear_sweep, sweep_pattern};
use occupancy::{Cover, Occupancy};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

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

/// Whether a unit ghosts through other bodies (old `GameContext` `ignoresUnits`): workers
/// delivering goods, or on harvest traffic to a source that does not opt back into
/// collision, and archers in lookouts.
pub fn ignores_units(state: &State, profiles: &Profiles, e: &Entity) -> bool {
    if crate::garrisons::ghost(e) {
        return true;
    }
    let Some(u) = e.unit.as_ref() else {
        return false;
    };
    if !profiles.get(e.definition).work {
        return false;
    }
    let job = u.work.job.and_then(|id| state.economy.job(id));
    if job.is_some_and(|j| j.kind == crate::economy::JobKind::Deliver) {
        return true;
    }
    let source = match (job, &u.order) {
        (Some(j), _) if j.kind == crate::economy::JobKind::Harvest => j.source,
        (Some(_), _) => None,
        (None, Some(crate::orders::Order::Gather { target })) => Some(*target),
        (None, _) => None,
    };
    source
        .and_then(|id| state.entities.get(id))
        .is_some_and(|s| !profiles.get(s.definition).gathering_collision)
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

/// Remembered adjacent body steps of one radius: nine direction bits (stationary
/// included) per cell. Shared through `&` by the step closures, hence the cells.
#[derive(Debug)]
pub(crate) struct StepCache {
    known: Vec<std::cell::Cell<u16>>,
    clear: Vec<std::cell::Cell<u16>>,
}

impl StepCache {
    fn new(cells: usize) -> Self {
        Self {
            known: vec![std::cell::Cell::new(0); cells],
            clear: vec![std::cell::Cell::new(0); cells],
        }
    }

    fn get(&self, cell: u32, direction: u32, compute: impl FnOnce() -> bool) -> bool {
        let (i, bit) = (cell as usize, 1u16 << direction);
        if self.known[i].get() & bit == 0 {
            let value = compute();
            self.known[i].set(self.known[i].get() | bit);
            let clear = self.clear[i].get();
            self.clear[i].set(if value { clear | bit } else { clear & !bit });
        }
        self.clear[i].get() & bit != 0
    }

    /// Forgets steps from cells within `padding` of changed cells.
    fn invalidate(&self, size: u32, changed: &[u32], padding: i32) {
        let s = size as i32;
        for &cell in changed {
            if cell >= size * size {
                continue;
            }
            let (x, y) = ((cell % size) as i32, (cell / size) as i32);
            for ny in (y - padding).max(0)..=(y + padding).min(s - 1) {
                for nx in (x - padding).max(0)..=(x + padding).min(s - 1) {
                    self.known[(ny * s + nx) as usize].set(0);
                }
            }
        }
    }
}

/// What decides a static blocker's cells: definition, position, rotation, standing flags
/// and floor.
type Signature = (crate::catalog::Def, i32, i32, i32, u64, Option<String>);

pub struct Spatial {
    terrain: Arc<Terrain>,
    size: u32,
    pub profiles: Profiles,
    /// The constants movement and the attack approach play by (the lab may change them).
    pub tuning: crate::movement::tuning::Tuning,
    /// Per node: walkable terrain (or deck) not covered by a building or resource.
    walk: Vec<bool>,
    occupancy: Occupancy,
    /// Static blocker signatures by entity id, to detect changes (ids are dense, so a
    /// vector beats a map when every tree on a forest map is checked several times a tick).
    statics: Vec<Option<Signature>>,
    /// Signatures held in `statics`.
    static_count: usize,
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
    /// Scratch for [`Spatial::sync_static`].
    present: Vec<EntityId>,
    /// Remembered adjacent body steps per radius (milli-cells).
    steps: BTreeMap<i32, StepCache>,
    /// Remembered body steps between neighbouring walk-surface nodes, per body (radius,
    /// height bits); cleared on every static change.
    layered_steps: BTreeMap<(i32, u64), BTreeMap<(u32, u32), bool>>,
    /// Prefix sums of ground cells under a deck, `(size + 1)²` (layered maps only).
    deck_cells: Vec<u32>,
    pub stats: Stats,
}

impl Spatial {
    /// Builds the spatial model for `state` on `terrain`. `scales` gives the appearance
    /// scale of placements that declare one (resource collision discs scale with it).
    pub fn new(
        terrain: Arc<Terrain>,
        catalog: &Catalog,
        state: &State,
        scales: BTreeMap<String, f64>,
    ) -> Self {
        let size = terrain.size();
        let profiles = Profiles::new(catalog);
        let layered = terrain.surfaces().is_some();
        let nodes = terrain
            .surfaces()
            .map_or((size * size) as usize, |s| s.node_count() as usize);
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
            tuning: Default::default(),
            clearance: (!layered).then(|| {
                Clearance::new(size, |c| walk[c as usize]).with_steep(terrain.steep_edges().any())
            }),
            terrain,
            size,
            profiles,
            walk,
            occupancy,
            statics: Vec::new(),
            static_count: 0,
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
            present: Vec::new(),
            deck_cells: Vec::new(),
            layered_steps: BTreeMap::new(),
            steps: BTreeMap::new(),
            stats: Stats::default(),
        };
        if let Some(surfaces) = this.terrain.surfaces() {
            let n = this.size as usize;
            let mut prefix = vec![0u32; (n + 1) * (n + 1)];
            for y in 0..n {
                let mut row = 0;
                for x in 0..n {
                    row += u32::from(surfaces.has_deck((y * n + x) as u32));
                    prefix[(y + 1) * (n + 1) + x + 1] = prefix[y * (n + 1) + x + 1] + row;
                }
            }
            this.deck_cells = prefix;
        }
        this.sync_static(state);
        this.sync_bodies(state);
        this.prepare_meshes(state);
        this
    }

    /// Brings the model to `state` from whatever state it followed before (a replay's
    /// jump back or ahead): the static blockers and bodies are synced as after every tick,
    /// so what was built stays and only what differs is redone; the caches start afresh,
    /// as in a model built from `state` ([`Spatial::new`]).
    pub fn rewind(&mut self, state: &State) {
        self.scratch = Scratch::default();
        self.surface_scratch = SurfaceScratch::default();
        self.corridors = None;
        self.steps.clear();
        self.layered_steps.clear();
        self.present.clear();
        self.sync_static(state);
        self.sync_bodies(state);
        self.prepare_meshes(state);
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
            None => (spot.x >= 0
                && spot.y >= 0
                && spot.x < self.size as i32
                && spot.y < self.size as i32)
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
        f64::from(
            self.terrain
                .height_cm((cy * self.size as i64 + cx) as usize),
        ) / 100.0
    }

    /// Whether a ground body standing at `p` stands on one level: no two neighbouring
    /// open cells under its sweep's rays differ by more than a climbable step. Bridges'
    /// decks are not judged.
    fn level(&self, p: Fixed, body: &Body) -> bool {
        if p.deck.is_some() || self.layered() {
            return true;
        }
        let size = i64::from(self.size);
        let mut cells: Vec<(i64, i64)> = self
            .pattern(body.radius_milli())
            .iter()
            .map(|&(ox, oy)| {
                (
                    (i64::from(p.x + ox) + 500).div_euclid(1000),
                    (i64::from(p.y + oy) + 500).div_euclid(1000),
                )
            })
            .collect();
        cells.sort_unstable();
        cells.dedup();
        let heights = self.terrain.heights();
        let open = |x: i64, y: i64| {
            (x >= 0 && y >= 0 && x < size && y < size)
                .then(|| (y * size + x) as usize)
                .filter(|&i| self.walk[i])
        };
        cells.iter().all(|&(x, y)| {
            let Some(a) = open(x, y) else {
                return true;
            };
            [(x + 1, y), (x, y + 1)].into_iter().all(|(nx, ny)| {
                cells.binary_search(&(nx, ny)).is_err()
                    || open(nx, ny).is_none_or(|b| {
                        (i32::from(heights[a]) - i32::from(heights[b])).abs() <= MAX_GROUND_STEP_CM
                    })
            })
        })
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
        let (x, y) = (
            f64::from(e.position.x) / 1000.0,
            f64::from(e.position.y) / 1000.0,
        );
        let rotation = f64::from(e.rotation) / 1000.0;
        let size = i64::from(self.size);
        let bounds = footprint_cells(x, y, profile.footprint, rotation);
        let in_map = |cx: i64, cy: i64| {
            (cx >= 0 && cy >= 0 && cx < size && cy < size).then(|| (cy * size + cx) as u32)
        };
        if building {
            cover.building = bounds
                .cells()
                .filter_map(|(cx, cy)| in_map(cx, cy))
                .collect();
        }
        if resource {
            cover.resource = self.collision_cells(e);
        }
        cover
    }

    /// The ground cells a resource closes while it stands, whether or not it stands now:
    /// its footprint, widened to its scaled collision disc (old `collision`).
    pub fn collision_cells(&self, e: &Entity) -> Vec<u32> {
        let profile = self.profiles.get(e.definition);
        let (x, y) = (
            f64::from(e.position.x) / 1000.0,
            f64::from(e.position.y) / 1000.0,
        );
        let rotation = f64::from(e.rotation) / 1000.0;
        let size = i64::from(self.size);
        let bounds = footprint_cells(x, y, profile.footprint, rotation);
        let in_map = |cx: i64, cy: i64| {
            (cx >= 0 && cy >= 0 && cx < size && cy < size).then(|| (cy * size + cx) as u32)
        };
        match profile.collision_radius {
            None => bounds
                .cells()
                .filter_map(|(cx, cy)| in_map(cx, cy))
                .collect(),
            Some(radius) => {
                let scale = e
                    .placement
                    .as_ref()
                    .and_then(|p| self.scales.get(p))
                    .copied()
                    .unwrap_or(1.0);
                let r = radius * scale;
                let mut cells = Vec::new();
                let (y0, y1) = (
                    bounds.min_y.min((y - r).ceil() as i64),
                    bounds.max_y.max((y + r).floor() as i64),
                );
                let (x0, x1) = (
                    bounds.min_x.min((x - r).ceil() as i64),
                    bounds.max_x.max((x + r).floor() as i64),
                );
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
        }
    }

    /// Brings static occupancy up to date with the buildings and resources in `state`,
    /// touching only entities whose blocking footprint changed.
    pub fn sync_static(&mut self, state: &State) {
        let _span = tracing::trace_span!("static occupancy").entered();
        let mut changed = Vec::new();
        let mut present = std::mem::take(&mut self.present);
        present.clear();
        for e in state.entities.iter() {
            if !self.is_static(e) {
                continue;
            }
            present.push(e.id);
            self.sync_one(e, &mut changed);
        }
        // Every present blocker has a signature, so equal counts mean nothing went away.
        let present_count = present.len();
        if present_count != self.static_count {
            let gone: Vec<EntityId> = (0..self.statics.len() as u32)
                .map(EntityId)
                .filter(|id| {
                    self.statics[id.0 as usize].is_some() && present.binary_search(id).is_err()
                })
                .collect();
            for id in gone {
                self.forget_static(id);
                self.occupancy.set(id, Cover::default(), &mut changed);
            }
        }
        self.present = present;
        if !changed.is_empty() {
            changed.sort_unstable();
            changed.dedup();
            self.invalidate(&changed);
        }
    }

    fn is_static(&self, e: &Entity) -> bool {
        self.profiles.get(e.definition).kind == Kind::Building || e.resource.is_some()
    }

    /// Brings one static blocker's cells up to date if what decides them changed.
    fn sync_one(&mut self, e: &Entity, changed: &mut Vec<u32>) {
        let alive = e.hp.is_none_or(|hp| hp > 0);
        let standing = e.resource.as_ref().is_some_and(|r| r.amount > 0);
        let flags = u64::from(alive) | u64::from(standing) << 1;
        let slot = e.id.0 as usize;
        if let Some(Some((definition, x, y, rotation, held, surface))) = self.statics.get(slot)
            && (*definition, *x, *y, *rotation, *held)
                == (e.definition, e.position.x, e.position.y, e.rotation, flags)
            && *surface == e.surface
        {
            return;
        }
        let cover = self.cover(e);
        self.occupancy.set(e.id, cover, changed);
        if self.statics.len() <= slot {
            self.statics.resize(slot + 1, None);
        }
        let signature = (
            e.definition,
            e.position.x,
            e.position.y,
            e.rotation,
            flags,
            e.surface.clone(),
        );
        if self.statics[slot].replace(signature).is_none() {
            self.static_count += 1;
        }
    }

    /// Drops a static blocker's signature; whether it had one.
    fn forget_static(&mut self, id: EntityId) -> bool {
        let held = self
            .statics
            .get_mut(id.0 as usize)
            .and_then(Option::take)
            .is_some();
        self.static_count -= usize::from(held);
        held
    }

    /// Follows one entity's static change at once (a site placed, a resource depleted or
    /// regrown, an upgrade, a removal), so later queries in the same pass see it (old
    /// `appendOccupancy`, `refreshAfterRemoval`, `refreshOccupancyDefinition`). The full
    /// [`Spatial::sync_static`] would find the same change.
    pub fn sync_entity(&mut self, state: &State, id: EntityId) {
        let mut changed = Vec::new();
        match state.entities.get(id) {
            Some(e) if self.is_static(e) => self.sync_one(e, &mut changed),
            found => {
                if self.forget_static(id) {
                    self.occupancy.set(id, Cover::default(), &mut changed);
                }
                // A unit that left the world stops blocking at once, as the old game
                // removed it from the spatial index.
                if found.is_none() {
                    self.bodies.remove(id);
                }
            }
        }
        if !changed.is_empty() {
            changed.sort_unstable();
            changed.dedup();
            self.invalidate(&changed);
        }
    }

    /// Whether two nodes lie in one walkable region (old `sectors.connected`).
    pub fn connected(&mut self, a: u32, b: u32) -> bool {
        let topo = Topo {
            walk: &self.walk,
            heights: self.terrain.heights(),
            surfaces: self.terrain.surfaces(),
            occupancy: &self.occupancy,
        };
        self.sectors.connected(&topo, a, b)
    }

    fn invalidate(&mut self, changed: &[u32]) {
        let surfaces = self.terrain.surfaces();
        let mut opened = false;
        for &node in changed {
            let base = match surfaces {
                Some(s) => s.walkable(node),
                None => self.terrain.walkable(node as usize),
            };
            let walk = base && !self.occupancy.blocked(node);
            opened |= walk && !self.walk[node as usize];
            self.walk[node as usize] = walk;
            if let Some(clearance) = &mut self.clearance {
                clearance.refresh(node, self.walk[node as usize]);
            }
        }
        self.layered_steps.clear();
        for (&radius, cache) in &self.steps {
            cache.invalidate(self.size, changed, (radius + 999) / 1000 + 2);
        }
        let padding = 1 + (self.profiles.max_radius + 500) / 1000;
        // Body steps pass only walkable cells, so closing cells opens no route.
        for grid in self.grids.values_mut() {
            if opened {
                grid.invalidate(Some(changed), padding);
            } else {
                grid.invalidate_closed(changed, padding);
            }
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
                self.height(
                    f64::from(e.position.x) / 1000.0,
                    f64::from(e.position.y) / 1000.0,
                    deck,
                )
            } else {
                0.0
            },
            air: crate::abilities::forms::air(profile.body.air, e),
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
            if let Some(record) = self.record(e, ignores_units(state, &self.profiles, e)) {
                self.bodies.update(record);
            }
        }
        self.bodies.end_sync();
    }

    /// Refreshes one unit's body, re-deciding whether it ghosts (its job changed).
    pub fn refresh_body(&mut self, state: &State, e: &Entity) {
        if self.bodies.get(e.id).is_none() {
            return;
        }
        if let Some(record) = self.record(e, ignores_units(state, &self.profiles, e)) {
            self.bodies.update(record);
        }
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
        let mut body = self.profiles.get(e.definition).body;
        // Flight forms change the collision class, never the body size.
        body.air = crate::abilities::forms::air(body.air, e);
        body
    }

    // ----- sweeps --------------------------------------------------------------------------

    /// Whether `body` can sweep from `from` to `to` over terrain and static blockers, its
    /// centre line also avoiding `blocked` nodes (old `clearSegment`).
    pub fn clear_segment(
        &self,
        from: Fixed,
        to: Fixed,
        blocked: Option<&dyn Reserved>,
        body: &Body,
    ) -> bool {
        let radius = body.radius_milli();
        let pattern = self.pattern(radius);
        if body.air {
            return from.deck.is_none()
                && to.deck.is_none()
                && clear_sweep(from, to, self.size, &pattern, |_, _| true)
                && blocked.is_none_or(|b| clear_ray(from, to, self.size, |_, c| !b.has(c)));
        }
        if let Some(surfaces) = self.terrain.surfaces() {
            if from.deck.is_none() && to.deck.is_none() && !self.decks_near(from, to, radius) {
                // Away from every deck the layered rules reduce to the ground grid, with
                // live claims closing every ray of the sweep.
                let ground = Ground {
                    walk: &self.walk,
                    heights: self.terrain.heights(),
                };
                let step = |a: u32, b: u32| ground.step(a, b) && blocked.is_none_or(|r| !r.has(b));
                return clear_ray(from, to, self.size, step)
                    && clear_sweep(from, to, self.size, &pattern, step);
            }
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

    /// Whether a deck spans any cell a sweep of `radius` between the points may touch.
    fn decks_near(&self, from: Fixed, to: Fixed, radius: i32) -> bool {
        let size = self.size as i64;
        let cell = |v: i64| (v + 500).div_euclid(1000);
        let r = i64::from(radius);
        let x0 = (cell(i64::from(from.x.min(to.x)) - r) - 1).clamp(0, size - 1);
        let x1 = (cell(i64::from(from.x.max(to.x)) + r) + 1).clamp(0, size - 1);
        let y0 = (cell(i64::from(from.y.min(to.y)) - r) - 1).clamp(0, size - 1);
        let y1 = (cell(i64::from(from.y.max(to.y)) + r) + 1).clamp(0, size - 1);
        let stride = (size + 1) as usize;
        let at = |x: i64, y: i64| self.deck_cells[y as usize * stride + x as usize];
        at(x1 + 1, y1 + 1) + at(x0, y0) - at(x0, y1 + 1) - at(x1 + 1, y0) > 0
    }

    fn ground_sweep(
        &self,
        from: Fixed,
        to: Fixed,
        radius: i32,
        pattern: &[(i32, i32)],
        ground: &Ground,
    ) -> bool {
        // The old game tried the envelope proof only on long or wide sweeps; it is a sound
        // positive proof for any sweep and far cheaper than the rays, so it always goes
        // first. An unproven sweep still takes the exact rays.
        if let Some(clearance) = &self.clearance
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
    pub fn clear_local_segment(
        &self,
        from: Fixed,
        to: Fixed,
        blocked: &dyn Reserved,
        body: &Body,
    ) -> bool {
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
        let radius = body.radius_milli();
        self.ground_sweep(from, to, radius, &self.pattern(radius), &ground)
    }

    /// Whether a body fits at a cell centre (old `unitWalkable`).
    pub fn unit_walkable(&self, spot: Spot, body: &Body) -> bool {
        let p = spot.fixed();
        // On the ground grid the body's compiled sweep of its own cell answers the same,
        // read from the bit planes.
        let radius = body.radius_milli();
        if !body.air
            && spot.deck.is_none()
            && !self.layered()
            && let (Some(sweep), Some(planes)) = (
                self.adjacent.get(&radius).filter(|_| radius >= 500),
                routing::planes(&self.clearance, &self.terrain),
            )
            && spot.x >= 0
            && spot.y >= 0
            && spot.x < self.size as i32
            && spot.y < self.size as i32
        {
            let ground = Ground {
                walk: &self.walk,
                heights: self.terrain.heights(),
            };
            let cell = spot.y as u32 * self.size + spot.x as u32;
            return sweep.clear_planes(self.size, cell, cell, planes, |a, b| ground.step(a, b));
        }
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
        let (lo_x, hi_x) = (
            i64::from(from.x.min(to.x)) - reach,
            i64::from(from.x.max(to.x)) + reach,
        );
        let (lo_y, hi_y) = (
            i64::from(from.y.min(to.y)) - reach,
            i64::from(from.y.max(to.y)) + reach,
        );
        let floor = if self.layered() {
            self.height(
                f64::from(from.x) / 1000.0,
                f64::from(from.y) / 1000.0,
                from.deck,
            )
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

    /// The bodies touching the mover at `from` that [`Spatial::unit_segment_clear`] from
    /// there would test (same filters), for answering many segments from one origin:
    /// [`NearBodies::blocks`] proves a segment blocked without the full query.
    pub fn near_bodies(
        &self,
        from: Fixed,
        except: Option<EntityId>,
        actor: Option<&Body>,
    ) -> NearBodies {
        let mover = except.and_then(|id| self.bodies.get(id));
        let mut near = NearBodies {
            from,
            bodies: Vec::new(),
        };
        if mover.is_some_and(|m| m.ignores) {
            return near;
        }
        let (radius, height, air) = self.query_body(actor, mover);
        let reach = i64::from(radius + self.profiles.max_radius);
        let (x, y) = (i64::from(from.x), i64::from(from.y));
        let layered = self.layered();
        let floor = if layered {
            self.height(
                f64::from(from.x) / 1000.0,
                f64::from(from.y) / 1000.0,
                from.deck,
            )
        } else {
            0.0
        };
        self.bodies
            .within(x - reach, y - reach, x + reach, y + reach, |unit| {
                if Some(unit.id) == except || !unit.solid() || unit.air != air {
                    return true;
                }
                if layered && (floor >= unit.floor + unit.height || unit.floor >= floor + height) {
                    return true;
                }
                let (ux, uy) = (i64::from(unit.x), i64::from(unit.y));
                let separation = i64::from(radius + unit.radius);
                if (ux - x).pow(2) + (uy - y).pow(2) < separation * separation {
                    near.bodies.push((ux, uy, separation * separation));
                }
                true
            });
        near
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
        !self.claimed(spot, except, &body, known)
    }

    /// Bodies touching a mover standing at any cell centre of a rectangle, answered once
    /// for the whole rectangle: [`Spatial::free_scanned`] then answers what
    /// [`Spatial::free`] would at each cell without querying the bodies again. `None` on
    /// layered maps, where the answer depends on each spot's floor.
    pub fn spot_scan(
        &self,
        (x0, y0): (i32, i32),
        (x1, y1): (i32, i32),
        except: Option<EntityId>,
        body: &Body,
    ) -> Option<SpotScan> {
        if self.layered() || x0 > x1 || y0 > y1 {
            return None;
        }
        let ignores = except
            .and_then(|id| self.bodies.get(id))
            .is_some_and(|m| m.ignores);
        let (w, h) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        let mut scan = SpotScan {
            x0,
            y0,
            w,
            h,
            touched: vec![false; w * h],
            ignores,
        };
        if ignores {
            return Some(scan);
        }
        // What `unit_segment_clear(p, p, except, Some(body), None, None)` tests at each
        // centre `p`: solid bodies of the mover's layer other than itself, inside its reach
        // box, whose centres stand closer than the two radii.
        let radius = body.radius_milli();
        let reach = i64::from(radius + self.profiles.max_radius);
        let (lo_x, lo_y) = (i64::from(x0) * 1000 - reach, i64::from(y0) * 1000 - reach);
        let (hi_x, hi_y) = (i64::from(x1) * 1000 + reach, i64::from(y1) * 1000 + reach);
        self.bodies.within(lo_x, lo_y, hi_x, hi_y, |unit| {
            if Some(unit.id) == except || !unit.solid() || unit.air != body.air {
                return true;
            }
            let (ux, uy) = (i64::from(unit.x), i64::from(unit.y));
            let separation = i64::from(radius + unit.radius);
            let span = separation.min(reach);
            let cells = |u: i64, lo: i32, hi: i32| {
                let a = (u - span).div_euclid(1000).max(i64::from(lo));
                let b = ((u + span).div_euclid(1000) + 1).min(i64::from(hi));
                a..=b
            };
            let square = separation * separation;
            for cy in cells(uy, y0, y1) {
                let sy = uy - cy * 1000;
                if sy.abs() > reach {
                    continue;
                }
                let row = (cy - i64::from(y0)) as usize * w;
                for cx in cells(ux, x0, x1) {
                    let sx = ux - cx * 1000;
                    // `geometry::body_blocks_segment` of a point (exact in integers: the
                    // offsets are within the reach).
                    if sx.abs() <= reach && sx * sx + sy * sy < square {
                        scan.touched[row + (cx - i64::from(x0)) as usize] = true;
                    }
                }
            }
            true
        });
        Some(scan)
    }

    /// [`Spatial::free`] at a cell of a [`SpotScan`] made for the same mover and body,
    /// without the terrain fit ([`Spatial::unit_walkable`]), which the caller tests.
    pub fn free_scanned(
        &self,
        scan: &SpotScan,
        spot: Spot,
        except: Option<EntityId>,
        body: &Body,
    ) -> bool {
        if spot.x < 0 || spot.y < 0 || spot.x >= self.size as i32 || spot.y >= self.size as i32 {
            return false;
        }
        let (cx, cy) = ((spot.x - scan.x0) as usize, (spot.y - scan.y0) as usize);
        assert!(cx < scan.w && cy < scan.h, "spot outside its scan");
        if scan.touched[cy * scan.w + cx] {
            return false;
        }
        scan.ignores || !self.claimed(spot, except, body, None)
    }

    /// Whether another body stands in or has reserved `spot` (the last test of
    /// [`Spatial::free`]).
    fn claimed(
        &self,
        spot: Spot,
        except: Option<EntityId>,
        body: &Body,
        known: Option<&dyn Fn(&BodyRecord) -> bool>,
    ) -> bool {
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
                && (e.deck == spot.deck
                    || (layered && (e.floor - spot_floor).abs() < e.height.max(body.height)))
                && known.is_none_or(|k| k(e))
        };
        let standing = self
            .bodies
            .in_cell(spot.x, spot.y)
            .any(|e| geometry::grid(e.x) == spot.x && geometry::grid(e.y) == spot.y && occupies(e));
        standing || node.is_some_and(|n| self.bodies.reserved_at(n).any(&occupies))
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
        let (px, py) = (
            f64::from(target.position.x) / 1000.0,
            f64::from(target.position.y) / 1000.0,
        );
        let (mut dx, mut dy) = ((x - px).abs(), (y - py).abs());
        if profile.has_footprint {
            let f = profile
                .footprint
                .rotated(f64::from(target.rotation) / 1000.0);
            dx = (dx - f.width / 2.0).max(0.0);
            dy = (dy - f.depth / 2.0).max(0.0);
        }
        dx * dx + dy * dy
    }

    /// Squared free space between two bodies in cells (old `bodyRange`): a building's
    /// footprint counts from its edge, a unit's from its body.
    pub fn body_range(&self, a: &Entity, b: &Entity) -> f64 {
        let (x, y) = (
            f64::from(a.position.x) / 1000.0,
            f64::from(a.position.y) / 1000.0,
        );
        let other = if b.unit.is_some() {
            self.profiles.get(b.definition).body.radius
        } else {
            0.0
        };
        // Radii come off one after the other, as in the old game (float order matters).
        let gap = (self.point_range(x, y, b).sqrt()
            - self.profiles.get(a.definition).body.radius
            - other)
            .max(0.0);
        gap * gap
    }
}

fn definitions_radii(profiles: &Profiles, catalog: &Catalog) -> Vec<i32> {
    let mut radii: Vec<i32> = (0..catalog.content.definitions.len())
        .map(|i| {
            profiles
                .get(crate::catalog::Def(i as u16))
                .body
                .radius_milli()
        })
        .collect();
    radii.push(profiles.navigation_body.radius_milli());
    radii
}

/// Which cell centres of a rectangle a body touches, from [`Spatial::spot_scan`].
pub struct SpotScan {
    x0: i32,
    y0: i32,
    w: usize,
    h: usize,
    touched: Vec<bool>,
    /// The mover ghosts through bodies: only the terrain decides.
    ignores: bool,
}

/// Bodies touching a mover, from [`Spatial::near_bodies`].
pub struct NearBodies {
    from: Fixed,
    /// Position and squared separation from the mover, milli-cells.
    bodies: Vec<(i64, i64, i64)>,
}

impl NearBodies {
    /// True when one of these bodies blocks `from → to`, so
    /// [`Spatial::unit_segment_clear`] would say it is not clear (it tests the same
    /// bodies with the same geometry). False proves nothing.
    pub fn blocks(&self, to: Fixed) -> bool {
        let (dx, dy) = (i64::from(to.x - self.from.x), i64::from(to.y - self.from.y));
        let square = dx * dx + dy * dy;
        self.bodies.iter().any(|&(x, y, separation_squared)| {
            geometry::body_blocks_segment(self.from, dx, dy, square, x, y, separation_squared)
        })
    }
}
