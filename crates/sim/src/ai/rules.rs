//! What an AI player plays by: its timing and limits (code, the same for every race) and
//! the army it recruits, read from its race's data — never from lists of unit names
//! (D53). Hero abilities are learned in the order the hero lists them.

use crate::catalog::Catalog;
use canopy_content::{
    Id,
    game::{Creation, Definition, Kind, ProductionMode},
};

#[derive(Debug, Clone, PartialEq)]
pub struct AiWorkers {
    pub minimum: u32,
    pub target: u32,
    pub maximum: u32,
    pub reserve: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiArmy {
    pub minimum: u32,
    pub maximum: u32,
    pub home_guard: u32,
    pub engage_permille: u32,
    pub camp_permille: u32,
    pub retreat_health_permille: u32,
    pub pursuit_radius: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiLimits {
    pub commands_per_beat: u32,
    pub placement_candidates: u32,
}

/// A unit the AI recruits, and how much of the army it aims for relative to the others.
#[derive(Debug, Clone, PartialEq)]
pub struct AiComposition {
    pub definition: Id,
    pub weight: u32,
}

/// One AI player's rules; timings in simulation ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct AiRules {
    pub decision_ticks: u32,
    pub economy_ticks: u32,
    pub strategy_ticks: u32,
    pub operation_ticks: u32,
    pub reaction_ticks: u32,
    pub order_interval_ticks: u32,
    pub workers: AiWorkers,
    pub army: AiArmy,
    pub limits: AiLimits,
    /// The race's recruits, by id.
    pub composition: Vec<AiComposition>,
}

impl AiRules {
    /// The rules of an AI playing `race` (the default race when `None`).
    pub fn for_race(catalog: &Catalog, race: Option<&str>) -> Self {
        let t = crate::combat::ticks;
        let rules = &catalog.content.rules;
        let race = race.unwrap_or(&rules.default_race);
        Self {
            // Authored ticks (25 ms), converted.
            decision_ticks: t(10).max(1),
            economy_ticks: t(80),
            strategy_ticks: t(200),
            operation_ticks: t(40),
            reaction_ticks: t(20),
            order_interval_ticks: t(40),
            workers: AiWorkers {
                minimum: 6,
                target: 16,
                maximum: 28,
                reserve: 2,
            },
            army: AiArmy {
                minimum: 6,
                maximum: 44,
                home_guard: 2,
                engage_permille: 1100,
                camp_permille: 1400,
                retreat_health_permille: 280,
                pursuit_radius: 20,
            },
            limits: AiLimits {
                commands_per_beat: 4,
                placement_candidates: 24,
            },
            composition: recruits(catalog, race),
        }
    }
}

/// Total goods a definition costs to make; 0 when it is not made.
fn cost(d: &Definition) -> u32 {
    d.creation
        .as_ref()
        .map_or(0, |c| c.items().iter().map(|c| c.amount).sum())
}

/// The race's army: every trained, non-hero fighter of its roster that one of its own
/// buildings trains on order, weighted so each takes an even share of the spending
/// (cheaper units are more numerous). The economy's match-up against the enemy's armour
/// shifts the mix from there.
fn recruits(catalog: &Catalog, race: &str) -> Vec<AiComposition> {
    let content = &catalog.content;
    // Content without races (test worlds) is one race of everything.
    let roster: Vec<&Definition> = match content.rules.race(race) {
        Some(profile) => profile
            .race
            .roster
            .iter()
            .filter_map(|id| content.definition(id.as_str()))
            .collect(),
        None if content.rules.races.is_empty() => content.definitions.iter().collect(),
        None => return Vec::new(),
    };
    let trained = |d: &Definition| {
        roster.iter().any(|b| {
            b.kind == Kind::Building
                && b.behaviors
                    .production
                    .as_ref()
                    .is_some_and(|p| p.mode == ProductionMode::Queued && p.outputs.contains(&d.id))
        })
    };
    let mut out: Vec<AiComposition> = roster
        .iter()
        .filter(|d| {
            d.kind == Kind::Unit
                && !d.is_hero()
                && d.behaviors.combat.is_some()
                && d.behaviors.work.is_none()
                && matches!(d.creation, Some(Creation::Train { .. }))
                && cost(d) > 0
                && trained(d)
        })
        .map(|d| AiComposition {
            definition: d.id.clone(),
            weight: 100_000 / cost(d),
        })
        .collect();
    out.sort_by(|a, b| a.definition.cmp(&b.definition));
    out
}
