//! Routes (old `Spatial.route`, `findPath`, `findGridPath`, `meshPath`, `routeOrigin`,
//! `routeBlockers`, `opposingTraffic`): a direct swept segment when clear; otherwise the
//! navigation mesh for long clear routes, else grid A* (confined to a sector corridor
//! when long); then the cell path is smoothed to the farthest clear waypoints.

use super::{Body, Fixed, Ground, Spatial, Spot, Topo};
use crate::{
    entity::{Entity, EntityId},
    navigation::{
        cells::{CellSet, Reserved},
        corridors::{CorridorKey, RouteCorridors},
        grid::{GridNavigation, Search, can_traverse},
        mesh::{GroundMesh, build::MeshInput},
        mesh_grid::snapped_mesh_grid_path,
        smoothing::farthest_clear_waypoint,
        surfaces,
    },
    orders::Order,
    spatial::geometry::{append_ray_cells, length, length_ceil},
};

/// Routing counters for profiling and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub routes: u64,
    pub direct: u64,
    pub searches: u64,
    pub expanded: u64,
    pub fallbacks: u64,
    pub mesh_searches: u64,
    pub mesh_accepted: u64,
    pub mesh_repaired: u64,
    pub mesh_fallbacks: u64,
    pub shared_corridors: u64,
}

/// The unit asking for a path, for traffic-aware search weighting.
#[derive(Debug, Clone, Copy)]
pub struct Asker {
    pub id: EntityId,
    pub owner: canopy_content::placement::Owner,
    pub position: Fixed,
}

impl Spatial {
    /// Runs `query` with group route sharing enabled (old `sharedRoutes`): bodies of one
    /// owner and size moving to nearby goals in this pass may reuse a checked corridor.
    pub fn with_shared_routes<T>(&mut self, query: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.corridors.replace(RouteCorridors::default());
        let result = query(self);
        self.corridors = previous;
        result
    }

