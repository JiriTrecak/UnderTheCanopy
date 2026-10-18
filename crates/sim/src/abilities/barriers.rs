//! Field barriers (`status.barrier`): a curved pane in front of a unit that stops hostile
//! weapon missiles crossing it from the front, as a Warden's projector does.
//!
//! A missile is tested once, when it leaves: if its path from the shooter to the target
//! enters a hostile barrier's circle through the pane's arc, it flies only as far as the
//! pane and lands there without a hit. Shooters standing inside the pane's circle shoot
//! past it. Spells and shells are never stopped.

use super::{actor, statuses};
use crate::{
    combat::{Senses, projectiles::Missile},
    entity::{Entity, Milli},
    state::State,
};
use canopy_content::abilities::Barrier;

/// The widest barrier `e` holds now, if any.
pub fn barrier<'a>(senses: &Senses<'a>, e: &Entity, tick: u32) -> Option<&'a Barrier> {
    if e.statuses.is_empty() {
        return None;
    }
    e.statuses
        .iter()
        .filter(|s| s.expires > tick)
        .filter_map(|s| senses.catalog.spells.status(s.key).source.barrier.as_ref())
        .max_by(|a, b| a.radius.total_cmp(&b.radius))
}

/// Where along `origin → destination` (a share of the way) the path first enters the
/// barrier of a unit at `centre` facing `facing` (unit vector), if it enters through the
/// pane. All values are in cells.
pub fn entry(
    origin: (f64, f64),
    destination: (f64, f64),
    centre: (f64, f64),
    facing: (f64, f64),
    barrier: &Barrier,
) -> Option<f64> {
    let r = barrier.radius;
    let f = (origin.0 - centre.0, origin.1 - centre.1);
    let c = f.0 * f.0 + f.1 * f.1 - r * r;
    if c <= 0.0 {
        return None;
    }
    let v = (destination.0 - origin.0, destination.1 - origin.1);
    let a = v.0 * v.0 + v.1 * v.1;
    if a <= 0.0 {
        return None;
    }
    let b = 2.0 * (f.0 * v.0 + f.1 * v.1);
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    let p = (f.0 + t * v.0, f.1 + t * v.1);
    let cos_half = crate::math::cos((barrier.arc_degrees / 2.0).to_radians());
    ((p.0 * facing.0 + p.1 * facing.1) / r >= cos_half - 1e-9).then_some(t)
}

/// Stops `m` at the first hostile barrier its path crosses from the front (see the
/// module documentation).
pub fn intercept(senses: &Senses, state: &State, m: &mut Missile) {
    let tick = state.tick;
    let cells = |p: Milli| (f64::from(p.x) / 1000.0, f64::from(p.y) / 1000.0);
    let (origin, destination) = (cells(m.origin.position), cells(m.destination.position));
    let mut best: Option<(f64, Milli, Option<String>)> = None;
    for w in state.entities.iter() {
        if w.statuses.is_empty()
            || !w.hp.is_some_and(|hp| hp > 0)
            || w.unit.as_ref().is_none_or(|u| {
                u.contained.is_some() || u.garrison.is_some() || u.release.is_some()
            })
            || !senses.opponents(m.owner, None, w.owner, actor::camp(w))
        {
            continue;
        }
        let Some(b) = barrier(senses, w, tick) else {
            continue;
        };
        if statuses::stunned(senses.catalog, w) {
            continue;
        }
        let yaw = (f64::from(w.rotation) / 1000.0).to_radians();
        let facing = (crate::math::sin(yaw), crate::math::cos(yaw));
        let Some(t) = entry(origin, destination, cells(w.position), facing, b) else {
            continue;
        };
        if best.as_ref().is_some_and(|(bt, _, _)| *bt <= t) {
            continue;
        }
        let at = Milli {
            x: crate::terrain::round_half_up((origin.0 + t * (destination.0 - origin.0)) * 1000.0)
                as i32,
            y: crate::terrain::round_half_up((origin.1 + t * (destination.1 - origin.1)) * 1000.0)
                as i32,
        };
        best = Some((t, at, w.surface.clone()));
    }
    let Some((t, at, surface)) = best else {
        return;
    };
    let flight = m.impact.saturating_sub(m.launched);
    let elevation = m.origin.elevation_cm
        + crate::terrain::round_half_up(
            f64::from(m.destination.elevation_cm - m.origin.elevation_cm) * t,
        ) as i32;
    m.destination = crate::combat::projectiles::Aim {
        position: at,
        surface,
        elevation_cm: elevation,
    };
    m.impact = m.launched + ((f64::from(flight) * t).ceil() as u32).max(1);
    m.blocked = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_stops_shots_from_the_front_only() {
        let pane = Barrier {
            radius: 3.0,
            arc_degrees: 120.0,
        };
        // A unit at the origin facing +y; a shot from the front at a target behind it.
        let front = entry((0.0, 10.0), (0.0, -5.0), (0.0, 0.0), (0.0, 1.0), &pane);
        let t = front.expect("blocked");
        assert!((t - 7.0 / 15.0).abs() < 1e-9);
        // From behind it passes.
        assert!(entry((0.0, -10.0), (0.0, 5.0), (0.0, 0.0), (0.0, 1.0), &pane).is_none());
        // From the side, outside the arc, it passes.
        assert!(entry((10.0, 0.0), (-5.0, 0.0), (0.0, 0.0), (0.0, 1.0), &pane).is_none());
        // A shot that misses the circle passes, and a shooter inside shoots out.
        assert!(entry((10.0, 10.0), (10.0, -10.0), (0.0, 0.0), (0.0, 1.0), &pane).is_none());
        assert!(entry((0.0, 1.0), (0.0, 10.0), (0.0, 0.0), (0.0, 1.0), &pane).is_none());
    }
}
