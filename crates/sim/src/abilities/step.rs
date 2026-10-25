//! The ability passes of the tick (old `AbilityRuntime.tick`, `ambient`, `resolve`,
//! `auras`, `SpellStatuses.tick` and the ability lines of `Game.tick`).

use super::{
    Ctx, actor, ai, cast, deliveries, library, orders, persistent,
    program::{self, Aim, Ops, Run},
    reactions, status_ops, summons, taunt, weapons, world_ops,
};
use crate::state::State;
use canopy_content::abilities::{Activation, Anchor, Operation};

/// Step 2: the ability lifecycle, cast orders, autocast and taunts.
pub fn begin(ctx: &mut Ctx, state: &mut State) {
    let _span = tracing::trace_span!("ability lifecycle").entered();
    lifecycle(ctx, state);
    orders::tick(ctx, state, true);
    ai::ambient(ctx, state);
    taunt::reconcile_all(ctx, state);
}

/// Statuses and links, auras, weapon orders, casts and mana (old `tick`).
pub fn lifecycle(ctx: &mut Ctx, state: &mut State) {
    world_ops::tick_containment(ctx, state);
    world_ops::tick_splits(ctx, state);
    let tick = state.tick;
    let alive_source = |state: &State, id| state.entities.get(id).is_some_and(actor::alive);
    let visions: Vec<bool> = state
        .knowledge
        .spells
        .iter()
        .map(|v| v.expires > tick && v.caster.is_none_or(|c| alive_source(state, c)))
        .collect();
    let mut keep = visions.into_iter();
    state
        .knowledge
        .spells
        .retain(|_| keep.next().unwrap_or(false));
    crate::combat::deaths::expire_corpses(&mut state.combat, tick);
    summons::tick(ctx, state);
    status_ops::tick(ctx, state);
    {
        let _span = tracing::trace_span!("auras").entered();
        auras(ctx, state);
    }
    weapons::tick(ctx, state);
    cast::tick_casters(ctx, state);
}

/// Step 6: cast orders whose casters arrived in range during movement.
pub fn after_movement(ctx: &mut Ctx, state: &mut State) {
    let _span = tracing::trace_span!("cast approach").entered();
    orders::tick(ctx, state, false);
}

/// Step 8: reactions, persistent pulses, deliveries, releases and channels, then taunts.
pub fn resolve(ctx: &mut Ctx, state: &mut State) {
    let _span = tracing::trace_span!("ability delivery").entered();
    reactions::resolve(ctx, state);
    persistent::tick(ctx, state);
    deliveries::tick(ctx, state);
    cast::resolve_casters(ctx, state);
    taunt::reconcile_all(ctx, state);
}

/// Passive auras (old `auras`): source and query statuses once per holder, then each unit
/// in radius; every aura status lasts one tick and is rebuilt.
fn auras(ctx: &mut Ctx, state: &mut State) {
    // Statuses move nobody: while only they are applied, positions taken once serve every
    // holder's search.
    let mut standing = deliveries::Standing::default();
    let still = |ops: &[&Operation]| {
        ops.iter().all(|op| match op {
            Operation::Effect(e) => matches!(e, canopy_content::abilities::Effect::Status(_)),
            Operation::Branch(b) => b
                .then
                .iter()
                .chain(&b.otherwise)
                .all(|e| matches!(e, canopy_content::abilities::Effect::Status(_))),
        })
    };
    for id in cast::casters(state) {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        let Some(policy) = ctx.catalog().spells.caster(e.definition) else {
            continue;
        };
        let ranks = e
            .caster
            .as_ref()
            .map(|c| c.ranks.clone())
            .unwrap_or_default();
        let mut holder: Option<actor::Actor> = None;
        for (index, b) in policy.bindings.iter().enumerate() {
            let spell = ctx.ability(b.ability);
            let rank = ranks.get(index).copied().unwrap_or(0);
            let Some(aura) = &spell.aura else {
                continue;
            };
            if rank == 0
                || spell.activation != Activation::Passive
                || library::parameters(spell, rank).is_none()
            {
                continue;
            }
            let h = match &holder {
                Some(h) => h.clone(),
                None => {
                    let Some(a) = actor::get(&ctx.senses, state, id) else {
                        break;
                    };
                    holder = Some(a.clone());
                    a
                }
            };
            if !h.alive || !h.targetable {
                break;
            }
            let once: Vec<&Operation> = spell
                .on_release
                .iter()
                .filter(|op| match op {
                    Operation::Effect(e) => e.target() == Anchor::Caster || e.query().is_some(),
                    Operation::Branch(_) => false,
                })
                .collect();
            if !once.is_empty() {
                run(
                    ctx,
                    state,
                    &h,
                    Aim::Unit(Box::new(h.clone())),
                    b.ability,
                    rank,
                    Ops::Picked(&once),
                );
                if !still(&once) {
                    standing.forget();
                }
            }
            let per: Vec<&Operation> = spell
                .on_release
                .iter()
                .filter(|op| match op {
                    Operation::Effect(e) => !(e.target() == Anchor::Caster || e.query().is_some()),
                    Operation::Branch(_) => true,
                })
                .collect();
            if per.is_empty() {
                continue;
            }
            let center = (h.x(), h.y());
            let near = deliveries::candidates_among(
                ctx,
                state,
                &h,
                spell,
                center,
                aura.radius,
                standing.of(state),
            );
            let moves = !still(&per);
            for target in near {
                if aura.melee_only && !target.melee {
                    continue;
                }
                run(
                    ctx,
                    state,
                    &h,
                    Aim::Unit(Box::new(target)),
                    b.ability,
                    rank,
                    Ops::Picked(&per),
                );
                if moves {
                    standing.forget();
                }
            }
        }
    }
}

fn run(
    ctx: &mut Ctx,
    state: &mut State,
    holder: &actor::Actor,
    aim: Aim,
    ability: library::AbilityId,
    rank: u8,
    ops: Ops,
) {
    program::run(
        ctx,
        state,
        Run {
            caster: holder.clone(),
            aim,
            cast: 1,
            ability,
            rank,
            power: 1000,
            aura: true,
            area_defaults: false,
            operations: ops,
            origin_viewers: None,
            visual_origin: None,
            creation_grant: false,
            carry: None,
        },
    );
}
