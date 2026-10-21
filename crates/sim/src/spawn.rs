//! Turning a map into initial placements, checking them against the terrain, and turning
//! placements into entities (old `expandMap`/`validatePlacements`/`placementOccupancyError`/
//! `startingResourceClearanceError` in `src/content/map.ts`, `src/content/startingHero.ts`,
//! and the terrain rules of `playableMapError` in `src/shared/map/playable.ts`).

use crate::{
    catalog::{Catalog, Def},
    entity::{Construction, Entity, EntityId, LooseItem, Milli, Resource, Unit},
    setup::{MatchSetup, Slot, SlotKind},
    state::State,
    terrain::{
        CellBounds, Terrain,
        footprint::{
            Bounds, Extent, body_overlaps_bounds, footprint_cells, half_extents,
            placement_geometry_error, resource_center_separation,
        },
        round_half_up,
    },
};
use canopy_content::{
    Content, Id,
    conditions::Locomotion,
    game::{Creation, Definition, Kind, Rules, StartingSetup},
    map::{PlayerStart, UtcMap},
    placement::{Activation, InitialConstruction, InitialState, Owner, Placement, Point},
};
use std::collections::{BTreeMap, BTreeSet};

/// The placements a match starts with.
#[derive(Debug, Clone, Default)]
pub struct Placements {
    /// In placement-id order: the map's own entities, each player's starting setup (fort
    /// and starting units) on skirmish maps, and the resources projected from the map's
    /// scenery.
    pub list: Vec<Placement>,
    /// The placements of players' starting units (not the fort): who starts gathering.
    pub starting_units: BTreeSet<String>,
}

/// Every placement the match starts with.
pub fn placements(
    map: &UtcMap,
    catalog: &Catalog,
    terrain: &Terrain,
    setup: &MatchSetup,
) -> Result<Placements, String> {
    expand(map, &catalog.content, terrain, &setup.slots, setup.seed)
}

/// The starting hero of a computer seat that chose none: one of the setup's choices,
/// drawn from the match seed and the seat, so computer opponents vary their heroes.
fn computer_hero(setup: &StartingSetup, seed: u32, player: u8) -> Option<&str> {
    let choices = &setup.hero.as_ref()?.choices;
    let mut x = seed ^ (u32::from(player) + 1).wrapping_mul(0x9e37_79b1);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    let n = u32::try_from(choices.len()).ok().filter(|&n| n > 0)?;
    choices.get((x % n) as usize).map(Id::as_str)
}

fn expand(
    map: &UtcMap,
    content: &Content,
    terrain: &Terrain,
    slots: &[Slot],
    seed: u32,
) -> Result<Placements, String> {
    let rules = &content.rules;
    for slot in slots {
        race_setup(rules, slot.race.as_deref())?;
    }
    let mut result = map.entities.clone();
    let mut started = BTreeSet::new();
    if skirmish(map) {
        // A seat nobody plays (left open or closed in the lobby) is not in the match: its
        // start gets no base and the map's own placements for it are left out.
        let seated = |owner: Owner| match owner {
            Owner::Player(n) => slots.iter().any(|s| s.player == n),
            _ => true,
        };
        result.retain(|p| seated(p.owner));
        let resources = resources(map, terrain);
        for start in &map.player_starts {
            let slot = slots
                .iter()
                .find(|s| u32::from(s.player) + 1 == u32::from(start.player));
            if !seated(start.owner()) {
                continue;
            }
            let setup = race_setup(rules, slot.and_then(|s| s.race.as_deref()))?;
            let known = rules.starting_setup.id.as_str() == start.setup
                || rules.races.values().any(|r| {
                    r.starting_setup
                        .as_ref()
                        .is_some_and(|s| s.id.as_str() == start.setup)
                });
            if !known {
                return Err(format!(
                    "Player {}: unknown setup {}",
                    start.player, start.setup
                ));
            }
            let owner = start.owner();
            let prefix = format!("start.player.{}", start.player);
            result.push(Placement {
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
            });
            let rotation = formation_rotation(start, setup, &resources, content);
            let hero = slot.and_then(|s| {
                s.hero.as_deref().or_else(|| {
                    (s.kind == SlotKind::Ai)
                        .then(|| computer_hero(setup, seed, s.player))
                        .flatten()
                })
            });
            for (index, (definition, (ox, oy))) in starting_units(setup, hero)?.enumerate() {
                let (x, y) = starting_unit_position(start.x, start.z, rotation, ox, oy)?;
                let id = format!("{prefix}/unit-{:03}", index + 1);
                started.insert(id.clone());
                result.push(Placement {
                    activation: None,
                    id,
                    map_knowledge: None,
                    definition: definition.clone(),
                    position: Point {
                        x,
                        y,
                        surface: None,
                    },
                    rotation,
                    owner,
                    initial_state: None,
                    appearance: None,
                });
            }
        }
    }
    result.extend(terrain.scene().resources.iter().cloned());
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Placements {
        list: result,
        starting_units: started,
    })
}

fn skirmish(map: &UtcMap) -> bool {
    map.mission.is_none() && !map.sandbox
}

/// The map's entities and its projected resources: what starting formations face.
fn resources<'a>(map: &'a UtcMap, terrain: &'a Terrain) -> Vec<&'a Placement> {
    map.entities
        .iter()
        .chain(&terrain.scene().resources)
        .collect()
}

/// A race's starting setup; the default race when `race` is `None` (old `raceDefinition`).
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

