//! Ability and effect rules that span files, checked on a small test-owned data folder.

use canopy_content::{Content, Errors};
use serde_json::{Value, json};

/// A complete, valid data folder: one fort, one caster unit, one spell and its effect.
#[derive(Clone)]
struct World {
    game: Value,
    abilities: Value,
    effects: Value,
    manifest: Value,
}

fn record(id: &str, role: &str, file: &str, render: Value) -> Value {
    json!({"id": id, "name": id, "kind": "model", "tags": [], "revision": 1, "profile": "test",
        "outputs": [{"role": role, "path": format!("assets/library/{id}/{file}"), "sha256": "0".repeat(64), "bytes": 1}],
        "render": render, "scenery": []})
}

fn statue() -> Value {
    let id = "asset.test.statue";
    let mut statue = record(
        id,
        "image",
        "image.png",
        json!([{"id": id, "file": format!("assets/library/{id}/geometry.glb")}]),
    );
    statue["outputs"].as_array_mut().unwrap().push(
        json!({"role": "model", "path": format!("assets/library/{id}/geometry.glb"), "sha256": "0".repeat(64), "bytes": 1}),
    );
    statue
}

fn world() -> World {
    let icon = "asset.test.icon";
    let manifest = json!({"version": 1, "records": [
        record(icon, "image", "image.png", json!([{"id": icon, "image": "assets/library/asset.test.icon/image.png"}])),
        record("asset.test.model", "model", "geometry.glb", json!([{"id": "asset.test.model", "file": "assets/library/asset.test.model/geometry.glb"}])),
        record("asset.test.spark", "image", "image.png", json!([])),
        // Publishes an image file but renders as a model, so it is no icon.
        statue(),
    ]});
    let meta = json!({"name": "Do", "description": "", "icon": icon, "priority": 0});
    let actions: serde_json::Map<String, Value> = [
        "move",
        "attack",
        "stop",
        "hold",
        "patrol",
        "follow",
        "garrison",
        "unload",
        "build",
        "produce",
        "cancel",
        "rally",
        "pause",
        "upgrade",
        "cancelUpgrade",
        "research",
        "cancelResearch",
    ]
    .into_iter()
    .map(|name| (name.to_string(), meta.clone()))
    .collect();
    let game = json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "test", "armorTypes": {"plain": {"name": "Plain", "icon": icon}},
            "maxUnits": 10, "maxSupply": 10, "supplyIcon": icon, "maxBuildings": 10,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {}, "lootPools": {}, "armorCoefficient": 0.06,
            "damageTypes": {"spell": {"name": "Spell", "appliesArmor": false}},
            "heroStunDurationPermille": 500,
            "experience": {"unitRewards": [], "heroRewards": [], "neutralLevelCap": 5,
                "neutralMultipliersPermille": [], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": []},
            "damageMultipliers": {"spell": {"plain": 1000}},
            "startingSetup": {"id": "setup.test", "fort": "building.test.fort", "inventory": {}, "units": []},
        },
        "definitions": [
            {"id": "building.test.fort", "kind": "building", "name": "Fort", "description": "",
                "asset": "asset.test.model", "icon": icon,
                "body": {"maxHp": 100, "armor": 0, "armorType": "plain"},
                "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5}},
            {"id": "unit.test.mage", "kind": "unit", "name": "Mage", "description": "",
                "asset": "asset.test.model", "icon": icon, "supplyCost": 1,
                "dimensions": {"radius": 0.5, "height": 1, "formationSpacing": 1},
                "body": {"maxHp": 50, "armor": 0, "armorType": "plain"},
                "behaviors": {"movement": {"speed": 1}, "playerControl": {},
                    "abilities": {"maxMana": 100, "manaRegenPerSecond": 0.5, "bindings": [
                        {"id": "bolt", "ability": "ability.test.bolt", "initialRank": 1,
                            "controls": ["player", "ai"], "command": {"hotkey": "Q", "column": 1},
                            "ai": {"preference": "enemy", "intervalTicks": 10}}]}}}
        ]
    });
    let abilities = json!({"schemaVersion": 1, "abi": "abilities-1", "library": {"schemaVersion": 1,
        "abilities": [{"schemaVersion": 1, "id": "ability.test.bolt", "name": "Bolt", "activation": "targeted",
            "ranks": [{"damage": 10}, {"damage": 20}],
            "targeting": {"kind": "unit", "alive": true, "visible": true, "relations": ["enemy"],
                "allowSelf": false, "range": 6, "outOfRange": "reject"},
            "cast": {"prepareTicks": 4, "recoverTicks": 4,
                "cost": {"resource": "mana", "amount": 10, "commit": "release"},
                "cooldown": {"ticks": 40, "start": "release", "scope": "ability"},
                "cancelBeforeRelease": "refund",
                "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
            "onRelease": [{"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"}],
            "presentation": "presentation.test.bolt"}],
        "presentations": [{"schemaVersion": 1, "id": "presentation.test.bolt", "icon": icon,
            "animations": {"prepare": "cast", "release": "cast", "recover": "idle", "fallback": "idle"},
            "effects": [{"id": "hit", "effect": "effect.test.spark", "event": "damaged", "anchor": "target"}]}]}});
    let effects = json!({"schemaVersion": 1, "effects": [{"schemaVersion": 1, "id": "effect.test.spark",
        "name": "Spark", "durationTicks": 10, "layers": [{"id": "flash", "shape": "billboard",
            "colour": "#ffffff", "accent": "#ffcc00", "durationTicks": 10, "count": 1, "size": 1, "height": 1,
            "texture": {"asset": "asset.test.spark", "role": "image", "index": 1}}]}]});
    World {
        game,
        abilities,
        effects,
        manifest,
    }
}

