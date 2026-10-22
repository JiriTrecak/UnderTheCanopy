//! Playing a campaign (docs/campaign.md "Playing a campaign", "What carries over"): the
//! player's progress through each campaign in the user folder, what a mission starts
//! with, and where the campaign goes when a step ends. The screens are
//! `screens/campaign.rs`; the flow between them is `screens/flow.rs`.
//!
//! A campaign plays straight through: Continue runs the next step, and finishing it (a
//! story read, a card shown, a mission won) saves the progress and runs the one after.
//! Replaying a finished step changes nothing and returns to the campaign screen.

use crate::{paths::Paths, settings::write_atomic};
use anyhow::Result;
use canopy_content::{
    Content,
    campaign::{Campaign, Step},
};
use canopy_sim::{
    campaign::{CampaignStart, CarriedHero},
    mission::state::Variable,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The player's progress through one campaign (`campaigns/<id>.json` in the user folder).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Progress {
    /// The furthest step reached, by id: the next one to play (the last one once the
    /// campaign is complete). `None` before the first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub furthest: Option<String>,
    /// Every step is done.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub complete: bool,
    /// The heroes won missions carried on, by definition.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub heroes: BTreeMap<String, CarriedHero>,
    /// The campaign variables mission scripts set: numbers, text and booleans.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Value>,
}

impl Progress {
    /// The progress of campaign `id`; a fresh one when there is none (or it cannot be
    /// read).
    pub fn load(paths: &Paths, id: &str) -> Self {
        std::fs::read(paths.campaign_progress(id))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths, id: &str) -> Result<()> {
        write_atomic(
            &paths.campaign_progress(id),
            &serde_json::to_vec_pretty(self)?,
        )
    }

    /// Whether anything was played yet.
    pub fn started(&self) -> bool {
        self.furthest.is_some() || self.complete
    }

    /// The index of the next step to play; the number of steps once complete.
    pub fn next(&self, campaign: &Campaign) -> usize {
        if self.complete {
            return campaign.steps.len();
        }
        self.furthest
            .as_deref()
            .and_then(|id| campaign.position(id))
            .unwrap_or(0)
    }

    /// How many steps the campaign screen shows: those done and the next one.
    pub fn shown(&self, campaign: &Campaign) -> usize {
        (self.next(campaign) + 1).min(campaign.steps.len())
    }

    /// Step `index` is done: the one after it is reached. A step before the next one
    /// (replayed) changes nothing.
    pub fn finish(&mut self, campaign: &Campaign, index: usize) {
        if self.complete || index < self.next(campaign) {
            return;
        }
        match campaign.steps.get(index + 1) {
            Some(step) => self.furthest = Some(step.id().to_string()),
            None => {
                self.complete = true;
                self.furthest = campaign.steps.last().map(|s| s.id().to_string());
            }
        }
    }

    /// The mission at `index` was won with these heroes and campaign variables: they carry
    /// on and the step is done. A replayed mission changes nothing.
    pub fn won(
        &mut self,
        campaign: &Campaign,
        index: usize,
        heroes: Vec<CarriedHero>,
        variables: &BTreeMap<String, Variable>,
    ) {
        if self.complete || index < self.next(campaign) {
            return;
        }
        for hero in heroes {
            self.heroes.insert(hero.definition.to_string(), hero);
        }
        self.variables = variables
            .iter()
            .filter_map(|(name, value)| Some((name.clone(), to_json(value)?)))
            .collect();
        self.finish(campaign, index);
    }

    /// What the mission at step `index` starts with: the campaign's tech up to it, the
    /// carried heroes, its hero level cap and the campaign variables.
    pub fn start(&self, campaign: &Campaign, content: &Content, index: usize) -> CampaignStart {
        let mission = campaign.steps.get(index).and_then(Step::mission);
        CampaignStart {
            player: 0,
            locked: campaign.locked(content, index),
            heroes: self.heroes.values().cloned().collect(),
            hero_level_cap: mission.and_then(|m| m.hero_level_cap),
            variables: self
                .variables
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), from_json(value)?)))
                .collect(),
        }
    }
}

/// A campaign variable as the progress file keeps it.
fn to_json(value: &Variable) -> Option<Value> {
    Some(match value {
        Variable::String(s) => Value::String(s.clone()),
        Variable::Integer(i) => Value::from(*i),
        Variable::Number(n) => serde_json::Number::from_f64(*n).map(Value::Number)?,
        Variable::Boolean(b) => Value::Bool(*b),
    })
}

