//! Casting (old `AbilityRuntime.reason`, `eligibility`, `cast`, `cancel`, the caster loop of
//! `tick` and the release loop of `resolve`): acceptance, turning and preparation, release,
//! channel waves, recovery, mana reservation, cooldowns and regeneration.

use super::{
    Ctx, Extra, Mark,
    actor::{self, Actor, Side},
    deliveries,
    events::EventKind,
    library::{self, AbilityId},
    persistent, program,
    state::{Channel, Pending, Phase},
    statuses, weapons, world_ops,
};
use crate::{
    entity::{EntityId, Milli},
    state::State,
    stats::Stats,
};
use canopy_content::abilities::{
    Ability, Activation, Control, Delivery, Effect, Operation, TargetingKind,
};

/// What a cast is aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aim {
    Unit(EntityId),
    /// A whole cell, in milli-cells.
    Point(Milli),
}

pub const OUT_OF_RANGE: &str = "message.target-is-out-of-range";

/// The caster's ability state, policy and stats (old `CasterAccess`).
pub struct Access {
    pub actor: Actor,
    pub stats: Stats,
}

pub fn access(ctx: &Ctx, state: &State, id: EntityId) -> Option<Access> {
    let e = state.entities.get(id)?;
    e.caster.as_ref()?;
    Some(Access {
        actor: actor::of(&ctx.senses, state, e),
        stats: Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner)),
    })
}

/// Whether `target` passes the ability's targeting filter and condition (old
/// `matchesSpellTarget`).
pub fn matches_target(spell: &Ability, target: &Actor, caster: &Actor, side: Side) -> bool {
    actor::matches(target, spell.targeting.filter.as_ref())
        && spell
            .targeting
            .condition
            .as_ref()
            .is_none_or(|c| actor::condition(c, side, caster, target))
}

/// Whether the spell may affect `target` past its spell immunity (old `acceptsSpell`).
pub fn accepts(spell: &Ability, target: &Actor, side: Side) -> bool {
    statuses::accepts(
        spell.pierces_spell_immunity == Some(true),
        target.spell_immunity,
        side == Side::Enemy,
    )
}

/// The release operations a target would receive (old `releaseEffects`).
pub fn release_effects<'a>(
    spell: &'a Ability,
    side: Side,
    caster: &Actor,
    target: &Actor,
) -> Vec<&'a Effect> {
    let mut out = Vec::new();
    for op in &spell.on_release {
        match op {
            Operation::Effect(e) => out.push(e),
            Operation::Branch(b) => {
                let arm = if actor::condition(&b.condition, side, caster, target) {
                    &b.then
                } else {
                    &b.otherwise
                };
                out.extend(arm.iter());
            }
        }
    }
    out
}

fn delivery_count(spell: &Ability, rank: u8) -> usize {
    match &spell.delivery {
        Some(Delivery::Swarm { count, .. }) => library::value(spell, rank, count).max(0) as usize,
        Some(_) => 1,
        None => 0,
    }
}