fn load(world: &World, name: &str) -> Result<Content, Errors> {
    let mut folder = canopy_content::testing::DataFolder::new(
        world.game.clone(),
        world.manifest["records"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    folder.abilities = world.abilities["library"].clone();
    folder.effects = world.effects["effects"].clone();
    folder.load(&format!("abilities-{name}"))
}

/// Loads a broken world and returns its error messages.
fn problems(world: &World, name: &str) -> Vec<String> {
    match load(world, name) {
        Ok(_) => panic!("{name}: expected errors"),
        Err(errors) => errors.0.iter().map(|e| e.to_string()).collect(),
    }
}

fn assert_reports(world: &World, name: &str, expected: &str) {
    let found = problems(world, name);
    assert!(
        found.len() == 1 && found[0].contains(expected),
        "{name}: expected only {expected:?}, found {found:#?}"
    );
}

#[test]
fn the_fixture_world_is_valid() {
    let content = load(&world(), "valid").unwrap_or_else(|errors| panic!("{errors}"));
    let ability = content.abilities.ability("ability.test.bolt").unwrap();
    assert_eq!(ability.description, "");
    let caster = content
        .definition("unit.test.mage")
        .and_then(|d| d.behaviors.abilities.as_ref())
        .unwrap();
    assert_eq!(caster.bindings[0].ability.as_str(), "ability.test.bolt");
    assert_eq!(content.effects[0].layers[0].start_tick, 0);
}

#[test]
fn bindings_must_name_known_abilities() {
    let mut w = world();
    w.game["definitions"][1]["behaviors"]["abilities"]["bindings"][0]["ability"] =
        json!("ability.test.missing");
    assert_reports(
        &w,
        "unknown-ability",
        "Unknown ability ability.test.missing",
    );
}

#[test]
fn commands_require_player_control() {
    let mut w = world();
    w.game["definitions"][1]["behaviors"]["abilities"]["bindings"][0]["controls"] = json!(["ai"]);
    assert_reports(&w, "command-control", "A command requires player control");
}

#[test]
fn damage_types_must_exist() {
    let mut w = world();
    w.abilities["library"]["abilities"][0]["onRelease"][0]["damageType"] = json!("frost");
    assert_reports(&w, "damage-type", "unknown damage type frost");
}

#[test]
fn summons_must_be_units() {
    let mut w = world();
    w.abilities["library"]["abilities"][0]["onRelease"][0] = json!({"op": "summon",
        "target": "target", "amount": 1, "definition": "building.test.fort", "replace": false, "radius": 2});
    assert_reports(
        &w,
        "summon",
        "summons require a unit definition (building.test.fort)",
    );
}

#[test]
fn spell_icons_must_be_images() {
    let mut w = world();
    w.abilities["library"]["presentations"][0]["icon"] = json!("asset.test.statue");
    assert_reports(&w, "icon", "Spell icon must reference an image");
}

#[test]
fn presentations_bind_known_effects() {
    let mut w = world();
    w.abilities["library"]["presentations"][0]["effects"][0]["effect"] = json!("effect.test.none");
    assert_reports(
        &w,
        "unknown-effect",
        "Unknown visual effect effect.test.none",
    );
}

#[test]
fn state_effects_name_their_kind_s_states_known_effects_and_sockets() {
    let mut w = world();
    let effect = |extra: Value| {
        let mut e = json!({"effect": "effect.test.spark"});
        e.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        e
    };
    // Buildings rest, work, spawn and stand; units move, fight and cast; both fire on
    // clip events from sockets.
    w.game["definitions"][0]["presentation"] = json!({"effects": [
        effect(json!({"state": "idle"})), effect(json!({"state": "always", "socket": "socket_motes"})),
        effect(json!({"event": "pump"}))]});
    w.game["definitions"][1]["presentation"] = json!({"effects": [
        effect(json!({"state": "move", "socket": "socket_exhaust"})), effect(json!({"state": "cast"})),
        effect(json!({"event": "release", "socket": "socket_muzzle"}))]});
    load(&w, "state-effects").unwrap_or_else(|errors| panic!("{errors}"));
    let mut wrong = w.clone();
    wrong.game["definitions"][1]["presentation"]["effects"][0]["state"] = json!("spawn");
    assert_reports(&wrong, "unit-state", "unknown effect state spawn");
    let mut wrong = w.clone();
    wrong.game["definitions"][0]["presentation"]["effects"][0]["state"] = json!("move");
    assert_reports(&wrong, "building-state", "unknown effect state move");
    let mut wrong = w.clone();
    wrong.game["definitions"][1]["presentation"]["effects"][2]["effect"] =
        json!("effect.test.none");
    assert_reports(
        &wrong,
        "state-effect",
        "unknown presentation effect effect.test.none",
    );
    let mut wrong = w.clone();
    wrong.game["definitions"][1]["presentation"]["effects"][2]["socket"] = json!("muzzle");
    assert_reports(&wrong, "state-socket", "effect socket muzzle");
    let mut wrong = w;
    wrong.game["definitions"][1]["presentation"]["effects"][1]["event"] = json!("release");
    assert_reports(&wrong, "both", "exactly one of state and event");
}

#[test]
fn effect_resources_must_be_published() {
    let mut w = world();
    w.effects["effects"][0]["layers"][0]["texture"]["index"] = json!(2);
    assert_reports(
        &w,
        "resource",
        "Missing published effect image asset.test.spark/image:2",
    );
}

#[test]
fn unknown_fields_are_reported_against_the_ability_file() {
    let mut w = world();
    w.abilities["library"]["abilities"][0]["cooldown"] = json!(5);
    assert_reports(
        &w,
        "unknown-field",
        "base/abilities/ability.test.bolt.json · cooldown: unknown field",
    );
}
