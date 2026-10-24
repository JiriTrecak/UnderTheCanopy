//! What an AI knows of the ground it moves over (old `ObservedBlockers` in `ai/frame.ts`
//! and `ai/observedNavigation.ts`).
//!
//! Only authored terrain and the buildings and standing resources the player knows
//! (seen or remembered) enter it; moving units stay the simulation's business. The
//! engine's own sweeps, sector graph, grid search and navigation mesh run over this
//! observed walkability, so the AI judges body clearance exactly as movement does without
//! seeing hidden blockers. Everything here is derived: rebuilt from knowledge on load, and
//! every cache answers as a cold rebuild would.

use super::geography::{Geography, Pt};
use crate::{
    navigation::{
        grid::{GridNavigation, Scratch, Search},
        mesh::{GroundMesh, build::MeshInput},
        mesh_grid::snapped_mesh_grid_path,
        sectors::{Sectors, Topology},
        surfaces::{SurfaceScratch, clear_layered_sweep},
    },
    spatial::{
        adjacent::{AdjacentSweep, Planes},
        clearance::Clearance,
        geometry::{Fixed, clear_sweep, sweep_pattern},
    },
    terrain::{MAX_GROUND_STEP_CM, round_half_up},
};
use std::collections::BTreeMap;

/// One known blocker's cells and what decided them.
#[derive(Debug, Clone, PartialEq)]
struct Record {
    signature: (u16, i32, i32, i32, bool),
    cells: Vec<u32>,
}

/// A known building or standing resource, as the blocker index needs it.
pub struct Blocker {
    pub id: u32,
    /// Definition, position (milli), rotation (millidegrees), building.
    pub signature: (u16, i32, i32, i32, bool),
    pub cells: Vec<u32>,
}

/// Cells closed by known buildings and resources, reference-counted so overlapping
/// footprints stay closed until the last one goes.
#[derive(Debug, Clone)]
pub struct ObservedBlockers {
    records: BTreeMap<u32, Record>,
    counts: Vec<u16>,
    /// Cells whose closed state may have changed since navigation last looked.
    changes: Vec<u32>,
}

impl ObservedBlockers {
    pub fn new(cells: usize) -> Self {
        Self {
            records: BTreeMap::new(),
            counts: vec![0; cells],
            changes: Vec::new(),
        }
    }

    pub fn blocked(&self, cell: u32) -> bool {
        self.counts.get(cell as usize).is_some_and(|&c| c > 0)
    }

    /// Whether a known blocker with this signature is recorded for `id`.
    pub fn current(&self, id: u32, signature: &(u16, i32, i32, i32, bool)) -> bool {
        self.records
            .get(&id)
            .is_some_and(|r| &r.signature == signature)
    }

    /// Brings the index up to date with the blockers known now (`present`, every one of
    /// them, in id order; `changed` the ones whose signature differs from the record).
    pub fn update(&mut self, present: &[u32], changed: Vec<Blocker>) {
        for b in changed {
            if let Some(old) = self.records.remove(&b.id) {
                self.lift(&old.cells);
            }
            for &cell in &b.cells {
                let count = &mut self.counts[cell as usize];
                if *count == 0 {
                    self.changes.push(cell);
                }
                *count += 1;
            }
            self.records.insert(
                b.id,
                Record {
                    signature: b.signature,
                    cells: b.cells,
                },
            );
        }
        if self.records.len() != present.len() {
            let gone: Vec<u32> = self
                .records
                .keys()
                .copied()
                .filter(|id| present.binary_search(id).is_err())
                .collect();
            for id in gone {
                let record = self.records.remove(&id).expect("record");
                self.lift(&record.cells);
            }
        }
    }

    fn lift(&mut self, cells: &[u32]) {
        for &cell in cells {
            let count = &mut self.counts[cell as usize];
            *count -= 1;
            if *count == 0 {
                self.changes.push(cell);
            }
        }
    }

    fn take_changes(&mut self) -> Vec<u32> {
        let mut changes = std::mem::take(&mut self.changes);
        changes.sort_unstable();
        changes.dedup();
        changes
    }
}

