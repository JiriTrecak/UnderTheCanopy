//! What follows a death (old end of `Combat.resolve` and `Game.onCombatDeath`): pursuers
//! who saw it stop, the killer's side earns experience, a camp may pay out, the body
//! becomes a corpse, witnesses remember it, and the entity leaves the world, except a hero,
//! who is kept as fallen.

use super::{CombatState, Corpse, resolve::finish_witnessed, senses::Senses};
use crate::{
    camps,
    entity::{EntityId, Place},
    heroes,
    state::{FactKind, State},
};
use canopy_content::{conditions::UnitNature, placement::Owner};

/// Ticks a corpse lasts.
pub const CORPSE_TICKS: u32 = 2400;
/// Most corpses kept; the oldest go first.
pub const MAX_CORPSES: usize = 512;

/// Forgets expired corpses.
pub fn expire_corpses(combat: &mut CombatState, tick: u32) {
    combat.corpses.retain(|c| c.expires > tick);
}

/// Who struck a lethal blow: the source entity and the owner credited with the kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lethal {
    pub source: EntityId,
    pub owner: Owner,
}

/// A death to process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dead {
    pub id: EntityId,
    pub lethal: Option<Lethal>,
}

/// Processes the deaths of one resolution, in id order: kill reactions, witnessed pursuits,
/// experience, then each death. Vision sensors should already exclude the dead
/// (`Vision::sync`).
pub fn process(ctx: &mut crate::abilities::Ctx, state: &mut State, dead: &[Dead]) {
    let _span = tracing::trace_span!("deaths").entered();
    for d in dead {
        if let Some(l) = d.lethal {
            crate::abilities::reactions::kill(ctx, state, l.source, d.id, Some(l.owner));
        }
    }
    let senses = ctx.senses;
    for d in dead {
        finish_witnessed(&senses, state, d.id);
    }
    for d in dead {
        let Some(owner) = d.lethal.map(|l| l.owner).filter(|&o| o != Owner::Nobody) else {
            continue;
        };
        let Some(target) = state.entities.get(d.id) else {
            continue;
        };
        let camp = target
            .unit
            .as_ref()
            .and_then(|u| u.camp.as_ref())
            .map(|c| c.id.as_str());
        if senses.opponents(owner, None, target.owner, camp) {
            heroes::award(state, senses.catalog, senses.teams, ctx.limits, d.id, owner);
        }
    }
    for d in dead {
        credited(ctx, state, d.id, d.lethal);
    }
}

/// One death (old `Game.onCombatDeath`).
pub fn die(ctx: &mut crate::abilities::Ctx, state: &mut State, id: EntityId) {
    credited(ctx, state, id, None);
}

/// One death, the kill credited to `lethal`'s owner in the observers' death log.
fn credited(
    ctx: &mut crate::abilities::Ctx,
    state: &mut State,
    id: EntityId,
    lethal: Option<Lethal>,
) {
    let Some(e) = state.entities.get(id) else {
        return;
    };
    if e.fallen || e.hp.is_none_or(|hp| hp > 0) {
        return;
    }
    ctx.events.fallen(crate::abilities::events::Fallen {
        sequence: 0,
        tick: state.tick,
        entity: id,
        definition: e.definition,
        owner: e.owner,
        killer: lethal.map(|l| l.owner).filter(|&k| k != Owner::Nobody),
        source: lethal.map(|l| l.source),
    });
    // Abilities hear of the death and its casts end; units other than heroes drop items.
    crate::abilities::reactions::death(ctx, state, id);
    crate::abilities::cast::cancel(ctx, state, id, "Caster died");
    let senses = ctx.senses;
    camps::on_death(state, senses.catalog, ctx.spatial, senses.camps, id);
    crate::neutrals::on_death(state, senses.catalog, ctx.spatial, id);
    crate::items::inventory::on_death(ctx, state, id);
    capture_corpse(&senses, state, id);
    senses
        .vision
        .record_death(state, senses.catalog, senses.terrain, senses.rules, id);
    let e = state.entities.get_mut(id).expect("dead");
    e.statuses.clear();
    let (owner, hero) = (e.owner, senses.combat.profile(e.definition).hero);
    state.fact(owner, FactKind::Death, "Entity destroyed");
    // Split forms and carriers let go of what they hold (old `onRemoving`).
    crate::abilities::world_ops::removing(ctx, state, id);
    crate::abilities::world_ops::release_hosted(ctx, state, id);
    // Jobs, employment, carried goods and paid tasks of the dead (old `Economy.remove`).
    {
        let mut ectx = ctx.economy.ctx(state, senses.catalog, ctx.spatial);
        crate::economy::work::release_references(&mut ectx, id, false);
    }
    crate::garrisons::evict(state, ctx.spatial, id);
    crate::tunnels::collapse(state, ctx.spatial, id);
    let held = state.entities.get(id).expect("dead").clone();
    crate::economy::work::record_losses(state, senses.catalog, &held, false);
    if hero {
        let e = state.entities.get_mut(id).expect("dead");
        heroes::retain(e);
        if let Some(q) = &mut e.equipment {
            q.statuses.clear();
            q.hits.clear();
        }
        crate::items::tidy(e);
        if let Some(c) = &mut e.caster {
            c.pending = None;
            c.weapon_order = None;
        }
    } else {
        state.entities.remove(id);
    }
}

/// Keeps the body of a dead ordinary unit (old `SpellCorpses.capture`).
fn capture_corpse(senses: &Senses, state: &mut State, id: EntityId) {
    let e = state.entities.get(id).expect("dead");
    let d = senses.catalog.get(e.definition);
    let Some(u) = &e.unit else {
        return;
    };
    if d.is_hero()
        || crate::abilities::actor::summoned(e)
        || d.unit_nature == Some(UnitNature::Mechanical)
        || u.contained.is_some()
        || u.garrison.is_some()
        || u.release.is_some()
        || state.combat.corpses.iter().any(|c| c.id == id)
    {
        return;
    }
    let last = senses.terrain.size() as i32 - 1;
    let (x, y) = e.position.cell();
    let corpse = Corpse {
        id,
        definition: e.definition,
        owner: e.owner,
        position: Place {
            x: x.clamp(0, last),
            y: y.clamp(0, last),
            surface: e.surface.clone(),
        },
        rotation: e.rotation,
        died: state.tick,
        expires: state.tick + CORPSE_TICKS,
        camp: u.camp.as_ref().map(|c| c.id.clone()),
        progression: e.progression.clone(),
        caster: e.caster.as_ref().map(|c| {
            Box::new(crate::abilities::Caster {
                mana: 0,
                regeneration: 0,
                pending: None,
                weapon_order: None,
                ..(**c).clone()
            })
        }),
    };
    let tick = state.tick;
    expire_corpses(&mut state.combat, tick);
    state.combat.corpses.push(corpse);
    state.combat.corpses.sort_by_key(|c| (c.died, c.id));
    let excess = state.combat.corpses.len().saturating_sub(MAX_CORPSES);
    state.combat.corpses.drain(..excess);
}