    /// The body's adjacent-cell step, remembered per directed pair until occupancy around
    /// it changes (old `TerrainStepCache`).
    fn step_for(&self, radius: i32) -> impl Fn(u32, u32) -> bool + '_ {
        let exact = body_step(
            self.size,
            &self.walk,
            self.terrain.heights(),
            &self.adjacent,
            radius,
            planes(&self.clearance, &self.terrain),
        );
        let cache = self.steps.get(&radius);
        let size = self.size;
        move |a, b| {
            let Some(cache) = cache else {
                return exact(a, b);
            };
            let n = size * size;
            if a >= n || b >= n {
                return exact(a, b);
            }
            let dx = (b % size) as i32 - (a % size) as i32;
            let dy = (b / size) as i32 - (a / size) as i32;
            if dx.abs() > 1 || dy.abs() > 1 {
                return exact(a, b);
            }
            cache.get(a, ((dy + 1) * 3 + dx + 1) as u32, || exact(a, b))
        }
    }

    fn ensure_navigation(&mut self, radius: i32) {
        let cells = (self.size * self.size) as usize;
        self.steps
            .entry(radius)
            .or_insert_with(|| super::StepCache::new(cells));
        if radius >= 500 && !self.adjacent.contains_key(&radius) {
            self.adjacent
                .insert(radius, super::adjacent::AdjacentSweep::new(radius));
        }
        self.grids
            .entry(radius)
            .or_insert_with(|| GridNavigation::new(self.size));
    }

    /// A cell path from `start` to `goal` for `body` avoiding `blocked` cells and costing at
    /// most `max_cost` (old `findPath`).
    pub fn find_path(
        &mut self,
        start: u32,
        goal: u32,
        blocked: Option<&CellSet>,
        max_cost: i64,
        body: &Body,
        asker: Option<Asker>,
    ) -> Option<Vec<u32>> {
        let _span = tracing::trace_span!("path search").entered();
        if !self.valid_node(start) || !self.valid_node(goal) {
            return None;
        }
        if !self.unit_walkable(self.spot(goal), body) {
            return None;
        }
        let n = self.size * self.size;
        if body.air {
            if start >= n || goal >= n {
                return None;
            }
            let q = Search {
                blocked,
                max_cost,
                ..Search::default()
            };
            let path = self.air.path(
                &mut self.scratch,
                &mut |_, _| true,
                |_, _| true,
                start,
                goal,
                q,
            );
            self.stats.searches += 1;
            self.stats.expanded += u64::from(self.scratch.expanded);
            return path;
        }
        let radius = body.radius_milli();
        if !self.layered() {
            self.ensure_navigation(radius);
            if self.grids[&radius].proven_unreachable(start, goal) {
                return None;
            }
            let chebyshev = (i64::from(start % self.size) - i64::from(goal % self.size))
                .abs()
                .max((i64::from(start / self.size) - i64::from(goal / self.size)).abs());
            if blocked.is_none_or(|b| b.is_empty()) && body.radius <= 16.0 && chebyshev >= 16 {
                if let Some(path) = self.mesh_path(start, goal, max_cost, body) {
                    return Some(path);
                }
                return self.find_grid_path(start, goal, blocked, max_cost, body, asker, 512);
            }
        }
        self.find_grid_path(start, goal, blocked, max_cost, body, asker, 128)
    }

    /// How many ground cells `body` can walk to from `start`, counting up to `limit`: the
    /// grid search's own steps, so what it counts a route can reach. Maps with bridge
    /// decks walk their surfaces instead, and answer `limit`.
    pub fn room(&mut self, start: u32, body: &Body, limit: usize) -> usize {
        if self.layered() || body.air || !self.valid_node(start) {
            return limit;
        }
        self.reach(start, body, limit).len()
    }

    /// The ground cells `body` can walk to from `start`, in breadth-first order, at most
    /// `limit` of them: the grid search's own steps, so a route search reaches exactly
    /// these. Ground only.
    pub fn reach(&mut self, start: u32, body: &Body, limit: usize) -> Vec<u32> {
        if !self.valid_node(start) || start >= self.size * self.size {
            return Vec::new();
        }
        let radius = body.radius_milli();
        self.ensure_navigation(radius);
        let size = self.size;
        let step = self.step_for(radius);
        let mut seen = CellSet::new((size * size) as usize);
        seen.insert(start);
        let mut queue = vec![start];
        let mut next = 0;
        while next < queue.len() && queue.len() < limit {
            let cell = queue[next];
            next += 1;
            let (x, y) = ((cell % size) as i32, (cell / size) as i32);
            for (dx, dy) in crate::navigation::grid::DIRECTIONS {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= size as i32 || ny >= size as i32 {
                    continue;
                }
                let to = ny as u32 * size + nx as u32;
                if queue.len() < limit && !seen.contains(to) && can_traverse(size, cell, to, &step)
                {
                    seen.insert(to);
                    queue.push(to);
                }
            }
        }
        queue
    }

    /// Grid A* (or the walk-surface search on layered maps), confined to a sector corridor
    /// for long routes with a full-search fallback (old `findGridPath`).
    #[allow(clippy::too_many_arguments)]
    pub fn find_grid_path(
        &mut self,
        start: u32,
        goal: u32,
        blocked: Option<&CellSet>,
        max_cost: i64,
        body: &Body,
        asker: Option<Asker>,
        probe_limit: usize,
    ) -> Option<Vec<u32>> {
        if !self.valid_node(start)
            || !self.valid_node(goal)
            || !self.unit_walkable(self.spot(goal), body)
        {
            return None;
        }
        if body.air {
            return self.find_path(start, goal, blocked, max_cost, body, asker);
        }
        let blocked = blocked.filter(|b| !b.is_empty());
        let terrain = self.terrain.clone();
        if let Some(layers) = terrain.surfaces() {
            return self.layered_path(layers, start, goal, blocked, max_cost, body);
        }
        let size = self.size;
        let radius = body.radius_milli();
        self.ensure_navigation(radius);
        if self.grids[&radius].proven_unreachable(start, goal) {
            return None;
        }
        let dx = (i64::from(start % size) - i64::from(goal % size)).abs();
        let dy = (i64::from(start / size) - i64::from(goal / size)).abs();
        let topo = Topo {
            walk: &self.walk,
            heights: self.terrain.heights(),
            surfaces: None,
            occupancy: &self.occupancy,
        };
        let corridor = if dx.max(dy) >= 32 {
            match self.sectors.corridor(&topo, start, goal) {
                Err(()) => return None,
                Ok(c) => c,
            }
        } else {
            None
        };
        // Long retries through dense friendly traffic may accept a slightly longer route
        // for a smaller frontier; crossing streams keep exact search.
        let mut nearby = 0;
        if let Some(b) = blocked {
            let (sx, sy) = (i64::from(start % size), i64::from(start / size));
            for cell in b.iter() {
                if (i64::from(cell % size) - sx).abs() <= 8
                    && (i64::from(cell / size) - sy).abs() <= 8
                {
                    nearby += 1;
                    if nearby > 3 {
                        break;
                    }
                }
            }
        }
        let convoy = dx.max(dy) >= 64
            && nearby > 3
            && asker.is_some_and(|a| !self.opposing_traffic(a, self.spot(goal), body.air));
        let heuristic_permille = if blocked.is_some() && dx.max(dy) >= 64 && (nearby <= 3 || convoy)
        {
            1200
        } else {
            1000
        };
        let mut search = Search {
            blocked,
            max_cost,
            corridor: corridor.as_deref(),
            heuristic_permille,
            probe_limit,
            remember_origin: true,
        };
        let route = self.grid_search(radius, start, goal, search, false);
        if route.is_some() || corridor.is_none() {
            return route;
        }
        // Traffic may close every portal of the preferred corridor: search fully, once a
        // flood shows the search can succeed (an unreachable goal is the common case here,
        // and the flood exhausts it far cheaper than the ordered search).
        self.stats.fallbacks += 1;
        search.corridor = None;
        if !self
            .grid_search(radius, start, goal, search, true)
            .is_some()
        {
            return None;
        }
        self.grid_search(radius, start, goal, search, false)
    }

    /// The grid search for `radius`; with `flood`, only whether it would succeed (an
    /// empty route for yes).
    fn grid_search(
        &mut self,
        radius: i32,
        start: u32,
        goal: u32,
        search: Search,
        flood: bool,
    ) -> Option<Vec<u32>> {
        let mut grid = self.grids.remove(&radius).expect("grid navigation");
        let mut scratch = std::mem::take(&mut self.scratch);
        let result = {
            let mut step = body_step(
                self.size,
                &self.walk,
                self.terrain.heights(),
                &self.adjacent,
                radius,
                planes(&self.clearance, &self.terrain),
            );
            let topo = Topo {
                walk: &self.walk,
                heights: self.terrain.heights(),
                surfaces: None,
                occupancy: &self.occupancy,
            };
            let sectors = &mut self.sectors;
            if flood {
                grid.reaches(
                    &mut scratch,
                    &mut step,
                    |a, b| sectors.connected(&topo, a, b),
                    start,
                    goal,
                    search,
                )
                .then(Vec::new)
            } else {
                grid.path(
                    &mut scratch,
                    &mut step,
                    |a, b| sectors.connected(&topo, a, b),
                    start,
                    goal,
                    search,
                )
            }
        };
        self.stats.searches += 1;
        self.stats.expanded += u64::from(scratch.expanded);
        self.scratch = scratch;
        self.grids.insert(radius, grid);
        result
    }

    fn layered_path(
        &mut self,
        layers: &crate::terrain::WalkSurfaces,
        start: u32,
        goal: u32,
        blocked: Option<&CellSet>,
        max_cost: i64,
        body: &Body,
    ) -> Option<Vec<u32>> {
        let (a, b) = (self.spot(start), self.spot(goal));
        let corridor = if (a.x - b.x).abs().max((a.y - b.y).abs()) >= 32 {
            let topo = Topo {
                walk: &self.walk,
                heights: self.terrain.heights(),
                surfaces: Some(layers),
                occupancy: &self.occupancy,
            };
            match self.sectors.corridor(&topo, start, goal) {
                Err(()) => return None,
                Ok(c) => c,
            }
        } else {
            None
        };
        let mut scratch = std::mem::take(&mut self.surface_scratch);
        // Body steps between neighbouring nodes depend only on static occupancy: remember
        // them per body until it changes.
        let key = (body.radius_milli(), body.height.to_bits());
        let mut steps = self.layered_steps.remove(&key).unwrap_or_default();
        let search = |this: &Self,
                      scratch: &mut surfaces::SurfaceScratch,
                      steps: &mut std::collections::BTreeMap<(u32, u32), bool>,
                      corridor: Option<&[bool]>| {
            let topo = Topo {
                walk: &this.walk,
                heights: this.terrain.heights(),
                surfaces: Some(layers),
                occupancy: &this.occupancy,
            };
            let blocked_node = |n: &crate::terrain::SurfaceNode| {
                this.occupancy.blocked(n.id)
                    || blocked.is_some_and(|b| b.contains(n.id))
                    || corridor.is_some_and(|c| !c[this.sectors.sector(&topo, n.id) as usize])
            };
            surfaces::path(
                layers,
                scratch,
                start,
                goal,
                blocked_node,
                max_cost,
                |p, q| {
                    *steps.entry((p, q)).or_insert_with(|| {
                        this.clear_segment(this.point(p), this.point(q), None, body)
                    })
                },
            )
        };
        let route = search(self, &mut scratch, &mut steps, corridor.as_deref());
        self.stats.searches += 1;
        let route = if route.is_some() || corridor.is_none() {
            route
        } else {
            self.stats.fallbacks += 1;
            search(self, &mut scratch, &mut steps, None)
        };
        self.surface_scratch = scratch;
        self.layered_steps.insert(key, steps);
        route
    }

    fn mesh_input(&self) -> MeshInput<'_> {
        let n = (self.size * self.size) as usize;
        MeshInput {
            size: self.size,
            walkable: &self.walk[..n],
            heights: self.terrain.heights(),
        }
    }

    /// A long route along the navigation mesh, rasterised and checked cell by cell
    /// against the real body sweep, then compacted (old `meshPath`).
    /// Ensures the navigation mesh of a body exists.
    pub(super) fn mesh(&mut self, body: &Body) {
        let radius = body.radius_milli();
        if !self.meshes.contains_key(&radius) {
            let mesh = GroundMesh::new(self.mesh_input(), body.radius);
            self.meshes.insert(radius, mesh);
        }
    }

    fn mesh_path(&mut self, start: u32, goal: u32, max_cost: i64, body: &Body) -> Option<Vec<u32>> {
        let radius = body.radius_milli();
        self.mesh(body);
        let mut mesh = self.meshes.remove(&radius).expect("mesh");
        mesh.prepare(self.mesh_input());
        let (a, b) = (self.spot(start), self.spot(goal));
        let result = mesh.path(
            (f64::from(a.x), f64::from(a.y)),
            (f64::from(b.x), f64::from(b.y)),
        );
        self.meshes.insert(radius, mesh);
        self.stats.mesh_searches += 1;
        let Some(result) = result else {
            self.stats.mesh_fallbacks += 1;
            return None;
        };
        self.ensure_navigation(radius);
        let rasterised = {
            let step = self.step_for(radius);
            self.rasterise(start, &result, body, &step)
        };
        let Some((compact, repaired)) = rasterised else {
            self.stats.mesh_fallbacks += 1;
            return None;
        };
        if repaired {
            self.stats.mesh_repaired += 1;
        }
        let size = self.size;
        let mut cost = 0i64;
        let mut cell = start;
        for &next in &compact {
            cost += if next % size != cell % size && next / size != cell / size {
                1414
            } else {
                1000
            };
            cell = next;
        }
        if cost > max_cost {
            self.stats.mesh_fallbacks += 1;
            return None;
        }
        self.stats.mesh_accepted += 1;
        Some(compact)
    }

    /// Converts a mesh polyline into compacted grid cells, repairing grazed corners.
    fn rasterise(
        &self,
        start: u32,
        result: &[(f64, f64)],
        body: &Body,
        step: &impl Fn(u32, u32) -> bool,
    ) -> Option<(Vec<u32>, bool)> {
        let size = self.size;
        let mut path: Vec<u32> = Vec::new();
        let mut raster_clear = true;
        let first = self.point(start);
        let mut points = vec![first];
        let mut previous = first;
        for p in &result[1..] {
            let next = Fixed::new(
                crate::terrain::round_half_up(p.0 * 1000.0) as i32,
                crate::terrain::round_half_up(p.1 * 1000.0) as i32,
            );
            // A rounded corner can graze the boundary without invalidating the corridor;
            // once repair is needed it validates the whole route.
            if raster_clear && !self.clear_segment(previous, next, None, body) {
                raster_clear = false;
            }
            if raster_clear && !append_ray_cells(previous, next, size, step, &mut path) {
                raster_clear = false;
            }
            points.push(next);
            previous = next;
        }
        let mut repaired = false;
        if !raster_clear {
            path = snapped_mesh_grid_path(&points, size, step)?;
            repaired = true;
        }
        // Collapse cardinal pairs the raster took around a legal diagonal.
        let mut compact: Vec<u32> = Vec::with_capacity(path.len());
        for cell in path {
            while !compact.is_empty() {
                let before = if compact.len() > 1 {
                    compact[compact.len() - 2]
                } else {
                    start
                };
                if !can_traverse(size, before, cell, step) {
                    break;
                }
                compact.pop();
            }
            compact.push(cell);
        }
        Some((compact, repaired))
    }

    /// Whether other units' routes cross this one head-on nearby (old `opposingTraffic`):
    /// such crossings keep exact search so yielding stays stable.
    fn opposing_traffic(&self, asker: Asker, destination: Spot, air: bool) -> bool {
        let (fx, fy) = (
            f64::from(asker.position.x) / 1000.0,
            f64::from(asker.position.y) / 1000.0,
        );
        let (tx, ty) = (f64::from(destination.x), f64::from(destination.y));
        let (dx, dy) = (tx - fx, ty - fy);
        let (min_x, max_x) = (fx.min(tx) - 8.0, fx.max(tx) + 8.0);
        let (min_y, max_y) = (fy.min(ty) - 8.0, fy.max(ty) + 8.0);
        self.bodies.iter().any(|other| {
            let Some(goal) = other.goal else {
                return false;
            };
            if other.id == asker.id || !other.has_route || !other.solid() || other.air != air {
                return false;
            }
            let (px, py) = (f64::from(other.x) / 1000.0, f64::from(other.y) / 1000.0);
            if other.owner != asker.owner && (px - fx).abs().max((py - fy).abs()) > 16.0 {
                return false;
            }
            let q = self.spot(goal);
            let (qx, qy) = (f64::from(q.x), f64::from(q.y));
            if dx * (qx - px) + dy * (qy - py) >= 0.0 {
                return false;
            }
            !(max_x < px.min(qx) || min_x > px.max(qx) || max_y < py.min(qy) || min_y > py.max(qy))
        })
    }

    /// The node a mover joins its route from: its own cell centre if reachable, else the
    /// nearest neighbouring centre it can reach (old `routeOrigin`).
    fn route_origin(
        &self,
        cell: Spot,
        from: Fixed,
        blocked: Option<&dyn Reserved>,
        body: &Body,
    ) -> Option<u32> {
        let center = self.node(cell)?;
        if self.clear_segment(from, self.point(center), blocked, body) {
            return Some(center);
        }
        let mut candidates = Vec::with_capacity(8);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let p = Spot::new(cell.x + dx, cell.y + dy, cell.deck);
                if !self.valid_spot(p) {
                    continue;
                }
                let Some(node) = self.node(p) else {
                    continue;
                };
                let point = p.fixed();
                let distance =
                    length_ceil(i64::from(point.x - from.x), i64::from(point.y - from.y));
                candidates.push((distance, node, point));
            }
        }
        candidates.sort_by_key(|&(distance, node, _)| (distance, node));
        candidates
            .into_iter()
            .find(|&(_, _, point)| self.clear_segment(from, point, blocked, body))
            .map(|(_, node, _)| node)
    }

    /// Where a body wedged by terrain steps to, if it is wedged: the nearest free cell
    /// centre within two cells beyond its radius where it stands on one level and from
    /// which it can step on to a neighbouring centre.
    ///
    /// The sweep forbids crossing a step too high to climb, but not standing across one:
    /// a body sliding along a cliff edge (beside a ramp, typically) can end up straddling
    /// it, and then every sweep to a cell centre around it crosses the step, so no route
    /// can start and the unit stays there for good. It is wedged only when terrain and
    /// static blockers alone stop it; units around it never count.
    fn unwedge(&self, id: EntityId, here: Spot, from: Fixed, body: &Body) -> Option<Spot> {
        if body.air || self.route_origin(here, from, None, body).is_some() {
            return None;
        }
        self.wedge_exit(here, Some(id), body, None)
    }

    /// Where a route for `body` standing at `at` starts, other units disregarded: its own
    /// cell, a neighbouring centre it reaches, or where it steps to when terrain wedges it
    /// ([`Spatial::route`]). `None`: no route can ever start there.
    pub fn route_start(&self, at: Fixed, body: &Body) -> Option<u32> {
        let (x, y) = crate::entity::Milli { x: at.x, y: at.y }.cell();
        let here = Spot::new(x, y, at.deck);
        if let Some(node) = self.route_origin(here, at, None, body) {
            return Some(node);
        }
        if body.air {
            return None;
        }
        let nobody = |_: &super::BodyRecord| false;
        self.wedge_exit(here, None, body, Some(&nobody))
            .and_then(|spot| self.node(spot))
    }

    /// The nearest free cell centre within two cells beyond a wedged body's radius where it
    /// stands on one level and from which it can step on to a neighbouring centre.
    fn wedge_exit(
        &self,
        here: Spot,
        except: Option<EntityId>,
        body: &Body,
        known: Option<&dyn Fn(&super::BodyRecord) -> bool>,
    ) -> Option<Spot> {
        let leaves = |p: Spot| {
            let at = p.fixed();
            self.level(at, body)
                && (-1..=1).any(|dy: i32| {
                    (-1..=1).any(|dx: i32| {
                        let q = Spot::new(p.x + dx, p.y + dy, p.deck);
                        (dx, dy) != (0, 0)
                            && self.valid_spot(q)
                            && self.clear_segment(at, q.fixed(), None, body)
                    })
                })
        };
        // Two cells beyond the body's own radius: a wide body (a hero's) straddles an
        // edge from further off its centre.
        let reach = 2 + body.radius.ceil() as i32;
        self.nearest(here, reach, except, Some(body), Some(&leaves), known)
    }

    /// Plans a route for `e` to `destination` and stores it on the unit (old `route`).
    /// `avoid_units` treats other bodies' cells as walls for the search.
    pub fn route(&mut self, e: &mut Entity, destination: Spot, avoid_units: bool) -> bool {
        let _span = tracing::trace_span!("route").entered();
        let routed = self.plan_route(e, destination, avoid_units);
        self.update_body(e);
        routed
    }

    /// [`Spatial::route`] with the old default: avoid units unless moving or attacking.
    pub fn route_default(&mut self, e: &mut Entity, destination: Spot) -> bool {
        let avoid = !matches!(
            e.unit.as_ref().and_then(|u| u.order.as_ref()),
            Some(Order::Move { .. })
        );
        self.route(e, destination, avoid)
    }

    fn plan_route(&mut self, e: &mut Entity, destination: Spot, avoid_units: bool) -> bool {
        let size = self.size as i32;
        if e.unit.is_none()
            || destination.x < 0
            || destination.y < 0
            || destination.x > size - 1
            || destination.y > size - 1
        {
            return false;
        }
        self.stats.routes += 1;
        if let Some(idle) = e.unit.as_mut().and_then(|u| u.idle.as_mut()) {
            idle.walking = false;
        }
        let deck = self.deck_of(e);
        let (cx, cy) = e.position.cell();
        let here = Spot::new(cx, cy, deck);
        let (Some(goal), Some(mut own)) = (self.node(destination), self.node(here)) else {
            return false;
        };
        let body = self.body_of(e);
        let ignores = self.bodies.get(e.id).is_some_and(|b| b.ignores);
        let mut blockers = std::mem::take(&mut self.blockers);
        blockers.reset(self.node_count() as usize);
        if avoid_units && !ignores {
            for other in self.bodies.iter() {
                if other.id == e.id || !other.solid() || other.air != body.air {
                    continue;
                }
                blockers.insert(other.node);
                if let Some(waypoint) = other.reservation {
                    blockers.insert(waypoint);
                }
            }
        }
        let blocked: Option<&CellSet> = (!blockers.is_empty()).then_some(&blockers);
        let reserved = blocked.map(|b| b as &dyn Reserved);
        let mut from = Fixed::on(e.position.x, e.position.y, deck);
        let target = destination.fixed();
        let direct = self.clear_segment(from, target, reserved, &body);
        if direct {
            self.stats.direct += 1;
        }
        let mut origin = if direct {
            Some(own)
        } else {
            self.route_origin(here, from, reserved, &body)
        };
        if origin.is_none()
            && let Some(spot) = self.unwedge(e.id, here, from, &body)
            && let Some(node) = self.node(spot)
        {
            from = spot.fixed();
            e.position = crate::entity::Milli {
                x: from.x,
                y: from.y,
            };
            own = node;
            origin = Some(node);
        }
        let Some(origin) = origin else {
            self.blockers = blockers;
            return false;
        };
        let moving = matches!(
            e.unit.as_ref().and_then(|u| u.order.as_ref()),
            Some(Order::Move { .. })
        );
        let corridor_key: Option<CorridorKey> = (self.corridors.is_some()
            && !direct
            && !avoid_units
            && !self.layered()
            && !body.air
            && moving)
            .then(|| {
                (
                    owner_code(e.owner),
                    i64::from(body.radius_milli()),
                    body.height.to_bits(),
                )
            });
        let shared = corridor_key.and_then(|key| {
            let corridors = self.corridors.as_ref().expect("corridors");
            corridors.find(
                key,
                from,
                target,
                goal,
                |n| self.point(n),
                |a, b| self.clear_segment(a, b, None, &body),
            )
        });
        let reused = shared.is_some();
        if reused {
            self.stats.shared_corridors += 1;
        }
        let asker = Asker {
            id: e.id,
            owner: e.owner,
            position: from,
        };
        let searched = match &shared {
            Some(route) => Some(route.clone()),
            None if direct => Some(vec![goal]),
            // Routes have no cost limit, so joining the origin needs no budget.
            None => self.find_path(origin, goal, blocked, i64::MAX, &body, Some(asker)),
        };
        let Some(searched) = searched else {
            self.blockers = blockers;
            return false;
        };
        let waypoints = match shared {
            Some(route) => route,
            None => {
                let mut path = searched;
                if origin != own {
                    path.insert(0, origin);
                }
                let mut waypoints = Vec::new();
                let mut anchor = from;
                let mut i = 0;
                let mut failed = false;
                while i < path.len() {
                    if !self.clear_segment(anchor, self.point(path[i]), reserved, &body) {
                        // An interrupted mover beside a corner joins via its cell centre.
                        let center = self.point(own);
                        if i != 0
                            || !self.clear_segment(anchor, center, reserved, &body)
                            || !self.clear_segment(center, self.point(path[i]), reserved, &body)
                        {
                            failed = true;
                            break;
                        }
                        waypoints.push(own);
                        anchor = center;
                    }
                    let farthest = farthest_clear_waypoint(i, path.len() - 1, |index| {
                        self.clear_segment(anchor, self.point(path[index]), reserved, &body)
                    });
                    waypoints.push(path[farthest]);
                    anchor = self.point(path[farthest]);
                    i = farthest + 1;
                }
                if failed {
                    self.blockers = blockers;
                    return false;
                }
                waypoints
            }
        };
        self.blockers = blockers;
        let mut waypoints = waypoints;
        if waypoints.is_empty() && (from.x != target.x || from.y != target.y) {
            waypoints.push(goal);
        }
        if let Some(key) = corridor_key
            && !reused
            && let Some(corridors) = self.corridors.as_mut()
        {
            corridors.remember(key, from, target, &waypoints);
        }
        let unit = e.unit.as_mut().expect("unit");
        unit.route = waypoints;
        unit.detour = None;
        unit.goal = Some(goal);
        unit.settled = false;
        true
    }

    /// The straight-line length in milli-cells of a route from `from` through its nodes.
    pub fn route_length(&self, from: Fixed, route: &[u32]) -> f64 {
        let mut total = 0.0;
        let mut anchor = from;
        for &node in route {
            let p = self.point(node);
            total += length(i64::from(p.x - anchor.x), i64::from(p.y - anchor.y));
            anchor = p;
        }
        total
    }
}

