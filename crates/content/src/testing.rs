//! A complete, valid data folder written from test-owned JSON, for tests in any crate
//! (feature `testing`). Tests never use shipped content ids.

use crate::{Content, Errors};
use serde_json::{Value, json};
use std::path::PathBuf;

/// The files of a data folder. Libraries start empty; tests fill in what they need.
pub struct DataFolder {
    pub game: Value,
    /// `{abilities, presentations}`: abilities with their presentations by id, as the old
    /// library held them (written as one ability file each).
    pub abilities: Value,
    /// Visual effects (one file each).
    pub effects: Value,
    /// Asset records in the old registry shape (`outputs`, `render`, `scenery`), written as
    /// asset folders with one-byte files, scenery definitions and definition presentation.
    pub records: Vec<Value>,
    /// Landscape catalogue entries in the old shape: kinds, harvesting, water.
    pub landscape: Value,
}

impl DataFolder {
    pub fn new(game: Value, records: Vec<Value>) -> Self {
        Self {
            game,
            abilities: json!({"schemaVersion": 1, "abilities": [], "presentations": []}),
            effects: json!([]),
            records,
            landscape: json!([]),
        }
    }

    /// Writes the folder to a fresh directory named after `name` (unique per process) and
    /// returns its path: the game document and the asset records split into a base
    /// package (one file per element, one folder per asset, as docs/packages.md lays it
    /// out).
    pub fn write(&self, name: &str) -> PathBuf {
        // A folder per call: tests of one process may load the same name at once.
        static WRITES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("canopy-data-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |file: &str, bytes: &[u8]| {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        let json = |value: &Value| value.to_string().into_bytes();
        let base =
            |file: &str, value: &Value| write(&format!("{}/{file}", crate::BASE_DIR), &json(value));
        base(
            "package.json",
            &json!({"format": 1, "id": "base", "kind": "base", "name": "Test", "version": "1.0.0"}),
        );
        let assets = Assets::new(&self.records, &self.landscape);
        assets.write(&base, &write);
        let mut game = self.game.clone();
        assets.rename_rules(&mut game);
        let game = &game;
        for set in game["behaviorSets"].as_array().into_iter().flatten() {
            base(
                &format!("behaviors/{}.json", set["id"].as_str().unwrap()),
                set,
            );
        }
        let mut overrides = game["actions"]["overrides"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for definition in game["definitions"].as_array().into_iter().flatten() {
            let mut d = definition.as_object().unwrap().clone();
            let kind = d
                .remove("kind")
                .and_then(|k| k.as_str().map(str::to_string))
                .unwrap();
            let id = d["id"].as_str().unwrap().to_string();
            let verb = if kind == "building" {
                "build"
            } else {
                "produce"
            };
            if let Some(command) = overrides.remove(&format!("{verb}:{id}")) {
                d.insert("command".into(), command);
            }
            assets.rename_definition(&mut d);
            let folder = match kind.as_str() {
                "unit" => "units",
                "building" => "buildings",
                "item" => "items",
                _ => "resources",
            };
            base(&format!("{folder}/{id}.json"), &Value::Object(d));
        }
        assert!(
            overrides.is_empty(),
            "command overrides without a definition: {overrides:?}"
        );
        let mut commands = game["actions"].as_object().cloned().unwrap_or_default();
        commands.remove("overrides");
        base("rules/commands.json", &Value::Object(commands));
        let mut rules = game["rules"].as_object().cloned().unwrap_or_default();
        for key in ["id", "heroStunDurationPermille"] {
            rules.remove(key);
        }
        let mut split = |key: &str, folder: &str| {
            for (id, value) in rules
                .remove(key)
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default()
            {
                let mut value = value.as_object().unwrap().clone();
                value.insert("id".into(), Value::String(id.clone()));
                base(&format!("{folder}/{id}.json"), &Value::Object(value));
            }
        };
        split("research", "research");
        split("lootPools", "loot-pools");
        split("races", "races");
        for (id, campaign) in rules
            .remove("campaigns")
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
        {
            base(
                &format!("campaigns/{id}.utcc/package.json"),
                &json!({"format": 1, "id": id, "kind": "campaign", "name": campaign["name"], "version": "1.0.0"}),
            );
            base(
                &format!("campaigns/{id}.utcc/campaign.json"),
                &json!({"race": campaign["race"], "steps": campaign.get("steps").cloned().unwrap_or(json!([]))}),
            );
        }
        let mut combat = serde_json::Map::new();
        for key in [
            "armorTypes",
            "damageTypes",
            "damageMultipliers",
            "armorCoefficient",
        ] {
            if let Some(value) = rules.remove(key) {
                combat.insert(key.into(), value);
            }
        }
        base("rules/combat.json", &Value::Object(combat));
        if let Some(experience) = rules.remove("experience") {
            base("rules/experience.json", &experience);
        }
        // Content without races keeps its one starting setup with the game rules.
        base("rules/game.json", &Value::Object(rules));
        let presentations: Vec<&Value> = self.abilities["presentations"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        for ability in self.abilities["abilities"].as_array().into_iter().flatten() {
            let mut a = ability.as_object().unwrap().clone();
            a.remove("schemaVersion");
            if let Some(Value::String(pid)) = a.get("presentation").cloned()
                && let Some(p) = presentations.iter().find(|p| p["id"] == pid.as_str())
            {
                let mut p = p.as_object().unwrap().clone();
                p.remove("schemaVersion");
                p.remove("id");
                a.insert("presentation".into(), Value::Object(p));
            }
            base(
                &format!("abilities/{}.json", a["id"].as_str().unwrap()),
                &Value::Object(a),
            );
        }
        for effect in self.effects.as_array().into_iter().flatten() {
            let mut e = effect.as_object().unwrap().clone();
            e.remove("schemaVersion");
            base(
                &format!("effects/{}.json", e["id"].as_str().unwrap()),
                &Value::Object(e),
            );
        }
        extract_text(&root.join(crate::BASE_DIR));
        root
    }

    pub fn load(&self, name: &str) -> Result<Content, Errors> {
        Content::load(&self.write(name))
    }
}

/// Moves the names and descriptions written into a test package's content files into its
/// text library (`text/en/content.json`, D96), as `tools/text/migrate_content.py` does for
/// shipped packages, so tests keep writing their elements whole.
pub fn extract_text(package: &std::path::Path) {
    use serde_json::Map;
    let mut text = Map::new();
    let read = |path: &std::path::Path| -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let write = |path: &std::path::Path, value: &Value| {
        std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap()
    };
    fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files(&path, out);
            } else if path.extension().is_some_and(|e| e == "json") {
                out.push(path);
            }
        }
    }
    let mut take = |value: &mut Value, key: &str, fields: &[&str]| {
        if let Some(object) = value.as_object_mut() {
            for field in fields {
                if let Some(string) = object.remove(*field) {
                    text.insert(format!("{key}.{field}"), string);
                }
            }
        }
    };
    for folder in [
        "units",
        "buildings",
        "items",
        "resources",
        "abilities",
        "research",
        "races",
    ] {
        let mut found = Vec::new();
        files(&package.join(folder), &mut found);
        for path in found {
            let mut value = read(&path);
            let Some(id) = value.get("id").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            let key = if folder == "races" {
                format!("race.{id}")
            } else {
                id
            };
            take(&mut value, &key, &["name", "description"]);
            write(&path, &value);
        }
    }
    let commands = package.join("rules/commands.json");
    if commands.exists() {
        let mut value = read(&commands);
        for (group, prefix) in [
            ("actions", "command."),
            ("categories", ""),
            ("navigation", "navigation."),
        ] {
            if let Some(Value::Object(entries)) = value.get_mut(group) {
                for (id, entry) in entries.iter_mut() {
                    take(entry, &format!("{prefix}{id}"), &["name", "description"]);
                }
            }
        }
        write(&commands, &value);
    }
    let combat = package.join("rules/combat.json");
    if combat.exists() {
        let mut value = read(&combat);
        for (field, prefix) in [("armorTypes", "armor"), ("damageTypes", "damage")] {
            if let Some(Value::Object(types)) = value.get_mut(field) {
                for (id, entry) in types.iter_mut() {
                    take(entry, &format!("{prefix}.{id}"), &["name"]);
                }
            }
        }
        write(&combat, &value);
    }
    if !text.is_empty() {
        let dir = package.join(crate::text::folder(crate::text::SOURCE_LANGUAGE));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("content.json");
        let mut existing = if path.exists() {
            read(&path).as_object().cloned().unwrap_or_default()
        } else {
            Map::new()
        };
        existing.extend(text);
        write(&path, &Value::Object(existing));
    }
}

