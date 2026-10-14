//! Traffic: bodies never overlap or cut through terrain, crowds pass chokepoints, opposing
//! streams pass each other, deadlocks are negotiated and idle allies step aside.

mod movement_fixture;

use canopy_sim::{World, entity::EntityId, spatial::Fixed};
use movement_fixture::*;
use serde_json::json;

/// Runs `ticks`, checking after every tick that each unit's step was clear of terrain and
/// that no two ground bodies overlap more than the movement order allows.
fn run_checked(world: &mut World, ids: &[u32], ticks: u32) {
    let radius = |w: &World, id: u32| w.spatial().body_of(get(w, EntityId(id))).radius;
    for _ in 0..ticks {
        let before: Vec<Fixed> = ids
            .iter()
            .map(|&id| {
                let p = get(world, EntityId(id)).position;
                Fixed::new(p.x, p.y)
            })
            .collect();
        run(world, 1);
        for (i, &id) in ids.iter().enumerate() {
            let e = get(world, EntityId(id));
            let after = Fixed::new(e.position.x, e.position.y);
            let body = world.spatial().body_of(e);
            assert!(
                world.spatial().clear_segment(before[i], after, None, &body),
                "{id} cut through terrain"
            );
            for &other in &ids[i + 1..] {
                let o = get(world, EntityId(other));
                let gap = distance(
                    (
                        f64::from(e.position.x) / 1000.0,
                        f64::from(e.position.y) / 1000.0,
                    ),
                    (
                        f64::from(o.position.x) / 1000.0,
                        f64::from(o.position.y) / 1000.0,
                    ),
                );
                // Sequential movers may close in on space vacated within the tick.
                assert!(
                    gap >= radius(world, id) + radius(world, other) - 0.01,
                    "{id} and {other} overlap: {gap}"
                );
            }
        }
    }
}

fn idle(world: &World, id: u32) -> bool {
    let u = get(world, EntityId(id)).unit.as_ref().unwrap();
    u.order.is_none() && u.route.is_empty()
}

#[test]
fn opposing_formations_pass_through_each_other() {
    let mut units = Vec::new();
    for i in 0..8 {
        units.push(unit(
            &format!("w{i}"),
            "unit.walker",
            40 + i % 4,
            40 + i / 4,
        ));
        units.push(unit(
            &format!("e{i}"),
            "unit.walker",
            70 + i % 4,
            40 + i / 4,
        ));
    }
    let map = map(units, flat(), vec![]);
    let mut world = world(&map);
    let west: Vec<u32> = (0..8).map(|i| id(&world, &format!("w{i}"))).collect();
    let east: Vec<u32> = (0..8).map(|i| id(&world, &format!("e{i}"))).collect();
    command(
        &mut world,
        json!({"type": "move", "actors": west, "destination": {"x": 90, "y": 41}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": east, "destination": {"x": 20, "y": 41}}),
    );
    let all: Vec<u32> = west.iter().chain(&east).copied().collect();
    run_checked(&mut world, &all, 1400);
    for &id in &all {
        assert!(idle(&world, id), "{id} still moving");
    }
    for &id in &west {
        assert!(get(&world, EntityId(id)).position.x >= 85_000);
    }
    for &id in &east {
        assert!(get(&world, EntityId(id)).position.x <= 25_000);
    }
}

#[test]
fn a_crowd_squeezes_through_a_gap() {
    // A cliff wall at x = 60 with a three-cell gap at y 49–51.
    let wall = |x: i32, y: i32| (59..=61).contains(&x) && !(49..=51).contains(&y);
    let mut units = Vec::new();
    for i in 0..24 {
        units.push(unit(
            &format!("w{i}"),
            "unit.walker",
            30 + (i % 6) * 2,
            40 + (i / 6) * 2,
        ));
    }
    for i in 0..6 {
        units.push(unit(
            &format!("e{i}"),
            "unit.walker",
            80 + (i % 3) * 2,
            48 + (i / 3) * 2,
        ));
    }
    let map = map(units, walls(wall), vec![]);
    let mut world = world(&map);
    let west: Vec<u32> = (0..24).map(|i| id(&world, &format!("w{i}"))).collect();
    let east: Vec<u32> = (0..6).map(|i| id(&world, &format!("e{i}"))).collect();
    command(
        &mut world,
        json!({"type": "move", "actors": west, "destination": {"x": 90, "y": 50}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": east, "destination": {"x": 30, "y": 50}}),
    );
    let all: Vec<u32> = west.iter().chain(&east).copied().collect();
    run_checked(&mut world, &all, 3000);
    let through = west
        .iter()
        .filter(|&&id| get(&world, EntityId(id)).position.x > 62_000)
        .count()
        + east
            .iter()
            .filter(|&&id| get(&world, EntityId(id)).position.x < 58_000)
            .count();
    assert_eq!(through, all.len(), "everyone passes the gap");
}

#[test]
fn a_head_on_deadlock_in_a_doorway_is_negotiated() {
    // A cliff wall at x = 101 with a one-cell doorway at y = 100.
    let wall = |x: i32, y: i32| x == 101 && y != 100;
    let units = vec![
        unit("a", "unit.walker", 100, 100),
        unit("c", "unit.walker", 101, 100),
        unit("b", "unit.walker", 102, 100),
    ];
    let map = map(units, walls(wall), vec![]);
    let mut world = world(&map);
    let (a, b, c) = (id(&world, "a"), id(&world, "b"), id(&world, "c"));
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 110, "y": 100}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [b, c], "destination": {"x": 90, "y": 100}}),
    );
    let mut yielded = false;
    for _ in 0..1600 {
        run_checked(&mut world, &[a, b, c], 1);
        yielded |= [a, b, c].iter().any(|&id| {
            get(&world, EntityId(id))
                .unit
                .as_ref()
                .unwrap()
                .detour
                .as_ref()
                .is_some_and(|d| d.yielding.is_some())
        });
    }
    assert!(yielded, "someone stepped aside");
    assert!([a, b, c].iter().all(|&id| idle(&world, id)));
    assert!(get(&world, EntityId(a)).position.x > 105_000);
    assert!(
        get(&world, EntityId(b)).position.x < 95_000
            && get(&world, EntityId(c)).position.x < 95_000
    );
}

#[test]
fn an_idle_ally_in_the_way_steps_aside() {
    // A corridor one trooper wide (y 98–102) between cliffs, with a side pocket at x 100.
    let wall = |x: i32, y: i32| {
        !(98..=102).contains(&y)
            && !((96..=104).contains(&x) && (90..=110).contains(&y))
            && (80..=120).contains(&x)
    };
    let units = vec![
        unit("mover", "unit.trooper", 84, 100),
        unit("idler", "unit.trooper", 100, 100),
    ];
    let map = map(units, walls(wall), vec![]);
    let mut world = world(&map);
    let (mover, idler) = (id(&world, "mover"), id(&world, "idler"));
    command(
        &mut world,
        json!({"type": "move", "actors": [mover], "destination": {"x": 116, "y": 100}}),
    );
    run_checked(&mut world, &[mover, idler], 1200);
    assert_eq!(at(&world, "mover"), (116.0, 100.0));
    assert!(idle(&world, mover));
    assert_ne!(
        at(&world, "idler"),
        (100.0, 100.0),
        "the idle unit made room"
    );
}