/// Whether the aim is valid for the caster (old `eligibility`).
#[allow(clippy::too_many_arguments)]
pub fn eligibility(
    ctx: &mut Ctx,
    state: &State,
    caster: &Actor,
    aim: Aim,
    ability: AbilityId,
    rank: u8,
    accepting: bool,
    ignore_range: bool,
) -> Result<(), &'static str> {
    let spell = ctx.ability(ability);
    if !caster.alive || caster.blocked {
        return Err("message.caster-is-unable-to-cast");
    }
    if spell.delivery.is_some()
        && state.abilities.deliveries.len() + delivery_count(spell, rank)
            > super::state::MAX_DELIVERIES
    {
        return Err("message.too-many-active-spell-deliveries");
    }
    let range = library::value(spell, rank, &spell.targeting.range).max(0) * 1000;
    if spell.targeting.kind == TargetingKind::Caster {
        if aim != Aim::Unit(caster.id) {
            return Err("message.this-ability-targets-its-caster");
        }
        if !matches_target(spell, caster, caster, Side::Ally) {
            return Err("message.target-does-not-match-spell-filters");
        }
        if !accepts(spell, caster, Side::Ally) {
            return Err("message.target-is-immune-to-this-spell");
        }
        return validate_world(ctx, state, caster.id, aim, ability, rank);
    }
    let target_id = match aim {
        Aim::Point(p) => {
            if spell.targeting.kind != TargetingKind::Point || !valid_point(ctx, p) {
                return Err("message.choose-a-valid-ground-position");
            }
            if spell.targeting.visible
                && !super::visible_point(ctx, state, caster.owner, p, Some(caster))
            {
                return Err("message.target-is-not-visible");
            }
            validate_world(ctx, state, caster.id, aim, ability, rank)?;
            if !ignore_range && caster.distance2(p) > range * range {
                return Err(OUT_OF_RANGE);
            }
            return Ok(());
        }
        Aim::Unit(id) => id,
    };
    if spell.targeting.kind != TargetingKind::Unit {
        return Err("message.choose-a-ground-position");
    }
    let Some(target) = actor::get(&ctx.senses, state, target_id) else {
        return Err("message.choose-a-living-unit");
    };
    if !target.alive || (!target.unit && spell.targeting.include_buildings != Some(true)) {
        return Err("message.choose-a-living-unit");
    }
    if !target.targetable {
        return Err("message.target-cannot-be-targeted");
    }
    if target.id == caster.id && !spell.targeting.allow_self {
        return Err("message.cannot-target-self");
    }
    let side = actor::relation(&ctx.senses, caster, &target);
    if !side.is(&spell.targeting.relations) {
        return Err("message.invalid-target-relationship");
    }
    if !super::visible(ctx, state, caster.owner, target.id, Some(caster)) {
        return Err("message.target-is-not-visible");
    }
    if !matches_target(spell, &target, caster, side) {
        return Err("message.target-does-not-match-spell-filters");
    }
    if !accepts(spell, &target, side) {
        return Err("message.target-is-immune-to-this-spell");
    }
    let effects = release_effects(spell, side, caster, &target);
    if accepting
        && !effects.is_empty()
        && effects.iter().all(|e| matches!(e, Effect::Heal(_)))
        && target.hp >= target.max_hp
    {
        return Err("message.target-is-already-at-full-health");
    }
    validate_world(ctx, state, caster.id, aim, ability, rank)?;
    if !ignore_range && caster.distance2(target.position) > range * range {
        return Err(OUT_OF_RANGE);
    }
    Ok(())
}

fn valid_point(ctx: &Ctx, p: Milli) -> bool {
    let size = ctx.spatial.size() as i32 * 1000;
    p.x % 1000 == 0 && p.y % 1000 == 0 && p.x >= 0 && p.y >= 0 && p.x < size && p.y < size
}

/// Charges need grounded units; the first world operation must be possible (old
/// `validateAim`).
fn validate_world(
    ctx: &mut Ctx,
    state: &State,
    caster: EntityId,
    aim: Aim,
    ability: AbilityId,
    rank: u8,
) -> Result<(), &'static str> {
    let spell = ctx.ability(ability);
    if matches!(spell.delivery, Some(Delivery::Charge { .. })) {
        let catalog = ctx.catalog();
        let grounded = |id: EntityId| {
            state
                .entities
                .get(id)
                .is_some_and(|e| e.unit.is_some() && !super::forms::airborne(catalog, e))
        };
        let ok = grounded(caster) && matches!(aim, Aim::Unit(t) if grounded(t));
        return if ok {
            Ok(())
        } else {
            Err("message.charge-requires-grounded-units")
        };
    }
    world_ops::validate_aim(ctx, state, caster, aim, ability, rank)
}

/// Whether the caster may cast the binding at `aim` now (old `reason`); `Err` carries the
/// rejection message.
#[allow(clippy::too_many_arguments)]
pub fn reason(
    ctx: &mut Ctx,
    state: &State,
    caster: EntityId,
    binding: &str,
    aim: Aim,
    control: Control,
    ignore_range: bool,
) -> Result<(), &'static str> {
    reason_as(
        ctx,
        state,
        caster,
        None,
        binding,
        aim,
        control,
        ignore_range,
    )
}

