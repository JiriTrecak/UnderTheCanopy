//! Map rules that need the content, against a small test-owned data set.

mod map_fixture;

use canopy_content::{Content, map};
use map_fixture::{parse, set};
use serde_json::{Value, json};
use std::sync::OnceLock;

fn definition(id: &str, kind: &str, extra: Value) -> Value {
    let mut definition = json!({
        "id": id, "kind": kind, "name": id, "description": "",
        "asset": "asset.model", "icon": "asset.icon",
    });
    for (key, value) in extra.as_object().expect("object") {
        definition[key] = value.clone();
    }
    definition
}

fn game() -> Value {
    let body = json!({"maxHp": 100, "armor": 0, "armorType": "armor.plain"});
    let building = |extra: Value| {
        let mut fields = json!({"body": body, "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5}});
        for (key, value) in extra.as_object().expect("object") {
            fields[key] = value.clone();
        }
        fields
    };
    let unit = |behaviors: Value| json!({"body": body, "supplyCost": 1, "dimensions": {"radius": 0.4, "height": 1, "formationSpacing": 1}, "behaviors": behaviors});
    let combat = json!({"damage": 1, "damageType": "damage.plain", "range": 1, "cooldownTicks": 10, "aggroRange": 5});
    let level = |experience: u32, max_hp: u32| json!({"experience": experience, "maxHp": max_hp, "damage": 0, "armor": 0, "cooldownTicks": 1, "maxMana": 0, "healthRegenPerSecond": 0, "manaRegenPerSecond": 0});
    let meta = json!({"name": "Command", "description": "", "icon": "asset.icon", "priority": 0});
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
    let equipment = json!({"type": "equipment", "damage": 1, "armor": 0, "maxHp": 0});
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 100, "maxSupply": 100, "supplyIcon": "asset.icon", "maxBuildings": 100,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {},
            "lootPools": {
                "loot.common": {"rolls": 1, "entries": [{"item": "item.charm", "weight": 1}]},
                "loot.legend": {"rolls": 2, "maxTier": 3, "entries": [{"item": "item.crown", "weight": 1}]},
            },
            "armorCoefficient": 0.06,
            "damageTypes": {"damage.plain": {"name": "Plain", "appliesArmor": true}},
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [1], "heroRewards": [1], "neutralLevelCap": 2,
                "neutralMultipliersPermille": [500], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": [0],
            },
            "damageMultipliers": {"damage.plain": {"armor.plain": 1000}},
            "startingSetup": {
                "id": "setup.test", "fort": "building.hall", "inventory": {"item.coin": 10},
                "units": [{"definition": "unit.worker", "offset": {"x": 0, "y": 4}}],
            },
        },
        "definitions": [
            definition("building.hall", "building", building(json!({"behaviors": {"storage": {"capacity": 50, "accepts": ["item.coin"]}}}))),
            definition("building.mine", "building", building(json!({"yield": 100, "harvesting": {"activeWorkers": 1, "recommendedWorkers": 1, "searchRadius": 4}}))),
            definition("unit.worker", "unit", unit(json!({"movement": {"speed": 1}}))),
            definition("unit.creep", "unit", unit(json!({"movement": {"speed": 1}, "combat": combat, "campDefense": {}}))),
            definition("unit.hero", "unit", {
                let mut hero = unit(json!({"movement": {"speed": 1}, "progression": {"levels": [level(0, 100), level(100, 150)], "experienceRadius": 5}}));
                hero["hero"] = json!(true);
                hero
            }),
            definition("item.coin", "item", json!({"stackLimit": 100, "currency": true})),
            definition("item.charm", "item", json!({"stackLimit": 1, "itemTier": 1, "itemEffect": equipment})),
            definition("item.crown", "item", json!({"stackLimit": 1, "itemTier": 3, "itemEffect": equipment})),
            definition("resource.tree", "resource", json!({"asset": "asset.tree", "yield": 10})),
        ],
    })
}

fn content() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        let records = vec![
            canopy_content::testing::image("asset.icon"),
            canopy_content::testing::model("asset.model"),
            canopy_content::testing::scenery_model("asset.tree"),
        ];
        canopy_content::testing::DataFolder::new(game(), records)
            .load("map-rules")
            .unwrap_or_else(|errors| panic!("{errors}"))
    })
}

fn creep(id: &str) -> Value {
    json!({"id": id, "definition": "unit.creep", "position": {"x": 100, "y": 100}, "owner": "none"})
}

fn camp(id: &str, members: &[&str]) -> Value {
    json!({"id": id, "members": members, "home": {"x": 100, "y": 100}, "aggroRange": 6, "leash": 12, "aggression": "players"})
}

/// The fixture map with a guarded camp; valid against the fixture content.
fn valid() -> Value {
    let mut document = map_fixture::map();
    set(
        &mut document,
        "/entities",
        json!([
            creep("creep.1"),
            {"id": "mine.1", "definition": "building.mine", "position": {"x": 60.5, "y": 60.5}, "owner": "none", "initialState": {"amount": 50, "construction": "complete"}},
            {"id": "tree.1", "definition": "resource.tree", "position": {"x": 70, "y": 70}, "owner": "none", "appearance": {"asset": "scenery.asset.tree-prop", "scale": 1.2}},
            {"id": "charm", "definition": "item.charm", "position": {"x": 72, "y": 70}, "owner": "none", "initialState": {"quantity": 1}},
        ]),
    );
    set(
        &mut document,
        "/camps",
        json!([camp("camp.1", &["creep.1"])]),
    );
    document
}

