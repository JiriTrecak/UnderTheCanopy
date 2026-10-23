//! Bridges (layered walk surfaces) and flying units.

mod movement_fixture;

use movement_fixture::*;
use serde_json::json;

/// A river across rows 60–72 with a bridge deck over it at x = 50.
fn river() -> canopy_content::map::UtcMap {
    let wet = |_: i32, y: i32| (60..=72).contains(&y);
    let ground = terrain(
        move |x, y| if wet(x, y) { -0.8 } else { 0.0 },
        move |x, y| if wet(x, y) { 0.4 } else { -1.0 },
    );
    let units = vec![
        unit("a", "unit.walker", 50, 50),
        unit("b", "unit.walker", 52, 82),
    ];
    map(
        units,
        ground,
        vec![
            json!({"id": "bridge.1", "asset": "scenery.asset.bridge-prop", "x": 50, "y": 66, "elevation": 0.6}),
        ],
    )
}

#[test]
fn walkers_cross_a_river_on_the_bridge_only() {
    let map = river();
    let mut world = world(&map);
    let (a, b) = (id(&world, "a"), id(&world, "b"));
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 70, "y": 90}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [b], "destination": {"x": 30, "y": 40}}),
    );
    let (mut a_on_deck, mut b_on_deck) = (false, false);
    for _ in 0..1600 {
        run(&mut world, 1);
        for (placement, seen) in [("a", &mut a_on_deck), ("b", &mut b_on_deck)] {
            let e = entity(&world, placement);
            let (x, y) = at(&world, placement);
            if (60.0..=72.0).contains(&y) {
                assert_eq!(
                    e.surface.as_deref(),
                    Some("bridge.1"),
                    "{placement} over the river at {x},{y}"
                );
                assert!((47.5..=52.5).contains(&x));
                *seen = true;
            }
        }
    }
    assert!(a_on_deck && b_on_deck);
    assert_eq!(at(&world, "a"), (70.0, 90.0));
    assert_eq!(at(&world, "b"), (30.0, 40.0));
    assert!(entity(&world, "a").surface.is_none() && entity(&world, "b").surface.is_none());
    assert!(errors(&world).is_empty(), "{:?}", errors(&world));
}

#[test]
fn bridge_routes_survive_a_save() {
    let map = river();
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 70, "y": 90}}),
    );
    let mut on_deck = 0;
    while entity(&world, "a").surface.is_none() {
        run(&mut world, 1);
        on_deck += 1;
        assert!(on_deck < 1000);
    }
    let mut copy = reload(&world, &map);
    for _ in 0..600 {
        world.step();
        copy.step();
        assert_eq!(world.state(), copy.state());
    }
}

#[test]
fn flyers_cross_walls_and_water_and_ignore_ground_bodies() {
    let wall = |x: i32, _| (59..=61).contains(&x);
    let mut units = vec![
        unit("f", "unit.flyer", 40, 50),
        unit("g", "unit.walker", 40, 60),
    ];
    // A block of walkers in the flyer's way.
    for i in 0..9 {
        units.push(unit(
            &format!("w{i}"),
            "unit.walker",
            48 + i % 3,
            49 + i / 3,
        ));
    }
    let map = map(units, walls(wall), vec![]);
    let mut world = world(&map);
    let (f, g) = (id(&world, "f"), id(&world, "g"));
    command(
        &mut world,
        json!({"type": "move", "actors": [f, g], "destination": {"x": 80, "y": 50}}),
    );
    let mut max_dy: f64 = 0.0;
    for _ in 0..700 {
        run(&mut world, 1);
        max_dy = max_dy.max((at(&world, "f").1 - 50.0).abs());
    }
    // Straight over the walkers and the cliff.
    assert_eq!(at(&world, "f"), (80.0, 50.0));
    assert!(max_dy < 0.01, "{max_dy}");
    // The cliff runs the whole map: the walker cannot follow.
    assert!(at(&world, "g").0 < 60.0);
    assert_eq!(errors(&world), ["message.move-destination-is-unreachable"]);
}
