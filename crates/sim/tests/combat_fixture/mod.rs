//! Test-owned content and sandbox maps for the combat tests: melee, ranged, siege, charging
//! and flying fighters, a levelling hero, camp creatures, a lookout tower and loot. No
//! shipped content.
#![allow(dead_code)]

use canopy_content::{
    Content,
    map::UtcMap,
    placement::Owner,
    testing::{DataFolder, image, model},
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

fn body(hp: u32, armor: u32, kind: &str) -> Value {
    json!({"maxHp": hp, "armor": armor, "armorType": kind})
}

/// A weapon: damage, damage type, range, cooldown, aggro, wind-up, recovery.
#[allow(clippy::too_many_arguments)]
fn weapon(
    damage: u32,
    kind: &str,
    range: f64,
    cooldown: u32,
    aggro: u32,
    windup: u32,
    recovery: u32,
    buffer: f64,
) -> Value {
    json!({
        "damage": damage, "damageType": kind, "range": range, "cooldownTicks": cooldown,
        "aggroRange": aggro,
        "attack": {"windupTicks": windup, "recoveryTicks": recovery, "rangeBuffer": buffer},
    })
}

fn with(mut value: Value, extra: Value) -> Value {
    for (key, v) in extra.as_object().expect("object") {
        value[key] = v.clone();
    }
    value
}

fn level(experience: u32, max_hp: u32, damage: u32, armor: u32, cooldown: u32) -> Value {
    json!({"experience": experience, "maxHp": max_hp, "damage": damage, "armor": armor,
        "cooldownTicks": cooldown, "maxMana": 0, "healthRegenPerSecond": 0, "manaRegenPerSecond": 0})
}

pub fn game() -> Value {
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
    let control = json!({});
    let unit = |radius: f64, body: Value, behaviors: Value| {
        json!({
            "body": body, "supplyCost": 1, "vision": 16,
            "dimensions": {"radius": radius, "height": 1.5, "formationSpacing": 1.2},
            "behaviors": behaviors,
        })
    };
    let fighter = |radius: f64, hp: u32, armor: u32, kind: &str, speed: f64, combat: Value| {
        unit(
            radius,
            body(hp, armor, kind),
            json!({"playerControl": control, "movement": {"speed": speed}, "combat": combat}),
        )
    };
    let blade =
        |damage, cooldown, aggro| weapon(damage, "damage.blade", 0.6, cooldown, aggro, 10, 10, 0.6);
    let mut hero = fighter(
        0.5,
        200,
        3,
        "armor.heavy",
        4.0,
        weapon(15, "damage.blade", 0.6, 40, 12, 10, 10, 0.6),
    );
    hero["hero"] = json!(true);
    hero["behaviors"]["progression"] = json!({
        "levels": [level(0, 200, 15, 3, 40), level(100, 260, 19, 4, 38), level(250, 320, 23, 5, 36)],
        "experienceRadius": 12,
    });
    let mut wasp = fighter(0.4, 60, 0, "armor.light", 6.0, blade(6, 30, 10));
    wasp["behaviors"]["movement"] = json!({"speed": 6, "locomotion": "air", "flightHeight": 3});
    let camp_member = |combat: Value| {
        unit(
            0.4,
            body(80, 0, "armor.light"),
            json!({"movement": {"speed": 4}, "combat": combat, "campDefense": {}}),
        )
    };
    let building = |hp: u32, extra: Value| {
        with(
            json!({"body": body(hp, 0, "armor.structure"), "footprint": {"width": 4, "depth": 4},
                "entrance": {"x": 0.5, "y": 2.5}, "vision": 10}),
            extra,
        )
    };
    let trinket = json!({"stackLimit": 1, "itemTier": 1,
        "itemEffect": {"type": "equipment", "damage": 1, "armor": 0, "maxHp": 0}});
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
            "maxUnits": 400, "maxSupply": 400, "supplyIcon": "asset.icon", "maxBuildings": 100,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {},
            "lootPools": {"loot.test": {"rolls": 3, "maxTier": 2, "entries": [
                {"item": "item.trinket", "weight": 3}, {"item": "item.charm", "weight": 2}, {"item": null, "weight": 1},
            ]}},
            "armorCoefficient": 0.06,
            "damageTypes": {
                "damage.blade": {"name": "Blade", "appliesArmor": true},
                "damage.pierce": {"name": "Pierce", "appliesArmor": true},
                "damage.blast": {"name": "Blast", "appliesArmor": true},
                "spell": {"name": "Spell", "appliesArmor": false},
            },
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [20, 40, 60], "heroRewards": [100, 150, 200], "neutralLevelCap": 3,
                "neutralMultipliersPermille": [1000, 700], "summonMultiplierPermille": 500,
                "globalFallback": true, "soloHeroTierBonusPermille": [0, 100, 250],
            },
            "damageMultipliers": {
                "damage.blade": {"armor.light": 1000, "armor.heavy": 600, "armor.structure": 400},
                "damage.pierce": {"armor.light": 1200, "armor.heavy": 500, "armor.structure": 300},
                "damage.blast": {"armor.light": 1000, "armor.heavy": 1000, "armor.structure": 1500},
                "spell": {"armor.light": 1000, "armor.heavy": 1000, "armor.structure": 500},
            },
            "startingSetup": {
                "id": "setup.test", "fort": "building.hall", "inventory": {}, "units": [],
            },
            "ai": {
                "revision": 1, "decisionTicks": 1, "economyTicks": 1, "strategyTicks": 1,
                "operationTicks": 1, "reactionTicks": 1, "orderIntervalTicks": 1,
                "workers": {"minimum": 1, "target": 2, "maximum": 3, "reserve": 0},
                "army": {"minimum": 1, "maximum": 2, "homeGuard": 0, "engagePermille": 1, "campPermille": 1, "retreatHealthPermille": 1, "pursuitRadius": 1},
                "limits": {"commandsPerBeat": 1, "placementCandidates": 1, "spellCandidates": 1},
                "composition": [{"definition": "unit.swordsman", "weight": 1}], "skillPreference": [],
            },
        },
        "definitions": [
            definition("building.barracks", "building", building(500, json!({
                "behaviors": {"production": {"mode": "queued", "outputs": ["unit.swordsman"], "workerSlots": 0, "queueCapacity": 5}},
                "creation": {"method": "construct", "items": [], "workTicks": 100},
            }))),
            definition("building.hall", "building", building(1000, json!({
                "heroCapacity": 2,
                "behaviors": {"storage": {"capacity": 10, "accepts": ["item.trinket"]}},
            }))),
            definition("building.tower", "building", building(400, json!({
                "behaviors": {"playerControl": control},
                "garrison": {"capacity": 1, "accepts": ["unit.bowman"], "height": 6},
            }))),
            definition("item.charm", "item", trinket.clone()),
            definition("item.trinket", "item", trinket),
            definition("unit.bowman", "unit", fighter(0.4, 70, 0, "armor.light", 4.0, with(
                weapon(8, "damage.pierce", 10.0, 50, 14, 16, 10, 0.75),
                json!({"projectile": {"speed": 20}}),
            ))),
            definition("unit.critter", "unit", camp_member(blade(6, 40, 8))),
            definition("unit.hero", "unit", hero),
            definition("unit.knight", "unit", fighter(0.5, 160, 6, "armor.heavy", 3.0, blade(14, 50, 12))),
            definition("unit.lancer", "unit", fighter(0.4, 110, 1, "armor.light", 5.0, with(
                blade(12, 48, 24),
                json!({"charge": {"minRange": 6, "maxRange": 20, "cooldownTicks": 200, "durationTicks": 80, "speedPermille": 2000, "damagePermille": 2000}}),
            ))),
            definition("unit.mortar", "unit", fighter(0.5, 90, 0, "armor.light", 3.0, with(
                weapon(20, "damage.blast", 16.0, 80, 18, 20, 12, 0.75),
                json!({"shell": {"flightTicks": 30, "radius": 2, "slowPermille": 300, "slowTicks": 60}}),
            ))),
            definition("unit.spitter", "unit", camp_member(with(
                weapon(5, "damage.pierce", 6.0, 50, 8, 16, 10, 0.75),
                json!({"projectile": {"speed": 20}}),
            ))),
            definition("unit.swordsman", "unit", with(
                fighter(0.4, 100, 2, "armor.light", 4.0, blade(10, 40, 12)),
                json!({"creation": {"method": "train", "items": [], "workTicks": 40}}),
            )),
            definition("unit.wasp", "unit", wasp),
            definition("unit.worker", "unit", unit(0.4, body(50, 0, "armor.light"), json!({
                "playerControl": control, "movement": {"speed": 4},
                "work": {"carryCapacity": 1, "builds": ["building.barracks"]},
            }))),
        ],
    })
}

