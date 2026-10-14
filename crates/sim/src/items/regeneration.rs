//! Health regeneration (old `regeneration.ts`): every tick a unit adds its regeneration in
//! milli-points per second to an integer accumulator; 40,000 make a point. Lapsed flight
//! forms land and pools follow the rebuilt auras first.

use crate::{
    abilities::{Ctx, actor, forms},
    entity::EntityId,
    state::State,
    stats::Stats,
};

/// Accumulator units per point of health.
pub const POINT: u32 = 40_000;

/// Step 3 (old `Regeneration.tick`).
pub fn tick(ctx: &mut Ctx, state: &mut State) {
    let tick = state.tick;
    let units: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| e.unit.is_some() && actor::alive(e))
        .map(|e| e.id)
        .collect();
    for id in units {
        forms::reconcile_flight(ctx, state, id);
        forms::reconcile_weapon(ctx.catalog(), state, id);
        let e = state.entities.get(id).expect("unit");
        let stats = Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner));
        let e = state.entities.get_mut(id).expect("unit");
        if let Some(hp) = &mut e.hp {
            *hp = (*hp).min(stats.max_hp);
        }
        let held = e
            .unit
            .as_ref()
            .is_some_and(|u| u.contained.is_some() || u.release.is_some());
        if e.ready_tick > tick || held {
            continue;
        }
        let Some(hp) = e.hp else {
            continue;
        };
        let u = e.unit.as_mut().expect("unit");
        let mut hp = hp;
        if hp < stats.max_hp {
            u.regeneration +=
                crate::abilities::cast::js_round(stats.health_regen * 1000.0).max(0.0) as u32;
            hp = stats.max_hp.min(hp + u.regeneration / POINT);
            u.regeneration %= POINT;
        }
        if hp == stats.max_hp {
            u.regeneration = 0;
        }
        e.hp = Some(hp);
    }
}
