//! Hero revival: the queue shares its building's training lane, pays a level-based bill,
//! and brings the fallen hero back beside the doorway. Fallen heroes are set up directly
//! (combat makes them in play).

mod economy_fixture;

use canopy_sim::{economy::Status, entity::EntityId, state::FactKind};
use economy_fixture::*;
use serde_json::{Value, json};

fn revive(building: u32, hero: u32) -> Value {
    json!({"type": "revive", "actor": building, "hero": hero})
}

/// A sanctuary, a fallen hero (kept at zero health) and a living one.
fn sanctuary_world() -> canopy_sim::World {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        place("sanctuary", SANCTUARY, 35.5, 19.5),
        place("house", HOUSE, 13.5, 33.5),
        place("fallen", HERO, 30.0, 40.0),
        place_for("enemy-hero", HERO, "player.2", 60.0, 60.0),
    ]));
    fund(&mut world, P1, AMBER, 200);
    let hero = id(&world, "fallen");
    fall(&mut world, hero);
    world
}

/// The hero falls as combat leaves it: kept, fallen.
fn fall(world: &mut canopy_sim::World, hero: u32) {
    let e = world.state_mut().entities.get_mut(EntityId(hero)).unwrap();
    canopy_sim::heroes::retain(e);
}

#[test]
fn a_fallen_hero_is_revived_beside_the_doorway() {
    let mut world = sanctuary_world();
    let (sanctuary, fallen) = (id(&world, "sanctuary"), id(&world, "fallen"));
    assert_eq!(try_command(&mut world, revive(sanctuary, fallen)), None);
    assert_eq!(wallet(&world, P1, AMBER), 170, "level one costs the base");
    let q = &get(&world, sanctuary).revival.as_ref().unwrap().queue;
    assert_eq!((q.len(), q[0].level, q[0].progress), (1, 1, 1));
    assert_eq!(
        get(&world, sanctuary).production.as_ref().unwrap().status,
        Status::RevivingHero
    );
    let colony = world.view(P1).colony.unwrap();
    assert_eq!(
        colony.supply.training, 3,
        "a started revival trains its hero's supply"
    );
    let back = run_until(&mut world, 100, |w| {
        get(w, fallen).hp.is_some_and(|hp| hp > 0)
    });
    assert_eq!(back + 1, 40, "forty ticks of work at level one");
    let hero = get(&world, fallen);
    assert_eq!(hero.hp, Some(300));
    assert_eq!(hero.ready_tick, world.state().tick);
    let (x, y) = hero.position.cell();
    assert!(
        (x - 36).abs() <= 2 && (y - 24).abs() <= 2,
        "beside the doorway at (36, 24): {x},{y}"
    );
    assert!(
        get(&world, sanctuary)
            .revival
            .as_ref()
            .unwrap()
            .queue
            .is_empty()
    );
    let consumed = world
        .state()
        .facts
        .iter()
        .find(|f| f.message == "message.hero-revival-completed")
        .unwrap();
    assert_eq!(
        (consumed.kind, consumed.amount),
        (FactKind::Consumed, Some(30))
    );
    assert!(
        world
            .state()
            .facts
            .iter()
            .any(|f| f.message == "unit.test.hero has returned")
    );
}

#[test]
fn revival_validation_and_refund() {
    let mut world = sanctuary_world();
    let (sanctuary, hall, fallen, enemy) = (
        id(&world, "sanctuary"),
        id(&world, "hall"),
        id(&world, "fallen"),
        id(&world, "enemy-hero"),
    );
    assert_eq!(
        try_command(&mut world, revive(hall, fallen)),
        Some("message.select-a-completed-revival-building".into())
    );
    assert_eq!(
        try_command(&mut world, revive(sanctuary, enemy)),
        Some("message.choose-one-of-your-fallen-heroes".into())
    );
    assert_eq!(try_command(&mut world, revive(sanctuary, fallen)), None);
    assert_eq!(
        try_command(&mut world, revive(sanctuary, fallen)),
        Some("message.hero-is-already-being-revived".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "cancelRevival", "actor": sanctuary, "hero": enemy})
        ),
        Some("message.hero-is-not-queued-here".into())
    );
    run(&mut world, 10);
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "cancelRevival", "actor": sanctuary, "hero": fallen})
        ),
        None
    );
    assert_eq!(wallet(&world, P1, AMBER), 200, "the whole bill comes back");
    assert!(get(&world, fallen).hp == Some(0));
    fund(&mut world, P1, AMBER, 10);
    assert_eq!(
        try_command(&mut world, revive(sanctuary, fallen)),
        Some("message.insufficient-resources".into())
    );
}

#[test]
fn revival_and_training_share_one_lane_in_queue_order() {
    let mut world = sanctuary_world();
    let (sanctuary, fallen) = (id(&world, "sanctuary"), id(&world, "fallen"));
    command(
        &mut world,
        json!({"type": "produce", "actor": sanctuary, "definition": SOLDIER}),
    );
    command(&mut world, revive(sanctuary, fallen));
    command(
        &mut world,
        json!({"type": "produce", "actor": sanctuary, "definition": SOLDIER}),
    );
    world.step();
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "produce", "actor": sanctuary, "definition": SOLDIER})
        ),
        Some("message.queue-is-full".into()),
        "three entries fill the shared lane"
    );
    // The first soldier trains while the revival waits.
    run(&mut world, 10);
    assert_eq!(
        get(&world, sanctuary).revival.as_ref().unwrap().queue[0].progress,
        0
    );
    run_until(&mut world, 60, |w| all_of(w, P1, SOLDIER).len() == 1);
    // Then the revival, before the second soldier.
    run_until(&mut world, 100, |w| {
        get(w, fallen).hp.is_some_and(|hp| hp > 0)
    });
    assert_eq!(all_of(&world, P1, SOLDIER).len(), 1);
    run_until(&mut world, 60, |w| all_of(w, P1, SOLDIER).len() == 2);
}

#[test]
fn the_roster_keeps_a_fallen_hero_and_refuses_a_second_copy() {
    let mut world = sanctuary_world();
    let sanctuary = id(&world, "sanctuary");
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "produce", "actor": sanctuary, "definition": HERO})
        ),
        Some("message.this-hero-already-belongs-to-your".into())
    );
    let mut empty = self::world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        place("sanctuary", SANCTUARY, 35.5, 19.5),
        place("other", SANCTUARY, 35.5, 35.5),
    ]));
    fund(&mut empty, P1, AMBER, 200);
    let (sanctuary, other) = (id(&empty, "sanctuary"), id(&empty, "other"));
    assert_eq!(
        try_command(
            &mut empty,
            json!({"type": "produce", "actor": sanctuary, "definition": HERO})
        ),
        None
    );
    assert_eq!(
        try_command(
            &mut empty,
            json!({"type": "produce", "actor": other, "definition": HERO})
        ),
        Some("message.this-hero-already-belongs-to-your".into()),
        "a queued hero holds its place"
    );
    let _ = EntityId(other);
}

#[test]
fn a_fallen_hero_stays_in_its_owners_view_to_be_revived() {
    let mut world = sanctuary_world();
    world.step();
    let fallen = EntityId(id(&world, "fallen"));
    let mine = world.view(P1);
    assert_eq!(mine.battle.fallen, [fallen]);
    let hero = mine
        .entities
        .iter()
        .find(|e| e.id() == fallen)
        .expect("the card revives it from the view");
    assert_eq!(hero.combat.as_ref().map(|c| c.stats.level), Some(1));
    assert!(
        !world.view(P2).entities.iter().any(|e| e.id() == fallen),
        "rivals do not see it"
    );
}
