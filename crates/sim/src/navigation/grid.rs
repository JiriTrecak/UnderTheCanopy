//! Eight-neighbour integer A* on the cell grid (old `Navigation`,
//! `src/sim/game/navigation.ts`): cardinal steps cost 1000, diagonals 1414, octile
//! heuristic, ties by (f, h, cell). A diagonal needs both cardinal side corridors clear.
//! The step predicate is the caller's (a body sweep); its answers are cached per directed
//! edge and invalidated around static changes. Live reservations are never cached.

use super::cells::CellSet;
use std::{cmp::Reverse, collections::BinaryHeap};

pub const CARDINAL_COST: i64 = 1000;
pub const DIAGONAL_COST: i64 = 1414;
/// Neighbour order: cardinals first, then diagonals; bit `i` in the edge cache.
pub const DIRECTIONS: [(i32, i32); 8] = [
    (0, -1),
    (-1, 0),
    (1, 0),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Cache bit of a step `(dx, dy)`.
fn direction_bit(dx: i32, dy: i32) -> u8 {
    const BITS: [u8; 9] = [16, 1, 32, 2, 0, 4, 64, 8, 128];
    BITS[((dy + 1) * 3 + dx + 1) as usize]
}

/// Adjacent movement with both side corridors clear for a diagonal.
pub fn can_traverse(size: u32, from: u32, to: u32, mut step: impl FnMut(u32, u32) -> bool) -> bool {
    let n = size * size;
    if from >= n || to >= n {
        return false;
    }
    let dx = (to % size) as i32 - (from % size) as i32;
    let dy = (to / size) as i32 - (from / size) as i32;
    if dx.abs() > 1 || dy.abs() > 1 || (dx == 0 && dy == 0) || !step(from, to) {
        return false;
    }
    if dx == 0 || dy == 0 {
        return true;
    }
    let side_x = (from as i64 + i64::from(dx)) as u32;
    let side_y = (from as i64 + i64::from(dy) * i64::from(size)) as u32;
    step(from, side_x) && step(from, side_y) && step(side_x, to) && step(side_y, to)
}

pub fn octile(dx: i64, dy: i64) -> i64 {
    let (dx, dy) = (dx.abs(), dy.abs());
    CARDINAL_COST * dx.max(dy) + (DIAGONAL_COST - CARDINAL_COST) * dx.min(dy)
}

/// Reusable search buffers shared by every grid search.
#[derive(Debug, Default, Clone)]
pub struct Scratch {
    prev: Vec<u32>,
    cost: Vec<i64>,
    seen: Vec<u32>,
    closed: Vec<u32>,
    epoch: u32,
    heap: BinaryHeap<Reverse<(i64, i64, u32)>>,
    probe: Vec<u32>,
    reverse: Vec<u32>,
    /// Nodes expanded by the latest search.
    pub expanded: u32,
}

impl Scratch {
    fn begin(&mut self, n: usize) -> u32 {
        if self.seen.len() != n {
            self.prev = vec![0; n];
            self.cost = vec![0; n];
            self.seen = vec![0; n];
            self.closed = vec![0; n];
            self.probe = vec![0; n];
            self.epoch = 0;
        }
        self.epoch += 1;
        if self.epoch == u32::MAX {
            self.seen.fill(0);
            self.closed.fill(0);
            self.probe.fill(0);
            self.epoch = 1;
        }
        self.expanded = 0;
        self.epoch
    }
}

/// How one search is constrained.
#[derive(Clone, Copy)]
pub struct Search<'a> {
    /// Cells that may not be entered (live traffic).
    pub blocked: Option<&'a CellSet>,
    /// Largest accepted route cost; `i64::MAX` for none.
    pub max_cost: i64,
    /// Allowed 16-cell sectors, row-major.
    pub corridor: Option<&'a [bool]>,
    /// Heuristic weight in permille; above 1000 trades optimality for a smaller frontier.
    pub heuristic_permille: i64,
    /// Goal-side reverse probe budget.
    pub probe_limit: usize,
    /// Remember an exhausted unrestricted search's reachable region.
    pub remember_origin: bool,
}

impl Default for Search<'_> {
    fn default() -> Self {
        Self {
            blocked: None,
            max_cost: i64::MAX,
            corridor: None,
            heuristic_permille: 1000,
            probe_limit: 128,
            remember_origin: false,
        }
    }
}

