//! Persistent instances (old `startPersistent`, `persistent`): zones, toggles and sustained
//! effects that pulse the release program on their own after the release.

use super::{
    Ctx, Extra, Mark,
    actor::{self, Actor},
    cast,
    events::EventKind,
    library,
    program::{self, Aim, Ops, Run},
    state::{Instance, MAX_INSTANCES, Pending},
};
use crate::state::State;
use canopy_content::abilities::PersistentAnchor;

/// Starts an instance at release (old `startPersistent`).
pub fn start(ctx: &mut Ctx, state: &mut State, caster: &Actor, p: &Pending) {
    if state.abilities.instances.len() >= MAX_INSTANCES {
        return;
    }
    let spell = ctx.ability(p.ability);
    let Some(d) = &spell.persistent else {
        return;
    };
    let mark = cast::mark_of(ctx, state, p, caster);
    let tick = state.tick;
    let source = state
        .entities
        .get(caster.id)
        .map(|e| Box::new(actor::source(&ctx.senses, state, e)));
    state.abilities.instances.push(Instance {
        cast: p.id,
        ability: p.ability,
        rank: p.rank,
        source: caster.id,
        owner: caster.owner,
        target: mark.id,
        point: mark.position,
        origin: caster.position,
        started: tick,
        next_tick: tick + library::value(spell, p.rank, &d.interval_ticks).max(0) as u32,
        expires: tick + library::value(spell, p.rank, &d.duration_ticks).max(0) as u32,
        source_context: source,
    });
}

/// Ends the `instance` statuses of a cast (old `endInstance`).
pub fn end_instance(ctx: &mut Ctx, state: &mut State, cast: u32) {
    super::status_ops::end_instance(ctx, state, cast);
}

/// Pulses and ends instances (old `persistent`).
pub fn tick(ctx: &mut Ctx, state: &mut State) {
    let mut i = 0;
    while i < state.abilities.instances.len() {
        let mut instance = state.abilities.instances[i].clone();
        let spell = ctx.ability(instance.ability);
        let d = spell.persistent.clone();
        let source = actor::get(&ctx.senses, state, instance.source);
        let caster = source
            .clone()
            .unwrap_or_else(|| match &instance.source_context {
                Some(s) => actor::restore(ctx.catalog(), s),
                None => actor::placeholder(
                    instance.source,
                    instance.owner,
                    crate::catalog::Def(0),
                    instance.origin,
                ),
            });
        let target = instance
            .target
            .and_then(|t| actor::get(&ctx.senses, state, t));
        let tick = state.tick;
        let center: Option<Aim> = match d.as_ref().map(|d| d.anchor) {
            Some(PersistentAnchor::Caster) => Some(Aim::Unit(Box::new(caster.clone()))),
            Some(PersistentAnchor::Target) => target.clone().map(|t| Aim::Unit(Box::new(t))),
            _ => Some(Aim::Point(instance.point)),
        };
        let mut reason: Option<&str>;
        if let Some(d) = &d {
            let target_lost = d.anchor == PersistentAnchor::Target
                && target.as_ref().is_none_or(|t| !t.alive || !t.targetable);
            let target_ineligible = d.anchor == PersistentAnchor::Target
                && target.as_ref().is_some_and(|t| {
                    let side = actor::relation(&ctx.senses, &caster, t);
                    !cast::matches_target(spell, t, &caster, side) || !cast::accepts(spell, t, side)
                });
            let tethered = match (d.tether_range, &center) {
                (Some(r), Some(c)) => {
                    let r = (r * 1000.0).round() as i64;
                    actor::distance2(c.position(), caster.position) > r * r
                }
                _ => false,
            };
            reason = if tick >= instance.expires {
                Some("Expired")
            } else if source.as_ref().is_some_and(|s| s.owner != instance.owner) {
                Some("Owner changed")
            } else if d.ends_with_caster && !source.as_ref().is_some_and(|s| s.alive) {
                Some("Caster died")
            } else if d.ends_with_caster && source.as_ref().is_some_and(|s| !s.targetable) {
                Some("Caster unavailable")
            } else if center.is_none() || target_lost {
                Some("Target lost")
            } else if target_ineligible {
                Some("Target no longer eligible")
            } else if tethered {
                Some("Tether broken")
            } else {
                None
            };
            if reason.is_none() && tick >= instance.next_tick {
                let upkeep = library::value(spell, instance.rank, &d.upkeep_mana).max(0) as u32;
                let mana = state
                    .entities
                    .get(instance.source)
                    .and_then(|e| e.caster.as_ref())
                    .map_or(0, |c| c.mana);
                if mana < upkeep {
                    reason = Some("Mana exhausted");
                } else {
                    if upkeep > 0
                        && let Some(c) = state
                            .entities
                            .get_mut(instance.source)
                            .and_then(|e| e.caster.as_mut())
                    {
                        c.mana -= upkeep;
                    }
                    instance.next_tick +=
                        library::value(spell, instance.rank, &d.interval_ticks).max(0) as u32;
                    state.abilities.instances[i].next_tick = instance.next_tick;
                    let center = center.clone().expect("center");
                    let radius = spell
                        .targeting
                        .radius
                        .as_ref()
                        .map(|r| library::value(spell, instance.rank, r) as f64);
                    let extra = Extra {
                        radius,
                        ..Extra::default()
                    };
                    super::emit(
                        ctx,
                        state,
                        &caster,
                        &center.mark(),
                        instance.cast,
                        instance.ability,
                        EventKind::Wave,
                        extra,
                        None,
                    );
                    program::run(
                        ctx,
                        state,
                        Run {
                            caster: caster.clone(),
                            aim: center,
                            cast: instance.cast,
                            ability: instance.ability,
                            rank: instance.rank,
                            power: 1000,
                            aura: false,
                            area_defaults: true,
                            operations: Ops::Program(&spell.on_release),
                            origin_viewers: None,
                            visual_origin: None,
                            creation_grant: false,
                            carry: None,
                        },
                    );
                }
            }
        } else {
            reason = Some("Definition unavailable");
        }
        // Programs may have toggled or ended instances: find this one again.
        let at = state
            .abilities
            .instances
            .iter()
            .position(|x| x.cast == instance.cast);
        match (reason, at) {
            (Some(reason), Some(j)) => {
                state.abilities.instances.remove(j);
                end_instance(ctx, state, instance.cast);
                let mark = center
                    .as_ref()
                    .map_or(Mark::point(instance.point), Aim::mark);
                super::emit(
                    ctx,
                    state,
                    &caster,
                    &mark,
                    instance.cast,
                    instance.ability,
                    EventKind::Finished,
                    Extra::reason(reason),
                    None,
                );
                i = j;
            }
            (None, Some(j)) => i = j + 1,
            (_, None) => {}
        }
    }
}
