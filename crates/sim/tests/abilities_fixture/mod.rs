//! Test-owned content for the ability and item tests: a hero and a caster whose bindings
//! each test chooses, fighters, summonable units, a flyer, a camp creature and items. No
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
    abilities::{AbilityEvent, EventKind},
    entity::{Entity, EntityId},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub const SIZE: u32 = 256;
const HALO: i32 = 16;

pub fn definition(id: &str, kind: &str, extra: Value) -> Value {
    let mut d = json!({"id": id, "kind": kind, "name": id, "description": "", "asset": "asset.model", "icon": "asset.icon"});
    for (k, v) in extra.as_object().expect("object") {
        d[k] = v.clone();
    }
    d
}

/// A test item with the given effect.
pub fn item(id: &str, effect: Value) -> Value {
    definition(
        id,
        "item",
        json!({"stackLimit": 1, "itemTier": 1, "itemEffect": effect}),
    )
}

fn weapon(damage: u32, kind: &str, range: f64, cooldown: u32, aggro: u32) -> Value {
    json!({"damage": damage, "damageType": kind, "range": range, "cooldownTicks": cooldown, "aggroRange": aggro,
        "attack": {"windupTicks": 10, "recoveryTicks": 10, "rangeBuffer": 0.6}})
}

fn level(
    experience: u32,
    max_hp: u32,
    damage: u32,
    max_mana: u32,
    health_regen: f64,
    mana_regen: f64,
) -> Value {
    json!({"experience": experience, "maxHp": max_hp, "damage": damage, "armor": 2, "cooldownTicks": 40,
        "maxMana": max_mana, "healthRegenPerSecond": health_regen, "manaRegenPerSecond": mana_regen})
}

/// What a test adds to the content.
#[derive(Default, Clone)]
pub struct Spec {
    /// Ability definitions (presentations are generated).
    pub abilities: Vec<Value>,
    /// Bindings of `unit.hero`, `unit.caster`, `unit.critter` (neutral) and `unit.imp`.
    pub hero: Vec<Value>,
    pub caster: Vec<Value>,
    pub critter: Vec<Value>,
    pub imp: Vec<Value>,
    pub warrior: Vec<Value>,
    pub archer: Vec<Value>,
    /// Extra definitions (items, units).
    pub definitions: Vec<Value>,
}

/// A binding with every control.
pub fn bind(id: &str, ability: &str, rank: u32) -> Value {
    json!({"id": id, "ability": ability, "initialRank": rank, "controls": ["player", "ai", "scenario"]})
}

/// A hero binding learned by level.
pub fn learnable(id: &str, ability: &str, levels: &[u32]) -> Value {
    json!({"id": id, "ability": ability, "initialRank": 0, "controls": ["player", "ai", "scenario"],
        "learning": {"requiredLevels": levels}})
}

fn caster(max_mana: u32, regen: f64, bindings: &[Value]) -> Value {
    json!({"maxMana": max_mana, "manaRegenPerSecond": regen, "bindings": bindings})
}

