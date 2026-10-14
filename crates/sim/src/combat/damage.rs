//! The damage pipeline every hit goes through (old `Combat.damage`): spell damage
//! multipliers and immunities, the damage-type matchup and armour, then invulnerability
//! and item shields, then spell immunity, mana shields and spell shields.

use super::{rules::DamageType, senses::Senses};
use crate::{entity::EntityId, state::State, stats::Stats};

/// Damage `raw_milli` (thousandths) of `kind` does to `target`, consuming shields.
pub fn damage(
    senses: &Senses,
    state: &mut State,
    target: EntityId,
    raw_milli: u64,
    kind: DamageType,
) -> u32 {
    let catalog = senses.catalog;
    let Some(e) = state.entities.get(target) else {
        return 0;
    };
    let spell = senses.combat.spell;
    let multiplier = crate::abilities::statuses::damage_multiplier(catalog, e, kind.0, spell.0);
    let raw = raw_milli as f64 / 1000.0 * multiplier / 1000.0;
    if raw <= 0.0 {
        return 0;
    }
    let Some(armor) = senses.combat.profile(e.definition).armor else {
        return 0;
    };
    let value = Stats::of(catalog, e, crate::stats::research(state, e.owner)).armor;
    let mitigated = senses.combat.mitigate_points(raw, kind, armor, value);
    let e = state.entities.get_mut(target).expect("target");
    let after_items = crate::items::effects::absorb_with(catalog, e, mitigated);
    crate::abilities::statuses::absorb(catalog, e, after_items, kind.0, spell.0)
}

/// Whether hits of `kind` cannot hurt the unit at all (old `spellDamageMultiplier === 0`).
pub fn immune(senses: &Senses, state: &State, target: EntityId, kind: DamageType) -> bool {
    state.entities.get(target).is_some_and(|e| {
        crate::abilities::statuses::damage_multiplier(
            senses.catalog,
            e,
            kind.0,
            senses.combat.spell.0,
        ) == 0.0
    })
}