/// Grid navigation for one step rule (one body radius), with its edge cache.
#[derive(Debug, Clone)]
pub struct GridNavigation {
    size: u32,
    edges: Vec<u8>,
    known: Vec<u8>,
    /// The exact reachable region of the latest exhausted unrestricted search.
    origin: Option<(u32, Vec<bool>)>,
}

impl GridNavigation {
    pub fn new(size: u32) -> Self {
        let n = (size * size) as usize;
        Self {
            size,
            edges: vec![0; n],
            known: vec![0; n],
            origin: None,
        }
    }

    /// Forgets cached edges within `padding` cells of changed cells (all, for `None`).
    pub fn invalidate(&mut self, cells: Option<&[u32]>, padding: i32) {
        self.origin = None;
        let Some(cells) = cells else {
            self.known.fill(0);
            return;
        };
        let size = self.size as i32;
        for &cell in cells {
            let (x, y) = ((cell % self.size) as i32, (cell / self.size) as i32);
            for ny in (y - padding).max(0)..=(y + padding).min(size - 1) {
                for nx in (x - padding).max(0)..=(x + padding).min(size - 1) {
                    self.known[(ny * size + nx) as usize] = 0;
                }
            }
        }
    }

    /// Whether an earlier exhausted search from `start` proved `goal` unreachable.
    pub fn known_unreachable(&self, start: u32, goal: u32) -> bool {
        self.origin
            .as_ref()
            .is_some_and(|(s, cells)| *s == start && !cells[goal as usize])
    }

    fn edge(
        &mut self,
        from: u32,
        to: u32,
        bit: u8,
        step: &mut impl FnMut(u32, u32) -> bool,
    ) -> bool {
        let f = from as usize;
        if self.known[f] & bit == 0 {
            let clear = if bit < 16 {
                step(from, to)
            } else {
                let size = i64::from(self.size);
                let dx = i64::from(to % self.size) - i64::from(from % self.size);
                let dy = i64::from(to / self.size) - i64::from(from / self.size);
                let side_x = (i64::from(from) + dx) as u32;
                let side_y = (i64::from(from) + dy * size) as u32;
                let (bx, by) = (direction_bit(dx as i32, 0), direction_bit(0, dy as i32));
                step(from, to)
                    && self.edge(from, side_x, bx, step)
                    && self.edge(from, side_y, by, step)
                    && self.edge(side_x, to, by, step)
                    && self.edge(side_y, to, bx, step)
            };
            if clear {
                self.edges[f] |= bit;
            } else {
                self.edges[f] &= !bit;
            }
            self.known[f] |= bit;
        }
        self.edges[f] & bit != 0
    }

    /// Whether `a → b` (a step of `(dx, dy)`) is traversable and free of blocked cells.
    fn traverse(
        &mut self,
        a: u32,
        b: u32,
        dx: i32,
        dy: i32,
        blocked: Option<&CellSet>,
        step: &mut impl FnMut(u32, u32) -> bool,
    ) -> bool {
        if !self.edge(a, b, direction_bit(dx, dy), step) {
            return false;
        }
        let Some(blocked) = blocked else {
            return true;
        };
        if blocked.contains(b) {
            return false;
        }
        if dx != 0 && dy != 0 {
            let side_x = (i64::from(a) + i64::from(dx)) as u32;
            let side_y = (i64::from(a) + i64::from(dy) * i64::from(self.size)) as u32;
            if blocked.contains(side_x) || blocked.contains(side_y) {
                return false;
            }
        }
        true
    }

