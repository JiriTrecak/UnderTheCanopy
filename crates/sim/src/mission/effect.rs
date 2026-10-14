//! What a successful run changes outside the mission state. Effects are queued during the
//! run and applied in call order only after the whole run succeeded.

use super::{Script, entity_by_placement_mut, systems::Systems};
use crate::{
    catalog::{Catalog, Def},
    entity::{EntityId, Milli, Unit},
    spawn,
    state::{Outcome, State},
};
use canopy_content::{Id, placement::Owner};

/// Entities are named by placement id and resolved when the effect applies, so a unit
/// spawned earlier in the same run can be ordered.
#[derive(Debug, Clone, PartialEq)]
pub enum MissionEffect {
    /// Create a script-activated placement, joining its camp.
    Spawn {
        placement: String,
        camp: Option<String>,
    },
    /// Order a unit to a cell (M2.2 orders).
    Move {
        placement: String,
        destination: Milli,
        attack_move: bool,
    },
    /// Order a unit to escort another (M2.2 orders).
    Follow { placement: String, target: String },
    /// Order a forced attack (M2.2 orders, M2.4 combat).
    Attack { placement: String, target: String },
    /// Interrupt a unit's work so it can turn; the turn itself is mission state (M2.5
    /// economy interrupt).
    Face { placement: String },
    /// Spell damage from nobody, resolved by the next tick's combat (M2.4).
    Damage {
        target: EntityId,
        source: Option<EntityId>,
        amount: u32,
    },
    /// Restore a living unit's health and mana (M2.4 stats, M2.6 mana).
    Recover { placement: String },
    /// Give a unit to another owner, interrupting its work.
    Transfer { placement: String, owner: Owner },
    /// Turn a unit into another definition, keeping its identity and health fraction.
    Transform {
        placement: String,
        definition: Def,
        owner: Owner,
        camp: Option<String>,
    },
    /// Set a hero item slot; taking an item clamps health and mana (M2.6 items).
    Equip {
        entity: EntityId,
        slot: usize,
        item: Option<Id>,
    },
    /// Player 1 wins the mission.
    Win,
    /// Player 1 loses the mission.
    Lose,
}

/// The campaign player.
pub const CAMPAIGN_PLAYER: Owner = Owner::Player(0);

pub(super) fn apply(
    effect: &MissionEffect,
    state: &mut State,
    catalog: &Catalog,
    script: &Script,
    systems: &dyn Systems,
) {
    match effect {
        MissionEffect::Spawn { placement, camp: _ } => {
            let Some(placement) = script.placement(placement) else {
                return;
            };
            // The definition was checked when the script asked, so creation cannot fail.
            if let Ok(id) = spawn::create(state, catalog, placement) {
                let tick = state.tick;
                if let Some(entity) = state.entities.get_mut(id) {
                    // Created mid-tick: it takes part from the next tick.
                    entity.ready_tick = tick + 1;
                }
            }
            // M2.4: camp membership (`camp`) once units carry it.
        }
        MissionEffect::Transfer { placement, owner } => {
            if let Some(entity) = entity_by_placement_mut(state, placement)
                && entity.unit.is_some()
                && super::alive(entity)
            {
                // M2.5: interrupt the unit's job.
                entity.owner = *owner;
            }
        }
        MissionEffect::Transform {
            placement,
            definition,
            owner,
            camp: _,
        } => {
            let Some(entity) = entity_by_placement_mut(state, placement) else {
                return;
            };
            let before = systems.max_hp(catalog, entity);
            // M2.5: interrupt the unit's job.
            entity.definition = *definition;
            entity.owner = *owner;
            // A fresh unit: orders and transient state end; position is kept.
            entity.unit = Some(Box::<Unit>::default());
            // M2.4: camp membership (`camp`) once units carry it.
            let after = systems.max_hp(catalog, entity);
            let hp = u64::from(entity.hp.unwrap_or(before));
            entity.hp = Some(if before == 0 {
                after.max(1)
            } else {
                // Round half up, as the old game did, in exact integers.
                let scaled =
                    (2 * u64::from(after) * hp + u64::from(before)) / (2 * u64::from(before));
                u32::try_from(scaled).unwrap_or(u32::MAX).max(1)
            });
        }
        MissionEffect::Win => {
            state.outcome = Some(Outcome {
                winner: Some(CAMPAIGN_PLAYER),
                defeated: Vec::new(),
            });
        }
        MissionEffect::Lose => {
            state.outcome = Some(Outcome {
                winner: None,
                defeated: vec![CAMPAIGN_PLAYER],
            });
        }
        // Applied by systems later build steps add; see each variant.
        MissionEffect::Move { .. }
        | MissionEffect::Follow { .. }
        | MissionEffect::Attack { .. }
        | MissionEffect::Face { .. }
        | MissionEffect::Damage { .. }
        | MissionEffect::Recover { .. }
        | MissionEffect::Equip { .. } => {}
    }
}
