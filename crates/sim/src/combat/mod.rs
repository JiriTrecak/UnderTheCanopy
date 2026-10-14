//! Combat (old `combat.ts`, `attackApproach.ts`, `attackPositions.ts`, `targetIndex.ts`,
//! `combatIntent.ts`, `charge.ts`, `attackTiming.ts`, `damage.ts`, `missiles.ts`,
//! `shellCombat.ts` and the death handling of `game.ts`).
//!
//! **Planning** (step 5, [`plan::plan`]): every ready unit on the map, in id order,
//! advances its weapon timeline, keeps or drops its target, acquires the nearest hostile
//! one (staggered: a unit looks around on ticks where `tick % 8 == id % 8`), approaches a
//! firing or striking position, starts a charge, or carries out its move, patrol, follow
//! or hold order. Neutral camp members return home past their leash.
//!
//! **Resolution** (step 7, [`resolve::resolve`]): shells, then missiles land; then every
//! unit whose swing reaches its impact tick strikes, launches a missile or lobs a shell.
//! Damage is summed per target and applied in id order, so every hit of a tick lands
//! simultaneously; then deaths ([`deaths`]) and experience ([`crate::heroes`]).
//!
//! Engine-level content ids are contracts checked when the world starts ([`rules`]).

pub mod approach;
pub mod charge;
pub mod damage;
pub mod deaths;
pub mod plan;
pub mod projectiles;
pub mod resolve;
pub mod rules;
pub mod senses;
pub mod targets;
pub mod view;

pub use projectiles::{Missile, Shell};
pub use rules::Rules;
pub use senses::Senses;
pub use view::{Battle, CombatView};

use crate::entity::{EntityId, Place};
use canopy_content::placement::Owner;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Units look for targets on ticks where `tick % STAGGER == id % STAGGER`.
pub const STAGGER: u32 = 8;
/// Ticks between attack approach replans.
pub const APPROACH_RETRY_TICKS: u32 = 6;

/// A swing in progress: wind-up until `impact`, recovery until `ends`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attack {
    pub target: EntityId,
    /// The attack cycle captured when the swing began.
    pub cycle_ticks: u32,
    pub started: u32,
    pub impact: u32,
    pub ends: u32,
    /// The blow landed or the shot left.
    pub released: bool,
    /// The form combat profile it was begun with, when not the unit's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<crate::catalog::Def>,
}

/// Where a target was last seen, so a unit can search for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pursuit {
    pub target: EntityId,
    /// The cell it stood on.
    pub position: Place,
    pub seen_tick: u32,
}

/// A charge: faster movement and a harder first blow against one target.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Charge {
    /// `None` once the charge ended; the cooldown stays.
    pub target: Option<EntityId>,
    pub expires: u32,
    /// No new charge before this tick.
    pub ready_tick: u32,
    /// Movement speed while charging, permille.
    pub speed_permille: u32,
    /// The form combat profile it was begun with, when not the unit's own.
    pub profile: Option<crate::catalog::Def>,
}

/// A movement slow from a shell splash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slow {
    pub permille: u32,
    pub expires: u32,
}

/// A unit's combat state.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitCombat {
    /// Ticks until the weapon may swing again.
    #[serde(default)]
    pub cooldown: u32,
    pub attack: Option<Attack>,
    pub pursuit: Option<Pursuit>,
    pub charge: Option<Charge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slows: Vec<Slow>,
}

impl UnitCombat {
    /// Nothing to save.
    pub fn is_idle(&self) -> bool {
        *self == Self::default()
    }

    /// The strongest slow still running.
    pub fn slow(&self) -> u32 {
        self.slows.iter().map(|s| s.permille).max().unwrap_or(0)
    }
}

/// A body left by a dead unit, for abilities that raise or consume corpses (M2.6).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Corpse {
    pub id: EntityId,
    pub definition: crate::catalog::Def,
    pub owner: Owner,
    /// The cell it lies on.
    pub position: Place,
    pub rotation: i32,
    pub died: u32,
    pub expires: u32,
    pub camp: Option<String>,
    pub progression: Option<crate::heroes::Progression>,
    /// Learned ranks, cooldowns and autocast, kept for resurrection (no mana, no cast).
    pub caster: Option<Box<crate::abilities::Caster>>,
}

/// Damage a mission script dealt, applied by the next combat resolution.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptedHit {
    pub target: EntityId,
    pub source: Option<EntityId>,
    pub amount: u32,
}

/// Saved combat state outside entities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombatState {
    /// Homing missiles in flight (and landed ones for a few ticks, for views), by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missiles: Vec<Missile>,
    /// Shells in flight (and landed ones for a while, for views), by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shells: Vec<Shell>,
    /// Corpses by death tick, then id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corpses: Vec<Corpse>,
    /// Mission damage waiting for the next resolution, in call order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripted: Vec<ScriptedHit>,
}

/// Content ticks (25 ms each, `AUTHORED_TICK_MS`) in simulation ticks.
pub fn ticks(authored: u32) -> u32 {
    authored * canopy_content::game::AUTHORED_TICK_MS / (1000 / crate::mission::TICKS_PER_SECOND)
}

/// Integer `round(a · b / c)` with halves up (JavaScript `Math.round`) for non-negative
/// values and `c > 0`.
pub fn round_ratio(a: u64, b: u64, c: u64) -> u64 {
    (2 * a * b + c) / (2 * c)
}

/// `Math.round`: halves toward +∞.
pub fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Movement speed factors of a unit, permille each (old movement budget): its stats'
/// move speed (slows, M2.6 items) and its charge.
pub fn speed_factors(
    catalog: &crate::catalog::Catalog,
    e: &crate::entity::Entity,
    tick: u32,
) -> (u32, u32) {
    let charge = match e.unit.as_ref().and_then(|u| u.combat.charge.as_ref()) {
        Some(c) if charge::charging(e, tick) => c.speed_permille,
        _ => 1000,
    };
    (crate::stats::move_speed_permille(catalog, e), charge)
}

/// Combat's derived side, rebuilt from content, map and setup (never saved).
#[derive(Debug)]
pub struct Runtime {
    pub rules: Rules,
    pub camps: crate::camps::Camps,
    pub teams: crate::teams::Teams,
    pub limits: crate::heroes::Limits,
    pub scratch: plan::Scratch,
}

impl Runtime {
    pub fn new(
        catalog: &crate::catalog::Catalog,
        map: &canopy_content::map::UtcMap,
        setup: &crate::setup::MatchSetup,
    ) -> Result<Self, String> {
        Ok(Self {
            rules: Rules::new(catalog)?,
            camps: crate::camps::Camps::new(map),
            teams: crate::teams::Teams::new(setup),
            limits: crate::heroes::Limits {
                level_cap: map.mission.as_ref().and_then(|m| m.hero_level_cap),
            },
            scratch: plan::Scratch::default(),
        })
    }
}
