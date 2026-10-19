//! Test-owned content and small maps for the AI tests: a worker, a melee soldier, an
//! archer, a levelling hero and a camp creature; a hall (drop-off, trains workers), a
//! house and a barracks; amber deposits; the same on a power network (D66). No
//! shipped content.
#![allow(dead_code)]

use canopy_content::{
    Content,
    map::UtcMap,
    placement::Owner,
    testing::{DataFolder, image, model},
};
use canopy_sim::{
    MatchSetup, Slot, SlotKind, World,
    ai::Memory,
    entity::{Entity, EntityId},
};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

pub const SIZE: u32 = 256;
const HALO: i32 = 16;

pub const AMBER: &str = "item.test.amber";
pub const WORKER: &str = "unit.test.worker";
pub const SOLDIER: &str = "unit.test.soldier";
pub const ARCHER: &str = "unit.test.archer";
pub const HERO: &str = "unit.test.hero";
pub const CREEP: &str = "unit.test.creep";
pub const HALL: &str = "building.test.hall";
pub const HOUSE: &str = "building.test.house";
pub const BARRACKS: &str = "building.test.barracks";
pub const MINE: &str = "building.test.mine";
/// An endless deposit gathered only through a well built on it (D62); maps place none
/// unless a test adds them.
pub const SPRING: &str = "building.test.spring";
pub const WELL: &str = "building.test.well";
pub const ESSENCE: &str = "item.test.essence";
/// A hero spell whose value lies in its second operation: a token hit, then the blow.
pub const SMITE: &str = "ability.test.smite";
pub const RUSH: &str = "ability.test.rush";
pub const CHARM: &str = "item.test.charm";
pub const WARD: &str = "ability.test.ward";

pub const P1: Owner = Owner::Player(0);
pub const P2: Owner = Owner::Player(1);

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

fn level(experience: u32, max_hp: u32, damage: u32) -> Value {
    json!({"experience": experience, "maxHp": max_hp, "damage": damage, "armor": 2,
        "cooldownTicks": 30, "maxMana": 100, "healthRegenPerSecond": 0, "manaRegenPerSecond": 1})
}

