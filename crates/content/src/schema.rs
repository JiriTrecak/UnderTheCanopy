//! JSON Schemas of the documents as packages store them (docs/editor.md): generated from
//! the content types, then shaped like the files where the loader reshapes them
//! (definitions gain their `kind` from their folder, keyed collections carry their `id`,
//! the rules files are parts of [`Rules`]). The editor builds its forms from them; MCP
//! describes its tools with them.

use crate::{
    abilities::Ability,
    assets::package::{AssetDefinition, SceneryDefinition, WaterFile},
    effects::VisualEffect,
    game::{ActionOverride, Actions, Definition, Experience, LootPool, Race, Research, Rules},
    id::Id,
};
use schemars::{JsonSchema, generate::SchemaSettings};
use serde_json::{Value, json};

fn root<T: JsonSchema>() -> Value {
    SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value()
}

/// `T`'s schema with `id` as its first, required property (keyed collections drop it
/// before typing).
fn with_id<T: JsonSchema>() -> Value {
    let mut schema = root::<T>();
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        let mut ordered = serde_json::Map::new();
        ordered.insert("id".into(), root::<Id>());
        ordered.extend(std::mem::take(properties));
        *properties = ordered;
    }
    require(&mut schema, "id");
    schema
}

fn require(schema: &mut Value, name: &str) {
    let required = schema
        .as_object_mut()
        .expect("an object schema")
        .entry("required")
        .or_insert_with(|| json!([]));
    if let Some(list) = required.as_array_mut()
        && !list.iter().any(|v| v == name)
    {
        list.insert(0, json!(name));
    }
}

fn drop_properties(schema: &mut Value, names: &[&str]) {
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        for name in names {
            properties.shift_remove(*name);
        }
    }
    if let Some(required) = schema.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|v| !names.iter().any(|n| v == *n));
    }
}

/// A definition file: the folder gives the kind, and `command` carries the definition's
/// command card overrides.
fn definition() -> Value {
    let mut schema = root::<Definition>();
    drop_properties(&mut schema, &["kind"]);
    let command = root::<ActionOverride>();
    if let Some(defs) = command.get("$defs").and_then(Value::as_object) {
        let target = schema
            .as_object_mut()
            .expect("an object schema")
            .entry("$defs")
            .or_insert_with(|| json!({}));
        for (name, def) in defs {
            target
                .as_object_mut()
                .expect("an object")
                .insert(name.clone(), def.clone());
        }
    }
    let mut command = command;
    if let Some(object) = command.as_object_mut() {
        object.remove("$defs");
        object.remove("$schema");
    }
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert("command".into(), command);
    }
    schema
}

/// The rule keys assembled from other files, absent from `rules/game.json` and
/// `rules/combat.json`.
const ASSEMBLED_RULES: [&str; 5] = [
    "research",
    "lootPools",
    "races",
    "startingSetup",
    "experience",
];

/// The schema of the document at `path` (`/`-separated from its package's root), when the
/// format has one.
pub fn of_document(path: &str) -> Option<Value> {
    if path == crate::campaign::CAMPAIGN_FILE {
        return Some(root::<crate::campaign::Campaign>());
    }
    let (folder, rest) = path.split_once('/')?;
    Some(match folder {
        "abilities" => root::<Ability>(),
        "effects" => root::<VisualEffect>(),
        "units" | "buildings" | "items" | "resources" => definition(),
        "races" => with_id::<Race>(),
        "research" => with_id::<Research>(),
        "loot-pools" => with_id::<LootPool>(),
        "scenery" => root::<SceneryDefinition>(),
        "assets" if rest.ends_with("/asset.json") => root::<AssetDefinition>(),
        "landscape" if rest.starts_with("water/") => root::<WaterFile>(),
        "rules" => match rest {
            "experience.json" => root::<Experience>(),
            "commands.json" => {
                let mut schema = root::<Actions>();
                drop_properties(&mut schema, &["overrides"]);
                schema
            }
            "game.json" | "combat.json" => {
                // Each file holds part of the rules: every field is optional here.
                let mut schema = root::<Rules>();
                drop_properties(&mut schema, &ASSEMBLED_RULES);
                if let Some(object) = schema.as_object_mut() {
                    object.remove("required");
                }
                schema
            }
            _ => return None,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_have_file_shaped_schemas() {
        let unit = of_document("units/ants/worker.json").unwrap();
        let properties = unit["properties"].as_object().unwrap();
        assert!(properties.contains_key("command"));
        assert!(!properties.contains_key("kind"));
        let race = of_document("races/ants.json").unwrap();
        assert_eq!(
            race["properties"]
                .as_object()
                .unwrap()
                .keys()
                .next()
                .unwrap(),
            "id"
        );
        assert!(race["required"].as_array().unwrap().contains(&json!("id")));
        let game = of_document("rules/game.json").unwrap();
        assert!(game.get("required").is_none());
        assert!(
            !game["properties"]
                .as_object()
                .unwrap()
                .contains_key("races")
        );
        assert!(of_document("assets/units/knight/asset.json").is_some());
        assert!(of_document("assets/units/knight/data.json").is_none());
        assert!(of_document("behaviors/army.json").is_none());
        // Ids are marked for pickers.
        let ability = of_document("abilities/x.json").unwrap();
        assert!(ability.to_string().contains("canopy-id"));
    }
}