pub fn game(spec: &Spec) -> Value {
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
    .map(|n| (n.to_string(), meta.clone()))
    .collect();
    let control = json!({});
    let unit = |radius: f64, hp: u32, armor: u32, behaviors: Value| {
        json!({"body": {"maxHp": hp, "armor": armor, "armorType": "armor.light"}, "supplyCost": 1, "vision": 16,
            "dimensions": {"radius": radius, "height": 1.5, "formationSpacing": 1.2}, "behaviors": behaviors})
    };
    let mut hero = unit(
        0.5,
        300,
        2,
        json!({"playerControl": control, "movement": {"speed": 4},
        "combat": weapon(12, "damage.blade", 0.6, 40, 12),
        "progression": {"levels": [level(0, 300, 12, 200, 1.0, 2.0), level(100, 360, 15, 240, 1.5, 2.5), level(250, 420, 18, 280, 2.0, 3.0), level(500, 480, 21, 320, 2.5, 3.5)], "experienceRadius": 12},
        "inventory": {"slots": 6, "pickupRange": 1.5}}),
    );
    hero["hero"] = json!(true);
    if spec.hero.is_empty() {
        for level in hero["behaviors"]["progression"]["levels"]
            .as_array_mut()
            .expect("levels")
        {
            level["maxMana"] = json!(0);
            level["manaRegenPerSecond"] = json!(0);
        }
    } else {
        hero["behaviors"]["abilities"] = caster(200, 2.0, &spec.hero);
    }
    let with_caster = |mut d: Value, bindings: &[Value], mana: u32| {
        if !bindings.is_empty() {
            d["behaviors"]["abilities"] = caster(mana, 2.0, bindings);
        }
        d
    };
    let caster_unit = with_caster(
        unit(
            0.4,
            150,
            0,
            json!({"playerControl": control, "movement": {"speed": 4}, "combat": weapon(6, "damage.blade", 0.6, 40, 10)}),
        ),
        &spec.caster,
        300,
    );
    let warrior = with_caster(
        unit(
            0.4,
            100,
            2,
            json!({"playerControl": control, "movement": {"speed": 4}, "combat": weapon(10, "damage.blade", 0.6, 40, 12)}),
        ),
        &spec.warrior,
        100,
    );
    let archer = with_caster(
        unit(
            0.4,
            70,
            0,
            json!({"playerControl": control, "movement": {"speed": 4},
            "combat": {"damage": 8, "damageType": "damage.pierce", "range": 10.0, "cooldownTicks": 50, "aggroRange": 14,
                "attack": {"windupTicks": 16, "recoveryTicks": 10, "rangeBuffer": 0.75}, "projectile": {"speed": 20}}}),
        ),
        &spec.archer,
        100,
    );
    let mut imp = with_caster(
        unit(
            0.4,
            60,
            0,
            json!({"playerControl": control, "movement": {"speed": 5}, "combat": weapon(5, "damage.blade", 0.6, 30, 10)}),
        ),
        &spec.imp,
        100,
    );
    imp["supplyCost"] = json!(0);
    let mut sprite = unit(
        0.3,
        40,
        0,
        json!({"playerControl": control, "movement": {"speed": 5}, "combat": weapon(4, "damage.blade", 0.6, 30, 10)}),
    );
    sprite["supplyCost"] = json!(0);
    let mut wasp = unit(
        0.4,
        60,
        0,
        json!({"playerControl": control, "movement": {"speed": 6, "locomotion": "air", "flightHeight": 3},
        "combat": weapon(6, "damage.blade", 0.6, 30, 10)}),
    );
    wasp["supplyCost"] = json!(1);
    let mut golem = unit(
        0.5,
        200,
        4,
        json!({"playerControl": control, "movement": {"speed": 3}, "combat": weapon(14, "damage.blade", 0.6, 50, 10)}),
    );
    golem["unitNature"] = json!("mechanical");
    let critter = with_caster(
        unit(
            0.4,
            80,
            0,
            json!({"movement": {"speed": 4}, "combat": weapon(6, "damage.blade", 0.6, 40, 8), "campDefense": {}}),
        ),
        &spec.critter,
        100,
    );
    let building = |hp: u32, extra: Value| {
        let mut b = json!({"body": {"maxHp": hp, "armor": 0, "armorType": "armor.structure"}, "footprint": {"width": 4, "depth": 4},
            "entrance": {"x": 0.5, "y": 2.5}, "vision": 10});
        for (k, v) in extra.as_object().expect("object") {
            b[k] = v.clone();
        }
        b
    };
    let trinket = json!({"stackLimit": 1, "itemTier": 1, "itemEffect": {"type": "equipment", "damage": 1, "armor": 0, "maxHp": 0}});
    let mut definitions = vec![
        definition(
            "building.barracks",
            "building",
            building(
                500,
                json!({
            "behaviors": {"production": {"mode": "queued", "outputs": ["unit.warrior"], "workerSlots": 0, "queueCapacity": 5}},
            "creation": {"method": "construct", "items": [], "workTicks": 100}}),
            ),
        ),
        definition(
            "building.hall",
            "building",
            building(
                1000,
                json!({"heroCapacity": 2, "supplyProvided": 200,
            "behaviors": {"storage": {"capacity": 10, "accepts": ["item.trinket"]}}}),
            ),
        ),
        definition("item.trinket", "item", trinket),
        definition("unit.archer", "unit", archer),
        definition("unit.caster", "unit", caster_unit),
        definition("unit.critter", "unit", critter),
        definition("unit.golem", "unit", golem),
        definition("unit.hero", "unit", hero),
        definition("unit.imp", "unit", imp),
        definition("unit.sprite", "unit", sprite),
        definition("unit.warrior", "unit", {
            let mut w = warrior;
            w["creation"] = json!({"method": "train", "items": [], "workTicks": 40});
            w
        }),
        definition("unit.wasp", "unit", wasp),
        definition(
            "unit.worker",
            "unit",
            unit(
                0.4,
                50,
                0,
                json!({"playerControl": control, "movement": {"speed": 4},
            "work": {"carryCapacity": 1, "builds": ["building.barracks"]}}),
            ),
        ),
    ];
    definitions.extend(spec.definitions.iter().cloned());
    definitions.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
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
            "research": {}, "lootPools": {},
            "armorCoefficient": 0.06,
            "damageTypes": {
                "damage.blade": {"name": "Blade", "appliesArmor": true},
                "damage.pierce": {"name": "Pierce", "appliesArmor": true},
                "spell": {"name": "Spell", "appliesArmor": false},
            },
            "heroStunDurationPermille": 500,
            "experience": {"unitRewards": [20, 40, 60], "heroRewards": [100, 150, 200], "neutralLevelCap": 3,
                "neutralMultipliersPermille": [1000, 700], "summonMultiplierPermille": 500, "globalFallback": true,
                "soloHeroTierBonusPermille": [0, 100, 250]},
            "damageMultipliers": {
                "damage.blade": {"armor.light": 1000, "armor.heavy": 600, "armor.structure": 400},
                "damage.pierce": {"armor.light": 1200, "armor.heavy": 500, "armor.structure": 300},
                "spell": {"armor.light": 1000, "armor.heavy": 1000, "armor.structure": 500},
            },
            "startingSetup": {"id": "setup.test", "fort": "building.hall", "inventory": {}, "units": []},
            "ai": {
                "revision": 1, "decisionTicks": 1, "economyTicks": 1, "strategyTicks": 1, "operationTicks": 1, "reactionTicks": 1, "orderIntervalTicks": 1,
                "workers": {"minimum": 1, "target": 2, "maximum": 3, "reserve": 0},
                "army": {"minimum": 1, "maximum": 2, "homeGuard": 0, "engagePermille": 1, "campPermille": 1, "retreatHealthPermille": 1, "pursuitRadius": 1},
                "limits": {"commandsPerBeat": 1, "placementCandidates": 1, "spellCandidates": 1},
                "composition": [{"definition": "unit.warrior", "weight": 1}], "skillPreference": [],
            },
        },
        "definitions": definitions,
    })
}

