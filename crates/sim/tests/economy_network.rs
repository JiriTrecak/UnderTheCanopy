//! The power network (D66): placement needs it, nodes chain from sources, losing a node
//! pauses work until power returns, construction goes on regardless, research widens it,
//! and the derived network survives saves.

mod economy_fixture;

use canopy_sim::{World, economy::Status, entity::EntityId};
use economy_fixture::*;
use serde_json::{Value, json};

fn build(worker: u32, definition: &str, x: f64, y: f64) -> Value {
    json!({"type": "build", "actors": [worker], "definition": definition, "position": {"x": x, "y": y}})
}

/// Destroys an entity the way a lethal blow does.
fn destroy(world: &mut World, target: u32) {
    let target = EntityId(target);
    world
        .state_mut()
        .combat
        .scripted
        .push(canopy_sim::combat::ScriptedHit {
            target,
            source: None,
            amount: 1,
        });
    world.state_mut().entities.get_mut(target).unwrap().hp = Some(1);
    run(world, 2);
    assert!(world.state().entities.get(target).is_none(), "destroyed");
}

fn unpowered(world: &mut World, id: u32) -> bool {
    world
        .verify_economy()
        .expect("the network matches a rebuild");
    world.economy().network().unpowered(EntityId(id))
}

fn covers(world: &World, x: f64, y: f64) -> bool {
    world
        .economy()
        .network()
        .covers(P1, (x * 1000.0) as i64, (y * 1000.0) as i64)
}

/// A nexus (radius 16) at (19.5, 19.5) and a worker.
fn base() -> Vec<Value> {
    vec![
        place("nexus", NEXUS, 19.5, 19.5),
        place("worker", WORKER, 26.0, 28.0),
    ]
}

#[test]
fn buildings_that_need_power_are_placed_only_inside_the_network() {
    let mut world = world(&sandbox(base()));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 50);
    let worker = id(&world, "worker");
    // 22 cells from the nexus: outside its 16.
    assert_eq!(
        try_command(&mut world, build(worker, WORKSHOP, 41.5, 21.5)),
        Some(format!(
            "Build inside your network: extend it with a {PYLON}"
        ))
    );
    assert_eq!(wallet(&world, P1, AMBER), 100, "nothing is paid");
    // About 10 cells away: inside.
    assert_eq!(
        try_command(&mut world, build(worker, WORKSHOP, 29.5, 21.5)),
        None
    );
    assert_eq!(all_of(&world, P1, WORKSHOP).len(), 1);
    // Buildings that need no power go anywhere.
    assert_eq!(
        try_command(&mut world, build(worker, HOUSE, 61.5, 61.5)),
        None
    );
}

#[test]
fn a_source_placed_on_a_deposit_goes_wherever_the_deposit_is_and_powers_around_it() {
    let mut entities = base();
    entities.extend([
        // Far outside the nexus's disc.
        resource("spring", SPRING, 81.5, 81.5),
        place("scout", WORKER, 74.0, 74.0),
    ]);
    let mut world = world(&sandbox(entities));
    fund(&mut world, P1, AMBER, 100);
    let scout = id(&world, "scout");
    assert!(!covers(&world, 81.5, 81.5));
    assert_eq!(
        try_command(&mut world, build(scout, WELL, 81.5, 81.5)),
        None,
        "a well needs no power: it goes on its deposit"
    );
    let well = all_of(&world, P1, WELL)[0];
    assert!(!covers(&world, 81.5, 89.5), "a site powers nothing yet");
    run_until(&mut world, 600, |w| get(w, well).construction.is_none());
    run(&mut world, 1);
    assert!(covers(&world, 81.5, 89.5), "the finished well does");
    assert_eq!(
        try_command(&mut world, build(scout, WORKSHOP, 81.5, 89.5)),
        None
    );
    destroy(&mut world, well);
    assert!(!covers(&world, 81.5, 89.5), "a fallen well powers nothing");
}

#[test]
fn a_node_extends_the_network_only_when_chained_to_a_source() {
    let mut entities = base();
    entities.extend([
        // 14 cells from the nexus: chained; it reaches 12 further.
        place("near-pylon", PYLON, 33.5, 21.5),
        place("near-shop", WORKSHOP, 45.5, 21.5),
        // A pylon on its own powers nothing, not even the shop beside it.
        place("far-pylon", PYLON, 101.5, 101.5),
        place("far-shop", WORKSHOP, 105.5, 101.5),
    ]);
    let mut world = world(&sandbox(entities));
    run(&mut world, 1);
    let (near, far) = (id(&world, "near-shop"), id(&world, "far-shop"));
    assert!(!unpowered(&mut world, near));
    assert!(unpowered(&mut world, far));
    assert!(covers(&world, 45.5, 21.5));
    assert!(!covers(&world, 101.5, 101.5));
    // A chain: a third pylon in the near one's disc reaches further still.
    fund(&mut world, P1, AMBER, 100);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, PYLON, 41.5, 29.5)),
        None
    );
    let site = all_of(&world, P1, PYLON)[2];
    assert!(!covers(&world, 49.5, 37.5), "a site extends nothing yet");
    run_until(&mut world, 600, |w| get(w, site).construction.is_none());
    run(&mut world, 1);
    assert!(covers(&world, 49.5, 37.5), "the finished pylon does");
}

