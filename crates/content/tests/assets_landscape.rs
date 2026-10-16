//! The landscape catalogue: water profiles, ground textures, placeable models and the rules
//! tying them to the asset records.

use canopy_content::{
    Error,
    assets::{
        Manifest,
        landscape::{self, LandscapeAsset},
    },
    game::Definition,
};
use serde_json::{Value, json};

fn with(mut base: Value, fields: Value) -> Value {
    for (key, value) in fields.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

fn asset(id: &str, kind: &str, fields: Value) -> Value {
    with(
        json!({"id": id, "name": id, "kind": kind, "scale": 1.0, "clearance": 0.0}),
        fields,
    )
}

fn ground_texture() -> Value {
    let image = |index: u32| json!({"role": "albedo", "index": index});
    json!({
        "tileWorldSize": 8.0, "projection": {"type": "cliff", "verticalWorldSize": 4.0,
            "startSlope": 0.5, "fullSlope": 1.0},
        "reflectance": 1.0, "shading": "painted", "sourceIds": ["Ldrt"],
        "albedo": image(1), "normal": {"role": "normal", "index": 1},
        "occlusion": {"role": "occlusion", "index": 1}, "roughness": {"role": "roughness", "index": 1},
        "metalness": {"role": "metalness", "index": 1}, "thumbnail": {"role": "image", "index": 1},
        "runtime": {"albedoRoughness": {"role": "data", "index": 1},
            "normalOpacity": {"role": "data", "index": 2}, "occlusionMetalness": {"role": "data", "index": 3}},
        "atlas": {"columns": 1, "rows": 1, "fullTiles": [0], "corners": [null, 0, null, null,
            null, null, null, null, null, null, null, null, null, null, null, 0]},
    })
}

/// Records: models with scenery, a ground texture and a water profile.
fn catalogue() -> Vec<Value> {
    let model = |id: &str| asset(id, "foliage", json!({"scenery": format!("{id}-prop")}));
    let mut assets: Vec<Value> = ["rock", "reed", "tree", "flower"].map(model).into();
    assets[2]["harvesting"] =
        json!({"definition": "resource.test.tree", "asset": "asset.test.tree"});
    assets.push(asset(
        "ground.test",
        "terrain-material",
        json!({
            "terrain": ground_texture(),
            "terrainImages": {"albedo": "assets/library/ground.test/albedo.png"},
        }),
    ));
    assets.push(asset(
        "water.test",
        "water-profile",
        json!({"water": {
        "shallowColor": "#336655", "deepColor": "#112233", "clarity": 2.0, "rippleScale": 0.1,
        "rippleStrength": 0.1, "foamStrength": 0.5, "reflectionStrength": 0.5,
        "causticStrength": 0.5, "cloudStrength": 0.1, "flowSpeed": 1.0,
        "waves": {"height": 0.3, "length": 12.0, "speed": 0.6, "depthStart": 0.4, "depthEnd": 2.5,
            "shallowStrength": 0.02, "shoreWidth": 0.6, "crestStrength": 0.2}}}),
    ));
    assets
}

fn manifest(assets: &[Value]) -> Manifest {
    let records: Vec<Value> = assets
        .iter()
        .map(|a| {
            let id = a["id"].as_str().unwrap();
            let path = format!("assets/library/{id}/albedo.png");
            let scenery: Vec<Value> = a.get("scenery").map_or(vec![], |s| {
                vec![json!({"id": s, "name": id, "category": "foliage", "type": "prop",
                    "file": format!("library/{id}/albedo.png")})]
            });
            json!({"id": id, "name": id, "kind": "data", "tags": [], "revision": 1,
                "profile": "data", "outputs": [{"role": "image", "path": path,
                    "sha256": "0".repeat(64), "bytes": 1}],
                "render": if id == "tree" { json!([{"id": "asset.test.tree", "file": path}]) } else { json!([]) },
                "scenery": scenery})
        })
        .collect();
    serde_json::from_value(json!({"version": 1, "records": records})).unwrap()
}

fn resource() -> Vec<Definition> {
    vec![
        serde_json::from_value(json!({"id": "resource.test.tree", "kind": "resource",
            "name": "Tree", "description": "", "model": "asset.test.tree",
            "icon": "asset.test.icon", "behaviors": {}}))
        .unwrap(),
    ]
}

fn validate(assets: &[Value], manifest: &Manifest) -> Vec<Error> {
    let assets: Vec<LandscapeAsset> = serde_json::from_value(Value::from(assets.to_vec())).unwrap();
    let mut errors = Vec::new();
    landscape::validate(&assets, manifest, &resource(), &mut errors);
    errors
}

#[test]
fn a_consistent_catalogue_passes() {
    let assets = catalogue();
    let errors = validate(&assets, &manifest(&assets));
    assert_eq!(errors, vec![]);
}

#[test]
fn references_must_resolve() {
    let assets = catalogue();
    let published = manifest(&assets);
    let broken = |index: usize, fields: Value| {
        let mut assets = assets.clone();
        assets[index] = with(assets[index].clone(), fields);
        let errors = validate(&assets, &published);
        assert_eq!(errors.len(), 1, "{errors:?}");
        errors[0].message.clone()
    };
    assert!(
        broken(
            2,
            json!({"harvesting": {"definition": "unit.test.tree", "asset": "asset.test.tree"}})
        )
        .contains("resource definition")
    );
    assert!(
        broken(
            2,
            json!({"harvesting": {"definition": "resource.test.tree", "asset": ""}})
        )
        .contains("render asset")
    );
    assert!(
        broken(
            4,
            json!({"terrainImages": {"albedo": "assets/library/x/albedo.png"}})
        )
        .contains("not a published output")
    );
    assert!(broken(0, json!({"scenery": "elsewhere"})).contains("unknown scenery"));
}
