//! Step 7: combat resolution (old `Combat.resolve`). Shells and missiles land first, then
//! units release their swings in id order; damage is summed per target and applied in id
//! order, so every hit of a tick lands at once and two units can kill each other.

use super::{
    Attack, charge,
    projectiles::{self, launch_missile, launch_shell},
    rules::{DamageType, Delivery},
    senses::Senses,
};
use crate::{
    entity::{Entity, EntityId},
    orders::{Order, finish_combat},
    spatial::Spatial,
    state::State,
    stats::Stats,
};
use canopy_content::placement::Owner;
use std::collections::BTreeMap;

/// One hit waiting to be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub source: EntityId,
    /// Who the kill is credited to.
    pub owner: Owner,
    pub target: EntityId,
    pub damage_milli: u64,
    pub damage_type: DamageType,
    /// A weapon hit (missiles), which items react to (M2.6).
    pub weapon: bool,
}

/// What a resolution did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Entities that died, in id order, with the owner credited for each kill.
    pub dead: Vec<(EntityId, Option<Owner>)>,
}

fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
}

fn active(e: &Entity, tick: u32) -> bool {
    alive(e)
        && e.ready_tick <= tick
        && e.unit
            .as_ref()
            .is_some_and(|u| u.contained.is_none() && u.release.is_none())
}

fn forced(e: &Entity) -> bool {
    e.unit
        .as_ref()
        .is_some_and(|u| matches!(u.order, Some(Order::Attack { force: true, .. })))
}

/// Begins a swing at `b` when the weapon may hit it, the line is clear and the unit faces
/// it (old `beginAttack`).
pub fn begin_attack(senses: &Senses, state: &mut State, a: EntityId, b: EntityId) {
    let (ea, eb) = (
        state.entities.get(a).expect("attacker"),
        state.entities.get(b).expect("target"),
    );
    if !senses.weapon_eligible(ea, eb) || !senses.terrain_clear(state, ea, eb) {
        return;
    }
    let here = (
        f64::from(ea.position.x) / 1000.0,
        f64::from(ea.position.y) / 1000.0,
    );
    let there = (
        f64::from(eb.position.x) / 1000.0,
        f64::from(eb.position.y) / 1000.0,
    );
    if !crate::movement::facing::facing(ea.rotation, here, there) {
        return;
    }
    let weapon = senses.combat.weapon(ea.definition).expect("weapon");
    let cycle =
        Stats::of(senses.catalog, ea, crate::stats::research(state, ea.owner)).cooldown_ticks;
    let (windup, recovery) = weapon.timing(cycle);
    let tick = state.tick;
    let u = state
        .entities
        .get_mut(a)
        .expect("attacker")
        .unit
        .as_mut()
        .expect("unit");
    u.combat.attack = Some(Attack {
        target: b,
        cycle_ticks: cycle,
        started: tick,
        impact: tick + windup,
        ends: tick + windup + recovery,
        released: false,
    });
    u.combat.cooldown = cycle;
    crate::orders::discard_navigation(u);
}

/// Damage `raw_milli` of `kind` does to `target` after matchup and armour (old
/// `Combat.damage`; M2.6 adds spell multipliers, absorption and item shields).
pub fn mitigate(
    senses: &Senses,
    state: &State,
    target: &Entity,
    raw_milli: u64,
    kind: DamageType,
) -> u32 {
    let Some(armor) = senses.combat.profile(target.definition).armor else {
        return 0;
    };
    let value = Stats::of(
        senses.catalog,
        target,
        crate::stats::research(state, target.owner),
    )
    .armor;
    senses.combat.mitigate(raw_milli, kind, armor, value)
}

