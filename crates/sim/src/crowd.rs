//! The measured crowd battle (`canopy shot --crowd N`, `just bench-sim`): eight players
//! with N soldiers each facing each other around the middle of a map's starts. A stress
//! scenario for measurements, not a game mode; it picks its soldiers from the content's
//! rules, never by id.

use crate::{MatchSetup, Slot, SlotKind, math, terrain::footprint::placement_geometry_error};
use canopy_content::{
    Content,
    game::{Definition, Kind},
    map::UtcMap,
    placement::{Owner, Placement, Point},
};

/// Players of a crowd battle.
pub const PLAYERS: u8 = 8;

/// The building-grid position nearest to (x, z) for `d`'s footprint.
fn aligned(d: &Definition, x: f64, z: f64) -> Point {
    let (x, z) = (x.floor(), z.floor());
    let steps = (0..8).map(|i| f64::from(i) * 0.5);
    let (dx, dz) = steps
        .clone()
        .flat_map(|dx| steps.clone().map(move |dz| (dx, dz)))
        .find(|(dx, dz)| placement_geometry_error(d, x + dx, z + dz, 0.0).is_none())
        .unwrap_or_default();
    Point {
        x: x + dx,
        y: z + dz,
        surface: None,
    }
}

/// Adds eight groups of `per_player` soldiers to `map` in a ring around the middle of the
/// starts, each facing the centre: the default race's animated, trainable, non-hero
/// fighters in turn. Each player also keeps a small building behind its soldiers, so
/// skirmish rules keep it in the match. Returns the match setup (eight human slots, no
/// computer players).
pub fn crowd(map: &mut UtcMap, content: &Content, per_player: u32) -> Result<MatchSetup, String> {
    let race = content
        .rules
        .races
        .get(&content.rules.default_race)
        .ok_or("the default race is missing")?;
    let fighters: Vec<&canopy_content::Id> = race
        .roster
        .iter()
        .filter(|id| {
            content.definition(id.as_str()).is_some_and(|d| {
                d.kind == Kind::Unit
                    && !d.is_hero()
                    && d.behaviors.combat.is_some()
                    && d.behaviors.work.is_none()
                    && content
                        .look(d.id.as_str())
                        .is_some_and(|a| a.character.is_some())
            })
        })
        .collect();
    if fighters.is_empty() {
        return Err("the default race has no soldiers".into());
    }
    // A colony without buildings is defeated at once (skirmish rules).
    let shelter = race
        .roster
        .iter()
        .filter_map(|id| content.definition(id.as_str()))
        .filter(|d| d.kind == Kind::Building)
        .min_by_key(|d| d.footprint.as_ref().map_or(u32::MAX, |f| f.width * f.depth))
        .ok_or("the default race has no building")?;
    let n = map.player_starts.len().max(1) as f64;
    let cx = map.player_starts.iter().map(|s| s.x).sum::<f64>() / n;
    let cz = map.player_starts.iter().map(|s| s.z).sum::<f64>() / n;
    let side = (f64::from(per_player)).sqrt().ceil() as u32;
    for player in 0..PLAYERS {
        let angle = f64::from(player) / f64::from(PLAYERS) * std::f64::consts::TAU;
        let (dx, dz) = (math::sin(angle), math::cos(angle));
        let (gx, gz) = (cx + dx * 24.0, cz + dz * 24.0);
        let facing = math::atan2(-dx, -dz).to_degrees().rem_euclid(360.0);
        map.entities.push(Placement {
            activation: None,
            id: format!("crowd-{player}-shelter"),
            map_knowledge: None,
            definition: shelter.id.clone(),
            position: aligned(shelter, gx + dx * 22.0, gz + dz * 22.0),
            rotation: 0.0,
            owner: Owner::Player(player),
            initial_state: None,
            appearance: None,
        });
        for i in 0..per_player {
            let (row, column) = (i / side, i % side);
            // Rows step away from the centre, columns across.
            let across = f64::from(column) - f64::from(side - 1) / 2.0;
            let back = f64::from(row);
            // One unit per navigation cell, so none waits to be placed.
            let x = (gx + dz * across * 1.5 + dx * back * 1.5).round();
            let z = (gz - dx * across * 1.5 + dz * back * 1.5).round();
            let definition = fighters[(i as usize + player as usize) % fighters.len()].clone();
            map.entities.push(Placement {
                activation: None,
                id: format!("crowd-{player}-{i}"),
                map_knowledge: None,
                definition,
                position: Point {
                    x,
                    y: z,
                    surface: None,
                },
                rotation: facing,
                owner: Owner::Player(player),
                initial_state: None,
                appearance: None,
            });
        }
    }
    Ok(MatchSetup {
        seed: 1,
        slots: (0..PLAYERS)
            .map(|player| Slot {
                player,
                kind: SlotKind::Human,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    })
}

/// Adds one of every unit and building of the content to `map`, in rows around the middle
/// of the starts: units first, then buildings, each in id order, all owned by player 0
/// (neutrals by nobody), facing south. A review scenario for captures (`canopy shot
/// --gallery`): every model at once, to compare looks before and after a change. Returns
/// the match setup (player 0 human, nobody else). With `only`, just the definitions whose
/// id contains it (one race's roster).
pub fn gallery(
    map: &mut UtcMap,
    content: &Content,
    only: Option<&str>,
) -> Result<MatchSetup, String> {
    let kept = |d: &&Definition| only.is_none_or(|text| d.id.as_str().contains(text));
    let in_roster = |id: &str| {
        content
            .rules
            .races
            .values()
            .any(|r| r.roster.iter().any(|x| x.as_str() == id))
    };
    let n = map.player_starts.len().max(1) as f64;
    let cx = (map.player_starts.iter().map(|s| s.x).sum::<f64>() / n).floor();
    let cz = (map.player_starts.iter().map(|s| s.z).sum::<f64>() / n).floor();
    let units: Vec<&Definition> = content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Unit)
        .filter(kept)
        .collect();
    let buildings: Vec<&Definition> = content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Building)
        .filter(kept)
        .collect();
    let owner = |d: &Definition| {
        if in_roster(d.id.as_str()) {
            Owner::Player(0)
        } else {
            Owner::Nobody
        }
    };
    // Units: rows of 12 north of the middle (clear of a main hall standing there), each unit given room by its formation
    // spacing (at least 6 cells), so large machines don't overlap their neighbours.
    const UNIT_COLUMNS: usize = 12;
    let room = |d: &Definition| {
        d.dimensions
            .as_ref()
            .map_or(6.0, |m| (m.formation_spacing * 2.0).max(6.0))
    };
    let mut z = cz - 16.0;
    for (r, row) in units.chunks(UNIT_COLUMNS).enumerate() {
        let width: f64 = row.iter().map(|d| room(d)).sum();
        let depth = row.iter().map(|d| room(d)).fold(6.0, f64::max);
        z -= depth / 2.0;
        let mut x = cx - width / 2.0;
        for (c, d) in row.iter().enumerate() {
            let i = r * UNIT_COLUMNS + c;
            x += room(d) / 2.0;
            map.entities.push(Placement {
                activation: None,
                id: format!("gallery-unit-{i}"),
                map_knowledge: None,
                definition: d.id.clone(),
                position: Point {
                    x: x.round(),
                    y: z.round(),
                    surface: None,
                },
                rotation: 180.0,
                owner: owner(d),
                initial_state: None,
                appearance: None,
            });
            x += room(d) / 2.0;
        }
        z -= depth / 2.0;
    }
    // Buildings: rows of 6, 20 cells apart, south of the middle.
    const BUILDING_COLUMNS: usize = 6;
    for (i, d) in buildings.iter().enumerate() {
        let (row, column) = (i / BUILDING_COLUMNS, i % BUILDING_COLUMNS);
        let x = cx + (column as f64 - (BUILDING_COLUMNS as f64 - 1.0) / 2.0) * 20.0;
        let z = cz + 12.0 + row as f64 * 20.0;
        map.entities.push(Placement {
            activation: None,
            id: format!("gallery-building-{i}"),
            map_knowledge: None,
            definition: d.id.clone(),
            position: aligned(d, x, z),
            rotation: 0.0,
            owner: owner(d),
            initial_state: None,
            appearance: None,
        });
    }
    Ok(MatchSetup {
        seed: 1,
        slots: vec![Slot {
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    })
}

/// Trying an ability in a real match (the editor's encounter, docs/editor.md): `caster`
/// (a unit that binds the ability) stands a little way from player 0's start towards the
/// middle of the map, a hero at its third level so it can learn three ranks; three of its
/// race's other units stand beside it as allies to target, and a camp of the map's
/// neutral creeps waits ahead as enemies. Player 0 is the only player (no computer
/// players), on its start with its starting setup.
pub fn encounter(map: &mut UtcMap, content: &Content, caster: &str) -> Result<MatchSetup, String> {
    let definition = content
        .definitions
        .iter()
        .find(|d| d.id.as_str() == caster)
        .ok_or_else(|| format!("no unit {caster}"))?;
    let start = map.player_starts.first().ok_or("the map has no start")?;
    let n = map.player_starts.len().max(1) as f64;
    let (mx, mz) = (
        map.player_starts.iter().map(|s| s.x).sum::<f64>() / n,
        map.player_starts.iter().map(|s| s.z).sum::<f64>() / n,
    );
    // 24 cells from the start towards the middle (or east when there is one start).
    let (dx, dz) = (mx - start.x, mz - start.z);
    let length = (dx * dx + dz * dz).sqrt();
    let (ux, uz) = if length > 1.0 {
        (dx / length, dz / length)
    } else {
        (1.0, 0.0)
    };
    let (cx, cz) = ((start.x + ux * 24.0).floor(), (start.z + uz * 24.0).floor());
    let facing = math::atan2(ux, uz).to_degrees();
    let experience = definition
        .behaviors
        .progression
        .as_ref()
        .and_then(|p| p.levels.get(2).or(p.levels.last()))
        .map(|l| l.experience);
    let place = |id: String,
                 d: &Definition,
                 x: f64,
                 z: f64,
                 owner: Owner,
                 experience: Option<u32>| Placement {
        activation: None,
        id,
        map_knowledge: None,
        definition: d.id.clone(),
        // Units stand on navigation cell centres.
        position: Point {
            x: x.round(),
            y: z.round(),
            surface: None,
        },
        rotation: facing,
        owner,
        initial_state: experience.map(|experience| canopy_content::placement::InitialState {
            experience: Some(experience),
            ..Default::default()
        }),
        appearance: None,
    };
    map.entities.push(place(
        "encounter-caster".into(),
        definition,
        cx,
        cz,
        Owner::Player(0),
        experience,
    ));
    // Allies: the race's other trainable units (the caster's race is its id's second part).
    let race = caster.split('.').nth(1).unwrap_or_default();
    let allies: Vec<&Definition> = content
        .definitions
        .iter()
        .filter(|d| {
            d.kind == Kind::Unit
                && d.id.as_str() != caster
                && d.id.as_str().split('.').nth(1) == Some(race)
                && d.behaviors.progression.is_none()
        })
        .take(3)
        .collect();
    let (px, pz) = (-uz, ux);
    for (i, ally) in allies.iter().enumerate() {
        let side = i as f64 - 1.0;
        map.entities.push(place(
            format!("encounter-ally-{i}"),
            ally,
            cx - ux * 4.0 + px * side * 3.0,
            cz - uz * 4.0 + pz * side * 3.0,
            Owner::Player(0),
            None,
        ));
    }
    // Enemies: neutral creeps (units nobody's race lists) ahead.
    let in_roster = |id: &str| {
        content
            .rules
            .races
            .values()
            .any(|r| r.roster.iter().any(|x| x.as_str() == id))
    };
    let creeps: Vec<&Definition> = content
        .definitions
        .iter()
        .filter(|d| {
            d.kind == Kind::Unit && !in_roster(d.id.as_str()) && d.behaviors.progression.is_none()
        })
        .take(3)
        .collect();
    for (i, creep) in creeps.iter().enumerate() {
        let side = i as f64 - 1.0;
        map.entities.push(place(
            format!("encounter-enemy-{i}"),
            creep,
            cx + ux * 14.0 + px * side * 3.5,
            cz + uz * 14.0 + pz * side * 3.5,
            Owner::Nobody,
            None,
        ));
    }
    Ok(MatchSetup {
        seed: 1,
        slots: vec![Slot {
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    })
}
