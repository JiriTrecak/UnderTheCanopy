//! Campaigns in the editor (docs/campaign.md "The editor", docs/editor.md "Campaigns"): a
//! campaign package's `campaign.json` read as steps, the tech each mission allows, and the
//! commands that change them. The views are [`view`] (the campaign's steps and its
//! panel), [`tech`] (a mission's Tech view in the map editor) and [`players`] (a mission
//! map's computer players); [`check`] adds what the editor sees to the problems list.
//!
//! Campaigns are JSON documents in the store (docs/campaign.md "Formats"); nothing here
//! names a shipped content id.

pub mod art;
pub mod check;
pub mod players;
pub mod tech;
pub mod tools;
pub mod view;

use crate::document::Command;
use crate::library::{Element, Node};
use canopy_content::collections::CAMPAIGNS;
use canopy_content::package::{MANIFEST, MAP_EXTENSION, Package};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The base package's folder of campaign packages.
pub const DIR: &str = "campaigns";
/// A campaign package's document of steps.
pub const FILE: &str = "campaign.json";
/// A campaign package's folder of mission maps.
pub const MISSIONS: &str = "missions";

/// The campaign package folder of `document` when it is a campaign's `campaign.json`.
pub fn root_of(document: &str) -> Option<&str> {
    let root = document.strip_suffix(&format!("/{FILE}"))?;
    (root.starts_with(&format!("{DIR}/")) && root.ends_with(".utcc")).then_some(root)
}

/// For a document of one of a campaign's mission maps: the campaign's folder and the
/// map's id (`campaigns/vanguard.utcc/missions/a.utcm/map.json` → (`campaigns/vanguard.utcc`, `a`)).
pub fn mission_of(document: &str) -> Option<(&str, &str)> {
    if !document.starts_with(&format!("{DIR}/")) {
        return None;
    }
    let marker = format!(".utcc/{MISSIONS}/");
    let at = document.find(&marker)?;
    let root = &document[..at + ".utcc".len()];
    let rest = &document[at + marker.len()..];
    let name = rest.split('/').next()?;
    let id = name.strip_suffix(&format!(".{MAP_EXTENSION}"))?;
    (!id.is_empty()).then_some((root, id))
}

/// Whether a change to `path` changes what the Maps library lists under its campaigns: a
/// campaign's steps, a campaign's or a mission map's name.
pub fn lists_maps(path: &str) -> bool {
    path.starts_with(&format!("{DIR}/"))
        && (path.ends_with(&format!(".utcc/{FILE}"))
            || path.ends_with(".utcc/package.json")
            || (is_mission_document(path) && path.ends_with(".utcm/package.json")))
}

/// Whether `document` belongs to a campaign's mission map.
pub fn is_mission_document(document: &str) -> bool {
    mission_of(document).is_some()
}

/// A mission map's package folder.
pub fn map_root(campaign: &str, map: &str) -> String {
    format!("{campaign}/{MISSIONS}/{map}.{MAP_EXTENSION}")
}

/// The campaign's id: its package folder's name.
pub fn id_of(root: &str) -> &str {
    let name = root.rsplit('/').next().unwrap_or(root);
    name.strip_suffix(".utcc").unwrap_or(name)
}

pub fn document(root: &str) -> String {
    format!("{root}/{FILE}")
}

/// The kinds of step (docs/campaign.md "Steps").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepKind {
    Chapter,
    Story,
    Mission,
    Cinematic,
}

impl StepKind {
    pub const ALL: [StepKind; 4] = [
        StepKind::Chapter,
        StepKind::Story,
        StepKind::Mission,
        StepKind::Cinematic,
    ];

