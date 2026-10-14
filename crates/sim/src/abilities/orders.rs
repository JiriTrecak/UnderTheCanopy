//! Ability commands and cast orders (old `SpellCastOrders`, the `castAbility`,
//! `abilityAutocast` and `learnAbility` branches of `Game.command`, `learnAbility`): a cast
//! out of range becomes an order that walks into range and casts on arrival.

use super::{
    Ctx,
    cast::{self, Aim, OUT_OF_RANGE},
    library,
};
use crate::{
    action::{Action, CastTarget},
    entity::{EntityId, Milli},
    orders::{Order, discard_navigation, release_combat},
    spatial::Spot,
    state::State,
    stats::Stats,
};
use canopy_content::{abilities::Control, placement::Owner};

/// Whether `action` is an ability command.
pub fn handles(action: &Action) -> bool {
    matches!(
        action,
        Action::CastAbility { .. } | Action::AbilityAutocast { .. } | Action::LearnAbility { .. }
    )
}

pub fn aim(target: &CastTarget) -> Aim {
    match target {
        CastTarget::Unit { entity } => Aim::Unit(EntityId(*entity)),
        CastTarget::Point { position } => Aim::Point(Milli {
            x: position.x as i32 * 1000,
            y: position.y as i32 * 1000,
        }),
    }
}

/// The actor of a single-actor command after the old eligibility rules.
pub fn actor_of(ctx: &Ctx, state: &State, owner: Owner, actor: u32) -> Result<EntityId, String> {
    let id = EntityId(actor);
    let Some(e) = state.entities.get(id) else {
        return Err("No eligible controlled actors".into());
    };
    if e.owner != owner {
        return Err("Not your actors".into());
    }
    let ok = super::actor::alive(e)
        && ctx
            .catalog()
            .get(e.definition)
            .behaviors
            .player_control
            .is_some()
        && e.unit
            .as_ref()
            .is_none_or(|u| u.taunt.is_none() && u.contained.is_none() && u.release.is_none());
    if ok {
        Ok(id)
    } else {
        Err("No eligible controlled actors".into())
    }
}

/// Applies an ability command; `control` is how the owner's slot plays (player or AI).
pub fn apply(
    ctx: &mut Ctx,
    state: &mut State,
    owner: Owner,
    action: &Action,
    control: Control,
) -> Result<(), String> {
    match action {
        Action::AbilityAutocast {
            actor,
            binding,
            enabled,
        } => {
            let id = actor_of(ctx, state, owner, *actor)?;
            let e = state.entities.get(id).expect("actor");
            let policy = ctx.catalog().spells.caster(e.definition);
            let found = policy.and_then(|p| p.binding(binding));
            let ok = match (found, &e.caster) {
                (Some((i, b)), Some(c)) => {
                    b.allows(Control::Player)
                        && b.allows(Control::Ai)
                        && ctx.ability(b.ability).autocast.is_some()
                        && c.ranks.get(i).copied().unwrap_or(0) > 0
                }
                _ => false,
            };
            if !ok {
                return Err("This ability cannot autocast".into());
            }
            let (i, _) = found.expect("binding");
            let c = state
                .entities
                .get_mut(id)
                .and_then(|e| e.caster.as_mut())
                .expect("caster");
            c.autocast.insert(i as u8, *enabled);
            Ok(())
        }
        Action::CastAbility {
            actor,
            binding,
            target,
        } => {
            let id = actor_of(ctx, state, owner, *actor)?;
            issue(ctx, state, id, binding, target, control).map_err(str::to_string)
        }
        Action::LearnAbility { actor, ability } => {
            let id = actor_of(ctx, state, owner, *actor)?;
            learn(ctx, state, id, ability).map_err(str::to_string)
        }
        _ => Err("Invalid request".into()),
    }
}

/// Learns the next rank of a binding (old `learnAbility`).
pub fn learn(
    ctx: &Ctx,
    state: &mut State,
    id: EntityId,
    binding: &str,
) -> Result<(), &'static str> {
    let e = state.entities.get(id).expect("actor");
    let policy = ctx.catalog().spells.caster(e.definition);
    let found = policy.and_then(|p| p.binding(binding));
    let Some(c) = &e.caster else {
        return Err("Cannot learn this ability");
    };
    let Some((index, b)) = found.filter(|(_, b)| b.learning.is_some()) else {
        return Err("Cannot learn this ability");
    };
    if !super::actor::alive(e) || c.pending.is_some() || c.weapon_order.is_some() {
        return Err("Cannot learn this ability");
    }
    let level = Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner)).level;
    let rank = c.ranks.get(index).copied().unwrap_or(0);
    let Some(&required) = b
        .learning
        .as_ref()
        .expect("learning")
        .get(usize::from(rank))
    else {
        return Err("Ability is fully learned");
    };
    if required > level {
        return Err("Hero level is too low");
    }
    let bindings = &policy.expect("policy").bindings;
    let spent: u32 = bindings
        .iter()
        .enumerate()
        .map(|(i, b)| {
            u32::from(
                c.ranks
                    .get(i)
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(b.initial_rank),
            )
        })
        .sum();
    if spent >= level {
        return Err("No skill points available");
    }
    state
        .entities
        .get_mut(id)
        .and_then(|e| e.caster.as_mut())
        .expect("caster")
        .ranks[index] += 1;
    Ok(())
}

