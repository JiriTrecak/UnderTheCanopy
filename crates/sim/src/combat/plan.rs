//! Step 5: order and combat planning (old `Combat.plan`/`planUnits`). One pass over the
//! ready units on the map, in id order, decides what each does this tick; later units see
//! the routes earlier ones chose.

use super::{
    APPROACH_RETRY_TICKS, Pursuit, STAGGER, approach, charge, projectiles,
    resolve::begin_attack,
    senses::{Senses, SightCache},
    targets::TargetIndex,
};
use crate::{
    camps,
    entity::{Entity, EntityId, Place},
    orders::{Order, discard_navigation, finish_combat, plan as orders, release_combat},
    spatial::{Spatial, Spot},
    state::State,
};
use std::collections::BTreeSet;

/// Scratch kept between passes (derived, never saved).
#[derive(Debug, Default)]
pub struct Scratch {
    pub targets: TargetIndex,
    pub sight: SightCache,
    candidates: Vec<EntityId>,
}

fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
}

fn unit(state: &State, id: EntityId) -> &crate::entity::Unit {
    state
        .entities
        .get(id)
        .expect("unit")
        .unit
        .as_ref()
        .expect("unit")
}

fn unit_mut(state: &mut State, id: EntityId) -> &mut crate::entity::Unit {
    state
        .entities
        .get_mut(id)
        .expect("unit")
        .unit
        .as_mut()
        .expect("unit")
}

/// Plans every unit (step 5, after garrisons). Units in a lookout that is `unpowered`
/// (D66) hold their fire.
pub fn plan(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    scratch: &mut Scratch,
    unpowered: &BTreeSet<EntityId>,
) {
    let _span = tracing::trace_span!("combat planning").entered();
    spatial.sync_bodies(state);
    projectiles::expire(state);
    {
        let _span = tracing::trace_span!("target index").entered();
        scratch
            .targets
            .refresh(state, senses.catalog, spatial.size());
    }
    scratch.sight.begin(state.counters.entity as usize);
    let senses = Senses {
        cache: Some(&scratch.sight),
        ..*senses
    };
    let tick = state.tick;
    let units: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| alive(e) && e.ready_tick <= tick && crate::spatial::present(e))
        .map(|e| e.id)
        .collect();
    let targets = &mut scratch.targets;
    let candidates = &mut scratch.candidates;
    spatial.with_shared_routes(|spatial| {
        for id in units {
            plan_unit(&senses, spatial, state, targets, candidates, id, unpowered);
        }
    });
}

