//! Defeat and victory in skirmish (old `Game.isDefeated`, the colony cleanup at the end of
//! `Game.tick`, `colony.ts`). A colony without buildings, finished or not, is defeated and
//! its remaining units leave the match; the match ends when at most one team remains.
//! Missions decide their outcome by script; sandboxes never end.

use crate::{
    catalog::Catalog,
    economy::{self, Ctx, Index},
    state::{FactKind, Outcome, State},
    teams::Teams,
};
use canopy_content::{game::Kind, placement::Owner};
use std::collections::BTreeSet;

/// How a map decides its outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rules {
    /// Annihilation: colonies without buildings are defeated.
    Skirmish,
    /// The mission script wins or loses.
    Mission,
    /// Never ends.
    Sandbox,
}

/// Whether `owner` still has a building (old `hasColonyBuildings`).
pub fn has_buildings(state: &State, catalog: &Catalog, index: &Index, owner: Owner) -> bool {
    index.buildings.iter().any(|&id| {
        state.entities.get(id).is_some_and(|e| {
            e.owner == owner && economy::alive(e) && catalog.kind(e.definition) == Kind::Building
        })
    })
}

/// Whether `owner` is out of the match (old `isDefeated`).
pub fn defeated(
    rules: Rules,
    state: &State,
    catalog: &Catalog,
    index: &Index,
    owner: Owner,
) -> bool {
    match rules {
        Rules::Sandbox => false,
        Rules::Mission => state
            .outcome
            .as_ref()
            .is_some_and(|o| o.defeated.contains(&owner)),
        Rules::Skirmish => !has_buildings(state, catalog, index, owner),
    }
}

/// Step 11 (skirmish): defeated colonies are cleared away and the match ends when at most
/// one team is left. Returns whether anything changed (sight needs another pass).
pub fn advance(ctx: &mut Ctx, teams: &Teams) -> bool {
    let owners = teams.owners().to_vec();
    let defeated: Vec<Owner> = owners
        .iter()
        .copied()
        .filter(|&o| !has_buildings(ctx.state, ctx.catalog, ctx.index, o))
        .collect();
    if defeated.is_empty() {
        return false;
    }
    let mut changed = false;
    for &owner in &defeated {
        let abandoned: Vec<_> = ctx
            .state
            .entities
            .iter()
            .filter(|e| e.owner == owner)
            .map(|e| e.id)
            .collect();
        if !abandoned.is_empty() {
            economy::stock::record(
                ctx.state,
                owner,
                FactKind::Death,
                "message.colony-defeated",
                None,
            );
            changed = true;
        }
        for id in abandoned {
            if ctx.state.entities.get(id).is_some() {
                economy::work::remove(ctx, id, false);
            }
        }
    }
    let remaining: Vec<Owner> = owners
        .iter()
        .copied()
        .filter(|o| !defeated.contains(o))
        .collect();
    let left: BTreeSet<u8> = remaining.iter().filter_map(|&o| teams.team_of(o)).collect();
    if left.len() <= 1 {
        ctx.state.outcome = Some(Outcome {
            winner: remaining.first().copied(),
            defeated,
        });
        changed = true;
    }
    changed
}
