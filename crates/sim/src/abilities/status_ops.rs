//! Operations on statuses and pools (old `SpellStatuses.apply`, `tick`, `Combat.abilityHit`,
//! `gameHost.heal`): status application with stacking, refresh and control immunity,
//! dispels, drains, mana, heals, spell damage through the ordinary damage pipeline, the
//! status lifecycle and periodic damage.

use super::{
    Ctx,
    actor::{self, Actor},
    library::{self, AbilityId, StatusKey, control},
    program::{Aim, Recipient},
    state::{MAX_STATUSES, Status, UNTIL_DEATH},
    statuses, taunt, world_ops,
};
use crate::{
    combat::{
        deaths::{Dead, Lethal},
        rules::DamageType,
    },
    entity::EntityId,
    state::State,
    stats::Stats,
};
use canopy_content::{
    abilities::{DispelPolarity, DrainResource, Effect, Polarity, StackScope, StatusLifetime},
    game::Kind,
    placement::Owner,
};

/// Heals a living body (old `gameHost.heal`); returns the health restored.
pub fn heal(ctx: &mut Ctx, state: &mut State, target: EntityId, amount: u32) -> i64 {
    let Some(e) = state.entities.get(target) else {
        return 0;
    };
    if !actor::alive(e) || e.hp.is_none() {
        return 0;
    }
    let max = Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner)).max_hp;
    let e = state.entities.get_mut(target).expect("target");
    let hp = e.hp.as_mut().expect("body");
    let before = *hp;
    *hp = max.min(before.saturating_add(amount));
    i64::from(*hp) - i64::from(before)
}

/// Spell or item damage through mitigation, absorption, rescue and the death pipeline (old
/// `Combat.abilityHit` with its deaths); returns the damage dealt.
#[allow(clippy::too_many_arguments)]
pub fn spell_damage(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    owner: Owner,
    target: EntityId,
    amount: u32,
    kind: u8,
) -> u32 {
    let Some(b) = state.entities.get(target) else {
        return 0;
    };
    if !actor::alive(b) || b.hp.is_none() {
        return 0;
    }
    let damage = crate::combat::damage::damage(
        &ctx.senses,
        state,
        target,
        u64::from(amount) * 1000,
        DamageType(kind),
    );
    let catalog = ctx.catalog();
    let e = state.entities.get_mut(target).expect("target");
    statuses::wake(catalog, e, damage);
    let before = e.hp.unwrap_or(0);
    e.hp = Some(before.saturating_sub(damage));
    super::clamp_pools(ctx, state, target);
    let dealt = before.min(damage);
    let dead = state.entities.get(target).is_some_and(|e| e.hp == Some(0))
        && !crate::items::effects::rescue(ctx, state, target);
    if dead {
        crate::combat::deaths::process(
            ctx,
            state,
            &[Dead {
                id: target,
                lethal: Some(Lethal { source, owner }),
            }],
        );
    }
    super::reactions::combat_event(ctx, state, source, target, dealt, false, false);
    dealt
}

/// What a non-resource operation needs (old `SpellStatuses.apply` arguments).
pub struct Apply<'a> {
    pub source: EntityId,
    pub owner: Owner,
    pub source_camp: Option<String>,
    pub target: &'a Recipient,
    pub ability: AbilityId,
    pub rank: u8,
    pub cast: u32,
    pub effect: &'a Effect,
    /// The resolved amount (with power for drain and mana).
    pub amount: i64,
    pub restore_caster: bool,
    pub aura: bool,
    pub aim: &'a Aim,
    pub origin: &'a Actor,
    pub creation_grant: bool,
}