    /// Its `kind` in campaign.json.
    pub fn name(self) -> &'static str {
        match self {
            StepKind::Chapter => "chapter",
            StepKind::Story => "story",
            StepKind::Mission => "mission",
            StepKind::Cinematic => "cinematic",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StepKind::Chapter => "Chapter",
            StepKind::Story => "Story",
            StepKind::Mission => "Mission",
            StepKind::Cinematic => "Cinematic",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    pub fn of(step: &Value) -> Option<Self> {
        step.get("kind")
            .and_then(Value::as_str)
            .and_then(Self::parse)
    }
}

/// The campaign's steps (none when it has no list).
pub fn steps(campaign: &Value) -> &[Value] {
    campaign
        .get("steps")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// A text field of `value` (empty when absent).
pub fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value.get(field).and_then(Value::as_str).unwrap_or_default()
}

/// The mission number of each step: `Some(n)` (1-based) for missions.
pub fn mission_numbers(campaign: &Value) -> Vec<Option<usize>> {
    let mut n = 0;
    steps(campaign)
        .iter()
        .map(|s| {
            (StepKind::of(s) == Some(StepKind::Mission)).then(|| {
                n += 1;
                n
            })
        })
        .collect()
}

/// The step whose mission plays map `map`, if any.
pub fn step_of_map(campaign: &Value, map: &str) -> Option<usize> {
    steps(campaign)
        .iter()
        .position(|s| StepKind::of(s) == Some(StepKind::Mission) && text(s, "map") == map)
}

/// What the player may build, train and research in one mission (docs/campaign.md
/// "Tech"): unlocked before it, new in it, blocked in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tech {
    /// Unlocked by an earlier mission, with the step that unlocked it first.
    pub before: BTreeMap<String, usize>,
    pub new: Vec<String>,
    pub blocked: Vec<String>,
}

/// A node's state in a mission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TechState {
    /// No mission so far has unlocked it.
    Locked,
    /// Unlocked by an earlier mission.
    Available,
    /// Unlocked from this mission on.
    New,
    /// Taken away in this mission only.
    Blocked,
}

impl TechState {
    pub fn label(self) -> &'static str {
        match self {
            TechState::Locked => "Locked",
            TechState::Available => "Available",
            TechState::New => "New",
            TechState::Blocked => "Blocked",
        }
    }

    /// Whether the player may have it in the mission.
    pub fn allowed(self) -> bool {
        matches!(self, TechState::Available | TechState::New)
    }
}

fn ids(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

impl Tech {
    /// The tech of step `step` (a mission; any other step gives what is unlocked before).
    pub fn at(campaign: &Value, step: usize) -> Self {
        let mut tech = Tech::default();
        for (i, s) in steps(campaign).iter().enumerate() {
            if StepKind::of(s) != Some(StepKind::Mission) {
                continue;
            }
            let new = ids(s.pointer("/tech/new"));
            if i == step {
                tech.blocked = ids(s.pointer("/tech/blocked"));
                tech.new = new;
                break;
            }
            for id in new {
                tech.before.entry(id).or_insert(i);
            }
        }
        tech
    }

    pub fn state(&self, id: &str) -> TechState {
        if self.blocked.iter().any(|b| b == id) {
            TechState::Blocked
        } else if self.new.iter().any(|n| n == id) {
            TechState::New
        } else if self.before.contains_key(id) {
            TechState::Available
        } else {
            TechState::Locked
        }
    }

    /// The states a node may be set to here: an earlier unlock cannot be new again.
    pub fn choices(&self, id: &str) -> &'static [TechState] {
        if self.before.contains_key(id) {
            &[TechState::Available, TechState::Blocked]
        } else {
            &[TechState::Locked, TechState::New, TechState::Blocked]
        }
    }

    /// What a click makes of `id`: Locked → New → Blocked → Locked, or for an earlier
    /// unlock Available → Blocked → Available.
    pub fn next(&self, id: &str) -> TechState {
        let choices = self.choices(id);
        let at = choices
            .iter()
            .position(|s| *s == self.state(id))
            .unwrap_or(0);
        choices[(at + 1) % choices.len()]
    }

    /// The mission's lists with `id` set to `to`.
    pub fn with(&self, id: &str, to: TechState) -> (Vec<String>, Vec<String>) {
        let mut new = self.new.clone();
        let mut blocked = self.blocked.clone();
        new.retain(|n| n != id);
        blocked.retain(|b| b != id);
        match to {
            TechState::New if !self.before.contains_key(id) => new.push(id.to_string()),
            TechState::Blocked => blocked.push(id.to_string()),
            _ => {}
        }
        (new, blocked)
    }

    /// "2 new · 1 blocked", or "no changes".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.new.is_empty() {
            parts.push(format!("{} new", self.new.len()));
        }
        if !self.blocked.is_empty() {
            parts.push(format!("{} blocked", self.blocked.len()));
        }
        if parts.is_empty() {
            "no tech changes".into()
        } else {
            parts.join(" · ")
        }
    }
}

/// The command that sets step `step`'s tech lists.
pub fn set_tech(document: &str, step: usize, new: Vec<String>, blocked: Vec<String>) -> Command {
    Command::Set {
        document: document.to_string(),
        pointer: format!("/steps/{step}/tech"),
        value: json!({"new": new, "blocked": blocked}),
        at: None,
    }
}

