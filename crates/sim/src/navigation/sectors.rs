//! 16×16-cell sector graph (old `SectorNavigation`, `src/sim/game/sectorNavigation.ts`).
//!
//! Each connected piece of walkable ground inside a sector is a region; regions link across
//! sector borders. Connectivity answers "can this cell reach that one at all" in O(1), and
//! a region-level A* yields the corridor of sectors that long grid searches are confined
//! to. Only sectors touched by a static change are relabelled.

use super::grid::{DIRECTIONS, can_traverse};
use std::collections::BTreeSet;

pub const SECTOR_SIZE: u32 = 16;

/// The walk graph sectors are built over.
pub trait Topology {
    /// Node count (cells, plus deck nodes on layered maps).
    fn count(&self) -> u32;
    /// Ground cell of a node.
    fn cell(&self, node: u32) -> u32;
    /// Whether a body may stand on the node.
    fn walkable(&self, node: u32) -> bool;
    /// For layered maps: the node's neighbours. Ground maps use the cell grid and `step`.
    fn neighbors(&self, node: u32, out: &mut Vec<u32>);
    /// For ground maps: a single-cell step.
    fn step(&self, from: u32, to: u32) -> bool;
    fn layered(&self) -> bool;
}

#[derive(Debug, Clone, Default)]
struct Region {
    links: BTreeSet<u32>,
    network: u32,
}

#[derive(Debug, Clone)]
pub struct Sectors {
    size: u32,
    width: u32,
    labels: Vec<i32>,
    stride: u32,
    /// Regions of each sector; a region's id is `sector · stride + index`.
    regions: Vec<Vec<Region>>,
    /// Nodes of each sector in id order (layered maps only).
    nodes: Option<Vec<Vec<u32>>>,
    dirty: BTreeSet<u32>,
    /// Region-level plans by (source, target), most recent last.
    plans: Vec<(i32, i32, Option<Vec<u32>>)>,
    /// Regions expanded by the latest corridor search.
    pub expanded: u32,
}

impl Sectors {
    pub fn new(size: u32, topology: &impl Topology) -> Self {
        let width = size.div_ceil(SECTOR_SIZE);
        let count = topology.count();
        let mut this = Self {
            size,
            width,
            labels: vec![-1; count as usize],
            stride: SECTOR_SIZE * SECTOR_SIZE,
            regions: vec![Vec::new(); (width * width) as usize],
            nodes: None,
            dirty: (0..width * width).collect(),
            plans: Vec::new(),
            expanded: 0,
        };
        if topology.layered() {
            let mut nodes = vec![Vec::new(); (width * width) as usize];
            for id in 0..count {
                nodes[this.sector_of_cell(topology.cell(id)) as usize].push(id);
            }
            this.stride = nodes.iter().map(Vec::len).max().unwrap_or(0) as u32 + 1;
            this.nodes = Some(nodes);
        }
        this.prepare(topology);
        this
    }

    fn sector_of_cell(&self, cell: u32) -> u32 {
        (cell / self.size / SECTOR_SIZE) * self.width + (cell % self.size) / SECTOR_SIZE
    }