/// Casts now, or walks into range first (old `SpellCastOrders.issue`).
pub fn issue(
    ctx: &mut Ctx,
    state: &mut State,
    id: EntityId,
    binding: &str,
    target: &CastTarget,
    control: Control,
) -> Result<(), &'static str> {
    let aim = aim(target);
    match cast::reason(ctx, state, id, binding, aim, control, false) {
        Err(reason) if reason != OUT_OF_RANGE => Err(reason),
        Ok(()) => {
            let was_order = state
                .entities
                .get(id)
                .and_then(|e| e.unit.as_ref())
                .and_then(|u| u.order.clone())
                .filter(|o| matches!(o, Order::CastAbility { .. }));
            cast::cast(ctx, state, id, binding, aim, control)?;
            if let Some(previous) = was_order
                && let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut())
                && u.order.as_ref() == Some(&previous)
            {
                u.order = None;
                discard_navigation(u);
            }
            Ok(())
        }
        Err(reason) => {
            cast::reason(ctx, state, id, binding, aim, control, true)?;
            let e = state.entities.get(id).expect("caster");
            let movable = e.unit.as_ref().is_some_and(|u| u.work.cargo.is_none())
                && ctx.catalog().get(e.definition).behaviors.movement.is_some();
            if !movable {
                return Err(reason);
            }
            let order = Order::CastAbility {
                binding: binding.to_string(),
                target: target.clone(),
                owner: e.owner,
                control,
            };
            if crate::orders::issue(state, id, order, false) {
                Ok(())
            } else {
                Err("Caster cannot move to cast")
            }
        }
    }
}

/// Cast orders: cast once in range, else approach (old `SpellCastOrders.tick`). The second
/// pass after movement (`plan == false`) only casts.
pub fn tick(ctx: &mut Ctx, state: &mut State, plan: bool) {
    let tick = state.tick;
    let units: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| {
            e.ready_tick <= tick
                && crate::spatial::present(e)
                && matches!(
                    e.unit.as_ref().and_then(|u| u.order.as_ref()),
                    Some(Order::CastAbility { .. })
                )
        })
        .map(|e| e.id)
        .collect();
    for id in units {
        let e = state.entities.get(id).expect("unit");
        let Some(Order::CastAbility {
            binding,
            target,
            owner,
            control,
        }) = e.unit.as_ref().and_then(|u| u.order.clone())
        else {
            continue;
        };
        let aim = aim(&target);
        let reason = if e.owner != owner {
            Err("Caster changed owner")
        } else {
            cast::reason(ctx, state, id, &binding, aim, control, false)
        };
        match reason {
            Ok(()) => {
                let _ = cast::cast(ctx, state, id, &binding, aim, control);
                continue;
            }
            Err(r) if r != OUT_OF_RANGE => {
                let u = state
                    .entities
                    .get_mut(id)
                    .and_then(|e| e.unit.as_mut())
                    .expect("unit");
                u.order = None;
                discard_navigation(u);
                release_combat(u);
                continue;
            }
            Err(_) => {}
        }
        let u = e.unit.as_ref().expect("unit");
        if !plan || u.retry_at > tick {
            continue;
        }
        let point = match aim {
            Aim::Unit(t) => state.entities.get(t).map_or(e.position, |t| t.position),
            Aim::Point(p) => p,
        };
        let Some((index, b)) = ctx
            .catalog()
            .spells
            .caster(e.definition)
            .and_then(|p| p.binding(&binding))
        else {
            continue;
        };
        let spell = ctx.ability(b.ability);
        let rank = e
            .caster
            .as_ref()
            .and_then(|c| c.ranks.get(index))
            .copied()
            .unwrap_or(0);
        let range = library::value(spell, rank, &spell.targeting.range) as f64;
        let (tx, ty) = (f64::from(point.x) / 1000.0, f64::from(point.y) / 1000.0);
        // Keep a valid route while tracking a moving target; retry blocked traffic slowly.
        if !u.route.is_empty()
            && let Some(goal) = u.goal
        {
            let g = ctx.spatial.spot(goal);
            let (dx, dy) = (f64::from(g.x) - tx, f64::from(g.y) - ty);
            if dx * dx + dy * dy <= range * range {
                continue;
            }
        }
        let from = (
            f64::from(e.position.x) / 1000.0,
            f64::from(e.position.y) / 1000.0,
        );
        let deck = ctx.spatial.deck_of(e);
        let u = state
            .entities
            .get_mut(id)
            .and_then(|e| e.unit.as_mut())
            .expect("unit");
        discard_navigation(u);
        release_combat(u);
        u.retry_at = tick + 12;
        let mut seen: Vec<(i32, i32)> = Vec::new();
        let mut candidates: Vec<(i32, i32, f64)> = Vec::new();
        let base = crate::math::atan2(from.1 - ty, from.0 - tx);
        for fraction in [1.0, 0.75, 0.5, 0.25, 0.0] {
            for i in 0..32 {
                let angle = base + f64::from(i) * std::f64::consts::PI / 16.0;
                let reach = (range - 0.5).max(0.0) * fraction;
                let x = cast::js_round(tx + crate::math::cos(angle) * reach) as i32;
                let y = cast::js_round(ty + crate::math::sin(angle) * reach) as i32;
                if seen.contains(&(x, y)) {
                    continue;
                }
                let (dx, dy) = (f64::from(x) - tx, f64::from(y) - ty);
                if dx * dx + dy * dy > range * range {
                    continue;
                }
                seen.push((x, y));
                if ctx.spatial.valid_spot(Spot::new(x, y, deck)) {
                    let (fx, fy) = (f64::from(x) - from.0, f64::from(y) - from.1);
                    candidates.push((x, y, fx * fx + fy * fy));
                }
            }
        }
        candidates.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
        for (x, y, _) in candidates {
            let e = state.entities.get_mut(id).expect("unit");
            if ctx.spatial.route(e, Spot::new(x, y, deck), false) {
                break;
            }
        }
    }
}
