//! Weapon modifiers and weapon casts (old `combatModifiers.ts`, `weaponCasts.ts`): critical
//! strikes, evasion, cleave and attack bonuses from passives and statuses, and manual or
//! automatic enhanced weapon releases.

use super::{
    Ctx, Extra, Mark,
    actor::{self, Actor},
    cast::{self, Aim},
    events::EventKind,
    library::{self, AbilityId},
    state::{Source, WeaponOrder},
    statuses,
};
use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId},
    orders::Order,
    state::State,
};
use canopy_content::{
    abilities::{Activation, CombatModifiers, Control, WeaponFilter},
    conditions::TargetFilter,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// An attack bonus at its rank.
#[derive(Debug, Clone)]
pub struct Bonus {
    pub amount: u32,
    pub mana_cost: u32,
    pub weapon: WeaponFilter,
    pub blocked_by_silence: bool,
    pub filter: Option<TargetFilter>,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Cleave {
    pub damage_permille: u32,
    pub radius: f64,
    pub arc_degrees: f64,
    pub max_targets: u32,
    pub include_buildings: bool,
    pub filter: Option<TargetFilter>,
}

/// One ability's (or one status's) weapon policies at its strongest rank.
#[derive(Debug, Clone)]
pub struct Grant {
    pub ability: AbilityId,
    pub rank: u8,
    pub critical: Option<(u32, u32)>,
    pub evasion: u32,
    pub cleave: Option<Cleave>,
    pub attack_bonus: Option<Bonus>,
}

fn grant_of(ctx_catalog: &Catalog, ability: AbilityId, rank: u8, m: &CombatModifiers) -> Grant {
    let a = &ctx_catalog.content.abilities.abilities[usize::from(ability.0)];
    let v = |amount| library::value(a, rank, amount).max(0) as u32;
    Grant {
        ability,
        rank,
        critical: m
            .critical
            .as_ref()
            .map(|c| (v(&c.chance_permille), v(&c.multiplier_permille))),
        evasion: m.evasion_permille.as_ref().map_or(0, v),
        cleave: m.cleave.as_ref().map(|c| Cleave {
            damage_permille: v(&c.damage_permille),
            radius: f64::from(v(&c.radius)),
            arc_degrees: c.arc_degrees,
            max_targets: c.max_targets,
            include_buildings: c.include_buildings,
            filter: c.filter.clone(),
        }),
        attack_bonus: m.attack_bonus.as_ref().map(|b| Bonus {
            amount: v(&b.amount),
            mana_cost: v(&b.mana_cost),
            weapon: b.weapon,
            blocked_by_silence: b.blocked_by_silence,
            filter: b.filter.clone(),
            status: b.status.clone(),
        }),
    }
}

/// The unit's weapon policies (old `combatModifiers`): learned passives and live statuses,
/// one per ability (or ability and status) at the strongest rank, in key order.
pub fn grants(catalog: &Catalog, holder: &Entity, tick: u32) -> Vec<Grant> {
    let mut entries: Vec<(String, AbilityId, u8, &CombatModifiers)> = Vec::new();
    let library = &catalog.content.abilities.abilities;
    let mut push = |key: String, ability: AbilityId, rank: u8, m| {
        if library::parameters(&library[usize::from(ability.0)], rank).is_none() {
            return;
        }
        match entries.iter_mut().find(|(k, ..)| *k == key) {
            Some(e) if rank > e.2 => {
                e.2 = rank;
                e.3 = m;
            }
            Some(_) => {}
            None => entries.push((key, ability, rank, m)),
        }
    };
    if let (Some(policy), Some(c)) = (catalog.spells.caster(holder.definition), &holder.caster) {
        for (i, b) in policy.bindings.iter().enumerate() {
            let a = &library[usize::from(b.ability.0)];
            let rank = c.ranks.get(i).copied().unwrap_or(0);
            if let (Activation::Passive, Some(m), true) =
                (a.activation, &a.combat_modifiers, rank > 0)
            {
                push(a.id.as_str().to_string(), b.ability, rank, m);
            }
        }
    }
    for s in &holder.statuses {
        if s.expires <= tick {
            continue;
        }
        let d = catalog.spells.status(s.key);
        if let Some(m) = &d.source.combat_modifiers {
            let a = &library[usize::from(s.key.ability.0)];
            push(
                format!("{}:{}", a.id.as_str(), d.name),
                s.key.ability,
                s.rank,
                m,
            );
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
        .into_iter()
        .map(|(_, ability, rank, m)| grant_of(catalog, ability, rank, m))
        .collect()
}

/// The strongest critical strike that rolls (old `criticalStrike`).
pub fn critical(
    catalog: &Catalog,
    state: &mut State,
    holder: EntityId,
) -> Option<(AbilityId, u32)> {
    let e = state.entities.get(holder)?;
    if e.statuses.is_empty() && e.caster.is_none() {
        return None;
    }
    let list = grants(catalog, e, state.tick);
    let mut result: Option<(AbilityId, u32)> = None;
    for g in list {
        let Some((chance, multiplier)) = g.critical else {
            continue;
        };
        if chance > 0
            && (chance == 1000 || crate::camps::random_below(state, 1000) < chance)
            && multiplier > result.map_or(1000, |r| r.1)
        {
            result = Some((g.ability, multiplier));
        }
    }
    result
}

/// Whether the holder evades a weapon hit, and through which ability (old `evadeWeapon`).
pub fn evade(catalog: &Catalog, state: &mut State, holder: EntityId) -> Option<AbilityId> {
    let e = state.entities.get(holder)?;
    if e.statuses.is_empty() && e.caster.is_none() {
        return None;
    }
    let mut list: Vec<Grant> = grants(catalog, e, state.tick)
        .into_iter()
        .filter(|g| g.evasion > 0)
        .collect();
    list.sort_by(|a, b| b.evasion.cmp(&a.evasion));
    let best = list.first()?;
    (best.evasion == 1000 || crate::camps::random_below(state, 1000) < best.evasion)
        .then_some(best.ability)
}

/// The strongest cleave (old `cleaveWeapon`).
pub fn cleave(catalog: &Catalog, holder: &Entity, tick: u32) -> Option<Grant> {
    if holder.statuses.is_empty() && holder.caster.is_none() {
        return None;
    }
    let mut list: Vec<Grant> = grants(catalog, holder, tick)
        .into_iter()
        .filter(|g| g.cleave.as_ref().is_some_and(|c| c.damage_permille > 0))
        .collect();
    list.sort_by(|a, b| {
        b.cleave
            .as_ref()
            .expect("cleave")
            .damage_permille
            .cmp(&a.cleave.as_ref().expect("cleave").damage_permille)
    });
    list.into_iter().next()
}

/// A weapon attack's enhancement, saved with its missile (old `WeaponEnhancement`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enhancement {
    pub ability: AbilityId,
    pub rank: u8,
    pub status: Option<String>,
    pub source_context: Option<Box<Source>>,
    pub bonus: u32,
    pub cast: u32,
}

/// Chooses the strongest affordable attack bonus and pays its mana (old `enhanceWeapon`).
fn enhance(
    catalog: &Catalog,
    state: &mut State,
    holder: EntityId,
    target: EntityId,
    projectile: bool,
    extra: Vec<Grant>,
    exclusive: bool,
) -> Option<(AbilityId, u8, u32, Option<String>)> {
    let (e, t) = (state.entities.get(holder)?, state.entities.get(target)?);
    let traits = actor::plain(catalog, t);
    let silenced = statuses::silenced(catalog, e);
    let mana = e.caster.as_ref().map_or(0, |c| c.mana);
    let mut list: Vec<Grant> = if exclusive {
        Vec::new()
    } else {
        grants(catalog, e, state.tick)
    };
    list.extend(extra);
    let mut usable: Vec<Grant> = list
        .into_iter()
        .filter(|g| {
            g.attack_bonus.as_ref().is_some_and(|b| {
                (b.amount > 0 || b.status.is_some())
                    && match b.weapon {
                        WeaponFilter::Any => true,
                        WeaponFilter::Projectile => projectile,
                        WeaponFilter::Melee => !projectile,
                    }
                    && (!b.blocked_by_silence || !silenced)
                    && actor::matches(&traits, b.filter.as_ref())
                    && mana >= b.mana_cost
            })
        })
        .collect();
    usable.sort_by(|a, b| {
        b.attack_bonus
            .as_ref()
            .expect("bonus")
            .amount
            .cmp(&a.attack_bonus.as_ref().expect("bonus").amount)
    });
    let g = usable.into_iter().next()?;
    let b = g.attack_bonus.expect("bonus");
    if b.mana_cost > 0
        && let Some(c) = state
            .entities
            .get_mut(holder)
            .and_then(|e| e.caster.as_mut())
    {
        c.mana -= b.mana_cost;
    }
    Some((g.ability, g.rank, b.amount, b.status))
}

/// Why a unit cannot use a weapon cast on `target` (old `SpellWeaponCasts.reason`).
pub fn cast_reason(
    ctx: &Ctx,
    state: &State,
    source: EntityId,
    target: EntityId,
    ability: AbilityId,
) -> Result<(), &'static str> {
    let catalog = ctx.catalog();
    let spell = ctx.ability(ability);
    let (Some(e), Some(t)) = (state.entities.get(source), state.entities.get(target)) else {
        return Err("This unit cannot use that weapon ability");
    };
    let profile = super::forms::weapon_def(catalog, e);
    let combat = catalog.get(profile).behaviors.combat.as_ref();
    let Some(w) = &spell.weapon_cast else {
        return Err("This unit cannot use that weapon ability");
    };
    let held = e
        .unit
        .as_ref()
        .is_none_or(|u| u.contained.is_some() || u.garrison.is_some() || u.release.is_some());
    let Some(combat) = combat.filter(|c| c.shell.is_none()) else {
        return Err("This unit cannot use that weapon ability");
    };
    if held {
        return Err("This unit cannot use that weapon ability");
    }
    let kind = if combat.projectile.is_some() {
        WeaponFilter::Projectile
    } else {
        WeaponFilter::Melee
    };
    if w.weapon != WeaponFilter::Any && w.weapon != kind {
        return Err("Ability requires a different weapon kind");
    }
    let weapon = ctx.senses.combat.weapon(profile).expect("weapon");
    let spell_type = ctx.senses.combat.spell;
    let reaches = weapon.reaches(super::forms::airborne(catalog, t));
    if !reaches
        || statuses::disarmed(catalog, e)
        || statuses::ethereal(catalog, e)
        || (weapon.damage_type != spell_type && statuses::ethereal(catalog, t))
    {
        return Err("Weapon cannot attack this target");
    }
    if e.unit.as_ref().is_some_and(|u| u.work.cargo.is_some()) {
        return Err("Return carried resources before using this weapon ability");
    }
    Ok(())
}

/// Orders a manual weapon cast: an attack whose next release is enhanced (old `order`).
#[allow(clippy::too_many_arguments)]
pub fn order(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    target: EntityId,
    ability: AbilityId,
    binding: u8,
    rank: u8,
    cast: u32,
) {
    crate::orders::issue(
        state,
        source,
        Order::Attack {
            target,
            force: false,
        },
        false,
    );
    let tick = state.tick;
    let catalog = ctx.catalog();
    let e = state.entities.get_mut(source).expect("caster");
    let profile = super::forms::weapon_def(catalog, e);
    let owner = e.owner;
    e.caster.as_mut().expect("caster").weapon_order = Some(WeaponOrder {
        id: cast,
        binding,
        ability,
        rank,
        target,
        owner,
        profile,
        started: tick,
    });
}

fn first_control(ctx: &Ctx, e: &Entity, binding: u8) -> Control {
    ctx.catalog()
        .spells
        .caster(e.definition)
        .and_then(|p| p.bindings.get(usize::from(binding)))
        .and_then(|b| b.controls.first().copied())
        .unwrap_or(Control::Player)
}

fn abandon(ctx: &mut Ctx, state: &mut State, id: EntityId, reason: &str) {
    let target = state
        .entities
        .get(id)
        .and_then(|e| e.caster.as_ref())
        .and_then(|c| c.weapon_order.as_ref())
        .map(|o| o.target);
    cast::cancel(ctx, state, id, reason);
    let tick = state.tick;
    if let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut())
        && matches!(u.order, Some(Order::Attack { target: t, .. }) if Some(t) == target)
    {
        crate::orders::finish_combat(u, tick);
    }
}

/// Drops weapon orders that no longer hold (old `SpellWeaponCasts.tick`).
pub fn tick(ctx: &mut Ctx, state: &mut State) {
    let ids: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| e.unit.is_some() && e.caster.as_ref().is_some_and(|c| c.weapon_order.is_some()))
        .map(|e| e.id)
        .collect();
    for id in ids {
        let e = state.entities.get(id).expect("caster");
        let order = e
            .caster
            .as_ref()
            .and_then(|c| c.weapon_order.clone())
            .expect("order");
        let policy = ctx.catalog().spells.caster(e.definition);
        let binding = policy.and_then(|p| p.bindings.get(usize::from(order.binding)));
        let u = e.unit.as_ref();
        let invalid = u.is_none()
            || e.owner != order.owner
            || super::forms::weapon_def(ctx.catalog(), e) != order.profile
            || !matches!(u.and_then(|u| u.order.as_ref()), Some(Order::Attack { target, .. }) if *target == order.target)
            || binding.is_none()
            || e.caster
                .as_ref()
                .and_then(|c| c.ranks.get(usize::from(order.binding)))
                .copied()
                != Some(order.rank);
        let reason = if invalid {
            Err("Weapon order interrupted")
        } else {
            let control = first_control(ctx, e, order.binding);
            let id_str = binding.expect("binding").id.clone();
            cast::reason(
                ctx,
                state,
                id,
                &id_str,
                Aim::Unit(order.target),
                control,
                false,
            )
        };
        if let Err(reason) = reason {
            abandon(ctx, state, id, reason);
        }
    }
}

