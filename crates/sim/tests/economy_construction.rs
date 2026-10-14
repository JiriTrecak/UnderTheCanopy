//! Construction: placement rules with the old messages, a site from bill to completion,
//! cancelling with a refund, and repair.

mod economy_fixture;

use canopy_sim::{orders::Order, state::FactKind};
use economy_fixture::*;
use serde_json::{Value, json};

fn build(worker: u32, definition: &str, x: f64, y: f64) -> Value {
    json!({"type": "build", "actors": [worker], "definition": definition, "position": {"x": x, "y": y}})
}

fn base() -> Vec<Value> {
    vec![
        place("hall", HALL, 19.5, 19.5),
        place("worker", WORKER, 26.0, 26.0),
        resource("mine", MINE, 41.5, 13.5),
        resource("tree", TREE, 60.0, 60.0),
    ]
}

#[test]
fn a_site_is_paid_up_front_built_with_growing_health_and_completed() {
    let mut world = world(&sandbox(base()));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 50);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, HOUSE, 29.5, 29.5)),
        None
    );
    let site = *all_of(&world, P1, HOUSE).first().expect("site");
    let e = get(&world, site);
    assert_eq!(
        (wallet(&world, P1, AMBER), wallet(&world, P1, WOOD)),
        (80, 40)
    );
    assert_eq!(e.inventory.get(AMBER).copied(), Some(20));
    assert_eq!(e.inventory.get(WOOD).copied(), Some(10));
    assert_eq!(e.hp, Some(20), "a tenth of its health");
    assert_eq!(e.construction.as_ref().map(|c| c.progress), Some(0));
    assert_eq!(
        get(&world, worker).unit.as_ref().unwrap().order,
        Some(Order::Construct {
            target: canopy_sim::entity::EntityId(site)
        })
    );
    run_until(&mut world, 400, |w| get(w, site).construction.is_none());
    let e = get(&world, site);
    assert_eq!(e.hp, Some(200));
    assert!(e.inventory.is_empty(), "the bill is consumed");
    let consumed = &world.state().accounting.consumed;
    assert_eq!(
        (consumed.get(AMBER).copied(), consumed.get(WOOD).copied()),
        (Some(20), Some(10))
    );
    assert_eq!(e.ready_tick, world.state().tick, "opens on the next tick");
    assert_eq!(get(&world, worker).unit.as_ref().unwrap().order, None);
    assert!(world.state().economy.jobs.is_empty());
    world.verify_economy().unwrap();
}

#[test]
fn health_grows_with_each_tick_of_work() {
    let mut world = world(&sandbox(base()));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 50);
    let worker = id(&world, "worker");
    command(&mut world, build(worker, HOUSE, 29.5, 29.5));
    world.step();
    let site = all_of(&world, P1, HOUSE)[0];
    let mut track = Vec::new();
    for _ in 0..300 {
        world.step();
        let e = get(&world, site);
        match &e.construction {
            Some(c) => track.push((c.progress, e.hp.unwrap())),
            None => break,
        }
    }
    let working: Vec<_> = track.iter().filter(|(p, _)| *p > 0).collect();
    assert_eq!(working.len(), 39, "forty ticks of work, the last completes");
    for &&(progress, hp) in &working {
        assert_eq!(hp, 20 + 180 * progress / 40);
    }
}

#[test]
fn cancelling_a_site_refunds_half_and_loses_the_rest() {
    let mut world = world(&sandbox(base()));
    fund(&mut world, P1, AMBER, 100);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, BARRACKS, 27.5, 35.5)),
        None
    );
    let site = all_of(&world, P1, BARRACKS)[0];
    assert_eq!(wallet(&world, P1, AMBER), 70);
    run(&mut world, 20);
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "cancel", "actor": site, "queue": 5})
        ),
        Some("Project has no training queue".into())
    );
    assert_eq!(
        try_command(&mut world, json!({"type": "cancel", "actor": site})),
        None
    );
    assert!(all_of(&world, P1, BARRACKS).is_empty());
    assert_eq!(wallet(&world, P1, AMBER), 85);
    let lost = world
        .state()
        .facts
        .iter()
        .find(|f| f.message == "Unrefunded project resources")
        .expect("loss recorded");
    assert_eq!((lost.kind, lost.amount), (FactKind::Lost, Some(15)));
    assert_eq!(world.state().accounting.lost.get(AMBER).copied(), Some(15));
    assert!(
        world.state().economy.jobs.is_empty(),
        "the builder's job ends"
    );
    let free = world
        .spatial()
        .node(canopy_sim::spatial::Spot::new(27, 35, None))
        .unwrap();
    assert!(
        world.spatial().walkable(free),
        "its footprint opens at once"
    );
    world.verify_economy().unwrap();
}

