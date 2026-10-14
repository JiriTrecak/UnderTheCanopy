//! The model index, map previews and texture data files.

use canopy_content::{
    Error,
    assets::{
        Manifest,
        models::{self, PublishedModel},
        previews, textures,
    },
};
use serde_json::{Value, json};
use std::path::PathBuf;

/// A fresh data folder holding `files`.
fn folder(name: &str, files: &[(&str, String)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("canopy-indexes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, text) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    root
}

fn model() -> Value {
    json!({
        "id": "model.test",
        "transform": {"scale": 1.0, "pivot": [0.0, 0.5, 0.0], "up": "Y", "forward": "-Z"},
        "capabilities": {
            "dimensions": {"radius": 0.5, "height": 1.0, "formationSpacing": 1.2},
            "teamColor": {"mode": "mask", "slots": ["Body"], "mask": {"role": "team_mask", "index": 1}},
            "animations": [{"semantic": "walk", "clip": "Walk", "loop": true, "nominalSpeed": 1.5,
                "events": [{"name": "step", "time": 0.25}]}],
            "sockets": [{"name": "hand", "node": "Hand.R", "offset": [0.0, 0.1, 0.0]}],
            "blocker": {"shape": "box", "size": [1.0, 2.0], "offset": [0.0, 0.0]},
            "groundContact": {"mode": "terrain", "ambientRadius": 2.0},
            "canopy": true,
            "harvesting": {"replacement": "model.stump"},
        },
        "groundContact": "terrain",
        "canopy": true,
        "geometry": ["assets/library/model.test/geometry.glb"],
        "scenery": ["test-prop"],
    })
}

fn manifest() -> Manifest {
    serde_json::from_value(json!({"version": 1, "records": [{
        "id": "model.test", "name": "Test", "kind": "model", "tags": [], "revision": 1,
        "profile": "prop", "render": [],
        "outputs": [{"role": "model", "path": "assets/library/model.test/geometry.glb",
            "sha256": "0".repeat(64), "bytes": 1}],
        "scenery": [{"id": "test-prop", "name": "Test", "category": "other", "type": "prop",
            "file": "library/model.test/geometry.glb"}],
    }]}))
    .unwrap()
}

fn check(raw: Value) -> Vec<Error> {
    let model: PublishedModel = serde_json::from_value(raw).unwrap();
    let mut errors = Vec::new();
    models::validate(&[model], &manifest(), &mut errors);
    errors
}

#[test]
fn models_round_trip_and_resolve() {
    let parsed: PublishedModel = serde_json::from_value(model()).unwrap();
    assert!(
        parsed
            .capabilities
            .as_ref()
            .unwrap()
            .animations
            .as_ref()
            .unwrap()[0]
            .looping
    );
    assert_eq!(serde_json::to_value(&parsed).unwrap(), model());
    assert_eq!(check(model()), vec![]);
}

#[test]
fn models_are_strict() {
    let parse = |raw: Value| serde_json::from_value::<PublishedModel>(raw).is_err();
    let mut flag = model();
    flag["canopy"] = json!(false);
    assert!(parse(flag), "canopy is the literal true");
    let mut events = model();
    events["capabilities"]["animations"][0]
        .as_object_mut()
        .unwrap()
        .remove("events");
    assert!(parse(events), "zod defaults are written out");
    let mut extra = model();
    extra["capabilities"]["lod"] = json!(2);
    assert!(parse(extra));
}

#[test]
fn model_index_matches_the_manifest() {
    let mut copy = model();
    copy["groundContact"] = json!("pivot");
    assert!(check(copy)[0].message.contains("groundContact"));
    let mut stray = model();
    stray["geometry"] = json!(["assets/library/model.test/other.glb"]);
    stray["scenery"] = json!(["other-prop"]);
    assert_eq!(check(stray).len(), 2);
    let mut unpublished = model();
    unpublished["id"] = json!("model.other");
    assert_eq!(check(unpublished)[0].location, "model.other");
}

fn overview() -> Value {
    json!({
        "name": "Fixture Isles", "size": 512, "description": "Two islands.",
        "starts": [
            {"player": 1, "x": 10.5, "z": 20.0, "setup": "setup.test", "mainFort": "start.1/fort"},
            {"player": 2, "x": 400.0, "z": 300.5, "rotation": 180, "setup": "setup.test",
                "mainFort": "start.2/fort"},
        ],
        "camps": 3, "sandbox": false, "playable": null, "sourceHash": "c6a4d0cc",
        "mission": {"campaign": "test", "order": 1, "title": "First",
            "presentation": {"chapter": "One", "tips": ["Look up"]}},
        "image": "fixture.webp", "custom": false, "generationHash": "ab",
    })
}

fn previews(
    name: &str,
    entries: Value,
) -> (
    Option<std::collections::BTreeMap<String, previews::MapOverview>>,
    Vec<Error>,
) {
    let root = folder(
        name,
        &[
            (previews::MAP_PREVIEWS_FILE, entries.to_string()),
            ("assets/maps/previews/fixture.webp", String::new()),
        ],
    );
    let mut errors = Vec::new();
    (previews::load(&root, &mut errors), errors)
}

#[test]
fn map_previews_load() {
    let (loaded, errors) = previews("valid", json!({"fixture": overview()}));
    assert_eq!(errors, vec![]);
    let loaded = loaded.unwrap();
    assert_eq!(loaded["fixture"].playable, None);
    assert_eq!(
        serde_json::to_value(&loaded["fixture"]).unwrap(),
        overview(),
        "playable stays null"
    );
}

#[test]
fn map_previews_are_checked() {
    let mut missing = overview();
    missing.as_object_mut().unwrap().remove("playable");
    assert!(
        previews("playable", json!({"fixture": missing}))
            .0
            .is_none(),
        "playable is required"
    );

    let mut broken = overview();
    broken["size"] = json!(300);
    broken["image"] = json!("gone.webp");
    broken["starts"][1]["player"] = json!(1);
    let (_, errors) = previews("broken", json!({"fixture": broken}));
    let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages.len(), 3, "{messages:?}");
}

#[test]
fn texture_data_is_consistent() {
    let volume = |size: u32, rgba: &str| json!({"size": size, "rgba": rgba});
    let grades = |day: Value| {
        json!({"day": day, "day_to_night": volume(1, "AAAAAA=="), "night": volume(1, "AAAAAA=="),
            "night_to_day": volume(1, "AAAAAA=="), "winter_day": volume(1, "AAAAAA==")})
    };
    let root = folder(
        "textures",
        &[
            ("a/grades.json", grades(volume(1, "AAAAAA==")).to_string()),
            ("a/short.json", grades(volume(2, "AAAAAA==")).to_string()),
            (
                "a/cube.json",
                json!({"size": 2, "levels": 2, "decodedBytes": 120,
                "faceOrder": ["+X", "-X", "+Y", "-Y", "+Z", "-Z"], "method": "test"})
                .to_string(),
            ),
            (
                "a/table.json",
                json!({"width": 2, "height": 2, "decodedBytes": 15,
                "method": "test", "samples": 8})
                .to_string(),
            ),
        ],
    );
    let mut errors = Vec::new();
    assert!(textures::load_daytime_grades(&root, "a/grades.json", &mut errors).is_some());
    assert!(textures::load_cube_metadata(&root, "a/cube.json", &mut errors).is_some());
    assert_eq!(errors, vec![]);
    textures::load_daytime_grades(&root, "a/short.json", &mut errors);
    textures::load_table_metadata(&root, "a/table.json", &mut errors);
    let locations: Vec<_> = errors.iter().map(|e| e.location.as_str()).collect();
    assert_eq!(locations, ["day", "decodedBytes"]);
}