fn game() -> Value {
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
    let cost = |amount: u32| json!([{"item": AMBER, "amount": amount}]);
    let body =
        |hp: u32, armor: u32, kind: &str| json!({"maxHp": hp, "armor": armor, "armorType": kind});
    let dimensions =
        |radius: f64| json!({"radius": radius, "height": 1.5, "formationSpacing": 1.2});
    let blade = |damage: u32, cooldown: u32| {
        json!({"damage": damage, "damageType": "damage.blade", "range": 0.6, "cooldownTicks": cooldown,
            "aggroRange": 10, "attack": {"windupTicks": 8, "recoveryTicks": 8, "rangeBuffer": 0.6}})
    };
    let fighter = |hp: u32, combat: Value, train: u32| {
        json!({
            "body": body(hp, 1, "armor.light"), "supplyCost": 1, "vision": 14,
            "dimensions": dimensions(0.4),
            "creation": {"method": "train", "items": cost(train), "workTicks": 30},
            "behaviors": {"playerControl": {}, "movement": {"speed": 4}, "combat": combat},
        })
    };
    let mut archer = fighter(
        70,
        json!({"damage": 8, "damageType": "damage.pierce", "range": 7, "cooldownTicks": 40,
            "aggroRange": 12, "attack": {"windupTicks": 12, "recoveryTicks": 8, "rangeBuffer": 0.75},
            "projectile": {"speed": 20}}),
        15,
    );
    archer["vision"] = json!(16);
    let hero = json!({
        "hero": true, "body": body(300, 2, "armor.heavy"), "supplyCost": 4, "vision": 16,
        "dimensions": dimensions(0.5),
        "creation": {"method": "train", "items": cost(60), "workTicks": 60},
        "behaviors": {
            "playerControl": {}, "movement": {"speed": 4}, "combat": blade(20, 30),
            "progression": {"levels": [level(0, 300, 20), level(80, 360, 24), level(200, 420, 28)],
                "experienceRadius": 14},
            "inventory": {"slots": 2, "pickupRange": 1.5},
            "abilities": {"maxMana": 100, "manaRegenPerSecond": 1, "bindings": [
                {"id": "smite", "ability": SMITE, "initialRank": 0, "controls": ["player", "ai", "scenario"],
                    "command": {"hotkey": "Q", "column": 1},
                    "learning": {"requiredLevels": [1]},
                    "ai": {"preference": "enemy", "intervalTicks": 40}},
                {"id": "rush", "ability": RUSH, "initialRank": 0, "controls": ["player", "ai", "scenario"],
                    "command": {"hotkey": "W", "column": 2},
                    "learning": {"requiredLevels": [1]}},
                {"id": "ward", "ability": WARD, "initialRank": 0, "controls": ["player", "ai", "scenario"],
                    "command": {"hotkey": "E", "column": 3},
                    "learning": {"requiredLevels": [2]}},
            ]},
        },
    });
    let building = |hp: u32, width: u32, extra: Value| {
        let mut b = json!({"body": body(hp, 0, "armor.structure"), "vision": 12,
            "footprint": {"width": width, "depth": width},
            "entrance": {"x": 0.5, "y": f64::from(width) / 2.0 + 0.5}});
        for (key, value) in extra.as_object().expect("object") {
            b[key] = value.clone();
        }
        b
    };
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {
                "armor.light": {"name": "Light", "icon": "asset.icon"},
                "armor.heavy": {"name": "Heavy", "icon": "asset.icon"},
                "armor.structure": {"name": "Structure", "icon": "asset.icon"},
            },
            "maxUnits": 200, "maxSupply": 200, "supplyIcon": "asset.icon", "maxBuildings": 20,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {}, "lootPools": {}, "armorCoefficient": 0.06,
            "damageTypes": {
                "damage.blade": {"name": "Blade", "appliesArmor": true},
                "damage.pierce": {"name": "Pierce", "appliesArmor": true},
                "spell": {"name": "Spell", "appliesArmor": false},
            },
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [20, 40], "heroRewards": [100, 150], "neutralLevelCap": 3,
                "neutralMultipliersPermille": [1000, 700], "summonMultiplierPermille": 500,
                "globalFallback": true, "soloHeroTierBonusPermille": [0, 100],
            },
            "damageMultipliers": {
                "damage.blade": {"armor.light": 1000, "armor.heavy": 800, "armor.structure": 600},
                "damage.pierce": {"armor.light": 1200, "armor.heavy": 600, "armor.structure": 400},
                "spell": {"armor.light": 1000, "armor.heavy": 1000, "armor.structure": 500},
            },
            "startingSetup": {
                "id": "setup.test", "fort": HALL, "inventory": {AMBER: 300},
                "hero": {"default": HERO, "choices": [HERO], "offset": {"x": 0, "y": -10}},
                "units": [
                    {"definition": WORKER, "offset": {"x": -2, "y": 8}},
                    {"definition": WORKER, "offset": {"x": 0, "y": 8}},
                    {"definition": WORKER, "offset": {"x": 2, "y": 8}},
                    {"definition": WORKER, "offset": {"x": 4, "y": 8}},
                ],
                "gathering": [{"item": AMBER, "workers": 3, "radius": 30}],
            },
        },
        "definitions": [
            definition(BARRACKS, "building", building(400, 8, json!({
                "requires": [HALL],
                "creation": {"method": "construct", "items": cost(40), "workTicks": 40, "workAnimation": "build"},
                "behaviors": {"playerControl": {},
                    "storage": {"capacity": 100, "accepts": [AMBER]},
                    "production": {"mode": "queued", "outputs": [SOLDIER, ARCHER], "workerSlots": 0, "queueCapacity": 3}},
            }))),
            definition(HALL, "building", building(1200, 8, json!({
                "supplyProvided": 8, "heroCapacity": 1,
                "creation": {"method": "construct", "items": cost(80), "workTicks": 60, "workAnimation": "build"},
                "behaviors": {
                    "playerControl": {},
                    "storage": {"capacity": 1000, "accepts": [AMBER], "dropoff": true,
                        "gathering": {"item": AMBER, "radius": 30}},
                    "production": {"mode": "queued", "outputs": [WORKER, HERO], "workerSlots": 0, "queueCapacity": 3},
                },
            }))),
            definition(HOUSE, "building", building(200, 4, json!({
                "supplyProvided": 6,
                "creation": {"method": "construct", "items": cost(20), "workTicks": 30, "workAnimation": "build"},
                "behaviors": {"playerControl": {}},
            }))),
            definition(MINE, "building", json!({
                "body": body(1000, 5, "armor.structure"), "footprint": {"width": 4, "depth": 4},
                "entrance": {"x": 0.5, "y": 2.5}, "yield": 5000, "constructionClearance": 2,
                "gatheringUnitCollision": false,
                "harvesting": {"activeWorkers": 2, "recommendedWorkers": 3, "searchRadius": 0},
            })),
            definition(SPRING, "building", json!({
                "body": body(1000, 5, "armor.structure"), "footprint": {"width": 8, "depth": 8},
                "entrance": {"x": 0.5, "y": 4.5}, "yield": 500, "constructionClearance": 2,
                "gatheringUnitCollision": false,
                "harvesting": {"activeWorkers": 2, "recommendedWorkers": 2, "searchRadius": 0,
                    "needsBuilding": true, "endless": true},
            })),
            definition(WELL, "building", building(300, 8, json!({
                "placementOn": {"source": SPRING},
                "creation": {"method": "construct", "items": cost(20), "workTicks": 30, "workAnimation": "build"},
                "behaviors": {"playerControl": {},
                    "storage": {"capacity": 1000, "accepts": [ESSENCE], "dropoff": true}},
            }))),
            definition(ESSENCE, "item", json!({
                "currency": true, "stackLimit": 100, "displayOrder": 20,
                "creation": {"method": "harvest", "items": [], "source": SPRING, "amount": 5,
                    "workTicks": 10, "workAnimation": "chop"},
            })),
            definition(AMBER, "item", json!({
                "currency": true, "stackLimit": 100, "displayOrder": 10,
                "creation": {"method": "harvest", "items": [], "source": MINE, "amount": 5,
                    "workTicks": 8, "workAnimation": "chop"},
            })),
            definition(ARCHER, "unit", archer),
            definition(CHARM, "item", json!({"stackLimit": 1, "itemTier": 1,
                "itemEffect": {"type": "equipment", "damage": 2, "armor": 0, "maxHp": 0}})),
            definition(CREEP, "unit", json!({
                "body": body(80, 0, "armor.light"), "supplyCost": 1, "vision": 10, "experienceYield": 60,
                "dimensions": dimensions(0.4),
                "behaviors": {"movement": {"speed": 4}, "combat": blade(6, 40), "campDefense": {}},
            })),
            definition(HERO, "unit", hero),
            definition(SOLDIER, "unit", fighter(120, blade(10, 30), 15)),
            definition(WORKER, "unit", json!({
                "body": body(60, 0, "armor.light"), "supplyCost": 1, "vision": 10,
                "dimensions": dimensions(0.4),
                "creation": {"method": "train", "items": cost(10), "workTicks": 20},
                "behaviors": {
                    "playerControl": {}, "movement": {"speed": 4},
                    "work": {"carryCapacity": 5, "builds": [HALL, HOUSE, BARRACKS, WELL],
                        "harvests": [AMBER, ESSENCE]},
                },
            })),
        ],
    })
}

