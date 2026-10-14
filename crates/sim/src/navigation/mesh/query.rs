//! Corridor search over the ground mesh (old `GroundMeshQuery`,
//! `src/shared/navigation/groundMeshQuery.ts`): weighted A* over convex polygons through
//! portal midpoints, then a funnel pass tightens the corridor. Movement validates the
//! result against the real collision; the mesh only proposes a route.
//!
//! Tiles keep stable polygon ids (`tile · 65536 + index`) and sorted portals, so a tile
//! rebuilt after a local edit links exactly as in a cold build.

use super::build::{TILE, TileGeometry};
use super::polygons::{P, convex_polygons, cross};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Ref {
    tile: u32,
    index: u32,
}

impl Ref {
    fn id(self) -> u64 {
        u64::from(self.tile) * 65536 + u64::from(self.index)
    }
}

#[derive(Debug, Clone)]
struct Portal {
    to: Ref,
    a: P,
    b: P,
    midpoint: P,
}

#[derive(Debug, Clone)]
struct Polygon {
    points: Vec<P>,
    portals: Vec<Portal>,
    region: u32,
    seen: u32,
    closed: u32,
    cost: i64,
    previous: Option<(Ref, u32)>,
}

#[derive(Debug, Clone)]
struct Edge {
    node: u32,
    a: P,
    b: P,
}

#[derive(Debug, Clone, Default)]
struct Region {
    component: u32,
    links: BTreeSet<(u32, u32)>,
}

#[derive(Debug, Clone)]
struct QueryTile {
    nodes: Vec<Polygon>,
    buckets: BTreeMap<(i64, i64), Vec<u32>>,
    borders: [Vec<Edge>; 4],
    regions: Vec<Region>,
    boundary: Vec<u32>,
}

fn link(
    polygons: &mut [Polygon],
    first: &Edge,
    last: &Edge,
    a: P,
    b: P,
    first_ref: Ref,
    last_ref: Ref,
) {
    let midpoint = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    polygons[first.node as usize].portals.push(Portal {
        to: last_ref,
        a: b,
        b: a,
        midpoint,
    });
    polygons[last.node as usize].portals.push(Portal {
        to: first_ref,
        a,
        b,
        midpoint,
    });
}

fn equal(a: P, b: P) -> bool {
    (a.0 - b.0).abs() < 1e-8 && (a.1 - b.1).abs() < 1e-8
}

fn round(v: f64) -> i64 {
    crate::terrain::round_half_up(v) as i64
}

/// Integer cost on the half-milli lattice with exact square-root rounding.
fn distance(a: P, b: P) -> i64 {
    let dx = round((a.0 - b.0) * 2000.0);
    let dz = round((a.1 - b.1) * 2000.0);
    crate::math::isqrt_ceil((dx * dx + dz * dz) as u64) as i64
}

fn key(p: P) -> (i64, i64) {
    (round(p.0 * 1000.0), round(p.1 * 1000.0))
}

fn funnel(start: P, goal: P, portals: &[(P, P)]) -> Vec<P> {
    let mut gates = Vec::with_capacity(portals.len() + 2);
    gates.push((start, start));
    gates.extend_from_slice(portals);
    gates.push((goal, goal));
    let mut out = vec![start];
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut li, mut ri) = (0usize, 0usize);
    let mut i = 1;
    while i < gates.len() {
        let (l, r) = gates[i];
        if cross(apex, right, r) <= 0.0 {
            if equal(apex, right) || cross(apex, left, r) > 0.0 {
                right = r;
                ri = i;
            } else {
                out.push(left);
                apex = left;
                right = apex;
                left = apex;
                i = li;
                ri = li;
                i += 1;
                continue;
            }
        }
        if cross(apex, left, l) >= 0.0 {
            if equal(apex, left) || cross(apex, right, l) < 0.0 {
                left = l;
                li = i;
            } else {
                out.push(right);
                apex = right;
                left = apex;
                right = apex;
                i = ri;
                li = ri;
            }
        }
        i += 1;
    }
    if !equal(*out.last().expect("start"), goal) {
        out.push(goal);
    }
    let mut compact: Vec<P> = Vec::with_capacity(out.len());
    for p in out {
        if compact.last().is_some_and(|&last| equal(last, p)) {
            continue;
        }
        while compact.len() >= 2 {
            let (a, b) = (compact[compact.len() - 2], compact[compact.len() - 1]);
            if cross(a, b, p).abs() > 1e-8
                || (b.0 - a.0) * (p.0 - b.0) + (b.1 - a.1) * (p.1 - b.1) < 0.0
            {
                break;
            }
            compact.pop();
        }
        compact.push(p);
    }
    compact
}