/// An id for a new step: `base` (a slug of its title), numbered when taken.
pub fn unique_step_id(campaign: &Value, base: &str) -> String {
    let taken: BTreeSet<&str> = steps(campaign).iter().map(|s| text(s, "id")).collect();
    let base = slug(base);
    let base = if base.is_empty() { "step".into() } else { base };
    if !taken.contains(base.as_str()) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.contains(id.as_str()))
        .expect("some number is free")
}

/// Lowercase words joined by hyphens.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A new step of `kind`.
pub fn new_step(kind: StepKind, id: &str, title: &str, map: Option<&str>) -> Value {
    let mut step = json!({"kind": kind.name(), "id": id, "title": title});
    match kind {
        StepKind::Chapter => {}
        StepKind::Story => step["pages"] = json!([{"text": "A new page."}]),
        StepKind::Mission => {
            step["map"] = json!(map.unwrap_or(id));
            step["tech"] = json!({"new": [], "blocked": []});
        }
        StepKind::Cinematic => {}
    }
    step
}

/// The commands that insert `step` at `index` of the campaign's steps (making the list
/// when it has none).
pub fn insert_step(document: &str, campaign: &Value, index: usize, step: Value) -> Command {
    if campaign.get("steps").is_some_and(Value::is_array) {
        Command::Insert {
            document: document.to_string(),
            pointer: "/steps".into(),
            index: index.min(steps(campaign).len()),
            value: step,
        }
    } else {
        Command::Set {
            document: document.to_string(),
            pointer: "/steps".into(),
            value: json!([step]),
            at: None,
        }
    }
}

/// The commands that move step `from` to stand at `to` (an index in the list as it is
/// before the move; `len` puts it last).
pub fn move_step(document: &str, campaign: &Value, from: usize, to: usize) -> Vec<Command> {
    let list = steps(campaign);
    let Some(step) = list.get(from) else {
        return Vec::new();
    };
    let to = to.min(list.len());
    let target = if to > from { to - 1 } else { to };
    if target == from {
        return Vec::new();
    }
    vec![
        Command::Remove {
            document: document.to_string(),
            pointer: format!("/steps/{from}"),
        },
        Command::Insert {
            document: document.to_string(),
            pointer: "/steps".into(),
            index: target,
            value: step.clone(),
        },
    ]
}

/// The commands that make a new campaign package `campaigns/<id>.utcc`: its manifest and
/// a campaign of `race` with its first chapter (a campaign has a step at least).
pub fn new_campaign(
    id: &str,
    name: &str,
    race: &str,
    base_version: &str,
) -> Result<Vec<Command>, String> {
    canopy_content::Id::new(id)?;
    let root = format!("{DIR}/{id}.utcc");
    Ok(vec![
        Command::Create {
            document: format!("{root}/{MANIFEST}"),
            value: json!({
                "format": 1,
                "id": id,
                "kind": "campaign",
                "name": name,
                "version": "1.0.0",
                "dependencies": {"base": format!(">={base_version}")},
            }),
        },
        Command::Create {
            document: document(&root),
            value: json!({"race": race, "steps": [new_step(StepKind::Chapter, "chapter-one", "Chapter One", None)]}),
        },
    ])
}

/// The biome a new mission map gets: a forest one (its name or description says so, in
/// the package at `root`) when there is one, else the first.
pub fn mission_biome(root: Option<&std::path::Path>) -> &'static str {
    let forest = |id: &str| {
        root.and_then(|r| {
            std::fs::read_to_string(r.join(format!("landscape/biomes/{id}.json"))).ok()
        })
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .is_some_and(|b| {
            [text(&b, "name"), text(&b, "description")]
                .iter()
                .any(|t| t.to_lowercase().contains("forest"))
        })
    };
    canopy_content::map::BIOMES
        .iter()
        .find(|b| forest(b))
        .copied()
        .unwrap_or(canopy_content::map::BIOMES[0])
}

/// The maps in a campaign's `missions/` folder (package ids), sorted.
pub fn mission_maps(package: &Package, campaign: &str) -> Vec<String> {
    let dir = format!("{campaign}/{MISSIONS}");
    let mut out: Vec<String> = package
        .entries(&dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, is_dir)| *is_dir)
        .filter_map(|(name, _)| {
            name.strip_suffix(&format!(".{MAP_EXTENSION}"))
                .map(str::to_string)
        })
        .filter(|id| package.is_file(&format!("{}/map.json", map_root(campaign, id))))
        .collect();
    out.sort();
    out
}

