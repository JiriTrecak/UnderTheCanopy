//! What a match reaches (`canopy_content::reach`), on a small test-owned data folder: the
//! effects of everything in play, through any chain of references, and nothing else.

mod map_fixture;

use canopy_content::{Content, map::UtcMap, reach};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const ICON: &str = "asset.test.icon";

fn record(id: &str, role: &str, file: &str, render: Value) -> Value {
    json!({"id": id, "name": id, "kind": "model", "tags": [], "revision": 1, "profile": "test",
        "outputs": [{"role": role, "path": format!("assets/library/{id}/{file}"), "sha256": "0".repeat(64), "bytes": 1}],
        "render": render, "scenery": []})
}

fn unit(id: &str, abilities: &[&str]) -> Value {
    let bindings: Vec<Value> = abilities
        .iter()
        .enumerate()
        .map(|(i, a)| {
            json!({"id": format!("a{i}"), "ability": a, "initialRank": 1,
                "controls": ["player", "ai"], "command": {"hotkey": "Q", "column": i + 1},
                "ai": {"preference": "enemy", "intervalTicks": 10}})
        })
        .collect();
    let mut unit = json!({"id": id, "kind": "unit", "name": id, "description": "",
        "asset": "asset.test.model", "icon": ICON, "supplyCost": 1,
        "dimensions": {"radius": 0.5, "height": 1, "formationSpacing": 1},
        "body": {"maxHp": 50, "armor": 0, "armorType": "plain"},
        "behaviors": {"movement": {"speed": 1}, "playerControl": {}}});
    if !bindings.is_empty() {
        unit["behaviors"]["abilities"] =
            json!({"maxMana": 100, "manaRegenPerSecond": 0.5, "bindings": bindings});
    }
    unit
}

fn ability(id: &str, release: Value, effect: Option<&str>) -> (Value, Value) {
    let presentation = format!("presentation.{id}");
    let ability = json!({"schemaVersion": 1, "id": id, "name": id, "activation": "targeted",
        "ranks": [{"damage": 10}],
        "targeting": {"kind": "unit", "alive": true, "visible": true, "relations": ["enemy"],
            "allowSelf": false, "range": 6, "outOfRange": "reject"},
        "cast": {"prepareTicks": 4, "recoverTicks": 4,
            "cost": {"resource": "mana", "amount": 10, "commit": "release"},
            "cooldown": {"ticks": 40, "start": "release", "scope": "ability"},
            "cancelBeforeRelease": "refund",
            "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
        "onRelease": [release], "presentation": presentation});
    let effects: Vec<Value> = effect
        .into_iter()
        .map(|e| json!({"id": "fx", "effect": e, "event": "released", "anchor": "target"}))
        .collect();
    let presentation = json!({"schemaVersion": 1, "id": presentation, "icon": ICON,
        "animations": {"prepare": "cast", "release": "cast", "recover": "idle", "fallback": "idle"},
        "effects": effects});
    (ability, presentation)
}

fn effect(id: &str) -> Value {
    json!({"schemaVersion": 1, "id": id, "name": id, "durationTicks": 10, "layers": [
        {"id": "flash", "shape": "billboard", "colour": "#ffffff", "accent": "#ffcc00",
            "durationTicks": 10, "count": 1, "size": 1, "height": 1,
            "texture": {"asset": "asset.test.spark", "role": "image", "index": 1}}]})
}

fn setup(id: &str, unit: &str) -> Value {
    json!({"id": id, "fort": "building.test.fort", "inventory": {},
        "units": [{"definition": unit, "offset": {"x": 0, "y": 6}}]})
}

/// Two races: the red one's mage casts a bolt and calls a wisp whose flare has its own
/// effect; the blue one's knight smashes. A camp's loot pool holds a glinting charm, heroes
/// level up with a flourish, and an effect nothing refers to is left over.
fn content() -> Content {
    let damage = json!({"op": "damage", "target": "target", "amount": 10, "damageType": "spell"});
    let summon = json!({"op": "summon", "target": "target", "amount": 1,
        "definition": "unit.test.wisp", "replace": false, "radius": 2});
    let abilities = [
        ability(
            "ability.test.bolt",
            damage.clone(),
            Some("effect.test.spark"),
        ),
        ability("ability.test.call", summon, None),
        ability(
            "ability.test.flare",
            damage.clone(),
            Some("effect.test.burst"),
        ),
        ability("ability.test.smash", damage, Some("effect.test.smash")),
    ];
    let meta = json!({"name": "Do", "description": "", "icon": ICON, "priority": 0});
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
            "id": "test", "armorTypes": {"plain": {"name": "Plain", "icon": ICON}},
            "maxUnits": 10, "maxSupply": 10, "supplyIcon": ICON, "maxBuildings": 10,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {},
            "lootPools": {"loot.test.camp": {"rolls": 1, "entries": [{"item": "item.test.charm", "weight": 1}]}},
            "armorCoefficient": 0.06,
            "damageTypes": {"spell": {"name": "Spell", "appliesArmor": false}},
            "heroStunDurationPermille": 500,
            "experience": {"unitRewards": [], "heroRewards": [], "neutralLevelCap": 5,
                "neutralMultipliersPermille": [], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": []},
            "damageMultipliers": {"spell": {"plain": 1000}},
            "startingSetup": setup("setup.test", "unit.test.mage"),
            "defaultRace": "red",
            "levelUp": {"effect": "effect.test.level"},
            "races": {
                "red": {"name": "Red", "description": "", "roster": ["unit.test.mage"],
                    "startingSetup": setup("setup.red", "unit.test.mage")},
                "blue": {"name": "Blue", "description": "", "roster": ["unit.test.knight"],
                    "startingSetup": setup("setup.blue", "unit.test.knight")},
            },
        },
        "definitions": [
            {"id": "building.test.fort", "kind": "building", "name": "Fort", "description": "",
                "asset": "asset.test.model", "icon": ICON,
                "body": {"maxHp": 100, "armor": 0, "armorType": "plain"},
                "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5}},
            unit("unit.test.mage", &["ability.test.bolt", "ability.test.call"]),
            unit("unit.test.wisp", &["ability.test.flare"]),
            unit("unit.test.knight", &["ability.test.smash"]),
            unit("unit.test.beast", &[]),
            {"id": "item.test.charm", "kind": "item", "name": "Charm", "description": "",
                "asset": "asset.test.model", "icon": ICON, "stackLimit": 1, "itemTier": 1,
                "itemEffect": {"type": "equipment", "damage": 0, "armor": 1, "maxHp": 0},
                "presentation": {"effects": [{"effect": "effect.test.glint", "state": "always"}]}},
        ]
    });
    let records = vec![
        record(
            ICON,
            "image",
            "image.png",
            json!([{"id": ICON, "image": "assets/library/asset.test.icon/image.png"}]),
        ),
        record("asset.test.spark", "image", "image.png", json!([])),
        record(
            "asset.test.model",
            "model",
            "geometry.glb",
            json!([{"id": "asset.test.model", "file": "assets/library/asset.test.model/geometry.glb"}]),
        ),
    ];
    let mut folder = canopy_content::testing::DataFolder::new(game, records);
    folder.abilities = json!({"schemaVersion": 1,
        "abilities": abilities.iter().map(|a| a.0.clone()).collect::<Vec<_>>(),
        "presentations": abilities.iter().map(|a| a.1.clone()).collect::<Vec<_>>()});
    folder.effects = json!(
        [
            "effect.test.spark",
            "effect.test.burst",
            "effect.test.smash",
            "effect.test.glint",
            "effect.test.level",
            "effect.test.omen",
            "effect.test.unused",
        ]
        .map(effect)
    );
    folder
        .load("reach")
        .unwrap_or_else(|errors| panic!("{errors}"))
}

