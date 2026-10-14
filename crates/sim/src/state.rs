//! The complete saved simulation state. Everything derived (spatial grids, navigation,
//! indexes) lives outside it and is rebuilt on load.

use crate::{
    entity::{Entities, Stock},
    mission::MissionState,
    vision::Knowledge,
};
use canopy_content::placement::Owner;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// Monotonic id counters. Ids are never reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counters {
    pub entity: u32,
    pub job: u32,
    pub queue: u32,
    pub fact: u32,
    pub cast: u32,
    pub missile: u32,
    pub shell: u32,
    pub vision: u32,
}

impl Default for Counters {
    fn default() -> Self {
        Self {
            entity: 1,
            job: 1,
            queue: 1,
            fact: 1,
            cast: 1,
            missile: 1,
            shell: 1,
            vision: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FactKind {
    Command,
    Error,
    Produced,
    Consumed,
    Lost,
    Death,
    Released,
}

/// A player-facing event: rejected commands, production, losses.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub id: u32,
    pub tick: u32,
    pub owner: Owner,
    pub kind: FactKind,
    pub message: String,
    pub item: Option<canopy_content::Id>,
    pub amount: Option<u32>,
}

/// Most facts kept; older ones drop off.
pub const MAX_FACTS: usize = 128;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accounting {
    pub produced: Stock,
    pub consumed: Stock,
    pub lost: Stock,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub winner: Option<Owner>,
    pub defeated: Vec<Owner>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub tick: u32,
    /// Gameplay random generator (xorshift32); never zero.
    pub random: u32,
    pub counters: Counters,
    pub entities: Entities,
    pub wallets: BTreeMap<Owner, Stock>,
    pub accounting: Accounting,
    pub facts: Vec<Fact>,
    pub objectives: BTreeMap<Owner, u32>,
    pub cleared_camps: Vec<String>,
    pub outcome: Option<Outcome>,
    /// Campaign script progress; `None` outside missions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission: Option<MissionState>,
    /// What each team and player has seen (`vision`).
    pub knowledge: Knowledge,
}

impl State {
    pub fn new(seed: u32) -> Self {
        Self {
            tick: 0,
            random: if seed == 0 { 1 } else { seed },
            counters: Counters::default(),
            entities: Entities::default(),
            wallets: BTreeMap::new(),
            accounting: Accounting::default(),
            facts: Vec::new(),
            objectives: BTreeMap::new(),
            cleared_camps: Vec::new(),
            outcome: None,
            mission: None,
            knowledge: Knowledge::default(),
        }
    }

    pub fn fact(&mut self, owner: Owner, kind: FactKind, message: impl Into<String>) {
        let id = self.counters.fact;
        self.counters.fact += 1;
        self.facts.push(Fact {
            id,
            tick: self.tick,
            owner,
            kind,
            message: message.into(),
            item: None,
            amount: None,
        });
        if self.facts.len() > MAX_FACTS {
            self.facts.remove(0);
        }
    }

    /// Next value of the gameplay generator (xorshift32).
    pub fn random(&mut self) -> u32 {
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        x
    }
}
