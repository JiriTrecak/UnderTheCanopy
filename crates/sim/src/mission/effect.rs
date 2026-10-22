//! What a successful run changes outside the mission state. Effects are queued during the
//! run and applied in call order only after the whole run succeeded.

use super::{Script, entity_by_placement, entity_by_placement_mut, systems::Systems};
use crate::{
    catalog::{Catalog, Def},
    entity::{EntityId, Milli, Place, Unit},
    orders::{self, Order},
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
    /// Order a unit to a cell.
    Move {
        placement: String,
        destination: Milli,
        attack_move: bool,
    },
    /// Order a unit to escort another.
    Follow { placement: String, target: String },
    /// Order a forced attack.
    Attack { placement: String, target: String },
    /// Interrupt a unit's work so it can turn; the turn itself is mission state.
    Face { placement: String },
    /// Spell damage from nobody, resolved by the next tick's combat.
    Damage {
        target: EntityId,
        source: Option<EntityId>,
        amount: u32,
    },
    /// Restore a living unit's health and mana.
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
    /// Set a hero item slot; taking an item clamps health and mana.
    Equip {
        entity: EntityId,
        slot: usize,
        item: Option<Id>,
    },
    /// A script's order to a computer player of the mission (zero-based player).
    Ai {
        player: u8,
        order: crate::ai::campaign::Order,
    },
    /// A campaign variable or the campaign player's tech changes (`crate::campaign`).
    Campaign(crate::campaign::Change),
    /// Put a unit at a cell at once, ending what it does (a skipped scene's walk);
    /// `walking`: only while it still has an order.
    Place {
        placement: String,
        destination: Milli,
        walking: bool,
    },
    /// Turn a unit to face a point at once (a skipped scene's turn).
    Turn { placement: String, target: Milli },
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
        MissionEffect::Spawn { placement, camp } => {
            let Some(placement) = script.placement(placement) else {
                return;
            };
            // The definition was checked when the script asked, so creation cannot fail.
            if let Ok(id) = spawn::create(state, catalog, placement) {
                let tick = state.tick;
                if let Some(entity) = state.entities.get_mut(id) {
                    // Created mid-tick: it takes part from the next tick.
                    entity.ready_tick = tick + 1;
                    join_camp(entity, camp.as_deref());
                }
            }
        }
        MissionEffect::Move {
            placement,
            destination,
            attack_move,
        } => {
            if let Some(id) = living_unit(state, placement) {
                let order = Order::Move {
                    destination: Place::new(
                        destination.x / Milli::CELL,
                        destination.y / Milli::CELL,
                    ),
                    attack_move: *attack_move,
                };
                orders::issue(state, id, order, false);
            }
        }
        MissionEffect::Follow { placement, target } => {
            let target = super::entity_by_placement(state, target)
                .filter(|t| super::alive(t))
                .map(|t| t.id);
            if let Some(target) = target
                && let Some(id) = living_unit(state, placement)
            {
                orders::issue(
                    state,
                    id,
                    Order::Follow {
                        target,
                        escort: true,
                    },
                    false,
                );
            }
        }
        MissionEffect::Attack { placement, target } => {
            let target = super::entity_by_placement(state, target)
                .filter(|t| super::alive(t))
                .map(|t| t.id);
            if let Some(target) = target
                && let Some(id) = living_unit(state, placement)
            {
                orders::issue(
                    state,
                    id,
                    Order::Attack {
                        target,
                        force: true,
                    },
                    false,
                );
            }
        }
        MissionEffect::Face { placement } => {
            if let Some(id) = living_unit(state, placement) {
                crate::economy::interrupt(state, id, None);
            }
        }
        MissionEffect::Damage {
            target,
            source,
            amount,
        } => {
            state.combat.scripted.push(crate::combat::ScriptedHit {
                target: *target,
                source: *source,
                amount: *amount,
            });
        }
        MissionEffect::Recover { placement } => {
            let research = entity_by_placement(state, placement)
                .map(|e| crate::stats::research(state, e.owner).to_vec())
                .unwrap_or_default();
            if let Some(entity) = entity_by_placement_mut(state, placement)
                && entity.unit.is_some()
                && super::alive(entity)
                && !systems.fallen(entity)
            {
                let stats = crate::stats::Stats::of(catalog, entity, &research);
                entity.hp = Some(stats.max_hp);
                if let Some(c) = &mut entity.caster {
                    c.mana = stats.max_mana;
                }
            }
        }
        MissionEffect::Transfer { placement, owner } => {
            if let Some(id) = living_unit(state, placement) {
                crate::economy::interrupt(state, id, None);
                state.entities.get_mut(id).expect("unit").owner = *owner;
            }
        }
        MissionEffect::Transform {
            placement,
            definition,
            owner,
            camp,
        } => {
            let Some(id) = entity_by_placement_mut(state, placement).map(|e| e.id) else {
                return;
            };
            crate::economy::interrupt(state, id, None);
            let entity = state.entities.get_mut(id).expect("entity");
            let before = systems.max_hp(catalog, entity);
            entity.definition = *definition;
            entity.owner = *owner;
            // A fresh unit: orders and transient state end; position is kept.
            entity.unit = Some(Box::<Unit>::default());
            // Item statuses end with the old body; spell statuses carry over (old game).
            if let Some(equipment) = &mut entity.equipment {
                equipment.statuses.clear();
            }
            crate::items::tidy(entity);
            join_camp(entity, camp.as_deref());
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
        MissionEffect::Ai { player, order } => {
            crate::ai::campaign::command(state, *player, order);
        }
        MissionEffect::Place {
            placement,
            destination,
            walking,
        } => {
            let Some(id) = living_unit(state, placement) else {
                return;
            };
            if *walking && !state.entities.get(id).is_some_and(|e| systems.has_order(e)) {
                return;
            }
            if !crate::economy::interrupt(state, id, None) {
                return;
            }
            let entity = state.entities.get_mut(id).expect("unit");
            if let Some(u) = entity.unit.as_mut() {
                crate::orders::discard_navigation(u);
                u.order = None;
            }
            entity.position = *destination;
        }
        MissionEffect::Turn { placement, target } => {
            if let Some(entity) = entity_by_placement_mut(state, placement)
                && entity.unit.is_some()
                && super::alive(entity)
            {
                let rotation = i64::from(entity.rotation);
                entity.rotation =
                    super::systems::heading(entity.position, rotation, *target) as i32;
            }
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
        MissionEffect::Campaign(change) => crate::campaign::apply(state, change),
        MissionEffect::Equip { entity, slot, item } => {
            let item = item.as_ref().and_then(|id| catalog.find(id.as_str()));
            crate::items::inventory::set_slot(catalog, state, *entity, *slot, item);
        }
    }
}

/// The living unit placed as `placement`.
fn living_unit(state: &State, placement: &str) -> Option<crate::entity::EntityId> {
    super::entity_by_placement(state, placement)
        .filter(|e| e.unit.is_some() && super::alive(e))
        .map(|e| e.id)
}

/// Makes a unit a member of a camp.
fn join_camp(entity: &mut crate::entity::Entity, camp: Option<&str>) {
    if let (Some(u), Some(camp)) = (entity.unit.as_mut(), camp) {
        u.camp = Some(crate::camps::Member {
            id: camp.to_string(),
            returning: false,
        });
    }
}
