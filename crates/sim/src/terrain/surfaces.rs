//! Layered walk surfaces: the ground grid plus sparse nodes on bridge decks (old
//! `WalkSurfaces`, `src/shared/map/walkSurfaces.ts`, without its path search, which the
//! movement step adds).
//!
//! Node ids `0..size²` are the ground cells; deck nodes follow, ordered by deck (stamp
//! id) and then row-major cell. Adding a bridge never replaces the floor beneath it.
//! Static regions (same-level areas joined only by authored ramps) are built once at load
//! so reachability is a table lookup.

use super::{
    bridges::BridgeSurface,
    round_half_up,
    tactical::{
        MAX_GROUND_STEP_CM, SIGHT_HEIGHT_CM, TacticalTerrain, TerrainPoint, UPHILL_VISIBILITY_CM,
    },
};
use crate::math;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Neighbour order: cardinals first, then diagonals.
const DIRECTIONS: [(i64, i64); 8] = [
    (0, -1),
    (-1, 0),
    (1, 0),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];
/// Ramp ends connect within this many cells of a deck's end.
const RAMP_REACH: f64 = 1.5;

/// Index of a deck in [`WalkSurfaces::decks`] (sorted by stamp id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeckId(pub u16);

/// A walk-surface node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceNode {
    pub id: u32,
    /// Ground cell index `y * size + x`.
    pub cell: u32,
    pub x: u32,
    pub y: u32,
    /// Floor height in centimetres.
    pub height: i32,
    /// Navigation level: 0 for the ground, the deck's level otherwise.
    pub level: u32,
    pub deck: Option<DeckId>,
}

/// A position (in cells) on the ground or on a deck, with an elevation in world units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SurfacePoint {
    pub x: f64,
    pub y: f64,
    pub deck: Option<DeckId>,
    pub elevation: f64,
}

