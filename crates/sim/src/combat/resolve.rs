//! Step 7: combat resolution (old `Combat.resolve`). Item damage, shells and missiles land
//! first, then units release their swings in id order; damage is summed per target and
//! applied in id order, so every hit of a tick lands at once and two units can kill each
//! other.
//!
//! Abilities and items take part where the old game wired them in: casting, charging,
//! stunned, disarmed and ethereal units stand down; a release may be a weapon cast or an
//! attack bonus; critical strikes, evasion, cleave, concealment bonuses and marks modify
//! blows; hits proc items, lifesteal and reflection, apply weapon marks and record combat
//! events for triggers; rescue items save units at zero health.

use super::{
    Attack, charge,
    deaths::{Dead, Lethal},
    projectiles::{launch_missile, launch_shell},
    rules::{DamageType, Delivery},
    senses::Senses,
};
use crate::{
    abilities::{
        AbilityId, Ctx,
        events::EventKind,
        state::WeaponKind,
        statuses,
        weapons::{self, Enhancement},
    },
    entity::{Entity, EntityId},
    orders::{Order, finish_combat},
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
    /// A weapon hit (missiles): evadable, procs items.
    pub weapon: bool,
    /// The critical strike it carries, for events.
    pub critical: Option<AbilityId>,
    /// The cleave it comes from, for events.
    pub cleave: Option<AbilityId>,
    pub enhancement: Option<Box<Enhancement>>,
}

impl Default for Hit {
    fn default() -> Self {
        Self {
            source: EntityId(0),
            owner: Owner::Nobody,
            target: EntityId(0),
            damage_milli: 0,
            damage_type: DamageType(0),
            weapon: false,
            critical: None,
            cleave: None,
            enhancement: None,
        }
    }
}

/// What a resolution did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Entities that died, in id order, with the lethal hit's source and owner.
    pub dead: Vec<Dead>,
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
    let weapon = senses.weapon(ea).expect("weapon");
    let profile = senses.weapon_def(ea);
    let own = ea.definition;
    let cycle =
        Stats::of(senses.catalog, ea, crate::stats::research(state, ea.owner)).cooldown_ticks;
    let (windup, recovery) = weapon.timing(cycle);
    let tick = state.tick;
    let u = unit_mut(state, a);
    u.combat.attack = Some(Attack {
        target: b,
        cycle_ticks: cycle,
        started: tick,
        impact: tick + windup,
        ends: tick + windup + recovery,
        released: false,
        profile: (profile != own).then_some(profile),
    });
    u.combat.cooldown = cycle;
    crate::orders::discard_navigation(u);
}

/// Whether the unit stands down from weapon combat this tick: casting, charging by an
/// ability, stunned, disarmed or ethereal (old resolve and plan guards), or held by
/// cinematic mode.
pub fn standing_down(senses: &Senses, state: &State, e: &Entity) -> bool {
    let catalog = senses.catalog;
    e.caster.as_ref().is_some_and(|c| c.pending.is_some())
        || crate::abilities::deliveries::charging(catalog, state, e.id)
        || statuses::stunned(catalog, e)
        || statuses::disarmed(catalog, e)
        || statuses::ethereal(catalog, e)
        || crate::mission::held(state, e)
}