#[test]
fn losing_a_node_pauses_work_until_power_returns() {
    let mut entities = base();
    entities.extend([
        place("pylon", PYLON, 33.5, 21.5),
        place("shop", WORKSHOP, 45.5, 21.5),
        place("house", HOUSE, 61.5, 61.5),
    ]);
    let mut world = world(&sandbox(entities));
    fund(&mut world, P1, AMBER, 200);
    let (shop, pylon) = (id(&world, "shop"), id(&world, "pylon"));
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "produce", "actor": shop, "definition": SOLDIER})
        ),
        None
    );
    run_until(&mut world, 100, |w| {
        get(w, shop)
            .production
            .as_ref()
            .and_then(|p| p.active.as_ref())
            .is_some_and(|a| a.progress >= 5)
    });
    let busy = |world: &World| {
        let view = world.view(P1);
        let work = view
            .entity(EntityId(shop))
            .and_then(|e| e.work.clone())
            .unwrap_or_default();
        (work.busy, work.unpowered)
    };
    assert_eq!(busy(&world), (true, false));
    destroy(&mut world, pylon);
    assert!(unpowered(&mut world, shop));
    let progress = |world: &World| {
        get(world, shop)
            .production
            .as_ref()
            .and_then(|p| p.active.as_ref())
            .map(|a| a.progress)
    };
    let held = progress(&world);
    run(&mut world, 60);
    assert_eq!(progress(&world), held, "training holds while unpowered");
    assert_eq!(
        get(&world, shop).production.as_ref().unwrap().status,
        Status::Unpowered
    );
    assert_eq!(busy(&world), (false, true), "the dark building shows it");
    // A new pylon where the old one stood brings the power back; training resumes and
    // the soldier steps out.
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, PYLON, 33.5, 21.5)),
        None
    );
    let site = *all_of(&world, P1, PYLON).last().unwrap();
    run_until(&mut world, 600, |w| get(w, site).construction.is_none());
    run(&mut world, 1);
    assert!(!unpowered(&mut world, shop));
    run_until(&mut world, 200, |w| !all_of(w, P1, SOLDIER).is_empty());
}

#[test]
fn construction_goes_on_outside_the_network_and_the_building_waits_for_power() {
    let mut entities = base();
    entities.push(place("pylon", PYLON, 33.5, 21.5));
    let mut world = world(&sandbox(entities));
    fund(&mut world, P1, AMBER, 100);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, WORKSHOP, 41.5, 25.5)),
        None
    );
    let site = all_of(&world, P1, WORKSHOP)[0];
    let pylon = id(&world, "pylon");
    destroy(&mut world, pylon);
    assert!(!unpowered(&mut world, site), "a site needs no power");
    run_until(&mut world, 600, |w| get(w, site).construction.is_none());
    run(&mut world, 1);
    assert!(unpowered(&mut world, site), "the finished workshop is dark");
}

#[test]
fn research_widens_the_network() {
    let mut entities = base();
    entities.push(place("shop", WORKSHOP, 29.5, 21.5));
    let mut world = world(&sandbox(entities));
    fund(&mut world, P1, AMBER, 100);
    let shop = id(&world, "shop");
    let edge = 19.5 + f64::from(NEXUS_RADIUS) + f64::from(AMPLIFY_CELLS) / 2.0;
    assert!(!covers(&world, edge, 19.5));
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "research", "actor": shop, "research": AMPLIFY})
        ),
        None
    );
    run_until(&mut world, 200, |w| {
        w.state()
            .economy
            .researched(P1)
            .iter()
            .any(|r| r == AMPLIFY)
    });
    run(&mut world, 1);
    assert!(covers(&world, edge, 19.5));
    world.verify_economy().unwrap();
}

#[test]
fn a_restored_save_has_the_same_network_and_plays_on_identically() {
    let mut entities = base();
    entities.extend([
        place("pylon", PYLON, 33.5, 21.5),
        place("shop", WORKSHOP, 45.5, 21.5),
        place("house", HOUSE, 61.5, 61.5),
    ]);
    let map = sandbox(entities);
    let mut world = world(&map);
    fund(&mut world, P1, AMBER, 200);
    let (shop, pylon) = (id(&world, "shop"), id(&world, "pylon"));
    command(
        &mut world,
        json!({"type": "produce", "actor": shop, "definition": SOLDIER}),
    );
    run(&mut world, 10);
    destroy(&mut world, pylon);
    let mut restored = World::restore(content(), &map, world.save()).unwrap();
    assert!(unpowered(&mut restored, shop));
    for _ in 0..80 {
        world.step();
        restored.step();
        assert_eq!(world.fingerprint(), restored.fingerprint());
    }
    assert_eq!(world.save(), restored.save());
}