/// The setup's starting units followed by the chosen (or default) hero, with offsets.
fn starting_units<'s>(
    setup: &'s StartingSetup,
    selected: Option<&str>,
) -> Result<impl Iterator<Item = (&'s Id, (i32, i32))>, String> {
    let hero = match (&setup.hero, selected) {
        (None, Some(_)) => return Err("This setup has no starting hero choice".into()),
        (None, None) => None,
        (Some(policy), selected) => {
            let id = selected.unwrap_or(policy.default.as_str());
            let choice = policy
                .choices
                .iter()
                .find(|c| c.as_str() == id)
                .ok_or_else(|| format!("Unavailable starting hero: {id}"))?;
            Some((choice, (policy.offset.x, policy.offset.y)))
        }
    };
    Ok(setup
        .units
        .iter()
        .map(|u| (&u.definition, (u.offset.x, u.offset.y)))
        .chain(hero))
}

/// A starting unit's cell: the offset rotated in quarter turns around the start, with the
/// navigation-centre bias rotating with the formation (old `startingUnitPosition`).
fn starting_unit_position(
    x: f64,
    z: f64,
    rotation: f64,
    ox: i32,
    oy: i32,
) -> Result<(f64, f64), String> {
    let quarter = rotation / 90.0;
    if quarter.fract() != 0.0 {
        return Err("Starting formations rotate in 90-degree steps".into());
    }
    let dx = f64::from(ox) + round_half_up(x) - x;
    let dy = f64::from(oy) + round_half_up(z) - z;
    let (rx, ry) = match (quarter as i64).rem_euclid(4) {
        0 => (dx, dy),
        1 => (dy, -dx),
        2 => (-dx, -dy),
        _ => (-dy, dx),
    };
    Ok((x + rx, z + ry))
}

/// The formation's rotation: facing the nearest source of the setup's first gathered item
/// within the gathering radius, in quarter turns (old `startingFormationRotation`). Ties
/// go to the lower placement id; without such a source the start's own rotation stands.
fn formation_rotation(
    start: &PlayerStart,
    setup: &StartingSetup,
    resources: &[&Placement],
    content: &Content,
) -> f64 {
    let rotation = f64::from(start.rotation.unwrap_or(0));
    let base = content
        .definition(setup.fort.as_str())
        .and_then(|fort| fort.behaviors.storage.as_ref()?.gathering.as_ref())
        .map(|g| (&g.item, g.radius));
    let gathering = base.or_else(|| {
        let first = setup.gathering.as_ref()?.first()?;
        Some((&first.item, f64::from(first.radius)))
    });
    let Some((item, radius)) = gathering else {
        return rotation;
    };
    let Some(Creation::Harvest { source, .. }) = content
        .definition(item.as_str())
        .and_then(|d| d.creation.as_ref())
    else {
        return rotation;
    };
    let distance = |p: &Placement| {
        let (dx, dz) = (p.position.x - start.x, p.position.y - start.z);
        dx * dx + dz * dz
    };
    let nearest = resources
        .iter()
        .filter(|p| {
            p.definition == *source
                && p.owner == Owner::Nobody
                && p.activation.is_none()
                && initial_amount(p, content) > 0
                && distance(p) <= radius * radius
        })
        .min_by(|a, b| {
            distance(a)
                .total_cmp(&distance(b))
                .then_with(|| a.id.cmp(&b.id))
        });
    let Some(source) = nearest else {
        return rotation;
    };
    let (dx, dz) = (source.position.x - start.x, source.position.y - start.z);
    if dx == 0.0 && dz == 0.0 {
        rotation
    } else if dx.abs() > dz.abs() {
        if dx > 0.0 { 90.0 } else { 270.0 }
    } else if dz > 0.0 {
        0.0
    } else {
        180.0
    }
}

/// A placement's starting yield: its own amount, else its definition's.
fn initial_amount(p: &Placement, content: &Content) -> u32 {
    p.initial_state
        .as_ref()
        .and_then(|s| s.amount)
        .or_else(|| content.definition(p.definition.as_str())?.resource_yield)
        .unwrap_or(0)
}

fn definition<'c>(content: &'c Content, p: &Placement) -> Result<&'c Definition, String> {
    content
        .definition(p.definition.as_str())
        .ok_or_else(|| format!("{}: unknown definition {}", p.id, p.definition))
}

/// The placement rules that need geometry, on the match's actual placements: cell
/// centres and the building lattice, unique ids (projected resources included), map
/// bounds, building footprints on the map, known walk surfaces, and owners with a slot.
pub fn validate(
    catalog: &Catalog,
    terrain: &Terrain,
    setup: &MatchSetup,
    placements: &[Placement],
) -> Result<(), String> {
    let content = &catalog.content;
    let size = f64::from(terrain.size());
    let mut ids = BTreeSet::new();
    for p in placements {
        let d = definition(content, p)?;
        let (x, y) = (p.position.x, p.position.y);
        if let Some(error) = placement_geometry_error(d, x, y, p.rotation) {
            return Err(format!("{}: {error}", p.id));
        }
        if !ids.insert(p.id.as_str()) {
            return Err(format!("Duplicate placement {}", p.id));
        }
        if !(x >= 0.0 && y >= 0.0 && x < size && y < size) {
            return Err(format!("{}: outside map bounds", p.id));
        }
        if d.kind == Kind::Building
            && !footprint_cells(x, y, Extent::of(d.footprint.as_ref()), p.rotation)
                .inside(terrain.size())
        {
            return Err(format!(
                "{}: building footprint is outside map bounds",
                p.id
            ));
        }
        if let Some(surface) = &p.position.surface
            && terrain.floor(x, y, Some(surface)).is_none()
        {
            return Err(format!("{}: unknown or out-of-bounds walk surface", p.id));
        }
        if let Owner::Player(slot) = p.owner
            && p.activation.is_none()
            && !setup.slots.iter().any(|s| s.player == slot)
        {
            return Err(format!(
                "Placement owner player.{} has no match slot",
                u32::from(slot) + 1
            ));
        }
    }
    Ok(())
}

