//! The agent skills (`.claude/skills/*`) describe the content formats with examples; every
//! fenced block tagged with a kind (```json unit, ```json asset …) must read as that kind,
//! so a skill cannot drift from the code. Untagged blocks are fragments and are not read.

use canopy_content::{
    abilities::Ability,
    assets::package::{AssetDefinition, SceneryDefinition, WaterFile},
    effects::VisualEffect,
    game::{ActionOverride, Definition, LootPool, Race, Research},
    package::Manifest,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn skills() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.claude/skills")
}

/// Every markdown file below `dir`.
fn markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            markdown(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// (kind, JSON text, line) of every block whose info string is `json <kind>`.
fn blocks(text: &str) -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    let mut open: Option<(String, String, usize)> = None;
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if open.is_some() && trimmed.starts_with("```") {
            out.extend(open.take());
        } else if let Some((_, body, _)) = &mut open {
            body.push_str(line);
            body.push('\n');
        } else if let Some(info) = trimmed.strip_prefix("```json ") {
            open = Some((info.trim().to_string(), String::new(), i + 1));
        }
    }
    out
}

fn without_id(mut value: Value) -> Value {
    value.as_object_mut().map(|o| o.remove("id"));
    value
}

fn definition(mut value: Value, kind: &str) -> Result<Definition, String> {
    let object = value.as_object_mut().ok_or("not an object")?;
    object.remove("behaviorSets");
    object.remove("disabledBehaviors");
    // The loader takes the command card override off the definition.
    if let Some(command) = object.remove("command") {
        serde_json::from_value::<ActionOverride>(command).map_err(|e| format!("command: {e}"))?;
    }
    object.insert("kind".into(), Value::String(kind.into()));
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn check(kind: &str, value: Value) -> Result<(), String> {
    let e = |e: serde_json::Error| e.to_string();
    match kind {
        "package" => serde_json::from_value::<Manifest>(value)
            .map(drop)
            .map_err(e),
        "unit" | "building" | "item" | "resource" => definition(value, kind).map(drop),
        "race" => serde_json::from_value::<Race>(without_id(value))
            .map(drop)
            .map_err(e),
        "research" => serde_json::from_value::<Research>(without_id(value))
            .map(drop)
            .map_err(e),
        "loot-pool" => serde_json::from_value::<LootPool>(without_id(value))
            .map(drop)
            .map_err(e),
        "behavior" => match value.as_object() {
            Some(o) if o.contains_key("id") && o["behaviors"].is_object() && o.len() == 2 => Ok(()),
            _ => Err("a behaviour set is {id, behaviors}".into()),
        },
        "asset" => serde_json::from_value::<AssetDefinition>(value)
            .map(drop)
            .map_err(e),
        "scenery" => serde_json::from_value::<SceneryDefinition>(value)
            .map(drop)
            .map_err(e),
        "water" => serde_json::from_value::<WaterFile>(value)
            .map(drop)
            .map_err(e),
        "ability" => serde_json::from_value::<Ability>(value)
            .map(drop)
            .map_err(e),
        "effect" => serde_json::from_value::<VisualEffect>(value)
            .map(drop)
            .map_err(e),
        other => Err(format!("unknown example kind {other}")),
    }
}

#[test]
fn every_tagged_skill_example_reads_as_its_kind() {
    let mut files = Vec::new();
    markdown(&skills(), &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no skills found at {}",
        skills().display()
    );
    let mut checked = 0;
    let mut problems = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for (kind, body, line) in blocks(&text) {
            let result = serde_json::from_str::<Value>(&body)
                .map_err(|e| e.to_string())
                .and_then(|value| check(&kind, value));
            match result {
                Ok(()) => checked += 1,
                Err(message) => {
                    problems.push(format!("{}:{line} ({kind}): {message}", file.display()))
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(checked >= 10, "only {checked} examples checked");
}