#[test]
fn placement_rules_reject_with_the_old_messages() {
    let mut entities = base();
    entities.push(place("blocker", HOUSE, 29.5, 37.5));
    entities.push(place("soldier", SOLDIER, 50.0, 50.0));
    let slope = |x: i32, _: i32| {
        if (60..100).contains(&x) {
            0.3 * (x - 60) as f32
        } else {
            0.0
        }
    };
    let mut world = world(&sandbox_on(entities, terrain(slope, |_, _| -5.0)));
    fund(&mut world, P1, AMBER, 1000);
    fund(&mut world, P1, WOOD, 1000);
    let (worker, soldier) = (id(&world, "worker"), id(&world, "soldier"));
    let cases: Vec<(Value, &str)> = vec![
        // Workers only build what they know, so the command never reaches the unknown
        // definition check; placement queries (the AI) do.
        (
            build(worker, SOLDIER, 29.5, 29.5),
            "Select an eligible worker",
        ),
        (
            build(worker, HOUSE, 30.0, 29.5),
            "Align the building footprint to the building grid",
        ),
        (
            build(soldier, HOUSE, 29.5, 29.5),
            "Select an eligible worker",
        ),
        (
            build(worker, HALL, 255.5, 255.5),
            "Explore this location first",
        ),
        (build(worker, HOUSE, 253.5, 253.5), "Entrance blocked"),
        (
            build(worker, FORESTER, 29.5, 29.5),
            "Build within 8 cells of resource.test.tree",
        ),
        (build(worker, HOUSE, 25.5, 25.5), "Placement blocked"),
        (build(worker, HOUSE, 21.5, 21.5), "Placement blocked"),
        (
            build(worker, HOUSE, 37.5, 13.5),
            "Leave access around the resource deposit",
        ),
        (build(worker, BARRACKS, 75.5, 19.5), "Choose flatter ground"),
        (build(worker, HOUSE, 29.5, 33.5), "Entrance blocked"),
        (
            json!({"type": "build", "actors": [worker], "definition": HOUSE,
                "position": {"x": 29.5, "y": 29.5, "surface": "deck"}}),
            "Destination is not on a declared walk surface",
        ),
    ];
    for (action, message) in cases {
        assert_eq!(
            try_command(&mut world, action.clone()),
            Some(message.into()),
            "{action}"
        );
    }
    let point = canopy_sim::action::FoundationPoint {
        x: canopy_sim::action::HalfCells(59),
        y: canopy_sim::action::HalfCells(59),
        surface: None,
    };
    assert_eq!(
        world.can_build(
            P1,
            SOLDIER,
            &point,
            Some(canopy_sim::entity::EntityId(worker)),
            0
        ),
        Some("Unknown building".into())
    );
    assert_eq!(
        world.can_build(
            P1,
            HOUSE,
            &point,
            Some(canopy_sim::entity::EntityId(worker)),
            0
        ),
        None
    );
    fund(&mut world, P1, AMBER, 5);
    assert_eq!(
        try_command(&mut world, build(worker, HOUSE, 29.5, 29.5)),
        Some("Insufficient unreserved materials".into())
    );
    assert!(all_of(&world, P1, HOUSE).len() == 1, "only the blocker");
}

#[test]
fn prerequisites_count_upgraded_buildings() {
    let mut world = world(&sandbox(vec![place("worker", WORKER, 26.0, 26.0)]));
    fund(&mut world, P1, AMBER, 1000);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, BARRACKS, 35.5, 35.5)),
        Some("Requires building.test.hall".into())
    );
    let mut world = self::world(&sandbox(vec![
        place("keep", KEEP, 19.5, 19.5),
        place("worker", WORKER, 26.0, 26.0),
    ]));
    fund(&mut world, P1, AMBER, 1000);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, BARRACKS, 35.5, 35.5)),
        None,
        "a keep is an upgraded hall"
    );
}

#[test]
fn the_building_limit_counts_sites_too() {
    let mut entities = vec![place("worker", WORKER, 26.0, 26.0)];
    for i in 0..7 {
        entities.push(place(
            &format!("house-{i}"),
            HOUSE,
            5.5 + 8.0 * f64::from(i),
            61.5,
        ));
    }
    let mut world = world(&sandbox(entities));
    fund(&mut world, P1, AMBER, 1000);
    fund(&mut world, P1, WOOD, 1000);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, HOUSE, 29.5, 29.5)),
        None
    );
    assert_eq!(
        try_command(&mut world, build(worker, HOUSE, 37.5, 29.5)),
        Some("Building limit reached".into())
    );
}

