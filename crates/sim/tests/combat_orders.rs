//! Attack, attack-move, hold and stop orders; target choice and switching.

mod combat_fixture;

use canopy_sim::{World, entity::EntityId, orders::Order};
use combat_fixture::*;
use serde_json::json;

fn order(world: &World, placement: &str) -> Option<Order> {
    entity(world, placement).unit.as_ref()?.order.clone()
}

fn target(world: &World, placement: &str) -> Option<EntityId> {
    entity(world, placement).unit.as_ref()?.target
}

#[test]
fn attack_commands_are_checked_like_the_old_game() {
    let map = map(vec![
        placed("a", "unit.swordsman", 100.0, 100.0, 1),
        placed("b", "unit.swordsman", 100.0, 103.0, 1),
        placed("w", "unit.wasp", 140.0, 100.0, 2),
        placed("far", "unit.swordsman", 200.0, 200.0, 2),
    ]);
    let mut world = world(&map);
    let (a, b, w, far) = (
        id(&world, "a"),
        id(&world, "b"),
        id(&world, "w"),
        id(&world, "far"),
    );
    // Unseen, unreachable by the weapon, a friend without force, and someone else's unit.
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [a], "target": far}),
    );
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [a], "target": b}),
    );
    command(
        &mut world,
        2,
        json!({"type": "attack", "actors": [a], "target": b}),
    );
    run(&mut world, 1);
    assert_eq!(
        errors(&world),
        [
            "message.target-is-not-visible-and-damageable",
            "message.no-actors-support-that-order",
            "message.not-your-actors",
        ]
    );
    let _ = w;
    // Force attacks a friend until it dies; then the order ends.
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [a], "target": b, "force": true}),
    );
    run(&mut world, 1);
    assert_eq!(
        order(&world, "a"),
        Some(Order::Attack {
            target: EntityId(b),
            force: true
        })
    );
    assert_eq!(target(&world, "a"), Some(EntityId(b)));
    run_until(&mut world, 900, |w| !alive(w, "b"));
    run(&mut world, 1);
    assert_eq!(order(&world, "a"), None);
}

#[test]
fn queued_attacks_start_when_the_current_order_ends() {
    let map = map(vec![
        placed("a", "unit.swordsman", 100.0, 100.0, 1),
        placed("e", "unit.knight", 112.0, 100.0, 2),
    ]);
    let mut world = world(&map);
    let (a, e) = (id(&world, "a"), id(&world, "e"));
    command(&mut world, 1, json!({"type": "hold", "actors": [a]}));
    run(&mut world, 1);
    command(
        &mut world,
        1,
        json!({"type": "move", "actors": [a], "destination": {"x": 100, "y": 90}, "append": true}),
    );
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [a], "target": e, "append": true}),
    );
    run(&mut world, 1);
    // Hold never ends by itself: both wait in the queue.
    assert_eq!(order(&world, "a"), Some(Order::Hold));
    assert_eq!(
        entity(&world, "a").unit.as_ref().unwrap().order_queue.len(),
        2
    );
    command(&mut world, 1, json!({"type": "stop", "actors": [a]}));
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [a], "target": e, "append": true}),
    );
    run(&mut world, 2);
    assert_eq!(
        order(&world, "a"),
        Some(Order::Attack {
            target: EntityId(e),
            force: false
        })
    );
}

#[test]
fn holding_units_fight_only_what_comes_into_reach() {
    let map = map(vec![
        placed("a", "unit.swordsman", 100.0, 100.0, 1),
        placed("e", "unit.bowman", 108.0, 100.0, 2),
    ]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(&mut world, 1, json!({"type": "hold", "actors": [a]}));
    run(&mut world, 200);
    // Shot at from range, it stays put.
    let e = entity(&world, "a");
    assert_eq!((e.position.x, e.position.y), (100_000, 100_000));
    assert!(hp(&world, "a") < 100);
    assert_eq!(target(&world, "a"), None);
}

#[test]
fn attack_move_engages_on_the_way_and_carries_on() {
    let map = map(vec![
        placed("a", "unit.knight", 100.0, 100.0, 1),
        placed("e", "unit.swordsman", 112.0, 101.0, 2),
    ]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        1,
        json!({"type": "move", "actors": [a], "destination": {"x": 140, "y": 100}, "attackMove": true}),
    );
    run_until(&mut world, 900, |w| !alive(w, "e"));
    assert!(matches!(
        order(&world, "a"),
        Some(Order::Move {
            attack_move: true,
            ..
        })
    ));
    run_until(&mut world, 900, |w| order(w, "a").is_none());
    let k = entity(&world, "a");
    assert_eq!((k.position.x, k.position.y), (140_000, 100_000));
}

#[test]
fn a_plain_move_ignores_enemies() {
    let map = map(vec![
        placed("a", "unit.knight", 100.0, 100.0, 1),
        placed("e", "unit.bowman", 108.0, 104.0, 2),
    ]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(
        &mut world,
        1,
        json!({"type": "move", "actors": [a], "destination": {"x": 130, "y": 100}}),
    );
    for _ in 0..300 {
        world.step();
        assert_eq!(target(&world, "a"), None);
    }
}

#[test]
fn units_pick_the_nearest_enemy_then_the_lowest_id() {
    let map = map(vec![
        placed("a", "unit.swordsman", 100.0, 100.0, 1),
        // Equally far: the lower id wins.
        placed("e1", "unit.swordsman", 106.0, 100.0, 2),
        placed("e2", "unit.swordsman", 94.0, 100.0, 2),
        placed("e3", "unit.knight", 100.0, 107.0, 2),
    ]);
    let mut world = world(&map);
    let e1 = EntityId(id(&world, "e1"));
    run(&mut world, 2);
    assert_eq!(target(&world, "a"), Some(e1));
}