/// Why `start` cannot stand on `map`'s terrain, by the rules [`playable`] checks: on the
/// building lattice at least 8 cells inside the playable area, with room, dry and gentle
/// ground for every race's fort and its worker formation. The map editor asks this of a
/// start before it places or moves one.
pub fn start_problem(
    map: &UtcMap,
    content: &Content,
    terrain: &Terrain,
    start: &PlayerStart,
) -> Result<(), String> {
    let fort = content
        .definition(content.rules.starting_setup.fort.as_str())
        .ok_or("unknown starting fort")?;
    let area = map.playable_area();
    let (x0, z0) = (f64::from(area.x), f64::from(area.z));
    let (x1, z1) = (
        f64::from(area.x + area.width),
        f64::from(area.z + area.height),
    );
    let player = start.player;
    let rotation = f64::from(start.rotation.unwrap_or(0));
    if placement_geometry_error(fort, start.x, start.z, rotation).is_some()
        || start.x < x0 + 8.0
        || start.z < z0 + 8.0
        || start.x > x1 - 9.0
        || start.z > z1 - 9.0
    {
        return Err(format!(
            "Player {player} needs a building-grid-aligned start at least 8 navigation cells inside the map."
        ));
    }
    let resources = resources(map, terrain);
    let area = starting_area(start, &resources, content)?;
    if !area.inside(terrain.size()) {
        return Err(format!(
            "Player {player} needs room for the complete fort and worker formation."
        ));
    }
    let field = terrain.field();
    let dry = area.cells().all(|(x, z)| {
        let (x, z) = (x as f64, z as f64);
        field.sample(x, z) > field.water_at(x, z) + 0.1
    });
    if !dry {
        return Err(format!(
            "Player {player} needs dry ground for the fort and workers."
        ));
    }
    if !terrain.foundation_fits(area) {
        return Err(format!(
            "Player {player} needs flatter ground for the fort and workers."
        ));
    }
    if area
        .cells()
        .any(|(x, z)| terrain.cell(x, z).is_none_or(|c| !terrain.walkable(c)))
    {
        return Err(format!(
            "Player {player} needs open ground for the fort and workers."
        ));
    }
    Ok(())
}

/// The cells a start's forts and worker formation cover (every race's), as [`playable`]
/// checks them.
pub fn start_area(
    map: &UtcMap,
    content: &Content,
    terrain: &Terrain,
    start: &PlayerStart,
) -> Option<CellBounds> {
    starting_area(start, &resources(map, terrain), content).ok()
}

/// Why `definition` cannot stand at (`x`, `y`) on `map`'s terrain: its geometry (the
/// building lattice, cell centres), inside the playable area, every cell it covers open
/// and dry, and for a building gentle enough for its foundation.
pub fn site_problem(
    map: &UtcMap,
    terrain: &Terrain,
    definition: &Definition,
    x: f64,
    y: f64,
    rotation: f64,
) -> Option<String> {
    if let Some(error) = placement_geometry_error(definition, x, y, rotation) {
        return Some(error.to_string());
    }
    let building = definition.kind == Kind::Building;
    let cells = if building {
        footprint_cells(x, y, Extent::of(definition.footprint.as_ref()), rotation)
    } else {
        let (cx, cy) = (x.round() as i64, y.round() as i64);
        CellBounds {
            min_x: cx,
            min_y: cy,
            max_x: cx,
            max_y: cy,
        }
    };
    let area = map.playable_area();
    let inside = cells.min_x >= i64::from(area.x)
        && cells.min_y >= i64::from(area.z)
        && cells.max_x < i64::from(area.x + area.width)
        && cells.max_y < i64::from(area.z + area.height);
    if !inside {
        return Some("Inside the playable area only".into());
    }
    let field = terrain.field();
    for (cx, cy) in cells.cells() {
        let Some(cell) = terrain.cell(cx, cy) else {
            return Some("Inside the map only".into());
        };
        if !terrain.walkable(cell) {
            return Some("The ground is closed or under deep water".into());
        }
        if building
            && field.sample(cx as f64, cy as f64) <= field.water_at(cx as f64, cy as f64) + 0.1
        {
            return Some("Buildings need dry ground".into());
        }
    }
    if building && !terrain.foundation_fits(cells) {
        return Some("Choose flatter ground (no cliff or ramp under it)".into());
    }
    None
}

/// Whether the map can be played on its terrain: each start on the building lattice with
/// room, dry and gentle ground for every race's fort and formation; starting halls clear
/// of resources; nothing overlapping at the start. Checked for the default setups, as
/// the map is authored.
pub fn playable(map: &UtcMap, catalog: &Catalog, terrain: &Terrain) -> Result<(), String> {
    let content = &catalog.content;
    let resources = resources(map, terrain);
    for s in &map.player_starts {
        start_problem(map, content, terrain, s)?;
    }
    resource_clearance(map, content, &resources)?;
    occupancy(map, content, terrain)
}

