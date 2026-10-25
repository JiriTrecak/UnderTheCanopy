//! Test-owned content and mission maps for the mission tests. No shipped content or map ids.
#![allow(dead_code)]

#[path = "../../../content/tests/map_fixture/mod.rs"]
mod map_fixture;

use canopy_content::{Content, map::UtcMap};
use canopy_sim::{
    MatchSetup, Slot, SlotKind, World,
    mission::{MissionState, state::Variable},
};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock};

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
    let unit = |behaviors: Value| json!({"body": body, "supplyCost": 1, "dimensions": {"radius": 0.4, "height": 1, "formationSpacing": 1}, "behaviors": behaviors});
    let movement = json!({"speed": 1, "turnRate": 360});
    let combat = json!({"damage": 1, "damageType": "damage.plain", "range": 1, "cooldownTicks": 10, "aggroRange": 5});
    let level = |experience: u32, max_hp: u32| json!({"experience": experience, "maxHp": max_hp, "damage": 1, "armor": 0, "cooldownTicks": 10, "maxMana": 0, "healthRegenPerSecond": 0, "manaRegenPerSecond": 0});
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
    let mut hero = unit(json!({
        "movement": movement, "combat": combat,
        "progression": {"levels": [level(0, 100), level(100, 150)], "experienceRadius": 5},
        "inventory": {"slots": 2, "pickupRange": 1},
    }));
    hero["hero"] = json!(true);
    let mut hall = json!({"body": body, "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5}});
    hall["behaviors"] = json!({"storage": {"capacity": 50, "accepts": ["item.amber"]}});
    let mut big = unit(json!({"movement": movement, "combat": combat}));
    big["body"]["maxHp"] = json!(300);
    // A hostile camp's wagon whose killer takes the amber it carries.
    let mut cart = unit(json!({"movement": movement, "combat": combat, "campDefense": {}}));
    cart["body"]["maxHp"] = json!(3);
    cart["bounty"] = json!([{"item": "item.amber", "amount": 30}]);
    // A hero and a soldier the player commands (autoplay).
    let mut captain = hero.clone();
    captain["behaviors"]["playerControl"] = json!({});
    captain["vision"] = json!(10);
    let mut soldier = unit(json!({"movement": movement, "combat": combat, "playerControl": {}}));
    soldier["vision"] = json!(10);
    let mut hunter = unit(json!({"movement": movement, "combat": combat}));
    hunter["vision"] = json!(10);
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 100, "maxSupply": 100, "supplyIcon": "asset.icon", "maxBuildings": 100,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {},
            "lootPools": {},
            "armorCoefficient": 0.06,
            "damageTypes": {"damage.plain": {"name": "Plain", "appliesArmor": true}, "spell": {"name": "Spell", "appliesArmor": false}},
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [1], "heroRewards": [1], "neutralLevelCap": 2,
                "neutralMultipliersPermille": [500], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": [0],
            },
            "damageMultipliers": {"damage.plain": {"armor.plain": 1000}, "spell": {"armor.plain": 1000}},
            "startingSetup": {
                "id": "setup.test", "fort": "building.hall", "inventory": {"item.amber": 10},
                "units": [{"definition": "unit.settler", "offset": {"x": 0, "y": 4}}],
            },
        },
        "definitions": [
            definition("building.hall", "building", hall),
            definition("item.amber", "item", json!({"stackLimit": 100, "currency": true})),
            definition("item.charm", "item", json!({"stackLimit": 1, "itemTier": 1, "itemEffect": equipment})),
            definition("item.wood", "item", json!({"stackLimit": 100})),
            definition("unit.bandit", "unit", unit(json!({"movement": movement, "combat": combat, "campDefense": {}}))),
            definition("unit.brute", "unit", big),
            definition("unit.cart", "unit", cart),
            definition("unit.captain", "unit", captain),
            definition("unit.soldier", "unit", soldier),
            definition("unit.scout", "unit", unit(json!({"movement": movement, "playerControl": {}}))),
            definition("unit.hunter", "unit", hunter),
            definition("unit.marshal", "unit", hero),
            definition("unit.settler", "unit", unit(json!({"movement": movement, "work": {"carryCapacity": 1, "builds": []}}))),
            definition("unit.statue", "unit", unit(json!({}))),
            definition("unit.warrior", "unit", unit(json!({"movement": movement, "combat": combat}))),
        ],
    })
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let records = vec![
                canopy_content::testing::image("asset.icon"),
                canopy_content::testing::model("asset.model"),
            ];
            let mut folder = canopy_content::testing::DataFolder::new(game(), records);
            folder.effects = effects();
            Arc::new(
                folder
                    .load("sim-missions")
                    .unwrap_or_else(|errors| panic!("{errors}")),
            )
        })
        .clone()
}

/// A held glow (looping) and a burst (once) for `mission.effect_at`.
fn effects() -> Value {
    let light = |duration: u32| json!({"id": "light", "shape": "light", "colour": "#ffffff", "accent": "#ffffff", "durationTicks": duration, "count": 1, "size": 4, "height": 1});
    json!([
        {"id": "effect.glow", "name": "Glow", "durationTicks": 40, "loop": true, "layers": [light(40)]},
        {"id": "effect.burst", "name": "Burst", "durationTicks": 20, "loop": false, "layers": [light(20)]},
    ])
}