/// A body the AI asks about: a unit's position and size.
#[derive(Debug, Clone, Copy)]
pub struct Mover {
    pub at: Fixed,
    /// Cells.
    pub radius: f64,
    pub height: f64,
    pub air: bool,
}

impl Mover {
    pub fn point(&self) -> Pt {
        Pt {
            x: f64::from(self.at.x) / 1000.0,
            y: f64::from(self.at.y) / 1000.0,
            deck: self.at.deck,
        }
    }
}

/// A reachability question: from (milli x, y, deck) to (milli x, y, deck).
type Question = (i32, i32, Option<u16>, i32, i32, Option<u16>);

/// Navigation for one body size.
struct Profile {
    radius: i32,
    radius_cells: f64,
    height: f64,
    air: bool,
    pattern: Vec<(i32, i32)>,
    adjacent: AdjacentSweep,
    grid: GridNavigation,
    mesh: Option<GroundMesh>,
    /// Reachability answers for the current knowledge, bounded.
    answers: BTreeMap<Question, bool>,
}

struct Ground<'a> {
    walk: &'a [bool],
    heights: &'a [i16],
}

impl Ground<'_> {
    fn step(&self, a: u32, b: u32) -> bool {
        self.walk[b as usize]
            && (i32::from(self.heights[a as usize]) - i32::from(self.heights[b as usize])).abs()
                <= MAX_GROUND_STEP_CM
    }
}

impl Topology for Ground<'_> {
    fn count(&self) -> u32 {
        self.walk.len() as u32
    }
    fn cell(&self, node: u32) -> u32 {
        node
    }
    fn walkable(&self, node: u32) -> bool {
        self.walk[node as usize]
    }
    fn neighbors(&self, _: u32, out: &mut Vec<u32>) {
        out.clear();
    }
    fn step(&self, from: u32, to: u32) -> bool {
        Ground::step(self, from, to)
    }
    fn layered(&self) -> bool {
        false
    }
}

/// Per-player derived navigation over known terrain.
pub struct ObservedNavigation {
    size: u32,
    /// Land not covered by a known blocker, per ground cell.
    walk: Vec<bool>,
    clearance: Option<Clearance>,
    sectors: Option<Sectors>,
    profiles: Vec<((i32, u64, bool), Profile)>,
    scratch: Scratch,
    surface_scratch: SurfaceScratch,
    /// Reachability searches run (profiling).
    pub searches: u64,
    /// Compiled neighbour sweeps by radius (milli-cells), for checks outside the profiles.
    sweeps: BTreeMap<i32, AdjacentSweep>,
}

const MAX_ANSWERS: usize = 1024;

impl ObservedNavigation {
    pub fn new(geo: &Geography, blockers: &mut ObservedBlockers) -> Self {
        let size = geo.size;
        let n = (size * size) as usize;
        blockers.take_changes();
        let walk: Vec<bool> = (0..n as u32)
            .map(|c| geo.land(c) && !blockers.blocked(c))
            .collect();
        let layered = geo.layered();
        let clearance = (!layered).then(|| Clearance::new(size, |c| walk[c as usize]));
        let sectors = (!layered).then(|| {
            Sectors::new(
                size,
                &Ground {
                    walk: &walk,
                    heights: geo.terrain.heights(),
                },
            )
        });
        Self {
            size,
            walk,
            clearance,
            sectors,
            profiles: Vec::new(),
            scratch: Scratch::default(),
            surface_scratch: SurfaceScratch::default(),
            searches: 0,
            sweeps: BTreeMap::new(),
        }
    }

    /// Follows changes of the known blockers.
    pub fn sync(&mut self, geo: &Geography, blockers: &mut ObservedBlockers) {
        let candidates = blockers.take_changes();
        let mut changed = Vec::new();
        for cell in candidates {
            let open = geo.land(cell) && !blockers.blocked(cell);
            if self.walk[cell as usize] != open {
                self.walk[cell as usize] = open;
                changed.push(cell);
            }
        }
        if changed.is_empty() {
            return;
        }
        if let Some(c) = &mut self.clearance {
            for &cell in &changed {
                c.refresh(cell, self.walk[cell as usize]);
            }
        }
        if let Some(sectors) = &mut self.sectors {
            sectors.invalidate(
                &Ground {
                    walk: &self.walk,
                    heights: geo.terrain.heights(),
                },
                &changed,
            );
        }
        for (_, p) in &mut self.profiles {
            p.grid
                .invalidate(Some(&changed), p.radius_cells.ceil() as i32 + 1);
            if let Some(mesh) = &mut p.mesh {
                mesh.invalidate(&changed);
            }
            p.answers.clear();
        }
    }