/// The enhancement of a weapon release (old `SpellWeaponCasts.release`): `Err` cancels a
/// manual release that cannot happen, `Ok(None)` is an ordinary shot.
pub fn release(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    target: EntityId,
    projectile: bool,
) -> Result<Option<Enhancement>, ()> {
    let catalog = ctx.catalog();
    let Some(e) = state.entities.get(source) else {
        return Ok(None);
    };
    let order = e.caster.as_ref().and_then(|c| c.weapon_order.clone());
    let mut extras: Vec<Grant> = Vec::new();
    if let (Some(policy), Some(c)) = (catalog.spells.caster(e.definition), e.caster.clone()) {
        for (i, b) in policy.bindings.iter().enumerate() {
            let a = ctx.ability(b.ability);
            let rank = c.ranks.get(i).copied().unwrap_or(0);
            let Some(w) = &a.weapon_cast else {
                continue;
            };
            if rank == 0 {
                continue;
            }
            match &order {
                Some(o) => {
                    if usize::from(o.binding) != i {
                        continue;
                    }
                }
                None => {
                    let enabled = c.autocast.get(&(i as u8)).copied().unwrap_or_else(|| {
                        a.autocast.as_ref().is_some_and(|x| x.enabled_by_default)
                    });
                    if !b.allows(Control::Ai) || a.autocast.is_none() || !enabled {
                        continue;
                    }
                }
            }
            let control = if order.is_some() {
                b.controls.first().copied().unwrap_or(Control::Player)
            } else {
                Control::Ai
            };
            if cast::reason(ctx, state, source, &b.id, Aim::Unit(target), control, false).is_ok() {
                let r = library::parameters(a, rank).cloned().unwrap_or_default();
                let v = |amount: &canopy_content::abilities::Amount| {
                    amount.at(&r).unwrap_or(0).max(0) as u32
                };
                extras.push(Grant {
                    ability: b.ability,
                    rank,
                    critical: None,
                    evasion: 0,
                    cleave: None,
                    attack_bonus: Some(Bonus {
                        amount: v(&w.bonus),
                        mana_cost: v(&a.cast.cost.amount),
                        weapon: w.weapon,
                        blocked_by_silence: true,
                        filter: None,
                        status: w.status.clone(),
                    }),
                });
            }
        }
    }
    if let Some(o) = &order
        && (o.target != target || extras.is_empty())
    {
        abandon(ctx, state, source, "Weapon ability could not release");
        return Err(());
    }
    let Some((ability, rank, bonus, status)) = enhance(
        catalog,
        state,
        source,
        target,
        projectile,
        extras,
        order.is_some(),
    ) else {
        return Ok(None);
    };
    let spell = ctx.ability(ability);
    let cast = match &order {
        Some(o) => o.id,
        None => cast::next_cast(state),
    };
    if spell.weapon_cast.is_some() {
        let e = state.entities.get(source).expect("caster");
        let cdr = crate::stats::Stats::of(catalog, e, crate::stats::research(state, e.owner))
            .cooldown_reduction_permille;
        let cooldown = library::value(spell, rank, &spell.cast.cooldown.ticks).max(0);
        let ticks = cast::js_round(cooldown as f64 * f64::from(1000 - cdr as i32) / 1000.0) as u32;
        let tick = state.tick;
        let c = state
            .entities
            .get_mut(source)
            .and_then(|e| e.caster.as_mut())
            .expect("caster");
        c.cooldowns.insert(ability, tick + ticks);
        c.weapon_order = None;
        weapon_event(
            ctx,
            state,
            source,
            target,
            ability,
            cast,
            EventKind::Released,
            i64::from(bonus),
            None,
        );
        weapon_event(
            ctx,
            state,
            source,
            target,
            ability,
            cast,
            EventKind::Finished,
            i64::from(bonus),
            None,
        );
    }
    let source_context = status.as_ref().and_then(|_| {
        state
            .entities
            .get(source)
            .map(|e| Box::new(actor::source(&ctx.senses, state, e)))
    });
    Ok(Some(Enhancement {
        ability,
        rank,
        status,
        source_context,
        bonus,
        cast,
    }))
}

