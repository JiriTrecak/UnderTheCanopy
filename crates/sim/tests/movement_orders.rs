//! Move, stop, hold, patrol, follow and queued orders on test-owned content and maps.

mod movement_fixture;

use canopy_sim::orders::Order;
use movement_fixture::*;
use serde_json::json;

fn order(world: &canopy_sim::World, placement: &str) -> Option<Order> {
    entity(world, placement)
        .unit
        .as_ref()
        .unwrap()
        .order
        .clone()
}

#[test]
fn a_walker_crosses_open_ground_and_stops_on_the_cell() {
    let map = map(vec![unit("a", "unit.walker", 20, 20)], flat(), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 60, "y": 30}}),
    );
    run(&mut world, 2);
    assert!(order(&world, "a").is_some());
    let motion = |world: &canopy_sim::World| {
        let view = world.view(canopy_content::placement::Owner::Player(0));
        let e = view.entities.iter().find(|e| e.id().0 == a).unwrap();
        assert!(e.private.as_ref().unwrap().order == order(world, "a"));
        e.motion.unwrap().moving
    };
    // It walks off at once, turning as it goes (D51).
    assert!(motion(&world));
    // Speed 4 cells/s: 41.2 cells take a little over 10 s.
    run(&mut world, 452);
    assert_eq!(at(&world, "a"), (60.0, 30.0));
    let u = entity(&world, "a").unit.as_ref().unwrap();
    assert!(u.order.is_none() && u.route.is_empty(), "{u:?}");
    assert!(!motion(&world));
    assert!(errors(&world).is_empty(), "{:?}", errors(&world));
}

#[test]
fn walkers_go_around_a_cliff_and_never_cross_it() {
    // A wall at x = 40 for y ≤ 60; the way round is past its southern end.
    let map = map(
        vec![
            unit("a", "unit.walker", 30, 30),
            unit("t", "unit.trooper", 30, 40),
        ],
        walls(|x, y| (39..=41).contains(&x) && y <= 60),
        vec![],
    );
    let mut world = world(&map);
    let (a, t) = (id(&world, "a"), id(&world, "t"));
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 50, "y": 30}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [t], "destination": {"x": 50, "y": 40}}),
    );
    let mut deepest: f64 = 0.0;
    for _ in 0..1600 {
        run(&mut world, 1);
        for p in [at(&world, "a"), at(&world, "t")] {
            assert!(
                !(38.5..=41.5).contains(&p.0) || p.1 > 60.0,
                "inside the wall at {p:?}"
            );
        }
        deepest = deepest.max(at(&world, "a").1);
    }
    assert_eq!(at(&world, "a"), (50.0, 30.0));
    assert_eq!(at(&world, "t"), (50.0, 40.0));
    assert!(deepest > 60.0, "went round the end of the wall");
}

#[test]
fn a_body_wedged_across_a_cliff_edge_steps_free_and_goes_where_it_is_sent() {
    // A 3 m cliff along x = 99.5 (cells from x = 100 up are high). A wide body that slid
    // along it stands across the edge where every sweep to a cell centre around it crosses
    // the step: no route can start from there.
    let map = map(
        vec![unit("t", "unit.trooper", 100, 50)],
        walls(|x, _| x >= 100),
        vec![],
    );
    let mut world = world(&map);
    let t = id(&world, "t");
    // Placements sit on half cells; the slide leaves it off them.
    let e = world
        .state_mut()
        .entities
        .get_mut(canopy_sim::entity::EntityId(t))
        .unwrap();
    e.position.x = 100_400;
    let e = e.clone();
    world.spatial_mut().update_body(&e);
    assert_eq!(at(&world, "t"), (100.4, 50.0), "placed across the edge");
    let spatial = world.spatial();
    let body = spatial.body_of(&e);
    let from = canopy_sim::spatial::Fixed::new(e.position.x, e.position.y);
    let wedged = (99..=101).all(|x| {
        (49..=51).all(|y| {
            !spatial.clear_segment(
                from,
                canopy_sim::spatial::Fixed::cell(x, y, None),
                None,
                &body,
            )
        })
    });
    assert!(wedged, "the set-up wedges the body");
    command(
        &mut world,
        json!({"type": "move", "actors": [t], "destination": {"x": 120, "y": 50}}),
    );
    run(&mut world, 400);
    assert_eq!(at(&world, "t"), (120.0, 50.0));
    assert!(errors(&world).is_empty(), "{:?}", errors(&world));
}