fn read_json(package: &Package, path: &str) -> Option<Value> {
    package
        .read_text(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

/// The campaign's text file (D96): its steps' titles, pages, lines and notes.
pub fn text_file(root: &str) -> String {
    format!("{root}/text/{}/campaign.json", crate::text::LANGUAGE)
}

/// A step field's text: the library's when the field holds one of its keys, else the field.
pub fn said<'a>(library: &'a Value, value: &'a str) -> &'a str {
    library.get(value).and_then(Value::as_str).unwrap_or(value)
}

/// Every campaign package of `package`: its folder, its name and its campaign.json.
pub fn campaigns(package: &Package) -> Vec<(String, String, Value)> {
    let mut errors = Vec::new();
    CAMPAIGNS
        .read(package, &mut errors)
        .into_iter()
        .map(|found| {
            let name = read_json(package, &format!("{}/{MANIFEST}", found.path))
                .and_then(|m| m.get("name").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| found.id.clone());
            let value = read_json(package, &document(&found.path)).unwrap_or(Value::Null);
            (found.path, name, value)
        })
        .collect()
}

/// The Maps library's Campaigns group: each campaign, under it its maps in step order
/// ("2 · The Amber Watch"), then the maps of its `missions/` no step plays.
pub fn maps_group(package: &Package) -> Option<Node> {
    let mut groups = Vec::new();
    for (root, name, campaign) in campaigns(package) {
        let maps = mission_maps(package, &root);
        let numbers = mission_numbers(&campaign);
        let library = read_json(package, &text_file(&root)).unwrap_or_default();
        let mut children = Vec::new();
        let mut listed = BTreeSet::new();
        let element = |map: &str, name: String| {
            let path = map_root(&root, map);
            Element {
                id: map.to_string(),
                name,
                document: format!("{path}/map.json"),
                path,
            }
        };
        for (step, number) in steps(&campaign).iter().zip(&numbers) {
            let map = text(step, "map");
            let Some(number) = number else { continue };
            if !maps.iter().any(|m| m == map) || !listed.insert(map.to_string()) {
                continue;
            }
            let title = match said(&library, text(step, "title")) {
                "" => map,
                title => title,
            };
            children.push(Node::Element(element(map, format!("{number} · {title}"))));
        }
        for map in maps.iter().filter(|m| !listed.contains(m.as_str())) {
            let shown = read_json(package, &format!("{}/{MANIFEST}", map_root(&root, map)))
                .and_then(|m| m.get("name").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| map.clone());
            children.push(Node::Element(element(
                map,
                format!("{shown} · not in the campaign"),
            )));
        }
        let count = children.len();
        groups.push(Node::Folder {
            name,
            path: root,
            children,
            count,
        });
    }
    if groups.is_empty() {
        return None;
    }
    let count = groups.iter().map(Node::count).sum();
    Some(Node::Folder {
        name: "Campaigns".into(),
        path: DIR.into(),
        children: groups,
        count,
    })
}

/// A mission map's place in its campaign, for the map editor's Tech view and Players.
#[derive(Debug, Clone, PartialEq)]
pub struct MissionLink {
    /// The campaign's folder and its campaign.json as the store holds it.
    pub campaign: String,
    pub value: Value,
    /// The map's id, and the step whose mission plays it (none: not in the campaign).
    pub map: String,
    pub step: Option<usize>,
    /// The campaign's text library, which its steps' titles name.
    pub library: Value,
}

impl MissionLink {
    pub fn document(&self) -> String {
        document(&self.campaign)
    }

    /// A step's title as shown.
    pub fn title<'a>(&'a self, step: &'a Value) -> &'a str {
        said(&self.library, text(step, "title"))
    }

    /// The campaign's race.
    pub fn race(&self) -> &str {
        text(&self.value, "race")
    }
}

/// The value of a new map's `mission` section: the fields the map format requires,
/// filled from the campaign (its id, the step's title and number), and no players. Made
/// from the format's own schema, so it follows the format as it changes.
pub fn new_mission_section(campaign: &str, title: &str, order: usize) -> Value {
    let schema = schemars::schema_for!(canopy_content::map::Mission).to_value();
    let required: Vec<String> = ids(schema.get("required"));
    let properties = schema.get("properties").cloned().unwrap_or_default();
    let mut section = serde_json::Map::new();
    for field in required {
        let value = match field.as_str() {
            "campaign" => json!(campaign),
            "title" => json!(title),
            "order" => json!(order.max(1)),
            _ => match properties
                .get(&field)
                .and_then(|p| p.get("type"))
                .and_then(Value::as_str)
            {
                Some("array") => json!([]),
                Some("object") => json!({}),
                Some("integer") | Some("number") => json!(0),
                Some("boolean") => json!(false),
                _ => json!(""),
            },
        };
        section.insert(field, value);
    }
    section.insert("players".into(), json!([]));
    Value::Object(section)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign() -> Value {
        json!({"race": "r", "steps": [
            {"kind": "chapter", "id": "one", "title": "One"},
            {"kind": "mission", "id": "a", "map": "a", "title": "A",
             "tech": {"new": ["x", "y"], "blocked": []}},
            {"kind": "story", "id": "s", "title": "S", "pages": []},
            {"kind": "mission", "id": "b", "map": "b", "title": "B",
             "tech": {"new": ["z"], "blocked": ["x"]}},
            {"kind": "mission", "id": "c", "map": "c", "title": "C"},
        ]})
    }

    #[test]
    fn unlocks_accumulate_down_the_steps_and_blocks_last_one_mission() {
        let c = campaign();
        let b = Tech::at(&c, 3);
        assert_eq!(b.state("x"), TechState::Blocked);
        assert_eq!(b.state("y"), TechState::Available);
        assert_eq!(b.state("z"), TechState::New);
        assert_eq!(b.state("w"), TechState::Locked);
        let c3 = Tech::at(&c, 4);
        assert_eq!(c3.state("x"), TechState::Available);
        assert_eq!(c3.state("z"), TechState::Available);
        assert_eq!(c3.before["x"], 1);
    }

    #[test]
    fn a_click_cycles_locked_new_blocked_and_earlier_unlocks_between_available_and_blocked() {
        let c = campaign();
        let b = Tech::at(&c, 3);
        assert_eq!(b.next("w"), TechState::New);
        assert_eq!(b.next("z"), TechState::Blocked);
        assert_eq!(b.next("y"), TechState::Blocked);
        assert_eq!(b.next("x"), TechState::Available);
        let (new, blocked) = b.with("z", TechState::Blocked);
        assert!(new.is_empty());
        assert_eq!(blocked, ["x", "z"]);
        // An earlier unlock is never new again.
        let (new, _) = b.with("y", TechState::New);
        assert_eq!(new, ["z"]);
    }

    #[test]
    fn moving_a_step_lands_where_it_was_dropped() {
        let c = campaign();
        let doc = "campaigns/t.utcc/campaign.json";
        let mut store_value = c.clone();
        for command in move_step(doc, &c, 0, 3) {
            match command {
                Command::Remove { pointer, .. } => {
                    let i: usize = pointer.rsplit('/').next().unwrap().parse().unwrap();
                    store_value["steps"].as_array_mut().unwrap().remove(i);
                }
                Command::Insert { index, value, .. } => store_value["steps"]
                    .as_array_mut()
                    .unwrap()
                    .insert(index, value),
                _ => unreachable!(),
            }
        }
        let order: Vec<&str> = steps(&store_value).iter().map(|s| text(s, "id")).collect();
        assert_eq!(order, ["a", "s", "one", "b", "c"]);
        assert!(move_step(doc, &c, 2, 3).is_empty());
    }

    #[test]
    fn paths_name_campaigns_and_their_missions() {
        assert_eq!(
            root_of("campaigns/v.utcc/campaign.json"),
            Some("campaigns/v.utcc")
        );
        assert_eq!(
            mission_of("campaigns/v.utcc/missions/a-b.utcm/terrain/blocks.grid"),
            Some(("campaigns/v.utcc", "a-b"))
        );
        assert_eq!(mission_of("maps/skirmish/a.utcm/map.json"), None);
        assert_eq!(unique_step_id(&campaign(), "A"), "a-2");
        assert_eq!(
            unique_step_id(&campaign(), "The Glass Road"),
            "the-glass-road"
        );
    }

    #[test]
    fn a_new_mission_section_has_every_field_the_format_requires() {
        let section = new_mission_section("v", "Title", 2);
        let mission: Result<canopy_content::map::Mission, _> = serde_json::from_value(section);
        assert!(mission.is_ok(), "{mission:?}");
    }
}