fn plan_unit(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    targets: &mut TargetIndex,
    candidates: &mut Vec<EntityId>,
    id: EntityId,
    unpowered: &BTreeSet<EntityId>,
) {
    let tick = state.tick;
    // A lookout without power is dark: whoever stands in it neither aims nor shoots
    // until power returns (D66). Its orders stay.
    if unit(state, id)
        .garrison
        .as_ref()
        .is_some_and(|g| unpowered.contains(&g.building))
    {
        let u = unit_mut(state, id);
        crate::orders::suspend_combat(u, tick);
        u.combat.cooldown = u.combat.cooldown.saturating_sub(1);
        spatial.update_body(state.entities.get(id).expect("unit"));
        return;
    }
    charge::maintain(senses, state, id);
    let e = state.entities.get(id).expect("unit");
    let weapon = senses.weapon(e).copied();
    // A swing in progress holds the unit until it is released.
    if let Some(attack) = &unit(state, id).combat.attack {
        let target = attack.target;
        let victim = state.entities.get(target).filter(|v| alive(v));
        let seen = victim.is_some_and(|v| senses.perceives(state, e, v));
        if seen {
            remember(state, id, target);
        }
        let victim = state.entities.get(target).filter(|v| alive(v));
        let u = unit(state, id);
        let attack = u.combat.attack.as_ref().expect("attack");
        let e = state.entities.get(id).expect("unit");
        let ends = tick >= attack.ends
            || victim.is_none_or(|v| !senses.weapon_eligible(e, v))
            || u.target != Some(attack.target)
            || super::resolve::standing_down(senses, state, e)
            || !seen;
        let u = unit_mut(state, id);
        if ends {
            u.combat.attack = None;
        } else if !u.combat.attack.as_ref().expect("attack").released {
            discard_navigation(u);
            u.combat.cooldown = u.combat.cooldown.saturating_sub(1);
            spatial.update_body(state.entities.get(id).expect("unit"));
            return;
        }
    }
    let order = unit(state, id).order.clone();
    // Casting, walking to cast, charging by an ability and stunned units plan nothing.
    {
        let e = state.entities.get(id).expect("unit");
        if e.caster.as_ref().is_some_and(|c| c.pending.is_some())
            || matches!(order, Some(Order::CastAbility { .. }))
            || crate::abilities::deliveries::charging(senses.catalog, state, id)
            || crate::abilities::statuses::stunned(senses.catalog, e)
        {
            return;
        }
    }
    // A taunted unit fights its taunter (old taunt branch of `planUnits`).
    if let Some(taunt) = unit(state, id).taunt {
        taunted(senses, spatial, state, id, taunt.source, weapon);
        return;
    }
    {
        let e = state.entities.get(id).expect("unit");
        if crate::abilities::statuses::disarmed(senses.catalog, e)
            || crate::abilities::statuses::ethereal(senses.catalog, e)
        {
            crate::orders::suspend_combat(unit_mut(state, id), tick);
            if !matches!(
                order,
                Some(Order::Move { .. } | Order::Patrol { .. } | Order::Follow { .. })
            ) {
                spatial.update_body(state.entities.get(id).expect("unit"));
                return;
            }
        }
    }
    // Workers on a job, and work, pickup and garrison orders, move as their system plans.
    if unit(state, id).work.job.is_some() || order.as_ref().is_some_and(Order::stands_down) {
        release_combat(unit_mut(state, id));
        return;
    }
    {
        let u = unit_mut(state, id);
        u.combat.cooldown = u.combat.cooldown.saturating_sub(1);
    }
    // Camp members past the leash walk home and fight nothing on the way.
    let e = state.entities.get(id).expect("unit");
    if let Some(camp) = senses.camps.of(e) {
        let leash = f64::from(camp.leash);
        let distance = camps::home_distance(camp, e);
        let returning = unit(state, id).camp.as_ref().is_some_and(|c| c.returning);
        if distance > leash * leash || returning {
            let u = unit_mut(state, id);
            if !returning {
                discard_navigation(u);
                u.retry_at = tick;
            }
            release_combat(u);
            u.camp.as_mut().expect("camp").returning = true;
            u.order = None;
            if distance <= 1.0 {
                u.camp.as_mut().expect("camp").returning = false;
                discard_navigation(u);
            } else if u.route.is_empty() && u.retry_at <= tick {
                if let Some(p) = camps::home_spot(spatial, camp, id) {
                    let e = state.entities.get_mut(id).expect("unit");
                    spatial.route_default(e, p);
                }
                unit_mut(state, id).retry_at = tick + camps::RETURN_RETRY_TICKS;
            }
            spatial.update_body(state.entities.get(id).expect("unit"));
            return;
        }
    }
    let mut order = order;
    if let Some(Order::Patrol { origin, .. }) = &mut order
        && origin.is_none()
    {
        let (x, y) = state.entities.get(id).expect("unit").position.cell();
        *origin = Some(Place::new(x, y));
        unit_mut(state, id).order = order.clone();
    }
    match &order {
        Some(Order::Move {
            destination,
            attack_move: false,
        }) => {
            release_combat(unit_mut(state, id));
            orders::move_order(spatial, state, senses, id, destination);
            return;
        }
        Some(Order::Follow { target, escort }) => {
            orders::follow(spatial, state, id, *target, *escort);
            return;
        }
        _ => {}
    }
    let holding = matches!(order, Some(Order::Hold)) || unit(state, id).garrison.is_some();
    if holding {
        let e = state.entities.get(id).expect("unit");
        let out_of_reach = unit(state, id).target.is_some_and(|t| {
            state
                .entities
                .get(t)
                .is_none_or(|t| weapon.is_none_or(|w| spatial.body_range(e, t) > w.range * w.range))
        });
        let u = unit_mut(state, id);
        discard_navigation(u);
        if out_of_reach {
            finish_combat(u, tick);
        }
    }
    let u = unit(state, id);
    let mut target = match &order {
        Some(Order::Attack { target, .. }) => Some(*target),
        _ => u.target.or(if holding {
            None
        } else {
            u.combat.pursuit.as_ref().map(|p| p.target)
        }),
    };
    if let Some(w) = weapon
        && !holding
        && u.combat.pursuit.is_some()
    {
        let e = state.entities.get(id).expect("unit");
        let lost = target
            .and_then(|t| state.entities.get(t))
            .is_none_or(|t| !senses.perceives(state, e, t));
        if lost {
            let replacement = match order {
                Some(Order::Attack { .. }) => None,
                _ => closest(
                    senses,
                    spatial,
                    state,
                    targets,
                    candidates,
                    id,
                    f64::from(w.aggro),
                    false,
                ),
            };
            match replacement {
                Some(r) => {
                    target = Some(r);
                    let u = unit_mut(state, id);
                    u.combat.pursuit = None;
                    discard_navigation(u);
                    u.retry_at = tick;
                }
                None => {
                    search_last_seen(senses, spatial, state, id);
                    spatial.update_body(state.entities.get(id).expect("unit"));
                    return;
                }
            }
        }
    }
    let e = state.entities.get(id).expect("unit");
    let forced = matches!(order, Some(Order::Attack { force: true, .. }));
    let invalid = match target.map(|t| state.entities.get(t)) {
        None => unit(state, id).target.is_some(),
        Some(None) => true,
        Some(Some(t)) => {
            !alive(t)
                || t.hp.is_none()
                || t.unit.as_ref().is_some_and(|u| {
                    u.contained.is_some() || u.garrison.is_some() || u.release.is_some()
                })
                || !senses.weapon_eligible(e, t)
                || !senses.perceives(state, e, t)
                || (!forced && !senses.hostile(e, t))
        }
    };
    if invalid {
        target = None;
        finish_combat(unit_mut(state, id), tick);
    }
    let staggered = tick % STAGGER == id.0 % STAGGER;
    // Automatic pursuit must not pull a crowded front line past an enemy it can already
    // hit; explicit focus and committed swings stay.
    if let (Some(t), Some(w)) = (target, weapon)
        && !matches!(order, Some(Order::Attack { .. }))
        && unit(state, id).combat.attack.is_none()
        && staggered
    {
        let e = state.entities.get(id).expect("unit");
        let te = state.entities.get(t).expect("target");
        if spatial.body_range(e, te) > w.range * w.range
            && let Some(near) = closest(
                senses, spatial, state, targets, candidates, id, w.range, true,
            )
        {
            target = Some(near);
            let u = unit_mut(state, id);
            u.target = Some(near);
            u.combat.pursuit = None;
            discard_navigation(u);
            u.retry_at = tick;
        }
    }
    // A witnessed death or an exhausted search completes an explicit attack.
    if target.is_none() && matches!(order, Some(Order::Attack { .. })) {
        finish_combat(unit_mut(state, id), tick);
        spatial.update_body(state.entities.get(id).expect("unit"));
        return;
    }
    if target.is_none()
        && let Some(w) = weapon
        && (staggered
            || matches!(
                order,
                Some(Order::Move {
                    attack_move: true,
                    ..
                })
            ))
    {
        let range = if holding { w.range } else { f64::from(w.aggro) };
        target = closest(
            senses, spatial, state, targets, candidates, id, range, holding,
        );
    }
    if let (Some(t), Some(w)) = (target, weapon) {
        {
            let u = unit_mut(state, id);
            if u.target.is_none() && u.combat.pursuit.is_some() {
                discard_navigation(u);
                u.retry_at = tick;
            }
        }
        remember(state, id, t);
        unit_mut(state, id).target = Some(t);
        charge::maintain(senses, state, id);
        let (e, te) = (
            state.entities.get(id).expect("unit"),
            state.entities.get(t).expect("target"),
        );
        if spatial.body_range(e, te) <= w.range * w.range && senses.terrain_clear(state, e, te) {
            let u = unit_mut(state, id);
            discard_navigation(u);
            if u.combat.attack.is_none() && u.combat.cooldown == 0 {
                begin_attack(senses, state, id, t);
            }
            spatial.update_body(state.entities.get(id).expect("unit"));
            return;
        }
        if holding {
            unit_mut(state, id).target = None;
            spatial.update_body(state.entities.get(id).expect("unit"));
            return;
        }
        let u = unit(state, id);
        if u.retry_at <= tick {
            // An inherited goal may be in reach but taken by the victim or a front-line
            // ally: choose a usable attack position again.
            let goal = if u.route.is_empty() { None } else { u.goal };
            let stale = goal.is_some_and(|g| {
                approach::stale_goal(senses, spatial, state, id, t, spatial.spot(g))
            });
            if u.route.is_empty() || stale {
                approach::route_to_attack(senses, spatial, state, id, t);
                unit_mut(state, id).retry_at = tick + APPROACH_RETRY_TICKS;
            }
        }
        let research =
            charge::cooldown_research(state, senses.catalog, state.entities.get(id).expect("unit"));
        charge::start(state, senses, spatial, id, t, &research);
        spatial.update_body(state.entities.get(id).expect("unit"));
        return;
    }
    match order {
        Some(Order::Move { destination, .. }) => {
            orders::move_order(spatial, state, senses, id, &destination);
        }
        Some(Order::Patrol {
            destination,
            origin,
        }) => orders::patrol(spatial, state, senses, id, destination, origin),
        _ => spatial.update_body(state.entities.get(id).expect("unit")),
    }
}

