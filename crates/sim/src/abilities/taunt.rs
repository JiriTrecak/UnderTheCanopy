//! Taunts (old `taunt.ts`): a taunted combat unit attacks the living source of its newest
//! taunt status, then resumes. The taunt owns navigation, never the saved orders.

use super::{Ctx, actor, cast, library::control, statuses};
use crate::{
    entity::EntityId,
    orders::{discard_navigation, release_combat},
    state::State,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[skip_serializing_none]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Taunt {
    pub source: EntityId,
    /// Where an interrupted job resumes (economy, M2.5).
    pub resume_goal: Option<u32>,
}

/// The unit the newest valid taunt forces `id` to fight.
fn taunter(ctx: &Ctx, state: &State, id: EntityId) -> Option<EntityId> {
    let catalog = ctx.catalog();
    let e = state.entities.get(id)?;
    let u = e.unit.as_ref()?;
    let tick = state.tick;
    if !actor::alive(e)
        || u.contained.is_some()
        || u.garrison.is_some()
        || u.release.is_some()
        || catalog.get(e.definition).behaviors.combat.is_none()
        || statuses::control_immune(catalog, e, control::TAUNT)
    {
        return None;
    }
    let mut candidates: Vec<&super::Status> = e
        .statuses
        .iter()
        .filter(|s| {
            s.expires > tick
                && s.blocked & control::TAUNT == 0
                && catalog.spells.status(s.key).taunt
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.started
            .cmp(&a.started)
            .then(b.cast.cmp(&a.cast))
            .then(a.source.cmp(&b.source))
    });
    candidates.into_iter().find_map(|s| {
        let source = state.entities.get(s.source)?;
        let su = source.unit.as_ref();
        (actor::alive(source)
            && source.owner == s.owner
            && su.is_none_or(|u| {
                u.contained.is_none() && u.garrison.is_none() && u.release.is_none()
            })
            && ctx.senses.hostile(e, source))
        .then_some(source.id)
    })
}

/// Follows the unit's taunt statuses (old `reconcileTaunt`).
pub fn reconcile(ctx: &mut Ctx, state: &mut State, id: EntityId) {
    let Some(e) = state.entities.get(id) else {
        return;
    };
    let Some(u) = e.unit.as_ref() else {
        return;
    };
    let source = taunter(ctx, state, id);
    if u.taunt.map(|t| t.source) == source {
        return;
    }
    let tick = state.tick;
    let u = state
        .entities
        .get_mut(id)
        .expect("unit")
        .unit
        .as_mut()
        .expect("unit");
    // M2.5: a worker's job goal is kept to resume after the taunt.
    let resume_goal = u.taunt.and_then(|t| t.resume_goal);
    discard_navigation(u);
    release_combat(u);
    u.idle = None;
    u.retry_at = tick;
    match source {
        Some(source) => {
            u.taunt = Some(Taunt {
                source,
                resume_goal,
            });
            cast::cancel(ctx, state, id, "Taunted");
        }
        None => {
            u.taunt = None;
        }
    }
}

/// Every unit with a taunt or statuses (old `reconcileTaunts`), in id order.
pub fn reconcile_all(ctx: &mut Ctx, state: &mut State) {
    let ids: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| {
            e.unit
                .as_ref()
                .is_some_and(|u| u.taunt.is_some() || !e.statuses.is_empty())
        })
        .map(|e| e.id)
        .collect();
    for id in ids {
        reconcile(ctx, state, id);
    }
}