/// The old binary frontier, ordered by (score, polygon id), reproduced exactly.
#[derive(Debug, Default, Clone)]
struct Frontier {
    entries: Vec<(Ref, P, f64, i64)>,
}

impl Frontier {
    fn less(&self, score: f64, id: u64, index: usize) -> bool {
        let e = &self.entries[index];
        score < e.2 || (score == e.2 && id < e.0.id())
    }

    fn push(&mut self, node: Ref, cost: i64, score: f64, point: P) {
        let mut i = self.entries.len();
        self.entries.push((node, point, score, cost));
        while i > 0 {
            let parent = (i - 1) >> 1;
            if !self.less(score, node.id(), parent) {
                break;
            }
            self.entries[i] = self.entries[parent];
            i = parent;
        }
        self.entries[i] = (node, point, score, cost);
    }

    fn pop(&mut self) -> (Ref, P, i64) {
        let top = self.entries[0];
        let last = self.entries.pop().expect("nonempty frontier");
        let end = self.entries.len();
        if end > 0 {
            let (node, _, score, _) = last;
            let mut i = 0;
            while i * 2 + 1 < end {
                let mut child = i * 2 + 1;
                if child + 1 < end
                    && self.less(
                        self.entries[child + 1].2,
                        self.entries[child + 1].0.id(),
                        child,
                    )
                {
                    child += 1;
                }
                let c = &self.entries[child];
                if !(c.2 < score || (c.2 == score && c.0.id() < node.id())) {
                    break;
                }
                self.entries[i] = self.entries[child];
                i = child;
            }
            self.entries[i] = last;
        }
        (top.0, top.1, top.3)
    }
}

#[derive(Debug, Clone)]
pub struct MeshQuery {
    width: u32,
    weight: f64,
    tiles: Vec<Option<QueryTile>>,
    epoch: u32,
    frontier: Frontier,
    /// Polygons expanded by the latest search.
    pub expanded: u32,
}

impl MeshQuery {
    pub fn new(size: u32, weight: f64) -> Self {
        let width = size.div_ceil(TILE as u32);
        Self {
            width,
            weight,
            tiles: vec![None; (width * width) as usize],
            epoch: 0,
            frontier: Frontier::default(),
            expanded: 0,
        }
    }

    fn node(&self, r: Ref) -> &Polygon {
        &self.tiles[r.tile as usize].as_ref().expect("tile").nodes[r.index as usize]
    }

    fn node_mut(&mut self, r: Ref) -> &mut Polygon {
        &mut self.tiles[r.tile as usize].as_mut().expect("tile").nodes[r.index as usize]
    }

