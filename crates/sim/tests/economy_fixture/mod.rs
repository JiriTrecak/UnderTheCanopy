//! Test-owned content and maps for the economy tests: a worker, a soldier and a hero; a
//! hall (drop-off, trains workers, upgrades into a keep), a house, a barracks, a forge
//! (research), a sanctuary (revival and heroes) and a forester (plants trees); felled
//! trees and an amber mine. No shipped content.
#![allow(dead_code)]

use canopy_content::{
    Content,
    map::UtcMap,
    placement::Owner,
    testing::{DataFolder, image, model, scenery_model},
};
use canopy_sim::{
    Action, MatchSetup, Queued, Slot, SlotKind, World,
    entity::{Entity, EntityId},
    state::FactKind,
};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

pub const SIZE: u32 = 256;
const HALO: i32 = 16;

pub const WOOD: &str = "item.test.wood";
pub const AMBER: &str = "item.test.amber";
pub const WORKER: &str = "unit.test.worker";
pub const SOLDIER: &str = "unit.test.soldier";
pub const HERO: &str = "unit.test.hero";
pub const HALL: &str = "building.test.hall";
pub const KEEP: &str = "building.test.keep";
pub const HOUSE: &str = "building.test.house";
pub const BARRACKS: &str = "building.test.barracks";
pub const FORGE: &str = "building.test.forge";
pub const SANCTUARY: &str = "building.test.sanctuary";
pub const FORESTER: &str = "building.test.forester";
pub const MINE: &str = "building.test.mine";
pub const TREE: &str = "resource.test.tree";
pub const TOOLS: &str = "research.test.tools";
pub const VIGOUR: &str = "research.test.vigour";

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
    let body = |hp: u32| json!({"maxHp": hp, "armor": 0, "armorType": "armor.plain"});
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
    let dimensions = json!({"radius": 0.4, "height": 1.5, "formationSpacing": 1.0});
    let cost = |item: &str, amount: u32| json!({"item": item, "amount": amount});
    let storage = json!({"capacity": 1000, "accepts": [WOOD, AMBER], "dropoff": true,
        "gathering": {"item": AMBER, "radius": 30}});
    let hall_behaviors = json!({
        "playerControl": {},
        "storage": storage,
        "production": {"mode": "queued", "outputs": [WORKER], "workerSlots": 0, "queueCapacity": 3},
    });
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 12, "maxSupply": 20, "supplyIcon": "asset.icon", "maxBuildings": 8,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 4,
            "research": {
                TOOLS: {"name": "Tools", "description": "", "icon": "asset.icon", "priority": 1,
                    "items": [cost(AMBER, 20), cost(WOOD, 5)], "workTicks": 30,
                    "effects": [{"units": [WORKER], "treeWorkRate": 2}]},
                VIGOUR: {"name": "Vigour", "description": "", "icon": "asset.icon", "priority": 2,
                    "requires": [KEEP], "items": [cost(AMBER, 10)], "workTicks": 20,
                    "effects": [{"units": [WORKER], "maxHp": 50}]},
            },
            "lootPools": {}, "armorCoefficient": 0.06,
            "damageTypes": {"damage.plain": {"name": "Plain", "appliesArmor": true}, "spell": {"name": "Spell", "appliesArmor": false}},
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [1], "heroRewards": [1], "neutralLevelCap": 2,
                "neutralMultipliersPermille": [500], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": [0],
            },
            "damageMultipliers": {"damage.plain": {"armor.plain": 1000}, "spell": {"armor.plain": 1000}},
            "startingSetup": {
                "id": "setup.test", "fort": HALL, "inventory": {AMBER: 100, WOOD: 50},
                "units": [
                    {"definition": WORKER, "offset": {"x": -2, "y": 6}},
                    {"definition": WORKER, "offset": {"x": 0, "y": 6}},
                    {"definition": WORKER, "offset": {"x": 2, "y": 6}},
                ],
                "gathering": [{"item": AMBER, "workers": 2, "radius": 30}],
            },
        },
        "definitions": [
            definition(WOOD, "item", json!({
                "currency": true, "stackLimit": 100, "displayOrder": 20,
                "creation": {"method": "harvest", "items": [], "source": TREE, "amount": 5,
                    "workTicks": 20, "impactTick": 3, "animationTicks": 10, "workAnimation": "chop"},
            })),
            definition(AMBER, "item", json!({
                "currency": true, "stackLimit": 100, "displayOrder": 10,
                "creation": {"method": "harvest", "items": [], "source": MINE, "amount": 5,
                    "workTicks": 8, "workAnimation": "chop"},
            })),
            definition(TREE, "resource", json!({
                "asset": "asset.tree", "yield": 10,
                "felling": {"maxHp": 10, "fallTicks": 6, "decayTicks": 20},
                "harvesting": {"activeWorkers": 1, "recommendedWorkers": 1, "searchRadius": 6},
                "creation": {"method": "plant", "items": [], "workTicks": 12},
                "regrowthTicks": 50,
            })),
            definition(MINE, "building", json!({
                "body": body(1000), "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
                "yield": 100, "constructionClearance": 1, "gatheringUnitCollision": false,
                "harvesting": {"activeWorkers": 1, "recommendedWorkers": 2, "searchRadius": 0},
            })),
            definition(WORKER, "unit", json!({
                "body": body(100), "supplyCost": 1, "vision": 10, "dimensions": dimensions,
                "creation": {"method": "train", "items": [cost(AMBER, 10)], "workTicks": 20},
                "behaviors": {
                    "playerControl": {},
                    "movement": {"speed": 4, "walkSpeed": 2, "idleWander": true},
                    "work": {"carryCapacity": 5,
                        "builds": [HALL, HOUSE, BARRACKS, FORGE, SANCTUARY, FORESTER],
                        "harvests": [WOOD, AMBER]},
                },
            })),
            definition(SOLDIER, "unit", json!({
                "body": body(150), "supplyCost": 2, "vision": 10, "dimensions": dimensions,
                "creation": {"method": "train", "items": [cost(AMBER, 15)], "workTicks": 30},
                "behaviors": {"playerControl": {}, "movement": {"speed": 4},
                    "combat": {"damage": 400, "damageType": "damage.plain", "range": 1,
                        "cooldownTicks": 10, "aggroRange": 3}},
            })),
            definition(HERO, "unit", json!({
                "hero": true, "body": body(300), "supplyCost": 3, "vision": 10, "dimensions": dimensions,
                "creation": {"method": "train", "items": [cost(AMBER, 20)], "workTicks": 25},
                "behaviors": {"playerControl": {}, "movement": {"speed": 4}},
            })),
            definition(HALL, "building", json!({
                "body": body(400), "vision": 12, "footprint": {"width": 8, "depth": 8}, "entrance": {"x": 0.5, "y": 4.5},
                "creation": {"method": "construct", "items": [cost(AMBER, 40), cost(WOOD, 10)],
                    "workTicks": 60, "workAnimation": "build"},
                "supplyProvided": 6, "heroCapacity": 1,
                "upgrade": {"target": KEEP, "items": [cost(AMBER, 30)], "workTicks": 40},
                "behaviors": hall_behaviors.clone(),
            })),
            definition(KEEP, "building", json!({
                "body": body(500), "vision": 12, "footprint": {"width": 8, "depth": 8}, "entrance": {"x": 0.5, "y": 4.5},
                "supplyProvided": 6, "heroCapacity": 2,
                "behaviors": hall_behaviors,
            })),
            definition(HOUSE, "building", json!({
                "body": body(200), "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
                "creation": {"method": "construct", "items": [cost(AMBER, 20), cost(WOOD, 10)],
                    "workTicks": 40, "workAnimation": "build"},
                "supplyProvided": 4,
                "behaviors": {"playerControl": {}},
            })),
            definition(BARRACKS, "building", json!({
                "requires": [HALL], "body": body(300), "footprint": {"width": 8, "depth": 8}, "entrance": {"x": 0.5, "y": 4.5},
                "creation": {"method": "construct", "items": [cost(AMBER, 30)],
                    "workTicks": 50, "workAnimation": "build"},
                "behaviors": {
                    "playerControl": {},
                    "storage": {"capacity": 100, "accepts": [AMBER]},
                    "production": {"mode": "queued", "outputs": [SOLDIER], "workerSlots": 0, "queueCapacity": 2},
                },
            })),
            definition(FORGE, "building", json!({
                "body": body(300), "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
                "creation": {"method": "construct", "items": [cost(AMBER, 20)],
                    "workTicks": 30, "workAnimation": "build"},
                "behaviors": {"playerControl": {}, "research": {"outputs": [TOOLS, VIGOUR], "queueCapacity": 1}},
            })),
            definition(SANCTUARY, "building", json!({
                "body": body(300), "footprint": {"width": 8, "depth": 8}, "entrance": {"x": 0.5, "y": 4.5},
                "creation": {"method": "construct", "items": [cost(AMBER, 30)],
                    "workTicks": 30, "workAnimation": "build"},
                "behaviors": {
                    "playerControl": {},
                    "storage": {"capacity": 100, "accepts": [AMBER]},
                    "production": {"mode": "queued", "outputs": [HERO, SOLDIER], "workerSlots": 0, "queueCapacity": 3},
                    "revival": {"items": [cost(AMBER, 30)], "itemsPerLevel": [cost(AMBER, 5)],
                        "workTicks": 40, "workTicksPerLevel": 10, "queueCapacity": 3},
                },
            })),
            definition(FORESTER, "building", json!({
                "placementNear": {"source": TREE, "radius": 8}, "body": body(200), "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
                "creation": {"method": "construct", "items": [cost(WOOD, 10)],
                    "workTicks": 20, "workAnimation": "build"},
                "behaviors": {
                    "playerControl": {},
                    "production": {"mode": "automatic", "outputs": [TREE], "workerSlots": 1,
                        "workRadius": 12, "jobName": "Forester"},
                },
            })),
        ],
    })
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            // A tree is felled with its own model's clips.
            let tree = scenery_model("asset.tree");
            let records = vec![image("asset.icon"), model("asset.model"), tree];
            let folder = DataFolder::new(game(), records);
            Arc::new(
                folder
                    .load("sim-economy")
                    .unwrap_or_else(|errors| panic!("{errors}")),
            )
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