/// [`reason`] with the caster's snapshot already taken from this state (`None`: take it).
#[allow(clippy::too_many_arguments)]
pub fn reason_as(
    ctx: &mut Ctx,
    state: &State,
    caster: EntityId,
    snapshot: Option<&Actor>,
    binding: &str,
    aim: Aim,
    control: Control,
    ignore_range: bool,
) -> Result<(), &'static str> {
    let Some(e) = state.entities.get(caster) else {
        return Err("message.cannot-cast-this-ability");
    };
    let Some(policy) = ctx.catalog().spells.caster(e.definition) else {
        return Err("message.cannot-cast-this-ability");
    };
    let Some((index, b)) = policy.binding(binding) else {
        return Err("message.cannot-cast-this-ability");
    };
    let Some(state_caster) = e.caster.as_ref() else {
        return Err("message.cannot-cast-this-ability");
    };
    let spell = ctx.ability(b.ability);
    if spell.activation == Activation::Passive || !b.allows(control) {
        return Err("message.cannot-cast-this-ability");
    }
    let rank = state_caster.ranks.get(index).copied().unwrap_or(0);
    if library::parameters(spell, rank).is_none() {
        return Err("message.learn-this-ability-first");
    }
    let ability = b.ability;
    // The caster's snapshot (its stats are not needed here).
    let taken;
    let actor = match snapshot {
        Some(a) => a,
        None => {
            taken = actor::of(&ctx.senses, state, e);
            &taken
        }
    };
    if spell.persistent.as_ref().is_some_and(|p| p.toggle)
        && state
            .abilities
            .instances
            .iter()
            .any(|i| i.source == caster && i.ability == ability)
    {
        return if actor.alive {
            Ok(())
        } else {
            Err("message.caster-is-unable-to-cast")
        };
    }
    if state_caster.pending.is_some() || deliveries::charging(ctx.catalog(), state, caster) {
        return Err("message.caster-is-busy");
    }
    if state_caster.cooldowns.get(&ability).copied().unwrap_or(0) > state.tick {
        return Err("message.ability-is-cooling-down");
    }
    let cost = library::value(spell, rank, &spell.cast.cost.amount).max(0) as u32;
    if state_caster.mana < cost {
        return Err("message.not-enough-mana");
    }
    eligibility(ctx, state, actor, aim, ability, rank, true, ignore_range)?;
    if spell.weapon_cast.is_some()
        && let Aim::Unit(target) = aim
    {
        weapons::cast_reason(ctx, state, caster, target, ability)?;
    }
    Ok(())
}

/// Ticks to turn from the current facing to the aim (old `turnTicks`).
fn turn_ticks(ctx: &Ctx, state: &State, caster: EntityId, aim: Aim) -> u32 {
    let e = state.entities.get(caster).expect("caster");
    let point = match aim {
        Aim::Unit(id) => state.entities.get(id).map_or(e.position, |t| t.position),
        Aim::Point(p) => p,
    };
    let rate = ctx
        .catalog()
        .get(e.definition)
        .behaviors
        .movement
        .as_ref()
        .map_or(720, |m| m.turn_rate);
    let from = (
        f64::from(e.position.x) / 1000.0,
        f64::from(e.position.y) / 1000.0,
    );
    let to = (f64::from(point.x) / 1000.0, f64::from(point.y) / 1000.0);
    let current = f64::from(e.rotation) / 1000.0;
    let heading = crate::movement::facing::heading(from, to, current);
    let difference = crate::movement::facing::turn_difference(current, heading).abs();
    (difference / crate::movement::facing::turn_step(rate)).ceil() as u32
}

/// The event target of a cast: its point, its unit, or the caster.
pub fn mark_of(ctx: &Ctx, state: &State, p: &Pending, fallback: &Actor) -> Mark {
    if let Some(point) = p.point {
        return Mark::point(point);
    }
    match p.target.and_then(|t| actor::get(&ctx.senses, state, t)) {
        Some(t) => Mark::of(&t),
        None => Mark::of(fallback),
    }
}

/// The aim actor of a cast for programs: its unit, or a point.
pub fn aim_of(ctx: &Ctx, state: &State, p: &Pending, fallback: &Actor) -> program::Aim {
    if let Some(point) = p.point {
        return program::Aim::Point(point);
    }
    match p.target.and_then(|t| actor::get(&ctx.senses, state, t)) {
        Some(t) => program::Aim::Unit(Box::new(t)),
        None => program::Aim::Unit(Box::new(fallback.clone())),
    }
}