    /// Replaces the `changed` tiles with their new geometry (`None`: no walkable area) and
    /// relinks their seams and the connectivity.
    pub fn update(&mut self, changed: &[(u32, Option<&TileGeometry>)]) {
        let width = self.width;
        let mut affected = BTreeSet::new();
        for &(id, _) in changed {
            let (x, z) = (id % width, id / width);
            affected.insert(id);
            if x > 0 {
                affected.insert(id - 1);
            }
            if x + 1 < width {
                affected.insert(id + 1);
            }
            if z > 0 {
                affected.insert(id - width);
            }
            if z + 1 < width {
                affected.insert(id + width);
            }
            self.tiles[id as usize] = None;
        }
        for &(id, geometry) in changed {
            if let Some(geometry) = geometry {
                self.tiles[id as usize] = Some(build_tile(id, geometry));
            }
        }
        let mut interfaces = BTreeSet::new();
        for &(id, _) in changed {
            let (x, z) = (id % width, id / width);
            if x > 0 {
                interfaces.insert((id - 1) * 2);
            }
            if x + 1 < width {
                interfaces.insert(id * 2);
            }
            if z > 0 {
                interfaces.insert((id - width) * 2 + 1);
            }
            if z + 1 < width {
                interfaces.insert(id * 2 + 1);
            }
        }
        for &code in &interfaces {
            let id = code / 2;
            let vertical = code % 2 == 1;
            let other = id + if vertical { width } else { 1 };
            let (side_a, side_b) = if vertical { (3, 2) } else { (1, 0) };
            self.unlink(id, side_a, other);
            self.unlink(other, side_b, id);
            let (Some(a), Some(b)) = (&self.tiles[id as usize], &self.tiles[other as usize]) else {
                continue;
            };
            let along = |p: P| if vertical { p.0 } else { p.1 };
            let first = a.borders[side_a].clone();
            let last = b.borders[side_b].clone();
            let mut links = Vec::new();
            let mut j = 0;
            for edge in &first {
                let lo = along(edge.a).min(along(edge.b));
                let hi = along(edge.a).max(along(edge.b));
                while j < last.len() && along(last[j].a).max(along(last[j].b)) <= lo {
                    j += 1;
                }
                for other_edge in &last[j..] {
                    let o_lo = along(other_edge.a).min(along(other_edge.b));
                    let o_hi = along(other_edge.a).max(along(other_edge.b));
                    let (low, high) = (lo.max(o_lo), hi.min(o_hi));
                    if o_lo >= hi {
                        break;
                    }
                    if high - low <= 1e-8 {
                        continue;
                    }
                    let forward = along(edge.b) > along(edge.a);
                    let set = |p: P, v: f64| if vertical { (v, p.1) } else { (p.0, v) };
                    let pa = set(edge.a, if forward { low } else { high });
                    let pb = set(edge.b, if forward { high } else { low });
                    links.push((edge.clone(), other_edge.clone(), pa, pb));
                }
            }
            for (edge, other_edge, pa, pb) in links {
                let midpoint = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
                let first_ref = Ref {
                    tile: id,
                    index: edge.node,
                };
                let last_ref = Ref {
                    tile: other,
                    index: other_edge.node,
                };
                self.node_mut(first_ref).portals.push(Portal {
                    to: last_ref,
                    a: pb,
                    b: pa,
                    midpoint,
                });
                self.node_mut(last_ref).portals.push(Portal {
                    to: first_ref,
                    a: pa,
                    b: pb,
                    midpoint,
                });
            }
        }
        let mut sort_nodes = affected;
        for &code in &interfaces {
            let id = code / 2;
            sort_nodes.insert(id);
            sort_nodes.insert(id + if code % 2 == 1 { width } else { 1 });
        }
        let rebuilt: BTreeSet<u32> = changed.iter().map(|&(id, _)| id).collect();
        for &id in &sort_nodes {
            let Some(tile) = self.tiles[id as usize].as_mut() else {
                continue;
            };
            let list: Vec<u32> = if rebuilt.contains(&id) {
                (0..tile.nodes.len() as u32).collect()
            } else {
                tile.boundary.clone()
            };
            for index in list {
                tile.nodes[index as usize].portals.sort_by(|a, b| {
                    a.to.id()
                        .cmp(&b.to.id())
                        .then(a.a.0.total_cmp(&b.a.0))
                        .then(a.a.1.total_cmp(&b.a.1))
                });
            }
        }
        for &id in &sort_nodes {
            if let Some(tile) = self.tiles[id as usize].as_mut() {
                for region in &mut tile.regions {
                    region.links.clear();
                }
            }
        }
        // Region links need the target polygons' regions, which may live in other tiles.
        for &id in &sort_nodes {
            let Some(tile) = self.tiles[id as usize].as_ref() else {
                continue;
            };
            let mut found: Vec<(u32, (u32, u32))> = Vec::new();
            for &index in &tile.boundary {
                let node = &tile.nodes[index as usize];
                for portal in &node.portals {
                    if portal.to.tile != id {
                        let target = self.node(portal.to).region;
                        found.push((node.region, (portal.to.tile, target)));
                    }
                }
            }
            let tile = self.tiles[id as usize].as_mut().expect("tile");
            for (region, link) in found {
                tile.regions[region as usize].links.insert(link);
            }
        }
        let mut component = 0;
        for tile in self.tiles.iter_mut().flatten() {
            for region in &mut tile.regions {
                region.component = 0;
            }
        }
        for t in 0..self.tiles.len() {
            let count = self.tiles[t].as_ref().map_or(0, |tile| tile.regions.len());
            for r in 0..count {
                if self.region(t as u32, r as u32).component != 0 {
                    continue;
                }
                component += 1;
                self.region_mut(t as u32, r as u32).component = component;
                let mut queue = vec![(t as u32, r as u32)];
                let mut at = 0;
                while at < queue.len() {
                    let links: Vec<(u32, u32)> = self
                        .region(queue[at].0, queue[at].1)
                        .links
                        .iter()
                        .copied()
                        .collect();
                    at += 1;
                    for (lt, lr) in links {
                        if self.region(lt, lr).component == 0 {
                            self.region_mut(lt, lr).component = component;
                            queue.push((lt, lr));
                        }
                    }
                }
            }
        }
    }

