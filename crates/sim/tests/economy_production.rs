//! Production: queued training, its bill, timing and deployment; queue capacity; rally
//! points onto places, resources and units; pause; cancel; supply and unit limits.

mod economy_fixture;

use canopy_sim::{
    economy::Status,
    entity::{EntityId, Place},
    orders::Order,
    state::FactKind,
};
use economy_fixture::*;
use serde_json::{Value, json};

fn produce(building: u32, definition: &str) -> Value {
    json!({"type": "produce", "actor": building, "definition": definition})
}

fn hall_world(extra: Vec<Value>) -> canopy_sim::World {
    let mut entities = vec![place("hall", HALL, 19.5, 19.5)];
    entities.extend(extra);
    let mut world = world(&sandbox(entities));
    fund(&mut world, P1, AMBER, 200);
    world
}

#[test]
fn a_queued_unit_is_paid_trained_and_deployed() {
    let mut world = hall_world(vec![]);
    let hall = id(&world, "hall");
    assert_eq!(try_command(&mut world, produce(hall, WORKER)), None);
    let started = world.state().tick - 1;
    let p = get(&world, hall).production.as_ref().unwrap();
    assert_eq!(p.queue.len(), 1);
    assert_eq!(
        p.active.as_ref().map(|a| a.progress),
        Some(1),
        "starts and progresses the same tick"
    );
    assert_eq!(p.status, Status::Training);
    assert_eq!(wallet(&world, P1, AMBER), 190);
    assert_eq!(
        get(&world, hall).inventory.get(AMBER).copied(),
        Some(10),
        "the bill waits in the hall"
    );
    let ready = run_until(&mut world, 100, |w| !all_of(w, P1, WORKER).is_empty());
    assert_eq!(
        started + ready + 1,
        started + 20,
        "twenty ticks of training"
    );
    let unit = all_of(&world, P1, WORKER)[0];
    let e = get(&world, unit);
    assert_eq!(e.ready_tick, world.state().tick);
    assert_eq!(e.hp, Some(100));
    // Out of the side facing the doorway (entrance below the hall): the first ring cell
    // nearest to it.
    assert_eq!(e.position.cell(), (20, 24));
    let p = get(&world, hall).production.as_ref().unwrap();
    assert!(p.queue.is_empty() && p.active.is_none());
    assert_eq!((p.produced, p.status), (1, Status::Idle));
    assert!(get(&world, hall).inventory.is_empty());
    let consumed = world
        .state()
        .facts
        .iter()
        .find(|f| f.message == "message.production-input-consumed")
        .unwrap();
    assert_eq!(
        (consumed.kind, consumed.amount),
        (FactKind::Consumed, Some(10))
    );
    world.verify_economy().unwrap();
}

#[test]
fn production_command_validation() {
    let mut world = hall_world(vec![
        place("house", HOUSE, 33.5, 13.5),
        place("forester", FORESTER, 33.5, 25.5),
        place_for("theirs", WORKER, "player.2", 40.0, 40.0),
    ]);
    let (hall, house, forester, theirs) = (
        id(&world, "hall"),
        id(&world, "house"),
        id(&world, "forester"),
        id(&world, "theirs"),
    );
    assert_eq!(
        try_command(&mut world, produce(hall, SOLDIER)),
        Some("message.unsupported-output".into())
    );
    assert_eq!(
        try_command(&mut world, produce(house, WORKER)),
        Some("message.no-completed-production-capability".into())
    );
    assert_eq!(
        try_command(&mut world, produce(forester, TREE)),
        Some("message.unsupported-output".into())
    );
    for _ in 0..3 {
        command(&mut world, produce(hall, WORKER));
    }
    world.step();
    assert_eq!(
        try_command(&mut world, produce(hall, WORKER)),
        Some("message.queue-is-full".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "cancel", "actor": hall, "queue": 999})
        ),
        Some("message.queue-entry-no-longer-exists".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "rally", "actor": forester, "destination": {"x": 5, "y": 5}})
        ),
        Some("message.no-unit-production".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "rally", "actor": hall, "destination": null, "target": theirs})
        ),
        Some("message.rally-onto-a-visible-resource-or".into())
    );
    fund(&mut world, P1, AMBER, 0);
    let mut world2 = hall_world(vec![]);
    fund(&mut world2, P1, AMBER, 5);
    let hall2 = id(&world2, "hall");
    assert_eq!(
        try_command(&mut world2, produce(hall2, WORKER)),
        Some("message.insufficient-materials".into())
    );
}