    /// The sector of a node.
    pub fn sector(&self, topology: &impl Topology, node: u32) -> u32 {
        self.sector_of_cell(topology.cell(node))
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    /// Marks the sectors of changed nodes for relabelling.
    pub fn invalidate(&mut self, topology: &impl Topology, nodes: &[u32]) {
        for &node in nodes {
            if (node as usize) < self.labels.len() {
                self.dirty.insert(self.sector(topology, node));
            }
        }
    }

    fn bounds(&self, sector: u32) -> (u32, u32, u32, u32) {
        let x = (sector % self.width) * SECTOR_SIZE;
        let y = (sector / self.width) * SECTOR_SIZE;
        (
            x,
            y,
            self.size.min(x + SECTOR_SIZE),
            self.size.min(y + SECTOR_SIZE),
        )
    }

    fn nodes_in(&self, sector: u32) -> Vec<u32> {
        if let Some(nodes) = &self.nodes {
            return nodes[sector as usize].clone();
        }
        let (x0, y0, x1, y1) = self.bounds(sector);
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| y * self.size + x))
            .collect()
    }

    fn region(&self, id: i32) -> &Region {
        let id = id as u32;
        &self.regions[(id / self.stride) as usize][(id % self.stride) as usize]
    }

    fn region_mut(&mut self, id: i32) -> &mut Region {
        let id = id as u32;
        &mut self.regions[(id / self.stride) as usize][(id % self.stride) as usize]
    }

    /// Relabels dirty sectors and relinks their borders.
    pub fn prepare(&mut self, topology: &impl Topology) {
        if self.dirty.is_empty() {
            return;
        }
        self.plans.clear();
        let dirty = std::mem::take(&mut self.dirty);
        let mut boundary = BTreeSet::new();
        for &sector in &dirty {
            let (sx, sy) = ((sector % self.width) as i32, (sector / self.width) as i32);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (x, y) = (sx + dx, sy + dy);
                    if x >= 0 && y >= 0 && x < self.width as i32 && y < self.width as i32 {
                        boundary.insert(y as u32 * self.width + x as u32);
                    }
                }
            }
        }
        let mut out = Vec::new();
        let mut queue = Vec::new();
        for &sector in &dirty {
            let nodes = self.nodes_in(sector);
            for &node in &nodes {
                self.labels[node as usize] = -1;
            }
            let (x0, y0, x1, y1) = self.bounds(sector);
            let mut regions = Vec::new();
            for &start in &nodes {
                if self.labels[start as usize] != -1 || !topology.walkable(start) {
                    continue;
                }
                let id = (sector * self.stride + regions.len() as u32) as i32;
                regions.push(Region::default());
                self.labels[start as usize] = id;
                queue.clear();
                queue.push(start);
                let mut at = 0;
                while at < queue.len() {
                    let a = queue[at];
                    at += 1;
                    if topology.layered() {
                        topology.neighbors(a, &mut out);
                        for &next in &out {
                            if self.sector(topology, next) == sector
                                && self.labels[next as usize] == -1
                            {
                                self.labels[next as usize] = id;
                                queue.push(next);
                            }
                        }
                        continue;
                    }
                    let (ax, ay) = ((a % self.size) as i32, (a / self.size) as i32);
                    for &(dx, dy) in &DIRECTIONS[..4] {
                        let (nx, ny) = (ax + dx, ay + dy);
                        if nx < x0 as i32 || nx >= x1 as i32 || ny < y0 as i32 || ny >= y1 as i32 {
                            continue;
                        }
                        let next = ny as u32 * self.size + nx as u32;
                        if self.labels[next as usize] != -1 || !topology.step(a, next) {
                            continue;
                        }
                        self.labels[next as usize] = id;
                        queue.push(next);
                    }
                }
            }
            self.regions[sector as usize] = regions;
        }
        for &sector in &boundary {
            for region in &mut self.regions[sector as usize] {
                region.links.clear();
            }
        }
        for &sector in &boundary {
            let (x0, y0, x1, y1) = self.bounds(sector);
            let on_edge = |cell: u32| {
                let (x, y) = (cell % self.size, cell / self.size);
                x == x0 || x == x1 - 1 || y == y0 || y == y1 - 1
            };
            let nodes: Vec<u32> = self
                .nodes_in(sector)
                .into_iter()
                .filter(|&n| on_edge(topology.cell(n)))
                .collect();
            for from in nodes {
                let id = self.labels[from as usize];
                if id < 0 {
                    continue;
                }
                let mut links = Vec::new();
                if topology.layered() {
                    topology.neighbors(from, &mut out);
                    for &to in &out {
                        if self.sector(topology, to) != sector && self.labels[to as usize] >= 0 {
                            links.push(self.labels[to as usize]);
                        }
                    }
                } else {
                    let (x, y) = ((from % self.size) as i32, (from / self.size) as i32);
                    for (dx, dy) in DIRECTIONS {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= self.size as i32 || ny >= self.size as i32 {
                            continue;
                        }
                        let to = ny as u32 * self.size + nx as u32;
                        if self.sector_of_cell(to) == sector {
                            continue;
                        }
                        let other = self.labels[to as usize];
                        if other >= 0
                            && can_traverse(self.size, from, to, |a, b| topology.step(a, b))
                        {
                            links.push(other);
                        }
                    }
                }
                let region = self.region_mut(id);
                region.links.extend(links.into_iter().map(|l| l as u32));
            }
        }
        // Connected networks over all regions.
        for sector in &mut self.regions {
            for region in sector {
                region.network = 0;
            }
        }
        let mut network = 0;
        for sector in 0..self.regions.len() {
            for index in 0..self.regions[sector].len() {
                if self.regions[sector][index].network != 0 {
                    continue;
                }
                network += 1;
                let start = (sector as u32 * self.stride + index as u32) as i32;
                self.region_mut(start).network = network;
                let mut queue = vec![start];
                let mut at = 0;
                while at < queue.len() {
                    let links: Vec<i32> = self
                        .region(queue[at])
                        .links
                        .iter()
                        .map(|&l| l as i32)
                        .collect();
                    at += 1;
                    for next in links {
                        if self.region(next).network == 0 {
                            self.region_mut(next).network = network;
                            queue.push(next);
                        }
                    }
                }
            }
        }
    }

    fn network(&self, label: i32) -> Option<u32> {
        (label >= 0).then(|| self.region(label).network)
    }

    /// Whether `goal` can be reached from `start` over static terrain. A start covered by
    /// a new obstacle may still leave through a neighbour.
    pub fn connected(&mut self, topology: &impl Topology, start: u32, goal: u32) -> bool {
        if start as usize >= self.labels.len() || goal as usize >= self.labels.len() {
            return false;
        }
        self.prepare(topology);
        let Some(target) = self.network(self.labels[goal as usize]) else {
            return false;
        };
        if let Some(origin) = self.network(self.labels[start as usize]) {
            return origin == target;
        }
        if topology.layered() {
            let mut out = Vec::new();
            topology.neighbors(start, &mut out);
            return out
                .iter()
                .any(|&n| self.network(self.labels[n as usize]) == Some(target));
        }
        let (x, y) = ((start % self.size) as i32, (start / self.size) as i32);
        DIRECTIONS[..4].iter().any(|&(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= self.size as i32 || ny >= self.size as i32 {
                return false;
            }
            let next = ny as u32 * self.size + nx as u32;
            topology.step(start, next) && self.network(self.labels[next as usize]) == Some(target)
        })
    }

    /// The sectors a long search may use, row-major: the region-level route plus one
    /// sector around it. `Err(())` when disconnected; `Ok(None)` asks for an unrestricted
    /// local search (the start is not on a region).
    #[allow(clippy::result_unit_err)]
    pub fn corridor(
        &mut self,
        topology: &impl Topology,
        start: u32,
        goal: u32,
    ) -> Result<Option<Vec<bool>>, ()> {
        self.prepare(topology);
        self.expanded = 0;
        if !self.connected(topology, start, goal) {
            return Err(());
        }
        let (source, target) = (self.labels[start as usize], self.labels[goal as usize]);
        if source < 0 {
            return Ok(None);
        }
        let plan = match self.plans.iter().find(|p| p.0 == source && p.1 == target) {
            Some(p) => p.2.clone(),
            None => {
                let plan = self.plan(source, target);
                if self.plans.len() >= 128 {
                    self.plans.remove(0);
                }
                self.plans.push((source, target, plan.clone()));
                plan
            }
        };
        let Some(plan) = plan else {
            return Err(());
        };
        let width = self.width as i32;
        let mut allowed = vec![false; (self.width * self.width) as usize];
        for sector in plan {
            let (x, y) = ((sector % self.width) as i32, (sector / self.width) as i32);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < width && ny < width {
                        allowed[(ny * width + nx) as usize] = true;
                    }
                }
            }
        }
        Ok(Some(allowed))
    }

    /// Region-level A*: sectors from target back to source.
    fn plan(&mut self, source: i32, target: i32) -> Option<Vec<u32>> {
        use std::{
            cmp::Reverse,
            collections::{BTreeMap, BinaryHeap},
        };
        let stride = self.stride;
        let width = self.width;
        let sector_of = |id: i32| id as u32 / stride;
        let goal = sector_of(target);
        let (gx, gy) = ((goal % width) as i64, (goal / width) as i64);
        let heuristic = |id: i32| {
            let s = sector_of(id);
            let (dx, dy) = (
                ((s % width) as i64 - gx).abs(),
                ((s / width) as i64 - gy).abs(),
            );
            dx.max(dy) * 1000 + dx.min(dy) * 414
        };
        let mut costs: BTreeMap<i32, i64> = BTreeMap::new();
        let mut previous: BTreeMap<i32, i32> = BTreeMap::new();
        let mut closed: BTreeSet<i32> = BTreeSet::new();
        let mut heap = BinaryHeap::new();
        costs.insert(source, 0);
        let h = heuristic(source);
        heap.push(Reverse((h, h, source)));
        while let Some(Reverse((f, h, id))) = heap.pop() {
            let g = f - h;
            if closed.contains(&id) || costs.get(&id) != Some(&g) {
                continue;
            }
            self.expanded += 1;
            if id == target {
                let mut path = vec![sector_of(target)];
                let mut at = target;
                while at != source {
                    at = previous[&at];
                    path.push(sector_of(at));
                }
                return Some(path);
            }
            closed.insert(id);
            let sector = sector_of(id);
            for &next in &self.region(id).links {
                let next = next as i32;
                if closed.contains(&next) {
                    continue;
                }
                let other = sector_of(next);
                let diagonal = sector % width != other % width && sector / width != other / width;
                let g2 = g + if diagonal { 1414 } else { 1000 };
                if costs.get(&next).is_some_and(|&c| g2 >= c) {
                    continue;
                }
                costs.insert(next, g2);
                previous.insert(next, id);
                let h2 = heuristic(next);
                heap.push(Reverse((g2 + h2, h2, next)));
            }
        }
        None
    }

    /// The region label of a node (−1: not walkable).
    pub fn label(&self, node: u32) -> i32 {
        self.labels[node as usize]
    }
}
