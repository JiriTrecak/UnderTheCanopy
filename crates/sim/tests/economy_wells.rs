//! Buildings placed on a resource (D62, a Substance Well on its deposit): the well
//! snaps onto the deposit, only its owner's workers gather there and only once it is
//! finished, loads go into the well, the deposit never runs out, and it is left intact
//! when the well falls.

mod economy_fixture;

use canopy_sim::{World, economy::JobKind, entity::EntityId};
use economy_fixture::*;
use serde_json::{Value, json};

fn build(worker: u32, definition: &str, x: f64, y: f64) -> Value {
    json!({"type": "build", "actors": [worker], "definition": definition, "position": {"x": x, "y": y}})
}

fn gather(worker: u32, target: u32) -> Value {
    json!({"type": "gather", "actors": [worker], "target": target})
}

const AT: (f64, f64) = (41.5, 41.5);

fn base() -> Vec<Value> {
    vec![
        place("hall", HALL, 19.5, 19.5),
        place("worker", WORKER, 34.0, 34.0),
        place("helper", WORKER, 35.0, 34.0),
        resource("spring", SPRING, AT.0, AT.1),
    ]
}

/// Builds a well on the spring with `worker` and waits until it is finished.
fn finished_well(world: &mut World, worker: u32) -> u32 {
    fund(world, P1, AMBER, 100);
    assert_eq!(try_command(world, build(worker, WELL, AT.0, AT.1)), None);
    let well = all_of(world, P1, WELL)[0];
    run_until(world, 600, |w| get(w, well).construction.is_none());
    well
}

#[test]
fn a_well_stands_exactly_on_a_free_deposit() {
    let mut world = world(&sandbox(base()));
    fund(&mut world, P1, AMBER, 100);
    let (worker, helper) = (id(&world, "worker"), id(&world, "helper"));
    assert_eq!(
        try_command(&mut world, build(worker, WELL, 29.5, 29.5)),
        Some(canopy_sim::msg!(
            "message.build-on",
            name = canopy_content::text::name(SPRING)
        ))
    );
    assert_eq!(
        try_command(&mut world, build(worker, WELL, AT.0 + 4.0, AT.1)),
        Some(canopy_sim::msg!(
            "message.build-on",
            name = canopy_content::text::name(SPRING)
        )),
        "next to it is not on it"
    );
    assert_eq!(
        try_command(&mut world, build(worker, WELL, AT.0, AT.1)),
        None
    );
    let site = all_of(&world, P1, WELL)[0];
    let spring = id(&world, "spring");
    assert_eq!(
        get(&world, site).position,
        get(&world, spring).position,
        "the well covers the deposit"
    );
    assert_eq!(
        try_command(&mut world, build(helper, WELL, AT.0, AT.1)),
        Some(canopy_sim::msg!(
            "message.already-built-on",
            name = canopy_content::text::name(SPRING)
        ))
    );
    world.verify_economy().unwrap();
}

#[test]
fn only_the_owner_gathers_through_a_finished_well_and_it_never_runs_out() {
    let mut entities = base();
    entities.push(place_for("theirs", WORKER, "player.2", 50.0, 50.0));
    entities.push(place_for("their-hall", HALL, "player.2", 59.5, 59.5));
    let mut world = world(&sandbox(entities));
    let (worker, helper, theirs) = (
        id(&world, "worker"),
        id(&world, "helper"),
        id(&world, "theirs"),
    );
    let spring = id(&world, "spring");
    assert!(
        try_command(&mut world, gather(helper, spring)).is_some(),
        "no well yet"
    );
    let well = finished_well(&mut world, worker);
    // Sent to the well, a worker gathers the deposit under it.
    assert_eq!(try_command(&mut world, gather(helper, well)), None);
    run_until(&mut world, 400, |w| {
        job(w, helper).is_some_and(|j| j.kind == JobKind::Harvest)
    });
    let j = job(&world, helper).expect("harvest");
    assert_eq!(
        (j.source.map(|s| s.0), j.target.0),
        (Some(spring), well),
        "gathers the deposit and delivers into the well"
    );
    // Another player's workers may not use it.
    command_for(&mut world, 1, gather(theirs, spring));
    world.step();
    assert!(
        world
            .state()
            .facts
            .iter()
            .any(|f| f.owner == P2 && f.kind == canopy_sim::state::FactKind::Error),
        "refused for the other player"
    );
    run_until(&mut world, 3000, |w| wallet(w, P1, ESSENCE) >= 100);
    assert_eq!(
        get(&world, spring).resource.as_ref().unwrap().amount,
        50,
        "an endless deposit keeps its amount while it pays out twice its yield"
    );
    assert!(job(&world, theirs).is_none());
    world.verify_economy().unwrap();
}

#[test]
fn a_destroyed_well_leaves_its_deposit_intact_to_build_on_again() {
    let mut entities = base();
    entities.push(place_for("raider", SOLDIER, "player.2", 47.0, 47.0));
    let mut world = world(&sandbox(entities));
    let (worker, helper, raider) = (
        id(&world, "worker"),
        id(&world, "helper"),
        id(&world, "raider"),
    );
    let spring = id(&world, "spring");
    // The raider holds still until the well is up.
    command_for(&mut world, 1, json!({"type": "hold", "actors": [raider]}));
    let well = finished_well(&mut world, worker);
    assert_eq!(try_command(&mut world, gather(helper, well)), None);
    run_until(&mut world, 400, |w| {
        job(w, helper).is_some_and(|j| j.kind == JobKind::Harvest)
    });
    command_for(
        &mut world,
        1,
        json!({"type": "attack", "actors": [raider], "target": well}),
    );
    run_until(&mut world, 400, |w| {
        w.state()
            .entities
            .get(EntityId(well))
            .is_none_or(|e| e.hp == Some(0))
    });
    run(&mut world, 5);
    let deposit = get(&world, spring);
    assert_eq!(deposit.hp, Some(1000), "the deposit stands untouched");
    assert_eq!(deposit.resource.as_ref().unwrap().amount, 50);
    assert!(
        job(&world, helper).is_none_or(|j| j.kind != JobKind::Harvest),
        "gathering stops with the well"
    );
    assert!(
        try_command(&mut world, gather(helper, spring)).is_some(),
        "nothing to gather through"
    );
    // The raider leaves; the deposit takes a new well.
    command_for(&mut world, 1, json!({"type": "stop", "actors": [raider]}));
    fund(&mut world, P1, AMBER, 100);
    run(&mut world, 2);
    let builder = all_of(&world, P1, WORKER)[0];
    assert_eq!(
        try_command(&mut world, build(builder, WELL, AT.0, AT.1)),
        None
    );
    world.verify_economy().unwrap();
}
