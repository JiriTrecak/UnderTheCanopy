//! Triggers (old `reactions.ts`, `invoke`, `invokeLifecycle`): hit and damage reactions run
//! at the next reaction pass and cannot proc one another; lifecycle reactions (interval,
//! death, kill, weapon release) go through a saved FIFO, at most 128 per tick and 512
//! waiting.

use super::{
    Ctx, Extra, Mark,
    actor::{self, Actor, Side},
    cast,
    events::{EventKind, EventPoint},
    library::{self, AbilityId},
    program::{self, Aim, Ops, Run},
    state::{
        CombatEvent, LifecycleEvent, MAX_COMBAT_EVENTS, MAX_REACTIONS, MAX_TRIGGER_RECORDS,
        REACTIONS_PER_TICK, Reaction, Source, Timer, TriggerCooldown, TriggerKey, WeaponKind,
        entry as list_entry, lookup,
    },
    statuses,
};
use crate::{entity::EntityId, state::State};
use canopy_content::{
    abilities::{Activation, Trigger, TriggerEvent, TriggerWeapon},
    placement::Owner,
};

/// Records damage for hit and damage triggers (old `spellEvent`).
pub fn combat_event(
    ctx: &Ctx,
    state: &mut State,
    source: EntityId,
    target: EntityId,
    damage: u32,
    weapon: bool,
    melee: bool,
) {
    if damage > 0 && ctx.suppress == 0 && state.abilities.combat_events.len() < MAX_COMBAT_EVENTS {
        state.abilities.combat_events.push(CombatEvent {
            source,
            target,
            damage,
            weapon,
            melee,
        });
    }
}

/// A holder's source of triggers: a learned passive or a status.
#[derive(Debug, Clone)]
struct Entry {
    ability: AbilityId,
    rank: u8,
    status: Option<String>,
    started: Option<u32>,
    grant: Option<u32>,
    source: Option<Source>,
}

/// Learned passives and current statuses, by ability id then status (old `entries`).
fn entries(ctx: &Ctx, state: &State, holder: EntityId) -> Vec<Entry> {
    let Some(e) = state.entities.get(holder) else {
        return Vec::new();
    };
    let catalog = ctx.catalog();
    let mut out: Vec<Entry> = Vec::new();
    if let (Some(policy), Some(c)) = (catalog.spells.caster(e.definition), &e.caster) {
        for (i, b) in policy.bindings.iter().enumerate() {
            let rank = c.ranks.get(i).copied().unwrap_or(0);
            if ctx.ability(b.ability).activation != Activation::Passive || rank == 0 {
                continue;
            }
            match out
                .iter_mut()
                .find(|x| x.ability == b.ability && x.status.is_none())
            {
                Some(x) => x.rank = x.rank.max(rank),
                None => out.push(Entry {
                    ability: b.ability,
                    rank,
                    status: None,
                    started: None,
                    grant: None,
                    source: None,
                }),
            }
        }
    }
    for s in &e.statuses {
        if s.expires <= state.tick {
            continue;
        }
        out.push(Entry {
            ability: s.key.ability,
            rank: s.rank,
            status: Some(catalog.spells.status(s.key).name.clone()),
            started: Some(s.started),
            grant: Some(s.cast),
            source: s.source_context.as_deref().cloned(),
        });
    }
    out.sort_by(|a, b| {
        ctx.ability(a.ability)
            .id
            .cmp(&ctx.ability(b.ability).id)
            .then(
                a.status
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.status.as_deref().unwrap_or("")),
            )
    });
    out
}

fn triggers<'a>(ctx: &Ctx<'a>, entry: &Entry, event: TriggerEvent) -> Vec<(u8, &'a Trigger)> {
    ctx.ability(entry.ability)
        .triggers
        .iter()
        .flatten()
        .enumerate()
        .filter(|(_, t)| t.event == event && t.while_status.as_deref() == entry.status.as_deref())
        .map(|(i, t)| (i as u8, t))
        .collect()
}