    fn region(&self, tile: u32, region: u32) -> &Region {
        &self.tiles[tile as usize].as_ref().expect("tile").regions[region as usize]
    }

    fn region_mut(&mut self, tile: u32, region: u32) -> &mut Region {
        &mut self.tiles[tile as usize].as_mut().expect("tile").regions[region as usize]
    }

    fn unlink(&mut self, id: u32, side: usize, other: u32) {
        let Some(tile) = self.tiles[id as usize].as_mut() else {
            return;
        };
        let mut seen = BTreeSet::new();
        let nodes: Vec<u32> = tile.borders[side].iter().map(|e| e.node).collect();
        for node in nodes {
            if !seen.insert(node) {
                continue;
            }
            tile.nodes[node as usize]
                .portals
                .retain(|p| p.to.tile != other);
        }
    }

    fn locate(&self, p: P) -> Option<Ref> {
        let tile = f64::from(TILE);
        let tx = ((p.0 + 0.5) / tile).floor() as i64;
        let tz = ((p.1 + 0.5) / tile).floor() as i64;
        // At an exact seam the preceding tile is searched first.
        let seam = |v: f64| (v + 0.5) % tile == 0.0;
        let span = |t: i64, at_seam: bool| if at_seam { t - 1..=t } else { t..=t };
        let (xs, zs) = (span(tx, seam(p.0)), span(tz, seam(p.1)));
        let bucket = ((p.0 / 8.0).floor() as i64, (p.1 / 8.0).floor() as i64);
        let width = i64::from(self.width);
        for z in zs {
            for x in xs.clone() {
                if x < 0 || z < 0 || x >= width || z >= width {
                    continue;
                }
                let id = (z * width + x) as u32;
                let Some(t) = self.tiles[id as usize].as_ref() else {
                    continue;
                };
                for &index in t.buckets.get(&bucket).into_iter().flatten() {
                    let points = &t.nodes[index as usize].points;
                    let n = points.len();
                    if (0..n).all(|i| cross(points[i], points[(i + 1) % n], p) <= 1e-8) {
                        return Some(Ref { tile: id, index });
                    }
                }
            }
        }
        None
    }

