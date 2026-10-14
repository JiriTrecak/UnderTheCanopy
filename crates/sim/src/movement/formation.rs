//! Group move destinations (old `formationDestinations`, `src/sim/game/formation.ts`):
//! compact walkable slots around the destination, assigned spatially rather than by
//! selection order, with pair exchanges that reduce travel and crossing.

use crate::{
    entity::EntityId,
    spatial::{Fixed, Spot, geometry::grid},
    terrain::DeckId,
};
use std::collections::BTreeMap;

/// A unit taking part: its id and precise position in milli-cells.
#[derive(Debug, Clone, Copy)]
pub struct Member {
    pub id: EntityId,
    pub x: i32,
    pub y: i32,
    pub deck: Option<DeckId>,
}

/// Destination cells for `members` around `destination`. `walkable` says whether the group
/// body fits a cell; `straight` whether a member can walk straight to it.
pub fn formation_destinations(
    members: &[Member],
    destination: Spot,
    size: u32,
    walkable: impl Fn(Spot) -> bool,
    straight: impl Fn(Fixed, Spot) -> bool,
    spacing: f64,
) -> BTreeMap<EntityId, Spot> {
    let mut result = BTreeMap::new();
    if members.is_empty() {
        return result;
    }
    let n = members.len();
    let size = size as i32;
    let separated = |a: Spot, b: Spot| {
        let (dx, dy) = (f64::from(a.x - b.x), f64::from(a.y - b.y));
        dx * dx + dy * dy >= spacing * spacing - 1e-9
    };
    let in_map = |p: Spot| p.x >= 0 && p.y >= 0 && p.x < size && p.y < size;
    let mut actors = members.to_vec();
    actors.sort_by_key(|a| (a.x, a.y, a.id));
    // Keep an already compact group's shape through a clear translation.
    if n > 1 {
        let rounded: Vec<(i32, i32)> = actors.iter().map(|a| (grid(a.x), grid(a.y))).collect();
        let (min_x, max_x) = (
            rounded.iter().map(|p| p.0).min().unwrap_or(0),
            rounded.iter().map(|p| p.0).max().unwrap_or(0),
        );
        let (min_y, max_y) = (
            rounded.iter().map(|p| p.1).min().unwrap_or(0),
            rounded.iter().map(|p| p.1).max().unwrap_or(0),
        );
        let (width, height) = (f64::from(max_x - min_x + 1), f64::from(max_y - min_y + 1));
        let max_span = (n as f64).sqrt().ceil() * 2.0 * spacing.max(1.0);
        if width * height <= n as f64 * 2.0 * (spacing * spacing).max(1.0)
            && width <= max_span
            && height <= max_span
        {
            let center = |sum: i64| {
                // round(sum / 1000 / n), halves up.
                let d = 1000 * n as i64;
                (2 * sum + d).div_euclid(2 * d) as i32
            };
            let cx = center(actors.iter().map(|a| i64::from(a.x)).sum());
            let cy = center(actors.iter().map(|a| i64::from(a.y)).sum());
            let translated: Vec<Spot> = rounded
                .iter()
                .map(|&(x, y)| {
                    Spot::new(
                        x + destination.x - cx,
                        y + destination.y - cy,
                        destination.deck,
                    )
                })
                .collect();
            let mut cells: Vec<(i32, i32)> = translated.iter().map(|p| (p.x, p.y)).collect();
            cells.sort_unstable();
            cells.dedup();
            let fits = cells.len() == n
                && translated
                    .iter()
                    .enumerate()
                    .all(|(i, &p)| translated[..i].iter().all(|&q| separated(p, q)))
                && translated.iter().enumerate().all(|(i, &p)| {
                    in_map(p)
                        && walkable(p)
                        && straight(Fixed::on(actors[i].x, actors[i].y, actors[i].deck), p)
                });
            if fits {
                for (a, p) in actors.iter().zip(translated) {
                    result.insert(a.id, p);
                }
                return result;
            }
        }
    }
    let mut slots: Vec<Spot> = Vec::with_capacity(n);
    let search_radius =
        f64::from(size - 1).min(12f64.max((n as f64).sqrt().ceil() * 3.0 * spacing.max(1.0)));
    let mut radius = 0;
    while slots.len() < n && f64::from(radius) <= search_radius {
        let mut ring: Vec<Spot> = Vec::new();
        let mut consider = |x: i32, y: i32| {
            let p = Spot::new(destination.x + x, destination.y + y, destination.deck);
            if in_map(p) && walkable(p) {
                ring.push(p);
            }
        };
        if radius == 0 {
            consider(0, 0);
        } else {
            for x in -radius..=radius {
                consider(x, -radius);
                consider(x, radius);
            }
            for y in 1 - radius..radius {
                consider(-radius, y);
                consider(radius, y);
            }
        }
        let d2 = |p: &Spot| {
            let (dx, dy) = (
                i64::from(p.x - destination.x),
                i64::from(p.y - destination.y),
            );
            dx * dx + dy * dy
        };
        ring.sort_by(|a, b| d2(a).cmp(&d2(b)).then(a.y.cmp(&b.y)).then(a.x.cmp(&b.x)));
        for candidate in ring {
            if slots.len() == n {
                break;
            }
            if slots.iter().all(|&p| separated(p, candidate)) {
                slots.push(candidate);
            }
        }
        radius += 1;
    }
    slots.sort_by_key(|p| (p.x, p.y));
    let cost = |a: &Member, b: Spot| {
        let (dx, dy) = (
            i64::from(a.x) - i64::from(b.x) * 1000,
            i64::from(a.y) - i64::from(b.y) * 1000,
        );
        dx * dx + dy * dy
    };
    let count = n.min(slots.len());
    for _ in 0..3 {
        let mut changed = false;
        for i in 0..count {
            for j in i + 1..count {
                if cost(&actors[i], slots[j]) + cost(&actors[j], slots[i])
                    < cost(&actors[i], slots[i]) + cost(&actors[j], slots[j])
                {
                    slots.swap(i, j);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for i in 0..count {
        result.insert(actors[i].id, slots[i]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: u32, x: i32, y: i32) -> Member {
        Member {
            id: EntityId(id),
            x: x * 1000,
            y: y * 1000,
            deck: None,
        }
    }

    #[test]
    fn compact_groups_translate_and_scattered_ones_fill_rings() {
        let compact = [member(1, 10, 10), member(2, 11, 10), member(3, 10, 11)];
        let slots = formation_destinations(
            &compact,
            Spot::new(50, 50, None),
            64,
            |_| true,
            |_, _| true,
            1.0,
        );
        assert_eq!(slots[&EntityId(1)], Spot::new(50, 50, None));
        assert_eq!(slots[&EntityId(2)], Spot::new(51, 50, None));
        let scattered = [member(1, 0, 0), member(2, 40, 40), member(3, 5, 60)];
        let slots = formation_destinations(
            &scattered,
            Spot::new(30, 30, None),
            64,
            |p| p.x != 30,
            |_, _| true,
            1.0,
        );
        assert_eq!(slots.len(), 3);
        assert!(
            slots
                .values()
                .all(|p| p.x != 30 && (p.x - 30).abs() <= 1 && (p.y - 30).abs() <= 1)
        );
        // Spacing 2 keeps slots two cells apart.
        let slots = formation_destinations(
            &scattered,
            Spot::new(30, 30, None),
            64,
            |_| true,
            |_, _| true,
            2.0,
        );
        let spots: Vec<Spot> = slots.values().copied().collect();
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[..i] {
                assert!((a.x - b.x).pow(2) + (a.y - b.y).pow(2) >= 4);
            }
        }
    }
}