#[test]
fn cancelling_refunds_the_entry_and_stops_its_training() {
    let mut world = hall_world(vec![]);
    let hall = id(&world, "hall");
    command(&mut world, produce(hall, WORKER));
    command(&mut world, produce(hall, WORKER));
    world.step();
    assert_eq!(wallet(&world, P1, AMBER), 180);
    let (first, second) = {
        let q = &get(&world, hall).production.as_ref().unwrap().queue;
        (q[0].id, q[1].id)
    };
    run(&mut world, 5);
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "cancel", "actor": hall, "queue": first})
        ),
        None
    );
    assert_eq!(wallet(&world, P1, AMBER), 190);
    let p = get(&world, hall).production.as_ref().unwrap();
    assert_eq!(p.queue.iter().map(|q| q.id).collect::<Vec<_>>(), [second]);
    assert_eq!(
        p.active.as_ref().map(|a| (a.queue, a.progress)),
        Some((Some(second), 1)),
        "the next one starts afresh"
    );
}

#[test]
fn pausing_halts_training_until_resumed() {
    let mut world = hall_world(vec![]);
    let hall = id(&world, "hall");
    command(
        &mut world,
        json!({"type": "pause", "actor": hall, "paused": true}),
    );
    command(&mut world, produce(hall, WORKER));
    run(&mut world, 10);
    let p = get(&world, hall).production.as_ref().unwrap();
    assert_eq!(
        (p.status, p.active.is_none(), p.queue.len()),
        (Status::Paused, true, 1)
    );
    command(
        &mut world,
        json!({"type": "pause", "actor": hall, "paused": false}),
    );
    run(&mut world, 5);
    command(
        &mut world,
        json!({"type": "pause", "actor": hall, "paused": true}),
    );
    run(&mut world, 30);
    let p = get(&world, hall).production.as_ref().unwrap();
    assert_eq!(
        p.active.as_ref().map(|a| a.progress),
        Some(5),
        "progress froze while paused"
    );
    assert_eq!(p.status, Status::Paused);
    command(
        &mut world,
        json!({"type": "pause", "actor": hall, "paused": false}),
    );
    run_until(&mut world, 40, |w| !all_of(w, P1, WORKER).is_empty());
}

#[test]
fn rally_points_send_units_to_a_place_a_resource_or_a_friend() {
    let mut world = hall_world(vec![
        resource("mine", MINE, 25.5, 29.5),
        place("friend", SOLDIER, 30.0, 12.0),
    ]);
    let (hall, mine, friend) = (id(&world, "hall"), id(&world, "mine"), id(&world, "friend"));
    let order_of_next = |world: &mut canopy_sim::World| {
        let before = all_of(world, P1, WORKER).len();
        command(world, produce(hall, WORKER));
        run_until(world, 100, |w| all_of(w, P1, WORKER).len() > before);
        let unit = *all_of(world, P1, WORKER).last().unwrap();
        (unit, get(world, unit).unit.as_ref().unwrap().order.clone())
    };
    command(
        &mut world,
        json!({"type": "rally", "actor": hall, "destination": {"x": 30, "y": 30}}),
    );
    let (_, order) = order_of_next(&mut world);
    assert_eq!(
        order,
        Some(Order::Move {
            destination: Place::new(30, 30),
            attack_move: false
        })
    );
    command(
        &mut world,
        json!({"type": "rally", "actor": hall, "destination": null, "target": mine}),
    );
    let (unit, order) = order_of_next(&mut world);
    assert_eq!(
        order,
        Some(Order::Gather {
            target: EntityId(mine)
        })
    );
    // Harvesters set off on the next assignment.
    run(&mut world, 2);
    assert!(job(&world, unit).is_some());
    command(
        &mut world,
        json!({"type": "rally", "actor": hall, "destination": null, "target": friend}),
    );
    let (unit, order) = order_of_next(&mut world);
    assert_eq!(
        order,
        Some(Order::Follow {
            target: EntityId(friend),
            escort: false
        })
    );
    // Deployed on the side facing the followed unit (above the hall).
    assert!(
        get(&world, unit).position.y < 16_000,
        "{:?}",
        get(&world, unit).position
    );
    command(
        &mut world,
        json!({"type": "rally", "actor": hall, "destination": null}),
    );
    let (_, order) = order_of_next(&mut world);
    assert_eq!(order, None);
}