/// Accepts a cast (old `cast`): reserves mana and starts turning and preparing, or toggles
/// a running instance off, or orders a weapon cast.
pub fn cast(
    ctx: &mut Ctx,
    state: &mut State,
    caster: EntityId,
    binding: &str,
    aim: Aim,
    control: Control,
) -> Result<(), &'static str> {
    reason(ctx, state, caster, binding, aim, control, false)?;
    let e = state.entities.get(caster).expect("caster");
    let policy = ctx.catalog().spells.caster(e.definition).expect("policy");
    let (index, b) = policy.binding(binding).expect("binding");
    let ability = b.ability;
    let spell = ctx.ability(ability);
    let rank = e.caster.as_ref().expect("caster").ranks[index];
    let access = access(ctx, state, caster).expect("caster");
    if spell.persistent.as_ref().is_some_and(|p| p.toggle)
        && let Some(i) = state
            .abilities
            .instances
            .iter()
            .position(|i| i.source == caster && i.ability == ability)
    {
        let old = state.abilities.instances.remove(i);
        persistent::end_instance(ctx, state, old.cast);
        super::emit(
            ctx,
            state,
            &access.actor,
            &Mark::of(&access.actor),
            old.cast,
            ability,
            EventKind::Finished,
            Extra::reason("message.toggled-off"),
            None,
        );
        return Ok(());
    }
    if state
        .entities
        .get(caster)
        .and_then(|e| e.caster.as_ref())
        .is_some_and(|c| c.weapon_order.is_some())
    {
        cancel(ctx, state, caster, "message.replaced-by-a-new-ability");
    }
    if spell.weapon_cast.is_some()
        && let Aim::Unit(target) = aim
    {
        let id = next_cast(state);
        weapons::order(ctx, state, caster, target, ability, index as u8, rank, id);
        let mark = actor::get(&ctx.senses, state, target)
            .map_or(Mark::point(Milli::default()), |t| Mark::of(&t));
        super::emit(
            ctx,
            state,
            &access.actor,
            &mark,
            id,
            ability,
            EventKind::WeaponOrdered,
            Extra::default(),
            None,
        );
        return Ok(());
    }
    let escrow = library::value(spell, rank, &spell.cast.cost.amount).max(0) as u32;
    let start_tick = state.tick + turn_ticks(ctx, state, caster, aim);
    let release_tick = start_tick + spell.cast.prepare_ticks;
    let channel = spell.cast.channel.as_ref().map(|c| {
        let waves = library::value(spell, rank, &c.waves).max(0) as u32;
        let interval = library::value(spell, rank, &c.interval_ticks).max(0) as u32;
        (waves, interval)
    });
    let channel_ticks = channel.map_or(0, |(w, i)| w * i);
    let cooldown = library::value(spell, rank, &spell.cast.cooldown.ticks).max(0);
    let cooldown_ticks = js_round(
        cooldown as f64 * f64::from(1000 - access.stats.cooldown_reduction_permille as i32)
            / 1000.0,
    ) as u32;
    let id = next_cast(state);
    let pending = Pending {
        id,
        binding: index as u8,
        ability,
        rank,
        target: match aim {
            Aim::Unit(t) => Some(t),
            Aim::Point(_) => None,
        },
        point: match aim {
            Aim::Point(p) => Some(p),
            Aim::Unit(_) => None,
        },
        channel: channel.map(|(waves, interval)| Channel {
            wave: 0,
            next_wave_tick: release_tick + interval,
            end_tick: release_tick + waves * interval,
            origin: access.actor.position,
        }),
        owner: access.actor.owner,
        start_tick,
        release_tick,
        finish_tick: release_tick + spell.cast.recover_ticks + channel_ticks,
        phase: Phase::Preparing,
        escrow,
        cooldown_ticks,
    };
    {
        let c = state
            .entities
            .get_mut(caster)
            .and_then(|e| e.caster.as_mut())
            .expect("caster");
        c.mana -= escrow;
        c.pending = Some(pending.clone());
    }
    begin(ctx, state, caster);
    let mark = mark_of(ctx, state, &pending, &access.actor);
    super::emit(
        ctx,
        state,
        &access.actor,
        &mark,
        id,
        ability,
        EventKind::Accepted,
        Extra::default(),
        None,
    );
    Ok(())
}

/// `Math.round` (halves towards +∞).
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

pub fn next_cast(state: &mut State) -> u32 {
    let id = state.counters.cast;
    state.counters.cast += 1;
    id
}

