//! The map briefing and static geography (old `ai/briefing.ts`, `Geography` in
//! `ai/frame.ts`): authored, public map facts every AI knows from the start (terrain,
//! starts, camp sites and their nominal defenders, resource sites) and terrain
//! connectivity under the same step rule as movement. Nothing about the match: no
//! runtime entities, owners, camp survival or loot.

use crate::{
    action::CellPoint,
    catalog::{Catalog, Def},
    entity::{Milli, Place},
    fingerprint::Fnv,
    terrain::{DeckId, MAX_GROUND_STEP_CM, Terrain},
};
use canopy_content::{
    map::UtcMap,
    placement::{Aggression, MapKnowledge},
};
use std::sync::Arc;

/// A point in cells, optionally on a bridge deck.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub deck: Option<DeckId>,
}

impl Pt {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y, deck: None }
    }

    pub fn of(position: Milli, deck: Option<DeckId>) -> Self {
        Self {
            x: f64::from(position.x) / 1000.0,
            y: f64::from(position.y) / 1000.0,
            deck,
        }
    }

    /// Whole cells, halves rounded up (old `integerPoint`).
    pub fn rounded(self) -> Self {
        Self {
            x: round(self.x),
            y: round(self.y),
            deck: self.deck,
        }
    }

    /// Fixed-point milli-cells (old `fixed`).
    pub fn milli(self) -> (i32, i32) {
        (round(self.x * 1000.0) as i32, round(self.y * 1000.0) as i32)
    }
}

/// `Math.round`: halves toward +∞.
pub fn round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// Euclidean distance in cells.
pub fn dist(a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    (dx * dx + dy * dy).sqrt()
}

/// A base's own frame for site searches: `front` points from home to the middle of the
/// map's starts, and `hand` turns the first sidesteps away from home's nearest resource.
/// Searches stepping through it reach the same sites at every start of a mirrored or
/// rotated map, where fixed world directions (east first, rows from the top) favoured
/// some starts over others.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compass {
    front: (f64, f64),
    hand: f64,
}

impl Compass {
    /// The frame of a base at `home` facing `middle`, its first sidesteps turning away
    /// from `resource` (the nearest), so mirrored starts turn the mirrored way.
    pub fn facing(home: Pt, middle: Pt, resource: Option<Pt>) -> Self {
        let (dx, dy) = (middle.x - home.x, middle.y - home.y);
        let d = (dx * dx + dy * dy).sqrt();
        let front = if d < 1.0 {
            (1.0, 0.0)
        } else {
            (dx / d, dy / d)
        };
        let cross = resource.map_or(0.0, |r| front.0 * (r.y - home.y) - front.1 * (r.x - home.x));
        Compass {
            front,
            hand: if cross > 0.0 { -1.0 } else { 1.0 },
        }
    }

    /// The unit direction of step `k` of `n` around the circle, in the order front,
    /// one step to the hand side, one step to the other, two steps, and so on.
    pub fn direction(self, k: u32, n: u32) -> (f64, f64) {
        self.turn(Self::step(k, n))
    }

    /// The angle (radians, towards the hand side when positive) of step `k` of `n`.
    pub fn step(k: u32, n: u32) -> f64 {
        let n = n.max(1);
        let k = k % n;
        let side = if k % 2 == 1 { 1.0 } else { -1.0 };
        f64::from(k.div_ceil(2)) * side * std::f64::consts::TAU / f64::from(n)
    }

    /// The front turned by `angle` radians towards the hand side.
    pub fn turn(self, angle: f64) -> (f64, f64) {
        let angle = angle * self.hand;
        let (c, s) = (crate::math::cos(angle), crate::math::sin(angle));
        let (x, y) = self.front;
        (x * c - y * s, x * s + y * c)
    }
}

/// A neutral camp the map shows: where it is and who guards it.
#[derive(Debug, Clone, PartialEq)]
pub struct CampSite {
    pub id: String,
    pub point: Pt,
    pub radius: f64,
    /// Public defender definitions, sorted by id.
    pub composition: Vec<Def>,
}

/// An authored harvestable placement.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceSite {
    pub id: String,
    pub definition: Def,
    pub point: Pt,
}

/// Frozen map facts and terrain connectivity, shared by every AI of a match.
#[derive(Debug)]
pub struct Geography {
    pub terrain: Arc<Terrain>,
    pub size: u32,
    pub fingerprint: u32,
    /// Player starts, sorted by x then y.
    pub starts: Vec<Pt>,
    /// Camps that attack players and are not hidden, sorted by id.
    pub camps: Vec<CampSite>,
    pub resources: Vec<ResourceSite>,
    /// Ground regions by cell (0: not land); empty on layered maps.
    regions: Vec<u32>,
    /// Appearance scale of placements, by placement id (resource collision discs).
    pub scales: std::collections::BTreeMap<String, f64>,
}

