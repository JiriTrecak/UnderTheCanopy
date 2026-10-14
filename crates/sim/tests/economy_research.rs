//! Research and building upgrades: payment, progress, completion and effects, refunds on
//! cancel, and their validation.

mod economy_fixture;

use canopy_content::Id;
use canopy_sim::{economy::Status, entity::EntityId, state::FactKind};
use economy_fixture::*;
use serde_json::json;

fn research(building: u32, id: &str) -> serde_json::Value {
    json!({"type": "research", "actor": building, "research": id})
}

#[test]
fn research_is_paid_progresses_and_completes_for_the_colony() {
    let mut world = world(&sandbox(vec![
        place("forge", FORGE, 25.5, 25.5),
        place("other", FORGE, 33.5, 25.5),
        place("house", HOUSE, 25.5, 33.5),
    ]));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 20);
    let (forge, other, house) = (id(&world, "forge"), id(&world, "other"), id(&world, "house"));
    assert_eq!(try_command(&mut world, research(house, TOOLS)), Some("Select a completed research building".into()));
    assert_eq!(try_command(&mut world, research(forge, "research.test.unknown")), Some("Select a completed research building".into()));
    assert_eq!(try_command(&mut world, research(forge, VIGOUR)), Some("Requires building.test.keep".into()));
    assert_eq!(try_command(&mut world, research(forge, TOOLS)), None);
    assert_eq!((wallet(&world, P1, AMBER), wallet(&world, P1, WOOD)), (80, 15));
    assert_eq!(try_command(&mut world, research(other, TOOLS)), Some("Research already queued".into()));
    let done = run_until(&mut world, 60, |w| !w.state().economy.researched(P1).is_empty());
    assert_eq!(2 + done, 30, "thirty ticks: two passed with the commands above");
    assert_eq!(world.state().economy.researched(P1), [Id::new(TOOLS).unwrap()]);
    assert!(get(&world, forge).research.as_ref().unwrap().queue.is_empty());
    let facts: Vec<_> = world.state().facts.iter().filter(|f| f.kind == FactKind::Consumed).collect();
    assert_eq!(facts.len(), 2);
    assert!(world.state().facts.iter().any(|f| f.message == "Tools researched"));
    assert_eq!(try_command(&mut world, research(forge, TOOLS)), Some("Already researched".into()));
}

#[test]
fn cancelled_research_is_refunded_and_full_queues_refuse() {
    let mut world = world(&sandbox(vec![
        place("forge", FORGE, 25.5, 25.5),
        place("keep", KEEP, 35.5, 19.5),
    ]));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 20);
    let forge = id(&world, "forge");
    command(&mut world, research(forge, TOOLS));
    command(&mut world, research(forge, VIGOUR));
    world.step();
    assert_eq!(get(&world, forge).research.as_ref().unwrap().queue.len(), 2);
    assert_eq!(wallet(&world, P1, AMBER), 70);
    assert_eq!(
        try_command(&mut world, json!({"type": "cancelResearch", "actor": forge, "research": "research.test.other"})),
        Some("Research is not queued here".into())
    );
    assert_eq!(
        try_command(&mut world, json!({"type": "cancelResearch", "actor": forge, "research": VIGOUR})),
        None
    );
    assert_eq!(wallet(&world, P1, AMBER), 80);
    assert!(get(&world, forge).inventory.is_empty());
}

#[test]
fn health_research_raises_living_and_future_units() {
    let mut world = world(&sandbox(vec![
        place("forge", FORGE, 25.5, 25.5),
        place("keep", KEEP, 35.5, 19.5),
        with_state(place("hurt", WORKER, 30.0, 34.0), json!({"health": 40})),
    ]));
    fund(&mut world, P1, AMBER, 100);
    let (forge, keep, hurt) = (id(&world, "forge"), id(&world, "keep"), id(&world, "hurt"));
    command(&mut world, research(forge, VIGOUR));
    run_until(&mut world, 60, |w| !w.state().economy.researched(P1).is_empty());
    assert_eq!(get(&world, hurt).hp, Some(90), "wounds are kept");
    command(&mut world, json!({"type": "produce", "actor": keep, "definition": WORKER}));
    run_until(&mut world, 60, |w| all_of(w, P1, WORKER).len() == 2);
    let fresh = *all_of(&world, P1, WORKER).last().unwrap();
    assert_eq!(get(&world, fresh).hp, Some(150));
}

#[test]
fn an_upgrade_transforms_the_building_in_place() {
    let mut world = world(&sandbox(vec![
        with_state(place("hall", HALL, 19.5, 19.5), json!({"health": 350})),
        place("house", HOUSE, 33.5, 13.5),
    ]));
    fund(&mut world, P1, AMBER, 100);
    let (hall, house) = (id(&world, "hall"), id(&world, "house"));
    assert_eq!(
        try_command(&mut world, json!({"type": "upgrade", "actor": house})),
        Some("Select a completed upgradeable building".into())
    );
    assert_eq!(
        try_command(&mut world, json!({"type": "cancelUpgrade", "actor": hall})),
        Some("No upgrade in progress".into())
    );
    assert_eq!(try_command(&mut world, json!({"type": "upgrade", "actor": hall})), None);
    assert_eq!(wallet(&world, P1, AMBER), 70);
    assert_eq!(
        try_command(&mut world, json!({"type": "upgrade", "actor": hall})),
        Some("Upgrade already in progress".into())
    );
    // Production waits while the hall is upgrading.
    command(&mut world, json!({"type": "produce", "actor": hall, "definition": WORKER}));
    world.step();
    assert_eq!(get(&world, hall).production.as_ref().unwrap().status, Status::Upgrading);
    let done = run_until(&mut world, 60, |w| w.catalog().id(get(w, hall).definition).as_str() == KEEP);
    assert!(done > 0);
    assert_eq!(get(&world, hall).hp, Some(450), "wounds are kept: +100 capacity");
    assert!(get(&world, hall).upgrade.is_none());
    assert!(world.state().facts.iter().any(|f| f.message == "building.test.keep completed"));
    run_until(&mut world, 40, |w| !all_of(w, P1, WORKER).is_empty());
}

#[test]
fn a_cancelled_upgrade_is_refunded() {
    let mut world = world(&sandbox(vec![place("hall", HALL, 19.5, 19.5)]));
    fund(&mut world, P1, AMBER, 30);
    let hall = id(&world, "hall");
    assert_eq!(try_command(&mut world, json!({"type": "upgrade", "actor": hall})), None);
    assert_eq!(wallet(&world, P1, AMBER), 0);
    assert_eq!(
        try_command(&mut world, json!({"type": "upgrade", "actor": hall})),
        Some("Upgrade already in progress".into())
    );
    run(&mut world, 10);
    assert_eq!(try_command(&mut world, json!({"type": "cancelUpgrade", "actor": hall})), None);
    assert_eq!(wallet(&world, P1, AMBER), 30);
    assert_eq!(
        try_command(&mut world, json!({"type": "upgrade", "actor": EntityId(hall).0})),
        None
    );
    fund(&mut world, P1, AMBER, 0);
    let mut poor = self::world(&sandbox(vec![place("hall", HALL, 19.5, 19.5)]));
    let hall = id(&poor, "hall");
    assert_eq!(
        try_command(&mut poor, json!({"type": "upgrade", "actor": hall})),
        Some("Insufficient resources".into())
    );
}