    /// The funnel path from `start` to `goal`, attaching each endpoint to the mesh within
    /// `padding` cells. `None` when either endpoint is off the mesh or no corridor exists.
    pub fn path(&mut self, start: P, goal: P, padding: f64) -> Option<Vec<P>> {
        self.expanded = 0;
        let endpoint = |q: &Self, p: P| {
            q.locate(p).or_else(|| {
                if padding <= 0.0 {
                    return None;
                }
                for dz in [-padding, 0.0, padding] {
                    for dx in [-padding, 0.0, padding] {
                        if dx == 0.0 && dz == 0.0 {
                            continue;
                        }
                        if let Some(found) = q.locate((p.0 + dx, p.1 + dz)) {
                            return Some(found);
                        }
                    }
                }
                None
            })
        };
        let from = endpoint(self, start)?;
        let to = endpoint(self, goal)?;
        let component = |q: &Self, r: Ref| q.region(r.tile, q.node(r).region).component;
        if component(self, from) != component(self, to) {
            return None;
        }
        self.epoch += 1;
        if self.epoch == u32::MAX {
            for node in self
                .tiles
                .iter_mut()
                .flatten()
                .flat_map(|t| t.nodes.iter_mut())
            {
                node.seen = 0;
                node.closed = 0;
            }
            self.epoch = 1;
        }
        let epoch = self.epoch;
        {
            let node = self.node_mut(from);
            node.seen = epoch;
            node.cost = 0;
            node.previous = None;
        }
        self.frontier.entries.clear();
        self.frontier
            .push(from, 0, distance(start, goal) as f64, start);
        while !self.frontier.entries.is_empty() {
            let (current, point, cost) = self.frontier.pop();
            {
                let node = self.node_mut(current);
                if cost != node.cost || node.closed == epoch {
                    continue;
                }
                node.closed = epoch;
            }
            self.expanded += 1;
            if current == to {
                let mut gates = Vec::new();
                let mut at = to;
                while at != from {
                    let (previous, portal) = self.node(at).previous.expect("predecessor");
                    let p = &self.node(previous).portals[portal as usize];
                    gates.push((p.a, p.b));
                    at = previous;
                }
                gates.reverse();
                return Some(funnel(start, goal, &gates));
            }
            let count = self.node(current).portals.len();
            for index in 0..count {
                let (next, midpoint) = {
                    let p = &self.node(current).portals[index];
                    (p.to, p.midpoint)
                };
                if self.node(next).closed == epoch {
                    continue;
                }
                let target = if next == to { goal } else { midpoint };
                let next_cost = cost + distance(point, target);
                let n = self.node_mut(next);
                if n.seen == epoch && next_cost >= n.cost {
                    continue;
                }
                n.seen = epoch;
                n.cost = next_cost;
                n.previous = Some((current, index as u32));
                let score = next_cost as f64 + self.weight * distance(target, goal) as f64;
                self.frontier.push(next, next_cost, score, target);
            }
        }
        None
    }
}

/// An undirected edge by its rounded endpoints.
type EdgeKey = ((i64, i64), (i64, i64));