/// Whether a status or operation from `source` may affect `e` past its spell immunity
/// (old `SpellStatuses.accepts`).
fn accepts(ctx: &Ctx, state: &State, a: &Apply, target: EntityId) -> bool {
    let Some(e) = state.entities.get(target) else {
        return false;
    };
    let spell = ctx.ability(a.ability);
    let hostile = match state.entities.get(a.source) {
        Some(live) => ctx.senses.hostile_as(live, a.owner, e),
        None => {
            e.hp.is_some()
                && actor::alive(e)
                && actor::targetable(e)
                && ctx
                    .senses
                    .opponents(a.owner, a.source_camp.as_deref(), e.owner, actor::camp(e))
        }
    };
    statuses::accepts(
        spell.pierces_spell_immunity == Some(true),
        statuses::spell_immunity(ctx.catalog(), e, Some(state.tick)),
        hostile,
    )
}

/// Applies a mana, drain, status, dispel or world operation to one recipient; returns the
/// amount for events and records (old `SpellStatuses.apply`).
pub fn apply(ctx: &mut Ctx, state: &mut State, a: Apply) -> i64 {
    let target = a.target_id();
    if let Some(t) = target
        && let Some(e) = state.entities.get(t)
    {
        let creation_status = a.creation_grant && matches!(a.effect, Effect::Status(_));
        if (!creation_status && !accepts(ctx, state, &a, t))
            || !actor::matches(&actor::plain(ctx.catalog(), e), a.effect.filter())
        {
            return 0;
        }
    }
    if matches!(
        a.effect,
        Effect::Split(_)
            | Effect::Teleport(_)
            | Effect::Pull(_)
            | Effect::Vision(_)
            | Effect::Convert(_)
            | Effect::Contain(_)
            | Effect::ReleaseContained(_)
            | Effect::Sacrifice(_)
            | Effect::Found(_)
    ) {
        return world_ops::apply(ctx, state, &a);
    }
    let Some(t) = target else {
        return 0;
    };
    let Some(e) = state.entities.get(t) else {
        return 0;
    };
    if !actor::alive(e) {
        return 0;
    }
    match a.effect {
        Effect::Mana(_) => restore_mana(ctx, state, t, a.amount.max(0) as u32),
        Effect::Drain(d) => drain(ctx, state, &a, t, d),
        Effect::Status(s) => apply_status(ctx, state, &a, t, s),
        Effect::Dispel(d) => dispel(ctx, state, &a, t, d),
        _ => 0,
    }
}

impl Apply<'_> {
    fn target_id(&self) -> Option<EntityId> {
        match self.target {
            Recipient::Unit(a) => Some(a.id),
            Recipient::Point(_) => None,
        }
    }
}

/// Grants mana up to the pool minus reserved mana (old `mana` operation).
pub fn restore_mana(ctx: &Ctx, state: &mut State, target: EntityId, amount: u32) -> i64 {
    let Some(e) = state.entities.get(target) else {
        return 0;
    };
    if e.caster.is_none() {
        return 0;
    }
    let max = Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner)).max_mana;
    let c = state
        .entities
        .get_mut(target)
        .and_then(|e| e.caster.as_mut())
        .expect("caster");
    let before = c.mana;
    let limit = max.saturating_sub(c.escrow());
    c.mana = limit.min(before.saturating_add(amount));
    i64::from(c.mana) - i64::from(before)
}

fn drain(
    ctx: &mut Ctx,
    state: &mut State,
    a: &Apply,
    t: EntityId,
    d: &canopy_content::abilities::Drain,
) -> i64 {
    let kind = ctx.catalog().spells.damage_type(&d.damage_type);
    let amount = a.amount.max(0) as u32;
    let actual: u32 = match d.resource {
        DrainResource::Health => spell_damage(ctx, state, a.source, a.owner, t, amount, kind),
        DrainResource::Mana => match state.entities.get_mut(t).and_then(|e| e.caster.as_mut()) {
            Some(c) => {
                let n = c.mana.min(amount);
                c.mana -= n;
                n
            }
            None => 0,
        },
    };
    if a.restore_caster
        && let Some(caster) = state.entities.get(a.source)
        && actor::alive(caster)
        && caster.owner == a.owner
    {
        let stats = Stats::of(
            ctx.catalog(),
            caster,
            crate::stats::research(state, caster.owner),
        );
        let caster = state.entities.get_mut(a.source).expect("caster");
        match d.resource {
            DrainResource::Health => {
                if let Some(hp) = &mut caster.hp {
                    *hp = stats.max_hp.min(hp.saturating_add(actual));
                }
            }
            DrainResource::Mana => {
                if let Some(c) = &mut caster.caster {
                    let limit = stats.max_mana.saturating_sub(c.escrow());
                    c.mana = limit.min(c.mana + actual);
                }
            }
        }
    }
    if d.resource == DrainResource::Mana && d.damage_per_drained_permille > 0 && actual > 0 {
        let burn = (u64::from(actual) * u64::from(d.damage_per_drained_permille) / 1000) as u32;
        spell_damage(ctx, state, a.source, a.owner, t, burn, kind);
    }
    i64::from(actual)
}