/// Damage `raw_milli` of `kind` does to `target` after matchup and armour, without
/// consuming shields (for predictions; resolution uses [`super::damage::damage`]).
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
pub fn resolve(ctx: &mut Ctx, state: &mut State) -> Outcome {
    let _span = tracing::trace_span!("combat resolution").entered();
    let senses = ctx.senses;
    let tick = state.tick;
    let mut extra: Vec<Hit> = std::mem::take(&mut state.combat.scripted)
        .into_iter()
        .map(|s| Hit {
            source: s.source.unwrap_or(EntityId(0)),
            owner: Owner::Nobody,
            target: s.target,
            damage_milli: u64::from(s.amount) * 1000,
            damage_type: senses.combat.spell,
            ..Hit::default()
        })
        .collect();
    extra.extend(crate::items::effects::drain_hits(state));
    super::projectiles::land_shells(&senses, state, &mut extra);
    super::projectiles::land_missiles(&senses, state, &mut extra);
    // Damage summed per target, and who struck the lethal blow.
    let mut hits: BTreeMap<EntityId, u64> = BTreeMap::new();
    let mut lethal: BTreeMap<EntityId, Lethal> = BTreeMap::new();
    let units: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| active(e, tick))
        .map(|e| e.id)
        .collect();
    for a in units {
        crate::abilities::forms::reconcile_weapon(senses.catalog, state, a);
        let ea = state.entities.get(a).expect("unit");
        let u = ea.unit.as_ref().expect("unit");
        let weapon = senses.weapon(ea).copied();
        let victim = u.target.and_then(|t| state.entities.get(t));
        let keep = match (weapon, victim) {
            (Some(_), Some(b)) => {
                !standing_down(&senses, state, ea)
                    && alive(b)
                    && b.hp.is_some()
                    && senses.weapon_eligible(ea, b)
                    && senses.perceives(state, ea, b)
                    && (forced(ea) || senses.hostile(ea, b))
            }
            _ => false,
        };
        if !keep {
            if ea.unit.as_ref().is_some_and(|u| u.combat.attack.is_some()) {
                unit_mut(state, a).combat.attack = None;
            }
            continue;
        }
        let weapon = weapon.expect("weapon");
        let b = victim.expect("victim").id;
        let Some(attack) = &u.combat.attack else {
            let eb = state.entities.get(b).expect("victim");
            if u.combat.cooldown == 0
                && ctx.spatial.body_range(ea, eb) <= weapon.range * weapon.range
            {
                begin_attack(&senses, state, a, b);
            }
            continue;
        };
        if attack.target != b {
            unit_mut(state, a).combat.attack = None;
            continue;
        }
        if attack.released || tick < attack.impact {
            continue;
        }
        unit_mut(state, a)
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
        if !senses.terrain_clear(state, ea, eb) || ctx.spatial.body_range(ea, eb) > reach * reach {
            continue;
        }
        let owner = ea.owner;
        let shell = matches!(weapon.delivery, Delivery::Shell { .. });
        let projectile = matches!(weapon.delivery, Delivery::Missile { .. });
        let stats_damage =
            Stats::of(senses.catalog, ea, crate::stats::research(state, owner)).damage_milli;
        let base = if shell {
            stats_damage
        } else {
            charge::damage(state, &senses, a, b, stats_damage)
        };
        let enhancement = if shell {
            None
        } else {
            match weapons::release(ctx, state, a, b, projectile) {
                Err(weapons::Unreleasable) => continue,
                Ok(e) => e,
            }
        };
        let critical = if shell {
            None
        } else {
            weapons::critical(senses.catalog, state, a)
        };
        let cleave = if shell || projectile {
            None
        } else {
            state
                .entities
                .get(a)
                .and_then(|e| weapons::cleave(senses.catalog, e, tick))
        };
        let kind = if shell {
            WeaponKind::Siege
        } else if projectile {
            WeaponKind::Projectile
        } else {
            WeaponKind::Melee
        };
        crate::abilities::reactions::weapon_release(ctx, state, a, b, kind);
        let bonus = {
            let e = state.entities.get_mut(a).expect("unit");
            statuses::reveal(senses.catalog, e, tick, statuses::Action::Attack)
        };
        crate::abilities::clamp_pools(ctx, state, a);
        if shell {
            launch_shell(&senses, state, a, b, base + u64::from(bonus) * 1000);
            continue;
        }
        let marked = {
            let (ea, eb) = (
                state.entities.get(a).expect("unit"),
                state.entities.get(b).expect("victim"),
            );
            statuses::marked_bonus(senses.catalog, ea, eb, tick)
        };
        let multiplier = u64::from(critical.map_or(1000, |c| c.1));
        let additions = u64::from(bonus + marked + enhancement.as_ref().map_or(0, |e| e.bonus));
        let raw = (base + additions * 1000) * multiplier / 1000;
        if projectile {
            let flight = launch_missile(
                &senses,
                state,
                a,
                b,
                raw,
                critical.map(|c| c.0),
                enhancement.clone().map(Box::new),
            );
            if let Some(e) = &enhancement {
                weapons::weapon_event(
                    ctx,
                    state,
                    a,
                    b,
                    e.ability,
                    e.cast,
                    EventKind::Projectile,
                    i64::from(e.bonus),
                    Some(flight),
                );
            }
            continue;
        }
        if let Some(e) = &enhancement {
            weapons::weapon_event(
                ctx,
                state,
                a,
                b,
                e.ability,
                e.cast,
                EventKind::WeaponEnhanced,
                i64::from(e.bonus),
                None,
            );
        }
        if evaded(ctx, state, b, raw) {
            continue;
        }
        let damage = u64::from(super::damage::damage(
            &senses,
            state,
            b,
            raw,
            weapon.damage_type,
        ));
        let eb = state.entities.get(b).expect("victim");
        let left = u64::from(eb.hp.unwrap_or(0)).saturating_sub(hits.get(&b).copied().unwrap_or(0));
        let actual = damage.min(left);
        if actual > 0 && actual == left {
            lethal.insert(b, Lethal { source: a, owner });
        }
        *hits.entry(b).or_default() += damage;
        extra.extend(crate::items::effects::on_hit(
            ctx,
            state,
            a,
            b,
            actual as u32,
            weapon.damage_type,
        ));
        crate::abilities::reactions::combat_event(ctx, state, a, b, actual as u32, true, true);
        if actual > 0 {
            if let Some(e) = &enhancement {
                weapons::status(ctx, state, b, e);
                weapons::weapon_event(
                    ctx,
                    state,
                    a,
                    b,
                    e.ability,
                    e.cast,
                    EventKind::EnhancedHit,
                    actual as i64,
                    None,
                );
            }
            if let Some((ability, _)) = critical {
                weapons::modifier_event(
                    ctx,
                    state,
                    a,
                    b,
                    ability,
                    EventKind::CriticalStrike,
                    actual as i64,
                );
            }
            let cleave_raw = raw.saturating_sub(u64::from(marked) * 1000 * multiplier / 1000);
            extra.extend(cleave_hits(
                ctx,
                state,
                a,
                b,
                cleave_raw,
                weapon.damage_type,
                cleave.as_ref(),
            ));
        }
    }
    let mut i = 0;
    while i < extra.len() {
        let hit = extra[i].clone();
        i += 1;
        let source = state.entities.get(hit.source);
        let source_alive = source.is_some_and(alive);
        let Some(b) = state.entities.get(hit.target) else {
            continue;
        };
        if !alive(b) || b.hp.is_none() || b.unit.as_ref().is_some_and(|u| u.garrison.is_some()) {
            continue;
        }
        if super::damage::immune(&senses, state, hit.target, hit.damage_type) {
            continue;
        }
        if hit.weapon && evaded(ctx, state, hit.target, hit.damage_milli) {
            continue;
        }
        let damage = u64::from(super::damage::damage(
            &senses,
            state,
            hit.target,
            hit.damage_milli,
            hit.damage_type,
        ));
        let b = state.entities.get(hit.target).expect("target");
        let left =
            u64::from(b.hp.unwrap_or(0)).saturating_sub(hits.get(&b.id).copied().unwrap_or(0));
        let actual = damage.min(left);
        if actual > 0 && actual == left {
            lethal.insert(
                hit.target,
                Lethal {
                    source: hit.source,
                    owner: hit.owner,
                },
            );
        }
        if actual > 0 {
            if let Some(e) = &hit.enhancement {
                weapons::status(ctx, state, hit.target, e);
                weapons::weapon_event(
                    ctx,
                    state,
                    hit.source,
                    hit.target,
                    e.ability,
                    e.cast,
                    EventKind::EnhancedHit,
                    actual as i64,
                    None,
                );
            }
            if let Some(ability) = hit.critical {
                weapons::modifier_event(
                    ctx,
                    state,
                    hit.source,
                    hit.target,
                    ability,
                    EventKind::CriticalStrike,
                    actual as i64,
                );
            }
            if let Some(ability) = hit.cleave {
                weapons::modifier_event(
                    ctx,
                    state,
                    hit.source,
                    hit.target,
                    ability,
                    EventKind::Cleaved,
                    actual as i64,
                );
            }
        }
        if hit.weapon && source_alive {
            extra.extend(crate::items::effects::on_hit(
                ctx,
                state,
                hit.source,
                hit.target,
                actual as u32,
                hit.damage_type,
            ));
        }
        crate::abilities::reactions::combat_event(
            ctx,
            state,
            hit.source,
            hit.target,
            actual as u32,
            hit.weapon,
            false,
        );
        *hits.entry(hit.target).or_default() += damage;
    }
    let mut outcome = Outcome::default();
    for (id, damage) in hits {
        let e = state.entities.get_mut(id).expect("target");
        statuses::wake(senses.catalog, e, damage.min(u64::from(u32::MAX)) as u32);
        let hp = e.hp.as_mut().expect("body");
        *hp = (u64::from(*hp).saturating_sub(damage)) as u32;
        crate::abilities::clamp_pools(ctx, state, id);
        let dead = state.entities.get(id).is_some_and(|e| e.hp == Some(0))
            && !crate::items::effects::rescue(ctx, state, id);
        if dead {
            outcome.dead.push(Dead {
                id,
                lethal: lethal.get(&id).copied(),
            });
        }
    }
    outcome
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