/// A published asset record.
pub fn record(id: &str, outputs: Vec<Value>, render: Value, scenery: Value) -> Value {
    json!({
        "id": id, "name": id, "kind": "model", "tags": [], "revision": 1, "profile": "test",
        "outputs": outputs, "render": render, "scenery": scenery,
    })
}

pub fn output(role: &str, path: &str) -> Value {
    json!({"role": role, "path": path, "sha256": "0".repeat(64), "bytes": 1})
}

/// An image asset whose render id equals its record id.
pub fn image(id: &str) -> Value {
    let path = format!("assets/library/{id}/image.png");
    record(
        id,
        vec![output("image", &path)],
        json!([{"id": id, "image": path}]),
        json!([]),
    )
}

/// A model asset whose render id equals its record id.
pub fn model(id: &str) -> Value {
    let path = format!("assets/library/{id}/geometry.glb");
    record(
        id,
        vec![output("model", &path)],
        json!([{"id": id, "file": path}]),
        json!([]),
    )
}

/// A model that is also placeable scenery (as resources require).
pub fn scenery_model(id: &str) -> Value {
    let path = format!("assets/library/{id}/geometry.glb");
    let scenery = format!("{id}-prop");
    record(
        id,
        vec![output("model", &path)],
        json!([{"id": id, "file": path, "sceneryAsset": scenery}]),
        json!([{"id": scenery, "name": "Prop", "category": "resource", "type": "prop",
            "file": format!("library/{id}/geometry.glb")}]),
    )
}