/// The asset records the content references.
pub fn records() -> Vec<Value> {
    vec![image("asset.icon"), model("asset.model")]
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let folder = DataFolder::new(game(), records());
            Arc::new(
                folder
                    .load("sim-combat")
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

/// Committed terrain from ground heights per cell (world units), dry everywhere.
pub fn terrain(ground: impl Fn(i32, i32) -> f32) -> Value {
    terrain_with_water(ground, |_, _| -1.0)
}

/// Committed terrain from ground and water heights per cell (world units).
pub fn terrain_with_water(
    ground: impl Fn(i32, i32) -> f32,
    water: impl Fn(i32, i32) -> f32,
) -> Value {
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

pub fn flat() -> Value {
    terrain(|_, _| 0.0)
}

/// A placement owned by player `player` (1-based; 0 for neutral). Buildings stand
/// complete.
pub fn placed(id: &str, definition: &str, x: f64, y: f64, player: u8) -> Value {
    let owner = if player == 0 {
        "none".to_string()
    } else {
        format!("player.{player}")
    };
    let mut p = json!({"id": id, "definition": definition, "position": {"x": x, "y": y}, "rotation": 0, "owner": owner});
    if definition.starts_with("building.") {
        p["initialState"] = json!({"construction": "complete"});
    }
    p
}

/// A camp of neutral members guarding `home`.
pub fn camp(
    id: &str,
    members: &[&str],
    home: (f64, f64),
    aggro: u32,
    leash: u32,
    extra: Value,
) -> Value {
    with(
        json!({"id": id, "members": members, "home": {"x": home.0, "y": home.1},
            "aggroRange": aggro, "leash": leash, "aggression": "players"}),
        extra,
    )
}

/// The map document: two players, the given entities, camps and terrain.
pub fn document(entities: Vec<Value>, camps: Vec<Value>, terrain: Value) -> Value {
    json!({
        "v": 3,
        "size": SIZE,
        "name": "Combat fixture",
        "stamps": [],
        "playerStarts": [
            {"player": 1, "x": 13.5, "z": 13.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
            {"player": 2, "x": 241.5, "z": 241.5, "setup": "setup.test", "mainFort": "start.player.2/main-fort"},
        ],
        "entities": entities,
        "camps": camps,
        "authoring": {"version": 1, "terrain": terrain, "objects": []},
    })
}

/// A sandbox map (no forts) for any number of players.
pub fn sandbox(entities: Vec<Value>) -> UtcMap {
    let mut doc = document(entities, vec![], flat());
    doc["sandbox"] = json!(true);
    doc["playerStarts"]
        .as_array_mut()
        .expect("starts")
        .truncate(1);
    UtcMap::from_value(doc, "fixture.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

/// `players` free-for-all slots.
pub fn free_for_all(players: u8) -> MatchSetup {
    MatchSetup {
        seed: 7,
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

pub fn map(entities: Vec<Value>) -> UtcMap {
    map_with(entities, vec![], flat())
}

pub fn map_with(entities: Vec<Value>, camps: Vec<Value>, terrain: Value) -> UtcMap {
    UtcMap::from_value(document(entities, camps, terrain), "fixture.utcmap")
        .unwrap_or_else(|errors| panic!("{errors}"))
}

/// Two players on separate teams; `allied` puts them on one team.
pub fn setup(allied: bool) -> MatchSetup {
    let slot = |player: u8| Slot {
        player,
        kind: SlotKind::Human,
        name: None,
        team: allied.then_some(0),
        hero: None,
        race: None,
    };
    MatchSetup {
        seed: 7,
        slots: vec![slot(0), slot(1)],
    }
}

pub fn world(map: &UtcMap) -> World {
    World::new(content(), map, setup(false)).unwrap_or_else(|e| panic!("{e}"))
}

pub fn entity<'a>(world: &'a World, placement: &str) -> &'a Entity {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement))
        .unwrap_or_else(|| panic!("no entity {placement}"))
}

pub fn find<'a>(world: &'a World, placement: &str) -> Option<&'a Entity> {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement))
}

pub fn id(world: &World, placement: &str) -> u32 {
    entity(world, placement).id.0
}

pub fn hp(world: &World, placement: &str) -> u32 {
    find(world, placement).and_then(|e| e.hp).unwrap_or(0)
}

pub fn alive(world: &World, placement: &str) -> bool {
    find(world, placement).is_some_and(|e| e.hp.is_some_and(|hp| hp > 0))
}

pub fn get(world: &World, id: EntityId) -> &Entity {
    world.state().entities.get(id).expect("entity")
}

/// Issues a command for `player` (1-based), applied on the next step.
pub fn command(world: &mut World, player: u8, action: Value) {
    let action: Action = serde_json::from_value(action).expect("action");
    let tick = world.tick();
    world
        .enqueue(Queued {
            tick,
            player: player - 1,
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

/// Runs until `done` holds or `limit` ticks pass; returns the ticks run.
pub fn run_until(world: &mut World, limit: u32, done: impl Fn(&World) -> bool) -> u32 {
    for n in 0..limit {
        if done(world) {
            return n;
        }
        world.step();
    }
    panic!("condition not reached in {limit} ticks");
}

pub fn errors(world: &World) -> Vec<String> {
    world
        .state()
        .facts
        .iter()
        .filter(|f| f.kind == canopy_sim::state::FactKind::Error)
        .map(|f| f.message.clone())
        .collect()
}

pub fn player(n: u8) -> Owner {
    Owner::Player(n - 1)
}

/// A save round trip through JSON on the same map.
pub fn reload(world: &World, map: &UtcMap) -> World {
    let text = serde_json::to_string(&world.save()).unwrap();
    World::restore(content(), map, serde_json::from_str(&text).unwrap())
        .unwrap_or_else(|e| panic!("{e}"))
}
