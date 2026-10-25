//! Test-owned content and `.utcmap` documents for the terrain tests: a hall, a worker, a
//! harvestable tree whose wood the starting workers gather, and scenery with a bridge
//! deck and a blocker. No shipped content.
#![allow(dead_code)]

use canopy_content::{
    Content,
    map::UtcMap,
    testing::{DataFolder, image, model, scenery_model},
};
use canopy_sim::{MatchSetup, Slot, SlotKind};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

pub const SIZE: u32 = 256;
const HALO: i32 = 16;

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
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 100, "maxSupply": 100, "supplyIcon": "asset.icon", "maxBuildings": 100,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {}, "lootPools": {}, "armorCoefficient": 0.06,
            "damageTypes": {"damage.plain": {"name": "Plain", "appliesArmor": true}, "spell": {"name": "Spell", "appliesArmor": false}},
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [1], "heroRewards": [1], "neutralLevelCap": 2,
                "neutralMultipliersPermille": [500], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": [0],
            },
            "damageMultipliers": {"damage.plain": {"armor.plain": 1000}, "spell": {"armor.plain": 1000}},
            "startingSetup": {
                "id": "setup.test", "fort": "building.hall", "inventory": {"item.wood": 5},
                "gathering": [{"item": "item.wood", "workers": 2, "radius": 20}],
                "units": [
                    {"definition": "unit.worker", "offset": {"x": -1, "y": 4}},
                    {"definition": "unit.worker", "offset": {"x": 1, "y": 4}},
                ],
            },
        },
        "definitions": [
            definition("building.hall", "building", json!({
                "body": body, "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
                "behaviors": {"storage": {"capacity": 50, "accepts": ["item.wood"]}},
            })),
            definition("unit.worker", "unit", json!({
                "body": body, "supplyCost": 1,
                "dimensions": {"radius": 0.4, "height": 1.5, "formationSpacing": 1},
                "behaviors": {"movement": {"speed": 1}},
            })),
            definition("item.wood", "item", json!({
                "stackLimit": 100,
                "creation": {"method": "harvest", "items": [], "source": "resource.tree", "amount": 1, "workTicks": 10},
            })),
            definition("resource.tree", "resource", json!({
                "asset": "asset.tree", "yield": 10, "constructionClearance": 2,
                "harvesting": {"activeWorkers": 1, "recommendedWorkers": 1, "searchRadius": 4},
            })),
        ],
    })
}

/// A scenery model whose scenery entry carries `extra` (a deck or blockers).
fn scenery_with(id: &str, extra: Value) -> Value {
    let mut record = scenery_model(id);
    for (key, value) in extra.as_object().expect("object") {
        record["scenery"][0][key] = value.clone();
    }
    record
}

fn landscape() -> Value {
    let asset = |id: &str, kind: &str, harvesting: Option<Value>| {
        let mut asset = json!({"id": id, "name": id, "kind": kind, "scenery": format!("{id}-prop"),
            "scale": 1, "clearance": 0});
        if let Some(harvesting) = harvesting {
            asset["harvesting"] = harvesting;
        }
        asset
    };
    json!([
        asset(
            "asset.tree",
            "tree",
            Some(json!({"definition": "resource.tree", "asset": "asset.tree"}))
        ),
        asset("asset.rock", "prop", None),
        asset("asset.bridge", "bridge", None),
        asset("asset.wall", "prop", None),
    ])
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let records = vec![
                image("asset.icon"),
                model("asset.model"),
                scenery_model("asset.tree"),
                scenery_model("asset.rock"),
                scenery_with(
                    "asset.bridge",
                    json!({"type": "span", "deck": {"width": 4, "depth": 12, "height": 0.4,
                        "arch": 0, "thickness": 0.4, "level": 1,
                        "connections": {"start": 0, "end": 0}}}),
                ),
                scenery_with(
                    "asset.wall",
                    json!({"blockers": [{"width": 6, "depth": 1}]}),
                ),
            ];
            let mut folder = DataFolder::new(game(), records);
            folder.landscape = landscape();
            Arc::new(
                folder
                    .load("sim-terrain")
                    .unwrap_or_else(|errors| panic!("{errors}")),
            )
        })
        .clone()
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const SYMBOLS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(SYMBOLS[(word >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A vertex grid from a function of world position (x, z).
fn grid(f: impl Fn(i32, i32) -> f32) -> String {
    let vertices = SIZE as i32 + 2 * HALO + 1;
    let bytes: Vec<u8> = (0..vertices)
        .flat_map(|z| (0..vertices).map(move |x| (x - HALO, z - HALO)))
        .flat_map(|(x, z)| f(x, z).to_le_bytes())
        .collect();
    base64(&bytes)
}

/// Committed terrain with the given ground and water heights per vertex.
pub fn terrain(ground: impl Fn(i32, i32) -> f32, water: impl Fn(i32, i32) -> f32) -> Value {
    let span = (SIZE + 2 * HALO as u32) as usize;
    let flow: Vec<u8> = (0..span * span).flat_map(|_| [128, 128, 0, 0]).collect();
    let zero = grid(|_, _| 0.0);
    json!({
        "version": 1, "size": SIZE,
        "heights": grid(ground), "waterHeights": grid(water),
        "waterFlow": base64(&flow), "waterProfiles": [],
        "grass": zero, "rock": zero, "paint": [],
    })
}

/// A flat, dry skirmish map with two starts on the building lattice.
pub fn map() -> Value {
    json!({
        "v": 3,
        "size": SIZE,
        "name": "Terrain fixture",
        "stamps": [],
        "playerStarts": [
            {"player": 1, "x": 41.5, "z": 41.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
            {"player": 2, "x": 201.5, "z": 201.5, "setup": "setup.test", "mainFort": "start.player.2/main-fort"},
        ],
        "entities": [],
        "camps": [],
        "authoring": {"version": 1, "terrain": terrain(|_, _| 0.0, |_, _| -1.0), "objects": []},
    })
}

pub fn object(id: &str, asset: &str, x: f64, z: f64) -> Value {
    json!({"id": id, "asset": asset, "x": x, "z": z})
}

pub fn parse(document: Value) -> UtcMap {
    UtcMap::from_value(document, "fixture.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

pub fn setup(players: u8) -> MatchSetup {
    MatchSetup {
        seed: 7,
        slots: (0..players)
            .map(|player| Slot {
                difficulty: Default::default(),
                player,
                kind: SlotKind::Ai,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}