/// Cells the map's trees close to armies: each tree's cell and its four neighbours (its
/// trunk and the bodies that cannot squeeze past it).
pub fn tree_cells(terrain: &Terrain) -> Vec<bool> {
    let size = terrain.size() as usize;
    let mut closed = vec![false; size * size];
    for tree in &terrain.scene().resources {
        let (x, z) = (
            tree.position.x.floor() as i64,
            tree.position.y.floor() as i64,
        );
        for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            if let Some(c) = terrain.cell(x + dx, z + dz) {
                closed[c] = true;
            }
        }
    }
    closed
}

/// The cells ground units can reach from world (`x`, `z`): walkable cells joined across
/// their sides where no step is too steep, around the `closed` ones (trees: see
/// [`tree_cells`]). Buildings and units are left out.
pub fn reachable_from(terrain: &Terrain, closed: &[bool], x: f64, z: f64) -> Vec<bool> {
    let size = terrain.size() as usize;
    let mut reached = vec![false; size * size];
    let Some(first) = terrain.cell(x.floor() as i64, z.floor() as i64) else {
        return reached;
    };
    let steep = terrain.steep_edges();
    let mut queue = vec![first];
    reached[first] = true;
    while let Some(cell) = queue.pop() {
        let (cx, cz) = (cell % size, cell / size);
        for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
            let (nx, nz) = (cx as i64 + i64::from(dx), cz as i64 + i64::from(dz));
            let Some(next) = terrain.cell(nx, nz) else {
                continue;
            };
            if reached[next]
                || closed.get(next).copied().unwrap_or(false)
                || !terrain.walkable(next)
                || steep.entered(nx as usize, nz as usize, dx, dz)
            {
                continue;
            }
            reached[next] = true;
            queue.push(next);
        }
    }
    reached
}

/// Cells (radius) the largest bodies armies field keep from cliffs, deep water and trees:
/// a hero twice a warrior's size. Lanes narrower than twice this close to them, and
/// computer players' routes give up there.
pub const ARMY_RADIUS: i64 = 3;

/// `closed` grown by `radius` cells (square), with the cells beside steep steps and
/// those that are not walkable closed first: where a body of that radius cannot stand.
pub fn narrowed(terrain: &Terrain, closed: &[bool], radius: i64) -> Vec<bool> {
    let size = terrain.size() as i64;
    let steep = terrain.steep_edges();
    let blocked = |x: i64, z: i64| {
        let c = (z * size + x) as usize;
        closed[c]
            || !terrain.walkable(c)
            || [(1, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|&(dx, dz)| steep.entered(x as usize, z as usize, dx, dz))
    };
    // Distance (square rings) to the nearest blocked cell, up to `radius`.
    let mut out = vec![false; (size * size) as usize];
    let mut frontier: Vec<(i64, i64)> = Vec::new();
    for z in 0..size {
        for x in 0..size {
            if blocked(x, z) {
                out[(z * size + x) as usize] = true;
                frontier.push((x, z));
            }
        }
    }
    for _ in 0..radius {
        let mut next = Vec::new();
        for (x, z) in frontier {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let (nx, nz) = (x + dx, z + dz);
                    if nx >= 0 && nz >= 0 && nx < size && nz < size {
                        let c = (nz * size + nx) as usize;
                        if !out[c] {
                            out[c] = true;
                            next.push((nx, nz));
                        }
                    }
                }
            }
        }
        frontier = next;
    }
    out
}

/// Whether armies, their largest bodies included, can walk from the first start to every
/// other start, every camp and every resource placement (amber), around cliffs, deep water
/// and trees. Computer players march at what they cannot reach and stay stuck there.
pub fn connected(map: &UtcMap, terrain: &Terrain) -> Result<(), String> {
    let Some(first) = map.player_starts.first() else {
        return Ok(());
    };
    // Buildings are left out, so the fort's own cells count as ground.
    let closed = narrowed(terrain, &tree_cells(terrain), ARMY_RADIUS);
    let reached = reachable_from(terrain, &closed, first.x, first.z);
    for s in &map.player_starts[1..] {
        if !near(terrain, &reached, s.x, s.z, 12) {
            return Err(format!(
                "Player {}'s start cannot be reached on foot from Player {}'s: cliffs, deep water or trees close the way, or leave it narrower than {} cells.",
                s.player,
                first.player,
                2 * ARMY_RADIUS + 1
            ));
        }
    }
    for camp in &map.camps {
        if !near(terrain, &reached, camp.home.x, camp.home.y, 10) {
            return Err(format!(
                "{} cannot be reached on foot from Player {}'s start: cliffs, deep water or trees close the way, or leave it narrower than {} cells.",
                camp.id,
                first.player,
                2 * ARMY_RADIUS + 1
            ));
        }
    }
    let members: std::collections::BTreeSet<&str> = map
        .camps
        .iter()
        .flat_map(|c| c.members.iter().map(String::as_str))
        .collect();
    for e in map.entities.iter().filter(|e| {
        e.owner == canopy_content::placement::Owner::Nobody && !members.contains(e.id.as_str())
    }) {
        if !near(terrain, &reached, e.position.x, e.position.y, 10) {
            return Err(format!(
                "{} cannot be reached on foot from Player {}'s start: cliffs, deep water or trees close the way, or leave it narrower than {} cells.",
                e.id,
                first.player,
                2 * ARMY_RADIUS + 1
            ));
        }
    }
    Ok(())
}