    fn profile(&mut self, geo: &Geography, m: &Mover) -> usize {
        let radius = round_half_up(m.radius * 1000.0) as i32;
        let key = (
            radius,
            if geo.layered() { m.height.to_bits() } else { 0 },
            m.air,
        );
        if let Some(i) = self.profiles.iter().position(|(k, _)| *k == key) {
            return i;
        }
        self.profiles.push((
            key,
            Profile {
                radius,
                radius_cells: m.radius,
                height: m.height,
                air: m.air,
                pattern: sweep_pattern(radius),
                adjacent: AdjacentSweep::new(radius),
                grid: GridNavigation::new(self.size),
                mesh: None,
                answers: BTreeMap::new(),
            },
        ));
        self.profiles.len() - 1
    }

    /// The neighbour sweep of a body of `radius` milli-cells, compiled once.
    pub fn sweep(&mut self, radius: i32) -> &AdjacentSweep {
        self.sweeps
            .entry(radius)
            .or_insert_with(|| AdjacentSweep::new(radius))
    }

    /// The neighbour sweep [`ObservedNavigation::sweep`] compiled for `radius`.
    pub fn compiled_sweep(&self, radius: i32) -> Option<&AdjacentSweep> {
        self.sweeps.get(&radius)
    }

    /// Builds the sector graph and the meshes of these ground bodies ahead of play.
    pub fn prepare(&mut self, geo: &Geography, movers: &[Mover]) {
        if let Some(sectors) = &mut self.sectors {
            sectors.prepare(&Ground {
                walk: &self.walk,
                heights: geo.terrain.heights(),
            });
        }
        if geo.layered() {
            return;
        }
        for m in movers.iter().filter(|m| !m.air) {
            let i = self.profile(geo, m);
            self.mesh(geo, i);
        }
    }

    fn mesh(&mut self, geo: &Geography, i: usize) {
        let _span = tracing::trace_span!("ai mesh").entered();
        let input = MeshInput {
            size: self.size,
            walkable: &self.walk,
            heights: geo.terrain.heights(),
        };
        let p = &mut self.profiles[i].1;
        match &mut p.mesh {
            Some(mesh) => mesh.prepare(input),
            None => p.mesh = Some(GroundMesh::new(input, p.radius_cells)),
        }
    }

    /// Whether the body's sweep from `a` to `b` stays on known open ground.
    fn clear(&self, geo: &Geography, i: usize, a: Fixed, b: Fixed) -> bool {
        let p = &self.profiles[i].1;
        if p.air {
            return a.deck.is_none()
                && b.deck.is_none()
                && clear_sweep(a, b, self.size, &p.pattern, |_, _| true);
        }
        if let Some(s) = geo.terrain.surfaces() {
            let walk = &self.walk;
            let n = walk.len() as u32;
            let walkable = |id: u32| s.walkable(id) && (id >= n || walk[id as usize]);
            return clear_layered_sweep(s, a, b, p.height, p.radius, &p.pattern, &walkable, None);
        }
        let dx = (b.x - a.x).abs();
        let dy = (b.y - a.y).abs();
        if (p.radius >= 500 || dx.max(dy) >= 16_000)
            && self
                .clearance
                .as_ref()
                .is_some_and(|c| c.proves_clear(geo.terrain.steep_edges(), a, b, p.radius))
        {
            return true;
        }
        let ground = Ground {
            walk: &self.walk,
            heights: geo.terrain.heights(),
        };
        clear_sweep(a, b, self.size, &p.pattern, |x, y| ground.step(x, y))
    }

