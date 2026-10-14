//! The seam between the AI and abilities and items (M2.6; old `abilities/ai.ts` and the
//! ability and item parts of `ai/tactics.ts`).
//!
//! Everything the AI needs from the ability and item runtime goes through this module,
//! so the M2.6 step fills it in one place:
//!
//! - [`casting`], [`stunned`]: whether an own unit is busy casting or stunned. Free
//!   workers, healthy soldiers, reactions and hero decisions skip such units.
//! - [`gear`]: a hero's carried items, their ready ticks and its mana and ability
//!   cooldowns. [`super::tactics`] picks items up, drops weaker ones and uses healing
//!   from it; without gear nothing is picked up.
//! - [`pickup_target`]: the item a unit's current order picks up (order continuity, hero
//!   decisions, healthy soldiers).
//! - [`ability_actions`]: learning and casting, which runs first each decision and claims
//!   its casters for the beat.
//!
//! # What `ability_actions` must do (old `abilityActions`)
//!
//! For each own unit in id order that has abilities, no pending cast, no weapon-cast
//! order and is not stunned:
//! 1. Learn: if the hero's spent points (ranks above initial) are below its level, pick
//!    the binding with AI control whose next rank's required level is reached, sorted by
//!    that required level (highest first), then current rank (lowest first), then the
//!    race's `skillPreference` order, then binding order; emit `learnAbility` and move
//!    to the next caster when it is accepted for the beat.
//! 2. Cast: for each binding with an `ai` policy and AI control, skipping passive and
//!    autocast abilities, unlearnt ones, ones on cooldown or without mana, and active
//!    toggles: with a strategic intent (`escape`, `reinforce`, `scout`) take the first
//!    sampled aim (old `strategicAbilityAims`, bounded samples, terrain validity and
//!    sight from the frame); otherwise score every observed candidate in range with the
//!    same operation recipients execution uses (old `abilityAimScore`), add corpse
//!    aims for point abilities that raise the dead, and cast at the best positive score
//!    (ties to the lower id). One cast per caster per beat. (`limits.spellCandidates`
//!    exists in content but the old AI never read it.)
//!
//! Known old bug to fix there (D14): the old AI scores only the first `onRelease`
//! operation of an ability, so abilities whose value lies in a later operation are
//! undervalued. Score every operation in order against the predicted recipients.

use super::{
    Beat,
    frame::{Frame, Known},
};
use crate::{catalog::Def, entity::Entity};

/// A hero's carried items as the AI may know them (its own hero only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gear {
    /// Item definition per slot.
    pub slots: Vec<Option<Def>>,
    /// First tick each slot's active use is ready again.
    pub ready: Vec<u32>,
    pub mana: u32,
    /// Ticks at which its abilities come off cooldown.
    pub cooldowns: Vec<u32>,
}

/// Whether the unit is casting (M2.6).
pub fn casting(_e: &Entity) -> bool {
    false
}

/// Whether the unit is stunned (M2.6).
pub fn stunned(_e: &Entity) -> bool {
    false
}

/// The hero's gear, when it carries items (M2.6).
pub fn gear(_f: &Frame, _hero: &Known) -> Option<Gear> {
    None
}

/// The item an own unit's current order picks up (M2.6).
pub fn pickup_target(_f: &Frame, _e: &Known) -> Option<u32> {
    None
}

/// Learns and casts abilities (M2.6; see the module documentation).
pub fn ability_actions(_beat: &mut Beat) {}