fn apply_status(
    ctx: &mut Ctx,
    state: &mut State,
    a: &Apply,
    t: EntityId,
    s: &canopy_content::abilities::StatusEffect,
) -> i64 {
    let catalog = ctx.catalog();
    let spell = ctx.ability(a.ability);
    let Some(key) = catalog.spells.status_named(a.ability, &s.id) else {
        return 0;
    };
    let decl = catalog.spells.status(key);
    let now = state.tick;
    let e = state.entities.get(t).expect("target");
    let d = catalog.get(e.definition);
    let u = e.unit.as_ref();
    if s.form.as_ref().is_some_and(|f| f.movement.is_some())
        && (d.behaviors.movement.is_none()
            || u.is_some_and(|u| {
                u.garrison.is_some() || u.contained.is_some() || u.release.is_some()
            }))
    {
        return 0;
    }
    if (s.concealment.is_some() || s.form.is_some() || decl.ethereal) && u.is_none() {
        return 0;
    }
    let rank_mods = &decl.rank(a.rank).modifiers;
    if (decl.taunt
        || decl.stun
        || rank_mods.rooted
        || decl.disarm
        || decl.silence
        || decl.item_blocked)
        && d.kind != Kind::Unit
    {
        return 0;
    }
    let immune = statuses::control_immunities(catalog, e);
    let mut blocked = 0u8;
    let controls = decl.controls(a.rank);
    for bit in [
        control::STUN,
        control::DISARM,
        control::SILENCE,
        control::ITEM_BLOCKED,
        control::TAUNT,
        control::ROOT,
        control::MOVE_SLOW,
        control::ATTACK_SLOW,
    ] {
        if controls & bit == 0 {
            continue;
        }
        let refused = immune & bit != 0
            || (bit == control::TAUNT
                && (d.behaviors.combat.is_none() || !taunt_hostile(ctx, state, a, t)));
        if refused {
            blocked |= bit;
        }
    }
    if blocked != 0 && !decl.has_payload(a.rank, blocked) {
        return 0;
    }
    let hero = d.is_hero();
    let duration: u32 = if s.lifetime == StatusLifetime::UntilDeath {
        UNTIL_DEATH - now
    } else if hero && let Some(h) = &s.hero_duration {
        library::value(spell, a.rank, h).max(0) as u32
    } else {
        a.amount.max(0) as u32
    };
    let source_context = if decl.needs_source {
        let context = if a.origin.restored {
            None
        } else {
            state
                .entities
                .get(a.source)
                .map(|src| actor::source(&ctx.senses, state, src))
        };
        match context {
            Some(c) if c.owner == a.owner => Some(Box::new(c)),
            _ => return 0,
        }
    } else {
        None
    };
    let source_scope = matches!(decl.stacking, Some((_, StackScope::Source)));
    let e = state.entities.get_mut(t).expect("target");
    let existing = e
        .statuses
        .iter()
        .position(|x| x.key == key && (!source_scope || x.source == a.source));
    if a.aura
        && let Some(i) = existing
    {
        let x = &e.statuses[i];
        if x.rank > a.rank || (x.rank == a.rank && x.source < a.source) {
            return 0;
        }
    }
    let previous = existing.map(|i| e.statuses.remove(i));
    if e.statuses.len() >= MAX_STATUSES {
        return 0;
    }
    let stacks = decl.stacking.map(|(max, _)| {
        let had = previous
            .as_ref()
            .filter(|p| p.expires > now)
            .map_or(0, |p| p.stacks.unwrap_or(1));
        max.min(had + 1)
    });
    e.statuses.push(Status {
        key,
        source: a.source,
        owner: a.owner,
        rank: a.rank,
        cast: a.cast,
        started: now,
        expires: now.saturating_add(if a.aura { 1 } else { duration }),
        next_tick: now.saturating_add(s.periodic.as_ref().map_or(duration, |p| p.interval_ticks)),
        stacks,
        shield: decl.rank(a.rank).shield,
        blocked,
        aura: a.aura,
        source_context,
    });
    after_status_change(ctx, state, t, !a.aura);
    i64::from(duration)
}

