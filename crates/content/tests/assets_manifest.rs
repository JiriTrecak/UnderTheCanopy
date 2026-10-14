//! `assets/manifest.json`: strict records and the publication rules.

use canopy_content::{Error, assets};
use serde_json::{Value, json};
use std::path::PathBuf;

const SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn output(path: &str) -> Value {
    json!({"role": "model", "path": path, "sha256": SHA, "bytes": 3})
}

fn record(id: &str, outputs: Vec<Value>, render: Value, scenery: Value) -> Value {
    json!({
        "id": id, "name": "Fixture", "kind": "model", "tags": ["test"], "revision": 2,
        "profile": "prop", "outputs": outputs, "render": render, "scenery": scenery,
    })
}

/// A model with one render binding and one scenery entry, both on its own geometry.
fn model(id: &str) -> Value {
    let file = format!("assets/library/{id}/geometry.glb");
    record(
        id,
        vec![output(&file)],
        json!([{"id": format!("asset.render.{id}"), "file": file, "scale": 1.5}]),
        json!([{
            "id": format!("{id}-prop"), "name": "Prop", "category": "foliage", "type": "prop",
            "file": format!("library/{id}/geometry.glb"),
            "blockers": [{"width": 1.0, "depth": 2.0, "shape": "ellipse", "yaw": 0.5}],
        }]),
    )
}

/// Writes `records` as the manifest of a fresh data folder and creates every output file.
fn load(name: &str, records: Vec<Value>) -> (Option<assets::Manifest>, Vec<Error>) {
    let root: PathBuf =
        std::env::temp_dir().join(format!("canopy-manifest-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("assets")).unwrap();
    for record in &records {
        for output in record["outputs"].as_array().into_iter().flatten() {
            let path = output["path"].as_str().unwrap();
            if path.starts_with("assets/") && !path.contains("missing") {
                let file = root.join(path);
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, b"abc").unwrap();
            }
        }
    }
    let manifest = json!({"version": 1, "records": records});
    std::fs::write(root.join("assets/manifest.json"), manifest.to_string()).unwrap();
    let mut errors = Vec::new();
    let manifest = assets::load(&root, &mut errors);
    (manifest, errors)
}

fn messages(errors: &[Error]) -> Vec<&str> {
    errors.iter().map(|e| e.message.as_str()).collect()
}

#[test]
fn published_records_load() {
    let (manifest, errors) = load("valid", vec![model("tree.a"), model("tree.b")]);
    assert_eq!(messages(&errors), Vec::<&str>::new());
    let manifest = manifest.unwrap();
    let render = assets::render_assets(&manifest);
    assert_eq!(render.len(), 2);
    assert_eq!(render[0].scale, Some(1.5));
    let scenery: Vec<_> = assets::scenery(&manifest).map(|s| s.id.as_str()).collect();
    assert_eq!(scenery, ["tree.a-prop", "tree.b-prop"]);
    assert_eq!(
        serde_json::to_value(&manifest).unwrap()["records"][0],
        model("tree.a"),
        "round trip keeps the authored form"
    );
}

#[test]
fn unknown_fields_and_versions_are_rejected() {
    let mut extra = model("tree.a");
    extra["status"] = json!("published");
    let (manifest, errors) = load("record-field", vec![extra]);
    assert!(manifest.is_none());
    assert!(errors[0].message.contains("status"), "{errors:?}");

    let mut extra = model("tree.a");
    extra["scenery"][0]["blockers"][0]["height"] = json!(1);
    let (manifest, errors) = load("scenery-field", vec![extra]);
    assert!(manifest.is_none());
    assert!(errors[0].location.contains("blockers"), "{errors:?}");

    let mut kind = model("tree.a");
    kind["kind"] = json!("unit");
    assert!(load("kind", vec![kind]).0.is_none());
}

#[test]
fn ids_and_output_paths_are_unique() {
    let mut twin = model("tree.a");
    twin["outputs"] = json!([output("assets/library/tree.A/geometry.glb")]);
    twin["render"] = json!([]);
    twin["scenery"] = json!([]);
    let (_, errors) = load("unique", vec![model("tree.a"), model("tree.a"), twin]);
    let messages = messages(&errors);
    assert!(messages.contains(&"duplicate asset tree.a"), "{messages:?}");
    assert!(
        messages.contains(&"duplicate scenery tree.a-prop"),
        "{messages:?}"
    );
    assert!(
        messages.contains(&"duplicate output assets/library/tree.A/geometry.glb"),
        "outputs differ only in case: {messages:?}"
    );
}

#[test]
fn outputs_must_exist_below_assets() {
    let mut record = model("tree.a");
    record["outputs"] = json!([
        output("assets/library/tree.a/geometry.glb"),
        output("assets/library/tree.a/missing.glb"),
        output("content/game.json"),
        output("assets/../secret.glb"),
    ]);
    let (_, errors) = load("paths", vec![record]);
    let messages = messages(&errors);
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("missing output"));
    assert!(messages[1].contains("below assets/"));
    assert!(messages[2].contains("below assets/"));
}

#[test]
fn render_files_are_published_outputs() {
    // Another record's output is fine: publication resolves bindings by asset.
    let mut shared = model("tree.b");
    shared["render"][0]["image"] = json!("assets/library/tree.a/geometry.glb");
    let (_, errors) = load("render-shared", vec![model("tree.a"), shared]);
    assert_eq!(messages(&errors), Vec::<&str>::new());

    let mut stray = model("tree.a");
    stray["render"][0]["harvestAnimation"] = json!("assets/library/elsewhere/fall.glb");
    let (_, errors) = load("render-stray", vec![stray]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.contains("not a published output"));
}

#[test]
fn scenery_files_are_outputs_of_their_record() {
    let mut other = model("tree.b");
    other["scenery"][0]["file"] = json!("library/tree.a/geometry.glb");
    let (_, errors) = load("scenery-file", vec![model("tree.a"), other]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.contains("not an output of its record"));
}
