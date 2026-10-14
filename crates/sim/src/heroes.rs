//! Experience and heroes (old `progression.ts`, `heroRoster.ts`, the hero half of
//! `Revival.retain`).
//!
//! One reward pool per combat death goes to the killer's team: units with levels near the
//! victim share it (by id, remainders to the lowest ids), else, when the rules allow it,
//! every eligible unit of the killer anywhere. Neutral kills pay less per level and stop
//! paying at a level cap; a lone hero earns a bonus that grows with roster capacity.
//!
//! Dead heroes are kept as **fallen** entities with their identity and experience, so
//! revival ([`crate::economy::revival`]) brings the same hero back.

use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId, Unit},
    state::{FactKind, State},
    stats::Stats,
    teams::Teams,
};
use canopy_content::{Id, game::Definition, placement::Owner};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progression {
    pub experience: u32,
}

/// The progression a new entity of `definition` starts with.
pub fn initial(definition: &Definition, experience: Option<u32>) -> Option<Progression> {
    definition
        .behaviors
        .progression
        .as_ref()
        .map(|_| Progression {
            experience: experience.unwrap_or(0),
        })
}

/// A hero waiting for revival.
pub fn fallen(e: &Entity) -> bool {
    e.fallen
}

/// Keeps a dead hero as a fallen entity (old `Revival.retain`): no health, a fresh unit,
/// nothing carried. It stays in the entity table with its id and experience but takes part
/// in nothing until revived.
pub fn retain(e: &mut Entity) {
    // It lies on the cell it fell on (old units dropped their sub-cell position).
    let (x, y) = e.position.cell();
    e.position = crate::entity::Milli {
        x: x * crate::entity::Milli::CELL,
        y: y * crate::entity::Milli::CELL,
    };
    e.hp = Some(0);
    e.fallen = true;
    e.unit = Some(Box::<Unit>::default());
    e.inventory.clear();
}

/// The heroes an owner has (alive, fallen or in training) and how many distinct heroes its
/// halls allow (old `heroRoster`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    pub capacity: u32,
    pub definitions: Vec<Id>,
}

impl Roster {
    pub fn of(state: &State, catalog: &Catalog, owner: Owner) -> Self {
        let mut roster = Self::default();
        for e in state.entities.iter().filter(|e| e.owner == owner) {
            let d = catalog.get(e.definition);
            if e.unit.is_some() || d.kind == canopy_content::game::Kind::Building {
                if d.is_hero() && !roster.definitions.contains(&d.id) {
                    roster.definitions.push(d.id.clone());
                }
                if e.hp.is_some_and(|hp| hp == 0) {
                    continue;
                }
                if e.construction.is_none() {
                    roster.capacity = roster.capacity.max(d.hero_capacity.unwrap_or(0));
                }
                // Paid recruits hold their place.
                for q in e.production.iter().flat_map(|p| &p.queue) {
                    let queued = catalog.get(q.definition);
                    if queued.is_hero() && !roster.definitions.contains(&queued.id) {
                        roster.definitions.push(queued.id.clone());
                    }
                }
            }
        }
        roster
    }

    pub fn used(&self) -> usize {
        self.definitions.len()
    }
}

/// Map-level limits on experience.
#[derive(Debug, Clone, Copy, Default)]
pub struct Limits {
    /// A mission's hero level cap.
    pub level_cap: Option<u32>,
}

