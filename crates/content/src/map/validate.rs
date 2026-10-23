//! Map rules that need the content: the old `validatePlacements` (src/content/map.ts) and
//! the non-spatial rules of `playableMapError` (src/shared/map/playable.ts).
//!
//! Rules that need navigation or footprint geometry, the terrain projection or the Lua
//! compiler are the simulation's: placement geometry, building footprints inside the map,
//! walk-surface floors, starting areas, resource clearance, occupancy, resources projected
//! from scenery objects, and the mission script's syntax.

use super::UtcMap;
use crate::{
    Content,
    error::Error,
    game::{Definition, Kind, Rules, StartingSetup},
    placement::{InitialConstruction, InitialState, Owner, Placement, Point},
};
use std::{borrow::Cow, collections::BTreeSet};

/// A placement the map declares, directly or through a player start.
struct Declared<'a> {
    placement: Cow<'a, Placement>,
    /// Starting units are positioned by their formation, which needs the terrain; their
    /// positions here are placeholders.
    positioned: bool,
}

struct Validator<'a> {
    map: &'a UtcMap,
    content: &'a Content,
    errors: Vec<Error>,
}

impl<'a> Validator<'a> {
    fn fail(&mut self, location: impl Into<String>, message: impl Into<String>) {
        let file = self.map.name.clone();
        self.errors.push(Error::new(&file, location, message));
    }

    fn definition(&mut self, location: &str, id: &str) -> Option<&'a Definition> {
        let found = self.content.definition(id);
        if found.is_none() {
            self.fail(location, format!("Unknown definition: {id}"));
        }
        found
    }

    fn has_start(&self, owner: Owner) -> bool {
        self.map.player_starts.iter().any(|s| s.owner() == owner)
    }
}

/// The starting setup of a race (the default race when `None`), as the old
/// `raceDefinition` resolves it.
fn race_setup<'r>(rules: &'r Rules, race: Option<&str>) -> Result<&'r StartingSetup, String> {
    let key = race.unwrap_or(&rules.default_race);
    match rules.races.get(key) {
        Some(race) => Ok(race
            .starting_setup
            .as_ref()
            .unwrap_or(&rules.starting_setup)),
        None if key == rules.default_race && rules.races.is_empty() => Ok(&rules.starting_setup),
        None => Err(format!("Unknown race: {key}")),
    }
}

/// Checks `map` against `content`. Errors name the map by its `name`.
pub fn validate(map: &UtcMap, content: &Content) -> Vec<Error> {
    let mut v = Validator {
        map,
        content,
        errors: Vec::new(),
    };
    for (at, message) in super::players::validate(map, content) {
        v.fail(at, message);
    }
    let declared = declared_placements(&mut v);
    placements(&mut v, &declared);
    camps(&mut v, &declared);
    main_forts(&mut v, &declared);
    playable(&mut v);
    for (i, id) in map.music.iter().flatten().enumerate() {
        match content.sounds.get(id.as_str()) {
            Some(e) if e.category == crate::sounds::SoundCategory::Music => {}
            Some(_) => v.fail(format!("music[{i}]"), format!("{id} is no music")),
            None => v.fail(format!("music[{i}]"), format!("unknown sound {id}")),
        }
    }
    v.errors
}

