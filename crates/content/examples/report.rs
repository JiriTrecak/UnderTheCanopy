//! Which engine features the data actually uses (`just content-report`): behaviours,
//! creation methods, item effects, ability operations and deliveries, effect layer shapes,
//! and map features. Planning input for the simulation and renderer.

use canopy_content::{Content, map};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, process::ExitCode};

type Counts = BTreeMap<String, usize>;

fn main() -> ExitCode {
    let data = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let content = match Content::load(&data) {
        Ok(content) => content,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };
    let mut sections: BTreeMap<&str, Counts> = BTreeMap::new();
    for definition in &content.definitions {
        let value = serde_json::to_value(definition).expect("serializable");
        *sections
            .entry("definition kinds")
            .or_default()
            .entry(definition.kind.name().into())
            .or_default() += 1;
        for behavior in value["behaviors"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k)
        {
            *sections
                .entry("behaviours")
                .or_default()
                .entry(behavior.clone())
                .or_default() += 1;
        }
        if let Some(method) = value["creation"]["method"].as_str() {
            *sections
                .entry("creation methods")
                .or_default()
                .entry(method.into())
                .or_default() += 1;
        }
        if let Some(kind) = value["itemEffect"]["type"].as_str() {
            *sections
                .entry("item effects")
                .or_default()
                .entry(kind.into())
                .or_default() += 1;
        }
    }
    let abilities = serde_json::to_value(&content.abilities).expect("serializable");
    collect(
        &abilities["abilities"],
        "op",
        sections.entry("ability operations").or_default(),
    );
    for ability in abilities["abilities"].as_array().into_iter().flatten() {
        for key in ["activation"] {
            if let Some(v) = ability[key].as_str() {
                *sections
                    .entry("ability activation")
                    .or_default()
                    .entry(v.into())
                    .or_default() += 1;
            }
        }
        if let Some(kind) = ability["targeting"]["kind"].as_str() {
            *sections
                .entry("ability targeting")
                .or_default()
                .entry(kind.into())
                .or_default() += 1;
        }
    }
    collect(
        &abilities["abilities"],
        "delivery",
        sections.entry("ability deliveries").or_default(),
    );
    let effects = serde_json::to_value(&content.effects).expect("serializable");
    collect(
        &effects,
        "shape",
        sections.entry("effect layer shapes").or_default(),
    );
    match map::library(&data) {
        Ok(maps) => {
            for file in maps {
                let features = sections.entry("map features").or_default();
                *features.entry("maps".into()).or_default() += 1;
                if let Ok(utcmap) = file.load() {
                    let value = serde_json::to_value(&utcmap).expect("serializable");
                    for key in ["mission", "stamps", "camps", "entities"] {
                        let used = match &value[key] {
                            Value::Array(items) => !items.is_empty(),
                            Value::Null => false,
                            _ => true,
                        };
                        if used {
                            *features.entry(key.into()).or_default() += 1;
                        }
                    }
                }
            }
        }
        Err(errors) => eprintln!("{errors}"),
    }
    for (section, counts) in &sections {
        println!("{section}:");
        for (name, count) in counts {
            println!("  {name:<28} {count}");
        }
    }
    ExitCode::SUCCESS
}

/// Counts every string value of `key` anywhere below `value`, or the `kind` of objects
/// stored under `key`.
fn collect(value: &Value, key: &str, counts: &mut Counts) {
    match value {
        Value::Object(object) => {
            for (k, v) in object {
                if k == key {
                    let name = v.as_str().or_else(|| v["kind"].as_str());
                    if let Some(name) = name {
                        *counts.entry(name.into()).or_default() += 1;
                    }
                }
                collect(v, key, counts);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect(item, key, counts)),
        _ => {}
    }
}