const REVALIDATE: [&str; 5] = ["caster", "target", "range", "visibility", "relationship"];

fn ability(
    id: &str,
    activation: &str,
    kind: &str,
    relations: &[&str],
    range: u32,
    mana: u32,
    on_release: Value,
) -> Value {
    json!({
        "schemaVersion": 1, "id": id, "name": id, "description": "", "activation": activation,
        "ranks": [{}],
        "targeting": {"kind": kind, "relations": relations, "allowSelf": kind == "self", "alive": true,
            "visible": true, "range": range, "outOfRange": "reject"},
        "cast": {"prepareTicks": 4, "recoverTicks": 4,
            "cost": {"resource": "mana", "amount": mana, "commit": "release"},
            "cooldown": {"ticks": 80, "start": "release", "scope": "ability"},
            "cancelBeforeRelease": "refund", "revalidateAtRelease": REVALIDATE},
        "onRelease": on_release,
        "presentation": format!("presentation.{}", id.trim_start_matches("ability.")),
    })
}

/// A passive that only sharpens its owner (it never needs casting).
fn passive(id: &str) -> Value {
    let mut a = ability(id, "passive", "self", &["ally"], 0, 0, json!([]));
    a["combatModifiers"] = json!({"evasionPermille": 50});
    a
}

fn library() -> Value {
    let abilities = vec![
        ability(
            SMITE,
            "targeted",
            "unit",
            &["enemy"],
            8,
            10,
            json!([
                {"op": "damage", "target": "target", "amount": 1, "damageType": "spell"},
                {"op": "damage", "target": "target", "amount": 40, "damageType": "spell"},
            ]),
        ),
        passive(RUSH),
        passive(WARD),
    ];
    let presentations: Vec<Value> = abilities
        .iter()
        .map(|a| {
            json!({"schemaVersion": 1, "id": a["presentation"], "icon": "asset.spell-icon",
                "animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"},
                "effects": []})
        })
        .collect();
    json!({"schemaVersion": 1, "abilities": abilities, "presentations": presentations})
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT.get_or_init(|| load(game(), "sim-ai")).clone()
}

