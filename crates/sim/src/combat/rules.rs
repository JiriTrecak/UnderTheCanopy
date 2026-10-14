//! Combat rules compiled once from content: weapons per definition, the damage-type ×
//! armour-type matrix and the engine-level ids the simulation relies on.
//!
//! # Contracts
//!
//! Content is input, but a few ids are part of the engine's contract with it. They are
//! named here, documented, and checked when a world starts ([`Rules::new`]), instead of
//! appearing as literals in the systems that use them:
//!
//! - [`SPELL_DAMAGE`]: the damage type of spell damage that names no type of its own
//!   (mission `damage`; later ability and item damage that defaults to spell). It must be
//!   declared in `rules.damageTypes` with a matchup row like every other type.
//!
//! The old game also fell back to a `"normal"` damage type when a unit without a weapon
//! was asked whether its weapon could hit something; content never defines it and the
//! question has no answer without a weapon, so here a unit without a weapon simply cannot
//! target anything.

use super::ticks;
use crate::catalog::{Catalog, Def};
use canopy_content::{conditions::Locomotion, game::Kind};

/// The damage type of untyped spell damage (see the module documentation).
pub const SPELL_DAMAGE: &str = "spell";

/// Index of a damage type in content order (sorted by id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DamageType(pub u8);

/// Index of an armour type in content order (sorted by id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArmorType(pub u8);

/// How a weapon delivers its damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Delivery {
    /// Strikes on impact.
    Melee,
    /// Launches a homing missile at `speed` cells per second.
    Missile { speed: u32 },
    /// Lobs a shell at the target's position that lands after `flight` ticks and hits
    /// everything eligible within `radius_milli`, slowing units.
    Shell {
        flight: u32,
        radius_milli: i64,
        slow_permille: u32,
        slow_ticks: u32,
    },
}

/// A charge: from `min_range` to `max_range` cells away the unit rushes its target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChargePolicy {
    pub min_range: u32,
    pub max_range: u32,
    pub cooldown: u32,
    pub duration: u32,
    pub speed_permille: u32,
    pub damage_permille: u32,
}

/// A definition's weapon in simulation units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weapon {
    pub damage: u32,
    pub damage_type: DamageType,
    /// Reach between body edges, cells.
    pub range: f64,
    /// Extra reach a released swing still connects at, cells.
    pub range_buffer: f64,
    /// Cells within which the unit looks for targets.
    pub aggro: u32,
    pub cooldown: u32,
    pub windup: u32,
    pub recovery: u32,
    pub delivery: Delivery,
    pub ground: bool,
    pub air: bool,
    pub charge: Option<ChargePolicy>,
}

impl Weapon {
    /// Shoots (missile or shell) rather than striking.
    pub fn ranged(&self) -> bool {
        !matches!(self.delivery, Delivery::Melee)
    }

    /// Whether it can hit a target that flies (`air`) or stands on the ground.
    pub fn reaches(&self, air: bool) -> bool {
        if air { self.air } else { self.ground }
    }

    /// Wind-up and recovery of one swing at `cycle` ticks (old `attackTiming`): the
    /// authored timeline scaled by the current attack cycle, captured when it begins.
    pub fn timing(&self, cycle: u32) -> (u32, u32) {
        let scale = |part: u32| {
            super::round_ratio(
                u64::from(part),
                u64::from(cycle),
                u64::from(self.cooldown.max(1)),
            ) as u32
        };
        (scale(self.windup).max(1), scale(self.recovery))
    }
}

/// What combat needs of every definition, by [`Def`].
#[derive(Debug, Clone)]
pub struct Profile {
    pub kind: Kind,
    pub air: bool,
    pub hero: bool,
    pub armor: Option<ArmorType>,
    pub weapon: Option<Weapon>,
}

#[derive(Debug, Clone)]
pub struct Rules {
    profiles: Vec<Profile>,
    /// Applies armour, per damage type.
    applies_armor: Vec<bool>,
    armor_types: usize,
    /// Permille per damage type, then armour type.
    matrix: Vec<u32>,
    armor_coefficient: f64,
    pub spell: DamageType,
}