impl Geography {
    pub fn new(
        map: &UtcMap,
        catalog: &Catalog,
        terrain: Arc<Terrain>,
        scales: std::collections::BTreeMap<String, f64>,
    ) -> Self {
        let size = terrain.size();
        let public = |k: Option<MapKnowledge>| k != Some(MapKnowledge::Hidden);
        let mut camps: Vec<CampSite> = map
            .camps
            .iter()
            .filter(|c| c.aggression == Aggression::Players && public(c.map_knowledge))
            .map(|c| {
                let mut composition: Vec<Def> = c
                    .members
                    .iter()
                    .filter_map(|id| map.entities.iter().find(|p| &p.id == id))
                    .filter(|p| public(p.map_knowledge))
                    .filter_map(|p| catalog.find(p.definition.as_str()))
                    .collect();
                composition.sort_unstable();
                CampSite {
                    id: c.id.clone(),
                    point: Pt::new(c.home.x, c.home.y),
                    radius: f64::from(c.leash.min(16)),
                    composition,
                }
            })
            .collect();
        camps.sort_by(|a, b| a.id.cmp(&b.id));
        let resources: Vec<ResourceSite> = map
            .entities
            .iter()
            .filter(|p| public(p.map_knowledge))
            .filter_map(|p| {
                let def = catalog.find(p.definition.as_str())?;
                (catalog.get(def).resource_yield.unwrap_or(0) > 0).then(|| ResourceSite {
                    id: p.id.clone(),
                    definition: def,
                    point: Pt::new(p.position.x, p.position.y),
                })
            })
            .collect();
        let mut starts: Vec<Pt> = map
            .player_starts
            .iter()
            .map(|s| Pt::new(s.x, s.z))
            .collect();
        starts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        let regions = if terrain.surfaces().is_some() {
            Vec::new()
        } else {
            ground_regions(&terrain)
        };
        let mut hash = Fnv::default();
        hash.word(size);
        for (&h, &w) in terrain.heights().iter().zip(terrain.walkable_cells()) {
            hash.word(h as u16 as u32 | u32::from(w) << 16);
        }
        if let Some(s) = terrain.surfaces() {
            hash.word(s.node_count());
        }
        for s in &starts {
            hash.int((s.x * 2.0) as i64);
            hash.int((s.y * 2.0) as i64);
        }
        for c in &camps {
            hash.text(&c.id);
            hash.int((c.point.x * 2.0) as i64);
            hash.int((c.point.y * 2.0) as i64);
            hash.int(c.radius as i64);
            for d in &c.composition {
                hash.text(catalog.id(*d).as_str());
            }
        }
        for r in &resources {
            hash.text(&r.id);
            hash.text(catalog.id(r.definition).as_str());
            hash.int((r.point.x * 2.0) as i64);
            hash.int((r.point.y * 2.0) as i64);
        }
        Self {
            size,
            fingerprint: hash.finish(),
            starts,
            camps,
            resources,
            regions,
            scales,
            terrain,
        }
    }

    pub fn layered(&self) -> bool {
        self.terrain.surfaces().is_some()
    }

    /// The search frame of a base at `home` (see [`Compass`]).
    pub fn compass(&self, home: Pt) -> Compass {
        let n = self.starts.len().max(1) as f64;
        let middle = if self.starts.len() > 1 {
            Pt::new(
                self.starts.iter().map(|s| s.x).sum::<f64>() / n,
                self.starts.iter().map(|s| s.y).sum::<f64>() / n,
            )
        } else {
            let c = f64::from(self.size) / 2.0;
            Pt::new(c, c)
        };
        let resource = self
            .resources
            .iter()
            .map(|r| r.point)
            .min_by(|a, b| dist(*a, home).total_cmp(&dist(*b, home)));
        Compass::facing(home, middle, resource)
    }

    /// The cell of a point, halves rounded up; may run into the next row at the edge as
    /// the old index did, and is `None` past the end of the map.
    pub fn index(&self, p: Pt) -> Option<u32> {
        let i = round(p.y) * f64::from(self.size) + round(p.x);
        (i >= 0.0 && i < f64::from(self.size * self.size)).then_some(i as u32)
    }

    pub fn inside(&self, p: Pt) -> bool {
        let s = f64::from(self.size);
        p.x >= 0.0 && p.y >= 0.0 && p.x < s && p.y < s
    }

    /// The walk-surface node of a rounded point on a layered map.
    pub fn node(&self, p: Pt) -> Option<u32> {
        let s = self.terrain.surfaces()?;
        let q = p.rounded();
        s.node_at(q.x as i64, q.y as i64, p.deck)
    }

