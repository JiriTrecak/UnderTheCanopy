//! A complete, valid data folder for integration tests, written from test-owned JSON.

#![allow(dead_code)]

use canopy_content::{Content, Errors};
use serde_json::{Value, json};
use std::path::PathBuf;

/// The files of a data folder. Libraries start empty; tests fill in what they need.
pub struct DataFolder {
    pub game: Value,
    /// `library` of `content/abilities/published.json`.
    pub abilities: Value,
    /// `effects` of `content/effects/published.json`.
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

    /// Writes the folder below the test target directory and returns its path.
    pub fn write(&self, name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("data-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let write = |file: &str, bytes: &[u8]| {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        let json = |value: Value| value.to_string().into_bytes();
        write(canopy_content::GAME_FILE, &json(self.game.clone()));
        write(
            canopy_content::ABILITIES_FILE,
            &json(json!({"schemaVersion": 1, "abi": "abilities-1", "library": self.abilities})),
        );
        write(
            canopy_content::EFFECTS_FILE,
            &json(json!({"schemaVersion": 1, "effects": self.effects})),
        );
        write(
            canopy_content::MANIFEST_FILE,
            &json(json!({"version": 1, "records": self.records})),
        );
        write(
            "assets/authoring/catalogue.json",
            &json(self.landscape.clone()),
        );
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