/// A caster commits to a cast (old `gameHost.begin`): concealment that breaks on casting
/// ends, and its orders and engagement are dropped.
fn begin(ctx: &mut Ctx, state: &mut State, id: EntityId) {
    let tick = state.tick;
    let catalog = ctx.catalog();
    if let Some(e) = state.entities.get_mut(id) {
        statuses::reveal(catalog, e, tick, statuses::Action::Cast);
    }
    super::clamp_pools(ctx, state, id);
    if let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
        crate::orders::release_combat(u);
        crate::orders::discard_navigation(u);
        u.order = None;
        u.idle = None;
        u.order_queue.clear();
    }
}

/// Cancels the caster's cast, weapon order and charge (old `cancel`): mana reserved before
/// release returns.
pub fn cancel(ctx: &mut Ctx, state: &mut State, id: EntityId, reason: &str) {
    let Some(access) = access(ctx, state, id) else {
        return;
    };
    let order = state
        .entities
        .get_mut(id)
        .and_then(|e| e.caster.as_mut())
        .and_then(|c| c.weapon_order.take());
    if let Some(order) = order {
        let mark = actor::get(&ctx.senses, state, order.target)
            .map_or_else(|| Mark::of(&access.actor), |t| Mark::of(&t));
        super::emit(
            ctx,
            state,
            &access.actor,
            &mark,
            order.id,
            order.ability,
            EventKind::WeaponOrderCancelled,
            Extra::reason(reason),
            None,
        );
    }
    let catalog = ctx.catalog();
    let mut i = state.abilities.deliveries.len();
    while i > 0 {
        i -= 1;
        let d = &state.abilities.deliveries[i];
        if d.source != id || !deliveries::is_charge(catalog, d.ability) {
            continue;
        }
        let d = state.abilities.deliveries.remove(i);
        let mark = d
            .target
            .and_then(|t| actor::get(&ctx.senses, state, t))
            .map_or_else(|| Mark::of(&access.actor), |t| Mark::of(&t));
        super::emit(
            ctx,
            state,
            &access.actor,
            &mark,
            d.cast,
            d.ability,
            EventKind::Cancelled,
            Extra::reason(reason),
            None,
        );
    }
    let Some(pending) = state
        .entities
        .get(id)
        .and_then(|e| e.caster.as_ref())
        .and_then(|c| c.pending.clone())
    else {
        return;
    };
    let max_mana = access.stats.max_mana;
    let c = state
        .entities
        .get_mut(id)
        .and_then(|e| e.caster.as_mut())
        .expect("caster");
    if pending.phase == Phase::Preparing {
        c.mana = max_mana.min(c.mana + pending.escrow);
    }
    c.pending = None;
    let mark = mark_of(ctx, state, &pending, &access.actor);
    let kind = if pending.phase == Phase::Recovering {
        EventKind::Finished
    } else {
        EventKind::Cancelled
    };
    super::emit(
        ctx,
        state,
        &access.actor,
        &mark,
        pending.id,
        pending.ability,
        kind,
        Extra::reason(reason),
        None,
    );
}

/// Units with ability state, in id order (old `casters()`).
pub fn casters(state: &State) -> Vec<EntityId> {
    state
        .entities
        .iter()
        .filter(|e| e.caster.is_some() && e.unit.is_some())
        .map(|e| e.id)
        .collect()
}

