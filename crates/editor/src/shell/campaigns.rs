//! The shell's side of campaigns (docs/editor.md "Campaigns"): the New campaign dialog,
//! what campaign views ask for (Play, opening a mission's map or its Tech view), and the
//! answers to MCP's campaign tools.

use super::*;
use crate::campaign::tech::{Graph, Reach, Source};
use crate::campaign::tools::Call;
use crate::campaign::view::Request as ViewRequest;
use crate::campaign::{self, StepKind, Tech, TechState};
use serde_json::json;

/// What the New campaign dialog decided.
pub(super) enum Answer {
    Open,
    Close,
    Run(Edit),
}

/// The New campaign dialog's body.
pub(super) fn new_campaign_dialog(
    ui: &mut Ui,
    id: &mut String,
    name: &mut String,
    race: &mut String,
    races: &[(String, String)],
    taken: bool,
    base_version: &str,
) -> Answer {
    let mut answer = Answer::Open;
    ui.label(RichText::new("New campaign").size(14.0).color(TEXT));
    ui.add_space(6.0);
    ui.label(RichText::new("Id").color(MUTED).size(11.0));
    ui.add(
        TextEdit::singleline(id)
            .font(egui::TextStyle::Monospace)
            .hint_text("vanguard")
            .desired_width(f32::INFINITY),
    );
    ui.label(RichText::new("Name").color(MUTED).size(11.0));
    ui.add(
        TextEdit::singleline(name)
            .hint_text("The Ant Vanguard")
            .desired_width(f32::INFINITY),
    );
    ui.label(
        RichText::new("Race the player plays")
            .color(MUTED)
            .size(11.0),
    );
    let shown = races
        .iter()
        .find(|(r, _)| r == race)
        .map_or(race.clone(), |(_, n)| n.clone());
    egui::ComboBox::from_id_salt("new-campaign-race")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (r, n) in races {
                ui.selectable_value(race, r.clone(), n);
            }
        });
    let problem = if id.is_empty() {
        Some("Type its id.".to_string())
    } else if let Err(e) = canopy_content::Id::new(id.as_str()) {
        Some(e)
    } else if id.contains('.') {
        Some("A campaign id has no dots.".into())
    } else if taken {
        Some("A campaign with this id is there already.".into())
    } else if race.is_empty() {
        Some("Choose its race.".into())
    } else {
        None
    };
    let root = format!("{}/{id}.utcc", campaign::DIR);
    ui.label(
        RichText::new(problem.clone().unwrap_or_else(|| {
            format!("Creates {root} with its first chapter: add stories, missions and cinematics in its view.")
        }))
        .color(FAINT)
        .size(11.0),
    );
    ui.add_space(12.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui
            .add_enabled(problem.is_none(), primary("Create"))
            .clicked()
        {
            let title = if name.trim().is_empty() {
                id.clone()
            } else {
                name.trim().to_string()
            };
            match campaign::new_campaign(id, &title, race, base_version) {
                Ok(commands) => {
                    answer = Answer::Run(Edit {
                        label: format!("Create campaign {id}"),
                        context: Context {
                            editor: Kind::Campaigns,
                            document: campaign::document(&root),
                            pointer: None,
                        },
                        commands,
                        merge: None,
                    })
                }
                Err(e) => tracing::warn!(error = %e, "editor: new campaign"),
            }
        }
        if ui.button("Cancel").clicked() {
            answer = Answer::Close;
        }
    });
    answer
}