/// A placement owned by player 1.
pub fn unit(id: &str, definition: &str, x: f64, y: f64) -> Value {
    json!({"id": id, "definition": definition, "position": {"x": x, "y": y}, "owner": "player.1"})
}

/// A mission map under construction.
pub struct Mission {
    pub entities: Vec<Value>,
    pub camps: Vec<Value>,
    pub regions: Vec<Value>,
    pub objectives: Vec<Value>,
    pub script: String,
    /// Saved cameras (`mission.cameras`), when any.
    pub cameras: Vec<Value>,
}

impl Mission {
    /// The standard cast: a hero, a warrior, a neutral settler, a hall, an immobile statue and
    /// a scripted warrior not yet spawned.
    pub fn new(script: &str) -> Self {
        Self {
            entities: vec![
                unit("hero", "unit.marshal", 60.0, 60.0),
                unit("guard", "unit.warrior", 63.0, 60.0),
                json!({"id": "volunteer", "definition": "unit.settler", "position": {"x": 64, "y": 62}, "owner": "none"}),
                json!({"id": "hall", "definition": "building.hall", "position": {"x": 41.5, "y": 41.5}, "owner": "player.1", "initialState": {"construction": "complete", "inventory": {"item.amber": 25}}}),
                unit("statue", "unit.statue", 66.0, 60.0),
                json!({"id": "sleeper", "definition": "unit.warrior", "position": {"x": 90, "y": 90}, "owner": "none", "activation": "script"}),
            ],
            camps: Vec::new(),
            regions: vec![json!({"id": "gate", "x": 60, "y": 64, "radius": 4})],
            objectives: vec![
                json!({"id": "reach", "title": "Reach", "description": "Reach the gate"}),
                json!({"id": "hold", "title": "Hold", "description": "Hold the gate"}),
                json!({"id": "loot", "title": "Loot", "description": "Find the charm", "optional": true}),
            ],
            script: script.into(),
            cameras: Vec::new(),
        }
    }

    pub fn camera(mut self, camera: Value) -> Self {
        self.cameras.push(camera);
        self
    }

    pub fn entity(mut self, placement: Value) -> Self {
        self.entities.push(placement);
        self
    }

    pub fn camp(mut self, camp: Value) -> Self {
        self.camps.push(camp);
        self
    }

    pub fn map(&self) -> UtcMap {
        let mut document = map_fixture::map();
        // The shared map fixture places a scenery rock this content doesn't publish.
        map_fixture::set(&mut document, "/authoring/objects", json!([]));
        map_fixture::set(&mut document, "/entities", json!(self.entities));
        map_fixture::set(&mut document, "/camps", json!(self.camps));
        // The cast is placed by the map; player 2 has a start but no seat, so no base.
        let second = document["playerStarts"][1].clone();
        map_fixture::set(&mut document, "/playerStarts", json!([second]));
        map_fixture::set(
            &mut document,
            "/mission",
            json!({"objectives": self.objectives, "regions": self.regions, "script": self.script}),
        );
        if !self.cameras.is_empty() {
            map_fixture::set(&mut document, "/mission/cameras", json!(self.cameras));
        }
        map_fixture::parse(document).unwrap_or_else(|errors| panic!("{errors}"))
    }

    pub fn world(&self) -> World {
        World::new(content(), &self.map(), setup()).unwrap_or_else(|e| panic!("{e}"))
    }
}

pub fn setup() -> MatchSetup {
    MatchSetup {
        seed: 7,
        slots: vec![Slot {
            difficulty: Default::default(),
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    }
}

/// A world on the standard cast running `script`.
pub fn world(script: &str) -> World {
    Mission::new(script).world()
}

pub fn steps(world: &mut World, ticks: u32) {
    for _ in 0..ticks {
        world.step();
    }
}

pub fn mission(world: &World) -> &MissionState {
    world.state().mission.as_ref().expect("a mission")
}

pub fn error(world: &World) -> Option<&str> {
    mission(world).error.as_deref()
}

/// Runs `on_start` and fails on a script error.
pub fn started(script: &str) -> World {
    let mut world = world(script);
    world.step();
    assert_eq!(error(&world), None);
    world
}

pub fn variable(world: &World, key: &str) -> Option<Variable> {
    mission(world).variables.get(key).cloned()
}

pub fn int(value: i64) -> Option<Variable> {
    Some(Variable::Integer(value))
}

pub fn boolean(value: bool) -> Option<Variable> {
    Some(Variable::Boolean(value))
}

pub fn string(value: &str) -> Option<Variable> {
    Some(Variable::String(value.into()))
}

pub fn entity<'a>(world: &'a World, placement: &str) -> Option<&'a canopy_sim::entity::Entity> {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement))
}

/// A save round trip through JSON on the same map.
pub fn reload(world: &World, mission: &Mission) -> World {
    let text = serde_json::to_string(&world.save()).unwrap();
    World::restore(
        content(),
        &mission.map(),
        serde_json::from_str(&text).unwrap(),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}
