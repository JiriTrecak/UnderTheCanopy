//! Repeatability, save/restore mid-move, and derived spatial data: occupancy maintained
//! incrementally answers exactly like a cold rebuild.

mod movement_fixture;

use canopy_content::placement::Placement;
use canopy_sim::{World, catalog::Catalog, entity::EntityId, spatial::Spot, spawn};
use movement_fixture::*;
use serde_json::json;

fn crowd() -> canopy_content::map::UtcMap {
    let wall = |x: i32, y: i32| (79..=81).contains(&x) && !(58..=62).contains(&y);
    let mut units = Vec::new();
    for i in 0..20 {
        units.push(unit(
            &format!("w{i:02}"),
            "unit.walker",
            40 + (i % 5) * 2,
            52 + (i / 5) * 2,
        ));
    }
    for i in 0..4 {
        units.push(unit(
            &format!("t{i}"),
            "unit.trooper",
            120 + (i % 2) * 4,
            56 + (i / 2) * 4,
        ));
    }
    map(units, walls(wall), vec![])
}

fn order(world: &mut World) {
    let west: Vec<u32> = (0..20).map(|i| id(world, &format!("w{i:02}"))).collect();
    let east: Vec<u32> = (0..4).map(|i| id(world, &format!("t{i}"))).collect();
    command(
        world,
        json!({"type": "move", "actors": west, "destination": {"x": 130, "y": 60}}),
    );
    command(
        world,
        json!({"type": "move", "actors": east, "destination": {"x": 30, "y": 60}}),
    );
}

fn hall(world: &mut World, id: &str, x: f64, y: f64) -> EntityId {
    let placement: Placement = serde_json::from_value(json!({
        "id": id, "definition": "building.hall", "position": {"x": x, "y": y}, "rotation": 0,
        "owner": "player.1", "initialState": {"construction": "complete"},
    }))
    .unwrap();
    let catalog = Catalog::new(content());
    spawn::create(world.state_mut(), &catalog, &placement).unwrap()
}

#[test]
fn identical_commands_give_identical_matches() {
    let map = crowd();
    let (mut a, mut b) = (world(&map), world(&map));
    order(&mut a);
    order(&mut b);
    for _ in 0..800 {
        a.step();
        b.step();
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
    assert_eq!(a.state(), b.state());
}

#[test]
fn a_restored_match_continues_identically_mid_move() {
    let map = crowd();
    let mut world = world(&map);
    order(&mut world);
    run(&mut world, 333);
    // Units are mid-segment, some detouring.
    assert!(
        world
            .state()
            .entities
            .iter()
            .any(|e| e.unit.as_ref().is_some_and(|u| u.segment.is_some()))
    );
    let mut copy = reload(&world, &map);
    assert_eq!(copy.state(), world.state());
    for _ in 0..900 {
        world.step();
        copy.step();
        assert_eq!(world.state(), copy.state());
    }
}

/// Every walkable flag and a sample of routes agree between two worlds' spatial models.
fn same_spatial(a: &mut World, b: &mut World) {
    let nodes = a.spatial().node_count();
    assert_eq!(nodes, b.spatial().node_count());
    for node in 0..nodes {
        assert_eq!(
            a.spatial().walkable(node),
            b.spatial().walkable(node),
            "node {node}"
        );
    }
    let size = a.spatial().size();
    let walker = a.spatial().body_of(entity(a, "w00"));
    let trooper = a.spatial().body_of(entity(a, "t0"));
    for (from, to) in [
        ((30, 30), (150, 90)),
        ((20, 60), (140, 60)),
        ((100, 20), (60, 100)),
        ((84, 70), (76, 50)),
    ] {
        let (s, g) = (from.1 * size + from.0, to.1 * size + to.0);
        for body in [&walker, &trooper] {
            let pa = a.spatial_mut().find_path(s, g, None, i64::MAX, body, None);
            let pb = b.spatial_mut().find_path(s, g, None, i64::MAX, body, None);
            assert_eq!(pa, pb, "{from:?} → {to:?}");
        }
    }
    assert!(a.spatial().node(Spot::new(0, 0, None)).is_some());
}

#[test]
fn incremental_occupancy_matches_a_cold_rebuild() {
    let map = crowd();
    let mut world = world(&map);
    order(&mut world);
    run(&mut world, 30);
    // A hall appears across the gap's approach, another elsewhere, then one goes away.
    let blocker = hall(&mut world, "new.hall.1", 73.5, 61.5);
    hall(&mut world, "new.hall.2", 101.5, 41.5);
    run(&mut world, 170);
    world.state_mut().entities.remove(blocker);
    run(&mut world, 60);
    let mut copy = reload(&world, &map);
    same_spatial(&mut world, &mut copy);
    for _ in 0..600 {
        world.step();
        copy.step();
        assert_eq!(world.state(), copy.state());
    }
    same_spatial(&mut world, &mut copy);
}

#[test]
fn units_replan_around_a_new_building() {
    let map = map(vec![unit("a", "unit.walker", 20, 61)], flat(), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 60, "y": 61}}),
    );
    run(&mut world, 40);
    hall(&mut world, "wall", 41.5, 61.5);
    for _ in 0..800 {
        run(&mut world, 1);
        let (x, y) = at(&world, "a");
        // The hall covers 39.5–43.5 × 59.5–63.5; the body is 0.4 cells wide each way.
        assert!(
            !((39.1..43.9).contains(&x) && (59.1..63.9).contains(&y)),
            "walked into the hall at {x},{y}"
        );
    }
    assert_eq!(at(&world, "a"), (60.0, 61.0));
}
