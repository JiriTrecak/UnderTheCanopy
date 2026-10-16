//! `assets/authoring/catalogue.json`: landscape recipes, water profiles, ground textures and
//! the rules tying them to published records.

use canopy_content::{
    Error,
    assets::{
        Manifest,
        landscape::{self, LandscapeAsset, LandscapeRecipe},
    },
    game::Definition,
};
use serde_json::{Value, json};

fn scatter(species: &str) -> Value {
    json!({
        "species": [{"asset": species, "weight": 2.0}], "spacing": 1.5, "probability": 0.5,
        "jitter": 0.25, "scaleMin": 0.5, "scaleMax": 1.5, "maxSlope": 1.0, "waterClearance": 0.5,
        "objectClearance": 0.0, "edgeFade": 2.0, "riverBank": {"min": 0.5, "max": 3.0},
        "patchiness": {"scale": 4.0, "strength": 0.75},
    })
}

fn with(mut base: Value, fields: Value) -> Value {
    for (key, value) in fields.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

fn recipes() -> Vec<Value> {
    vec![
        json!({"type": "terrain", "operation": "raise", "height": 4.0, "falloff": 8.0,
            "surface": "mountain", "chunks": scatter("rock")}),
        json!({"type": "river", "water": "water.test", "width": 3.0, "depth": 1.0,
            "bankWidth": 2.0, "bankMaterial": "ground.test", "flow": 0.5, "maxUphillGrade": 0.01,
            "details": {"banks": scatter("reed"), "shore": [with(scatter("rock"),
                json!({"id": "stones", "bank": {"min": -1.0, "max": 1.0}}))]}}),
        json!({"type": "path", "material": "ground.test", "width": 2.0, "shoulder": 1.0,
            "flatten": 0.5, "vegetationClearance": 1.0}),
        with(
            scatter("tree"),
            json!({"type": "forest", "interiorMargin": 0.0,
            "details": [with(scatter("reed"), json!({"id": "undergrowth"}))],
            "edge": with(scatter("reed"), json!({"width": 4.0}))}),
        ),
        with(
            scatter("reed"),
            json!({"type": "grass", "density": 2.0, "pattern": "patches"}),
        ),
        with(
            scatter("reed"),
            json!({"type": "ground-cover", "minSpacing": 0.25}),
        ),
        json!({"type": "meadow", "material": "ground.test", "canopy": 2.0, "reach": 6.0,
            "falloff": 4.0, "under": 0.2, "peak": 1.0, "open": 0.6, "edgeFade": 1.0,
            "noise": {"scale": 8.0, "strength": 0.5},
            "ragged": {"width": 1.0, "scale": 2.0, "strength": 0.5},
            "accents": [with(scatter("flower"), json!({"id": "flowers",
                "coverage": {"min": 0.2, "max": 0.8}, "trees": {"min": 0.0, "max": 6.0}}))]}),
    ]
}

#[test]
fn every_recipe_type_round_trips() {
    for raw in recipes() {
        let recipe: LandscapeRecipe = serde_json::from_value(raw.clone())
            .unwrap_or_else(|error| panic!("{}: {error}", raw["type"]));
        assert_eq!(serde_json::to_value(&recipe).unwrap(), raw);
    }
}

#[test]
fn recipes_are_strict() {
    let parse = |raw: Value| serde_json::from_value::<LandscapeRecipe>(raw).is_err();
    let [terrain, _, path, forest, grass, _, _]: [Value; 7] = recipes().try_into().unwrap();
    assert!(
        parse(with(grass, json!({"colour": "green"}))),
        "unknown scatter field"
    );
    assert!(parse(with(path, json!({"type": "road"}))), "unknown type");
    assert!(
        parse(with(terrain, json!({"operation": "tilt"}))),
        "unknown operation"
    );
    let mut forest = forest;
    forest.as_object_mut().unwrap().remove("interiorMargin");
    assert!(parse(forest), "zod defaults are written out");
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

/// Published records: models with scenery, a ground texture, a water profile and recipes.
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
    for (index, recipe) in recipes().into_iter().enumerate() {
        assets.push(asset(
            &format!("recipe.{index}"),
            "landscape-recipe",
            json!({"recipe": recipe}),
        ));
    }
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
    // A species without a scenery binding.
    let mut no_scenery = assets.clone();
    no_scenery[1].as_object_mut().unwrap().remove("scenery");
    assert!(
        validate(&no_scenery, &published)
            .iter()
            .any(|e| e.message.contains("species reed"))
    );

    let river = 7;
    assert!(
        broken(
            river,
            json!({"recipe": with(recipes()[1].clone(), json!({"water": "rock"}))})
        )
        .contains("water profile")
    );
    assert!(
        broken(
            river,
            json!({"recipe": with(recipes()[1].clone(), json!({"bedMaterial": "gone"}))})
        )
        .contains("material gone")
    );
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