/// The map's entities plus each player start's fort and starting units (skirmish and
/// mission maps; a mission's players may also start with only what the map places).
fn declared_placements<'a>(v: &mut Validator<'a>) -> Vec<Declared<'a>> {
    let map = v.map;
    let rules = &v.content.rules;
    let mut declared: Vec<Declared> = map
        .entities
        .iter()
        .map(|p| Declared {
            placement: Cow::Borrowed(p),
            positioned: true,
        })
        .collect();
    if map.sandbox {
        return declared;
    }
    // Without chosen slots every start uses the default race.
    let setup = match race_setup(rules, None) {
        Ok(setup) => Some(setup),
        Err(message) => {
            v.fail("playerStarts", message);
            None
        }
    };
    for start in &map.player_starts {
        let known = rules.starting_setup.id.as_str() == start.setup
            || rules.races.values().any(|r| {
                r.starting_setup
                    .as_ref()
                    .is_some_and(|s| s.id.as_str() == start.setup)
            });
        if !known {
            v.fail(
                "playerStarts",
                format!("Player {}: unknown setup {}", start.player, start.setup),
            );
        }
        let Some(setup) = setup else {
            continue;
        };
        let prefix = format!("start.player.{}", start.player);
        let owner = start.owner();
        declared.push(Declared {
            placement: Cow::Owned(Placement {
                activation: None,
                id: format!("{prefix}/main-fort"),
                map_knowledge: None,
                definition: setup.fort.clone(),
                position: Point {
                    x: start.x,
                    y: start.z,
                    surface: None,
                },
                rotation: f64::from(start.rotation.unwrap_or(0)),
                owner,
                initial_state: Some(InitialState {
                    construction: Some(InitialConstruction::Complete),
                    inventory: Some(setup.inventory.clone()),
                    ..InitialState::default()
                }),
                appearance: None,
            }),
            positioned: true,
        });
        let hero = setup.hero.as_ref().map(|h| &h.default);
        let units = setup.units.iter().map(|u| &u.definition).chain(hero);
        for (i, definition) in units.enumerate() {
            declared.push(Declared {
                placement: Cow::Owned(Placement {
                    activation: None,
                    id: format!("{prefix}/unit-{:03}", i + 1),
                    map_knowledge: None,
                    definition: definition.clone(),
                    position: Point {
                        x: start.x,
                        y: start.z,
                        surface: None,
                    },
                    rotation: 0.0,
                    owner,
                    initial_state: None,
                    appearance: None,
                }),
                positioned: false,
            });
        }
    }
    declared
}

fn placements(v: &mut Validator, declared: &[Declared]) {
    let map = v.map;
    let size = f64::from(map.size);
    let mut ids = BTreeSet::new();
    for entry in declared {
        let p = entry.placement.as_ref();
        let at = p.id.as_str();
        if !ids.insert(at) {
            v.fail(at, format!("Duplicate placement {at}"));
        }
        if p.activation.is_some() && map.mission.is_none() {
            v.fail(at, "scripted spawns require a mission map");
        }
        // A mission's players need no start: they begin with what the map places.
        if p.owner != Owner::Nobody && !v.has_start(p.owner) && map.mission.is_none() {
            v.fail(at, "missing owner slot");
        }
        if entry.positioned && (p.position.x >= size || p.position.y >= size) {
            v.fail(at, "outside map bounds");
        }
        let Some(d) = v.definition(at, p.definition.as_str()) else {
            continue;
        };
        if d.currency == Some(true) {
            v.fail(at, "currencies cannot be placed on the ground");
        }
        if d.harvesting.is_some() && p.owner != Owner::Nobody {
            v.fail(at, "mines must be neutral");
        }
        if p.position.surface.is_some() && matches!(d.kind, Kind::Building | Kind::Resource) {
            v.fail(
                at,
                "buildings and harvestable resources currently require ground",
            );
        }
        if d.kind == Kind::Building && (p.rotation / 90.0).fract() != 0.0 {
            v.fail(at, "buildings rotate in quarter turns");
        }
        if let Some(state) = &p.initial_state {
            initial_state(v, at, d, state);
        }
        if let Some(asset_id) = p.appearance.as_ref().and_then(|a| a.asset.as_ref()) {
            match v.content.asset(asset_id.as_str()) {
                None => v.fail(at, format!("Unknown asset: {asset_id}")),
                Some(asset) => {
                    if asset.file.is_none() {
                        v.fail(at, "appearance must reference a model");
                    }
                    let scenery = asset.scenery_asset.is_some();
                    if d.kind == Kind::Resource && !scenery {
                        v.fail(at, "resource appearance needs a scenery asset");
                    }
                    if d.kind != Kind::Resource && scenery {
                        v.fail(at, "batched scenery appearance requires a resource");
                    }
                }
            }
        }
    }
}