impl SurfacePoint {
    fn terrain(self) -> TerrainPoint {
        TerrainPoint {
            x: self.x,
            y: self.y,
            elevation: self.elevation,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DeckNode {
    cell: u32,
    height: i32,
    deck: DeckId,
}

#[derive(Debug, Clone)]
pub struct WalkSurfaces {
    size: u32,
    ground: Arc<TacticalTerrain>,
    ground_walkable: Vec<bool>,
    /// Headroom a body needs under a deck, in centimetres.
    clearance_cm: i32,
    decks: Vec<BridgeSurface>,
    /// Per deck: thickness in centimetres and the bounding square (lo x, hi x, lo y, hi y).
    thickness_cm: Vec<i32>,
    bounds: Vec<[f64; 4]>,
    deck_nodes: Vec<DeckNode>,
    /// Deck nodes above each ground cell: `column_nodes[column_start[c]..column_start[c+1]]`.
    column_start: Vec<u32>,
    column_nodes: Vec<u32>,
    /// Static region of each node (0: none) and connected component of each region.
    regions: Vec<u32>,
    components: Vec<u32>,
    links: usize,
}

impl WalkSurfaces {
    /// Builds the layered grid over the ground (`ground` heights, `ground_walkable` cells)
    /// for `surfaces`. Deck ids must be unique, levels 1–31, dimensions positive.
    pub fn new(
        ground: Arc<TacticalTerrain>,
        ground_walkable: Vec<bool>,
        mut surfaces: Vec<BridgeSurface>,
        clearance_cm: i32,
    ) -> Result<Self, String> {
        let size = ground.size();
        let cells = (size as usize).pow(2);
        assert_eq!(ground_walkable.len(), cells, "one walkable flag per cell");
        surfaces.sort_by(|a, b| a.id.cmp(&b.id));
        if surfaces.len() > usize::from(u16::MAX) {
            return Err("Too many walk surfaces".into());
        }
        let mut deck_nodes = Vec::new();
        let mut bounds = Vec::with_capacity(surfaces.len());
        for (index, deck) in surfaces.iter().enumerate() {
            let duplicate = index > 0 && surfaces[index - 1].id == deck.id;
            if deck.id.is_empty() || duplicate || !(1..=31).contains(&deck.level) {
                return Err("Walk surfaces require unique stamp IDs".into());
            }
            let numbers = [
                deck.x,
                deck.z,
                deck.base,
                deck.c,
                deck.s,
                deck.width,
                deck.depth,
                deck.height,
                deck.arch,
                deck.thickness,
            ];
            if !numbers.iter().all(|v| v.is_finite())
                || deck.width <= 0.0
                || deck.depth <= 0.0
                || deck.thickness <= 0.0
            {
                return Err("Invalid walk surface".into());
            }
            let radius = deck.reach();
            bounds.push([
                deck.x - radius,
                deck.x + radius,
                deck.z - radius,
                deck.z + radius,
            ]);
            let last = f64::from(size) - 1.0;
            let (y0, y1) = (
                (deck.z - radius).floor().max(0.0),
                (deck.z + radius).ceil().min(last),
            );
            let (x0, x1) = (
                (deck.x - radius).floor().max(0.0),
                (deck.x + radius).ceil().min(last),
            );
            if !(y0 <= y1 && x0 <= x1) {
                continue;
            }
            for y in y0 as u32..=y1 as u32 {
                for x in x0 as u32..=x1 as u32 {
                    if let Some(h) = deck.height_at(f64::from(x), f64::from(y)) {
                        deck_nodes.push(DeckNode {
                            cell: y * size + x,
                            height: round_half_up(h * 100.0) as i32,
                            deck: DeckId(index as u16),
                        });
                    }
                }
            }
        }
        // Columns in node order, so each column lists its decks by stamp id.
        let mut column_start = vec![0u32; cells + 1];
        for node in &deck_nodes {
            column_start[node.cell as usize + 1] += 1;
        }
        for i in 0..cells {
            column_start[i + 1] += column_start[i];
        }
        let mut fill = column_start.clone();
        let mut column_nodes = vec![0u32; deck_nodes.len()];
        for (i, node) in deck_nodes.iter().enumerate() {
            let slot = &mut fill[node.cell as usize];
            column_nodes[*slot as usize] = (cells + i) as u32;
            *slot += 1;
        }
        let thickness_cm = surfaces
            .iter()
            .map(|d| round_half_up(d.thickness * 100.0) as i32)
            .collect();
        let mut surfaces = Self {
            size,
            ground,
            ground_walkable,
            clearance_cm,
            decks: surfaces,
            thickness_cm,
            bounds,
            deck_nodes,
            column_start,
            column_nodes,
            regions: Vec::new(),
            components: Vec::new(),
            links: 0,
        };
        surfaces.build_regions();
        Ok(surfaces)
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    fn ground_count(&self) -> u32 {
        self.size * self.size
    }

    /// Ground cells plus deck nodes.
    pub fn node_count(&self) -> u32 {
        self.ground_count() + self.deck_nodes.len() as u32
    }

    /// Decks sorted by stamp id; [`DeckId`] indexes this.
    pub fn decks(&self) -> &[BridgeSurface] {
        &self.decks
    }

    pub fn deck(&self, id: DeckId) -> &BridgeSurface {
        &self.decks[usize::from(id.0)]
    }

    /// The deck placed by the stamp `name`.
    pub fn deck_id(&self, name: &str) -> Option<DeckId> {
        self.decks
            .binary_search_by(|d| d.id.as_str().cmp(name))
            .ok()
            .map(|i| DeckId(i as u16))
    }

    pub fn node(&self, id: u32) -> Option<SurfaceNode> {
        let ground = self.ground_count();
        if id < ground {
            let height = i32::from(self.ground.heights()[id as usize]);
            return Some(SurfaceNode {
                id,
                cell: id,
                x: id % self.size,
                y: id / self.size,
                height,
                level: 0,
                deck: None,
            });
        }
        let node = self.deck_nodes.get((id - ground) as usize)?;
        Some(SurfaceNode {
            id,
            cell: node.cell,
            x: node.cell % self.size,
            y: node.cell / self.size,
            height: node.height,
            level: self.deck(node.deck).level,
            deck: Some(node.deck),
        })
    }

    /// Deck nodes above a ground cell, by stamp id.
    pub fn column(&self, cell: u32) -> &[u32] {
        let c = cell as usize;
        &self.column_nodes[self.column_start[c] as usize..self.column_start[c + 1] as usize]
    }

    /// Whether a deck spans the cell.
    pub fn has_deck(&self, cell: u32) -> bool {
        !self.column(cell).is_empty()
    }

    /// The node at a cell on the ground or on a deck.
    pub fn node_at(&self, x: i64, y: i64, deck: Option<DeckId>) -> Option<u32> {
        let size = i64::from(self.size);
        if x < 0 || y < 0 || x >= size || y >= size {
            return None;
        }
        let cell = (y * size + x) as u32;
        match deck {
            None => Some(cell),
            Some(deck) => self
                .column(cell)
                .iter()
                .copied()
                .find(|&id| self.deck_nodes[(id - self.ground_count()) as usize].deck == deck),
        }
    }

    /// Every node at a cell: the ground first, then the decks above it.
    pub fn at(&self, x: i64, y: i64) -> impl Iterator<Item = u32> + '_ {
        let cell = self.node_at(x, y, None);
        let column = cell.map_or(&[][..], |c| self.column(c));
        cell.into_iter().chain(column.iter().copied())
    }

    /// Whether a body with the default headroom can stand on the node.
    pub fn walkable(&self, id: u32) -> bool {
        self.walkable_with(id, self.clearance_cm)
    }

    /// Whether a body needing `clearance_cm` of headroom can stand on the node: the floor
    /// is open ground or a deck above the ground, and no deck overhead is too low.
    pub fn walkable_with(&self, id: u32, clearance_cm: i32) -> bool {
        let Some(n) = self.node(id) else {
            return false;
        };
        let cell = n.cell as usize;
        if (n.deck.is_none() && !self.ground_walkable[cell])
            || n.height < i32::from(self.ground.heights()[cell])
        {
            return false;
        }
        for &other in self.column(n.cell) {
            if other == id {
                continue;
            }
            let upper = &self.deck_nodes[(other - self.ground_count()) as usize];
            let bottom = upper.height - self.thickness_cm[usize::from(upper.deck.0)];
            // A coplanar seam is floor, not an obstruction.
            if upper.height > n.height && bottom < n.height + clearance_cm {
                return false;
            }
        }
        true
    }

    /// Whether `n` is on a ramp end of its deck that connects to `level`.
    fn connects(&self, n: &SurfaceNode, level: u32) -> bool {
        let Some(deck) = n.deck else {
            return false;
        };
        let b = self.deck(deck);
        let z = b.s * (f64::from(n.x) - b.x) + b.c * (f64::from(n.y) - b.z);
        (z <= -b.depth / 2.0 + RAMP_REACH + b.ramps[0] && b.start == Some(level))
            || (z >= b.depth / 2.0 - RAMP_REACH - b.ramps[1] && b.end == Some(level))
    }

    fn edge(
        &self,
        a: &SurfaceNode,
        b: &SurfaceNode,
        blocked: &impl Fn(&SurfaceNode) -> bool,
    ) -> bool {
        if !self.walkable(b.id) || blocked(b) || (a.height - b.height).abs() > MAX_GROUND_STEP_CM {
            return false;
        }
        // Rails never become ramps just because terrain happens to be near.
        a.level == b.level || self.connects(a, b.level) || self.connects(b, a.level)
    }

    /// A single step between adjacent nodes; a step to the same node checks it is standable.
    pub fn step(&self, from: u32, to: u32, blocked: impl Fn(&SurfaceNode) -> bool) -> bool {
        let (Some(a), Some(b)) = (self.node(from), self.node(to)) else {
            return false;
        };
        if from == to {
            return self.walkable(to) && !blocked(&b);
        }
        let dx = a.x.abs_diff(b.x);
        let dy = a.y.abs_diff(b.y);
        if dx > 1 || dy > 1 || (dx != 0 && dy != 0 && a.deck != b.deck) {
            return false;
        }
        self.edge(&a, &b, &blocked)
    }

    /// Nodes reachable in one step, in direction order. Changing layers takes a cardinal
    /// step; diagonals need both cardinal neighbours on the same surface.
    pub fn neighbors(&self, id: u32, blocked: impl Fn(&SurfaceNode) -> bool, out: &mut Vec<u32>) {
        out.clear();
        let Some(a) = self.node(id) else {
            return;
        };
        let size = i64::from(self.size);
        for (dx, dy) in DIRECTIONS {
            let (x, y) = (i64::from(a.x) + dx, i64::from(a.y) + dy);
            if x < 0 || y < 0 || x >= size || y >= size {
                continue;
            }
            for next in self.at(x, y) {
                let b = self.node(next).expect("node");
                if !self.edge(&a, &b, &blocked) {
                    continue;
                }
                if dx != 0 && dy != 0 {
                    if a.deck != b.deck {
                        continue;
                    }
                    let side =
                        |x: i64, y: i64| self.node_at(x, y, a.deck).and_then(|n| self.node(n));
                    let (Some(sx), Some(sy)) = (
                        side(i64::from(a.x) + dx, i64::from(a.y)),
                        side(i64::from(a.x), i64::from(a.y) + dy),
                    ) else {
                        continue;
                    };
                    if !(self.edge(&a, &sx, &blocked)
                        && self.edge(&a, &sy, &blocked)
                        && self.edge(&sx, &b, &blocked)
                        && self.edge(&sy, &b, &blocked))
                    {
                        continue;
                    }
                }
                out.push(next);
            }
        }
    }

    /// Labels same-level regions, then joins regions across authored ramps into
    /// components.
    fn build_regions(&mut self) {
        let count = self.node_count() as usize;
        let mut labels = vec![0u32; count];
        let mut queue = Vec::with_capacity(count);
        let mut next = Vec::with_capacity(16);
        let mut region_count = 0u32;
        for id in 0..count as u32 {
            if labels[id as usize] != 0 || !self.walkable(id) {
                continue;
            }
            region_count += 1;
            labels[id as usize] = region_count;
            queue.clear();
            queue.push(id);
            let mut head = 0;
            while head < queue.len() {
                let current = queue[head];
                head += 1;
                let level = self.node(current).expect("node").level;
                self.neighbors(current, |_| false, &mut next);
                for &n in &next {
                    if labels[n as usize] != 0 || self.node(n).expect("node").level != level {
                        continue;
                    }
                    labels[n as usize] = region_count;
                    queue.push(n);
                }
            }
        }
        // Only deck nodes can join different levels.
        let mut links: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
        for id in self.ground_count()..count as u32 {
            let from = labels[id as usize];
            if from == 0 {
                continue;
            }
            self.neighbors(id, |_| false, &mut next);
            for &n in &next {
                let to = labels[n as usize];
                if to != 0 && to != from {
                    links.entry(from).or_default().insert(to);
                    links.entry(to).or_default().insert(from);
                }
            }
        }
        let mut components = vec![0u32; region_count as usize + 1];
        let mut component = 0;
        for start in 1..=region_count {
            if components[start as usize] != 0 {
                continue;
            }
            component += 1;
            components[start as usize] = component;
            let mut open = vec![start];
            while let Some(region) = open.pop() {
                for &to in links.get(&region).into_iter().flatten() {
                    if components[to as usize] == 0 {
                        components[to as usize] = component;
                        open.push(to);
                    }
                }
            }
        }
        self.links = links.values().map(BTreeSet::len).sum::<usize>() / 2;
        self.regions = labels;
        self.components = components;
    }

    /// Whether `to` can be reached from `from` across static terrain and decks. An
    /// obstructed origin may still leave through a neighbour.
    pub fn connected(&self, from: u32, to: u32) -> bool {
        let Some(&target) = self.regions.get(to as usize) else {
            return false;
        };
        if target == 0 {
            return false;
        }
        let goal = self.components[target as usize];
        match self.regions.get(from as usize) {
            Some(&origin) if origin != 0 => self.components[origin as usize] == goal,
            Some(_) => {
                let mut next = Vec::new();
                self.neighbors(from, |_| false, &mut next);
                next.iter()
                    .any(|&id| self.components[self.regions[id as usize] as usize] == goal)
            }
            None => false,
        }
    }

    /// Static regions and the ramps joining them.
    pub fn topology(&self) -> (u32, usize) {
        (self.components.len() as u32 - 1, self.links)
    }

    /// Floor height in world units: the deck at the point, or the ground between cell
    /// centres.
    pub fn height(&self, p: SurfacePoint) -> Option<f64> {
        match p.deck {
            Some(deck) => self.deck(deck).height_at(p.x, p.y),
            None => Some(self.ground.height(p.x, p.y) / 100.0),
        }
    }

    fn crosses_surface(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        let (lo_x, hi_x) = (a.x.min(b.x), a.x.max(b.x));
        let (lo_y, hi_y) = (a.y.min(b.y), a.y.max(b.y));
        self.bounds
            .iter()
            .any(|r| hi_x >= r[0] && lo_x <= r[1] && hi_y >= r[2] && lo_y <= r[3])
    }

    fn ground_only(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        a.deck.is_none() && b.deck.is_none() && !self.crosses_surface(a, b)
    }

    /// Both the floor and solid deck slabs intercept a straight projectile.
    pub fn shot_clear(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        if self.ground_only(a, b) {
            return self.ground.shot_clear(a.terrain(), b.terrain());
        }
        let (Some(ah), Some(bh)) = (self.height(a), self.height(b)) else {
            return false;
        };
        let sight = f64::from(SIGHT_HEIGHT_CM) / 100.0;
        let from = ah + a.elevation + sight;
        let to = bh + b.elevation + sight;
        if self.intersects_deck(a, b, from, to) {
            return false;
        }
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let steps = (math::hypot(math::hypot(dx, dy), to - from) * 4.0)
            .ceil()
            .max(1.0);
        for i in 1..steps as i64 {
            let t = i as f64 / steps;
            let ground = self.ground.height(a.x + dx * t, a.y + dy * t) / 100.0;
            if ground > from + (to - from) * t {
                return false;
            }
        }
        true
    }

    fn intersects_deck(&self, a: SurfacePoint, b: SurfacePoint, from: f64, to: f64) -> bool {
        for d in &self.decks {
            let ax = d.c * (a.x - d.x) - d.s * (a.y - d.z);
            let az = d.s * (a.x - d.x) + d.c * (a.y - d.z);
            let dx = d.c * (b.x - a.x) - d.s * (b.y - a.y);
            let dz = d.s * (b.x - a.x) + d.c * (b.y - a.y);
            let (mut enter, mut exit) = (0.0f64, 1.0f64);
            for (p, v, half) in [(ax, dx, d.width / 2.0), (az, dz, d.depth / 2.0)] {
                if v.abs() < 1e-10 {
                    if p.abs() > half {
                        exit = -1.0;
                        break;
                    }
                    continue;
                }
                let t1 = (-half - p) / v;
                let t2 = (half - p) / v;
                enter = enter.max(t1.min(t2));
                exit = exit.min(t1.max(t2));
            }
            if enter > exit {
                continue;
            }
            let signed = |t: f64| {
                let top = if d.triangles.is_empty() {
                    d.arched(az + dz * t)
                } else {
                    d.height_at(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
                        .unwrap_or(f64::NEG_INFINITY)
                };
                from + (to - from) * t - top
            };
            let steps = ((exit - enter) * math::hypot(math::hypot(dx, dz), to - from) * 4.0)
                .ceil()
                .max(1.0);
            let mut previous = signed(enter);
            if previous <= 0.0 && previous >= -d.thickness {
                return true;
            }
            for i in 1..=steps as i64 {
                let current = signed(enter + (exit - enter) * i as f64 / steps);
                // Crossing the whole slab between two samples still counts.
                if previous.is_finite()
                    && current.is_finite()
                    && previous.min(current) <= 0.0
                    && previous.max(current) >= -d.thickness
                {
                    return true;
                }
                previous = current;
            }
        }
        false
    }

    /// Ground keeps the RTS height allowance; solid decks still hide other layers.
    pub fn visible(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        if self.ground_only(a, b) {
            return self.ground.visible(a.terrain(), b.terrain());
        }
        let (Some(ah), Some(bh)) = (self.height(a), self.height(b)) else {
            return false;
        };
        let (from, to) = (ah + a.elevation, bh + b.elevation);
        let sight = f64::from(SIGHT_HEIGHT_CM) / 100.0;
        to <= from + f64::from(UPHILL_VISIBILITY_CM) / 100.0
            && !self.intersects_deck(a, b, from + sight, to + sight)
    }

    pub fn melee_clear(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        if self.ground_only(a, b) {
            return self.ground.melee_clear(a.terrain(), b.terrain());
        }
        let (Some(ah), Some(bh)) = (self.height(a), self.height(b)) else {
            return false;
        };
        (ah - bh).abs() <= f64::from(MAX_GROUND_STEP_CM) / 100.0 && self.shot_clear(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 16;

    /// Flat ground with a river on rows 7–9.
    fn ground() -> (Arc<TacticalTerrain>, Vec<bool>) {
        let n = (SIZE * SIZE) as usize;
        let walkable = (0..n)
            .map(|i| !(7..=9).contains(&(i / SIZE as usize)))
            .collect();
        (Arc::new(TacticalTerrain::new(SIZE, vec![0; n])), walkable)
    }

    /// A deck along y centred on (8, 8): 3 wide, 10 long, ramps to the ground at both ends.
    fn deck(id: &str, base: f64) -> BridgeSurface {
        BridgeSurface {
            id: id.into(),
            level: 1,
            start: Some(0),
            end: Some(0),
            x: 8.0,
            z: 8.0,
            base,
            c: 1.0,
            s: 0.0,
            width: 3.0,
            depth: 10.0,
            height: 0.3,
            arch: 0.0,
            rise: 0.0,
            thickness: 0.3,
            ramps: [0.0, 0.0],
            triangles: Vec::new(),
        }
    }

    fn surfaces(decks: Vec<BridgeSurface>) -> WalkSurfaces {
        let (ground, walkable) = ground();
        WalkSurfaces::new(ground, walkable, decks, 100).unwrap()
    }

    fn cell(x: u32, y: u32) -> u32 {
        y * SIZE + x
    }

    #[test]
    fn decks_add_nodes_above_the_ground() {
        let s = surfaces(vec![deck("bridge", 0.0)]);
        // x 7–9, y 3–13.
        assert_eq!(s.node_count(), SIZE * SIZE + 33);
        let id = s.deck_id("bridge").unwrap();
        let on_deck = s.node_at(8, 8, Some(id)).unwrap();
        let node = s.node(on_deck).unwrap();
        assert_eq!((node.x, node.y, node.height, node.level), (8, 8, 30, 1));
        assert!(s.has_deck(cell(8, 8)) && !s.has_deck(cell(8, 2)));
        assert_eq!(s.at(8, 8).collect::<Vec<_>>(), [cell(8, 8), on_deck]);
        assert_eq!(s.node_at(8, 2, Some(id)), None);
        assert_eq!(s.deck_id("missing"), None);
    }

    #[test]
    fn low_decks_close_the_ground_beneath_them() {
        let s = surfaces(vec![deck("bridge", 0.0)]);
        assert!(!s.walkable(cell(8, 4)));
        assert!(s.walkable_with(cell(8, 4), 0));
        assert!(s.walkable(cell(8, 2)));
        let high = surfaces(vec![deck("bridge", 2.0)]);
        assert!(high.walkable(cell(8, 4)));
    }

    #[test]
    fn ramps_join_banks_and_rails_do_not() {
        let id = |s: &WalkSurfaces, x, y| s.node_at(x, y, s.deck_id("bridge")).unwrap();
        let s = surfaces(vec![deck("bridge", 0.0)]);
        let mut out = Vec::new();
        // The start ramp (y ≤ 4) steps down to the bank.
        s.neighbors(id(&s, 7, 3), |_| false, &mut out);
        assert!(out.contains(&cell(6, 3)) && out.contains(&cell(7, 2)));
        // Mid-deck rails do not, although the bank beside them is walkable.
        s.neighbors(id(&s, 7, 6), |_| false, &mut out);
        assert!(!out.contains(&cell(6, 6)));
        assert!(s.step(id(&s, 7, 3), cell(6, 3), |_| false));
        assert!(!s.step(id(&s, 7, 6), cell(6, 6), |_| false));
        // Changing layers diagonally is not a step.
        assert!(!s.step(id(&s, 7, 3), cell(6, 2), |_| false));
        assert!(!s.step(id(&s, 7, 3), cell(6, 3), |n| n.cell == cell(6, 3)));
    }

    #[test]
    fn regions_connect_through_the_bridge_only() {
        let bridged = surfaces(vec![deck("bridge", 0.0)]);
        let (north, south) = (cell(0, 0), cell(0, 15));
        assert!(bridged.connected(north, south));
        assert_eq!(bridged.topology(), (3, 2));
        let open = surfaces(Vec::new());
        assert!(!open.connected(north, south));
        assert!(!open.connected(north, cell(0, 8)));
        // An obstructed origin may leave through a neighbour.
        assert!(open.connected(cell(0, 9), north) || open.connected(cell(0, 9), south));
    }

    #[test]
    fn decks_stop_shots_that_cross_their_slab() {
        let s = surfaces(vec![deck("bridge", 2.0)]);
        let point = |y: f64, elevation: f64| SurfacePoint {
            x: 8.0,
            y,
            deck: None,
            elevation,
        };
        // Eyes at 1.2 pass under the slab (2.0–2.3); at 2.2 they hit it.
        assert!(s.shot_clear(point(1.0, 0.0), point(15.0, 0.0)));
        assert!(!s.shot_clear(point(1.0, 1.0), point(15.0, 1.0)));
        assert!(!s.visible(point(1.0, 1.0), point(15.0, 1.0)));
        // Far from the deck, the ground rules apply.
        let side = |y: f64| SurfacePoint {
            x: 1.0,
            ..point(y, 1.0)
        };
        assert!(s.shot_clear(side(1.0), side(15.0)));
    }

    #[test]
    fn deck_ids_are_unique() {
        let (ground, walkable) = ground();
        let result = WalkSurfaces::new(ground, walkable, vec![deck("a", 0.0), deck("a", 1.0)], 100);
        assert_eq!(
            result.err().as_deref(),
            Some("Walk surfaces require unique stamp IDs")
        );
    }
}