impl Shell {
    /// Opens the New campaign dialog.
    pub(super) fn start_new_campaign(&mut self) {
        let content = self.checker.as_ref().and_then(|c| c.content().cloned());
        let races: Vec<(String, String)> = content
            .map(|c| {
                c.rules
                    .races
                    .iter()
                    .map(|(id, r)| (id.clone(), r.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        self.dialog = Some(Dialog::NewCampaign {
            id: String::new(),
            name: String::new(),
            race: races.first().map(|r| r.0.clone()).unwrap_or_default(),
            races,
        });
    }

    /// Each campaign's mission maps (ids by the campaign's folder), from the Maps library.
    pub(super) fn campaign_maps(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut out: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        if let Some(workspace) = self.workspaces.get(&Kind::Maps) {
            let mut elements = Vec::new();
            workspace
                .library
                .nodes
                .iter()
                .for_each(|n| n.elements(&mut elements));
            for e in elements {
                if let Some((root, map)) = campaign::mission_of(&e.document) {
                    out.entry(root.to_string())
                        .or_default()
                        .push(map.to_string());
                }
            }
        }
        for maps in out.values_mut() {
            maps.sort();
            maps.dedup();
        }
        out
    }

    /// Opens the mission map at `root` in the Maps editor (its Tech view when `tech`).
    fn open_mission_map(&mut self, root: &str, tech: bool) {
        if tech {
            let session = self
                .maps
                .entry(root.to_string())
                .or_insert_with(|| crate::map::Session::new(root));
            if session.blocks.view != crate::world::View::Tech {
                let next = session.blocks.toggle(crate::world::View::Tech);
                session.blocks.set_view(next);
            }
            session.tech.mission = None;
            session.tech.fit = true;
        }
        if let Some(here) = self.active_document() {
            self.back.push((self.current, here));
        }
        self.forward.clear();
        let document = format!("{root}/map.json");
        self.reveal(&Context {
            editor: Kind::Maps,
            document,
            pointer: None,
        });
    }

    /// What campaign views asked for this frame.
    pub(super) fn campaign_requests(&mut self) {
        for request in std::mem::take(&mut self.campaigns.requests) {
            match request {
                ViewRequest::Map { root, tech } => self.open_mission_map(&root, tech),
                ViewRequest::Play { document, step } => {
                    if let Err(error) = self.play_step(&document, step) {
                        self.say(error, true);
                    }
                }
            }
        }
    }

    /// Plays campaign `document` from step `index` (Play from here): the game runs the
    /// step from a copy of the campaign package with its unsaved edits, mission maps
    /// included, and comes back to the editor when it is left.
    fn play_step(&mut self, document: &str, index: usize) -> Result<(), String> {
        let root = campaign::root_of(document)
            .ok_or("not a campaign")?
            .to_string();
        let value = self
            .store
            .as_mut()
            .and_then(|s| s.get(document).ok().flatten().cloned())
            .ok_or("the campaign does not read")?;
        let step = campaign::steps(&value)
            .get(index)
            .cloned()
            .ok_or("no such step")?;
        let content = self
            .testable()
            .ok_or("Fix the problems first: the game loads only a package that passes the check")?;
        let id = campaign::id_of(&root).to_string();
        let (Some(store), Ok(package)) = (&self.store, &self.package) else {
            return Err("No package".into());
        };
        let target = std::env::temp_dir()
            .join(format!("canopy-editor-campaign-{}", std::process::id()))
            .join(format!("{id}.utcc"));
        let _ = std::fs::remove_dir_all(&target);
        crate::map::write_overlaid(
            &package.root.join(&root),
            &store.overlay().below(&root),
            &target,
        )
        .map_err(|e| format!("The campaign's edits could not be copied: {e}"))?;
        self.test = Some(crate::Test {
            content,
            map: None,
            map_package: None,
            encounter: None,
            autoplay: false,
            campaign: Some(crate::CampaignStep {
                campaign: id,
                step: campaign::text(&step, "id").to_string(),
                index,
                package: target,
            }),
        });
        Ok(())
    }

    /// The campaign an agent named by id or folder.
    fn campaign_root(&self, name: &str) -> Result<String, String> {
        let name = name
            .trim_end_matches('/')
            .trim_end_matches(&format!("/{}", campaign::FILE));
        let workspace = self
            .workspaces
            .get(&Kind::Campaigns)
            .ok_or("No campaigns library")?;
        let mut elements = Vec::new();
        workspace
            .library
            .nodes
            .iter()
            .for_each(|n| n.elements(&mut elements));
        elements
            .iter()
            .find(|e| e.id == name || e.path == name)
            .map(|e| e.path.clone())
            .ok_or_else(|| {
                format!("No campaign {name}; list_elements with editor campaigns lists them")
            })
    }

    fn campaign_value(&mut self, root: &str) -> Result<Value, String> {
        let document = campaign::document(root);
        self.store
            .as_mut()
            .and_then(|s| s.get(&document).ok().flatten().cloned())
            .ok_or_else(|| format!("{document} does not read"))
    }

    /// Runs a campaign step of `commands` on the campaign at `root`.
    fn run_campaign(
        &mut self,
        root: &str,
        label: &str,
        commands: Vec<Command>,
    ) -> Result<(), String> {
        let context = Context {
            editor: Kind::Campaigns,
            document: campaign::document(root),
            pointer: None,
        };
        let store = self.store.as_mut().ok_or("No package")?;
        store
            .run(label, context, commands)
            .map_err(|e| e.to_string())?;
        if let Ok(package) = self.package.clone() {
            self.refresh_libraries(&package);
        }
        Ok(())
    }

    /// The index of the step an agent named by id, index, or (for missions) map.
    fn step_index(value: &Value, step: &Value) -> Result<usize, String> {
        let list = campaign::steps(value);
        let found = match step {
            Value::Number(n) => n.as_u64().map(|n| n as usize).filter(|n| *n < list.len()),
            Value::String(s) => list
                .iter()
                .position(|x| campaign::text(x, "id") == s)
                .or_else(|| campaign::step_of_map(value, s)),
            _ => None,
        };
        found.ok_or_else(|| format!("No step {step} in the campaign"))
    }

    pub(super) fn answer_campaign(&mut self, call: Call) -> Result<Value, String> {
        match call {
            Call::Create(args) => {
                let root = format!("{}/{}.utcc", campaign::DIR, args.id);
                if matches!(
                    self.store
                        .as_mut()
                        .map(|s| s.get(&campaign::document(&root))),
                    Some(Ok(Some(_)))
                ) {
                    return Err(format!("{root} already exists"));
                }
                if let Some(content) = self.checker.as_ref().and_then(|c| c.content())
                    && !content.rules.races.contains_key(&args.race)
                {
                    return Err(format!("Unknown race {}", args.race));
                }
                let version = self
                    .package
                    .as_ref()
                    .map(|p| p.manifest.version.to_string())
                    .map_err(Clone::clone)?;
                let name = args.name.unwrap_or_else(|| args.id.clone());
                let commands = campaign::new_campaign(&args.id, &name, &args.race, &version)?;
                self.run_campaign(&root, &format!("Create campaign {}", args.id), commands)?;
                Ok(json!({ "created": root, "document": campaign::document(&root) }))
            }
            Call::Steps(args) => {
                let root = self.campaign_root(&args.campaign)?;
                let value = self.campaign_value(&root)?;
                // Titles are text keys (D96); their words are in the campaign's text file.
                let words = self
                    .store
                    .as_mut()
                    .and_then(|s| {
                        s.get(&format!(
                            "{root}/text/{}/campaign.json",
                            crate::text::LANGUAGE
                        ))
                        .ok()
                        .flatten()
                        .cloned()
                    })
                    .unwrap_or(Value::Null);
                let numbers = campaign::mission_numbers(&value);
                let steps: Vec<Value> = campaign::steps(&value)
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let mut out = json!({
                            "index": i,
                            "kind": campaign::text(s, "kind"),
                            "id": campaign::text(s, "id"),
                            "title": campaign::text(s, "title"),
                        });
                        if let Some(text) = words
                            .get(campaign::text(s, "title"))
                            .and_then(Value::as_str)
                        {
                            out["titleText"] = json!(text);
                        }
                        if let Some(n) = numbers[i] {
                            let tech = Tech::at(&value, i);
                            out["mission"] = json!(n);
                            out["map"] = json!(campaign::text(s, "map"));
                            out["tech"] = json!({"new": tech.new, "blocked": tech.blocked, "unlockedBefore": tech.before.len()});
                        }
                        out
                    })
                    .collect();
                let document = campaign::document(&root);
                let problems: Vec<Value> = self
                    .checker
                    .as_ref()
                    .and_then(|c| c.report())
                    .map(|r| {
                        r.problems
                            .iter()
                            .filter(|p| p.document.as_deref() == Some(document.as_str()))
                            .map(|p| json!({"location": p.location, "message": p.message, "warning": p.warning}))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json!({
                    "campaign": root,
                    "race": campaign::text(&value, "race"),
                    "background": campaign::text(&value, "background"),
                    "card": campaign::text(&value, "card"),
                    "steps": steps,
                    "problems": problems,
                    "missionMaps": self.campaign_maps().remove(&root).unwrap_or_default(),
                }))
            }
            Call::Add(args) => {
                let root = self.campaign_root(&args.campaign)?;
                let value = self.campaign_value(&root)?;
                let kind = StepKind::parse(&args.kind)
                    .ok_or("kind is chapter, story, mission or cinematic")?;
                let missions = campaign::steps(&value)
                    .iter()
                    .filter(|s| StepKind::of(s) == Some(StepKind::Mission))
                    .count();
                let title = args.title.unwrap_or_else(|| match kind {
                    StepKind::Mission => format!("Mission {}", missions + 1),
                    k => format!("New {}", k.name()),
                });
                let id = match args.id {
                    Some(id) => {
                        if campaign::steps(&value)
                            .iter()
                            .any(|s| campaign::text(s, "id") == id)
                        {
                            return Err(format!("A step has the id {id} already"));
                        }
                        id
                    }
                    None => campaign::unique_step_id(&value, &title),
                };
                let at = args.at.unwrap_or(usize::MAX);
                let mut commands = Vec::new();
                let mut map = args.map.clone();
                if kind == StepKind::Mission && map.is_none() {
                    let content = self
                        .checker
                        .as_ref()
                        .and_then(|c| c.content().cloned())
                        .ok_or("The package has not passed a check yet; see problems")?;
                    let maps = self.campaign_maps().remove(&root).unwrap_or_default();
                    let mut map_id = id.clone();
                    let mut n = 2;
                    while maps.contains(&map_id) {
                        map_id = format!("{id}-{n}");
                        n += 1;
                    }
                    let version = self
                        .package
                        .as_ref()
                        .map(|p| p.manifest.version.to_string())
                        .map_err(Clone::clone)?;
                    commands.extend(crate::map::new_map_at(
                        &content,
                        &version,
                        &campaign::map_root(&root, &map_id),
                        &map_id,
                        &title,
                        512,
                        campaign::mission_biome(
                            self.package.as_ref().ok().map(|p| p.root.as_path()),
                        ),
                    )?);
                    map = Some(map_id);
                }
                let step = campaign::new_step(kind, &id, &title, map.as_deref());
                let document = campaign::document(&root);
                let index = at.min(campaign::steps(&value).len());
                commands.push(campaign::insert_step(
                    &document,
                    &value,
                    index,
                    step.clone(),
                ));
                self.run_campaign(&root, &format!("Add {} {title}", kind.name()), commands)?;
                Ok(
                    json!({ "added": step, "index": index, "map": map.map(|m| campaign::map_root(&root, &m)) }),
                )
            }
            Call::Move(args) => {
                let root = self.campaign_root(&args.campaign)?;
                let value = self.campaign_value(&root)?;
                let from = Self::step_index(&value, &args.step)?;
                let len = campaign::steps(&value).len();
                let to = args.to.min(len - 1);
                // `to` is where it ends up: in the list before the move, that is past it.
                let before = if to > from { to + 1 } else { to };
                let commands =
                    campaign::move_step(&campaign::document(&root), &value, from, before);
                if commands.is_empty() {
                    return Ok(json!({ "moved": false }));
                }
                self.run_campaign(&root, "Move step", commands)?;
                Ok(json!({ "moved": true, "from": from, "to": to }))
            }
            Call::Remove(args) => {
                let root = self.campaign_root(&args.campaign)?;
                let value = self.campaign_value(&root)?;
                let index = Self::step_index(&value, &args.step)?;
                let document = campaign::document(&root);
                self.run_campaign(
                    &root,
                    "Remove step",
                    vec![Command::Remove {
                        document,
                        pointer: format!("/steps/{index}"),
                    }],
                )?;
                Ok(json!({ "removed": index }))
            }
            Call::Set(args) => {
                let root = self.campaign_root(&args.campaign)?;
                let value = self.campaign_value(&root)?;
                let index = Self::step_index(&value, &args.step)?;
                let step = &campaign::steps(&value)[index];
                let document = campaign::document(&root);
                let mut commands = Vec::new();
                for (field, v) in args.fields {
                    let pointer = format!("/steps/{index}/{field}");
                    if v.is_null() {
                        if step.get(&field).is_some() {
                            commands.push(Command::Remove {
                                document: document.clone(),
                                pointer,
                            });
                        }
                    } else {
                        commands.push(Command::Set {
                            document: document.clone(),
                            pointer,
                            value: v,
                            at: None,
                        });
                    }
                }
                let count = commands.len();
                self.run_campaign(&root, "Set step fields", commands)?;
                Ok(json!({ "step": index, "changed": count }))
            }
            Call::Tech(args) => {
                let root = self.campaign_root(&args.campaign)?;
                let value = self.campaign_value(&root)?;
                let index = Self::step_index(&value, &args.step)?;
                if StepKind::of(&campaign::steps(&value)[index]) != Some(StepKind::Mission) {
                    return Err("Only missions have tech".into());
                }
                let mut tech = Tech::at(&value, index);
                for (id, state) in &args.set {
                    let to = match state.as_str() {
                        "new" => TechState::New,
                        "blocked" => TechState::Blocked,
                        "locked" => TechState::Locked,
                        "available" => TechState::Available,
                        other => {
                            return Err(format!(
                                "{id}: {other} is not new, blocked, locked or available"
                            ));
                        }
                    };
                    let (new, blocked) = tech.with(id, to);
                    tech.new = new;
                    tech.blocked = blocked;
                }
                let command = campaign::set_tech(
                    &campaign::document(&root),
                    index,
                    tech.new.clone(),
                    tech.blocked.clone(),
                );
                self.run_campaign(&root, "Set mission tech", vec![command])?;
                let contradictions = self.contradictions(&value, &campaign::document(&root), index);
                Ok(
                    json!({ "new": tech.new, "blocked": tech.blocked, "contradictions": contradictions }),
                )
            }
            Call::Tree(args) => {
                let root = self.campaign_root(&args.campaign)?;
                let value = self.campaign_value(&root)?;
                let index = Self::step_index(&value, &args.step)?;
                let content = self.content()?;
                let tech = Tech::at(&value, index);
                let graph = Graph::build(
                    &Source::of(&content, campaign::text(&value, "race")),
                    &|_| true,
                );
                let reach = Reach::of(&graph, &tech);
                let tiers: Vec<String> = graph
                    .halls
                    .iter()
                    .map(|h| graph.nodes[*h].id.clone())
                    .collect();
                let nodes: Vec<Value> = graph
                    .nodes
                    .iter()
                    .map(|n| {
                        let card = &graph.nodes[graph.cards[n.card].node];
                        json!({
                            "id": n.id,
                            "name": n.name,
                            "kind": format!("{:?}", n.kind).to_lowercase(),
                            "tier": n.tier + 1,
                            "under": card.id,
                            "state": tech.state(&n.id).label().to_lowercase(),
                            "reachable": reach.reached.contains(&n.id) || reach.needs.get(&n.id).is_some_and(Result::is_ok),
                            "why": reach.needs.get(&n.id).and_then(|r| r.clone().err()),
                        })
                    })
                    .collect();
                let contradictions: Vec<Value> = reach
                    .contradictions(&tech)
                    .into_iter()
                    .map(|(id, why)| json!({"id": id, "why": why}))
                    .collect();
                Ok(json!({ "halls": tiers, "nodes": nodes, "contradictions": contradictions }))
            }
            Call::Players(args) => {
                let map = match (&args.map, &args.campaign, &args.step) {
                    (Some(map), _, _) => map.clone(),
                    (None, Some(campaign), Some(step)) => {
                        let root = self.campaign_root(campaign)?;
                        let value = self.campaign_value(&root)?;
                        let index = Self::step_index(&value, step)?;
                        let map = campaign::text(&campaign::steps(&value)[index], "map");
                        if map.is_empty() {
                            return Err(format!("Step {index} of {root} is not a mission"));
                        }
                        format!("{root}/{}/{map}.utcm", campaign::MISSIONS)
                    }
                    _ => return Err("Name the mission: map, or campaign and step".into()),
                };
                let root = self.map_root_of(&map)?;
                let (campaign_root, _) =
                    campaign::mission_of(&root).ok_or("Not a campaign's mission map")?;
                let campaign_root = campaign_root.to_string();
                let mut problems = Vec::new();
                for (i, p) in args.players.iter().enumerate() {
                    if let Err(e) =
                        serde_json::from_value::<canopy_content::map::MissionPlayer>(p.clone())
                    {
                        problems.push(format!("players[{i}]: {e}"));
                    }
                }
                let document = format!("{root}/map.json");
                let map = self
                    .store
                    .as_mut()
                    .and_then(|s| s.get(&document).ok().flatten().cloned())
                    .ok_or_else(|| format!("{document} does not read"))?;
                let players = Value::Array(args.players);
                let command = if map.get("mission").is_some_and(Value::is_object) {
                    Command::Set {
                        document: document.clone(),
                        pointer: "/mission/players".into(),
                        value: players,
                        at: None,
                    }
                } else {
                    let value = self.campaign_value(&campaign_root)?;
                    let map_id = campaign::mission_of(&root)
                        .map(|(_, m)| m.to_string())
                        .unwrap_or_default();
                    let step = campaign::step_of_map(&value, &map_id);
                    let title = step
                        .and_then(|s| campaign::steps(&value).get(s))
                        .map_or("", |s| campaign::text(s, "title"));
                    let order = step
                        .and_then(|s| campaign::mission_numbers(&value)[s])
                        .unwrap_or(1);
                    let mut section = campaign::new_mission_section(
                        campaign::id_of(&campaign_root),
                        title,
                        order,
                    );
                    section["players"] = players;
                    Command::Set {
                        document: document.clone(),
                        pointer: "/mission".into(),
                        value: section,
                        at: None,
                    }
                };
                let context = Context {
                    editor: Kind::Maps,
                    document,
                    pointer: Some("/mission/players".into()),
                };
                let store = self.store.as_mut().ok_or("No package")?;
                store
                    .run("Mission players", context, vec![command])
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "set": root, "formatProblems": problems }))
            }
        }
    }

