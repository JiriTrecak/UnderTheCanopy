//! Test-owned content and testbed maps for the vision tests: scouts with sight 6, a short
//! sighted watcher, a sightless mole, a flyer, a lookout tower and harvestable trees, on
//! 256-cell testbeds with optional plateaus and a bridge. No shipped content.
#![allow(dead_code)]

#[path = "../terrain_fixture/mod.rs"]
mod terrain_fixture;

use canopy_content::{
    Content,
    map::UtcMap,
    testing::{DataFolder, image, model, scenery_model},
};
use canopy_sim::{
    MatchSetup, Slot, SlotKind, World,
    entity::{EntityId, Milli},
    vision::{EXPLORED, UNEXPLORED, VISIBLE},
};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

pub use terrain_fixture::terrain;

pub const SIZE: u32 = 256;

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
    let unit = |vision: Option<u32>, movement: Value| {
        let mut unit = json!({
            "body": body, "supplyCost": 1,
            "dimensions": {"radius": 0.4, "height": 1.5, "formationSpacing": 1},
            "behaviors": {"movement": movement},
        });
        if let Some(vision) = vision {
            unit["vision"] = json!(vision);
        }
        unit
    };
    let walker = json!({"speed": 1});
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 100, "maxSupply": 100, "supplyIcon": "asset.icon", "maxBuildings": 100,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {}, "lootPools": {}, "armorCoefficient": 0.06,
            "damageTypes": {"damage.plain": {"name": "Plain", "appliesArmor": true}},
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [1], "heroRewards": [1], "neutralLevelCap": 2,
                "neutralMultipliersPermille": [500], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": [0],
            },
            "damageMultipliers": {"damage.plain": {"armor.plain": 1000}},
            "startingSetup": {
                "id": "setup.test", "fort": "building.hall", "inventory": {"item.wood": 5},
                "gathering": [{"item": "item.wood", "workers": 1, "radius": 20}],
                "units": [{"definition": "unit.mole", "offset": {"x": 0, "y": 4}}],
            },
            "ai": {
                "revision": 1, "decisionTicks": 1, "economyTicks": 1, "strategyTicks": 1,
                "operationTicks": 1, "reactionTicks": 1, "orderIntervalTicks": 1,
                "workers": {"minimum": 1, "target": 2, "maximum": 3, "reserve": 0},
                "army": {"minimum": 0, "maximum": 1, "homeGuard": 0, "engagePermille": 0, "campPermille": 0, "retreatHealthPermille": 0, "pursuitRadius": 1},
                "limits": {"commandsPerBeat": 1, "placementCandidates": 1, "spellCandidates": 1},
                "composition": [], "skillPreference": [],
            },
        },
        "definitions": [
            definition("building.hall", "building", json!({
                "body": body, "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
                "behaviors": {"storage": {"capacity": 50, "accepts": ["item.wood"]}},
            })),
            definition("building.tower", "building", json!({
                "body": body, "vision": 5, "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
            })),
            definition("unit.scout", "unit", unit(Some(6), walker.clone())),
            definition("unit.watcher", "unit", unit(Some(2), walker.clone())),
            definition("unit.mole", "unit", unit(None, walker.clone())),
            definition("unit.flyer", "unit", unit(Some(4), json!({"speed": 1, "locomotion": "air", "flightHeight": 6}))),
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

fn landscape() -> Value {
    let asset = |id: &str, kind: &str| json!({"id": id, "name": id, "kind": kind, "scenery": format!("{id}-prop"), "scale": 1, "clearance": 0});
    let mut tree = asset("asset.tree", "tree");
    tree["harvesting"] = json!({"definition": "resource.tree", "asset": "asset.tree"});
    json!([tree, asset("asset.bridge", "bridge")])
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let mut bridge = scenery_model("asset.bridge");
            bridge["scenery"][0]["type"] = json!("span");
            bridge["scenery"][0]["deck"] = json!({"width": 4, "depth": 12, "height": 0.4,
                "arch": 0, "thickness": 0.4, "level": 1, "connections": {"start": 0, "end": 0}});
            let records = vec![
                image("asset.icon"),
                model("asset.model"),
                scenery_model("asset.tree"),
                bridge,
            ];
            let mut folder = DataFolder::new(game(), records);
            folder.landscape = landscape();
            Arc::new(
                folder
                    .load("sim-vision")
                    .unwrap_or_else(|errors| panic!("{errors}")),
            )
        })
        .clone()
}

