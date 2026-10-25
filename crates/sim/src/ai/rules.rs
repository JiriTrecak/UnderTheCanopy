//! What an AI player plays by: its timing and limits (code, the same for every race), its
//! style (D75) and the army it recruits, read from its race's data — never from lists of
//! unit names (D53). Hero abilities are learned in the order the hero lists them.

use crate::{catalog::Catalog, setup::Difficulty};
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

/// How an AI plays (D75), chosen per player from the match seed. Every style reads the
/// same race data; they differ in how many workers they keep, when they invest in tiers,
/// expansions and research, when they attack, and which tier of recruit they favour (by
/// the units' `level`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AiStyle {
    /// The all-round player: a steady economy, tiers when it has an army, a mixed force.
    Standard,
    /// Few workers and an early wave of first-tier units, before the enemy can answer.
    Rush,
    /// Many workers and an early second hall; attacks late with a bigger economy behind it.
    Expand,
    /// Advances the tiers first and favours the higher-level units they unlock.
    Tech,
}

impl AiStyle {
    pub const ALL: [AiStyle; 4] = [Self::Standard, Self::Rush, Self::Expand, Self::Tech];

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Rush => "rush",
            Self::Expand => "expand",
            Self::Tech => "tech",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The style of `player` in a match of `seed`: the same on every peer.
    pub fn of_match(seed: u32, player: u8) -> Self {
        let mixed =
            (seed ^ (u32::from(player) + 1).wrapping_mul(0x85eb_ca6b)).wrapping_mul(0x9e37_79b1);
        Self::ALL[(mixed >> 28) as usize % Self::ALL.len()]
    }

    /// How much more (or less) of a recruit of `level` this style aims for, in permille.
    fn level_permille(self, level: u32) -> u32 {
        match (self, level) {
            (Self::Rush, 0..=1) => 4000,
            (Self::Rush, 2) => 1000,
            (Self::Rush, _) => 250,
            (Self::Tech, level) => 1000 * level.clamp(1, 4).pow(2),
            _ => 1000,
        }
    }
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
    pub style: AiStyle,
    /// Fighters it fields before it spends on tiers, research, heroes and outposts.
    pub invest_army: u32,
    /// Claims a second field once it has this many workers, whatever the first holds.
    pub early_expansion: Option<u32>,
    /// Until this tick the army attacks at its minimum size without creeping first or
    /// waiting to outmatch what it has seen: a rush.
    pub rush_until: u32,
    /// The workers it keeps once a rush is over (a rush that did not end the match turns
    /// to its economy).
    pub workers_after_rush: AiWorkers,
    /// One more producer of fighters for every this many fighters (up to three).
    pub army_per_producer: u32,
    /// Fighters it aims for per worker beyond the minimum.
    pub army_per_worker: u32,
    /// The race's recruits, by id.
    pub composition: Vec<AiComposition>,
    /// Definitions and research the mission's tech forbids ([`super::Brains::forbid`]).
    pub forbidden: Vec<Id>,
}

impl AiRules {
    /// These rules as a computer player of `difficulty` plays them. Every level plays the
    /// same AI for now (Jiri, 2026-10-25: one AI is pushed first, then the levels are
    /// decided); the levels will differ here.
    pub fn at(self, difficulty: Difficulty) -> Self {
        match difficulty {
            Difficulty::Easy | Difficulty::Normal | Difficulty::Hard => self,
        }
    }

    /// Whether a spending command would make something the mission's tech forbids.
    pub fn forbids(&self, action: &crate::action::Action) -> bool {
        use crate::action::Action;
        if self.forbidden.is_empty() {
            return false;
        }
        let made = match action {
            Action::Build { definition, .. } | Action::Produce { definition, .. } => {
                definition.as_str()
            }
            Action::Research { research, .. } => research.as_str(),
            _ => return false,
        };
        self.forbidden.iter().any(|f| f.as_str() == made)
    }

    /// The workers it keeps at `tick`.
    pub fn workers_at(&self, tick: u32) -> &AiWorkers {
        if self.rush_until == 0 || tick < self.rush_until {
            &self.workers
        } else {
            &self.workers_after_rush
        }
    }

    /// The rules of an AI playing `race` (the default race when `None`) in `style`.
    pub fn for_race(catalog: &Catalog, race: Option<&str>, style: AiStyle) -> Self {
        let t = crate::combat::ticks;
        let rules = &catalog.content.rules;
        let race = race.unwrap_or(&rules.default_race);
        let workers = AiWorkers {
            minimum: 6,
            target: 16,
            maximum: 28,
            reserve: 2,
        };
        let standard = Self {
            // Authored ticks (25 ms), converted.
            decision_ticks: t(10).max(1),
            economy_ticks: t(80),
            strategy_ticks: t(200),
            operation_ticks: t(40),
            reaction_ticks: t(20),
            order_interval_ticks: t(40),
            workers: workers.clone(),
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
            style,
            invest_army: 4,
            early_expansion: None,
            rush_until: 0,
            workers_after_rush: workers.clone(),
            army_per_producer: 12,
            army_per_worker: 2,
            composition: recruits(catalog, race, style),
            forbidden: Vec::new(),
        };
        match style {
            AiStyle::Standard => standard,
            AiStyle::Rush => Self {
                workers: AiWorkers {
                    target: 10,
                    maximum: 18,
                    ..standard.workers
                },
                army: AiArmy {
                    minimum: 5,
                    engage_permille: 900,
                    ..standard.army
                },
                invest_army: 10,
                rush_until: t(12 * 60 * 40),
                workers_after_rush: standard.workers.clone(),
                army_per_producer: 3,
                army_per_worker: 5,
                ..standard
            },
            AiStyle::Expand => Self {
                workers: AiWorkers {
                    target: 22,
                    maximum: 36,
                    ..standard.workers
                },
                army: AiArmy {
                    minimum: 8,
                    engage_permille: 1250,
                    ..standard.army
                },
                invest_army: 5,
                early_expansion: Some(14),
                ..standard
            },
            AiStyle::Tech => Self {
                army: AiArmy {
                    minimum: 7,
                    ..standard.army
                },
                invest_army: 2,
                ..standard
            },
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
/// (cheaper units are more numerous), then by the style's taste for its tier. The
/// economy's match-up against the enemy's armour shifts the mix from there.
fn recruits(catalog: &Catalog, race: &str, style: AiStyle) -> Vec<AiComposition> {
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
            weight: (100_000 / cost(d) * style.level_permille(d.level.unwrap_or(1)) / 1000).max(1),
        })
        .collect();
    out.sort_by(|a, b| a.definition.cmp(&b.definition));
    out
}