/// Cooldown, every-N counter, chance and mana, in that order (old `qualifies`).
fn qualifies(
    ctx: &Ctx,
    state: &mut State,
    holder: EntityId,
    entry: &Entry,
    index: u8,
    trigger: &Trigger,
) -> bool {
    let tick = state.tick;
    let key: TriggerKey = (entry.ability, index);
    let spell = ctx.ability(entry.ability);
    let Some(e) = state.entities.get(holder) else {
        return false;
    };
    let links = e.links.as_deref();
    let cooldowns = links.map_or(&[][..], |l| &l.trigger_cooldowns[..]);
    if lookup(cooldowns, key).is_some_and(|c| c.expires > tick) {
        return false;
    }
    if trigger.cooldown_ticks.is_some()
        && lookup(cooldowns, key).is_none()
        && cooldowns.len() >= MAX_TRIGGER_RECORDS
    {
        return false;
    }
    let counters = links.map_or(&[][..], |l| &l.counters[..]);
    if lookup(counters, key).is_none() && counters.len() >= MAX_TRIGGER_RECORDS {
        return false;
    }
    let e = state.entities.get_mut(holder).expect("holder");
    let counter = entry_mut(&mut super::links(e).counters, key);
    *counter = (*counter + 1) % trigger.every.max(1);
    if *counter != 0 {
        return false;
    }
    if trigger.chance_permille < 1000
        && crate::camps::random_below(state, 1000) >= trigger.chance_permille
    {
        return false;
    }
    let cost = library::value(spell, entry.rank, &trigger.mana_cost).max(0) as u32;
    let e = state.entities.get_mut(holder).expect("holder");
    let mana = e.caster.as_ref().map_or(0, |c| c.mana);
    if mana < cost {
        return false;
    }
    if cost > 0
        && let Some(c) = &mut e.caster
    {
        c.mana -= cost;
    }
    if let Some(duration) = &trigger.cooldown_ticks {
        let duration = library::value(spell, entry.rank, duration).max(0) as u32;
        if duration > 0 {
            *list_entry(&mut super::links(e).trigger_cooldowns, key, || {
                TriggerCooldown {
                    rank: entry.rank,
                    started: tick,
                    expires: tick,
                }
            }) = TriggerCooldown {
                rank: entry.rank,
                started: tick,
                expires: tick + duration,
            };
        }
    }
    true
}

fn entry_mut(list: &mut Vec<(TriggerKey, u32)>, key: TriggerKey) -> &mut u32 {
    list_entry(list, key, || 0)
}

/// Runs queued and immediate reactions (old `SpellReactions.resolve`).
pub fn resolve(ctx: &mut Ctx, state: &mut State) {
    let events = std::mem::take(&mut state.abilities.combat_events);
    for ev in events {
        if ev.damage == 0 {
            continue;
        }
        crate::camps::answer(&ctx.senses, state, ev.source, ev.target);
        if ev.weapon && state.entities.get(ev.source).is_some_and(actor::alive) {
            run(ctx, state, ev.source, ev.target, TriggerEvent::WeaponHit);
        }
        if state.entities.get(ev.target).is_some_and(actor::alive) {
            run(ctx, state, ev.target, ev.source, TriggerEvent::Damaged);
            if ev.weapon && ev.melee {
                run(ctx, state, ev.target, ev.source, TriggerEvent::MeleeDamaged);
            }
        }
    }
    intervals(ctx, state);
    // New deaths may queue more; the remainder waits for the next tick.
    for _ in 0..REACTIONS_PER_TICK {
        if state.abilities.reactions.is_empty() {
            break;
        }
        let record = state.abilities.reactions.remove(0);
        let spell = ctx.ability(record.ability);
        let Some(trigger) = spell
            .triggers
            .as_ref()
            .and_then(|t| t.get(usize::from(record.trigger)))
        else {
            continue;
        };
        if record.event == LifecycleEvent::Interval && !interval_valid(ctx, state, &record, trigger)
        {
            continue;
        }
        let caster = actor::restore(ctx.catalog(), &record.source);
        ctx.suppress += 1;
        invoke_lifecycle(ctx, state, &caster, &record, &trigger.operations);
        ctx.suppress -= 1;
    }
}

fn interval_valid(ctx: &Ctx, state: &State, record: &Reaction, trigger: &Trigger) -> bool {
    let Some(holder) = state.entities.get(record.subject) else {
        return false;
    };
    let held = holder
        .unit
        .as_ref()
        .is_some_and(|u| u.contained.is_some() || u.garrison.is_some() || u.release.is_some());
    let timer = holder
        .links
        .as_ref()
        .and_then(|l| lookup(&l.timers, (record.ability, record.trigger)));
    actor::alive(holder)
        && holder.owner == record.source.owner
        && timer.map(|t| t.started) == record.timer_started
        && !held
        && entries(ctx, state, holder.id).iter().any(|e| {
            e.ability == record.ability
                && e.rank == record.rank
                && e.status.as_deref() == trigger.while_status.as_deref()
                && e.grant == record.timer_grant
        })
}