fn initial_state(v: &mut Validator, at: &str, d: &Definition, state: &InitialState) {
    let levels = d.behaviors.progression.as_ref().map(|p| &p.levels);
    if let Some(experience) = state.experience {
        // A campaign's hero level cap applies when the mission is played in it.
        let within = levels
            .and_then(|levels| levels.last())
            .is_some_and(|top| experience <= top.experience);
        if !within {
            v.fail(at, "initial experience exceeds hero or mission limit");
        }
    }
    let level = levels.and_then(|levels| {
        levels
            .iter()
            .rfind(|l| l.experience <= state.experience.unwrap_or(0))
    });
    if let Some(health) = state.health {
        let valid = d
            .body
            .as_ref()
            .is_some_and(|body| health <= level.map_or(body.max_hp, |l| l.max_hp));
        if !valid {
            v.fail(at, "invalid initial health");
        }
    }
    if let Some(amount) = state.amount
        && d.resource_yield.is_none_or(|y| y == 0 || amount > y)
    {
        v.fail(at, "invalid initial yield");
    }
    if let Some(quantity) = state.quantity
        && (d.kind != Kind::Item || d.stack_limit.is_some_and(|limit| quantity > limit))
    {
        v.fail(at, "invalid loose stack quantity");
    }
    if state.construction.is_some() && d.kind != Kind::Building {
        v.fail(at, "construction on non-building");
    }
    if state.loot_pool.is_some() && d.breakable.is_none() {
        v.fail(at, "only breakables roll a loot pool");
    }
    if let Some(pool) = &state.loot_pool
        && !v.content.rules.loot_pools.contains_key(pool)
    {
        v.fail(at, format!("unknown loot pool {pool}"));
    }
    if let Some(wares) = &state.wares {
        if d.shop.is_none() {
            v.fail(at, "only shops sell wares");
        }
        if let Some(problem) = crate::registry::ware_problem(v.content, wares) {
            v.fail(at, problem);
        }
    }
    if let (Some(inventory), Some(_)) = (&state.inventory, &d.breakable) {
        for item in inventory.keys() {
            if !crate::registry::hero_item(v.content, item.as_str()) {
                v.fail(at, format!("a breakable holds hero items: {item}"));
            }
        }
        return;
    }
    if let Some(inventory) = &state.inventory {
        let Some(storage) = &d.behaviors.storage else {
            v.fail(at, "no storage");
            return;
        };
        let content = v.content;
        let produced_items = d
            .behaviors
            .production
            .iter()
            .flat_map(|p| &p.outputs)
            .filter(|id| {
                content
                    .definition(id.as_str())
                    .is_some_and(|o| o.kind == Kind::Item)
            });
        let legal: BTreeSet<_> = storage.accepts.iter().chain(produced_items).collect();
        for (item, &count) in inventory {
            if count != 0 && !legal.contains(item) {
                v.fail(at, format!("cannot store {item}"));
            }
        }
        if inventory.values().map(|&n| u64::from(n)).sum::<u64>() > u64::from(storage.capacity) {
            v.fail(at, "initial inventory exceeds capacity");
        }
    }
}

const MOST_LEGENDARY_REWARDS: u32 = 3;

fn camps(v: &mut Validator, declared: &[Declared]) {
    let map = v.map;
    let rules = &v.content.rules;
    let mut members = BTreeSet::new();
    let mut camp_ids = BTreeSet::new();
    let mut legendary_rewards = 0u32;
    let mut too_many_reported = false;
    let mut award = |v: &mut Validator, at: &str, legendary: bool, count: u32| {
        if !legendary {
            v.fail(at, "T3 loot requires a legendary camp");
        }
        legendary_rewards += count;
        if legendary_rewards > MOST_LEGENDARY_REWARDS && !too_many_reported {
            too_many_reported = true;
            v.fail(
                at,
                "Maps may award at most three legendary items from camps",
            );
        }
    };
    for camp in &map.camps {
        let at = camp.id.as_str();
        let legendary = camp.legendary == Some(true);
        let pool = camp.loot_pool.as_ref().and_then(|id| {
            let pool = rules.loot_pools.get(id);
            if pool.is_none() {
                v.fail(at, format!("unknown loot pool {id}"));
            }
            pool
        });
        for id in camp.fixed_drops.iter().flatten() {
            let Some(item) = v.definition(at, id.as_str()) else {
                continue;
            };
            if item.kind != Kind::Item {
                v.fail(at, format!("fixed drop must be an item: {id}"));
            }
            if item.item_tier == Some(3) {
                award(v, at, legendary, 1);
            }
        }
        if let Some(pool) = pool {
            let content = v.content;
            let legendary_pool = pool
                .entries
                .iter()
                .filter_map(|e| e.item.as_ref())
                .any(|id| {
                    content
                        .definition(id.as_str())
                        .is_some_and(|d| d.item_tier == Some(3))
                });
            if legendary_pool {
                award(v, at, legendary, pool.rolls);
            }
        }
        if !camp_ids.insert(at) {
            v.fail(at, "Duplicate camp ID");
        }
        for id in &camp.members {
            let valid = declared
                .iter()
                .map(|e| e.placement.as_ref())
                .find(|p| p.id == *id)
                .is_some_and(|p| {
                    p.owner == Owner::Nobody
                        && v.content
                            .definition(p.definition.as_str())
                            .is_some_and(|d| d.behaviors.camp_defense.is_some())
                })
                && !members.contains(id.as_str());
            if !valid {
                v.fail(at, format!("invalid/duplicate camp member {id}"));
            }
            members.insert(id.as_str());
        }
    }
    for entry in declared {
        let p = entry.placement.as_ref();
        let defends = v
            .content
            .definition(p.definition.as_str())
            .is_some_and(|d| d.behaviors.camp_defense.is_some());
        if defends && !members.contains(p.id.as_str()) {
            v.fail(&p.id, "camp defense requires an authored camp");
        }
    }
}