    /// The cells after `start` up to and including `goal`, or `None`.
    pub fn path(
        &mut self,
        s: &mut Scratch,
        step: &mut impl FnMut(u32, u32) -> bool,
        connected: impl FnOnce(u32, u32) -> bool,
        start: u32,
        goal: u32,
        q: Search,
    ) -> Option<Vec<u32>> {
        let size = self.size;
        let n = size * size;
        s.expanded = 0;
        if start >= n || goal >= n {
            return None;
        }
        if start == goal {
            return Some(Vec::new());
        }
        if !connected(start, goal) {
            return None;
        }
        let epoch = s.begin(n as usize);
        let blocked = q.blocked.filter(|b| !b.is_empty());
        if blocked.is_some_and(|b| b.contains(goal)) {
            return None;
        }
        let obstacle = |c: u32| blocked.is_some_and(|b| b.contains(c));
        let in_map = |x: i32, y: i32| x >= 0 && y >= 0 && x < size as i32 && y < size as i32;
        // A boxed-in start cannot reach anything.
        let (sx, sy) = ((start % size) as i32, (start / size) as i32);
        let mut open_start = false;
        for (dx, dy) in DIRECTIONS {
            let (x, y) = (sx + dx, sy + dy);
            if in_map(x, y)
                && self.traverse(start, y as u32 * size + x as u32, dx, dy, blocked, step)
            {
                open_start = true;
                break;
            }
        }
        if !open_start {
            return None;
        }
        // Reject an enclosed destination before flooding the map.
        let (gx, gy) = ((goal % size) as i32, (goal / size) as i32);
        let mut enterable = false;
        for (dx, dy) in DIRECTIONS {
            let (x, y) = (gx + dx, gy + dy);
            if !in_map(x, y) {
                continue;
            }
            let from = y as u32 * size + x as u32;
            if (from == start || !obstacle(from))
                && self.traverse(from, goal, -dx, -dy, blocked, step)
            {
                enterable = true;
                break;
            }
        }
        if !enterable {
            return None;
        }
        // A small goal-side pocket proves failure cheaply; larger ones fall through.
        s.reverse.clear();
        s.reverse.push(goal);
        s.probe[goal as usize] = epoch;
        let mut reached = false;
        let mut cursor = 0;
        while cursor < s.reverse.len() && s.reverse.len() < q.probe_limit {
            let to = s.reverse[cursor];
            let (x, y) = ((to % size) as i32, (to / size) as i32);
            for (dx, dy) in DIRECTIONS {
                let (nx, ny) = (x + dx, y + dy);
                if !in_map(nx, ny) {
                    continue;
                }
                let from = ny as u32 * size + nx as u32;
                if s.probe[from as usize] == epoch
                    || (from != start && obstacle(from))
                    || !self.traverse(from, to, -dx, -dy, blocked, step)
                {
                    continue;
                }
                if from == start {
                    reached = true;
                    break;
                }
                s.probe[from as usize] = epoch;
                s.reverse.push(from);
            }
            if reached {
                break;
            }
            cursor += 1;
        }
        if !reached && cursor >= s.reverse.len() {
            return None;
        }
        let weighted = q.heuristic_permille > 1000;
        let priority = |h: i64| {
            if weighted {
                h * q.heuristic_permille / 1000
            } else {
                h
            }
        };
        let heuristic = |x: i32, y: i32| octile(i64::from(x - gx), i64::from(y - gy));
        let h0 = heuristic(sx, sy);
        if h0 > q.max_cost {
            return None;
        }
        let sector_width = size.div_ceil(super::sectors::SECTOR_SIZE);
        s.heap.clear();
        s.seen[start as usize] = epoch;
        s.cost[start as usize] = 0;
        let p0 = priority(h0);
        s.heap.push(Reverse((p0, p0, start)));
        while let Some(Reverse((f, ph, id))) = s.heap.pop() {
            let g = f - ph;
            let i = id as usize;
            if s.closed[i] == epoch || g != s.cost[i] {
                continue;
            }
            if id == goal {
                let mut out = Vec::new();
                let mut at = goal;
                while at != start {
                    out.push(at);
                    at = s.prev[at as usize];
                }
                out.reverse();
                return Some(out);
            }
            s.closed[i] = epoch;
            s.expanded += 1;
            let (x, y) = ((id % size) as i32, (id / size) as i32);
            // Known-impassable edges are skipped before any other work.
            let candidates = self.edges[i] | !self.known[i];
            for (direction, (dx, dy)) in DIRECTIONS.into_iter().enumerate() {
                if candidates & (1 << direction) == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if !in_map(nx, ny) {
                    continue;
                }
                if let Some(corridor) = q.corridor {
                    let sector = (ny as u32 / super::sectors::SECTOR_SIZE) * sector_width
                        + nx as u32 / super::sectors::SECTOR_SIZE;
                    if !corridor[sector as usize] {
                        continue;
                    }
                }
                let next = ny as u32 * size + nx as u32;
                let j = next as usize;
                if !weighted && s.closed[j] == epoch {
                    continue;
                }
                let g2 = g + if dx != 0 && dy != 0 {
                    DIAGONAL_COST
                } else {
                    CARDINAL_COST
                };
                if s.seen[j] == epoch && g2 >= s.cost[j] {
                    continue;
                }
                let h = heuristic(nx, ny);
                if g2 + h > q.max_cost {
                    continue;
                }
                if let Some(blocked) = blocked
                    && (blocked.contains(next)
                        || (dx != 0
                            && dy != 0
                            && (blocked.contains((i64::from(id) + i64::from(dx)) as u32)
                                || blocked.contains(
                                    (i64::from(id) + i64::from(dy) * i64::from(size)) as u32,
                                ))))
                {
                    continue;
                }
                if !self.edge(id, next, 1 << direction, step) {
                    continue;
                }
                if weighted {
                    s.closed[j] = 0;
                }
                s.seen[j] = epoch;
                s.cost[j] = g2;
                s.prev[j] = id;
                let ph = priority(h);
                s.heap.push(Reverse((g2 + ph, ph, next)));
            }
        }
        // Only an exhaustive unrestricted search proves the whole reachable region.
        if q.remember_origin && q.corridor.is_none() && blocked.is_none() && q.max_cost == i64::MAX
        {
            let cells = (0..n as usize).map(|i| s.closed[i] == epoch).collect();
            self.origin = Some((start, cells));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walls(size: u32, blocked: &[u32]) -> impl FnMut(u32, u32) -> bool + '_ {
        let _ = size;
        move |_, b| !blocked.contains(&b)
    }

    #[test]
    fn paths_go_around_walls_and_cut_no_corners() {
        let size = 8;
        // A wall at x = 3, rows 0–5.
        let wall: Vec<u32> = (0..6).map(|y| y * size + 3).collect();
        let mut nav = GridNavigation::new(size);
        let mut scratch = Scratch::default();
        let mut step = walls(size, &wall);
        let path = nav
            .path(
                &mut scratch,
                &mut step,
                |_, _| true,
                1,
                6,
                Search::default(),
            )
            .expect("path");
        assert_eq!(path.last(), Some(&6));
        // Passes below the wall's end (y ≥ 6), never through it or diagonally past its tip.
        assert!(path.iter().all(|c| !wall.contains(c)));
        assert!(path.iter().any(|&c| c / size >= 6));
        // No diagonal squeezes past a wall corner.
        let cells: Vec<u32> = std::iter::once(1).chain(path.iter().copied()).collect();
        for w in cells.windows(2) {
            let (dx, dy) = (
                (w[1] % size) as i32 - (w[0] % size) as i32,
                (w[1] / size) as i32 - (w[0] / size) as i32,
            );
            if dx != 0 && dy != 0 {
                assert!(!wall.contains(&((w[0] as i32 + dx) as u32)));
                assert!(!wall.contains(&((w[0] as i32 + dy * size as i32) as u32)));
            }
        }
    }

    #[test]
    fn blocked_goals_and_closed_pockets_fail_fast() {
        let size = 8;
        let ring = [9, 10, 11, 17, 19, 25, 26, 27];
        let mut nav = GridNavigation::new(size);
        let mut scratch = Scratch::default();
        let mut step = walls(size, &ring);
        assert!(
            nav.path(
                &mut scratch,
                &mut step,
                |_, _| true,
                0,
                18,
                Search::default()
            )
            .is_none()
        );
        let mut set = CellSet::new(64);
        set.insert(40);
        let mut open = |_, _| true;
        let q = Search {
            blocked: Some(&set),
            ..Search::default()
        };
        assert!(
            nav.path(&mut scratch, &mut open, |_, _| true, 0, 40, q)
                .is_none()
        );
        // Unreachable regions are remembered by exhaustive searches.
        let mut nav = GridNavigation::new(size);
        let q = Search {
            remember_origin: true,
            probe_limit: 1,
            ..Search::default()
        };
        assert!(
            nav.path(&mut scratch, &mut step, |_, _| true, 0, 18, q)
                .is_none()
        );
    }
}