/// Interval timers: fixed cadence, no catch-up; held or silenced holders skip pulses
/// (old `intervals`).
fn intervals(ctx: &mut Ctx, state: &mut State) {
    let tick = state.tick;
    let holders: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| {
            e.caster.is_some()
                || !e.statuses.is_empty()
                || e.links.as_ref().is_some_and(|l| !l.timers.is_empty())
        })
        .map(|e| e.id)
        .collect();
    for holder in holders {
        let e = state.entities.get(holder).expect("holder");
        let owner = e.owner;
        let list = if actor::alive(e) {
            entries(ctx, state, holder)
        } else {
            Vec::new()
        };
        let mut wanted: Vec<TriggerKey> = Vec::new();
        for entry in &list {
            for (index, trigger) in triggers(ctx, entry, TriggerEvent::Interval) {
                let key = (entry.ability, index);
                let spell = ctx.ability(entry.ability);
                let period = trigger
                    .interval_ticks
                    .as_ref()
                    .map_or(1, |i| library::value(spell, entry.rank, i).max(1) as u32);
                wanted.push(key);
                let e = state.entities.get_mut(holder).expect("holder");
                let timers = e.links.as_ref().map_or(&[][..], |l| &l.timers[..]);
                let existing = lookup(timers, key).cloned();
                let stale = existing.as_ref().is_none_or(|t| {
                    t.rank != entry.rank
                        || t.grant != entry.grant
                        || t.owner != owner
                        || entry.started.is_some_and(|s| t.started != s)
                });
                if stale {
                    if existing.is_none() && timers.len() >= MAX_TRIGGER_RECORDS {
                        continue;
                    }
                    let started = entry.started.unwrap_or(tick);
                    *entry_timer(e, key) = Timer {
                        rank: entry.rank,
                        owner,
                        started,
                        next_tick: started + period,
                        grant: entry.grant,
                    };
                }
                let timer = entry_timer(e, key);
                if timer.next_tick > tick {
                    continue;
                }
                timer.next_tick += ((tick - timer.next_tick) / period + 1) * period;
                let started = timer.started;
                let e = state.entities.get(holder).expect("holder");
                let held = e.unit.as_ref().is_some_and(|u| {
                    u.contained.is_some() || u.garrison.is_some() || u.release.is_some()
                });
                if held
                    || (trigger.blocked_by_silence == Some(true)
                        && statuses::silenced(ctx.catalog(), e))
                    || state.abilities.reactions.len() >= MAX_REACTIONS
                    || !qualifies(ctx, state, holder, entry, index, trigger)
                {
                    continue;
                }
                enqueue(
                    ctx,
                    state,
                    holder,
                    holder,
                    entry,
                    index,
                    LifecycleEvent::Interval,
                    None,
                    None,
                    Some(started),
                );
            }
        }
        let e = state.entities.get_mut(holder).expect("holder");
        if let Some(l) = &mut e.links {
            l.timers.retain(|(k, _)| wanted.contains(k));
        }
        super::tidy(e);
    }
}

fn entry_timer(e: &mut crate::entity::Entity, key: TriggerKey) -> &mut Timer {
    list_entry(&mut super::links(e).timers, key, || Timer {
        rank: 1,
        owner: Owner::Nobody,
        started: 0,
        next_tick: 0,
        grant: None,
    })
}

/// Queues death triggers before the holder's statuses are cleared (old `death`).
pub fn death(ctx: &mut Ctx, state: &mut State, holder: EntityId) {
    let list = entries(ctx, state, holder);
    for entry in &list {
        for (index, trigger) in triggers(ctx, entry, TriggerEvent::Death) {
            if state.abilities.reactions.len() >= MAX_REACTIONS
                || !qualifies(ctx, state, holder, entry, index, trigger)
            {
                continue;
            }
            let source = if trigger.executor.is_some() {
                let Some(s) = &entry.source else {
                    continue;
                };
                Some(match state.entities.get(s.source) {
                    Some(live) if live.owner == s.owner => actor::source(&ctx.senses, state, live),
                    _ => s.clone(),
                })
            } else {
                None
            };
            enqueue(
                ctx,
                state,
                holder,
                holder,
                entry,
                index,
                LifecycleEvent::Death,
                source,
                None,
                None,
            );
        }
    }
}

