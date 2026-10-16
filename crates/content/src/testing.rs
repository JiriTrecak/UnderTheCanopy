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
    /// Records of `assets/manifest.json`; every output file is created on disk.
    pub records: Vec<Value>,
    /// `assets/authoring/catalogue.json`.
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
    /// returns its path: the game document split into a base package (one file per
    /// element, as docs/packages.md lays it out), and the asset registry beside it.
    pub fn write(&self, name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("canopy-data-{name}-{}", std::process::id()));
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
        let game = &self.game;
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
                &json!({"race": campaign["race"]}),
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
        write(
            crate::MANIFEST_FILE,
            &json(&json!({"version": 1, "records": self.records})),
        );
        write("assets/authoring/catalogue.json", &json(&self.landscape));
        for record in &self.records {
            for output in record["outputs"].as_array().into_iter().flatten() {
                write(output["path"].as_str().unwrap(), &[0]);
            }
        }
        root
    }

    pub fn load(&self, name: &str) -> Result<Content, Errors> {
        Content::load(&self.write(name))
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
