//! Vision stays deterministic and survives save and restore: many sensors wandering over
//! hills, stationary entities appearing, changing and vanishing, temporary sight, allies.
//! The incrementally maintained coverage must equal a rebuild from scratch throughout.

mod vision_fixture;

use canopy_content::placement::{Owner, Placement, Point};
use canopy_sim::{
    World,
    catalog::Catalog,
    entity::{EntityId, Milli},
    spawn,
    vision::{self, SpellVision},
};
use serde_json::Value;
use vision_fixture::*;

const UNITS: usize = 24;
const TEAMS: [Option<u8>; 3] = [Some(1), Some(1), None];

fn document() -> Value {
    let kinds = ["unit.scout", "unit.watcher", "unit.flyer", "unit.mole"];
    let mut entities: Vec<Value> = (0..UNITS)
        .map(|i| {
            place(
                &format!("u{i:02}"),
                kinds[i % kinds.len()],
                &format!("player.{}", i % 3 + 1),
                40.0 + (i * 5) as f64,
                60.0 + (i % 5 * 9) as f64,
            )
        })
        .collect();
    for (i, x) in [61.5, 101.5, 141.5].into_iter().enumerate() {
        entities.push(place(
            &format!("tower{i}"),
            "building.tower",
            &format!("player.{}", i + 1),
            x,
            121.5,
        ));
    }
    for i in 0..30 {
        entities.push(place(
            &format!("tree{i:02}"),
            "resource.tree",
            "none",
            35.0 + (i * 6) as f64,
            95.0 + (i % 4) as f64,
        ));
    }
    // A ridge with a plateau, so height filtering and flight both matter.
    testbed(
        entities,
        |x, z| {
            if (90..110).contains(&x) && z > 70 {
                4.0
            } else {
                0.5 * ((x / 30) % 2) as f32
            }
        },
        true,
    )
}

/// One tick of outside changes: movement, harvests, construction damage, felling,
/// planting and spells.
fn drive(world: &mut World, catalog: &Catalog, units: &[EntityId], trees: &[EntityId]) {
    let tick = world.state().tick;
    for (i, &id) in units.iter().enumerate() {
        let phase = (tick as i32 + i as i32 * 13) % 80;
        let offset = if phase < 40 { phase } else { 80 - phase };
        let at = Milli {
            x: (40 + i as i32 * 5) * 1000 + offset * 650,
            y: (60 + (i as i32 % 5) * 9) * 1000 + offset * (i as i32 % 3 - 1) * 400,
        };
        put(world, id, at);
    }
    let state = world.state_mut();
    if tick % 13 == 5 {
        let tree = trees[(tick as usize / 13) % trees.len()];
        if let Some(resource) = state
            .entities
            .get_mut(tree)
            .and_then(|e| e.resource.as_mut())
        {
            resource.amount = resource.amount.saturating_sub(1);
        }
    }
    if tick % 29 == 7 {
        state
            .entities
            .remove(trees[(tick as usize / 29) % trees.len()]);
    }
    if tick % 31 == 3 {
        let placement = Placement {
            activation: None,
            id: format!("planted{tick}"),
            map_knowledge: None,
            definition: canopy_content::Id::new("resource.tree").unwrap(),
            position: Point {
                x: f64::from(30 + tick % 150),
                y: 100.0,
                surface: None,
            },
            rotation: 0.0,
            owner: Owner::Nobody,
            initial_state: None,
            appearance: None,
        };
        spawn::create(state, catalog, &placement).unwrap();
    }
    if tick % 17 == 2 {
        // The towers: the only entities that are neither units nor resources.
        for tower in state
            .entities
            .iter_mut()
            .filter(|e| e.unit.is_none() && e.resource.is_none())
        {
            tower.hp = tower.hp.map(|hp| hp.saturating_sub(3).max(1));
        }
    }
    if tick % 23 == 11 {
        vision::add_spell_vision(
            state,
            SpellVision {
                id: 0,
                owner: Owner::Player((tick % 3) as u8),
                point: Milli {
                    x: (20 + tick as i32 % 200) * 1000 + 300,
                    y: 150_500,
                },
                surface: None,
                elevation: 0,
                radius: 5500,
                ignore_terrain: tick.is_multiple_of(2),
                detect_invisible: false,
                expires: tick + 15,
                caster: None,
            },
        );
    }
}

struct Run {
    world: World,
    catalog: Catalog,
    units: Vec<EntityId>,
    trees: Vec<EntityId>,
}

impl Run {
    fn new(world: World) -> Self {
        let units = (0..UNITS)
            .map(|i| id(&world, &format!("u{i:02}")))
            .collect();
        let trees = (0..30)
            .map(|i| id(&world, &format!("tree{i:02}")))
            .collect();
        Self {
            world,
            catalog: Catalog::new(content()),
            units,
            trees,
        }
    }

    fn ticks(&mut self, n: u32, check_every: u32) {
        for _ in 0..n {
            drive(&mut self.world, &self.catalog, &self.units, &self.trees);
            self.world.step();
            if self.world.state().tick.is_multiple_of(check_every) {
                verify(&self.world);
            }
        }
    }
}

/// Everything each player's view shows, comparable.
fn views(world: &World) -> Vec<String> {
    (0..3)
        .map(|p| {
            let view = world.view(Owner::Player(p));
            let fog = view.fog.as_ref().unwrap();
            format!(
                "{:?}|{:?}|{:?}|{:?}",
                view.entities,
                view.scenery.iter().collect::<Vec<_>>(),
                fog.cells,
                view.deaths
            )
        })
        .collect()
}

#[test]
fn incremental_vision_matches_a_rebuild_and_survives_save_and_restore() {
    let mut first = Run::new(start(document(), &TEAMS));
    first.ticks(60, 5);
    let saved = reload_on(&first.world, document());
    verify(&saved);
    assert_eq!(saved.state(), first.world.state());
    assert_eq!(views(&saved), views(&first.world));
    let mut resumed = Run {
        world: saved,
        catalog: Catalog::new(content()),
        units: first.units.clone(),
        trees: first.trees.clone(),
    };
    first.ticks(90, 10);
    resumed.ticks(90, 10);
    assert_eq!(resumed.world.state(), first.world.state());
    assert_eq!(views(&resumed.world), views(&first.world));

    // And again from scratch: identical.
    let mut second = Run::new(start(document(), &TEAMS));
    second.ticks(150, 1000);
    assert_eq!(second.world.state(), first.world.state());
    // Allies share fog and memory; the third player has its own.
    let knowledge = &first.world.state().knowledge;
    assert_eq!(knowledge.teams.len(), 2);
    assert!(
        first.world.view(Owner::Player(0)).fog.unwrap().cells
            == first.world.view(Owner::Player(1)).fog.unwrap().cells
    );
}

#[test]
fn saves_must_fit_the_match() {
    let world = start(document(), &TEAMS);
    let save = world.save();
    let mut other = save.clone();
    other.setup = setup(&[None, None, None]);
    assert!(World::restore(content(), &parse(document()), other).is_err());
    let mut cut = save;
    let cells = &mut cut.state.knowledge.teams[0].cells;
    *cells = vision::FogCells::filled(cells.len() - 1, 0);
    assert!(World::restore(content(), &parse(document()), cut).is_err());
}
