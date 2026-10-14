//! Short-lived group route reuse (old `RouteCorridors`, `src/sim/game/routeCorridors.ts`).
//! One instance lives for one planning pass, so earlier ticks and saves never influence
//! route choice. Every reused segment is swept for the requesting body.

use crate::spatial::geometry::{Fixed, length};

/// Which bodies may share a corridor: same owner and the same body dimensions.
pub type CorridorKey = (u8, i64, u64);

struct Corridor {
    key: CorridorKey,
    from: Fixed,
    target: Fixed,
    route: Vec<u32>,
}

#[derive(Default)]
pub struct RouteCorridors {
    corridors: Vec<Corridor>,
}

fn distance(a: Fixed, b: Fixed) -> f64 {
    length(i64::from(a.x) - i64::from(b.x), i64::from(a.y) - i64::from(b.y))
}

impl RouteCorridors {
    pub fn remember(&mut self, key: CorridorKey, from: Fixed, target: Fixed, route: &[u32]) {
        if self.corridors.len() < 16 && !route.is_empty() && route.len() <= 64 {
            self.corridors.push(Corridor {
                key,
                from,
                target,
                route: route.to_vec(),
            });
        }
    }

    /// A remembered route that starts near `from`, ends near `target` and stays within 120%
    /// of the direct distance, re-aimed at `goal` and swept clear for this body.
    pub fn find(
        &self,
        key: CorridorKey,
        from: Fixed,
        target: Fixed,
        goal: u32,
        point: impl Fn(u32) -> Fixed,
        mut clear: impl FnMut(Fixed, Fixed) -> bool,
    ) -> Option<Vec<u32>> {
        let direct = distance(from, target);
        if direct < 16000.0 {
            return None;
        }
        for candidate in &self.corridors {
            if candidate.key != key
                || distance(from, candidate.from) > 16000.0
                || distance(target, candidate.target) > 8000.0
            {
                continue;
            }
            let mut route = candidate.route.clone();
            let last = route.len() - 1;
            if last > 0 && clear(point(route[last - 1]), target) {
                route[last] = goal;
            } else if route[last] != goal {
                route.push(goal);
            }
            let mut total = 0.0;
            let mut anchor = from;
            for &cell in &route {
                let next = point(cell);
                total += distance(anchor, next);
                anchor = next;
            }
            if total > direct * 1.2 {
                continue;
            }
            anchor = from;
            let mut valid = true;
            for &cell in &route {
                let next = point(cell);
                if !clear(anchor, next) {
                    valid = false;
                    break;
                }
                anchor = next;
            }
            if valid {
                return Some(route);
            }
        }
        None
    }
}