/// Queues kill triggers of the unit that landed a lethal hit (old `kill`).
pub fn kill(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    subject: EntityId,
    owner: Option<Owner>,
) {
    let (Some(holder), Some(victim)) = (state.entities.get(source), state.entities.get(subject))
    else {
        return;
    };
    let mut alive_victim = victim.clone();
    alive_victim.hp = Some(1);
    if !actor::alive(holder)
        || owner.is_some_and(|o| o != holder.owner)
        || !ctx.senses.hostile(holder, &alive_victim)
    {
        return;
    }
    let traits = actor::plain(ctx.catalog(), victim);
    let unit = victim.unit.is_some();
    for entry in &entries(ctx, state, source) {
        for (index, trigger) in triggers(ctx, entry, TriggerEvent::Kill) {
            if (!unit && trigger.include_buildings != Some(true))
                || !actor::matches(&traits, trigger.filter.as_ref())
                || state.abilities.reactions.len() >= MAX_REACTIONS
                || !qualifies(ctx, state, source, entry, index, trigger)
            {
                continue;
            }
            enqueue(
                ctx,
                state,
                source,
                subject,
                entry,
                index,
                LifecycleEvent::Kill,
                None,
                None,
                None,
            );
        }
    }
}

/// Queues weapon release triggers when a unit commits a weapon attack (old
/// `weaponRelease`).
pub fn weapon_release(
    ctx: &mut Ctx,
    state: &mut State,
    holder: EntityId,
    subject: EntityId,
    weapon: WeaponKind,
) {
    let (Some(h), Some(s)) = (state.entities.get(holder), state.entities.get(subject)) else {
        return;
    };
    if ctx.suppress > 0 || !actor::alive(h) || !ctx.senses.hostile(h, s) {
        return;
    }
    let traits = actor::plain(ctx.catalog(), s);
    let unit = s.unit.is_some();
    let silenced = statuses::silenced(ctx.catalog(), h);
    for entry in &entries(ctx, state, holder) {
        for (index, trigger) in triggers(ctx, entry, TriggerEvent::WeaponRelease) {
            let kind = trigger.weapon.map(|w| match w {
                TriggerWeapon::Melee => WeaponKind::Melee,
                TriggerWeapon::Projectile => WeaponKind::Projectile,
                TriggerWeapon::Siege => WeaponKind::Siege,
            });
            if kind.is_some_and(|k| k != weapon)
                || (!unit && trigger.include_buildings != Some(true))
                || !actor::matches(&traits, trigger.filter.as_ref())
                || (trigger.blocked_by_silence == Some(true) && silenced)
                || state.abilities.reactions.len() >= MAX_REACTIONS
                || !qualifies(ctx, state, holder, entry, index, trigger)
            {
                continue;
            }
            enqueue(
                ctx,
                state,
                holder,
                subject,
                entry,
                index,
                LifecycleEvent::WeaponRelease,
                None,
                Some(weapon),
                None,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn enqueue(
    ctx: &mut Ctx,
    state: &mut State,
    holder: EntityId,
    subject: EntityId,
    entry: &Entry,
    index: u8,
    event: LifecycleEvent,
    source: Option<Source>,
    weapon: Option<WeaponKind>,
    timer_started: Option<u32>,
) {
    let (Some(h), Some(s)) = (state.entities.get(holder), state.entities.get(subject)) else {
        return;
    };
    let sight = ctx.senses.sight(state);
    let viewers: Vec<Owner> = super::owners(ctx)
        .iter()
        .copied()
        .filter(|&o| sight.visible(o, s) && (source.is_some() || sight.visible(o, h)))
        .collect();
    let explicit = source.is_some();
    let source = source.unwrap_or_else(|| actor::source(&ctx.senses, state, h));
    let subject_at = s.position;
    let cast = cast::next_cast(state);
    let _ = explicit;
    state.abilities.reactions.push(Reaction {
        event,
        source,
        timer_grant: if event == LifecycleEvent::Interval {
            entry.grant
        } else {
            None
        },
        timer_started: if event == LifecycleEvent::Interval {
            timer_started
        } else {
            None
        },
        weapon,
        subject,
        subject_at,
        cast,
        ability: entry.ability,
        rank: entry.rank,
        trigger: index,
        tick: state.tick,
        viewers,
    });
}

/// Hit and damage triggers run at once (old `run`).
fn run(ctx: &mut Ctx, state: &mut State, holder: EntityId, other: EntityId, event: TriggerEvent) {
    for entry in &entries(ctx, state, holder) {
        for (index, trigger) in triggers(ctx, entry, event) {
            if !qualifies(ctx, state, holder, entry, index, trigger) {
                continue;
            }
            ctx.suppress += 1;
            invoke(
                ctx,
                state,
                holder,
                other,
                entry.ability,
                entry.rank,
                &trigger.operations,
            );
            ctx.suppress -= 1;
        }
    }
}

/// An engine event's program (old `invoke`).
fn invoke(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    target: EntityId,
    ability: AbilityId,
    rank: u8,
    operations: &[canopy_content::abilities::Operation],
) {
    let (Some(caster), Some(aim)) = (
        actor::get(&ctx.senses, state, source),
        actor::get(&ctx.senses, state, target),
    ) else {
        return;
    };
    if !caster.alive {
        return;
    }
    let cast = cast::next_cast(state);
    program::run(
        ctx,
        state,
        Run {
            caster,
            aim: Aim::Unit(Box::new(aim)),
            cast,
            ability,
            rank,
            power: 1000,
            aura: false,
            area_defaults: false,
            operations: Ops::Program(operations),
            origin_viewers: None,
            visual_origin: None,
            creation_grant: false,
            carry: None,
        },
    );
}

/// A queued lifecycle program (old `invokeLifecycle`).
fn invoke_lifecycle(
    ctx: &mut Ctx,
    state: &mut State,
    caster: &Actor,
    record: &Reaction,
    operations: &[canopy_content::abilities::Operation],
) {
    let current = actor::get(&ctx.senses, state, caster.id);
    // A queued reward cannot follow a converted unit to its new owner.
    if record.event != LifecycleEvent::Death
        && current.as_ref().is_some_and(|c| c.owner != caster.owner)
    {
        return;
    }
    let point = Mark::point(record.subject_at);
    let visual =
        (record.event == LifecycleEvent::Death && record.subject != caster.id).then(|| {
            EventPoint {
                x: record.subject_at.x,
                y: record.subject_at.y,
                height: super::ground_height(ctx, record.subject_at, None),
            }
        });
    let kind = match record.event {
        LifecycleEvent::Interval => EventKind::Interval,
        LifecycleEvent::Death => EventKind::Death,
        LifecycleEvent::Kill => EventKind::Kill,
        LifecycleEvent::WeaponRelease => EventKind::WeaponRelease,
    };
    let subject_mark = Mark {
        id: Some(record.subject),
        position: record.subject_at,
        surface: None,
        height: None,
    };
    let target = match record.event {
        LifecycleEvent::WeaponRelease | LifecycleEvent::Interval => subject_mark,
        _ => point.clone(),
    };
    let extra = Extra {
        origin: visual,
        ..Extra::default()
    };
    super::emit(
        ctx,
        state,
        caster,
        &target,
        record.cast,
        record.ability,
        kind,
        extra,
        Some(record.viewers.clone()),
    );
    let aim = match record.event {
        LifecycleEvent::Interval => match current {
            Some(c) => Aim::Unit(Box::new(c)),
            None => Aim::Point(record.subject_at),
        },
        LifecycleEvent::WeaponRelease => match actor::get(&ctx.senses, state, record.subject) {
            Some(v)
                if v.alive
                    && v.targetable
                    && actor::relation(&ctx.senses, caster, &v) == Side::Enemy =>
            {
                Aim::Unit(Box::new(v))
            }
            _ => Aim::Point(record.subject_at),
        },
        _ => Aim::Point(record.subject_at),
    };
    program::run(
        ctx,
        state,
        Run {
            caster: caster.clone(),
            aim,
            cast: record.cast,
            ability: record.ability,
            rank: record.rank,
            power: 1000,
            aura: false,
            area_defaults: false,
            operations: Ops::Program(operations),
            origin_viewers: Some(record.viewers.clone()),
            visual_origin: visual,
            creation_grant: false,
            carry: None,
        },
    );
}