#[test]
fn an_unreachable_destination_ends_the_order_with_an_error() {
    // A closed ring of cliffs around (80, 80).
    let ring = |x: i32, y: i32| {
        let d = (x - 80).abs().max((y - 80).abs());
        d == 10
    };
    let map = map(vec![unit("a", "unit.walker", 20, 20)], walls(ring), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 80, "y": 80}}),
    );
    run(&mut world, 5);
    assert_eq!(errors(&world), ["Move destination is unreachable"]);
    assert!(order(&world, "a").is_none());
    assert_eq!(at(&world, "a"), (20.0, 20.0));
}

#[test]
fn stop_halts_at_once_and_clears_the_queue() {
    let map = map(vec![unit("a", "unit.walker", 20, 20)], flat(), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 80, "y": 20}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 80, "y": 80}, "append": true}),
    );
    run(&mut world, 100);
    let before = at(&world, "a");
    assert!(before.0 > 25.0);
    command(&mut world, json!({"type": "stop", "actors": [a]}));
    run(&mut world, 1);
    let u = entity(&world, "a").unit.as_ref().unwrap();
    assert!(u.order.is_none() && u.order_queue.is_empty() && u.route.is_empty());
    let stopped = at(&world, "a");
    run(&mut world, 100);
    assert_eq!(at(&world, "a"), stopped);
    assert!(stopped.0 - before.0 < 0.2);
}

#[test]
fn queued_orders_run_in_turn_and_the_queue_is_bounded() {
    let map = map(vec![unit("a", "unit.walker", 20, 20)], flat(), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 30, "y": 20}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 30, "y": 30}, "append": true}),
    );
    command(
        &mut world,
        json!({"type": "hold", "actors": [a], "append": true}),
    );
    run(&mut world, 1);
    assert_eq!(
        entity(&world, "a").unit.as_ref().unwrap().order_queue.len(),
        2
    );
    run(&mut world, 200);
    assert!((at(&world, "a").0 - 30.0).abs() < 1e-9 && at(&world, "a").1 > 20.0);
    run(&mut world, 200);
    assert_eq!(at(&world, "a"), (30.0, 30.0));
    assert_eq!(order(&world, "a"), Some(Order::Hold));
    // Sixteen queued orders fill the queue; the seventeenth is refused.
    for _ in 0..17 {
        command(
            &mut world,
            json!({"type": "hold", "actors": [a], "append": true}),
        );
    }
    run(&mut world, 1);
    assert_eq!(
        entity(&world, "a").unit.as_ref().unwrap().order_queue.len(),
        16
    );
    assert_eq!(errors(&world), ["No actors support that order"]);
}

#[test]
fn hold_keeps_a_unit_in_place() {
    let map = map(vec![unit("a", "unit.walker", 20, 20)], flat(), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "move", "actors": [a], "destination": {"x": 80, "y": 20}}),
    );
    run(&mut world, 40);
    command(&mut world, json!({"type": "hold", "actors": [a]}));
    run(&mut world, 1);
    let held = at(&world, "a");
    run(&mut world, 200);
    assert_eq!(at(&world, "a"), held);
    assert_eq!(order(&world, "a"), Some(Order::Hold));
}

#[test]
fn patrols_walk_back_and_forth() {
    let map = map(vec![unit("a", "unit.walker", 20, 20)], flat(), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        json!({"type": "patrol", "actors": [a], "destination": {"x": 30, "y": 20}}),
    );
    let mut arrivals = Vec::new();
    for _ in 0..800 {
        run(&mut world, 1);
        let p = at(&world, "a");
        if (p == (30.0, 20.0) || p == (20.0, 20.0)) && arrivals.last() != Some(&p) {
            arrivals.push(p);
        }
    }
    assert!(arrivals.len() >= 4, "{arrivals:?}");
    assert!(arrivals.windows(2).all(|w| w[0] != w[1]));
    assert!(matches!(order(&world, "a"), Some(Order::Patrol { .. })));
}