/// Runs step 7 and returns the deaths; the caller processes them ([`super::deaths`]).
pub fn resolve(senses: &Senses, spatial: &Spatial, state: &mut State) -> Outcome {
    let _span = tracing::trace_span!("combat resolution").entered();
    let tick = state.tick;
    let mut extra: Vec<Hit> = std::mem::take(&mut state.combat.scripted)
        .into_iter()
        .map(|s| Hit {
            source: s.source.unwrap_or(EntityId(0)),
            owner: Owner::Nobody,
            target: s.target,
            damage_milli: u64::from(s.amount) * 1000,
            damage_type: senses.combat.spell,
            weapon: false,
        })
        .collect();
    // M2.6: item effect hits join here.
    projectiles::land_shells(senses, state, &mut extra);
    projectiles::land_missiles(senses, state, &mut extra);
    // Damage summed per target, and who struck the lethal blow.
    let mut hits: BTreeMap<EntityId, u64> = BTreeMap::new();
    let mut lethal: BTreeMap<EntityId, Owner> = BTreeMap::new();
    let units: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| active(e, tick))
        .map(|e| e.id)
        .collect();
    for a in units {
        let ea = state.entities.get(a).expect("unit");
        let u = ea.unit.as_ref().expect("unit");
        let weapon = senses.combat.weapon(ea.definition).copied();
        let victim = u.target.and_then(|t| state.entities.get(t));
        // M2.6: casting, ability charges, stuns, disarms and ethereal forms stand down.
        let keep = match (weapon, victim) {
            (Some(_), Some(b)) => {
                alive(b)
                    && b.hp.is_some()
                    && senses.weapon_eligible(ea, b)
                    && senses.perceives(state, ea, b)
                    && (forced(ea) || senses.hostile(ea, b))
            }
            _ => false,
        };
        if !keep {
            if ea.unit.as_ref().is_some_and(|u| u.combat.attack.is_some()) {
                state
                    .entities
                    .get_mut(a)
                    .expect("unit")
                    .unit
                    .as_mut()
                    .expect("unit")
                    .combat
                    .attack = None;
            }
            continue;
        }
        let weapon = weapon.expect("weapon");
        let b = victim.expect("victim").id;
        let Some(attack) = &u.combat.attack else {
            let eb = state.entities.get(b).expect("victim");
            if u.combat.cooldown == 0 && spatial.body_range(ea, eb) <= weapon.range * weapon.range {
                begin_attack(senses, state, a, b);
            }
            continue;
        };
        if attack.target != b {
            state
                .entities
                .get_mut(a)
                .expect("unit")
                .unit
                .as_mut()
                .expect("unit")
                .combat
                .attack = None;
            continue;
        }
        if attack.released || tick < attack.impact {
            continue;
        }
        state
            .entities
            .get_mut(a)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit")
            .combat
            .attack
            .as_mut()
            .expect("attack")
            .released = true;
        let (ea, eb) = (
            state.entities.get(a).expect("unit"),
            state.entities.get(b).expect("victim"),
        );
        let reach = weapon.range + weapon.range_buffer;
        if !senses.terrain_clear(state, ea, eb) || spatial.body_range(ea, eb) > reach * reach {
            continue;
        }
        let owner = ea.owner;
        let base = Stats::of(senses.catalog, ea, crate::stats::research(state, owner)).damage_milli;
        // M2.6: weapon enhancements, critical strikes, cleave, concealment bonuses and
        // marks modify the blow here.
        match weapon.delivery {
            Delivery::Shell { .. } => {
                launch_shell(senses, state, a, b, base);
                continue;
            }
            Delivery::Missile { .. } => {
                let raw = charge::damage(state, senses.combat, a, b, base);
                launch_missile(senses, state, a, b, raw);
                continue;
            }
            Delivery::Melee => {}
        }
        let raw = charge::damage(state, senses.combat, a, b, base);
        let eb = state.entities.get(b).expect("victim");
        let damage = u64::from(mitigate(senses, state, eb, raw, weapon.damage_type));
        let left = u64::from(eb.hp.unwrap_or(0)).saturating_sub(hits.get(&b).copied().unwrap_or(0));
        let actual = damage.min(left);
        if actual > 0 && actual == left {
            lethal.insert(b, owner);
        }
        *hits.entry(b).or_default() += damage;
    }
    for hit in extra {
        let Some(b) = state.entities.get(hit.target) else {
            continue;
        };
        if !alive(b) || b.hp.is_none() || b.unit.as_ref().is_some_and(|u| u.garrison.is_some()) {
            continue;
        }
        let damage = u64::from(mitigate(
            senses,
            state,
            b,
            hit.damage_milli,
            hit.damage_type,
        ));
        let left =
            u64::from(b.hp.unwrap_or(0)).saturating_sub(hits.get(&b.id).copied().unwrap_or(0));
        let actual = damage.min(left);
        if actual > 0 && actual == left {
            lethal.insert(b.id, hit.owner);
        }
        *hits.entry(b.id).or_default() += damage;
    }
    let mut outcome = Outcome::default();
    for (id, damage) in hits {
        let e = state.entities.get_mut(id).expect("target");
        let hp = e.hp.as_mut().expect("body");
        *hp = (u64::from(*hp).saturating_sub(damage)) as u32;
        // M2.6: item rescue saves a unit at zero health here.
        if *hp == 0 {
            outcome.dead.push((id, lethal.get(&id).copied()));
        }
    }
    outcome
}

/// Ends the pursuits of everyone who saw `dead` die (old `finishWitnessedPursuits`); an
/// unseen death leaves pursuers searching as if the target hid in the fog.
pub fn finish_witnessed(senses: &Senses, state: &mut State, dead: EntityId) {
    let tick = state.tick;
    let Some(target) = state.entities.get(dead) else {
        return;
    };
    let witnesses: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|a| active(a, tick))
        .filter(|a| {
            let u = a.unit.as_ref().expect("unit");
            (u.target == Some(dead)
                || u.combat.pursuit.as_ref().is_some_and(|p| p.target == dead)
                || u.combat.attack.as_ref().is_some_and(|x| x.target == dead)
                || matches!(u.order, Some(Order::Attack { target, .. }) if target == dead))
                && senses.perceives(state, a, target)
        })
        .map(|a| a.id)
        .collect();
    for id in witnesses {
        let u = state
            .entities
            .get_mut(id)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit");
        finish_combat(u, tick);
    }
}
