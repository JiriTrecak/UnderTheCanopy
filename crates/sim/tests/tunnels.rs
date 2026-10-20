//! Tunnels: units ordered far away travel underground between their owner's mounds when
//! that is faster, one at a time through each mound, and come up where a fallen exit stood.

mod combat_fixture;

use canopy_sim::{World, entity::EntityId};
use combat_fixture::*;
use serde_json::json;

fn underground(world: &World, placement: &str) -> bool {
    entity(world, placement)
        .unit
        .as_ref()
        .is_some_and(|u| u.contained.is_some())
}

fn cell(world: &World, placement: &str) -> (i32, i32) {
    let p = entity(world, placement).position;
    (p.x / 1000, p.y / 1000)
}

fn near(world: &World, placement: &str, x: i32, y: i32) -> bool {
    let (cx, cy) = cell(world, placement);
    (cx - x).abs() <= 3 && (cy - y).abs() <= 3
}

/// Two mounds 200 cells apart, units beside the western one.
fn mounds(units: &[(&str, &str, f64)]) -> World {
    let mut entities = vec![
        placed("west", "building.mound", 21.5, 101.5, 1),
        placed("east", "building.mound", 221.5, 101.5, 1),
    ];
    for &(id, definition, y) in units {
        entities.push(placed(id, definition, 26.0, y, 1));
    }
    world(&map(entities))
}

fn go(world: &mut World, actors: &[&str], x: i32, y: i32) {
    let actors: Vec<u32> = actors.iter().map(|a| id(world, a)).collect();
    command(
        world,
        1,
        json!({"type": "move", "actors": actors, "destination": {"x": x, "y": y}}),
    );
}

#[test]
fn a_long_march_goes_underground_and_arrives_much_sooner() {
    let mut world = mounds(&[("a", "unit.swordsman", 104.0)]);
    go(&mut world, &["a"], 224, 110);
    // Walking 200 cells at 4 a second takes 50 seconds; the tunnel takes a fraction.
    let went_under = run_until(&mut world, 400, |w| underground(w, "a"));
    assert!(went_under < 400);
    // Underground it is gone from the map: no enemy can see or hit it.
    let view = world.view(player(2));
    let a = id(&world, "a");
    assert!(!view.entities.iter().any(|e| e.id().0 == a));
    let arrived = run_until(&mut world, 1200, |w| {
        w.state()
            .entities
            .get(EntityId(id(w, "a")))
            .unwrap()
            .unit
            .as_ref()
            .unwrap()
            .order
            .is_none()
    });
    assert!(arrived < 1200, "arrived after {arrived} more ticks");
    assert!(near(&world, "a", 224, 110));
    assert!(!underground(&world, "a"));
}

#[test]
fn a_short_walk_stays_above_ground() {
    let mut world = mounds(&[("a", "unit.swordsman", 104.0)]);
    go(&mut world, &["a"], 46, 104);
    for _ in 0..300 {
        run(&mut world, 1);
        assert!(!underground(&world, "a"));
    }
    assert!(near(&world, "a", 46, 104));
}

#[test]
fn units_too_big_for_tunnels_walk() {
    let mut world = mounds(&[("b", "unit.brute", 104.0)]);
    go(&mut world, &["b"], 224, 110);
    for _ in 0..400 {
        run(&mut world, 1);
        assert!(!underground(&world, "b"));
    }
}

#[test]
fn a_mound_lets_one_unit_under_at_a_time() {
    let mut world = mounds(&[
        ("a", "unit.swordsman", 98.0),
        ("b", "unit.swordsman", 104.0),
        ("c", "unit.swordsman", 110.0),
    ]);
    go(&mut world, &["a", "b", "c"], 224, 104);
    let mut entered: Vec<u32> = Vec::new();
    for _ in 0..600 {
        run(&mut world, 1);
        let under = ["a", "b", "c"]
            .iter()
            .filter(|a| underground(&world, a))
            .count();
        if under > entered.len() {
            entered.push(world.state().tick);
        }
        if entered.len() == 3 {
            break;
        }
    }
    assert_eq!(entered.len(), 3);
    assert!(entered.windows(2).all(|w| w[1] - w[0] >= 20), "{entered:?}");
}

#[test]
fn a_fallen_exit_spills_its_travellers_where_it_stood() {
    let mut world = mounds(&[("a", "unit.swordsman", 104.0)]);
    go(&mut world, &["a"], 236, 104);
    run_until(&mut world, 400, |w| underground(w, "a"));
    let east = EntityId(id(&world, "east"));
    world
        .state_mut()
        .combat
        .scripted
        .push(canopy_sim::combat::ScriptedHit {
            target: east,
            source: None,
            amount: 1,
        });
    world.state_mut().entities.get_mut(east).unwrap().hp = Some(1);
    run(&mut world, 2);
    assert!(find(&world, "east").is_none());
    assert!(!underground(&world, "a"));
    assert!(near(&world, "a", 221, 101) || cell(&world, "a").0 > 200);
}