/// Old-shape asset records converted as `tools/packages/migrate.py` converts shipped data:
/// render ids become asset (or scenery) ids, render presentation moves to the definitions.
struct Assets<'a> {
    records: &'a [Value],
    landscape: Vec<Value>,
    /// Render id → (new id, render entry, the model's display entry).
    renders: std::collections::BTreeMap<String, (String, Value, Value)>,
}

impl<'a> Assets<'a> {
    fn new(records: &'a [Value], landscape: &Value) -> Self {
        let mut renders = std::collections::BTreeMap::new();
        for record in records {
            let id = record["id"].as_str().unwrap();
            let has_glb = outputs(record).any(|(_, f)| f.ends_with(".glb"));
            let entries: Vec<&Value> = record["render"].as_array().into_iter().flatten().collect();
            let display = entries
                .iter()
                .find(|e| e.get("file").is_some() && e.get("sceneryAsset").is_none())
                .or_else(|| entries.iter().find(|e| e.get("file").is_some()))
                .map(|e| (*e).clone())
                .unwrap_or(Value::Null);
            for e in &entries {
                let new = if let Some(prop) = e["sceneryAsset"].as_str() {
                    format!("scenery.{prop}")
                } else if e.get("image").is_some() && has_glb {
                    format!("{id}.portrait")
                } else {
                    id.to_string()
                };
                renders.insert(
                    e["id"].as_str().unwrap().to_string(),
                    (new, (*e).clone(), display.clone()),
                );
            }
        }
        Self {
            records,
            landscape: landscape.as_array().cloned().unwrap_or_default(),
            renders,
        }
    }

    fn id(&self, old: &Value) -> Value {
        match old.as_str().and_then(|o| self.renders.get(o)) {
            Some((new, _, _)) => Value::String(new.clone()),
            None => old.clone(),
        }
    }

    fn rename_definition(&self, d: &mut serde_json::Map<String, Value>) {
        let Some(old) = d.remove("asset") else {
            return;
        };
        d.insert("model".into(), self.id(&old));
        if let Some(icon) = d.get("icon").cloned() {
            d.insert("icon".into(), self.id(&icon));
        }
        if let Some((_, entry, display)) = old.as_str().and_then(|o| self.renders.get(o)) {
            let mut presentation = serde_json::Map::new();
            for key in [
                "projectile",
                "projectileSocket",
                "impactEffect",
                "castContact",
                "cameraAnchor",
            ] {
                if let Some(v) = entry.get(key) {
                    presentation.insert(key.into(), v.clone());
                }
            }
            for key in ["healthHeight", "character"] {
                if let Some(v) = entry.get(key)
                    && entry.get("sceneryAsset").is_none()
                    && display.get(key) != Some(v)
                {
                    presentation.insert(key.into(), v.clone());
                }
            }
            if !presentation.is_empty() {
                d.insert("presentation".into(), Value::Object(presentation));
            }
        }
        if let Some(visuals) = d.get_mut("resourceVisuals") {
            if let Some(variants) = visuals["variants"].as_array_mut() {
                for v in variants.iter_mut() {
                    *v = self.id(v);
                }
            }
            if let Some(stages) = visuals["stages"].as_array_mut() {
                for stage in stages {
                    stage["asset"] = self.id(&stage["asset"]);
                }
            }
        }
    }

    /// Icons and scaffolds the rules name.
    fn rename_rules(&self, game: &mut Value) {
        let rules = &mut game["rules"];
        if rules.get("supplyIcon").is_some() {
            rules["supplyIcon"] = self.id(&rules["supplyIcon"]);
        }
        for key in ["research", "armorTypes"] {
            for v in rules[key]
                .as_object_mut()
                .into_iter()
                .flat_map(|m| m.values_mut())
            {
                v["icon"] = self.id(&v["icon"]);
            }
        }
        for race in rules["races"]
            .as_object_mut()
            .into_iter()
            .flat_map(|m| m.values_mut())
        {
            if let Some(c) = race.get_mut("construction") {
                c["scaffold"] = self.id(&c["scaffold"]);
            }
        }
        let actions = &mut game["actions"];
        for key in ["actions", "categories"] {
            for v in actions[key]
                .as_object_mut()
                .into_iter()
                .flat_map(|m| m.values_mut())
            {
                v["icon"] = self.id(&v["icon"]);
            }
        }
        for v in actions["navigation"]
            .as_object_mut()
            .into_iter()
            .flat_map(|m| m.values_mut())
        {
            v["icon"] = self.id(&v["icon"]);
        }
    }

