//! What the mission API reads from systems that later build steps add (orders, items,
//! heroes, containment). [`Current`] answers for the state that exists today; the step that
//! adds a system fills in its method.

use crate::{
    catalog::Catalog,
    entity::{Entity, Milli},
    math,
};
use canopy_content::Id;

pub trait Systems {
    /// The unit is carrying out an order (M2.2).
    fn has_order(&self, entity: &Entity) -> bool;
    /// Hero item slots (M2.6); `None` for entities without an inventory.
    fn equipment(&self, entity: &Entity) -> Option<Vec<Option<Id>>>;
    /// Inside a garrison or containing ability, or being released from one.
    fn contained(&self, entity: &Entity) -> bool;
    /// A fallen hero awaiting revival (M2.4).
    fn fallen(&self, entity: &Entity) -> bool;
    /// Maximum health with levels, bonuses and upgrades applied.
    fn max_hp(&self, catalog: &Catalog, entity: &Entity) -> u32;
}

/// The systems as built so far: orders, lookouts and held units, fallen heroes and
/// levelled health; no equipment yet (M2.6).
pub struct Current;

impl Systems for Current {
    fn has_order(&self, entity: &Entity) -> bool {
        entity.unit.as_ref().is_some_and(|u| u.order.is_some())
    }

    fn equipment(&self, _: &Entity) -> Option<Vec<Option<Id>>> {
        None
    }

    fn contained(&self, entity: &Entity) -> bool {
        entity
            .unit
            .as_ref()
            .is_some_and(|u| u.contained.is_some() || u.release.is_some())
    }

    fn fallen(&self, entity: &Entity) -> bool {
        crate::heroes::fallen(entity)
    }

    fn max_hp(&self, catalog: &Catalog, entity: &Entity) -> u32 {
        crate::stats::max_hp(catalog, entity)
    }
}

/// Degrees per second when a definition does not say.
pub const DEFAULT_TURN_RATE: u32 = 720;

/// Turns `entity` one tick toward `target` at `turn_rate` degrees per second. True once it
/// faces the target (within a degree beyond the last step).
pub fn turn_toward(entity: &mut Entity, target: Milli, turn_rate: u32) -> bool {
    let rate = i64::from(turn_rate) * 1000 / i64::from(crate::mission::TICKS_PER_SECOND);
    let rotation = i64::from(entity.rotation);
    let difference = turn_difference(rotation, heading(entity.position, rotation, target));
    entity.rotation = (rotation + difference.clamp(-rate, rate)).rem_euclid(FULL_TURN) as i32;
    difference.abs() <= rate + 1000
}

/// Millidegrees in a turn.
const FULL_TURN: i64 = 360_000;

/// World yaw toward `to` in millidegrees (+y forward, clockwise); `rotation` when already
/// there.
fn heading(from: Milli, rotation: i64, to: Milli) -> i64 {
    let dx = f64::from(to.x - from.x) / 1000.0;
    let dy = f64::from(to.y - from.y) / 1000.0;
    if dx * dx + dy * dy < 1e-10 {
        return rotation;
    }
    let degrees = (math::atan2(dx, dy).to_degrees() + 360.0) % 360.0;
    ((degrees * 1000.0).round() as i64).rem_euclid(FULL_TURN)
}

/// The shortest signed turn from `from` to `to`; a half turn goes clockwise.
fn turn_difference(from: i64, to: i64) -> i64 {
    let difference = (to - from).rem_euclid(FULL_TURN);
    if difference > FULL_TURN / 2 {
        difference - FULL_TURN
    } else {
        difference
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_follow_the_world_yaw() {
        let origin = Milli { x: 0, y: 0 };
        assert_eq!(heading(origin, 5, Milli { x: 0, y: 1000 }), 0);
        assert_eq!(heading(origin, 5, Milli { x: 1000, y: 0 }), 90_000);
        assert_eq!(heading(origin, 5, Milli { x: 0, y: -1000 }), 180_000);
        assert_eq!(heading(origin, 5, Milli { x: -1000, y: 0 }), 270_000);
        assert_eq!(heading(origin, 5, origin), 5);
        assert_eq!(turn_difference(350_000, 10_000), 20_000);
        assert_eq!(turn_difference(10_000, 350_000), -20_000);
        assert_eq!(turn_difference(0, 180_000), 180_000);
    }
}
