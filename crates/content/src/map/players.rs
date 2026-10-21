//! A mission's computer players (`mission.players`, docs/campaign.md "Campaign AI" and
//! "Formats"): their race, side, shared sight and the campaign AI behaviour that plays them.

use super::{Mission, UtcMap};
use crate::{
    Content,
    game::{Kind, Rules},
    id::Id,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// One computer player of a mission. Players the mission lists nowhere stay out of the
/// match; a listed player needs no player start (its units and buildings are what the map
/// places for it).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissionPlayer {
    /// 2 to 8 (player 1 is the campaign's player).
    pub player: u8,
    /// A race id; the rules' default race when absent.
    pub race: Option<String>,
    /// `ally` joins player 1's team; every `enemy` shares the other team.
    pub team: Team,
    /// An ally shares its sight with player 1 (the default for allies). Enemies never do.
    pub vision: Option<bool>,
    #[serde(default)]
    pub ai: PlayerAi,
}

impl MissionPlayer {
    /// Whether player 1's team sees what this player sees.
    pub fn shares_vision(&self) -> bool {
        self.team == Team::Ally && self.vision.unwrap_or(true)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Team {
    Ally,
    Enemy,
}

/// How the campaign AI plays a computer player.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Behaviour {
    /// Holds its ground: units defend where they stand; nothing is trained or rebuilt.
    #[default]
    Passive,
    /// Keeps its defenders at full strength and rebuilds its buildings; never attacks.
    Base,
    /// A base that also sends attack waves.
    Waves,
    /// The skirmish AI, held back by `skirmish`.
    Skirmish,
}

impl Behaviour {
    pub const ALL: [Behaviour; 4] = [Self::Passive, Self::Base, Self::Waves, Self::Skirmish];

    pub fn name(self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Base => "base",
            Self::Waves => "waves",
            Self::Skirmish => "skirmish",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerAi {
    #[serde(default)]
    pub behaviour: Behaviour,
    /// Present but inactive until the script wakes it (`mission.ai_wake`): its placements
    /// marked `activation: "script"` appear then, and its behaviour starts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dormant: bool,
    /// Base and Waves rebuild destroyed buildings the map placed (default true).
    pub rebuild: Option<bool>,
    /// Wave lines, played by the Waves behaviour (kept for a script that switches to it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waves: Vec<WaveLine>,
    /// The Skirmish behaviour's limits.
    pub skirmish: Option<SkirmishLimits>,
}

impl PlayerAi {
    pub fn rebuilds(&self) -> bool {
        self.rebuild.unwrap_or(true)
    }
}

/// A line of attack waves. Times are seconds since the player became active (match start,
/// or when the script woke it). Room is left for per-difficulty numbers.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaveLine {
    /// The first wave; absent: the line waits until the script turns waves on
    /// (`mission.ai_waves(player, true)`), which sends its first wave at once.
    pub after: Option<f64>,
    /// Seconds between waves; absent: one wave only.
    pub every: Option<f64>,
    /// The line sends nothing from this time on (a later line can take over).
    pub until: Option<f64>,
    pub units: Vec<WaveUnit>,
    /// More units every few waves.
    pub grow: Option<Grow>,
    /// The player's living heroes join each wave (heroes revive at their altar).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hero: bool,
    #[serde(default)]
    pub from: WaveSource,
    #[serde(default)]
    pub target: WaveTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaveUnit {
    pub unit: Id,
    pub count: u32,
}

/// `count` more of `unit` after every `every` waves sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grow {
    pub every: u32,
    pub unit: Id,
    pub count: u32,
}

/// Where a wave comes from: `"trained"` or `{"region": "<id>"}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WaveSource {
    /// Trained, free, at the player's own buildings that train each unit; a unit no
    /// building can train is skipped, so destroying production stops the waves.
    #[default]
    Trained,
    /// Arrives at a mission region (reinforcements, an army marching in).
    Region(String),
}

/// Where a wave goes: `"base"`, `"nearest"` or `{"region": "<id>"}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WaveTarget {
    /// The nearest enemy main base (for allies, the enemy's).
    #[default]
    Base,
    /// The nearest enemy units.
    Nearest,
    /// A mission region.
    Region(String),
}

/// How the Skirmish behaviour is held back.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkirmishLimits {
    /// `standard`, `rush`, `expand` or `tech`; drawn from the match seed when absent.
    pub style: Option<String>,
    /// Most fighters it fields.
    pub army_cap: Option<u32>,
    /// Decision intervals multiplied by this (1 to 10; 2 plays at half speed).
    pub pace: Option<f64>,
    /// Gathered goods multiplied by this (0 to 4; 0.5 halves its income).
    pub income: Option<f64>,
}

/// The skirmish AI's style names.
pub const STYLES: [&str; 4] = ["standard", "rush", "expand", "tech"];