/// A weapon outcome event (old `weaponEvent`, `combatEvent`).
#[allow(clippy::too_many_arguments)]
pub fn weapon_event(
    ctx: &mut Ctx,
    state: &State,
    source: EntityId,
    target: EntityId,
    ability: AbilityId,
    cast: u32,
    kind: EventKind,
    amount: i64,
    duration: Option<u32>,
) {
    let (Some(caster), Some(victim)) = (
        actor::get(&ctx.senses, state, source),
        actor::get(&ctx.senses, state, target),
    ) else {
        return;
    };
    let audience = (kind == EventKind::EnhancedHit).then(|| {
        super::owners(ctx)
            .iter()
            .copied()
            .filter(|&o| super::visible(ctx, state, o, target, None))
            .collect::<Vec<_>>()
    });
    let extra = Extra {
        amount: Some(amount),
        duration_ticks: duration,
        ..Extra::default()
    };
    super::emit(
        ctx,
        state,
        &caster,
        &Mark::of(&victim),
        cast,
        ability,
        kind,
        extra,
        audience,
    );
}

/// A critical strike, evasion or cleave event (old `combatEvent`).
pub fn modifier_event(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    target: EntityId,
    ability: AbilityId,
    kind: EventKind,
    amount: i64,
) {
    let cast = cast::next_cast(state);
    weapon_event(
        ctx, state, source, target, ability, cast, kind, amount, None,
    );
}

/// Applies the status of a landed enhanced hit (old `weaponStatus`).
pub fn status(ctx: &mut Ctx, state: &mut State, target: EntityId, e: &Enhancement) {
    let (Some(status), Some(source)) = (&e.status, &e.source_context) else {
        return;
    };
    let caster: Actor = actor::restore(ctx.catalog(), source);
    super::program::template(
        ctx, state, caster, target, e.cast, e.ability, e.rank, status,
    );
}