/// Whether the target evades a weapon hit (old `evaded`), with its event.
fn evaded(ctx: &mut Ctx, state: &mut State, target: EntityId, raw_milli: u64) -> bool {
    let Some(ability) = weapons::evade(ctx.senses.catalog, state, target) else {
        return false;
    };
    let cast = crate::abilities::cast::next_cast(state);
    weapons::weapon_event(
        ctx,
        state,
        target,
        target,
        ability,
        cast,
        EventKind::Evaded,
        (raw_milli / 1000) as i64,
        None,
    );
    true
}

/// A melee blow's cleave (old `cleave`): a share of it to enemies in the forward arc.
fn cleave_hits(
    ctx: &Ctx,
    state: &State,
    source: EntityId,
    primary: EntityId,
    raw_milli: u64,
    kind: DamageType,
    grant: Option<&weapons::Grant>,
) -> Vec<Hit> {
    let (Some(grant), Some(s), Some(p)) = (
        grant,
        state.entities.get(source),
        state.entities.get(primary),
    ) else {
        return Vec::new();
    };
    let Some(c) = &grant.cleave else {
        return Vec::new();
    };
    let senses = &ctx.senses;
    let origin = senses.eye(state, s);
    let (dx, dy) = (
        f64::from(p.position.x) / 1000.0 - origin.x,
        f64::from(p.position.y) / 1000.0 - origin.y,
    );
    let n = crate::math::hypot(dx, dy);
    let n = if n == 0.0 { 1.0 } else { n };
    let limit = crate::math::cos(c.arc_degrees * std::f64::consts::PI / 360.0);
    let offset = |t: &Entity| {
        (
            f64::from(t.position.x) / 1000.0 - origin.x,
            f64::from(t.position.y) / 1000.0 - origin.y,
        )
    };
    let mut chosen: Vec<(f64, EntityId)> = state
        .entities
        .iter()
        .filter(|t| t.hp.is_some() && alive(t))
        .filter(|t| {
            if t.id == primary
                || !senses.weapon_eligible(s, t)
                || !senses.hostile(s, t)
                || (t.unit.is_none() && !c.include_buildings)
            {
                return false;
            }
            let (x, y) = offset(t);
            let dist = crate::math::hypot(x, y);
            dist <= c.radius
                && (c.arc_degrees == 360.0
                    || dist == 0.0
                    || (dx * x + dy * y) / (n * dist) >= limit)
                && senses.attack_clear(state, origin, t, false)
                && crate::abilities::actor::matches(
                    &crate::abilities::actor::plain(senses.catalog, t),
                    c.filter.as_ref(),
                )
        })
        .map(|t| {
            let (x, y) = offset(t);
            (x * x + y * y, t.id)
        })
        .collect();
    chosen.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    chosen
        .into_iter()
        .take(c.max_targets as usize)
        .map(|(_, id)| Hit {
            source,
            owner: s.owner,
            target: id,
            damage_milli: raw_milli * u64::from(c.damage_permille) / 1000,
            damage_type: kind,
            cleave: Some(grant.ability),
            ..Hit::default()
        })
        .collect()
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
        finish_combat(unit_mut(state, id), tick);
    }
}