/// Committed terrain from ground and water heights per vertex (world units).
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

/// A placement owned by player 1 (`player.1`).
pub fn place(id: &str, definition: &str, x: f64, y: f64) -> Value {
    place_for(id, definition, "player.1", x, y)
}

pub fn place_for(id: &str, definition: &str, owner: &str, x: f64, y: f64) -> Value {
    json!({"id": id, "definition": definition, "position": {"x": x, "y": y}, "rotation": 0, "owner": owner})
}

/// A neutral resource placement.
pub fn resource(id: &str, definition: &str, x: f64, y: f64) -> Value {
    place_for(id, definition, "none", x, y)
}

/// A placement with an initial state.
pub fn with_state(mut placement: Value, state: Value) -> Value {
    placement["initialState"] = state;
    placement
}

/// A revealed sandbox map with the given entities on flat ground.
pub fn sandbox(entities: Vec<Value>) -> UtcMap {
    sandbox_on(entities, flat())
}

/// A revealed sandbox map with the given entities and terrain.
pub fn sandbox_on(entities: Vec<Value>, terrain: Value) -> UtcMap {
    let document = json!({
        "v": 3, "size": SIZE, "name": "Economy fixture", "sandbox": true, "stamps": [],
        "playerStarts": [
            {"player": 1, "x": 11.5, "z": 11.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
        ],
        "entities": entities, "camps": [],
        "authoring": {"version": 1, "terrain": terrain, "objects": []},
    });
    UtcMap::from_value(document, "economy.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

/// A two-player skirmish: each start gets the test setup (hall and three workers) beside
/// an amber mine; `extra` adds entities.
pub fn skirmish(extra: Vec<Value>) -> UtcMap {
    let mut entities = vec![
        resource("mine-1", MINE, 41.5, 57.5),
        resource("mine-2", MINE, 201.5, 185.5),
    ];
    entities.extend(extra);
    let document = json!({
        "v": 3, "size": SIZE, "name": "Economy skirmish", "stamps": [],
        "initialExploration": "explored",
        "playerStarts": [
            {"player": 1, "x": 43.5, "z": 43.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
            {"player": 2, "x": 203.5, "z": 203.5, "setup": "setup.test", "mainFort": "start.player.2/main-fort"},
        ],
        "entities": entities, "camps": [],
        "authoring": {"version": 1, "terrain": flat(), "objects": []},
    });
    UtcMap::from_value(document, "skirmish.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

pub fn setup(players: u8) -> MatchSetup {
    MatchSetup {
        seed: 11,
        slots: (0..players)
            .map(|player| Slot {
                player,
                kind: SlotKind::Human,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

pub fn world(map: &UtcMap) -> World {
    World::new(content(), map, setup(2)).unwrap_or_else(|e| panic!("{e}"))
}

pub const P1: Owner = Owner::Player(0);
pub const P2: Owner = Owner::Player(1);

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

pub fn get(world: &World, id: u32) -> &Entity {
    world.state().entities.get(EntityId(id)).expect("entity")
}

/// Issues a command for `player` (zero-based), applied at the next step.
pub fn command_for(world: &mut World, player: u8, action: Value) {
    let action: Action = serde_json::from_value(action).expect("action");
    let tick = world.tick();
    let seq = world.state().counters.fact;
    world
        .enqueue(Queued {
            tick,
            player,
            seq,
            action,
        })
        .expect("command accepted for scheduling");
}

pub fn command(world: &mut World, action: Value) {
    command_for(world, 0, action);
}

/// Issues a command, steps once and returns the error it raised, if any.
pub fn try_command(world: &mut World, action: Value) -> Option<String> {
    let before = world.state().counters.fact;
    command(world, action);
    world.step();
    world
        .state()
        .facts
        .iter()
        .find(|f| f.id >= before && f.kind == FactKind::Error && f.owner == P1)
        .map(|f| f.message.clone())
}

pub fn run(world: &mut World, ticks: u32) {
    for _ in 0..ticks {
        world.step();
    }
}

/// Steps until `done` holds, at most `limit` ticks; returns the ticks taken.
pub fn run_until(world: &mut World, limit: u32, done: impl Fn(&World) -> bool) -> u32 {
    for n in 0..limit {
        if done(world) {
            return n;
        }
        world.step();
    }
    panic!("condition not reached in {limit} ticks");
}

pub fn wallet(world: &World, owner: Owner, item: &str) -> u32 {
    world
        .state()
        .wallets
        .get(&owner)
        .and_then(|w| w.get(item))
        .copied()
        .unwrap_or(0)
}

/// Sets `owner`'s funds of `item`.
pub fn fund(world: &mut World, owner: Owner, item: &str, amount: u32) {
    let id = canopy_content::Id::new(item).expect("id");
    world
        .state_mut()
        .wallets
        .entry(owner)
        .or_default()
        .insert(id, amount);
}

/// Error facts so far.
pub fn errors(world: &World) -> Vec<String> {
    world
        .state()
        .facts
        .iter()
        .filter(|f| f.kind == FactKind::Error)
        .map(|f| f.message.clone())
        .collect()
}

/// Ids of `owner`'s entities of `definition`, in id order.
pub fn all_of(world: &World, owner: Owner, definition: &str) -> Vec<u32> {
    world
        .state()
        .entities
        .iter()
        .filter(|e| e.owner == owner && world.catalog().id(e.definition).as_str() == definition)
        .map(|e| e.id.0)
        .collect()
}

/// The job of a worker, if any.
pub fn job(world: &World, worker: u32) -> Option<&canopy_sim::economy::Job> {
    let job = get(world, worker).unit.as_ref()?.work.job?;
    world.state().economy.job(job)
}