fn build_tile(id: u32, geometry: &TileGeometry) -> QueryTile {
    let polygons = convex_polygons(geometry);
    assert!(
        polygons.len() < 65536,
        "navigation tile polygon limit exceeded"
    );
    let mut nodes: Vec<Polygon> = Vec::with_capacity(polygons.len());
    let mut buckets: BTreeMap<(i64, i64), Vec<u32>> = BTreeMap::new();
    let mut open: BTreeMap<EdgeKey, Edge> = BTreeMap::new();
    for (i, points) in polygons.into_iter().enumerate() {
        let index = i as u32;
        let floor8 = |v: f64| (v / 8.0).floor() as i64;
        let min_x = floor8(points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min));
        let max_x = floor8(points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max));
        let min_z = floor8(points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min));
        let max_z = floor8(points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max));
        for z in min_z..=max_z {
            for x in min_x..=max_x {
                buckets.entry((x, z)).or_default().push(index);
            }
        }
        nodes.push(Polygon {
            points: points.clone(),
            portals: Vec::new(),
            region: u32::MAX,
            seen: 0,
            closed: 0,
            cost: 0,
            previous: None,
        });
        for e in 0..points.len() {
            let (a, b) = (points[e], points[(e + 1) % points.len()]);
            let (ka, kb) = (key(a), key(b));
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            let edge = Edge { node: index, a, b };
            if let Some(other) = open.remove(&k) {
                let (oa, ob) = (other.a, other.b);
                link(
                    &mut nodes,
                    &other,
                    &edge,
                    oa,
                    ob,
                    Ref {
                        tile: id,
                        index: other.node,
                    },
                    Ref { tile: id, index },
                );
            } else {
                open.insert(k, edge);
            }
        }
    }
    let mut regions: Vec<Region> = Vec::new();
    for start in 0..nodes.len() {
        if nodes[start].region != u32::MAX {
            continue;
        }
        let region = regions.len() as u32;
        regions.push(Region::default());
        nodes[start].region = region;
        let mut queue = vec![start];
        let mut i = 0;
        while i < queue.len() {
            let current = queue[i];
            i += 1;
            for p in 0..nodes[current].portals.len() {
                let to = nodes[current].portals[p].to.index as usize;
                if nodes[to].region == u32::MAX {
                    nodes[to].region = region;
                    queue.push(to);
                }
            }
        }
    }
    let mut borders: [Vec<Edge>; 4] = Default::default();
    let mut boundary: BTreeSet<u32> = BTreeSet::new();
    let (min_x, max_x) = (
        f64::from(geometry.min_x) / 1000.0,
        f64::from(geometry.max_x) / 1000.0,
    );
    let (min_z, max_z) = (
        f64::from(geometry.min_z) / 1000.0,
        f64::from(geometry.max_z) / 1000.0,
    );
    // Border lists are sorted below by a total key (a convex polygon has at most one edge on
    // a seam line), so the order unmatched edges are visited in does not matter.
    for edge in open.values() {
        let (a, b) = (edge.a, edge.b);
        let side = if a.0 == b.0 && a.0 == min_x {
            0
        } else if a.0 == b.0 && a.0 == max_x {
            1
        } else if a.1 == b.1 && a.1 == min_z {
            2
        } else if a.1 == b.1 && a.1 == max_z {
            3
        } else {
            continue;
        };
        borders[side].push(edge.clone());
        boundary.insert(edge.node);
    }
    for (side, list) in borders.iter_mut().enumerate() {
        let along = |p: P| if side < 2 { p.1 } else { p.0 };
        list.sort_by(|a, b| {
            along(a.a)
                .min(along(a.b))
                .total_cmp(&along(b.a).min(along(b.b)))
                .then(a.node.cmp(&b.node))
        });
    }
    QueryTile {
        nodes,
        buckets,
        borders,
        regions,
        boundary: boundary.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn funnels_pull_tight_around_corners() {
        // Two gates forming a corner at (5, 5).
        let gates = [((5.0, 0.0), (5.0, 5.0)), ((5.0, 5.0), (10.0, 5.0))];
        let path = funnel((0.0, 0.0), (10.0, 10.0), &gates);
        assert_eq!(path.first(), Some(&(0.0, 0.0)));
        assert_eq!(path.last(), Some(&(10.0, 10.0)));
        assert!(path.len() <= 3);
    }

    #[test]
    fn distances_round_up_on_the_half_milli_lattice() {
        assert_eq!(distance((0.0, 0.0), (3.0, 4.0)), 10_000);
        assert_eq!(distance((0.0, 0.0), (0.0005, 0.0005)), 2);
    }
}
