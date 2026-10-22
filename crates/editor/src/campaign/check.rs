//! The campaign check (docs/campaign.md "The editor"): what is wrong with each campaign
//! of the package, unsaved edits included, added to the problems list after the loader's
//! own check. Errors are the runtime's rules (`canopy_content::campaign::validate`: the
//! format, the race, unique step ids, maps among `missions/`, the race's tech, assets that
//! exist) and broken mission maps; warnings are what a campaign should still settle
//! (missing art, tech contradictions, carried heroes missing from the next map, variables
//! read that nothing sets).

use super::tech::{Graph, Reach, Source};
use super::{StepKind, Tech, campaigns, map_root, mission_maps, mission_numbers, steps, text};
use crate::checker::Problem;
use canopy_content::Content;
use canopy_content::campaign::CampaignPackage;
use canopy_content::map::MapFile;
use canopy_content::package::{MANIFEST, Package};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Every campaign problem of `package` (laid over with the unsaved documents): the
/// runtime's own rules (`canopy_content::campaign::validate`) and what the editor sees.
pub fn problems(package: &Package, content: Option<&Content>) -> Vec<Problem> {
    let mut out = Vec::new();
    for (root, _, campaign) in campaigns(package) {
        let document = super::document(&root);
        let mut say = |location: String, message: String, warning: bool| {
            out.push(Problem {
                document: Some(document.clone()),
                location,
                message,
                file: document.clone(),
                warning,
            })
        };
        // The format and the runtime's rules, as the game will load it.
        let typed: Result<canopy_content::campaign::Campaign, _> =
            canopy_content::error::parse(campaign.clone(), &document, "");
        let typed = match typed {
            Ok(typed) => typed,
            Err(e) => {
                say(e.location, e.message, false);
                continue;
            }
        };
        let maps = mission_maps(package, &root);
        if let (Some(c), Ok(nested)) = (content, package.nested(&root)) {
            let missions = maps
                .iter()
                .map(|id| MapFile {
                    id: id.clone(),
                    group: "campaign",
                    file: map_root(&root, id),
                    path: package.root.join(map_root(&root, id)),
                })
                .collect();
            let checked = CampaignPackage {
                package: nested,
                campaign: typed,
                missions,
            };
            for e in canopy_content::campaign::validate(&checked, c) {
                say(e.location, e.message, false);
            }
        }
        let race = text(&campaign, "race");
        // Art the campaign should still have.
        for (field, what) in [("background", "background art"), ("card", "card art")] {
            if text(&campaign, field).is_empty() {
                say(field.into(), format!("No {what}"), true);
            }
        }
        let numbers = mission_numbers(&campaign);
        // The tree contradictions are worked out on, once per campaign.
        let graph = content.map(|c| Graph::build(&Source::of(c, race), &|_| true));
        // Heroes of player 1 the missions so far placed, by the mission that placed them.
        let mut carried: BTreeMap<String, usize> = BTreeMap::new();
        // Campaign variables set and read by the missions' scripts.
        let mut set_vars = BTreeSet::new();
        let mut read_vars: Vec<(usize, String)> = Vec::new();
        for (i, step) in steps(&campaign).iter().enumerate() {
            let at = |field: &str| format!("steps[{i}].{field}");
            match StepKind::of(step) {
                Some(StepKind::Story) => {
                    let pages = step.get("pages").and_then(Value::as_array);
                    for (p, page) in pages.into_iter().flatten().enumerate() {
                        if text(page, "art").is_empty() {
                            say(at(&format!("pages[{p}].art")), "No page art".into(), true);
                        }
                    }
                }
                Some(StepKind::Mission) => {
                    let n = numbers[i].unwrap_or(0);
                    if text(step, "art").is_empty() {
                        say(at("art"), "No loading-screen art".into(), true);
                    }
                    let map = text(step, "map");
                    let mut map_value = None;
                    if maps.iter().any(|m| m == map) {
                        match broken(package, &map_root(&root, map)) {
                            Ok(value) => map_value = Some(value),
                            Err(why) => {
                                say(at("map"), format!("The map {map} is broken: {why}"), false)
                            }
                        }
                    }
                    // Nothing allowed that cannot be reached.
                    if let Some(graph) = &graph {
                        let tech = Tech::at(&campaign, i);
                        let reach = Reach::of(graph, &tech);
                        for (id, why) in reach.contradictions(&tech) {
                            let name = graph
                                .nodes
                                .iter()
                                .find(|node| node.id == id)
                                .map_or(id.clone(), |node| node.name.clone());
                            say(
                                at("tech"),
                                format!("Mission {n} allows {name}, but {why}"),
                                true,
                            );
                        }
                    }
                    // Carried heroes the earlier missions had that this map leaves out.
                    if let Some(value) = &map_value {
                        let heroes = player_heroes(value, content);
                        for (hero, from) in &carried {
                            if !heroes.contains(hero) {
                                let name = content
                                    .and_then(|c| c.definition(hero))
                                    .map_or(hero.clone(), |d| d.name.clone());
                                say(
                                    at("map"),
                                    format!(
                                        "{name}, carried since mission {from}, is not placed on {map}"
                                    ),
                                    true,
                                );
                            }
                        }
                        for hero in heroes {
                            carried.entry(hero).or_insert(n);
                        }
                        let script = value
                            .pointer("/mission/script")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        set_vars.extend(calls(script, "campaign_set"));
                        read_vars.extend(calls(script, "campaign_get").into_iter().map(|v| (i, v)));
                    }
                }
                _ => {}
            }
        }
        for (i, name) in read_vars {
            if !set_vars.contains(&name) {
                say(
                    format!("steps[{i}].map"),
                    format!(
                        "The script reads the campaign variable {name:?}, which no mission sets"
                    ),
                    true,
                );
            }
        }
    }
    out
}

/// The map package at `root` reads as a map: its manifest, its map.json and its blocks.
fn broken(package: &Package, root: &str) -> Result<Value, String> {
    let read = |file: &str| -> Result<Value, String> {
        let text = package
            .read_text(&format!("{root}/{file}"))
            .map_err(|e| format!("{file}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))
    };
    read(MANIFEST)?;
    let map = read("map.json")?;
    if !package.is_file(&format!("{root}/terrain/blocks.grid")) {
        return Err("it has no blocks".into());
    }
    Ok(map)
}

/// The heroes the map places for player 1.
fn player_heroes(map: &Value, content: Option<&Content>) -> BTreeSet<String> {
    map.get("entities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|e| e.get("owner").and_then(Value::as_str) == Some("player.1"))
        .filter_map(|e| e.get("definition").and_then(Value::as_str))
        .filter(|d| {
            content
                .and_then(|c| c.definition(d))
                .is_some_and(|d| d.is_hero())
        })
        .map(str::to_string)
        .collect()
}

/// The names a script passes to `function` as a string literal (`campaign_get("x")`).
fn calls(script: &str, function: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = script;
    while let Some(at) = rest.find(function) {
        rest = &rest[at + function.len()..];
        let after = rest.trim_start();
        let Some(after) = after.strip_prefix('(') else {
            continue;
        };
        let after = after.trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        if let Some(end) = after[1..].find(quote) {
            out.push(after[1..1 + end].to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_name_the_variables_they_set_and_read() {
        let script = "mission.campaign_set(\"spared\", true)\nif mission.campaign_get( 'spared' ) then end\nlocal x = campaign_get(name)";
        assert_eq!(calls(script, "campaign_set"), ["spared"]);
        assert_eq!(calls(script, "campaign_get"), ["spared"]);
    }
}