fn owner_code(owner: canopy_content::placement::Owner) -> u8 {
    match owner {
        canopy_content::placement::Owner::Nobody => 0,
        canopy_content::placement::Owner::Player(slot) => slot + 1,
    }
}

/// The adjacent-cell step rule of a body radius on the ground grid: the compiled sweep
/// for bodies of half a cell or more, the centre cell otherwise.
/// The ground grid's closed and steep cells as bit planes (maps without decks).
fn planes<'a>(
    clearance: &'a Option<super::clearance::Clearance>,
    terrain: &'a crate::terrain::Terrain,
) -> Option<super::adjacent::Planes<'a>> {
    let steep = terrain.steep_edges();
    clearance.as_ref().map(|clearance| super::adjacent::Planes {
        closed: clearance.closed(),
        steep: steep.any(),
        words: steep.words_per_row(),
    })
}

fn body_step<'a>(
    size: u32,
    walk: &'a [bool],
    heights: &'a [i16],
    adjacent: &'a std::collections::BTreeMap<i32, super::adjacent::AdjacentSweep>,
    radius: i32,
    planes: Option<super::adjacent::Planes<'a>>,
) -> impl Fn(u32, u32) -> bool + 'a {
    let ground = Ground { walk, heights };
    let compiled = if radius >= 500 {
        adjacent.get(&radius)
    } else {
        None
    };
    move |a, b| match (compiled, planes) {
        (Some(sweep), Some(planes)) => {
            sweep.clear_planes(size, a, b, planes, |p, q| ground.step(p, q))
        }
        (Some(sweep), None) => sweep.clear(size, a, b, |p, q| ground.step(p, q)),
        (None, _) => ground.step(a, b),
    }
}