#[test]
fn only_training_takes_supply() {
    let mut entities = vec![place("house", HOUSE, 33.5, 13.5)];
    for i in 0..4 {
        entities.push(place(
            &format!("w{i}"),
            WORKER,
            30.0 + 2.0 * f64::from(i),
            30.0,
        ));
    }
    let mut world = hall_world(entities);
    let (hall, house) = (id(&world, "hall"), id(&world, "house"));
    // Supply 10 (hall 6, house 4); four workers use 4.
    for _ in 0..3 {
        command(&mut world, produce(hall, WORKER));
    }
    world.step();
    assert_eq!(
        get(&world, hall).production.as_ref().unwrap().queue.len(),
        3
    );
    let s = world.view(P1).colony.unwrap().supply;
    assert_eq!(
        (s.reserved, s.waiting, s.committed),
        (1, 2, 5),
        "the two waiting take none"
    );
    // The house falls: queued units already paid for wait for supply instead.
    run(&mut world, 25);
    world.remove_entity(EntityId(house));
    run(&mut world, 25);
    let workers = all_of(&world, P1, WORKER).len();
    assert_eq!(workers, 6, "five used, the one in training finishes");
    let p = get(&world, hall).production.as_ref().unwrap();
    assert_eq!(p.status, Status::SupplyBlocked);
    assert_eq!(p.queue.len(), 1);
    assert_eq!(
        p.status.label(world.economy().hint(P1)),
        canopy_sim::msg!(
            "status.supply-blocked-build",
            name = canopy_content::text::name("building.test.house")
        )
    );
    let s = world.view(P1).colony.unwrap().supply;
    assert_eq!((s.used, s.reserved, s.waiting, s.capacity), (6, 0, 1, 6));
    // Behind the waiting one, another can still be queued.
    assert_eq!(try_command(&mut world, produce(hall, WORKER)), None);
    world.step();
    let queue: Vec<u32> = get(&world, hall)
        .production
        .as_ref()
        .unwrap()
        .queue
        .iter()
        .map(|q| q.id)
        .collect();
    assert_eq!(queue.len(), 2);
    for entry in queue {
        command(
            &mut world,
            json!({"type": "cancel", "actor": hall, "queue": entry}),
        );
    }
    world.step();
    // With nothing ahead it would start at once, so it needs supply now.
    assert_eq!(
        try_command(&mut world, produce(hall, WORKER)),
        Some(canopy_sim::msg!(
            "message.not-enough-supply-build",
            used = 6,
            capacity = 6,
            name = canopy_content::text::name("building.test.house")
        ))
    );
}

#[test]
fn the_unit_limit_counts_queued_units() {
    let mut entities = vec![
        place("house-a", HOUSE, 33.5, 13.5),
        place("house-b", HOUSE, 37.5, 13.5),
    ];
    for i in 0..10 {
        entities.push(place(
            &format!("w{i}"),
            WORKER,
            30.0 + 2.0 * f64::from(i),
            40.0,
        ));
    }
    let mut world = hall_world(entities);
    let hall = id(&world, "hall");
    command(&mut world, produce(hall, WORKER));
    command(&mut world, produce(hall, WORKER));
    world.step();
    assert_eq!(
        try_command(&mut world, produce(hall, WORKER)),
        Some("message.unit-limit-reached".into())
    );
}

#[test]
fn one_training_can_hatch_a_pair_each_using_its_own_supply() {
    let mut world = hall_world(vec![place("barracks", BARRACKS, 39.5, 19.5)]);
    let barracks = id(&world, "barracks");
    assert_eq!(try_command(&mut world, produce(barracks, TWINS)), None);
    world.step();
    let colony = world.view(P1).colony.unwrap();
    assert_eq!((colony.supply.reserved, colony.supply.training), (2, 2));
    run_until(&mut world, 60, |w| !all_of(w, P1, TWINS).is_empty());
    let twins = all_of(&world, P1, TWINS);
    assert_eq!(twins.len(), 2, "one price, two units");
    assert_ne!(
        get(&world, twins[0]).position,
        get(&world, twins[1]).position
    );
    assert_eq!(wallet(&world, P1, AMBER), 188);
    assert_eq!(world.view(P1).colony.unwrap().supply.used, 2);
}