#[test]
fn followers_keep_close_to_a_moving_leader() {
    let map = map(
        vec![
            unit("leader", "unit.walker", 40, 40),
            unit("f", "unit.walker", 20, 20),
        ],
        flat(),
        vec![],
    );
    let mut world = world(&map);
    let (leader, f) = (id(&world, "leader"), id(&world, "f"));
    command(
        &mut world,
        json!({"type": "follow", "actors": [f], "target": leader}),
    );
    run(&mut world, 300);
    assert!(distance(at(&world, "f"), at(&world, "leader")) < 3.5);
    command(
        &mut world,
        json!({"type": "move", "actors": [leader], "destination": {"x": 90, "y": 60}}),
    );
    run(&mut world, 900);
    assert!(
        distance(at(&world, "f"), at(&world, "leader")) < 3.5,
        "{:?} {:?}",
        at(&world, "f"),
        at(&world, "leader")
    );
    assert!(matches!(order(&world, "f"), Some(Order::Follow { .. })));
    // A unit cannot follow itself, and following needs a friendly unit.
    command(
        &mut world,
        json!({"type": "follow", "actors": [f], "target": f}),
    );
    command(
        &mut world,
        json!({"type": "follow", "actors": [f], "target": 9999}),
    );
    run(&mut world, 1);
    assert_eq!(
        errors(&world),
        [
            "No actors support that order",
            "Follow requires a visible friendly unit"
        ]
    );
}

fn formation_goals(spacing: i32) -> Vec<(i32, i32)> {
    let units: Vec<_> = (0..9)
        .map(|i| {
            unit(
                &format!("u{i}"),
                "unit.walker",
                20 + (i % 3) * spacing,
                20 + (i / 3) * spacing,
            )
        })
        .collect();
    let map = map(units, flat(), vec![]);
    let mut world = world(&map);
    let ids: Vec<u32> = (0..9).map(|i| id(&world, &format!("u{i}"))).collect();
    command(
        &mut world,
        json!({"type": "move", "actors": ids, "destination": {"x": 70, "y": 70}}),
    );
    run(&mut world, 1);
    let goals: Vec<(i32, i32)> = (0..9)
        .map(|i| match order(&world, &format!("u{i}")) {
            Some(Order::Move { destination, .. }) => (destination.x, destination.y),
            other => panic!("{other:?}"),
        })
        .collect();
    run(&mut world, 900);
    for (i, goal) in goals.iter().enumerate() {
        assert_eq!(
            at(&world, &format!("u{i}")),
            (f64::from(goal.0), f64::from(goal.1))
        );
    }
    goals
}

#[test]
fn group_moves_keep_a_compact_shape_and_gather_loose_ones() {
    // Adjacent units keep their block, translated onto the destination.
    let compact = formation_goals(1);
    assert_eq!(
        compact,
        (0..9).map(|i| (69 + i % 3, 69 + i / 3)).collect::<Vec<_>>()
    );
    // A loose group gathers into the nearest free cells, nearest unit to nearest slot.
    let mut loose = formation_goals(3);
    loose.sort_unstable();
    assert_eq!(
        loose,
        (0..9).map(|i| (69 + i / 3, 69 + i % 3)).collect::<Vec<_>>()
    );
}

#[test]
fn attack_moves_need_a_weapon_and_keep_the_flag() {
    let map = map(
        vec![
            unit("w", "unit.walker", 20, 20),
            unit("f", "unit.fighter", 24, 20),
        ],
        flat(),
        vec![],
    );
    let mut world = world(&map);
    let (w, f) = (id(&world, "w"), id(&world, "f"));
    command(
        &mut world,
        json!({"type": "move", "actors": [w, f], "destination": {"x": 40, "y": 40}, "attackMove": true}),
    );
    run(&mut world, 1);
    assert!(order(&world, "w").is_none());
    assert!(matches!(
        order(&world, "f"),
        Some(Order::Move {
            attack_move: true,
            ..
        })
    ));
    run(&mut world, 600);
    assert_eq!(at(&world, "f"), (40.0, 40.0));
}

#[test]
fn commands_are_validated_like_the_old_game() {
    let map = map(
        vec![
            unit("w", "unit.walker", 20, 20),
            unit("post", "unit.post", 30, 20),
        ],
        flat(),
        vec![],
    );
    let mut world = world(&map);
    let (w, post) = (id(&world, "w"), id(&world, "post"));
    command(
        &mut world,
        json!({"type": "move", "actors": [9999], "destination": {"x": 40, "y": 40}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [w], "destination": {"x": 400, "y": 40}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [w], "destination": {"x": 40, "y": 40, "surface": "no-such-deck"}}),
    );
    command(
        &mut world,
        json!({"type": "move", "actors": [post], "destination": {"x": 40, "y": 40}}),
    );
    command(
        &mut world,
        json!({"type": "patrol", "actors": [post], "destination": {"x": 40, "y": 40}}),
    );
    run(&mut world, 1);
    assert_eq!(
        errors(&world),
        [
            "No eligible controlled actors",
            "Destination is not on a declared walk surface",
            "Destination is not on a declared walk surface",
            "No actors support that order",
            "No actors support that order",
        ]
    );
}
