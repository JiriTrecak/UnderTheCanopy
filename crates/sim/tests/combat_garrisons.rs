//! Single-slot lookouts: entering, height, shooting, immunity, leaving.

mod combat_fixture;

use canopy_sim::{World, entity::EntityId, vision::SightRules};
use combat_fixture::*;
use serde_json::json;

fn lookout(world: &World, placement: &str) -> Option<EntityId> {
    entity(world, placement)
        .unit
        .as_ref()?
        .garrison
        .as_ref()
        .map(|g| g.building)
}

fn tower_map(enemies: bool) -> canopy_content::map::UtcMap {
    let mut entities = vec![
        placed("t", "building.tower", 101.5, 101.5, 1),
        placed("a", "unit.bowman", 96.0, 100.0, 1),
        placed("a2", "unit.bowman", 92.0, 100.0, 1),
        placed("w", "unit.swordsman", 92.0, 104.0, 1),
    ];
    if enemies {
        entities.push(placed("e1", "unit.swordsman", 121.0, 100.0, 2));
    }
    map(entities)
}

#[test]
fn an_archer_walks_in_and_stands_on_the_lookout() {
    let map = tower_map(false);
    let mut world = world(&map);
    let (t, a, a2, w) = (
        id(&world, "t"),
        id(&world, "a"),
        id(&world, "a2"),
        id(&world, "w"),
    );
    // Only accepted units, one at a time.
    command(
        &mut world,
        1,
        json!({"type": "garrison", "actors": [w], "target": t}),
    );
    run(&mut world, 1);
    assert_eq!(
        errors(&world),
        ["The watchtower needs one available Archer and an empty lookout"]
    );
    command(
        &mut world,
        1,
        json!({"type": "garrison", "actors": [a, a2], "target": t}),
    );
    run(&mut world, 1);
    // The first eligible archer takes it; the lookout is promised.
    assert!(matches!(
        entity(&world, "a").unit.as_ref().unwrap().order,
        Some(canopy_sim::orders::Order::Garrison { .. })
    ));
    assert!(entity(&world, "a2").unit.as_ref().unwrap().order.is_none());
    run_until(&mut world, 300, |w| lookout(w, "a").is_some());
    let archer = entity(&world, "a");
    let tower = entity(&world, "t");
    assert_eq!(lookout(&world, "a"), Some(EntityId(t)));
    assert_eq!(archer.position, tower.position);
    assert!(matches!(
        archer.unit.as_ref().unwrap().order,
        Some(canopy_sim::orders::Order::Hold)
    ));
    // It sees from 6 m up and ghosts through bodies.
    assert_eq!(canopy_sim::vision::Current.lift_cm(archer), 600);
    assert!(canopy_sim::spatial::ignores_units(world.state(), archer));
    let view = world.view(player(1));
    assert_eq!(
        view.entity(EntityId(a))
            .unwrap()
            .combat
            .as_ref()
            .unwrap()
            .garrison,
        Some(EntityId(t))
    );
    // A second archer cannot join.
    command(
        &mut world,
        1,
        json!({"type": "garrison", "actors": [a2], "target": t}),
    );
    run(&mut world, 1);
    assert_eq!(errors(&world).len(), 2);
}

#[test]
fn the_lookout_shoots_and_cannot_be_targeted() {
    let map = tower_map(true);
    let mut world = world(&map);
    let (t, a, e1) = (id(&world, "t"), id(&world, "a"), id(&world, "e1"));
    command(
        &mut world,
        1,
        json!({"type": "garrison", "actors": [a], "target": t}),
    );
    run_until(&mut world, 300, |w| lookout(w, "a").is_some());
    // An attack on the archer inside is refused.
    command(
        &mut world,
        2,
        json!({"type": "attack", "actors": [e1], "target": a}),
    );
    run(&mut world, 1);
    assert_eq!(errors(&world), ["Target is not visible and damageable"]);
    command(
        &mut world,
        2,
        json!({"type": "move", "actors": [e1], "destination": {"x": 108, "y": 101}}),
    );
    run_until(&mut world, 900, |w| !alive(w, "e1"));
    // The archer shot it from the tower without leaving; the swordsman hit the tower only.
    assert_eq!(lookout(&world, "a"), Some(EntityId(t)));
    assert_eq!(hp(&world, "a"), 70);
}

#[test]
fn orders_unload_and_a_lost_tower_put_the_archer_outside() {
    let map = tower_map(false);
    let mut world = world(&map);
    let (t, a) = (id(&world, "t"), id(&world, "a"));
    command(
        &mut world,
        1,
        json!({"type": "garrison", "actors": [a], "target": t}),
    );
    run_until(&mut world, 300, |w| lookout(w, "a").is_some());
    // Hold and attack keep it inside; anything else takes it out at the entrance.
    command(&mut world, 1, json!({"type": "hold", "actors": [a]}));
    run(&mut world, 1);
    assert!(lookout(&world, "a").is_some());
    command(&mut world, 1, json!({"type": "unload", "actor": t}));
    run(&mut world, 1);
    assert!(lookout(&world, "a").is_none());
    let outside = entity(&world, "a").position;
    assert_eq!((outside.x / 1000, outside.y / 1000), (102, 104));
    command(&mut world, 1, json!({"type": "unload", "actor": t}));
    run(&mut world, 1);
    assert_eq!(errors(&world), ["The watchtower is empty"]);
    // Back in, then the tower is destroyed: the archer is released.
    command(
        &mut world,
        1,
        json!({"type": "garrison", "actors": [a], "target": t}),
    );
    run_until(&mut world, 300, |w| lookout(w, "a").is_some());
    world
        .state_mut()
        .combat
        .scripted
        .push(canopy_sim::combat::ScriptedHit {
            target: EntityId(t),
            source: None,
            amount: 1,
        });
    world.state_mut().entities.get_mut(EntityId(t)).unwrap().hp = Some(1);
    run(&mut world, 1);
    assert!(find(&world, "t").is_none());
    assert!(lookout(&world, "a").is_none());
    assert!(entity(&world, "a").unit.as_ref().unwrap().release.is_none());
}
