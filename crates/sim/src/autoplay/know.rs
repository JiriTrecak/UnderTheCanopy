//! What player 1 knows of the world, as the plan may ask it: its own units fully, others
//! only while seen (or, for buildings, as remembered under the fog), and the map's
//! placements where the map put them.

use super::PLAYER;
use crate::{World, entity::Entity, stats};
use canopy_content::placement::Owner;

pub const ME: Owner = Owner::Player(PLAYER);

pub fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0) && !e.fallen
}

/// Cells of an entity's position.
pub fn cells(e: &Entity) -> (f64, f64) {
    (
        f64::from(e.position.x) / 1000.0,
        f64::from(e.position.y) / 1000.0,
    )
}

pub fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Whether player 1 knows `e` now: its own, in sight, or a building it remembers.
pub fn known(world: &World, e: &Entity) -> bool {
    e.owner == ME || {
        let sight = world.sight();
        sight.seen(ME, e) || sight.remembered(ME, e.id).is_some()
    }
}

/// The entity of a placement as player 1 knows it: the living one first.
pub fn placement<'a>(world: &'a World, id: &str) -> Option<&'a Entity> {
    let mut found = None;
    for e in world
        .state()
        .entities
        .iter()
        .filter(|e| e.placement.as_deref() == Some(id))
    {
        if alive(e) {
            return known(world, e).then_some(e);
        }
        found = found.or(Some(e));
    }
    found.filter(|e| known(world, e))
}

/// Player 1's own living placement.
pub fn own<'a>(world: &'a World, id: &str) -> Option<&'a Entity> {
    placement(world, id).filter(|e| e.owner == ME && alive(e))
}

/// Player 1's fighting units: living, in the open, under its command, able to fight, not
/// workers, and not among `kept` (sorted ids the plan holds apart). Heroes too.
pub fn army<'a>(world: &'a World, kept: &[u32]) -> Vec<&'a Entity> {
    let catalog = world.catalog();
    world
        .state()
        .entities
        .iter()
        .filter(|e| e.owner == ME && alive(e) && kept.binary_search(&e.id.0).is_err())
        .filter(|e| e.unit.as_ref().is_some_and(|u| u.contained.is_none()))
        .filter(|e| {
            let b = &catalog.get(e.definition).behaviors;
            b.combat.is_some() && b.work.is_none() && b.player_control.is_some()
        })
        .collect()
}

pub fn ids(units: &[&Entity]) -> Vec<u32> {
    units.iter().map(|e| e.id.0).collect()
}

/// The middle of a group: the median cell on each axis (stragglers do not pull it).
pub fn middle(units: &[&Entity]) -> Option<(f64, f64)> {
    if units.is_empty() {
        return None;
    }
    let mut xs: Vec<i32> = units.iter().map(|e| e.position.x).collect();
    let mut zs: Vec<i32> = units.iter().map(|e| e.position.y).collect();
    xs.sort_unstable();
    zs.sort_unstable();
    let m = units.len() / 2;
    Some((f64::from(xs[m]) / 1000.0, f64::from(zs[m]) / 1000.0))
}

/// Health as a share of the most it can have.
pub fn health(world: &World, e: &Entity) -> f64 {
    let max = stats::max_hp(world.catalog(), e).max(1);
    f64::from(e.hp.unwrap_or(0)) / f64::from(max)
}

/// Whether the entity is a hero.
pub fn hero(world: &World, e: &Entity) -> bool {
    world.catalog().get(e.definition).hero == Some(true)
}

/// Whether player 1 has the ground at a cell in sight now.
pub fn in_sight(world: &World, (x, z): (f64, f64)) -> bool {
    let Some(cell) = world.terrain().cell(x.floor() as i64, z.floor() as i64) else {
        return false;
    };
    world
        .sight()
        .cells(ME)
        .is_some_and(|c| c.get(cell as u32) == crate::vision::knowledge::VISIBLE)
}