/// Format rules that need no content: `(location, message)` pairs.
pub(super) fn check(m: &Mission) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut fail = |at: String, message: &str| out.push((at, message.to_string()));
    let mut seen = Vec::new();
    for (i, p) in m.players.iter().enumerate() {
        let at = format!("mission.players[{i}]");
        if !(2..=8).contains(&p.player) {
            fail(format!("{at}.player"), "must be 2 to 8");
        }
        if seen.contains(&p.player) {
            fail(format!("{at}.player"), "Duplicate mission player");
        }
        seen.push(p.player);
        if p.vision == Some(true) && p.team == Team::Enemy {
            fail(format!("{at}.vision"), "Only allies share their vision");
        }
        let ai = &p.ai;
        if ai.waves.len() > 16 {
            fail(format!("{at}.ai.waves"), "must have at most 16 wave lines");
        }
        for (j, line) in ai.waves.iter().enumerate() {
            let at = format!("{at}.ai.waves[{j}]");
            let seconds =
                |v: Option<f64>| v.is_none_or(|v| v.is_finite() && (0.0..=36_000.0).contains(&v));
            if !seconds(line.after) {
                fail(format!("{at}.after"), "must be 0 to 36000 seconds");
            }
            if !seconds(line.until) {
                fail(format!("{at}.until"), "must be 0 to 36000 seconds");
            }
            if line
                .every
                .is_some_and(|v| !(v.is_finite() && (1.0..=36_000.0).contains(&v)))
            {
                fail(format!("{at}.every"), "must be 1 to 36000 seconds");
            }
            if line.units.is_empty() || line.units.len() > 16 {
                fail(format!("{at}.units"), "must have 1 to 16 entries");
            }
            let total: u32 = line.units.iter().map(|u| u.count).sum();
            if line.units.iter().any(|u| u.count == 0) || total > 100 {
                fail(
                    format!("{at}.units"),
                    "counts must be at least 1, at most 100 in all",
                );
            }
            if let Some(g) = &line.grow
                && (g.every == 0 || g.count == 0 || g.count > 20)
            {
                fail(
                    format!("{at}.grow"),
                    "every must be at least 1, count 1 to 20",
                );
            }
        }
        if let Some(s) = &ai.skirmish {
            if s.style.as_deref().is_some_and(|s| !STYLES.contains(&s)) {
                fail(
                    format!("{at}.ai.skirmish.style"),
                    "must be standard, rush, expand or tech",
                );
            }
            if s.army_cap.is_some_and(|c| c > 200) {
                fail(format!("{at}.ai.skirmish.armyCap"), "must be 0 to 200");
            }
            if s.pace
                .is_some_and(|v| !(v.is_finite() && (1.0..=10.0).contains(&v)))
            {
                fail(format!("{at}.ai.skirmish.pace"), "must be 1 to 10");
            }
            if s.income
                .is_some_and(|v| !(v.is_finite() && (0.0..=4.0).contains(&v)))
            {
                fail(format!("{at}.ai.skirmish.income"), "must be 0 to 4");
            }
        }
    }
    out
}

/// Rules that need the content and the map: races, wave units and regions exist.
pub(super) fn validate(map: &UtcMap, content: &Content) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(m) = &map.mission else {
        return out;
    };
    let rules: &Rules = &content.rules;
    let unit = |id: &Id| {
        content
            .definition(id.as_str())
            .is_some_and(|d| d.kind == Kind::Unit)
    };
    let region = |id: &str| m.regions.iter().any(|r| r.id == id);
    for (i, p) in m.players.iter().enumerate() {
        let at = format!("mission.players[{i}]");
        if let Some(race) = &p.race {
            let known = rules.races.contains_key(race)
                || (rules.races.is_empty() && *race == rules.default_race);
            if !known {
                out.push((format!("{at}.race"), format!("Unknown race: {race}")));
            }
        }
        for (j, line) in p.ai.waves.iter().enumerate() {
            let at = format!("{at}.ai.waves[{j}]");
            for (k, u) in line.units.iter().enumerate() {
                if !unit(&u.unit) {
                    out.push((
                        format!("{at}.units[{k}].unit"),
                        format!("Unknown unit: {}", u.unit),
                    ));
                }
            }
            if let Some(g) = &line.grow
                && !unit(&g.unit)
            {
                out.push((
                    format!("{at}.grow.unit"),
                    format!("Unknown unit: {}", g.unit),
                ));
            }
            if let WaveSource::Region(id) = &line.from
                && !region(id)
            {
                out.push((format!("{at}.from"), format!("Unknown region: {id}")));
            }
            if let WaveTarget::Region(id) = &line.target
                && !region(id)
            {
                out.push((format!("{at}.target"), format!("Unknown region: {id}")));
            }
        }
    }
    out
}