/// Whether any cell within `radius` of world (`x`, `z`) is reached.
pub fn near(terrain: &Terrain, reached: &[bool], x: f64, z: f64, radius: i64) -> bool {
    let (cx, cz) = (x.floor() as i64, z.floor() as i64);
    (-radius..=radius).any(|dz| {
        (-radius..=radius).any(|dx| {
            dx * dx + dz * dz <= radius * radius
                && terrain.cell(cx + dx, cz + dz).is_some_and(|c| reached[c])
        })
    })
}

/// Cells a start needs for its fort and the formation of every race and selectable hero
/// (old `startingArea`).
fn starting_area(
    start: &PlayerStart,
    resources: &[&Placement],
    content: &Content,
) -> Result<CellBounds, String> {
    let rules = &content.rules;
    let setups: Vec<&StartingSetup> = if rules.races.is_empty() {
        vec![&rules.starting_setup]
    } else {
        rules
            .races
            .values()
            .map(|r| r.starting_setup.as_ref().unwrap_or(&rules.starting_setup))
            .collect()
    };
    let rotation = f64::from(start.rotation.unwrap_or(0));
    let (mut lo_x, mut hi_x, mut lo_z, mut hi_z) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for setup in setups {
        let fort = content
            .definition(setup.fort.as_str())
            .ok_or_else(|| format!("unknown fort {}", setup.fort))?;
        let half = half_extents(Extent::of(fort.footprint.as_ref()), rotation);
        lo_x = lo_x.min(start.x - half.0);
        hi_x = hi_x.max(start.x + half.0);
        lo_z = lo_z.min(start.z - half.1);
        hi_z = hi_z.max(start.z + half.1);
        let facing = formation_rotation(start, setup, resources, content);
        let heroes = setup.hero.iter().flat_map(|h| {
            h.choices
                .iter()
                .map(move |id| (id, (h.offset.x, h.offset.y)))
        });
        let formation = setup
            .units
            .iter()
            .map(|u| (&u.definition, (u.offset.x, u.offset.y)))
            .chain(heroes);
        for (id, (ox, oy)) in formation {
            let (x, y) = starting_unit_position(start.x, start.z, facing, ox, oy)?;
            let radius = content
                .definition(id.as_str())
                .and_then(|d| d.dimensions.as_ref())
                .map_or(0.0, |d| d.radius);
            lo_x = lo_x.min(x - radius);
            hi_x = hi_x.max(x + radius);
            lo_z = lo_z.min(y - radius);
            hi_z = hi_z.max(y + radius);
        }
    }
    Ok(CellBounds {
        min_x: lo_x.floor() as i64,
        min_y: lo_z.floor() as i64,
        max_x: hi_x.ceil() as i64,
        max_y: hi_z.ceil() as i64,
    })
}

/// Starting halls keep the same resource access lane construction requires (old
/// `startingResourceClearanceError`).
fn resource_clearance(
    map: &UtcMap,
    content: &Content,
    resources: &[&Placement],
) -> Result<(), String> {
    if !skirmish(map) {
        return Ok(());
    }
    let hall = content
        .definition(content.rules.starting_setup.fort.as_str())
        .ok_or("unknown starting fort")?;
    for s in &map.player_starts {
        for resource in resources {
            let d = definition(content, resource)?;
            let Some(clearance) = d.construction_clearance else {
                continue;
            };
            if resource.activation == Some(Activation::Script)
                || initial_amount(resource, content) == 0
            {
                continue;
            }
            let (min_x, min_y) = resource_center_separation(
                Extent::of(hall.footprint.as_ref()),
                Extent::of(d.footprint.as_ref()),
                f64::from(clearance),
                f64::from(s.rotation.unwrap_or(0)),
                resource.rotation,
            );
            if (s.x - resource.position.x).abs() < min_x
                && (s.z - resource.position.y).abs() < min_y
            {
                return Err(format!(
                    "Player {}: {} is too close to {}; leave at least {min_x} cells horizontally or {min_y} cells vertically between centers.",
                    s.player, hall.name, d.name
                ));
            }
        }
    }
    Ok(())
}

/// A standing unit body at the start.
struct Body<'a> {
    id: &'a str,
    x: f64,
    y: f64,
    radius: f64,
    height: f64,
    floor: f64,
    air: bool,
}