/// Whether a taunt from the source would be hostile to `t` (old `combat.hostile(caster,e)`).
fn taunt_hostile(ctx: &Ctx, state: &State, a: &Apply, t: EntityId) -> bool {
    let Some(e) = state.entities.get(t) else {
        return false;
    };
    match state.entities.get(a.source) {
        Some(src) => ctx.senses.hostile(src, e),
        None => {
            actor::alive(e)
                && actor::targetable(e)
                && ctx.senses.opponents(a.owner, None, e.owner, actor::camp(e))
        }
    }
}

/// What follows a status change on a unit: stuns and silences cancel casts, stuns and
/// disarms drop swings, taunts are reconciled, pools are clamped.
pub fn after_status_change(ctx: &mut Ctx, state: &mut State, t: EntityId, clamp: bool) {
    let catalog = ctx.catalog();
    let Some(e) = state.entities.get(t) else {
        return;
    };
    let stun = statuses::stunned(catalog, e);
    let silence = statuses::silenced(catalog, e);
    let disarm = statuses::disarmed(catalog, e) || statuses::ethereal(catalog, e);
    if stun || silence {
        super::cast::cancel(ctx, state, t, if stun { "Stunned" } else { "Silenced" });
    }
    if (stun || disarm)
        && let Some(u) = state.entities.get_mut(t).and_then(|e| e.unit.as_mut())
    {
        u.combat.attack = None;
    }
    taunt::reconcile(ctx, state, t);
    if clamp {
        super::clamp_pools(ctx, state, t);
    }
}

fn dispel(
    ctx: &mut Ctx,
    state: &mut State,
    a: &Apply,
    t: EntityId,
    d: &canopy_content::abilities::Dispel,
) -> i64 {
    let catalog = ctx.catalog();
    let e = state.entities.get_mut(t).expect("target");
    let before = e.statuses.len();
    e.statuses.retain(|s| {
        let decl = catalog.spells.status(s.key);
        let remove = !s.aura
            && decl.dispel
            && match d.polarity {
                DispelPolarity::All => true,
                DispelPolarity::Positive => decl.polarity == Polarity::Positive,
                DispelPolarity::Negative => decl.polarity == Polarity::Negative,
            };
        !remove
    });
    let removed = (before - e.statuses.len()) as i64;
    super::clamp_pools(ctx, state, t);
    taunt::reconcile(ctx, state, t);
    let Some(e) = state.entities.get(t) else {
        return removed;
    };
    let hostile = actor::summoned(e)
        && match state.entities.get(a.source) {
            Some(src) => {
                let mut c = src.clone();
                c.owner = a.owner;
                ctx.senses.hostile(&c, e)
            }
            None => ctx.senses.opponents(a.owner, None, e.owner, actor::camp(e)),
        };
    if hostile {
        let kind = catalog.spells.damage_type(&d.damage_type);
        spell_damage(
            ctx,
            state,
            a.source,
            a.owner,
            t,
            a.amount.max(0) as u32,
            kind,
        );
    }
    removed
}