    /// The contradictions of step `index` of `value` (its campaign as just edited).
    fn contradictions(&mut self, _before: &Value, document: &str, index: usize) -> Vec<Value> {
        let Some(content) = self.checker.as_ref().and_then(|c| c.content().cloned()) else {
            return Vec::new();
        };
        let Some(value) = self
            .store
            .as_mut()
            .and_then(|s| s.get(document).ok().flatten().cloned())
        else {
            return Vec::new();
        };
        let tech = Tech::at(&value, index);
        let graph = Graph::build(
            &Source::of(&content, campaign::text(&value, "race")),
            &|_| true,
        );
        Reach::of(&graph, &tech)
            .contradictions(&tech)
            .into_iter()
            .map(|(id, why)| json!({"id": id, "why": why}))
            .collect()
    }

    /// The package path of a map an agent named by id or path (missions included).
    fn map_root_of(&self, name: &str) -> Result<String, String> {
        let name = name.trim_end_matches('/').trim_end_matches("/map.json");
        let workspace = self.workspaces.get(&Kind::Maps).ok_or("No maps library")?;
        let mut elements = Vec::new();
        workspace
            .library
            .nodes
            .iter()
            .for_each(|n| n.elements(&mut elements));
        elements
            .iter()
            .find(|e| e.path == name)
            .or_else(|| {
                elements
                    .iter()
                    .find(|e| e.id == name && campaign::is_mission_document(&e.document))
            })
            .map(|e| e.path.clone())
            .ok_or_else(|| format!("No map {name}"))
    }