/// A relay: the networked colony's node (D66).
pub const RELAY: &str = "building.test.relay";
/// Network radii in the networked content: the hall, the relay and the well.
pub const HALL_RADIUS: u32 = 24;
pub const RELAY_RADIUS: u32 = 20;
pub const WELL_RADIUS: u32 = 12;

/// The same content on a power network (D66): the hall and the well are sources, a relay
/// (a node that gives supply) extends it, and the barracks needs power.
pub fn networked_content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let mut game = game();
            let definitions = game["definitions"].as_array_mut().expect("definitions");
            for d in definitions.iter_mut() {
                match d["id"].as_str().expect("id") {
                    HALL => d["network"] = json!({"role": "source", "radius": HALL_RADIUS}),
                    WELL => d["network"] = json!({"role": "source", "radius": WELL_RADIUS}),
                    BARRACKS => d["power"] = json!("required"),
                    WORKER => {
                        d["behaviors"]["work"]["builds"] =
                            json!([HALL, HOUSE, BARRACKS, WELL, RELAY]);
                    }
                    _ => {}
                }
            }
            let mut relay = definitions
                .iter()
                .find(|d| d["id"] == HOUSE)
                .expect("house")
                .clone();
            relay["id"] = json!(RELAY);
            relay["name"] = json!(RELAY);
            relay["network"] = json!({"role": "node", "radius": RELAY_RADIUS});
            relay["power"] = json!("required");
            definitions.push(relay);
            load(game, "sim-ai-network")
        })
        .clone()
}

/// The same content with a well that pumps essence by itself (`income`, as the base
/// game's Substance Wells): no worker harvests essence and the spring takes nobody.
pub fn pumping_content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let mut game = game();
            let definitions = game["definitions"].as_array_mut().expect("definitions");
            for d in definitions.iter_mut() {
                match d["id"].as_str().expect("id") {
                    WELL => {
                        d["income"] = json!({"item": ESSENCE, "amount": 5, "intervalTicks": 20});
                    }
                    SPRING => {
                        d["harvesting"]["activeWorkers"] = json!(0);
                        d["harvesting"]["recommendedWorkers"] = json!(0);
                    }
                    WORKER => d["behaviors"]["work"]["harvests"] = json!([AMBER]),
                    _ => {}
                }
            }
            load(game, "sim-ai-pumping")
        })
        .clone()
}

/// A world on [`pumping_content`].
pub fn pumping_world(map: &UtcMap, both: bool) -> World {
    World::new(pumping_content(), map, setup(both)).unwrap_or_else(|e| panic!("{e}"))
}