#[test]
fn a_full_order_queue_refuses_an_appended_build() {
    let mut world = world(&sandbox(base()));
    fund(&mut world, P1, AMBER, 1000);
    fund(&mut world, P1, WOOD, 1000);
    let worker = id(&world, "worker");
    for i in 0..17 {
        command(
            &mut world,
            json!({"type": "move", "actors": [worker],
            "destination": {"x": 10 + i, "y": 40}, "append": true}),
        );
    }
    world.step();
    let mut action = build(worker, HOUSE, 29.5, 29.5);
    action["append"] = json!(true);
    assert_eq!(
        try_command(&mut world, action),
        Some("Order queue is full".into())
    );
}

#[test]
fn construct_orders_join_sites_and_finish_them() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        place("helper", WORKER, 30.0, 40.0),
        place("soldier", SOLDIER, 34.0, 40.0),
        place_for("theirs", HOUSE, "player.2", 61.5, 61.5),
    ]));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 100);
    // A site whose builder was sent elsewhere is finished by another worker.
    let helper = id(&world, "helper");
    command(&mut world, build(helper, HOUSE, 29.5, 29.5));
    world.step();
    let site = all_of(&world, P1, HOUSE)[0];
    command(
        &mut world,
        json!({"type": "move", "actors": [helper], "destination": {"x": 30, "y": 44}}),
    );
    run(&mut world, 60);
    assert_eq!(get(&world, site).construction.as_ref().unwrap().progress, 0);
    let (soldier, theirs) = (id(&world, "soldier"), id(&world, "theirs"));
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "construct", "actors": [helper], "target": theirs})
        ),
        Some("Select an owned unfinished building".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "construct", "actors": [soldier], "target": site})
        ),
        Some("Select an eligible worker".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "construct", "actors": [helper, soldier], "target": site})
        ),
        None
    );
    run_until(&mut world, 400, |w| get(w, site).construction.is_none());
}

#[test]
fn damaged_buildings_are_repaired_a_point_at_a_time() {
    let mut world = world(&sandbox(base()));
    let (hall, worker) = (id(&world, "hall"), id(&world, "worker"));
    world
        .state_mut()
        .entities
        .get_mut(canopy_sim::entity::EntityId(hall))
        .unwrap()
        .hp = Some(390);
    run_until(&mut world, 200, |w| {
        job(w, worker).is_some_and(|j| j.kind == canopy_sim::economy::JobKind::Repair)
    });
    let started = run_until(&mut world, 200, |w| get(w, hall).hp == Some(391));
    let next = run_until(&mut world, 10, |w| get(w, hall).hp == Some(392));
    assert_eq!(next, 4, "a point every repairTicks");
    let _ = started;
    run_until(&mut world, 100, |w| get(w, hall).hp == Some(400));
    world.step();
    assert!(job(&world, worker).is_none(), "the job ends at full health");
}

#[test]
fn unreachable_repairs_wait_before_workers_search_again() {
    use canopy_sim::economy::construction::REPAIR_RETRY_TICKS;
    // The hall stands on an island ringed by deep water; the worker is on the mainland.
    let ring = |x: i32, z: i32| {
        let d = (x - 19).abs().max((z - 19).abs());
        if (6..=9).contains(&d) { 2.0 } else { -1.0 }
    };
    let entities = vec![
        place("hall", HALL, 19.5, 19.5),
        place("worker", WORKER, 40.0, 40.0),
        resource("mine", MINE, 41.5, 13.5),
        resource("tree", TREE, 60.0, 60.0),
    ];
    let mut world = world(&sandbox_on(
        entities,
        economy_fixture::terrain(|_, _| 0.0, ring),
    ));
    let (hall, worker) = (id(&world, "hall"), id(&world, "worker"));
    let hall_id = canopy_sim::entity::EntityId(hall);
    world.state_mut().entities.get_mut(hall_id).unwrap().hp = Some(390);

    world.step();
    let first = world.state().economy.repair_retry.get(&hall_id).copied();
    assert_eq!(
        first,
        Some(REPAIR_RETRY_TICKS),
        "nobody could reach it: wait before searching again"
    );
    run(&mut world, REPAIR_RETRY_TICKS - 1);
    assert_eq!(
        world.state().economy.repair_retry.get(&hall_id).copied(),
        first,
        "no search while waiting"
    );
    run(&mut world, 2);
    let second = world.state().economy.repair_retry.get(&hall_id).copied();
    assert!(second > first, "searched again after the wait");
    assert!(job(&world, worker).is_none());
}