    /// Runs MCP's campaign tool `tool` with `args` (captures and tests drive campaigns as
    /// agents do).
    pub fn campaign_tool(&mut self, tool: &str, args: Value) -> Result<Value, String> {
        fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
            serde_json::from_value(args).map_err(|e| e.to_string())
        }
        let call = match tool {
            "create_campaign" => Call::Create(parse(args)?),
            "campaign_steps" => Call::Steps(parse(args)?),
            "add_campaign_step" => Call::Add(parse(args)?),
            "move_campaign_step" => Call::Move(parse(args)?),
            "remove_campaign_step" => Call::Remove(parse(args)?),
            "set_campaign_step" => Call::Set(parse(args)?),
            "set_mission_tech" => Call::Tech(parse(args)?),
            "mission_tech_tree" => Call::Tree(parse(args)?),
            "set_mission_players" => Call::Players(parse(args)?),
            other => return Err(format!("no campaign tool {other}")),
        };
        self.answer_campaign(call)
    }

    /// Selects step `index` of campaign `document` in its view (none: its settings).
    pub fn select_campaign_step(&mut self, document: &str, index: Option<usize>) {
        self.campaigns
            .states
            .entry(document.to_string())
            .or_default()
            .selected = index;
    }

    /// Opens a campaign's mission map (its Tech view when `tech`), as the campaign view's
    /// Edit and Open tech graph do.
    pub fn open_mission(&mut self, root: &str, tech: bool) {
        self.open_mission_map(root, tech);
    }

    /// The Tech view of the open mission map: the mission shown (a step index; none: the
    /// map's own) and whether it shows only the changes.
    pub fn set_tech_view(&mut self, mission: Option<usize>, changes_only: bool) {
        if let Some(root) = self.active_map_root()
            && let Some(session) = self.maps.get_mut(&root)
        {
            session.tech.mission = mission;
            session.tech.changes_only = changes_only;
            session.tech.fit = true;
        }
    }

    /// Interface images whose id holds `text` (captures pick a campaign's art).
    pub fn interface_images(&self, text: &str) -> Vec<String> {
        self.index
            .iter()
            .filter(|(id, e)| e.document.starts_with("assets/interface/") && id.contains(text))
            .map(|(id, _)| id.clone())
            .collect()
    }
}