fn main_forts(v: &mut Validator, declared: &[Declared]) {
    let map = v.map;
    if map.sandbox {
        return;
    }
    for start in &map.player_starts {
        let valid = declared
            .iter()
            .map(|e| e.placement.as_ref())
            .find(|p| p.id == start.main_fort)
            .is_some_and(|p| {
                p.owner == start.owner()
                    && v.content
                        .definition(p.definition.as_str())
                        .is_some_and(|d| d.kind == Kind::Building)
            });
        if !valid {
            v.fail(
                "playerStarts",
                format!("Player {}: invalid main fort objective", start.player),
            );
        }
    }
}

/// Where players may start, before any terrain is involved.
fn playable(v: &mut Validator) {
    let map = v.map;
    let starts = &map.player_starts;
    let size = f64::from(map.size);
    // A mission may start every player with only what the map places for them.
    if starts.is_empty() && map.mission.is_none() {
        v.fail("playerStarts", "Place a player start.");
    }
    if let Some(mission) = &map.mission
        && mission
            .regions
            .iter()
            .any(|r| r.x >= map.size || r.y >= map.size)
    {
        v.fail("mission.regions", "Mission region outside map.");
    }
    if let Some(mission) = &map.mission {
        let cameras = mission.cameras.as_deref().unwrap_or_default();
        for (i, camera) in cameras.iter().enumerate() {
            if let Some(problem) = camera.problem(map.size) {
                v.fail("mission.cameras", problem);
            }
            if cameras[..i].iter().any(|c| c.id == camera.id) {
                v.fail(
                    "mission.cameras",
                    format!("Camera {} is defined twice", camera.id),
                );
            }
        }
    }
    if map.mission.is_none()
        && !map.sandbox
        && !(v.has_start(Owner::Player(0)) && v.has_start(Owner::Player(1)))
    {
        v.fail("playerStarts", "Place Player 1 and Player 2 start points.");
    }
    if map.mission.is_none()
        && (1..=starts.len()).any(|n| !starts.iter().any(|s| usize::from(s.player) == n))
    {
        v.fail(
            "playerStarts",
            "Player slots must be consecutive, starting at Player 1.",
        );
    }
    let area = map.playable_area();
    let (x0, z0) = (f64::from(area.x), f64::from(area.z));
    let (x1, z1) = (
        f64::from(area.x + area.width),
        f64::from(area.z + area.height),
    );
    let _ = size;
    for s in starts {
        if s.x < x0 + 8.0 || s.z < z0 + 8.0 || s.x > x1 - 9.0 || s.z > z1 - 9.0 {
            v.fail(
                "playerStarts",
                format!(
                    "Player {} needs a building-grid-aligned start at least 8 navigation cells inside the map.",
                    s.player
                ),
            );
        }
    }
    let crowded = starts.iter().any(|s| {
        starts
            .iter()
            .any(|t| t.player != s.player && (t.x - s.x).hypot(t.z - s.z) < 20.0)
    });
    if crowded {
        v.fail(
            "playerStarts",
            "Player starts must be at least 20 cells apart.",
        );
    }
}