/// The fixture map with a looted camp and a mission that plays an effect from its script.
fn map() -> UtcMap {
    let mut map = map_fixture::map();
    map["entities"] = json!([{"id": "beast.1", "definition": "unit.test.beast",
        "position": {"x": 100, "y": 100}, "owner": "none"}]);
    map["camps"] = json!([{"id": "camp.test", "members": ["beast.1"], "home": {"x": 100, "y": 100},
        "aggroRange": 8, "leash": 14, "aggression": "players", "lootPool": "loot.test.camp"}]);
    map["mission"] = json!({"regions": [], "script":
        "function on_start()\n  mission.effect_at(\"omen\", \"effect.test.omen\", 10, 12)\nend\n"});
    UtcMap::from_value(map, "reach.utcm").unwrap_or_else(|errors| panic!("{errors}"))
}

fn effects(reach: &reach::Reach) -> BTreeSet<&str> {
    reach.effects.iter().map(String::as_str).collect()
}

#[test]
fn a_match_reaches_the_effects_of_what_is_in_play_and_nothing_else() {
    let content = content();
    let red = reach::of_match(&content, &map(), &["red"], &json!({}));
    assert_eq!(
        effects(&red),
        BTreeSet::from([
            // The mage's bolt, and the flare of the wisp it calls.
            "effect.test.spark",
            "effect.test.burst",
            // The camp's charm, the rules' level-up and the mission's script.
            "effect.test.glint",
            "effect.test.level",
            "effect.test.omen",
        ])
    );
    // Another race in play, or one of its units in the world (a save, a carried hero).
    let both = reach::of_match(&content, &map(), &["red", "blue"], &json!({}));
    assert!(both.effects.contains("effect.test.smash"));
    let world = json!({"entities": [{"definition": "unit.test.knight"}]});
    let carried = reach::of_match(&content, &map(), &["red"], &world);
    assert!(carried.effects.contains("effect.test.smash"));
    assert!(!carried.effects.contains("effect.test.unused"));
}

#[test]
fn the_reach_covers_every_effect_its_elements_refer_to() {
    let content = content();
    let reached = reach::of_match(&content, &map(), &["red", "blue"], &json!({}));
    // Whatever a reached element mentions anywhere, as an effect id, is reached too.
    let mut mentioned = BTreeSet::new();
    let mut note = |value: Value| {
        let text = value.to_string();
        for e in &content.effects {
            if text.contains(&format!("\"{}\"", e.id.as_str())) {
                mentioned.insert(e.id.to_string());
            }
        }
    };
    for d in &content.definitions {
        if reached.elements.contains(d.id.as_str()) {
            note(serde_json::to_value(d).unwrap());
            note(serde_json::to_value(content.look(d.id.as_str())).unwrap());
        }
    }
    for a in &content.abilities.abilities {
        if reached.elements.contains(a.id.as_str()) {
            note(serde_json::to_value(a).unwrap());
        }
    }
    assert!(!mentioned.is_empty());
    let missed: Vec<&String> = mentioned.difference(&reached.effects).collect();
    assert!(
        missed.is_empty(),
        "effects referred to but not reached: {missed:?}"
    );
}