/// Status expiry and periodic damage (old `SpellStatuses.tickEntity`): aura statuses are
/// rebuilt each tick, so all of them go first.
pub fn tick(ctx: &mut Ctx, state: &mut State) {
    let ids: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| !e.statuses.is_empty())
        .map(|e| e.id)
        .collect();
    let tick = state.tick;
    for id in ids {
        let e = state.entities.get_mut(id).expect("holder");
        let living = actor::alive(e);
        e.statuses
            .retain(|s| !s.aura && s.expires >= tick && living);
        let due: Vec<usize> = (0..e.statuses.len())
            .filter(|&i| {
                ctx.catalog()
                    .spells
                    .status(e.statuses[i].key)
                    .periodic
                    .is_some()
                    && e.statuses[i].next_tick <= tick
            })
            .collect();
        let pulses: Vec<Status> = due.iter().map(|&i| e.statuses[i].clone()).collect();
        for (i, s) in due.into_iter().zip(&pulses) {
            let decl = ctx.catalog().spells.status(s.key);
            let (interval, kind) = decl.periodic.expect("periodic");
            if let Some(x) = state
                .entities
                .get_mut(id)
                .and_then(|e| e.statuses.get_mut(i))
                .filter(|x| x.key == s.key && x.source == s.source && x.cast == s.cast)
            {
                x.next_tick += interval;
            }
            if !periodic_accepts(ctx, state, id, s) {
                continue;
            }
            let damage = decl.rank(s.rank).periodic_damage;
            spell_damage(ctx, state, s.source, s.owner, id, damage, kind);
        }
        if let Some(e) = state.entities.get_mut(id) {
            e.statuses.retain(|s| s.expires > tick);
        }
    }
}

fn periodic_accepts(ctx: &Ctx, state: &State, id: EntityId, s: &Status) -> bool {
    let Some(e) = state.entities.get(id) else {
        return false;
    };
    let spell = ctx.ability(s.key.ability);
    let camp = s.source_context.as_ref().and_then(|c| c.camp.clone());
    let hostile = match state.entities.get(s.source) {
        Some(src) => {
            let mut c = src.clone();
            c.owner = s.owner;
            ctx.senses.hostile(&c, e)
        }
        None => {
            e.hp.is_some()
                && actor::alive(e)
                && actor::targetable(e)
                && ctx
                    .senses
                    .opponents(s.owner, camp.as_deref(), e.owner, actor::camp(e))
        }
    };
    statuses::accepts(
        spell.pierces_spell_immunity == Some(true),
        statuses::spell_immunity(ctx.catalog(), e, Some(state.tick)),
        hostile,
    )
}

/// Whether the target holds any status of the ability (old `hasStatus`).
pub fn has_status(state: &State, target: EntityId, ability: AbilityId) -> bool {
    state
        .entities
        .get(target)
        .is_some_and(|e| e.statuses.iter().any(|s| s.key.ability == ability))
}

/// Removes the `instance` statuses of a persistent cast (old `endInstance`).
pub fn end_instance(ctx: &mut Ctx, state: &mut State, cast: u32) {
    let catalog = ctx.catalog();
    let ids: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| {
            e.statuses.iter().any(|s| {
                s.cast == cast && catalog.spells.status(s.key).lifetime == StatusLifetime::Instance
            })
        })
        .map(|e| e.id)
        .collect();
    for id in ids {
        let e = state.entities.get_mut(id).expect("holder");
        e.statuses.retain(|s| {
            s.cast != cast || catalog.spells.status(s.key).lifetime != StatusLifetime::Instance
        });
        super::clamp_pools(ctx, state, id);
    }
}

/// A status key for an ability's named status, if declared.
pub fn key(ctx: &Ctx, ability: AbilityId, name: &str) -> Option<StatusKey> {
    ctx.catalog().spells.status_named(ability, name)
}