/// The caster pass of step 2 (old `tick` after auras and weapon orders): interrupted casts
/// and channels end, recoveries finish, mana regenerates.
pub fn tick_casters(ctx: &mut Ctx, state: &mut State) {
    let tick = state.tick;
    for id in casters(state) {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        let Some(caster) = e.caster.as_ref() else {
            continue;
        };
        let pending = caster.pending.clone();
        // Without a cast only the pool's regeneration is left: the snapshot's life and
        // reach read straight from the entity.
        let Some(p) = &pending else {
            if actor::alive(e) && actor::targetable(e) {
                let stats = Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner));
                regenerate(state, id, true, &stats);
            }
            continue;
        };
        let Some(access) = access(ctx, state, id) else {
            continue;
        };
        {
            let moved = p
                .channel
                .as_ref()
                .is_some_and(|c| actor::distance2(access.actor.position, c.origin) > 10_000);
            if !access.actor.alive || access.actor.blocked || p.owner != access.actor.owner || moved
            {
                cancel(ctx, state, id, "message.caster-interrupted");
            } else if p.phase == Phase::Channeling
                && let Some(target) = p.target
                && channel_target_lost(ctx, state, &access.actor, p, target)
            {
                cancel(ctx, state, id, "message.channel-target-lost");
            } else if p.phase == Phase::Recovering && p.finish_tick <= tick {
                state
                    .entities
                    .get_mut(id)
                    .and_then(|e| e.caster.as_mut())
                    .expect("caster")
                    .pending = None;
                let mark = mark_of(ctx, state, p, &access.actor);
                super::emit(
                    ctx,
                    state,
                    &access.actor,
                    &mark,
                    p.id,
                    p.ability,
                    EventKind::Finished,
                    Extra::default(),
                    None,
                );
            }
        }
        regenerate(
            state,
            id,
            access.actor.alive && access.actor.targetable,
            &access.stats,
        );
    }
}

/// Mana regeneration of one caster pass. Reserved mana still occupies pool capacity:
/// regeneration cannot top up a refund.
fn regenerate(state: &mut State, id: EntityId, active: bool, stats: &Stats) {
    if !active {
        return;
    }
    let regen = js_round(stats.mana_regen * 1000.0) as u32;
    let c = state
        .entities
        .get_mut(id)
        .and_then(|e| e.caster.as_mut())
        .expect("caster");
    let limit = stats.max_mana.saturating_sub(c.escrow());
    c.regeneration += regen;
    let amount = c.regeneration / 40_000;
    c.regeneration %= 40_000;
    c.mana = limit.min(c.mana + amount);
    if c.mana == limit {
        c.regeneration = 0;
    }
}

fn channel_target_lost(
    ctx: &mut Ctx,
    state: &State,
    caster: &Actor,
    p: &Pending,
    target: EntityId,
) -> bool {
    let spell = ctx.ability(p.ability);
    let Some(t) = actor::get(&ctx.senses, state, target) else {
        return true;
    };
    if !t.alive || !t.targetable {
        return true;
    }
    if spell.targeting.kind != TargetingKind::Unit {
        return false;
    }
    let range = match spell.cast.channel.as_ref().and_then(|c| c.tether_range) {
        Some(r) => (r * 1000.0).round() as i64,
        None => library::value(spell, p.rank, &spell.targeting.range) * 1000,
    };
    let side = actor::relation(&ctx.senses, caster, &t);
    !super::visible(ctx, state, caster.owner, t.id, Some(caster))
        || !matches_target(spell, &t, caster, side)
        || !accepts(spell, &t, side)
        || !side.is(&spell.targeting.relations)
        || caster.distance2(t.position) > range * range
}

fn pending_of(state: &State, id: EntityId) -> Option<Pending> {
    state.entities.get(id)?.caster.as_ref()?.pending.clone()
}

fn set_pending(state: &mut State, id: EntityId, p: Pending) {
    if let Some(c) = state.entities.get_mut(id).and_then(|e| e.caster.as_mut())
        && c.pending.as_ref().is_some_and(|q| q.id == p.id)
    {
        c.pending = Some(p);
    }
}