/// Nothing overlaps at the start (old `placementOccupancyError`): footprints of buildings
/// and standing resources share no cell on the same floor; unit bodies stay on the map,
/// off static footprints, and apart from each other.
fn occupancy(map: &UtcMap, content: &Content, terrain: &Terrain) -> Result<(), String> {
    // Every start seated, with the default race and hero (as a person would sit).
    let every: Vec<Slot> = map
        .player_starts
        .iter()
        .map(|s| Slot {
            player: s.player.saturating_sub(1),
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        })
        .collect();
    let placements = expand(map, content, terrain, &every, 0)?.list;
    let size = terrain.size();
    let limit = f64::from(size) - 0.5;
    let layered = placements.iter().any(|p| p.position.surface.is_some());
    let floor = |x: f64, y: f64, surface: Option<&str>| {
        if layered {
            terrain.floor(x, y, surface).unwrap_or(0.0)
        } else {
            0.0
        }
    };
    let cell_index = |x: i64, y: i64| (y * i64::from(size) + x) as usize;
    let mut occupied: BTreeMap<usize, Vec<(&str, f64)>> = BTreeMap::new();
    let mut units: Vec<Body> = Vec::new();
    let mut unit_cells: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut max_radius = 0.0f64;
    for p in &placements {
        let d = definition(content, p)?;
        let amount = p.initial_state.as_ref().and_then(|s| s.amount);
        if d.kind == Kind::Item || (d.kind == Kind::Resource && amount == Some(0)) {
            continue;
        }
        let scripted = p.activation == Some(Activation::Script);
        let (x, y) = (p.position.x, p.position.y);
        let surface = p.position.surface.as_deref();
        if d.kind == Kind::Unit {
            let dimensions = d
                .dimensions
                .as_ref()
                .ok_or_else(|| format!("{}: unit without dimensions", p.id))?;
            let radius = dimensions.radius;
            if x - radius < -0.5 || y - radius < -0.5 || x + radius > limit || y + radius > limit {
                return Err(format!("{}: body is outside the playable map", p.id));
            }
            // Deferred units may emerge from an occupied cage.
            if scripted {
                continue;
            }
            let air =
                d.behaviors.movement.as_ref().and_then(|m| m.locomotion) == Some(Locomotion::Air);
            max_radius = max_radius.max(radius);
            let cell = cell_index(round_half_up(x) as i64, round_half_up(y) as i64);
            unit_cells.entry(cell).or_default().push(units.len());
            units.push(Body {
                id: &p.id,
                x,
                y,
                radius,
                height: dimensions.height,
                floor: floor(x, y, surface),
                air,
            });
            continue;
        }
        let bounds = footprint_cells(x, y, Extent::of(d.footprint.as_ref()), p.rotation);
        for (cx, cy) in bounds.cells() {
            if terrain.cell(cx, cy).is_none() {
                return Err(format!("{}: footprint is outside the playable map", p.id));
            }
            // Deferred scenery may replace a destroyed object.
            if scripted {
                continue;
            }
            let height = floor(cx as f64, cy as f64, surface);
            let column = occupied.entry(cell_index(cx, cy)).or_default();
            if let Some((other, _)) = column.iter().find(|(_, h)| (h - height).abs() < 2.0) {
                return Err(format!("{}: overlaps {other}", p.id));
            }
            column.push((&p.id, height));
        }
    }
    let last = i64::from(size) - 1;
    let milli = |v: f64| round_half_up(v * 1000.0) as i64;
    for unit in &units {
        // Static footprints meet the square navigation body; units meet as circles.
        if !unit.air {
            let y0 = ((unit.y - unit.radius + 0.5).floor() as i64).max(0);
            let y1 = ((unit.y + unit.radius + 0.5).floor() as i64).min(last);
            let x0 = ((unit.x - unit.radius + 0.5).floor() as i64).max(0);
            let x1 = ((unit.x + unit.radius + 0.5).floor() as i64).min(last);
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    let cell = Bounds {
                        min_x: cx as f64 - 0.5,
                        min_y: cy as f64 - 0.5,
                        max_x: cx as f64 + 0.5,
                        max_y: cy as f64 + 0.5,
                    };
                    let column = occupied.get(&cell_index(cx, cy)).into_iter().flatten();
                    for (other, height) in column {
                        if (height - unit.floor).abs() < 2.0
                            && body_overlaps_bounds(unit.x, unit.y, unit.radius, cell)
                        {
                            return Err(format!("{}: body overlaps {other}", unit.id));
                        }
                    }
                }
            }
        }
        let reach = unit.radius + max_radius;
        let y0 = ((unit.y - reach).floor() as i64).max(0);
        let y1 = ((unit.y + reach).ceil() as i64).min(last);
        let x0 = ((unit.x - reach).floor() as i64).max(0);
        let x1 = ((unit.x + reach).ceil() as i64).min(last);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                for &index in unit_cells.get(&cell_index(cx, cy)).into_iter().flatten() {
                    let other = &units[index];
                    if other.id >= unit.id || other.air != unit.air {
                        continue;
                    }
                    if layered
                        && (unit.floor >= other.floor + other.height
                            || other.floor >= unit.floor + unit.height)
                    {
                        continue;
                    }
                    let dx = milli(unit.x) - milli(other.x);
                    let dy = milli(unit.y) - milli(other.y);
                    let separation = milli(unit.radius) + milli(other.radius);
                    if dx * dx + dy * dy < separation * separation {
                        return Err(format!("{}: overlaps {}", unit.id, other.id));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Moves the map's own units that would overlap at the start ([`occupancy`]): a body on a
/// building or standing resource, or on another body, steps to the nearest cell centre
/// (within 12 cells) on walkable ground where it overlaps nothing; units placed earlier
/// in the map keep their spot. For camps laid out before their creeps' bodies grew, and
/// for imports. Returns the ids moved and those that found no room. Maps with decks are
/// left alone.
pub fn untangle(
    map: &mut UtcMap,
    content: &Content,
    terrain: &Terrain,
) -> Result<(Vec<String>, Vec<String>), String> {
    let every: Vec<Slot> = map
        .player_starts
        .iter()
        .map(|s| Slot {
            player: s.player.saturating_sub(1),
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        })
        .collect();
    let placements = expand(map, content, terrain, &every, 0)?.list;
    if placements.iter().any(|p| p.position.surface.is_some()) {
        return Ok((Vec::new(), Vec::new()));
    }
    let size = terrain.size();
    let limit = f64::from(size) - 0.5;
    let last = i64::from(size) - 1;
    let mut occupied = BTreeSet::new();
    // (radius, air) of every unit placement, by id.
    let mut units: BTreeMap<&str, (f64, bool)> = BTreeMap::new();
    for p in &placements {
        let d = definition(content, p)?;
        let amount = p.initial_state.as_ref().and_then(|s| s.amount);
        if d.kind == Kind::Item
            || (d.kind == Kind::Resource && amount == Some(0))
            || p.activation == Some(Activation::Script)
        {
            continue;
        }
        if d.kind == Kind::Unit {
            let radius = d.dimensions.as_ref().map_or(0.0, |b| b.radius);
            let air =
                d.behaviors.movement.as_ref().and_then(|m| m.locomotion) == Some(Locomotion::Air);
            units.insert(&p.id, (radius, air));
            continue;
        }
        let bounds = footprint_cells(
            p.position.x,
            p.position.y,
            Extent::of(d.footprint.as_ref()),
            p.rotation,
        );
        occupied.extend(bounds.cells());
    }
    let clear_of_statics = |x: f64, y: f64, radius: f64| {
        let y0 = ((y - radius + 0.5).floor() as i64).max(0);
        let y1 = ((y + radius + 0.5).floor() as i64).min(last);
        let x0 = ((x - radius + 0.5).floor() as i64).max(0);
        let x1 = ((x + radius + 0.5).floor() as i64).min(last);
        (y0..=y1).all(|cy| {
            (x0..=x1).all(|cx| {
                !occupied.contains(&(cx, cy))
                    || !body_overlaps_bounds(
                        x,
                        y,
                        radius,
                        Bounds {
                            min_x: cx as f64 - 0.5,
                            min_y: cy as f64 - 0.5,
                            max_x: cx as f64 + 0.5,
                            max_y: cy as f64 + 0.5,
                        },
                    )
            })
        })
    };
    let mut offsets: Vec<(i64, i64)> = (-12..=12)
        .flat_map(|i| (-12..=12).map(move |j| (i, j)))
        .filter(|(i, j)| i * i + j * j <= 144)
        .collect();
    offsets.sort_by_key(|&(i, j)| (i * i + j * j, j, i));
    let movable: BTreeSet<&str> = map
        .entities
        .iter()
        .filter(|p| units.contains_key(p.id.as_str()))
        .map(|p| p.id.as_str())
        .collect();
    // Bodies that stay: the starts' units; then each map unit once it has its spot.
    let mut standing: Vec<(f64, f64, f64, bool)> = placements
        .iter()
        .filter(|p| !movable.contains(p.id.as_str()))
        .filter_map(|p| {
            let &(r, air) = units.get(p.id.as_str())?;
            Some((p.position.x, p.position.y, r, air))
        })
        .collect();
    let (mut moved, mut stuck) = (Vec::new(), Vec::new());
    for p in map.entities.iter_mut() {
        let Some(&(radius, air)) = units.get(p.id.as_str()) else {
            continue;
        };
        let fits = |x: f64, y: f64| {
            x - radius >= -0.5
                && y - radius >= -0.5
                && x + radius <= limit
                && y + radius <= limit
                && (air
                    || (terrain
                        .cell(round_half_up(x) as i64, round_half_up(y) as i64)
                        .is_some_and(|c| terrain.walkable(c))
                        && clear_of_statics(x, y, radius)))
                && standing.iter().all(|&(ox, oy, or, oair)| {
                    // As the start's check measures: milli-cells, squared.
                    let milli = |v: f64| round_half_up(v * 1000.0) as i64;
                    let (dx, dy) = (milli(ox) - milli(x), milli(oy) - milli(y));
                    let apart = milli(or) + milli(radius);
                    oair != air || dx * dx + dy * dy >= apart * apart
                })
        };
        let (x0, y0) = (p.position.x, p.position.y);
        let spot = offsets
            .iter()
            .map(|&(dx, dy)| (x0 + dx as f64, y0 + dy as f64))
            .find(|&(x, y)| fits(x, y));
        match spot {
            Some((x, y)) => {
                if (x, y) != (x0, y0) {
                    p.position.x = x;
                    p.position.y = y;
                    moved.push(p.id.clone());
                }
                standing.push((x, y, radius, air));
            }
            None => {
                stuck.push(p.id.clone());
                standing.push((x0, y0, radius, air));
            }
        }
    }
    Ok((moved, stuck))
}

/// Creates the entity for a placement and adds it to the state. Placed buildings are
/// complete: the old game never read `initialState.construction`.
pub fn create(
    state: &mut State,
    catalog: &Catalog,
    placement: &Placement,
) -> Result<EntityId, String> {
    let def: Def = catalog.find(placement.definition.as_str()).ok_or_else(|| {
        format!(
            "{}: unknown definition {}",
            placement.id, placement.definition
        )
    })?;
    let milli = |v: f64| (v * f64::from(Milli::CELL)).round() as i32;
    let tick = state.tick;
    Ok(insert(
        state,
        catalog,
        Spawn {
            definition: def,
            owner: placement.owner,
            position: Milli {
                x: milli(placement.position.x),
                y: milli(placement.position.y),
            },
            surface: placement.position.surface.clone(),
            rotation: (placement.rotation * 1000.0).round() as i32,
            placement: Some(placement.id.clone()),
            initial: placement.initial_state.clone().unwrap_or_default(),
            complete: true,
            ready_tick: tick,
        },
    ))
}

/// A new entity: what it is, where, and in what state.
#[derive(Debug, Clone)]
pub struct Spawn {
    pub definition: Def,
    pub owner: Owner,
    pub position: Milli,
    pub surface: Option<String>,
    /// Millidegrees.
    pub rotation: i32,
    /// The map placement it comes from.
    pub placement: Option<String>,
    pub initial: InitialState,
    /// A building is complete, or a construction site at its starting health.
    pub complete: bool,
    pub ready_tick: u32,
}

/// Adds a new entity to the state (old `GameContext.create`): its parts follow from its
/// definition. A completed drop-off's authored currencies become its owner's funds.
pub fn insert(state: &mut State, catalog: &Catalog, spawn: Spawn) -> EntityId {
    let definition = catalog.get(spawn.definition);
    let initial = spawn.initial;
    let id = EntityId(state.counters.entity);
    state.counters.entity += 1;
    let site = definition.kind == Kind::Building && !spawn.complete;
    let site_hp = definition.body.as_ref().map(|b| {
        (u64::from(b.max_hp) * u64::from(catalog.content.rules.construction_hp_permille) / 1000)
            .max(1) as u32
    });
    let hp = if site {
        site_hp
    } else {
        initial
            .health
            .or(definition.body.as_ref().map(|b| b.max_hp))
    };
    let behaviors = &definition.behaviors;
    let mut entity = Entity {
        id,
        ready_tick: spawn.ready_tick,
        placement: spawn.placement,
        definition: spawn.definition,
        owner: spawn.owner,
        position: spawn.position,
        surface: spawn.surface,
        rotation: spawn.rotation,
        hp,
        inventory: initial.inventory.clone().unwrap_or_default(),
        unit: (definition.kind == Kind::Unit).then(Box::<Unit>::default),
        construction: site.then(|| Construction {
            progress: 0,
            supported_hp: site_hp.unwrap_or(1),
        }),
        resource: definition.resource_yield.map(|full| Resource {
            amount: initial.amount.unwrap_or(full),
            initial_amount: Some(full),
            growing_until: None,
            felling: definition
                .felling
                .as_ref()
                .map(|f| crate::economy::Felling::standing(f.max_hp)),
        }),
        item: (definition.kind == Kind::Item).then(|| LooseItem {
            runtime: None,
            quantity: initial.quantity.unwrap_or(1),
        }),
        production: behaviors
            .production
            .as_ref()
            .map(|_| Box::<crate::economy::Production>::default()),
        research: behaviors
            .research
            .as_ref()
            .map(|_| crate::economy::ResearchQueue::default()),
        upgrade: None,
        revival: behaviors
            .revival
            .as_ref()
            .map(|_| crate::economy::RevivalQueue::default()),
        progression: crate::heroes::initial(definition, initial.experience),
        fallen: false,
        caster: None,
        statuses: Vec::new(),
        links: None,
        equipment: None,
        income: 0,
    };
    // A complete body starts at full health for its level and its owner's research,
    // unless the placement says otherwise.
    if !site && initial.health.is_none() && entity.hp.is_some() {
        let research = crate::stats::research(state, entity.owner);
        entity.hp = Some(crate::stats::Stats::of(catalog, &entity, research).max_hp);
    }
    let dropoff = behaviors
        .storage
        .as_ref()
        .is_some_and(|s| s.dropoff == Some(true));
    if spawn.complete && dropoff && entity.owner != Owner::Nobody {
        let funds: Vec<(Id, u32)> = entity
            .inventory
            .iter()
            .filter(|(item, _)| crate::economy::stock::currency(catalog, item.as_str()))
            .map(|(item, &amount)| (item.clone(), amount))
            .collect();
        let wallet = state.wallets.entry(entity.owner).or_default();
        for (item, amount) in funds {
            crate::economy::stock::add(wallet, &item, amount);
            entity.inventory.remove(&item);
        }
    }
    crate::abilities::equip_new(catalog, &mut entity);
    crate::abilities::research_ranks(
        catalog,
        crate::stats::research(state, entity.owner),
        &mut entity,
    );
    state.entities.insert(entity);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formations_rotate_in_quarter_turns() {
        assert_eq!(
            starting_unit_position(10.0, 20.0, 0.0, 2, 3).unwrap(),
            (12.0, 23.0)
        );
        assert_eq!(
            starting_unit_position(10.0, 20.0, 90.0, 2, 3).unwrap(),
            (13.0, 18.0)
        );
        assert_eq!(
            starting_unit_position(10.0, 20.0, 180.0, 2, 3).unwrap(),
            (8.0, 17.0)
        );
        assert_eq!(
            starting_unit_position(10.0, 20.0, 270.0, 2, 3).unwrap(),
            (7.0, 22.0)
        );
        // A half-cell hall centre: the bias rotates with the formation.
        assert_eq!(
            starting_unit_position(10.5, 20.5, 0.0, 1, 0).unwrap(),
            (12.0, 21.0)
        );
        assert_eq!(
            starting_unit_position(10.5, 20.5, 180.0, 1, 0).unwrap(),
            (9.0, 20.0)
        );
        assert!(starting_unit_position(10.0, 20.0, 45.0, 1, 0).is_err());
    }
}
