//! Resolved unit statistics (old `stats.ts`): the definition, its level from experience,
//! then modifiers. One path for simulation, views and AI.
//!
//! Modifier hooks, filled by the steps that add their sources:
//! - research (M2.5): [`research`] lists an owner's completed research;
//! - items, statuses and permanent bonuses (M2.6): join in [`Stats::of`] where marked.

use crate::{catalog::Catalog, entity::Entity, state::State};
use canopy_content::{Id, placement::Owner};

/// Slowest and fastest movement, permille of the authored speed.
pub const MOVE_SPEED_RANGE: (u32, u32) = (200, 1800);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// 1-based.
    pub level: u32,
    pub max_hp: u32,
    /// Weapon damage in thousandths (research percentages stay exact).
    pub damage_milli: u64,
    pub armor: i32,
    pub cooldown_ticks: u32,
    pub max_mana: u32,
    pub move_speed_permille: u32,
}

/// An owner's completed research (sorted, as the old game kept it).
pub fn research(state: &State, owner: Owner) -> &[Id] {
    state.economy.researched(owner)
}

impl Stats {
    /// The entity's statistics with `research` (its owner's) applied.
    pub fn of(catalog: &Catalog, e: &Entity, research: &[Id]) -> Self {
        let d = catalog.get(e.definition);
        let levels = d.behaviors.progression.as_ref().map(|p| &p.levels);
        let experience = e.progression.as_ref().map_or(0, |p| p.experience);
        let (level, row) = match levels {
            Some(levels) if !levels.is_empty() => {
                let reached = levels.iter().filter(|l| l.experience <= experience).count();
                (reached as u32, Some(&levels[reached.saturating_sub(1)]))
            }
            _ => (d.level.unwrap_or(1), None),
        };
        let combat = d.behaviors.combat.as_ref();
        let mut max_hp = row.map_or(d.body.as_ref().map_or(0, |b| b.max_hp), |l| l.max_hp);
        let damage = row.map_or(combat.map_or(0, |c| c.damage), |l| l.damage);
        let mut armor = row.map_or(d.body.as_ref().map_or(0, |b| b.armor), |l| l.armor) as i32;
        let cooldown = crate::combat::ticks(
            row.map_or(combat.map_or(1, |c| c.cooldown_ticks), |l| l.cooldown_ticks),
        );
        let max_mana = row.map_or(
            d.behaviors.abilities.as_ref().map_or(0, |a| a.max_mana),
            |l| l.max_mana,
        );
        // M2.6: permanent bonuses, equipment and item modifiers add here.
        let mut damage_bonus: i64 = 0;
        for id in research {
            let Some(r) = catalog.content.rules.research.get(id) else {
                continue;
            };
            for effect in r.effects.iter().filter(|e| e.units.contains(&d.id)) {
                max_hp += effect.max_hp.unwrap_or(0);
                armor += effect.armor.unwrap_or(0) as i32;
                damage_bonus += i64::from(effect.damage_permille.unwrap_or(0));
            }
        }
        let attack_speed: u64 = 1000;
        let move_speed = move_speed_permille(e);
        Self {
            level,
            max_hp,
            damage_milli: u64::from(damage) * (1000 + damage_bonus).max(0) as u64,
            armor,
            cooldown_ticks: (crate::combat::round_ratio(
                u64::from(cooldown),
                1000,
                attack_speed.max(200),
            ) as u32)
                .max(1),
            max_mana,
            move_speed_permille: move_speed,
        }
    }

    /// Weapon damage rounded to whole points (old `Math.round`).
    pub fn damage_points(&self) -> u64 {
        (self.damage_milli + 500) / 1000
    }
}

/// Maximum health without research (definition and level).
pub fn max_hp(catalog: &Catalog, e: &Entity) -> u32 {
    Stats::of(catalog, e, &[]).max_hp
}

/// Movement speed in permille of the authored speed: slows (M2.6: item modifiers and slow
/// immunity), clamped to [`MOVE_SPEED_RANGE`].
pub fn move_speed_permille(e: &Entity) -> u32 {
    let slow = e.unit.as_ref().map_or(0, |u| u.combat.slow());
    // Old `round(1000 · (1000 − slow) / 1000)`, exact in integers.
    (1000u32.saturating_sub(slow)).clamp(MOVE_SPEED_RANGE.0, MOVE_SPEED_RANGE.1)
}