fn errors(document: Value) -> Vec<String> {
    let map = parse(document).unwrap_or_else(|e| panic!("{e}"));
    map::validate(&map, content())
        .into_iter()
        .map(|e| e.to_string())
        .collect()
}

fn fails(document: Value, expected: &str) {
    let found = errors(document);
    assert!(
        found.iter().any(|e| e.contains(expected)),
        "expected {expected:?}, got {found:?}"
    );
}

fn with(pointer: &str, value: Value) -> Value {
    let mut document = valid();
    set(&mut document, pointer, value);
    document
}

#[test]
fn valid_map_passes() {
    assert_eq!(errors(valid()), Vec::<String>::new());
}

#[test]
fn placements_reference_fitting_definitions() {
    let entity = |extra: Value| {
        let mut p = json!({"id": "p", "definition": "unit.worker", "position": {"x": 90, "y": 90}, "owner": "none"});
        for (key, value) in extra.as_object().expect("object") {
            p[key] = value.clone();
        }
        json!([creep("creep.1"), p])
    };
    for (extra, expected) in [
        (
            json!({"definition": "unit.missing"}),
            "Unknown definition: unit.missing",
        ),
        (
            json!({"definition": "item.charm", "initialState": {"quantity": 2}}),
            "invalid loose stack quantity",
        ),
        (
            json!({"definition": "building.mine", "owner": "player.1", "position": {"x": 90.5, "y": 90.5}}),
            "mines must be neutral",
        ),
        (json!({"owner": "player.3"}), "missing owner slot"),
        (
            json!({"activation": "script"}),
            "scripted spawns require a mission map",
        ),
        (json!({"id": "creep.1"}), "Duplicate placement creep.1"),
        (
            json!({"id": "start.player.1/unit-001"}),
            "Duplicate placement start.player.1/unit-001",
        ),
        (
            json!({"initialState": {"health": 101}}),
            "invalid initial health",
        ),
        (
            json!({"initialState": {"amount": 1}}),
            "invalid initial yield",
        ),
        (
            json!({"initialState": {"construction": "complete"}}),
            "construction on non-building",
        ),
        (
            json!({"initialState": {"inventory": {"item.coin": 1}}}),
            "no storage",
        ),
        (
            json!({"initialState": {"experience": 10}}),
            "initial experience exceeds",
        ),
        (
            json!({"definition": "unit.hero", "initialState": {"experience": 101}}),
            "initial experience exceeds",
        ),
        (
            json!({"definition": "unit.hero", "initialState": {"experience": 100, "health": 151}}),
            "invalid initial health",
        ),
        (
            json!({"definition": "building.hall", "rotation": 45, "position": {"x": 90.5, "y": 90.5}}),
            "quarter turns",
        ),
        (
            json!({"definition": "building.hall", "position": {"x": 90.5, "y": 90.5, "surface": "deck"}}),
            "require ground",
        ),
        (
            json!({"definition": "building.hall", "position": {"x": 90.5, "y": 90.5}, "initialState": {"inventory": {"item.charm": 1}}}),
            "cannot store item.charm",
        ),
        (
            json!({"definition": "building.hall", "position": {"x": 90.5, "y": 90.5}, "initialState": {"inventory": {"item.coin": 51}}}),
            "exceeds capacity",
        ),
        (
            json!({"definition": "item.coin"}),
            "currencies cannot be placed",
        ),
        (
            json!({"appearance": {"asset": "asset.missing"}}),
            "Unknown asset: asset.missing",
        ),
        (
            json!({"appearance": {"asset": "asset.icon"}}),
            "appearance must reference a model",
        ),
        (
            json!({"appearance": {"asset": "scenery.asset.tree-prop"}}),
            "batched scenery appearance requires a resource",
        ),
        (
            json!({"definition": "resource.tree", "appearance": {"asset": "asset.model"}}),
            "resource appearance needs a scenery asset",
        ),
        (
            json!({"definition": "unit.creep"}),
            "camp defense requires an authored camp",
        ),
    ] {
        fails(with("/entities", entity(extra)), expected);
    }
}

