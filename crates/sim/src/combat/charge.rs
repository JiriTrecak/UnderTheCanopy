//! Charges (old `charge.ts`): a unit with a charge rushes a unit target between its
//! minimum and maximum range, moving faster until the first blow, which hits harder.
//! Charging changes the movement budget only, never pathfinding or collision.

use super::{Charge, rules::Rules};
use crate::{
    entity::{Entity, EntityId},
    orders::Order,
    spatial::{Fixed, Spatial},
    state::State,
};

/// Ends a charge that no longer applies (old `maintainCharge`).
pub fn maintain(state: &mut State, id: EntityId) {
    let tick = state.tick;
    let Some(e) = state.entities.get(id) else {
        return;
    };
    let Some(u) = &e.unit else {
        return;
    };
    let Some(target) = u.combat.charge.as_ref().and_then(|c| c.target) else {
        return;
    };
    let charge = u.combat.charge.as_ref().expect("charge");
    let victim_alive = state
        .entities
        .get(target)
        .is_some_and(|t| t.hp.is_none_or(|hp| hp > 0));
    // M2.5 jobs, M2.6 stuns, casts and roots also end a charge.
    let ends = charge.expires <= tick
        || !victim_alive
        || u.target != Some(target)
        || u.contained.is_some()
        || u.camp.as_ref().is_some_and(|c| c.returning)
        || matches!(
            u.order,
            Some(Order::Move {
                attack_move: false,
                ..
            })
        )
        || matches!(u.order, Some(Order::Attack { target: t, .. }) if t != target);
    if ends {
        let u = state
            .entities
            .get_mut(id)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit");
        u.combat.charge.as_mut().expect("charge").target = None;
    }
}

/// Starts a charge at `target` when it is in the charge band and the way is clear (old
/// `startCharge`).
pub fn start(
    state: &mut State,
    rules: &Rules,
    spatial: &Spatial,
    id: EntityId,
    target: EntityId,
    research_cooldown_permille: &[u32],
) {
    let tick = state.tick;
    let (Some(e), Some(t)) = (state.entities.get(id), state.entities.get(target)) else {
        return;
    };
    let Some(policy) = rules.weapon(e.definition).and_then(|w| w.charge) else {
        return;
    };
    let u = e.unit.as_ref().expect("unit");
    if t.unit.is_none()
        || u.combat.charge.as_ref().is_some_and(|c| c.target.is_some())
        || u.combat.charge.as_ref().map_or(0, |c| c.ready_tick) > tick
        || u.route.is_empty()
    {
        return;
    }
    let distance = spatial.point_range(
        f64::from(e.position.x) / 1000.0,
        f64::from(e.position.y) / 1000.0,
        t,
    );
    let (min, max) = (f64::from(policy.min_range), f64::from(policy.max_range));
    if distance < min * min || distance > max * max {
        return;
    }
    // Not while following a detour behind a wall.
    let from = Fixed::on(e.position.x, e.position.y, spatial.deck_of(e));
    let to = Fixed::on(t.position.x, t.position.y, spatial.deck_of(t));
    if !spatial.clear_segment(from, to, None, &spatial.body_of(e)) {
        return;
    }
    let mut cooldown = policy.cooldown;
    for &permille in research_cooldown_permille {
        cooldown =
            (super::round_ratio(u64::from(cooldown), u64::from(permille), 1000) as u32).max(1);
    }
    let u = state
        .entities
        .get_mut(id)
        .expect("unit")
        .unit
        .as_mut()
        .expect("unit");
    u.combat.charge = Some(Charge {
        target: Some(target),
        expires: tick + policy.duration,
        ready_tick: tick + cooldown,
        speed_permille: policy.speed_permille,
    });
}

/// The research multipliers on a unit's charge cooldown (M2.5 research).
pub fn cooldown_research(state: &State, catalog: &crate::catalog::Catalog, e: &Entity) -> Vec<u32> {
    let id = catalog.id(e.definition);
    crate::stats::research(state, e.owner)
        .iter()
        .filter_map(|r| catalog.content.rules.research.get(r))
        .flat_map(|r| &r.effects)
        .filter(|effect| effect.units.contains(id))
        .filter_map(|effect| effect.charge_cooldown_permille)
        .collect()
}

/// Whether the unit is charging its current target (movement speed).
pub fn charging(e: &Entity, tick: u32) -> bool {
    e.unit.as_ref().is_some_and(|u| {
        u.combat
            .charge
            .as_ref()
            .is_some_and(|c| c.target.is_some() && c.target == u.target && c.expires > tick)
    })
}

/// The damage of a blow at `target`: the charge bonus on the charge's first blow, which
/// ends it (old `chargeDamage`). In thousandths.
pub fn damage(
    state: &mut State,
    rules: &Rules,
    id: EntityId,
    target: EntityId,
    base_milli: u64,
) -> u64 {
    let tick = state.tick;
    let e = state.entities.get_mut(id).expect("unit");
    let def = e.definition;
    let u = e.unit.as_mut().expect("unit");
    let Some(charge) = u.combat.charge.as_mut() else {
        return base_milli;
    };
    if charge.target != Some(target) || charge.expires <= tick {
        return base_milli;
    }
    charge.target = None;
    let permille = rules
        .weapon(def)
        .and_then(|w| w.charge)
        .map_or(1000, |c| c.damage_permille);
    // Old `Math.round(base · permille / 1000)` on whole points.
    let points = base_milli as f64 / 1000.0 * f64::from(permille) / 1000.0;
    super::round(points) as u64 * 1000
}