    /// Whether the body fits standing at `p`.
    pub fn fits(&mut self, geo: &Geography, p: Pt, m: &Mover) -> bool {
        let i = self.profile(geo, m);
        self.clear(geo, i, fixed(p), fixed(p))
    }

    /// Whether the mover's centre can be at `a` and sweep to `b` (an exact check).
    pub fn sweeps(&mut self, geo: &Geography, a: Fixed, b: Fixed, m: &Mover) -> bool {
        let i = self.profile(geo, m);
        self.clear(geo, i, a, b)
    }

    /// A conservative proof that a ground sweep of `radius` milli-cells is clear of known
    /// terrain; `false` is inconclusive.
    pub fn proves_ground_sweep(&self, geo: &Geography, a: Fixed, b: Fixed, radius: i32) -> bool {
        self.clearance
            .as_ref()
            .is_some_and(|c| c.proves_clear(geo.terrain.steep_edges(), a, b, radius))
    }

    /// Whether the mover can walk to `to` over known terrain.
    pub fn reachable(&mut self, geo: &Geography, from: &Mover, to: Pt) -> bool {
        let i = self.profile(geo, from);
        let (tx, ty) = to.milli();
        let key = (
            from.at.x,
            from.at.y,
            from.at.deck.map(|d| d.0),
            tx,
            ty,
            to.deck.map(|d| d.0),
        );
        if let Some(&answer) = self.profiles[i].1.answers.get(&key) {
            return answer;
        }
        let result = self.find_reachable(geo, i, from, to);
        let answers = &mut self.profiles[i].1.answers;
        if answers.len() >= MAX_ANSWERS {
            answers.clear();
        }
        answers.insert(key, result);
        result
    }

    /// Whether the body can step off the spot it stands on: it fits there and sweeps to
    /// its own cell centre or a neighbouring one, where every route over the cell grid
    /// begins. A body squeezed across a cliff edge or against a blocker can do neither and
    /// reaches nothing, wherever it is sent.
    pub fn can_leave(&mut self, geo: &Geography, m: &Mover) -> bool {
        if m.air {
            return true;
        }
        let i = self.profile(geo, m);
        (geo.layered() || self.clear(geo, i, m.at, m.at)) && self.join(geo, i, m).is_some()
    }

    /// The cell the body's route over the grid starts from: its own cell's centre on
    /// layered ground, else the first of its cell and the eight around it that it can
    /// sweep to.
    fn join(&self, geo: &Geography, i: usize, m: &Mover) -> Option<(i64, i64)> {
        let origin = m.point().rounded();
        let (ox, oy) = (origin.x as i64, origin.y as i64);
        if geo.terrain.surfaces().is_some() {
            return self.clear(geo, i, m.at, fixed(origin)).then_some((ox, oy));
        }
        std::iter::once((ox, oy))
            .chain(
                (-1..=1)
                    .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                    .filter(|&d| d != (0, 0))
                    .map(|(dx, dy)| (ox + dx, oy + dy)),
            )
            .find(|&(x, y)| self.clear(geo, i, m.at, Fixed::cell(x as i32, y as i32, None)))
    }