#[test]
fn camps_reference_members_and_loot() {
    let creeps = json!([creep("creep.1"), creep("creep.2"), {"id": "w", "definition": "unit.worker", "position": {"x": 9, "y": 9}, "owner": "none"}]);
    let base = with("/entities", creeps);
    let with_camps = |camps: Value| {
        let mut document = base.clone();
        set(&mut document, "/camps", camps);
        document
    };
    let legendary = |id: &str, extra: Value| {
        let mut c = camp(id, &[]);
        c["legendary"] = json!(true);
        for (key, value) in extra.as_object().expect("object") {
            c[key] = value.clone();
        }
        c
    };
    let mut ok = legendary("camp.1", json!({"lootPool": "loot.legend"}));
    ok["members"] = json!(["creep.1", "creep.2"]);
    assert_eq!(errors(with_camps(json!([ok]))), Vec::<String>::new());
    for (camps, expected) in [
        (
            json!([camp("camp.1", &["creep.1", "creep.2", "w"])]),
            "invalid/duplicate camp member w",
        ),
        (
            json!([camp("camp.1", &["creep.1", "nobody", "creep.2"])]),
            "invalid/duplicate camp member nobody",
        ),
        (
            json!([
                camp("camp.1", &["creep.1"]),
                camp("camp.2", &["creep.1", "creep.2"])
            ]),
            "invalid/duplicate camp member creep.1",
        ),
        (
            json!([camp("camp.1", &["creep.1"]), camp("camp.1", &["creep.2"])]),
            "Duplicate camp ID",
        ),
        (
            json!([camp("camp.1", &["creep.1"])]),
            "creep.2: camp defense requires an authored camp",
        ),
        (
            json!([{"lootPool": "loot.missing", "members": ["creep.1", "creep.2"], "id": "c", "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"}]),
            "unknown loot pool loot.missing",
        ),
        (
            json!([{"lootPool": "loot.legend", "members": ["creep.1", "creep.2"], "id": "c", "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"}]),
            "T3 loot requires a legendary camp",
        ),
        (
            json!([{"fixedDrops": ["unit.worker"], "members": ["creep.1", "creep.2"], "id": "c", "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"}]),
            "fixed drop must be an item",
        ),
        (
            json!([{"fixedDrops": ["item.missing"], "members": ["creep.1", "creep.2"], "id": "c", "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"}]),
            "Unknown definition: item.missing",
        ),
        (
            json!([
                {"lootPool": "loot.legend", "legendary": true, "members": ["creep.1"], "id": "a", "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"},
                {"fixedDrops": ["item.crown", "item.crown"], "legendary": true, "members": ["creep.2"], "id": "b", "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"},
            ]),
            "at most three legendary items",
        ),
    ] {
        fails(with_camps(camps), expected);
    }
}

#[test]
fn player_starts_are_playable() {
    let start = |player: u32, x: f64, setup: &str| json!({"player": player, "x": x, "z": 100.5, "setup": setup, "mainFort": format!("start.player.{player}/main-fort")});
    for (starts, expected) in [
        (
            json!([start(1, 40.5, "setup.other"), start(2, 200.5, "setup.test")]),
            "Player 1: unknown setup setup.other",
        ),
        (
            json!([start(1, 40.5, "setup.test"), start(3, 200.5, "setup.test")]),
            "Place Player 1 and Player 2",
        ),
        (
            json!([
                start(1, 40.5, "setup.test"),
                start(2, 200.5, "setup.test"),
                start(4, 120.5, "setup.test")
            ]),
            "consecutive",
        ),
        (
            json!([start(1, 4.5, "setup.test"), start(2, 200.5, "setup.test")]),
            "Player 1 needs a building-grid-aligned start",
        ),
        (
            json!([start(1, 40.5, "setup.test"), start(2, 50.5, "setup.test")]),
            "at least 20 cells apart",
        ),
    ] {
        fails(with("/playerStarts", starts), expected);
    }
    fails(
        with("/playerStarts/1/mainFort", json!("creep.1")),
        "Player 2: invalid main fort objective",
    );
}

#[test]
fn missions_reference_races_and_company() {
    let mut mission = with(
        "/mission",
        json!({"campaign": "C", "title": "T", "order": 1, "script": "x = 1", "regions": [], "company": ["hero"]}),
    );
    set(
        &mut mission,
        "/entities",
        json!([{"id": "hero", "definition": "unit.hero", "position": {"x": 50, "y": 50}, "owner": "player.1"}]),
    );
    set(&mut mission, "/camps", json!([]));
    let mut one_start = mission.clone();
    set(
        &mut one_start,
        "/playerStarts",
        json!([{"player": 1, "x": 40.5, "z": 40.5, "setup": "setup.test", "mainFort": "x"}]),
    );
    assert_eq!(errors(one_start.clone()), Vec::<String>::new());
    let changed = |pointer: &str, value: Value| {
        let mut document = one_start.clone();
        set(&mut document, pointer, value);
        document
    };
    fails(
        changed("/mission/race", json!("elves")),
        "Unknown race: elves",
    );
    fails(
        changed("/mission/company", json!(["nobody"])),
        "Invalid campaign company slot: nobody",
    );
    fails(
        changed("/entities/0/owner", json!("none")),
        "Invalid campaign company slot: hero",
    );
    fails(
        changed("/mission/playerRaces", json!({"player.2": "ants"})),
        "missing participant player.2",
    );
    fails(
        changed("/playerStarts/0/player", json!(2)),
        "A campaign mission requires Player 1",
    );
    fails(
        changed(
            "/mission/regions",
            json!([{"id": "far", "x": 300, "y": 1, "radius": 2}]),
        ),
        "Mission region outside map.",
    );
}