/// The taunted unit's plan (old taunt branch): engage the taunter when in reach, else
/// approach it.
fn taunted(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    id: EntityId,
    source: EntityId,
    weapon: Option<super::rules::Weapon>,
) {
    let tick = state.tick;
    {
        let u = unit_mut(state, id);
        u.combat.cooldown = u.combat.cooldown.saturating_sub(1);
    }
    let e = state.entities.get(id).expect("unit");
    let target = state.entities.get(source).filter(|t| alive(t));
    let usable = weapon.is_some()
        && target.is_some_and(|t| senses.weapon_eligible(e, t) && senses.perceives(state, e, t))
        && !crate::abilities::statuses::disarmed(senses.catalog, e)
        && !crate::abilities::statuses::ethereal(senses.catalog, e);
    if !usable {
        let u = unit_mut(state, id);
        discard_navigation(u);
        release_combat(u);
        spatial.update_body(state.entities.get(id).expect("unit"));
        return;
    }
    let w = weapon.expect("weapon");
    unit_mut(state, id).target = Some(source);
    remember(state, id, source);
    let (e, t) = (
        state.entities.get(id).expect("unit"),
        state.entities.get(source).expect("taunter"),
    );
    if spatial.body_range(e, t) <= w.range * w.range && senses.terrain_clear(state, e, t) {
        let u = unit_mut(state, id);
        discard_navigation(u);
        if u.combat.attack.is_none() && u.combat.cooldown == 0 {
            begin_attack(senses, state, id, source);
        }
    } else if unit(state, id).retry_at <= tick {
        approach::route_to_attack(senses, spatial, state, id, source);
        unit_mut(state, id).retry_at = tick + APPROACH_RETRY_TICKS;
    }
    spatial.update_body(state.entities.get(id).expect("unit"));
}

