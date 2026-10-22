//! What an AI player remembers between decisions (old `ai/state.ts`): saved with the
//! simulation state, bounded, and nothing derived. Rebuilding everything else from it and
//! from the player's knowledge gives identical decisions after a restore.

use crate::{action::Action, catalog::Def, entity::Place};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// Most commands an AI waits on before it issues more.
pub const MAX_PENDING: usize = 32;
/// Most remembered enemy units.
pub const MAX_SIGHTINGS: usize = 512;
/// Debug history kept.
pub const MAX_TRACE: usize = 80;
/// AI command sequence numbers start here, above any human command's.
pub const SEQUENCE_BASE: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MissionKind {
    Scout,
    Camp,
    Raid,
    Assault,
    Defend,
    Regroup,
}

impl MissionKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Scout => "scout",
            Self::Camp => "camp",
            Self::Raid => "raid",
            Self::Assault => "assault",
            Self::Defend => "defend",
            Self::Regroup => "regroup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Assemble,
    Travel,
    Engage,
    Recover,
}

/// The army's current operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    pub kind: MissionKind,
    /// A camp id, a building id or a start key.
    pub key: String,
    pub point: Place,
    pub started: u32,
    pub until: u32,
    pub stage: Stage,
    /// Where the squad's centre was when it last made progress.
    pub progress_point: Place,
    pub progress_tick: u32,
    pub members: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CampStatus {
    /// Known from the map, not seen yet.
    Expected,
    /// Defenders seen.
    Active,
    /// Its site seen with nobody there.
    Empty,
    /// Every defender seen died.
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampMemory {
    pub status: CampStatus,
    pub seen: u32,
    pub retry: u32,
    /// Defenders whose death was witnessed.
    pub killed: Vec<u32>,
    /// Defenders ever seen at the site.
    pub seen_ids: Vec<u32>,
}

impl Default for CampMemory {
    fn default() -> Self {
        Self {
            status: CampStatus::Expected,
            seen: 0,
            retry: 0,
            killed: Vec::new(),
            seen_ids: Vec::new(),
        }
    }
}

/// An enemy unit as last seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spotted {
    pub id: u32,
    pub definition: Def,
    pub point: Place,
    pub hp: u32,
    pub seen: u32,
}

/// The last order the AI gave a unit, to keep it rather than reissue it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMemo {
    /// The order without its actors.
    pub signature: String,
    pub tick: u32,
    pub point: Place,
    pub hp: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerReturn {
    pub target: u32,
    pub until: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub seq: u32,
    pub tick: u32,
    pub action: Action,
    pub reason: String,
}

/// Health of an own entity when last decided, and when it may react to damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Damage {
    pub hp: u32,
    pub seen: u32,
    pub react_at: u32,
}

/// The current intentions, for the debug overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub workers: u32,
    pub army: u32,
    pub hero_level: u32,
    pub hero_xp_needed: u32,
    pub economy: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub issued: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub builds: u32,
    pub recruits: u32,
    pub casts: u32,
    pub pickups: u32,
    pub revivals: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    pub tick: u32,
    pub reason: String,
    pub action: String,
    pub accepted: Option<bool>,
}

/// One AI player's memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    /// Zero-based player slot.
    pub player: u8,
    /// Fingerprint of the map briefing it was made for.
    pub briefing: u32,
    pub sequence: u32,
    pub next_economy: u32,
    pub next_strategy: u32,
    pub next_operation: u32,
    pub mission: Option<Mission>,
    pub guards: Vec<u32>,
    pub scout: Option<u32>,
    pub next_scout: u32,
    /// By camp id.
    pub camps: BTreeMap<String, CampMemory>,
    /// Enemy units in the order first seen.
    pub sightings: Vec<Spotted>,
    /// First sight of each hostile in view, by id.
    pub contacts: BTreeMap<u32, u32>,
    /// Timers by key (old `inspected`): `reject:<command>`, `start:<x>:<y>`, `item:<id>`,
    /// `replacement:<id>`, `economy:rebalance`, `economy:well`, `economy:expansion`,
    /// `economy:had-well` (when a well last stood), `economy:extend:<id>`,
    /// `economy:nosite:<id>` and stalled mission keys.
    pub inspected: BTreeMap<String, u32>,
    /// Building sites refused by the simulation, until a tick.
    pub failed_sites: BTreeMap<String, u32>,
    /// Where the next building site search starts; the AI's only randomness.
    pub placement_cursor: u32,
    /// Where the next search over the open ground beyond the rings goes on from.
    #[serde(default)]
    pub far_cursor: u32,
    pub orders: BTreeMap<u32, OrderMemo>,
    pub worker_returns: BTreeMap<u32, WorkerReturn>,
    /// Commands issued and not yet applied.
    pub pending: Vec<Pending>,
    pub damage: BTreeMap<u32, Damage>,
    pub plan: Plan,
    pub metrics: Metrics,
    pub trace: Vec<Trace>,
}

impl Memory {
    /// A fresh memory for `player` whose placement cursor starts from `seed`.
    pub fn new(player: u8, briefing: u32, seed: u32) -> Self {
        Self {
            player,
            briefing,
            sequence: 0,
            next_economy: 0,
            next_strategy: 0,
            next_operation: 0,
            mission: None,
            guards: Vec::new(),
            scout: None,
            next_scout: 0,
            camps: BTreeMap::new(),
            sightings: Vec::new(),
            contacts: BTreeMap::new(),
            inspected: BTreeMap::new(),
            failed_sites: BTreeMap::new(),
            placement_cursor: seed % 192,
            far_cursor: 0,
            orders: BTreeMap::new(),
            worker_returns: BTreeMap::new(),
            pending: Vec::new(),
            damage: BTreeMap::new(),
            plan: Plan {
                workers: 0,
                army: 0,
                hero_level: 1,
                hero_xp_needed: 0,
                economy: "Develop income".into(),
                reason: "Establish the colony".into(),
            },
            metrics: Metrics::default(),
            trace: Vec::new(),
        }
    }

    /// Checks the bounds a well-formed memory keeps.
    pub fn check(&self) -> Result<(), String> {
        if self.pending.len() > MAX_PENDING
            || self.sightings.len() > MAX_SIGHTINGS
            || self.trace.len() > MAX_TRACE
            || self.guards.len() > 16
            || self.mission.as_ref().is_some_and(|m| m.members.len() > 160)
        {
            return Err(format!(
                "AI memory of player {} is out of bounds",
                self.player
            ));
        }
        Ok(())
    }
}

/// A building site key (old `siteKey`).
pub fn site_key(definition: &str, x: f64, y: f64, rotation: i32) -> String {
    format!("{definition}:{x}:{y}:{rotation}")
}