/// Pays experience for `dead`, killed by a hit credited to `owner` (old
/// `Progression.award`). The caller checked that the kill was hostile.
pub fn award(
    state: &mut State,
    catalog: &Catalog,
    teams: &Teams,
    limits: Limits,
    dead: EntityId,
    owner: Owner,
) {
    if owner == Owner::Nobody {
        return;
    }
    let Some(victim) = state.entities.get(dead) else {
        return;
    };
    let definition = catalog.get(victim.definition);
    if definition.kind != canopy_content::game::Kind::Unit {
        return;
    }
    let policy = &catalog.content.rules.experience;
    let table = if definition.is_hero() {
        &policy.hero_rewards
    } else {
        &policy.unit_rewards
    };
    let level = Stats::of(catalog, victim, crate::stats::research(state, victim.owner)).level;
    let base = definition.experience_yield.unwrap_or_else(|| {
        let index = (level as usize).min(table.len()).saturating_sub(1);
        table.get(index).copied().unwrap_or(0)
    });
    // M2.6: summoned victims pay `summonMultiplierPermille`.
    let reward = base;
    if reward == 0 {
        return;
    }
    let neutral = victim.owner == Owner::Nobody;
    let (vx, vy) = (i64::from(victim.position.x), i64::from(victim.position.y));
    let ceiling = |e: &Entity| {
        let levels = &catalog
            .get(e.definition)
            .behaviors
            .progression
            .as_ref()
            .expect("progression")
            .levels;
        let mut top = levels.len();
        if let Some(cap) = limits.level_cap {
            top = top.min(cap as usize);
        }
        if neutral {
            top = top.min(policy.neutral_level_cap as usize);
        }
        levels[top.max(1) - 1].experience
    };
    let tick = state.tick;
    let eligible = |e: &Entity| {
        e.hp.is_some_and(|hp| hp > 0)
            && e.ready_tick <= tick
            && e.unit
                .as_ref()
                .is_some_and(|u| u.contained.is_none() && u.release.is_none())
            && e.progression
                .as_ref()
                .is_some_and(|p| p.experience < ceiling(e))
            && e.owner != Owner::Nobody
            && teams.allied(e.owner, owner)
    };
    let candidates: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| eligible(e))
        .map(|e| e.id)
        .collect();
    let nearby: Vec<EntityId> = candidates
        .iter()
        .copied()
        .filter(|&id| {
            let e = state.entities.get(id).expect("candidate");
            let radius = i64::from(
                catalog
                    .get(e.definition)
                    .behaviors
                    .progression
                    .as_ref()
                    .expect("progression")
                    .experience_radius,
            ) * 1000;
            let (dx, dy) = (i64::from(e.position.x) - vx, i64::from(e.position.y) - vy);
            dx * dx + dy * dy <= radius * radius
        })
        .collect();
    let heroes = if !nearby.is_empty() {
        nearby
    } else if policy.global_fallback {
        candidates
            .into_iter()
            .filter(|&id| state.entities.get(id).is_some_and(|e| e.owner == owner))
            .collect()
    } else {
        Vec::new()
    };
    let count = heroes.len() as u32;
    for (i, id) in heroes.into_iter().enumerate() {
        let e = state.entities.get(id).expect("hero");
        let research = crate::stats::research(state, e.owner);
        let before = Stats::of(catalog, e, research);
        let share = reward / count + u32::from((i as u32) < reward % count);
        let roster = Roster::of(state, catalog, e.owner);
        let bonus = if roster.used() == 1 {
            let bonuses = &policy.solo_hero_tier_bonus_permille;
            (roster.capacity as usize)
                .min(bonuses.len())
                .checked_sub(1)
                .and_then(|i| bonuses.get(i))
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        let multiplier = if neutral {
            (before.level as usize)
                .checked_sub(1)
                .and_then(|i| policy.neutral_multipliers_permille.get(i))
                .copied()
                .unwrap_or(0)
        } else {
            1000
        };
        let amount =
            u64::from(share) * u64::from(multiplier) * (1000 + u64::from(bonus)) / 1_000_000;
        let cap = ceiling(e);
        let owner = e.owner;
        let name = catalog.get(e.definition).name.clone();
        let e = state.entities.get_mut(id).expect("hero");
        let progression = e.progression.as_mut().expect("progression");
        progression.experience =
            (u64::from(progression.experience) + amount).min(u64::from(cap)) as u32;
        let after = {
            let e = state.entities.get(id).expect("hero");
            Stats::of(catalog, e, crate::stats::research(state, owner))
        };
        // Damage and mana already spent stay spent: levelling is not a refill.
        let e = state.entities.get_mut(id).expect("hero");
        if let Some(hp) = &mut e.hp {
            *hp =
                (i64::from(*hp) + i64::from(after.max_hp) - i64::from(before.max_hp)).max(0) as u32;
        }
        // M2.6: mana follows the same rule.
        if after.level > before.level {
            state.fact(
                owner,
                FactKind::Command,
                format!("{name} reached level {}", after.level),
            );
        }
    }
}
