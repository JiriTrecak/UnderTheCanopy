//! Test-owned content and sandbox maps for the movement tests: walkers of several sizes, a
//! flyer, an idle-wandering worker, an immobile post, a blocking building and a bridge
//! deck. No shipped content.
#![allow(dead_code)]

use canopy_content::{
    Content,
    map::UtcMap,
    testing::{DataFolder, image, model, scenery_model},
};
use canopy_sim::{
    Action, MatchSetup, Queued, Slot, SlotKind, World,
    entity::{Entity, EntityId},
};
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
        "move", "attack", "stop", "hold", "patrol", "follow", "garrison", "unload", "build", "produce",
        "cancel", "rally", "pause", "upgrade", "cancelUpgrade", "research", "cancelResearch",
    ]
    .into_iter()
    .map(|name| (name.to_string(), meta.clone()))
    .collect();
    let unit = |radius: f64, height: f64, spacing: f64, behaviors: Value| {
        json!({
            "body": body, "supplyCost": 1,
            "dimensions": {"radius": radius, "height": height, "formationSpacing": spacing},
            "behaviors": behaviors,
        })
    };
    let control = json!({});
    let combat = json!({"damage": 1, "damageType": "damage.plain", "range": 1, "cooldownTicks": 10, "aggroRange": 5});
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 400, "maxSupply": 400, "supplyIcon": "asset.icon", "maxBuildings": 100,
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
                "id": "setup.test", "fort": "building.hall", "inventory": {},
                "units": [{"definition": "unit.walker", "offset": {"x": 0, "y": 4}}],
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
            })),
            definition("resource.tree", "resource", json!({"asset": "asset.tree", "yield": 10, "collisionRadius": 1})),
            definition("unit.fighter", "unit", unit(0.4, 1.5, 1.0, json!({"playerControl": control, "movement": {"speed": 4}, "combat": combat}))),
            definition("unit.flyer", "unit", unit(0.6, 1.0, 1.5, json!({"playerControl": control, "movement": {"speed": 8, "locomotion": "air", "flightHeight": 3}}))),
            definition("unit.post", "unit", unit(0.4, 1.5, 1.0, json!({"playerControl": control}))),
            definition("unit.trooper", "unit", unit(1.5, 4.8, 3.25, json!({"playerControl": control, "movement": {"speed": 12}}))),
            definition("unit.walker", "unit", unit(0.4, 1.5, 1.0, json!({"playerControl": control, "movement": {"speed": 4}}))),
            definition("unit.wanderer", "unit", unit(0.4, 1.5, 1.0, json!({
                "playerControl": control,
                "movement": {"speed": 4, "walkSpeed": 2, "idleWander": true},
                "work": {"carryCapacity": 1, "builds": []},
            }))),
        ],
    })
}

/// A scenery model whose scenery entry carries `extra` (a deck).
fn scenery_with(id: &str, extra: Value) -> Value {
    let mut record = scenery_model(id);
    for (key, value) in extra.as_object().expect("object") {
        record["scenery"][0][key] = value.clone();
    }
    record
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let records = vec![
                image("asset.icon"),
                model("asset.model"),
                scenery_model("asset.tree"),
                scenery_with(
                    "asset.bridge",
                    json!({"type": "span", "deck": {"width": 4, "depth": 14, "height": 0.4,
                        "arch": 0, "thickness": 0.4, "level": 1,
                        "connections": {"start": 0, "end": 0}}}),
                ),
            ];
            let mut folder = DataFolder::new(game(), records);
            folder.landscape = json!([
                {"id": "asset.bridge", "name": "bridge", "kind": "bridge", "scenery": "asset.bridge-prop", "scale": 1, "clearance": 0},
            ]);
            Arc::new(folder.load("sim-movement").unwrap_or_else(|errors| panic!("{errors}")))
        })
        .clone()
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
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

fn grid(f: impl Fn(i32, i32) -> f32) -> String {
    let vertices = SIZE as i32 + 2 * HALO + 1;
    let bytes: Vec<u8> = (0..vertices)
        .flat_map(|z| (0..vertices).map(move |x| (x - HALO, z - HALO)))
        .flat_map(|(x, z)| f(x, z).to_le_bytes())
        .collect();
    base64(&bytes)
}

/// Committed terrain from ground and water heights per cell (world units).
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

/// Flat dry ground.
pub fn flat() -> Value {
    terrain(|_, _| 0.0, |_, _| -1.0)
}

/// Flat ground with 3 m cliffs (impassable) wherever `wall(x, y)` holds.
pub fn walls(wall: impl Fn(i32, i32) -> bool) -> Value {
    terrain(move |x, y| if wall(x, y) { 3.0 } else { 0.0 }, |_, _| -1.0)
}

/// A unit placement for player 1.
pub fn unit(id: &str, definition: &str, x: i32, y: i32) -> Value {
    json!({"id": id, "definition": definition, "position": {"x": x, "y": y}, "rotation": 0, "owner": "player.1"})
}

/// A sandbox map: player 1 alone, the given entities, terrain and stamps.
pub fn map(entities: Vec<Value>, terrain: Value, stamps: Vec<Value>) -> UtcMap {
    let document = json!({
        "v": 3,
        "size": SIZE,
        "name": "Movement fixture",
        "sandbox": true,
        "stamps": stamps,
        "playerStarts": [
            {"player": 1, "x": 8.5, "z": 8.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
        ],
        "entities": entities,
        "camps": [],
        "authoring": {"version": 1, "terrain": terrain, "objects": []},
    });
    UtcMap::from_value(document, "fixture.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

pub fn setup() -> MatchSetup {
    MatchSetup {
        seed: 7,
        slots: vec![Slot {
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    }
}

pub fn world(map: &UtcMap) -> World {
    World::new(content(), map, setup()).unwrap_or_else(|e| panic!("{e}"))
}

/// The entity placed as `placement`.
pub fn entity<'a>(world: &'a World, placement: &str) -> &'a Entity {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement))
        .unwrap_or_else(|| panic!("no entity {placement}"))
}

pub fn id(world: &World, placement: &str) -> u32 {
    entity(world, placement).id.0
}

pub fn get(world: &World, id: EntityId) -> &Entity {
    world.state().entities.get(id).expect("entity")
}

/// Issues a command for player 1, applied on the next step.
pub fn command(world: &mut World, action: Value) {
    let action: Action = serde_json::from_value(action).expect("action");
    let tick = world.tick();
    world
        .enqueue(Queued {
            tick,
            player: 0,
            seq: 0,
            action,
        })
        .expect("command accepted for scheduling");
}

pub fn run(world: &mut World, ticks: u32) {
    for _ in 0..ticks {
        world.step();
    }
}

/// Position in cells.
pub fn at(world: &World, placement: &str) -> (f64, f64) {
    let e = entity(world, placement);
    (f64::from(e.position.x) / 1000.0, f64::from(e.position.y) / 1000.0)
}

pub fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Error facts so far.
pub fn errors(world: &World) -> Vec<String> {
    world
        .state()
        .facts
        .iter()
        .filter(|f| f.kind == canopy_sim::state::FactKind::Error)
        .map(|f| f.message.clone())
        .collect()
}

/// A save round trip through JSON on the same map.
pub fn reload(world: &World, map: &UtcMap) -> World {
    let text = serde_json::to_string(&world.save()).unwrap();
    World::restore(content(), map, serde_json::from_str(&text).unwrap()).unwrap_or_else(|e| panic!("{e}"))
}