/// Records where `target` is seen now (old `rememberTarget`).
fn remember(state: &mut State, id: EntityId, target: EntityId) {
    let tick = state.tick;
    let t = state.entities.get(target).expect("target");
    let (x, y) = t.position.cell();
    let position = Place {
        x,
        y,
        surface: t.surface.clone(),
    };
    unit_mut(state, id).combat.pursuit = Some(Pursuit {
        target,
        position,
        seen_tick: tick,
    });
}

/// The nearest hostile body the unit's weapon can hit and that it perceives, by distance
/// then id (old `closestTarget`). With `weapon_reach` distances are between body edges
/// (in reach of the weapon), else from the unit's centre.
#[allow(clippy::too_many_arguments)]
fn closest(
    senses: &Senses,
    spatial: &Spatial,
    state: &State,
    targets: &mut TargetIndex,
    candidates: &mut Vec<EntityId>,
    id: EntityId,
    range: f64,
    weapon_reach: bool,
) -> Option<EntityId> {
    let _span = tracing::trace_span!("target acquisition").entered();
    let actor = state.entities.get(id).expect("unit");
    let radius = if weapon_reach {
        spatial.body_of(actor).radius
    } else {
        0.0
    };
    targets.near(actor.position, range + radius, candidates);
    let (x, y) = (
        f64::from(actor.position.x) / 1000.0,
        f64::from(actor.position.y) / 1000.0,
    );
    let mut best: Option<(f64, EntityId)> = None;
    let mut best_distance = range * range;
    for &candidate in candidates.iter() {
        let t = state.entities.get(candidate).expect("indexed");
        if !senses.hostile(actor, t) {
            continue;
        }
        let distance = if weapon_reach {
            spatial.body_range(actor, t)
        } else {
            spatial.point_range(x, y, t)
        };
        if distance > best_distance
            || best.is_some_and(|(_, b)| distance == best_distance && candidate >= b)
        {
            continue;
        }
        if !senses.weapon_eligible(actor, t) || !senses.perceives(state, actor, t) {
            continue;
        }
        best = Some((distance, candidate));
        best_distance = distance;
    }
    best.map(|(_, id)| id)
}