impl Rules {
    /// Compiles the rules; fails when content breaks an engine contract.
    pub fn new(catalog: &Catalog) -> Result<Self, String> {
        let rules = &catalog.content.rules;
        let types: Vec<&str> = rules.damage_types.keys().map(|k| k.as_str()).collect();
        let armors: Vec<&str> = rules.armor_types.keys().map(|k| k.as_str()).collect();
        if types.len() > usize::from(u8::MAX) || armors.len() > usize::from(u8::MAX) {
            return Err("too many damage or armour types".into());
        }
        let damage_type = |id: &str| types.binary_search(&id).ok().map(|i| DamageType(i as u8));
        let spell = damage_type(SPELL_DAMAGE).ok_or_else(|| {
            format!("rules.damageTypes must declare \"{SPELL_DAMAGE}\" (spell damage)")
        })?;
        let mut matrix = vec![0; types.len() * armors.len()];
        for (t, (id, _)) in rules.damage_types.iter().enumerate() {
            let row = rules
                .damage_multipliers
                .get(id)
                .ok_or_else(|| format!("rules.damageMultipliers has no row for {id}"))?;
            for (a, armor) in rules.armor_types.keys().enumerate() {
                matrix[t * armors.len() + a] = *row
                    .get(armor)
                    .ok_or_else(|| format!("rules.damageMultipliers.{id} misses {armor}"))?;
            }
        }
        let mut profiles = Vec::with_capacity(catalog.content.definitions.len());
        for d in &catalog.content.definitions {
            let air =
                d.behaviors.movement.as_ref().and_then(|m| m.locomotion) == Some(Locomotion::Air);
            let armor = match &d.body {
                None => None,
                Some(body) => Some(ArmorType(
                    armors
                        .binary_search(&body.armor_type.as_str())
                        .map_err(|_| format!("{}: unknown armour type", d.id))?
                        as u8,
                )),
            };
            let weapon = match &d.behaviors.combat {
                None => None,
                Some(c) => {
                    let delivery = match (&c.projectile, &c.shell) {
                        (Some(p), _) => Delivery::Missile { speed: p.speed },
                        (None, Some(s)) => Delivery::Shell {
                            flight: ticks(s.flight_ticks),
                            radius_milli: crate::terrain::round_half_up(s.radius * 1000.0) as i64,
                            slow_permille: s.slow_permille,
                            slow_ticks: ticks(s.slow_ticks),
                        },
                        (None, None) => Delivery::Melee,
                    };
                    let (ground, air) = match &c.targets {
                        Some(t) => (
                            t.contains(&Locomotion::Ground),
                            t.contains(&Locomotion::Air),
                        ),
                        None => (true, c.projectile.is_some()),
                    };
                    Some(Weapon {
                        damage: c.damage,
                        damage_type: damage_type(c.damage_type.as_str())
                            .ok_or_else(|| format!("{}: unknown damage type", d.id))?,
                        range: c.range,
                        range_buffer: c.attack.range_buffer,
                        aggro: c.aggro_range,
                        cooldown: ticks(c.cooldown_ticks).max(1),
                        windup: ticks(c.attack.windup_ticks),
                        recovery: ticks(c.attack.recovery_ticks),
                        delivery,
                        ground,
                        air,
                        charge: c.charge.as_ref().map(|c| ChargePolicy {
                            min_range: c.min_range,
                            max_range: c.max_range,
                            cooldown: ticks(c.cooldown_ticks),
                            duration: ticks(c.duration_ticks),
                            speed_permille: c.speed_permille,
                            damage_permille: c.damage_permille,
                        }),
                    })
                }
            };
            profiles.push(Profile {
                kind: d.kind,
                air,
                hero: d.is_hero(),
                armor,
                weapon,
            });
        }
        Ok(Self {
            profiles,
            applies_armor: rules
                .damage_types
                .values()
                .map(|t| t.applies_armor)
                .collect(),
            armor_types: armors.len(),
            matrix,
            armor_coefficient: rules.armor_coefficient,
            spell,
        })
    }

    pub fn profile(&self, def: Def) -> &Profile {
        &self.profiles[usize::from(def.0)]
    }

    pub fn weapon(&self, def: Def) -> Option<&Weapon> {
        self.profile(def).weapon.as_ref()
    }

    /// Whether `attacker`'s weapon can hit an entity of definition `target` (old
    /// `weaponTargets`): by locomotion; buildings stand on the ground.
    pub fn reaches(&self, attacker: Def, target: Def) -> bool {
        self.weapon(attacker)
            .is_some_and(|w| w.reaches(self.profile(target).air))
    }

    /// Damage after the matchup and armour (old `resolveDamage`): at least 1 unless the
    /// matchup is 0. `raw_milli` is the raw damage in thousandths.
    pub fn mitigate(&self, raw_milli: u64, kind: DamageType, armor: ArmorType, value: i32) -> u32 {
        let matchup = self.matrix[usize::from(kind.0) * self.armor_types + usize::from(armor.0)];
        if raw_milli == 0 || matchup == 0 {
            return 0;
        }
        let multiplier = if self.applies_armor[usize::from(kind.0)] {
            self.armor_multiplier(value)
        } else {
            1.0
        };
        let raw = raw_milli as f64 / 1000.0;
        super::round(raw * f64::from(matchup) / 1000.0 * multiplier).max(1.0) as u32
    }

    /// `1 / (1 + k·armour)`; negative armour mirrors it towards double damage.
    pub fn armor_multiplier(&self, armor: i32) -> f64 {
        armor_multiplier(self.armor_coefficient, armor)
    }
}

/// `1 / (1 + k·armour)` (old `armorMultiplier`); negative armour mirrors the curve, so it
/// approaches double damage.
pub fn armor_multiplier(k: f64, armor: i32) -> f64 {
    let armor = f64::from(armor);
    if armor < 0.0 {
        2.0 - 1.0 / (1.0 - k * armor)
    } else {
        1.0 / (1.0 + k * armor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armour_mitigates_and_negative_armour_amplifies() {
        assert_eq!(armor_multiplier(0.06, 0), 1.0);
        assert!((armor_multiplier(0.06, 10) - 1.0 / 1.6).abs() < 1e-12);
        // Mirrored: -10 armour takes 2 − 1/1.6 = 1.375 times the damage.
        assert!((armor_multiplier(0.06, -10) - 1.375).abs() < 1e-12);
        assert!(armor_multiplier(0.06, -1_000_000) < 2.0);
    }

    #[test]
    fn a_swing_scales_with_the_attack_cycle() {
        let weapon = Weapon {
            damage: 1,
            damage_type: DamageType(0),
            range: 1.0,
            range_buffer: 0.5,
            aggro: 5,
            cooldown: 54,
            windup: 12,
            recovery: 10,
            delivery: Delivery::Melee,
            ground: true,
            air: false,
            charge: None,
        };
        assert_eq!(weapon.timing(54), (12, 10));
        // A faster cycle shortens both phases, rounding halves up; wind-up stays ≥ 1.
        assert_eq!(weapon.timing(27), (6, 5));
        assert_eq!(weapon.timing(9), (2, 2));
        assert_eq!(weapon.timing(1), (1, 0));
    }
}