/// A placed entity.
pub fn place(id: &str, definition: &str, owner: &str, x: f64, y: f64) -> Value {
    json!({"id": id, "definition": definition, "owner": owner, "position": {"x": x, "y": y}})
}

/// A placed entity on a bridge deck.
pub fn place_on(id: &str, definition: &str, owner: &str, x: f64, y: f64, deck: &str) -> Value {
    json!({"id": id, "definition": definition, "owner": owner,
        "position": {"x": x, "y": y, "surface": deck}})
}

/// A testbed with `entities` on ground of the given height (metres) per vertex.
pub fn testbed(entities: Vec<Value>, ground: impl Fn(i32, i32) -> f32, hidden: bool) -> Value {
    let mut map = json!({
        "v": 3,
        "size": SIZE,
        "name": "Vision fixture",
        "sandbox": true,
        "stamps": [],
        "playerStarts": [{"player": 1, "x": 9.5, "z": 9.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"}],
        "entities": entities,
        "camps": [],
        "authoring": {"version": 1, "terrain": terrain(ground, |_, _| -1.0), "objects": []},
    });
    if hidden {
        map["initialExploration"] = json!("hidden");
    }
    map
}

pub fn flat(entities: Vec<Value>) -> Value {
    testbed(entities, |_, _| 0.0, true)
}

pub fn parse(document: Value) -> UtcMap {
    UtcMap::from_value(document, "vision.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

/// Players `0..teams.len()`, each on the given team (`None`: its own).
pub fn setup(teams: &[Option<u8>]) -> MatchSetup {
    MatchSetup {
        seed: 11,
        slots: teams
            .iter()
            .enumerate()
            .map(|(player, &team)| Slot {
                player: player as u8,
                kind: SlotKind::Human,
                name: None,
                team,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

pub fn start(document: Value, teams: &[Option<u8>]) -> World {
    World::new(content(), &parse(document), setup(teams)).unwrap_or_else(|e| panic!("{e}"))
}

/// The entity created from a placement.
pub fn id(world: &World, placement: &str) -> EntityId {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement))
        .unwrap_or_else(|| panic!("no {placement}"))
        .id
}

pub fn cell(x: i32, y: i32) -> u32 {
    (y * SIZE as i32 + x) as u32
}

pub fn milli(x: i32, y: i32) -> Milli {
    Milli {
        x: x * 1000,
        y: y * 1000,
    }
}

/// Teleports an entity, as a movement system would move it.
pub fn put(world: &mut World, id: EntityId, at: Milli) {
    world
        .state_mut()
        .entities
        .get_mut(id)
        .expect("entity")
        .position = at;
}

/// The fog of `player`'s team at a cell.
pub fn fog(world: &World, player: u8, x: i32, y: i32) -> u8 {
    world
        .sight()
        .cells(canopy_content::placement::Owner::Player(player))
        .expect("player")
        .get(cell(x, y))
}

pub fn visible(world: &World, player: u8, x: i32, y: i32) -> bool {
    fog(world, player, x, y) == VISIBLE
}

pub const FOG: [u8; 3] = [UNEXPLORED, EXPLORED, VISIBLE];

/// Cells of `player`'s team in sight, row by row.
pub fn sight_cells(world: &World, player: u8) -> Vec<u32> {
    let cells = world
        .sight()
        .cells(canopy_content::placement::Owner::Player(player))
        .expect("player");
    (0..cells.len() as u32)
        .filter(|&c| cells.get(c) == VISIBLE)
        .collect()
}

pub fn verify(world: &World) {
    world
        .vision()
        .verify(
            world.state(),
            world.catalog(),
            world.terrain(),
            &canopy_sim::vision::Current,
        )
        .unwrap_or_else(|e| panic!("tick {}: {e}", world.state().tick));
}

/// Saves and restores through JSON on the map built from `document`.
pub fn reload_on(world: &World, document: Value) -> World {
    let text = serde_json::to_string(&world.save()).expect("save");
    World::restore(
        content(),
        &parse(document),
        serde_json::from_str(&text).expect("load"),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}