    /// Whether ground (and decks) connect the two points.
    pub fn connected(&self, a: Pt, b: Pt) -> bool {
        if let Some(s) = self.terrain.surfaces() {
            return match (self.node(a), self.node(b)) {
                (Some(from), Some(to)) => s.connected(from, to),
                _ => false,
            };
        }
        if !self.inside(a) || !self.inside(b) {
            return false;
        }
        match (self.index(a), self.index(b)) {
            (Some(i), Some(j)) => {
                let r = self.regions[i as usize];
                r != 0 && r == self.regions[j as usize]
            }
            _ => false,
        }
    }

    /// Whether the cell is land (not water deeper than wading).
    pub fn land(&self, cell: u32) -> bool {
        self.terrain.walkable(cell as usize)
    }

    pub fn height(&self, cell: u32) -> i32 {
        i32::from(self.terrain.height_cm(cell as usize))
    }

    /// The deck name of a point, for commands.
    pub fn surface_name(&self, deck: Option<DeckId>) -> Option<String> {
        let s = self.terrain.surfaces()?;
        deck.map(|d| s.deck(d).id.clone())
    }

    pub fn deck_id(&self, name: Option<&str>) -> Option<DeckId> {
        name.and_then(|n| self.terrain.deck_id(n))
    }

    /// A whole-cell point as a command destination.
    pub fn cell_point(&self, p: Pt) -> CellPoint {
        let q = p.rounded();
        CellPoint {
            x: q.x.max(0.0) as u32,
            y: q.y.max(0.0) as u32,
            surface: self.surface_name(p.deck),
        }
    }

    /// A whole-cell point as remembered.
    pub fn place(&self, p: Pt) -> Place {
        let q = p.rounded();
        Place {
            x: q.x as i32,
            y: q.y as i32,
            surface: self.surface_name(p.deck),
        }
    }

    pub fn from_place(&self, p: &Place) -> Pt {
        Pt {
            x: f64::from(p.x),
            y: f64::from(p.y),
            deck: self.deck_id(p.surface.as_deref()),
        }
    }
}

/// Ground components joined by four-neighbour steps no higher than movement climbs.
fn ground_regions(terrain: &Terrain) -> Vec<u32> {
    let size = terrain.size() as usize;
    let land = terrain.walkable_cells();
    let heights = terrain.heights();
    let mut regions = vec![0u32; size * size];
    let mut queue = vec![0usize; size * size];
    let mut region = 0;
    for start in 0..land.len() {
        if !land[start] || regions[start] != 0 {
            continue;
        }
        let (mut head, mut tail) = (0, 1);
        queue[0] = start;
        region += 1;
        regions[start] = region;
        while head < tail {
            let a = queue[head];
            head += 1;
            let (x, y) = (a % size, a / size);
            let mut visit = |b: usize| {
                if land[b]
                    && regions[b] == 0
                    && (i32::from(heights[a]) - i32::from(heights[b])).abs() <= MAX_GROUND_STEP_CM
                {
                    regions[b] = region;
                    queue[tail] = b;
                    tail += 1;
                }
            };
            if x + 1 < size {
                visit(a + 1);
            }
            if x > 0 {
                visit(a - 1);
            }
            if y + 1 < size {
                visit(a + size);
            }
            if y > 0 {
                visit(a - size);
            }
        }
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrored and rotated starts search the mirrored and rotated sites in the same
    /// order, so no start of a symmetric map is favoured.
    #[test]
    fn mirrored_starts_search_mirrored_sites() {
        let close =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        let middle = Pt::new(300.0, 200.0);
        // Mirrored across x = 300: the resource sits below both halls.
        let west = Compass::facing(Pt::new(100.0, 200.0), middle, Some(Pt::new(104.0, 176.0)));
        let east = Compass::facing(Pt::new(500.0, 200.0), middle, Some(Pt::new(496.0, 176.0)));
        // Turned half round the middle.
        let south = Compass::facing(Pt::new(300.0, 0.0), middle, Some(Pt::new(276.0, 4.0)));
        let north = Compass::facing(Pt::new(300.0, 400.0), middle, Some(Pt::new(324.0, 396.0)));
        for k in 0..32 {
            let (w, e) = (west.direction(k, 32), east.direction(k, 32));
            assert!(close(w, (-e.0, e.1)), "step {k}: {w:?} mirrors {e:?}");
            let (s, n) = (south.direction(k, 32), north.direction(k, 32));
            assert!(close(s, (-n.0, -n.1)), "step {k}: {s:?} turns into {n:?}");
        }
        // The first sidestep turns away from the resource.
        let side = west.direction(1, 32);
        assert!(side.1 > 0.0, "{side:?}");
    }
}
