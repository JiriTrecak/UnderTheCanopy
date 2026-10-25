//! MCP's campaign tools (docs/editor.md "Campaigns"): what agents call to make a
//! campaign, add, move, remove and change its steps, set a mission's tech, read its tech
//! tree and set a mission's computer players. The shell answers them
//! (`shell/campaigns.rs`) through the same commands as the editor's own controls.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateCampaign {
    /// The campaign's id: lowercase words joined by hyphens (`vanguard`); its package is
    /// `campaigns/<id>.utcc`.
    pub id: String,
    /// Its name on the campaign screen.
    pub name: Option<String>,
    /// The race the player plays (a race id).
    pub race: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CampaignArgs {
    /// The campaign: its id (`vanguard`) or its package path (`campaigns/vanguard.utcc`).
    pub campaign: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AddStep {
    /// The campaign: its id or its package path.
    pub campaign: String,
    /// chapter, story, mission or cinematic.
    pub kind: String,
    pub title: Option<String>,
    /// The step's id (unique in the campaign); made from the title when absent.
    pub id: Option<String>,
    /// Where it goes in the list (0 first); last when absent.
    pub at: Option<usize>,
    /// A mission's map among the campaign's missions/; absent, the mission gets a new
    /// map package of flat blocks there.
    pub map: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MoveStep {
    pub campaign: String,
    /// The step: its id or its index.
    pub step: Value,
    /// The index it ends up at.
    pub to: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct StepArgs {
    pub campaign: String,
    /// The step: its id or its index.
    pub step: Value,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetStep {
    pub campaign: String,
    /// The step: its id or its index.
    pub step: Value,
    /// Fields to set, as campaign.json names them (title, art, chapterLine, story,
    /// heroLevelCap, pages, note, music, map, id …); null removes a field.
    pub fields: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetTech {
    pub campaign: String,
    /// The mission step: its id, its index, or its map's id.
    pub step: Value,
    /// Unit, building and research ids, each to `new` (unlocked from this mission on),
    /// `blocked` (taken away in this mission), or `locked` / `available` (neither: the
    /// mission leaves it as the missions before left it).
    pub set: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MissionPlayers {
    /// The mission map: its id or its package path (`campaigns/vanguard.utcc/missions/a.utcm`).
    /// Or name the mission as the other campaign tools do, by `campaign` and `step`.
    pub map: Option<String>,
    pub campaign: Option<String>,
    /// The mission step: its id or its index (with `campaign`).
    pub step: Option<Value>,
    /// The mission's computer players, replacing its list (docs/campaign.md "Formats"):
    /// `{player, race, team: ally|enemy, vision, ai: {behaviour: passive|base|waves|skirmish,
    /// dormant, rebuild, waves: [{after, every, until, units: [{unit, count}], grow, hero,
    /// from: "trained" | {region}, target: "base" | "nearest" | {region}}], skirmish}}`.
    pub players: Vec<Value>,
}

/// A campaign tool call.
#[derive(Debug, Clone)]
pub enum Call {
    Create(CreateCampaign),
    Steps(CampaignArgs),
    Add(AddStep),
    Move(MoveStep),
    Remove(StepArgs),
    Set(SetStep),
    Tech(SetTech),
    Tree(StepArgs),
    Players(MissionPlayers),
}