fn load(game: Value, folder_name: &str) -> Arc<Content> {
    let mut folder = DataFolder::new(
        game,
        vec![
            image("asset.icon"),
            image("asset.spell-icon"),
            model("asset.model"),
        ],
    );
    folder.abilities = library();
    Arc::new(
        folder
            .load(folder_name)
            .unwrap_or_else(|errors| panic!("{errors}")),
    )
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

/// Flat dry ground.
fn flat() -> Value {
    let span = (SIZE + 2 * HALO as u32) as usize;
    let flow: Vec<u8> = (0..span * span).flat_map(|_| [128, 128, 0, 0]).collect();
    let zero = grid(|_, _| 0.0);
    json!({
        "version": 1, "size": SIZE,
        "heights": grid(|_, _| 0.0), "waterHeights": grid(|_, _| -1.0),
        "waterFlow": base64(&flow), "waterProfiles": [],
        "grass": zero, "rock": zero, "paint": [],
    })
}

pub fn place_for(id: &str, definition: &str, owner: &str, x: f64, y: f64) -> Value {
    json!({"id": id, "definition": definition, "position": {"x": x, "y": y}, "rotation": 0, "owner": owner})
}

/// The first start's position.
pub const HOME: (f64, f64) = (35.5, 35.5);
/// The second start's position.
pub const AWAY: (f64, f64) = (211.5, 211.5);

/// A two-player skirmish on flat ground, both colonies beside amber deposits; `extra`
/// adds placements, `camps` neutral camps. Explored from the start.
pub fn skirmish(extra: Vec<Value>, camps: Vec<Value>) -> UtcMap {
    let mut entities = vec![
        place_for("mine-1", MINE, "none", 33.5, 53.5),
        place_for("mine-2", MINE, "none", 41.5, 53.5),
        place_for("mine-3", MINE, "none", 209.5, 193.5),
        place_for("mine-4", MINE, "none", 217.5, 193.5),
    ];
    entities.extend(extra);
    let document = json!({
        "v": 3, "size": SIZE, "name": "AI skirmish", "stamps": [],
        "initialExploration": "explored",
        "playerStarts": [
            {"player": 1, "x": HOME.0, "z": HOME.1, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
            {"player": 2, "x": AWAY.0, "z": AWAY.1, "setup": "setup.test", "mainFort": "start.player.2/main-fort"},
        ],
        "entities": entities, "camps": camps,
        "authoring": {"version": 1, "terrain": flat(), "objects": []},
    });
    UtcMap::from_value(document, "ai.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

/// A camp that attacks players, guarding `home` with its member placements.
pub fn camp(id: &str, home: (f64, f64), members: &[&str]) -> Value {
    json!({"id": id, "members": members, "home": {"x": home.0, "y": home.1},
        "aggroRange": 8, "leash": 14, "aggression": "players"})
}

/// Player 1 is an AI; player 2 an AI too when `both`, else a human who never acts.
pub fn setup(both: bool) -> MatchSetup {
    let kind = |ai: bool| if ai { SlotKind::Ai } else { SlotKind::Human };
    MatchSetup {
        seed: 3,
        slots: [kind(true), kind(both)]
            .into_iter()
            .enumerate()
            .map(|(player, kind)| Slot {
                player: player as u8,
                kind,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

pub fn world(map: &UtcMap, both: bool) -> World {
    World::new(content(), map, setup(both)).unwrap_or_else(|e| panic!("{e}"))
}

/// A world on [`networked_content`].
pub fn networked_world(map: &UtcMap, both: bool) -> World {
    World::new(networked_content(), map, setup(both)).unwrap_or_else(|e| panic!("{e}"))
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

pub fn get(world: &World, id: u32) -> &Entity {
    world.state().entities.get(EntityId(id)).expect("entity")
}

/// Living entities of `owner` and `definition`, in id order.
pub fn all_of(world: &World, owner: Owner, definition: &str) -> Vec<u32> {
    world
        .state()
        .entities
        .iter()
        .filter(|e| {
            e.owner == owner
                && e.hp != Some(0)
                && world.catalog().id(e.definition).as_str() == definition
        })
        .map(|e| e.id.0)
        .collect()
}

/// The AI memory of `player`.
pub fn memory(world: &World, player: u8) -> &Memory {
    world
        .state()
        .ai
        .iter()
        .find(|m| m.player == player)
        .expect("AI player")
}

pub fn memory_mut(world: &mut World, player: u8) -> &mut Memory {
    world
        .state_mut()
        .ai
        .iter_mut()
        .find(|m| m.player == player)
        .expect("AI player")
}

/// Creates an entity from a placement (as a scenario setup step).
pub fn spawn(world: &mut World, id: &str, definition: &str, owner: Owner, x: f64, y: f64) -> u32 {
    let owner = match owner {
        Owner::Nobody => "none".to_string(),
        Owner::Player(p) => format!("player.{}", p + 1),
    };
    let placement: canopy_content::placement::Placement =
        serde_json::from_value(place_for(id, definition, &owner, x, y)).expect("placement");
    let catalog = canopy_sim::catalog::Catalog::new(world.catalog().content.clone());
    canopy_sim::spawn::create(world.state_mut(), &catalog, &placement)
        .expect("spawn")
        .0
}