/// Walks to where a lost target was last seen, then gives up (old `searchLastSeen`).
fn search_last_seen(senses: &Senses, spatial: &mut Spatial, state: &mut State, id: EntityId) {
    let tick = state.tick;
    let memory = {
        let u = unit_mut(state, id);
        u.combat.attack = None;
        if let Some(c) = &mut u.combat.charge {
            c.target = None;
        }
        if u.target.is_some() {
            u.target = None;
            discard_navigation(u);
            u.retry_at = tick;
        }
        u.combat.pursuit.clone().expect("pursuit")
    };
    let e = state.entities.get(id).expect("unit");
    let deck = spatial.deck_of(e);
    let memory_deck = memory
        .position
        .surface
        .as_deref()
        .and_then(|s| spatial.deck_id(s));
    let at = |spot: Spot| {
        spot.deck == deck && e.position.x == spot.x * 1000 && e.position.y == spot.y * 1000
    };
    let u = unit(state, id);
    let here = Spot::new(memory.position.x, memory.position.y, memory_deck);
    if at(here) || (u.route.is_empty() && u.goal.is_some_and(|g| at(spatial.spot(g)))) {
        finish_combat(unit_mut(state, id), tick);
        return;
    }
    if u.route.is_empty() && u.retry_at <= tick {
        let known = |b: &crate::spatial::BodyRecord| {
            state
                .entities
                .get(b.id)
                .is_some_and(|other| senses.perceives(state, e, other))
        };
        let goal = spatial.nearest(here, 2, Some(id), None, None, Some(&known));
        let routed = match goal {
            Some(goal) => {
                let e = state.entities.get_mut(id).expect("unit");
                spatial.route(e, goal, false)
            }
            None => false,
        };
        if !routed {
            finish_combat(unit_mut(state, id), tick);
            return;
        }
        unit_mut(state, id).retry_at = tick + APPROACH_RETRY_TICKS;
    }
}