    fn find_reachable(&mut self, geo: &Geography, i: usize, from: &Mover, to: Pt) -> bool {
        let _span = tracing::trace_span!("ai reach search").entered();
        self.searches += 1;
        let a = from.at;
        let b = fixed(to);
        if !self.clear(geo, i, b, b) {
            return false;
        }
        if self.clear(geo, i, a, b) {
            return true;
        }
        if from.air {
            return false;
        }
        let origin = from.point().rounded();
        if let Some(s) = geo.terrain.surfaces() {
            if self.join(geo, i, from).is_none() {
                return false;
            }
            let (Some(start), Some(goal)) = (geo.node(origin), geo.node(to)) else {
                return false;
            };
            let clearance = round_half_up(from.height * 100.0) as i32;
            let walk = &self.walk;
            let n = walk.len() as u32;
            let mut scratch = std::mem::take(&mut self.surface_scratch);
            let point = |id: u32| {
                let node = s.node(id).expect("node");
                Fixed::cell(node.x as i32, node.y as i32, node.deck)
            };
            let route = crate::navigation::surfaces::path(
                s,
                &mut scratch,
                start,
                goal,
                |node| {
                    (node.id < n && !walk[node.id as usize]) || !s.walkable_with(node.id, clearance)
                },
                i64::MAX,
                |x, y| self.clear(geo, i, point(x), point(y)),
            );
            self.surface_scratch = scratch;
            return route.is_some();
        }
        let size = self.size as i64;
        let Some((jx, jy)) = self.join(geo, i, from) else {
            return false;
        };
        if jx < 0 || jy < 0 || jx >= size || jy >= size {
            return false;
        }
        let start = (jy * size + jx) as u32;
        let (gx, gy) = (to.x as i64, to.y as i64);
        if gx < 0 || gy < 0 || gx >= size || gy >= size {
            return false;
        }
        let goal = (gy * size + gx) as u32;
        if self.profiles[i].1.grid.known_unreachable(start, goal) {
            return false;
        }
        self.mesh(geo, i);
        let coarse = self.profiles[i]
            .1
            .mesh
            .as_mut()
            .expect("mesh")
            .path((jx as f64, jy as f64), (to.x, to.y));
        if let Some(path) = coarse {
            let mut anchor = Fixed::cell(jx as i32, jy as i32, None);
            let mut valid = true;
            for &(x, y) in &path[1..] {
                let next = Fixed::new(
                    round_half_up(x * 1000.0) as i32,
                    round_half_up(y * 1000.0) as i32,
                );
                if !self.clear(geo, i, anchor, next) {
                    valid = false;
                    break;
                }
                anchor = next;
            }
            if valid {
                return true;
            }
            let points: Vec<Fixed> = path
                .iter()
                .map(|&(x, y)| {
                    Fixed::new(
                        round_half_up(x * 1000.0) as i32,
                        round_half_up(y * 1000.0) as i32,
                    )
                })
                .collect();
            let ground = Ground {
                walk: &self.walk,
                heights: geo.terrain.heights(),
            };
            let p = &self.profiles[i].1;
            let step = |x: u32, y: u32| p.adjacent.clear(self.size, x, y, |u, v| ground.step(u, v));
            if snapped_mesh_grid_path(&points, self.size, step).is_some() {
                return true;
            }
        }
        let ground = Ground {
            walk: &self.walk,
            heights: geo.terrain.heights(),
        };
        let sectors = self.sectors.as_mut().expect("ground sectors");
        let corridor = match sectors.corridor(&ground, start, goal) {
            Err(()) => return false,
            Ok(c) => c,
        };
        let size = self.size;
        let mut scratch = std::mem::take(&mut self.scratch);
        let (_, p) = &mut self.profiles[i];
        let adjacent = &p.adjacent;
        let steep = geo.terrain.steep_edges();
        let planes = self.clearance.as_ref().map(|c| Planes {
            closed: c.closed(),
            steep: steep.any(),
            words: steep.words_per_row(),
        });
        let mut step = |x: u32, y: u32| match planes {
            Some(planes) => adjacent.clear_planes(size, x, y, planes, |u, v| ground.step(u, v)),
            None => adjacent.clear(size, x, y, |u, v| ground.step(u, v)),
        };
        let route = p.grid.path(
            &mut scratch,
            &mut step,
            |x, y| sectors.connected(&ground, x, y),
            start,
            goal,
            Search {
                corridor: corridor.as_deref(),
                probe_limit: 512,
                ..Search::default()
            },
        );
        // Only whether a route exists matters: the unrestricted retry floods.
        let mut found = route.is_some();
        if !found && corridor.is_some() {
            found = p.grid.reaches(
                &mut scratch,
                &mut step,
                |x, y| sectors.connected(&ground, x, y),
                start,
                goal,
                Search {
                    probe_limit: 512,
                    remember_origin: true,
                    ..Search::default()
                },
            );
        }
        self.scratch = scratch;
        found
    }
}

/// A point as fixed-point milli-cells.
pub fn fixed(p: Pt) -> Fixed {
    let (x, y) = p.milli();
    Fixed::on(x, y, p.deck)
}