fn from_json(value: &Value) -> Option<Variable> {
    match value {
        Value::String(s) => Some(Variable::String(s.clone())),
        Value::Bool(b) => Some(Variable::Boolean(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(Variable::Integer)
            .or_else(|| n.as_f64().map(Variable::Number)),
        _ => None,
    }
}

/// Where a campaign goes when the step at `index` ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// Runs the step at this index.
    Step(usize),
    /// Back to the campaign screen.
    Screen,
}

/// What follows step `index`: the next step when playing straight through (and there is
/// one), else the campaign screen.
pub fn after(campaign: &Campaign, index: usize, straight: bool) -> After {
    if straight && index + 1 < campaign.steps.len() {
        After::Step(index + 1)
    } else {
        After::Screen
    }
}

/// The step being played: which campaign, which step, and whether it is the next one
/// (straight through) or a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playing {
    pub campaign: String,
    pub step: usize,
    pub straight: bool,
    /// Its mission plays with the map's autoplay plan (Play with AI).
    pub autoplay: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn campaign() -> Campaign {
        serde_json::from_value(json!({
            "race": "red",
            "steps": [
                {"kind": "chapter", "id": "one", "title": "One"},
                {"kind": "mission", "id": "first", "map": "a", "title": "First", "heroLevelCap": 2},
                {"kind": "story", "id": "tale", "title": "Tale", "pages": [{"text": "x"}]},
                {"kind": "mission", "id": "second", "map": "b", "title": "Second"}
            ]
        }))
        .unwrap()
    }

    fn hero(definition: &str, experience: u32) -> CarriedHero {
        CarriedHero {
            definition: canopy_content::Id::new(definition).unwrap(),
            experience,
            learned: BTreeMap::new(),
            items: Vec::new(),
            bonuses: None,
        }
    }

    #[test]
    fn progress_round_trips_in_the_user_folder() {
        let user = crate::settings::temp_folder("campaign-progress");
        let paths = Paths {
            data: user.clone(),
            user: user.clone(),
            cache: user.clone(),
            replays: user.clone(),
        };
        assert_eq!(Progress::load(&paths, "c"), Progress::default());
        let c = campaign();
        let mut progress = Progress::default();
        progress.finish(&c, 0);
        let variables = [
            ("rescued".to_string(), Variable::Integer(3)),
            ("ally".to_string(), Variable::String("moths".into())),
            ("share".to_string(), Variable::Number(0.25)),
            ("burned".to_string(), Variable::Boolean(true)),
        ]
        .into_iter()
        .collect();
        progress.won(&c, 1, vec![hero("unit.red.queen", 80)], &variables);
        progress.save(&paths, "c").unwrap();
        assert!(paths.campaign_progress("c").is_file());
        let loaded = Progress::load(&paths, "c");
        assert_eq!(loaded, progress);
        assert_eq!(loaded.furthest.as_deref(), Some("tale"));
        let start = loaded.start(&c, &content(), 3);
        assert_eq!(start.variables, variables);
        assert_eq!(start.heroes, vec![hero("unit.red.queen", 80)]);
        assert_eq!(start.hero_level_cap, None);
        assert_eq!(loaded.start(&c, &content(), 1).hero_level_cap, Some(2));
    }

    /// Content without races: no tech to lock.
    fn content() -> Content {
        crate::sim::ai_fixture::content().as_ref().clone()
    }

    #[test]
    fn steps_advance_straight_through_and_replays_change_nothing() {
        let c = campaign();
        let mut progress = Progress::default();
        assert_eq!((progress.next(&c), progress.shown(&c)), (0, 1));
        progress.finish(&c, 0);
        assert_eq!((progress.next(&c), progress.shown(&c)), (1, 2));
        assert_eq!(after(&c, 0, true), After::Step(1));
        // The mission is won: its heroes carry on and the story is next.
        progress.won(&c, 1, vec![hero("unit.red.queen", 50)], &BTreeMap::new());
        assert_eq!(progress.next(&c), 2);
        // Replaying the first mission and winning it again moves nothing back.
        let before = progress.clone();
        progress.won(&c, 1, vec![hero("unit.red.queen", 10)], &BTreeMap::new());
        progress.finish(&c, 0);
        assert_eq!(progress, before);
        assert_eq!(after(&c, 1, false), After::Screen);
        progress.finish(&c, 2);
        progress.won(&c, 3, vec![hero("unit.red.queen", 120)], &BTreeMap::new());
        assert!(progress.complete);
        assert_eq!(progress.next(&c), 4);
        assert_eq!(progress.shown(&c), 4);
        assert_eq!(progress.heroes["unit.red.queen"].experience, 120);
        // The last step leads back to the campaign screen.
        assert_eq!(after(&c, 3, true), After::Screen);
    }
}
