//! Asset folders, scenery and landscape files of packages, and the registries built from
//! them (docs/packages.md). Test-owned ids only.

use canopy_content::{
    assets::{Files, package::index},
    package::Package,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Writes a package `id` of `kind` with `files` (path, JSON or raw bytes) under `root`.
fn package(root: &Path, id: &str, kind: &str, files: &[(&str, Value)]) -> Package {
    let dir = root.join(id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.json"),
        json!({"format": 1, "id": id, "kind": kind, "name": id, "version": "1.0.0"}).to_string(),
    )
    .unwrap();
    for (path, value) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let bytes = match value {
            Value::String(text) => text.clone().into_bytes(),
            other => other.to_string().into_bytes(),
        };
        std::fs::write(file, bytes).unwrap();
    }
    Package::open(&dir).unwrap()
}

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("canopy-assets-{name}-{}", std::process::id()))
}

fn pine() -> Value {
    json!({"id": "asset.pine", "name": "Pine", "kind": "tree",
        "resources": [{"role": "geometry", "index": 1, "format": "glb"},
                      {"role": "source", "index": 1, "format": "blend"}],
        "transform": {"scale": 1, "pivot": [0, 0, 0], "up": "Y", "forward": "+Z"},
        "capabilities": {"vegetationClearance": 1.5},
        "display": {"scale": 2, "healthHeight": 3}})
}

#[test]
fn asset_folders_build_the_registries() {
    let root = root("build");
    let base = package(
        &root,
        "base",
        "base",
        &[
            ("assets/forest/pine/asset.json", pine()),
            ("assets/forest/pine/geometry.glb", json!("glb")),
            ("assets/forest/pine/source.blend", json!("blend")),
            (
                "scenery/pine.json",
                json!({"id": "scenery.pine", "name": "Pine", "category": "foliage",
                "type": "prop", "model": "asset.pine", "harvesting": {"definition": "resource.tree"}}),
            ),
            (
                "landscape/water/clear.json",
                json!({"id": "water.clear", "name": "Clear", "water": {
                "shallowColor": "#ffffff", "deepColor": "#000000", "clarity": 1, "rippleScale": 1,
                "rippleStrength": 1, "foamStrength": 1, "reflectionStrength": 1, "causticStrength": 1,
                "cloudStrength": 1, "flowSpeed": 1}}),
            ),
        ],
    );
    let mut errors = Vec::new();
    let built = index(&[base], Files::Check, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let record = &built.manifest.records[0];
    // Runtime files are named by asset id; working sources are not cooked.
    assert_eq!(record.outputs.len(), 1);
    assert_eq!(record.outputs[0].path, "asset.pine/geometry.glb");
    assert!(built.sources["asset.pine/geometry.glb"].ends_with("assets/forest/pine/geometry.glb"));
    let ids: Vec<&str> = record.render.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["asset.pine", "scenery.pine"]);
    assert_eq!(record.render[0].scale, Some(2.0));
    assert_eq!(
        record.render[1].scenery_asset.as_deref(),
        Some("scenery.pine")
    );
    assert_eq!(record.scenery[0].file, "asset.pine/geometry.glb");
    assert_eq!(built.models[0].geometry, ["asset.pine/geometry.glb"]);
    let entry = |id: &str| {
        built
            .catalogue
            .iter()
            .find(|c| c.id.as_str() == id)
            .unwrap()
    };
    assert_eq!(
        entry("scenery.pine").harvesting.as_ref().unwrap().asset,
        "scenery.pine"
    );
    assert_eq!(entry("asset.pine").clearance, 1.5);
    assert!(entry("water.clear").water.is_some());
}

#[test]
fn missing_files_and_unknown_fields_are_reported() {
    let root = root("problems");
    let mut wrong = pine();
    wrong["id"] = json!("asset.odd");
    wrong["colour"] = json!("green");
    let base = package(
        &root,
        "base",
        "base",
        &[
            ("assets/pine/asset.json", pine()),
            ("assets/odd/asset.json", wrong),
            (
                "scenery/rock.json",
                json!({"id": "scenery.rock", "name": "Rock", "category": "landmark",
                "type": "prop", "model": "asset.nothing"}),
            ),
        ],
    );
    let mut errors = Vec::new();
    index(std::slice::from_ref(&base), Files::Check, &mut errors);
    let text: Vec<String> = errors.iter().map(ToString::to_string).collect();
    let has = |part: &str| text.iter().any(|t| t.contains(part));
    assert!(
        has("base/assets/pine/asset.json · resources: missing geometry.glb"),
        "{text:#?}"
    );
    assert!(
        has("base/assets/odd/asset.json · colour: unknown field"),
        "{text:#?}"
    );
    assert!(
        has("base/scenery/rock.json · model: asset.nothing is not a model asset"),
        "{text:#?}"
    );
    // A packaged game has no asset files beside the pack.
    let mut errors = Vec::new();
    index(&[base], Files::Skip, &mut errors);
    assert!(!errors.iter().any(|e| e.message.starts_with("missing")));
}

#[test]
fn a_later_package_replaces_the_files_it_has() {
    let root = root("layers");
    let base = package(
        &root,
        "base",
        "base",
        &[
            ("assets/pine/asset.json", pine()),
            ("assets/pine/geometry.glb", json!("old")),
            ("assets/pine/source.blend", json!("blend")),
        ],
    );
    let map = package(
        &root,
        "isle",
        "map",
        &[
            (
                "assets/trees/pine/asset.json",
                json!({"id": "asset.pine", "display": {"scale": 3}}),
            ),
            ("assets/trees/pine/geometry.glb", json!("new")),
        ],
    );
    let mut errors = Vec::new();
    let built = index(&[base, map], Files::Check, &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        built.sources["asset.pine/geometry.glb"].ends_with("isle/assets/trees/pine/geometry.glb")
    );
    assert_eq!(built.manifest.records[0].render[0].scale, Some(3.0));
}
