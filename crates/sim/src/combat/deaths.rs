//! What follows a death (old end of `Combat.resolve` and `Game.onCombatDeath`): pursuers
//! who saw it stop, the killer's side earns experience, a camp may pay out, the body
//! becomes a corpse, witnesses remember it, and the entity leaves the world, except a hero,
//! who is kept as fallen.

use super::{CombatState, Corpse, resolve::finish_witnessed, senses::Senses};
use crate::{
    camps,
    entity::{EntityId, Place},
    heroes,
    spatial::Spatial,
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

/// Processes the deaths of one resolution, in id order. `credit` is the owner of each
/// lethal hit. Vision sensors must already exclude the dead (`Vision::sync`).
pub fn process(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    economy: &mut crate::economy::Economy,
    limits: heroes::Limits,
    dead: &[(EntityId, Option<Owner>)],
) {
    let _span = tracing::trace_span!("deaths").entered();
    // M2.6: lethal-hit reactions run here.
    for &(id, _) in dead {
        finish_witnessed(senses, state, id);
    }
    for &(id, credit) in dead {
        let Some(owner) = credit.filter(|&o| o != Owner::Nobody) else {
            continue;
        };
        let target = state.entities.get(id).expect("dead");
        let camp = target
            .unit
            .as_ref()
            .and_then(|u| u.camp.as_ref())
            .map(|c| c.id.as_str());
        if senses.opponents(owner, None, target.owner, camp) {
            heroes::award(state, senses.catalog, senses.teams, limits, id, owner);
        }
    }
    for &(id, _) in dead {
        die(senses, spatial, state, economy, id);
    }
}

/// One death (old `Game.onCombatDeath`).
pub fn die(
    senses: &Senses,
    spatial: &mut Spatial,
    state: &mut State,
    economy: &mut crate::economy::Economy,
    id: EntityId,
) {
    let Some(e) = state.entities.get(id) else {
        return;
    };
    if e.fallen || e.hp.is_none_or(|hp| hp > 0) {
        return;
    }
    // M2.6: abilities hear of the death and the caster's casts end; non-hero units drop
    // carried items.
    camps::on_death(state, senses.catalog, spatial, senses.camps, id);
    capture_corpse(senses, state, id);
    senses
        .vision
        .record_death(state, senses.catalog, senses.terrain, senses.rules, id);
    let e = state.entities.get(id).expect("dead");
    let (owner, hero) = (e.owner, senses.combat.profile(e.definition).hero);
    state.fact(owner, FactKind::Death, "Entity destroyed");
    // Jobs, employment, carried goods and paid tasks of the dead (old `Economy.remove`).
    {
        let mut ctx = economy.ctx(state, senses.catalog, spatial);
        crate::economy::work::release_references(&mut ctx, id, false);
    }
    crate::garrisons::evict(state, spatial, id);
    let held = state.entities.get(id).expect("dead").clone();
    crate::economy::work::record_losses(state, senses.catalog, &held, false);
    if hero {
        heroes::retain(state.entities.get_mut(id).expect("dead"));
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
    // M2.6: summoned units leave no corpse either.
    if d.is_hero()
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
    };
    let tick = state.tick;
    expire_corpses(&mut state.combat, tick);
    state.combat.corpses.push(corpse);
    state.combat.corpses.sort_by_key(|c| (c.died, c.id));
    let excess = state.combat.corpses.len().saturating_sub(MAX_CORPSES);
    state.combat.corpses.drain(..excess);
}