    fn write(&self, base: &dyn Fn(&str, &Value), write: &dyn Fn(&str, &[u8])) {
        let landscape = |id: &str| self.landscape.iter().find(|l| l["id"] == id);
        for record in self.records {
            let id = record["id"].as_str().unwrap();
            let files: Vec<(String, String)> = outputs(record).collect();
            let has_glb = files.iter().any(|(_, f)| f.ends_with(".glb"));
            let portrait = record["render"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|e| e.get("image").is_some())
                && has_glb;
            let mut resources = Vec::new();
            for (_, file) in &files {
                let (stem, format) = file.rsplit_once('.').unwrap();
                let (role, index) = match stem.rsplit_once('_') {
                    Some((r, i)) if i.parse::<u32>().is_ok() => {
                        (r.to_string(), i.parse::<u32>().unwrap())
                    }
                    _ => (stem.to_string(), 1),
                };
                if portrait && role == "image" {
                    write(
                        &format!("{}/assets/{id}.portrait/{file}", crate::BASE_DIR),
                        &[0],
                    );
                    base(
                        &format!("assets/{id}.portrait/asset.json"),
                        &json!({"id": format!("{id}.portrait"), "name": id, "kind": "icon",
                            "resources": [{"role": "image", "index": 1, "format": format}]}),
                    );
                    continue;
                }
                write(&format!("{}/assets/{id}/{file}", crate::BASE_DIR), &[0]);
                resources.push(json!({"role": role, "index": index, "format": format}));
            }
            let entry = landscape(id);
            let kind =
                entry
                    .and_then(|e| e["kind"].as_str())
                    .unwrap_or(match record["kind"].as_str() {
                        Some("audio") => "audio",
                        _ if has_glb => "prop",
                        _ if files.iter().any(|(_, f)| f.starts_with("image")) => "icon",
                        _ => "texture",
                    });
            let mut asset =
                json!({"id": id, "name": record["name"], "kind": kind, "resources": resources});
            if has_glb {
                asset["transform"] =
                    json!({"scale": 1, "pivot": [0, 0, 0], "up": "Y", "forward": "+Z"});
            }
            if let Some((_, _, display)) = record["render"]
                .as_array()
                .and_then(|r| r.first())
                .and_then(|e| self.renders.get(e["id"].as_str().unwrap()))
            {
                let mut d = serde_json::Map::new();
                for key in ["scale", "character", "healthHeight"] {
                    if let Some(v) = display.get(key) {
                        d.insert(key.into(), v.clone());
                    }
                }
                if !d.is_empty() && has_glb {
                    asset["display"] = Value::Object(d);
                }
            }
            if let Some(terrain) = entry.and_then(|e| e.get("terrain")) {
                asset["terrain"] = terrain.clone();
            }
            base(&format!("assets/{id}/asset.json"), &asset);
            for scenery in record["scenery"].as_array().into_iter().flatten() {
                let prop = scenery["id"].as_str().unwrap();
                let mut s = scenery.as_object().unwrap().clone();
                s.remove("file");
                s.insert("id".into(), json!(format!("scenery.{prop}")));
                s.insert("model".into(), json!(id));
                if let Some(h) = entry
                    .filter(|e| e["scenery"] == prop)
                    .and_then(|e| e.get("harvesting"))
                {
                    s.insert("harvesting".into(), json!({"definition": h["definition"]}));
                }
                base(&format!("scenery/{prop}.json"), &Value::Object(s));
            }
        }
        for entry in &self.landscape {
            let id = entry["id"].as_str().unwrap();
            if let Some(water) = entry.get("water") {
                base(
                    &format!("landscape/water/{id}.json"),
                    &json!({"id": id, "name": entry["name"], "water": water}),
                );
            }
        }
    }
}

/// (role, file name) of each output of an old-shape record.
fn outputs(record: &Value) -> impl Iterator<Item = (String, String)> + '_ {
    record["outputs"].as_array().into_iter().flatten().map(|o| {
        let path = o["path"].as_str().unwrap();
        (
            o["role"].as_str().unwrap_or_default().to_string(),
            path.rsplit('/').next().unwrap().to_string(),
        )
    })
}