/// The release pass of step 8 (old `resolve` after deliveries): due releases, channel waves
/// and finished recoveries.
pub fn resolve_casters(ctx: &mut Ctx, state: &mut State) {
    for id in casters(state) {
        if pending_of(state, id).is_none() {
            continue;
        }
        let Some(access) = access(ctx, state, id) else {
            continue;
        };
        let Some(mut p) = pending_of(state, id) else {
            continue;
        };
        let tick = state.tick;
        let spell = ctx.ability(p.ability);
        // The event target is fixed for the whole pass (old `target`).
        let mark = mark_of(ctx, state, &p, &access.actor);
        let aim = match p.point {
            Some(point) => Aim::Point(point),
            None => Aim::Unit(p.target.unwrap_or(id)),
        };
        if p.phase == Phase::Preparing && p.release_tick <= tick {
            if let Err(reason) = eligibility(
                ctx,
                state,
                &access.actor,
                aim,
                p.ability,
                p.rank,
                false,
                false,
            ) {
                cancel(ctx, state, id, reason);
                continue;
            }
            p.phase = if p.channel.is_some() {
                Phase::Channeling
            } else {
                Phase::Recovering
            };
            p.escrow = 0;
            {
                let c = state
                    .entities
                    .get_mut(id)
                    .and_then(|e| e.caster.as_mut())
                    .expect("caster");
                c.cooldowns.insert(p.ability, tick + p.cooldown_ticks);
            }
            set_pending(state, id, p.clone());
            let mark = mark_of(ctx, state, &p, &access.actor);
            super::emit(
                ctx,
                state,
                &access.actor,
                &mark,
                p.id,
                p.ability,
                EventKind::Released,
                Extra::default(),
                None,
            );
            if p.channel.is_some() {
                super::emit(
                    ctx,
                    state,
                    &access.actor,
                    &mark,
                    p.id,
                    p.ability,
                    EventKind::WaveStarted,
                    Extra::default(),
                    None,
                );
            } else if spell.delivery.is_some() {
                deliveries::launch(ctx, state, &access.actor, &p);
            } else {
                if !persistent::delayed(spell) {
                    apply(ctx, state, &access.actor, &p);
                }
                if spell.persistent.is_some() {
                    persistent::start(ctx, state, &access.actor, &p);
                }
            }
        }
        // The release may have ended or replaced the cast.
        let still = |state: &State| pending_of(state, id).is_some_and(|q| q.id == p.id);
        if p.phase == Phase::Channeling
            && let Some(channel) = p.channel.clone()
            && channel.next_wave_tick <= state.tick
        {
            let access = self::access(ctx, state, id).expect("caster");
            let mark = mark_of(ctx, state, &p, &access.actor);
            super::emit(
                ctx,
                state,
                &access.actor,
                &mark,
                p.id,
                p.ability,
                EventKind::Wave,
                Extra::default(),
                None,
            );
            apply(ctx, state, &access.actor, &p);
            let interval = spell.cast.channel.as_ref().map_or(1, |c| {
                library::value(spell, p.rank, &c.interval_ticks).max(0) as u32
            });
            let c = p.channel.as_mut().expect("channel");
            c.wave += 1;
            if c.next_wave_tick >= c.end_tick {
                p.phase = Phase::Recovering;
                set_pending(state, id, p.clone());
            } else {
                c.next_wave_tick += interval;
                set_pending(state, id, p.clone());
                if still(state) {
                    let access = self::access(ctx, state, id).expect("caster");
                    super::emit(
                        ctx,
                        state,
                        &access.actor,
                        &mark,
                        p.id,
                        p.ability,
                        EventKind::WaveStarted,
                        Extra::default(),
                        None,
                    );
                }
            }
        }
        if still(state) && p.phase == Phase::Recovering && p.finish_tick <= state.tick {
            if let Some(c) = state.entities.get_mut(id).and_then(|e| e.caster.as_mut()) {
                c.pending = None;
            }
            let access = self::access(ctx, state, id).unwrap_or(access);
            super::emit(
                ctx,
                state,
                &access.actor,
                &mark,
                p.id,
                p.ability,
                EventKind::Finished,
                Extra::default(),
                None,
            );
        }
    }
}

/// Runs the release program against the cast's aim (old `apply`).
fn apply(ctx: &mut Ctx, state: &mut State, caster: &Actor, p: &Pending) {
    let aim = aim_of(ctx, state, p, caster);
    let spell = ctx.ability(p.ability);
    program::run(
        ctx,
        state,
        program::Run {
            caster: caster.clone(),
            aim,
            cast: p.id,
            ability: p.ability,
            rank: p.rank,
            power: 1000,
            aura: false,
            area_defaults: true,
            operations: program::Ops::Program(&spell.on_release),
            origin_viewers: None,
            visual_origin: None,
            creation_grant: false,
            carry: None,
        },
    );
}

/// Whether a caster's binding is learned at a rank with parameters.
pub fn learned(ctx: &Ctx, e: &crate::entity::Entity, index: usize) -> Option<(AbilityId, u8)> {
    let policy = ctx.catalog().spells.caster(e.definition)?;
    let b = policy.bindings.get(index)?;
    let rank = e.caster.as_ref()?.ranks.get(index).copied().unwrap_or(0);
    library::parameters(ctx.ability(b.ability), rank).map(|_| (b.ability, rank))
}
