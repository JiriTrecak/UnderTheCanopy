//! Resolved unit statistics (old `stats.ts`): the definition (or its form's combat profile),
//! its level from experience, permanent bonuses, equipment, research, then the modifiers of
//! statuses and items. One path for simulation, views and AI.

use crate::{catalog::Catalog, entity::Entity, state::State};
use canopy_content::{Id, placement::Owner};

/// Slowest and fastest movement, permille of the authored speed.
pub const MOVE_SPEED_RANGE: (u32, u32) = (200, 1800);
/// Caps of the combined modifiers.
pub const MAX_COOLDOWN_REDUCTION: i64 = 400;
pub const MAX_LIFESTEAL: i64 = 400;
pub const MAX_MELEE_REFLECTION: i64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub cooldown_reduction_permille: u32,
    pub lifesteal_permille: u32,
    pub melee_reflection_permille: u32,
    /// Points per second.
    pub health_regen: f64,
    pub mana_regen: f64,
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
        // A form's combat profile supplies the weapon (old `formDefinition`).
        let profile = crate::abilities::forms::weapon_def(catalog, e);
        let combat = catalog.get(profile).behaviors.combat.as_ref();
        let mut max_hp =
            i64::from(row.map_or(d.body.as_ref().map_or(0, |b| b.max_hp), |l| l.max_hp));
        let mut damage = i64::from(row.map_or(combat.map_or(0, |c| c.damage), |l| l.damage));
        let mut armor = i64::from(row.map_or(d.body.as_ref().map_or(0, |b| b.armor), |l| l.armor));
        let cooldown = crate::combat::ticks(
            row.map_or(combat.map_or(1, |c| c.cooldown_ticks), |l| l.cooldown_ticks),
        );
        let mut max_mana = i64::from(row.map_or(
            d.behaviors.abilities.as_ref().map_or(0, |a| a.max_mana),
            |l| l.max_mana,
        ));
        let mut health_regen = row.map_or(0.0, |l| l.health_regen_per_second);
        let mut mana_regen = row.map_or(
            d.behaviors
                .abilities
                .as_ref()
                .map_or(0.0, |a| a.mana_regen_per_second),
            |l| l.mana_regen_per_second,
        );
        let (bonus_hp, bonus_damage, bonus_armor, bonus_mana) =
            crate::items::flat_stats(catalog, e);
        max_hp += bonus_hp;
        damage += bonus_damage;
        armor += bonus_armor;
        max_mana += bonus_mana;
        let mut damage_bonus: i64 = 0;
        for id in research {
            let Some(r) = catalog.content.rules.research.get(id) else {
                continue;
            };
            for effect in r.effects.iter().filter(|e| e.units.contains(&d.id)) {
                max_hp += i64::from(effect.max_hp.unwrap_or(0));
                armor += i64::from(effect.armor.unwrap_or(0));
                damage_bonus += i64::from(effect.damage_permille.unwrap_or(0));
            }
        }
        let mut attack_speed: i64 = 1000;
        let mut move_speed: i64 = 1000;
        let mut cooldown_reduction: i64 = 0;
        let mut lifesteal: i64 = 0;
        let mut reflection: i64 = 0;
        if !e.statuses.is_empty() || e.equipment.is_some() {
            use crate::abilities::library::control;
            let immune = crate::abilities::statuses::control_immunities(catalog, e);
            let (immune_move, immune_attack) = (
                immune & control::MOVE_SLOW != 0,
                immune & control::ATTACK_SLOW != 0,
            );
            crate::abilities::statuses::for_each_modifier(catalog, e, &mut |m| {
                max_hp += m.max_hp;
                max_mana += m.max_mana;
                damage += m.damage;
                armor += m.armor;
                health_regen += m.health_regen;
                mana_regen += m.mana_regen;
                move_speed += if immune_move {
                    m.move_speed_permille.max(0)
                } else {
                    m.move_speed_permille
                };
                cooldown_reduction += m.cooldown_reduction_permille;
                lifesteal += m.lifesteal_permille;
                reflection += m.melee_reflection_permille;
                attack_speed += if immune_attack {
                    m.attack_speed_permille.max(0)
                } else {
                    m.attack_speed_permille
                };
                damage_bonus += m.damage_permille;
            });
        }
        Self {
            level,
            max_hp: max_hp.clamp(0, i64::from(u32::MAX)) as u32,
            damage_milli: (damage * (1000 + damage_bonus).max(0)).max(0) as u64,
            armor: armor.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            cooldown_ticks: (crate::combat::round_ratio(
                u64::from(cooldown),
                1000,
                attack_speed.max(200) as u64,
            ) as u32)
                .max(1),
            max_mana: max_mana.clamp(0, i64::from(u32::MAX)) as u32,
            move_speed_permille: slowed(catalog, e, move_speed),
            cooldown_reduction_permille: cooldown_reduction.clamp(0, MAX_COOLDOWN_REDUCTION) as u32,
            lifesteal_permille: lifesteal.clamp(0, MAX_LIFESTEAL) as u32,
            melee_reflection_permille: reflection.clamp(0, MAX_MELEE_REFLECTION) as u32,
            health_regen,
            mana_regen,
        }
    }

    /// Weapon damage rounded to whole points (old `Math.round`).
    pub fn damage_points(&self) -> u64 {
        (self.damage_milli + 500) / 1000
    }
}

/// Maximum health without research (definition, level, items and statuses).
pub fn max_hp(catalog: &Catalog, e: &Entity) -> u32 {
    Stats::of(catalog, e, &[]).max_hp
}

/// Movement speed in permille of the authored speed: modifiers, then the strongest shell
/// slow unless immune, clamped to [`MOVE_SPEED_RANGE`] (old `entityStats`).
pub fn move_speed_permille(catalog: &Catalog, e: &Entity) -> u32 {
    if e.statuses.is_empty() && e.equipment.is_none() {
        return slowed(catalog, e, 1000);
    }
    let immune = crate::abilities::statuses::control_immune(
        catalog,
        e,
        crate::abilities::library::control::MOVE_SLOW,
    );
    let mut speed: i64 = 1000;
    crate::abilities::statuses::for_each_modifier(catalog, e, &mut |m| {
        speed += if immune {
            m.move_speed_permille.max(0)
        } else {
            m.move_speed_permille
        };
    });
    slowed(catalog, e, speed)
}

fn slowed(catalog: &Catalog, e: &Entity, speed: i64) -> u32 {
    let immune = (!e.statuses.is_empty() || e.equipment.is_some())
        && crate::abilities::statuses::control_immune(
            catalog,
            e,
            crate::abilities::library::control::MOVE_SLOW,
        );
    let slow = if immune {
        0
    } else {
        e.unit.as_ref().map_or(0, |u| u.combat.slow()).min(1000)
    };
    // Old `Math.round(speed · (1000 − slow) / 1000)`.
    let scaled = (speed as f64 * f64::from(1000 - slow) / 1000.0 + 0.5).floor();
    (scaled as i64).clamp(i64::from(MOVE_SPEED_RANGE.0), i64::from(MOVE_SPEED_RANGE.1)) as u32
}