/// The ability library of a spec, with a minimal presentation per ability.
pub fn library(spec: &Spec) -> Value {
    let presentations: Vec<Value> = spec
        .abilities
        .iter()
        .map(|a| {
            json!({"schemaVersion": 1, "id": a["presentation"],
                "animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": []})
        })
        .collect();
    json!({"schemaVersion": 1, "abilities": spec.abilities, "presentations": presentations})
}

pub fn records() -> Vec<Value> {
    vec![image("asset.icon"), model("asset.model")]
}

/// Loads the content of a spec (a fresh data folder per name).
pub fn content(name: &str, spec: &Spec) -> Arc<Content> {
    let mut folder = DataFolder::new(game(spec), records());
    folder.abilities = library(spec);
    Arc::new(
        folder
            .load(name)
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

fn terrain() -> Value {
    let span = (SIZE + 2 * HALO as u32) as usize;
    let flow: Vec<u8> = (0..span * span).flat_map(|_| [128, 128, 0, 0]).collect();
    let zero = grid(|_, _| 0.0);
    json!({"version": 1, "size": SIZE, "heights": zero.clone(), "waterHeights": grid(|_, _| -1.0),
        "waterFlow": base64(&flow), "waterProfiles": [], "grass": zero.clone(), "rock": zero, "paint": []})
}

/// A placement (player 1-based; 0 for neutral).
pub fn placed(id: &str, definition: &str, x: f64, y: f64, player: u8) -> Value {
    let owner = if player == 0 {
        "none".to_string()
    } else {
        format!("player.{player}")
    };
    json!({"id": id, "definition": definition, "position": {"x": x, "y": y}, "rotation": 0, "owner": owner})
}

/// A sandbox map document with the given entities and camps.
pub fn document(entities: Vec<Value>, camps: Vec<Value>) -> Value {
    json!({
        "v": 3, "size": SIZE, "name": "Ability fixture", "stamps": [], "sandbox": true,
        "playerStarts": [{"player": 1, "x": 13.5, "z": 13.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"}],
        "entities": entities, "camps": camps,
        "authoring": {"version": 1, "terrain": terrain(), "objects": []},
    })
}

pub fn map(entities: Vec<Value>) -> UtcMap {
    map_with(entities, vec![])
}

pub fn map_with(entities: Vec<Value>, camps: Vec<Value>) -> UtcMap {
    UtcMap::from_value(document(entities, camps), "fixture.utcmap")
        .unwrap_or_else(|e| panic!("{e}"))
}

/// A camp of neutral members.
pub fn camp(id: &str, members: &[&str], home: (f64, f64)) -> Value {
    json!({"id": id, "members": members, "home": {"x": home.0, "y": home.1}, "aggroRange": 8, "leash": 20, "aggression": "players"})
}

/// Two players on separate teams (`allied` puts them on one).
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

pub fn world(name: &str, spec: &Spec, entities: Vec<Value>) -> World {
    World::new(content(name, spec), &map(entities), setup(false)).unwrap_or_else(|e| panic!("{e}"))
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

pub fn mana(world: &World, placement: &str) -> u32 {
    find(world, placement)
        .and_then(|e| e.caster.as_ref())
        .map_or(0, |c| c.mana)
}

pub fn alive(world: &World, placement: &str) -> bool {
    find(world, placement).is_some_and(|e| e.hp.is_some_and(|hp| hp > 0))
}

pub fn get(world: &World, id: EntityId) -> &Entity {
    world.state().entities.get(id).expect("entity")
}

/// Status names on an entity.
pub fn statuses(world: &World, placement: &str) -> Vec<String> {
    let Some(e) = find(world, placement) else {
        return Vec::new();
    };
    e.statuses
        .iter()
        .map(|s| world.catalog().spells.status(s.key).name.clone())
        .collect()
}

/// Issues a command for `player` (1-based), applied at the next step.
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
        .expect("scheduled");
}

/// Casts `binding` of `actor` at a unit.
pub fn cast_at(world: &mut World, player: u8, actor: u32, binding: &str, target: u32) {
    command(
        world,
        player,
        json!({"type": "castAbility", "actor": actor, "binding": binding, "target": {"kind": "unit", "entity": target}}),
    );
}

/// Casts `binding` of `actor` at a cell.
pub fn cast_point(world: &mut World, player: u8, actor: u32, binding: &str, x: u32, y: u32) {
    command(
        world,
        player,
        json!({"type": "castAbility", "actor": actor, "binding": binding, "target": {"kind": "point", "position": {"x": x, "y": y}}}),
    );
}

pub fn run(world: &mut World, ticks: u32) {
    for _ in 0..ticks {
        world.step();
    }
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

pub fn events(world: &mut World) -> Vec<AbilityEvent> {
    world.drain_events()
}

pub fn player(n: u8) -> Owner {
    Owner::Player(n - 1)
}

/// A save round trip through JSON on the same map.
pub fn reload(world: &World, content: Arc<Content>, map: &UtcMap) -> World {
    let text = serde_json::to_string(&world.save()).unwrap();
    World::restore(content, map, serde_json::from_str(&text).unwrap())
        .unwrap_or_else(|e| panic!("{e}"))
}

// ----- ability builders ------------------------------------------------------------------

const REVALIDATE: [&str; 5] = ["caster", "target", "range", "visibility", "relationship"];

/// An active ability: `kind` targeting ("unit", "point", "self"), relations, range, mana,
/// cooldown, prepare and recover ticks, and the release program.
#[allow(clippy::too_many_arguments)]
pub fn spell(
    id: &str,
    kind: &str,
    relations: &[&str],
    range: u32,
    mana: u32,
    cooldown: u32,
    prepare: u32,
    recover: u32,
    on_release: Value,
) -> Value {
    json!({
        "schemaVersion": 1, "id": id, "name": id, "description": "", "activation": "targeted",
        "ranks": [{}],
        "targeting": {"kind": kind, "relations": relations, "allowSelf": kind == "self", "alive": true, "visible": true,
            "range": range, "outOfRange": "reject"},
        "cast": {"prepareTicks": prepare, "recoverTicks": recover,
            "cost": {"resource": "mana", "amount": mana, "commit": "release"},
            "cooldown": {"ticks": cooldown, "start": "release", "scope": "ability"},
            "cancelBeforeRelease": "refund", "revalidateAtRelease": REVALIDATE},
        "onRelease": on_release,
        "presentation": format!("presentation.{}", id.trim_start_matches("ability.")),
    })
}

/// A passive ability with a release program (for auras) or triggers.
pub fn passive(id: &str, on_release: Value) -> Value {
    json!({
        "schemaVersion": 1, "id": id, "name": id, "description": "", "activation": "passive",
        "ranks": [{}],
        "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
        "cast": {"prepareTicks": 0, "recoverTicks": 0,
            "cost": {"resource": "mana", "amount": 0, "commit": "release"},
            "cooldown": {"ticks": 0, "start": "release", "scope": "ability"},
            "cancelBeforeRelease": "refund", "revalidateAtRelease": REVALIDATE},
        "onRelease": on_release,
        "presentation": format!("presentation.{}", id.trim_start_matches("ability.")),
    })
}

/// Merges `extra` into `base` (shallow).
pub fn with(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().expect("object") {
        base[k] = v.clone();
    }
    base
}

// ----- scripted runs ---------------------------------------------------------------------

/// A scripted match: placements, commands `(tick, player, action)` and direct state edits
/// `(tick, placement, {"hp"|"mana"|"experience": n})` applied before that tick runs.
pub struct Script {
    pub name: &'static str,
    pub spec: Spec,
    pub entities: Vec<Value>,
    pub camps: Vec<Value>,
    pub ticks: u32,
    pub commands: Vec<(u32, u8, Value)>,
    pub mutations: Vec<(u32, &'static str, Value)>,
}

/// What a script left behind.
pub struct Played {
    pub world: World,
    /// Entity ids by placement, as created.
    pub ids: std::collections::BTreeMap<String, EntityId>,
    pub events: Vec<AbilityEvent>,
    /// Fingerprint after every tick.
    pub prints: Vec<u32>,
}

impl Script {
    pub fn new(
        name: &'static str,
        spec: Spec,
        entities: Vec<Value>,
        ticks: u32,
        commands: Vec<(u32, u8, Value)>,
    ) -> Self {
        Self {
            name,
            spec,
            entities,
            camps: Vec::new(),
            ticks,
            commands,
            mutations: Vec::new(),
        }
    }

    pub fn set(mut self, tick: u32, placement: &'static str, values: Value) -> Self {
        self.mutations.push((tick, placement, values));
        self
    }

    pub fn content(&self) -> Arc<Content> {
        content(self.name, &self.spec)
    }

    pub fn map(&self) -> UtcMap {
        map_with(self.entities.clone(), self.camps.clone())
    }

    pub fn play(&self) -> Played {
        self.play_reloading(None)
    }

    /// Plays the script, replacing the world by a save round trip after tick `reload`.
    pub fn play_reloading(&self, reload: Option<u32>) -> Played {
        let content = self.content();
        let map = self.map();
        let mut world =
            World::new(content.clone(), &map, setup(false)).unwrap_or_else(|e| panic!("{e}"));
        let ids = world
            .state()
            .entities
            .iter()
            .filter_map(|e| Some((e.placement.clone()?, e.id)))
            .collect();
        let mut events = Vec::new();
        let mut prints = Vec::new();
        for t in 0..self.ticks {
            for (tick, placement, values) in &self.mutations {
                if *tick == t {
                    mutate(&mut world, placement, values);
                }
            }
            for (tick, player, action) in &self.commands {
                if *tick == t {
                    command(&mut world, *player, action.clone());
                }
            }
            world.step();
            events.extend(world.drain_events());
            prints.push(world.fingerprint());
            if reload == Some(t) {
                world = self::reload(&world, content.clone(), &map);
            }
        }
        Played {
            world,
            ids,
            events,
            prints,
        }
    }
}

fn mutate(world: &mut World, placement: &str, values: &Value) {
    let id = EntityId(id(world, placement));
    let e = world.state_mut().entities.get_mut(id).expect("entity");
    if let Some(v) = values.get("hp") {
        e.hp = Some(v.as_u64().expect("hp") as u32);
    }
    if let Some(v) = values.get("mana") {
        e.caster.as_mut().expect("caster").mana = v.as_u64().expect("mana") as u32;
    }
    if let Some(v) = values.get("experience") {
        e.progression.as_mut().expect("progression").experience =
            v.as_u64().expect("experience") as u32;
    }
}

impl Played {
    /// A compact listing of the events, for assertion messages.
    pub fn dump(&self) -> String {
        self.events
            .iter()
            .map(|e| {
                format!(
                    "T{} {:?} {:?}>{:?} {:?} {:?} {:?}",
                    e.tick,
                    e.kind,
                    e.caster.0,
                    e.target.map(|t| t.0),
                    e.amount,
                    e.status,
                    e.reason
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Events of one kind, in order.
    pub fn of(&self, kind: EventKind) -> Vec<&AbilityEvent> {
        self.events.iter().filter(|e| e.kind == kind).collect()
    }

    pub fn count(&self, kind: EventKind) -> usize {
        self.events.iter().filter(|e| e.kind == kind).count()
    }

    /// Total amount of the events of one kind.
    pub fn total(&self, kind: EventKind) -> i64 {
        self.events
            .iter()
            .filter(|e| e.kind == kind)
            .filter_map(|e| e.amount)
            .sum()
    }

    pub fn errors(&self) -> Vec<String> {
        errors(&self.world)
    }

    pub fn hp(&self, placement: &str) -> u32 {
        hp(&self.world, placement)
    }

    pub fn mana(&self, placement: &str) -> u32 {
        mana(&self.world, placement)
    }

    pub fn alive(&self, placement: &str) -> bool {
        alive(&self.world, placement)
    }

    pub fn id(&self, placement: &str) -> EntityId {
        self.ids[placement]
    }

    pub fn entity(&self, placement: &str) -> &Entity {
        entity(&self.world, placement)
    }

    /// Events of `kind` aimed at `placement`.
    pub fn at(&self, kind: EventKind, placement: &str) -> Vec<&AbilityEvent> {
        let target = Some(self.id(placement));
        self.events
            .iter()
            .filter(|e| e.kind == kind && e.target == target)
            .collect()
    }
}

/// Unit ids follow placement ids in lexical order from 1.
pub fn cast(actor: u32, binding: &str, target: u32) -> Value {
    json!({"type": "castAbility", "actor": actor, "binding": binding, "target": {"kind": "unit", "entity": target}})
}

pub fn cast_at_point(actor: u32, binding: &str, x: u32, y: u32) -> Value {
    json!({"type": "castAbility", "actor": actor, "binding": binding, "target": {"kind": "point", "position": {"x": x, "y": y}}})
}

pub fn moving(actor: u32, x: u32, y: u32) -> Value {
    json!({"type": "move", "actors": [actor], "destination": {"x": x, "y": y}})
}

/// Binds an ability to the caster and adds it to the library.
pub fn give(spec: &mut Spec, ability: Value, binding: &str) {
    let id = ability["id"].as_str().expect("id").to_string();
    spec.abilities.push(ability);
    spec.caster.push(bind(binding, &id, 1));
}
