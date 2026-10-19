//! One queue per building (D72): a building that trains and researches works on one item
//! at a time, in the order the player queued them; cancelling lets the next go on; saves
//! keep the order, and saves from before it load with research after the training.

mod economy_fixture;

use canopy_content::map::UtcMap;
use canopy_sim::{
    World,
    economy::{
        Status,
        lane::{self, Lane},
    },
};
use economy_fixture::*;
use serde_json::{Value, json};

/// A powered workshop (trains soldiers, researches Amplify) and a house for supply.
fn workshop() -> (UtcMap, World, u32) {
    let map = sandbox(vec![
        place("nexus", NEXUS, 19.5, 19.5),
        place("shop", WORKSHOP, 29.5, 21.5),
        place("house", HOUSE, 61.5, 61.5),
    ]);
    let mut world = world(&map);
    fund(&mut world, P1, AMBER, 200);
    let shop = id(&world, "shop");
    (map, world, shop)
}

fn train(shop: u32) -> Value {
    json!({"type": "produce", "actor": shop, "definition": SOLDIER})
}

fn research(shop: u32) -> Value {
    json!({"type": "research", "actor": shop, "research": AMPLIFY})
}

/// Progress of the unit in training, if one is under way.
fn training(world: &World, shop: u32) -> Option<u32> {
    get(world, shop)
        .production
        .as_ref()
        .and_then(|p| p.active.as_ref())
        .map(|a| a.progress)
}

/// Progress of the queued research, if any is queued.
fn researching(world: &World, shop: u32) -> Option<u32> {
    get(world, shop)
        .research
        .as_ref()
        .and_then(|r| r.queue.first())
        .map(|r| r.progress)
}

fn learned(world: &World) -> bool {
    world
        .state()
        .economy
        .researched(P1)
        .iter()
        .any(|r| r == AMPLIFY)
}

fn status(world: &World, shop: u32) -> Status {
    get(world, shop).production.as_ref().unwrap().status
}

#[test]
fn a_building_works_on_what_was_queued_first_then_the_next_whatever_its_kind() {
    let (_, mut world, shop) = workshop();
    // A unit, then research, then another unit.
    assert_eq!(try_command(&mut world, train(shop)), None);
    assert_eq!(try_command(&mut world, research(shop)), None);
    assert_eq!(try_command(&mut world, train(shop)), None);
    run(&mut world, 10);
    assert!(training(&world, shop).is_some_and(|p| p > 0));
    assert_eq!(researching(&world, shop), Some(0), "research waits");
    // The first unit steps out; the research goes on, and the second unit waits for it.
    run_until(&mut world, 100, |w| all_of(w, P1, SOLDIER).len() == 1);
    run(&mut world, 5);
    assert!(researching(&world, shop).is_some_and(|p| p > 0));
    assert_eq!(training(&world, shop), None, "the next unit waits");
    assert_eq!(status(&world, shop), Status::Researching);
    let order: Vec<Lane> = lane::of(get(&world, shop))
        .iter()
        .map(|i| i.lane())
        .collect();
    assert_eq!(order, [Lane::Research, Lane::Production]);
    run_until(&mut world, 100, learned);
    assert_eq!(all_of(&world, P1, SOLDIER).len(), 1);
    // Then the second unit trains.
    run(&mut world, 3);
    assert!(training(&world, shop).is_some_and(|p| p > 0));
    run_until(&mut world, 100, |w| all_of(w, P1, SOLDIER).len() == 2);
    assert!(lane::of(get(&world, shop)).is_empty());
}

#[test]
fn research_queued_first_holds_training_until_cancelled() {
    let (_, mut world, shop) = workshop();
    assert_eq!(try_command(&mut world, research(shop)), None);
    assert_eq!(try_command(&mut world, train(shop)), None);
    run(&mut world, 10);
    assert!(researching(&world, shop).is_some_and(|p| p > 0));
    assert_eq!(
        training(&world, shop),
        None,
        "the unit waits for the research"
    );
    let paid = wallet(&world, P1, AMBER);
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "cancelResearch", "actor": shop, "research": AMPLIFY})
        ),
        None
    );
    assert_eq!(wallet(&world, P1, AMBER), paid + 10, "refunded");
    run(&mut world, 3);
    assert!(
        training(&world, shop).is_some_and(|p| p > 0),
        "the unit goes on"
    );
    run_until(&mut world, 100, |w| !all_of(w, P1, SOLDIER).is_empty());
    assert!(!learned(&world));
}

#[test]
fn cancelling_the_unit_in_training_lets_the_research_go_on() {
    let (_, mut world, shop) = workshop();
    assert_eq!(try_command(&mut world, train(shop)), None);
    assert_eq!(try_command(&mut world, research(shop)), None);
    run(&mut world, 10);
    assert_eq!(researching(&world, shop), Some(0));
    let entry = get(&world, shop).production.as_ref().unwrap().queue[0].id;
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "cancel", "actor": shop, "queue": entry})
        ),
        None
    );
    assert_eq!(training(&world, shop), None);
    run(&mut world, 3);
    assert!(researching(&world, shop).is_some_and(|p| p > 0));
    run_until(&mut world, 100, learned);
    assert!(all_of(&world, P1, SOLDIER).is_empty());
}

#[test]
fn a_restored_save_keeps_the_order_and_plays_on_identically() {
    let (map, mut world, shop) = workshop();
    command(&mut world, research(shop));
    world.step();
    command(&mut world, train(shop));
    run(&mut world, 10);
    let text = serde_json::to_string(&world.save()).unwrap();
    let mut restored = World::restore(content(), &map, serde_json::from_str(&text).unwrap())
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(lane::head(get(&restored, shop)), Some(Lane::Research));
    for _ in 0..80 {
        world.step();
        restored.step();
        assert_eq!(world.fingerprint(), restored.fingerprint());
    }
    assert_eq!(world.save(), restored.save());
    assert!(learned(&restored));
}

#[test]
fn a_save_from_before_the_one_queue_loads_with_research_after_training() {
    let (map, mut world, shop) = workshop();
    // Research queued first; the old save did not record the order.
    command(&mut world, research(shop));
    world.step();
    command(&mut world, train(shop));
    run(&mut world, 2);
    let mut save = serde_json::to_value(world.save()).unwrap();
    let entities = save["state"]["entities"].as_array_mut().unwrap();
    let mut stripped = 0;
    for e in entities.iter_mut() {
        if let Some(queue) = e
            .pointer_mut("/research/queue")
            .and_then(Value::as_array_mut)
        {
            for entry in queue {
                stripped += usize::from(entry.as_object_mut().unwrap().remove("order").is_some());
            }
        }
    }
    assert_eq!(stripped, 1, "the save holds the order where expected");
    let mut old = World::restore(content(), &map, serde_json::from_value(save).unwrap())
        .unwrap_or_else(|e| panic!("{e}"));
    // Items queued after loading go after the old ones.
    assert_eq!(try_command(&mut old, train(shop)), None);
    let order: Vec<Lane> = lane::of(get(&old, shop)).iter().map(|i| i.lane()).collect();
    assert_eq!(order, [Lane::Production, Lane::Research, Lane::Production]);
    let held = researching(&old, shop);
    run_until(&mut old, 100, |w| !all_of(w, P1, SOLDIER).is_empty());
    assert_eq!(
        researching(&old, shop),
        held.map(|p| p + 1),
        "research waited for the old unit, and went on the tick it stepped out"
    );
    run_until(&mut old, 100, learned);
    assert_eq!(
        all_of(&old, P1, SOLDIER).len(),
        1,
        "the new unit waited too"
    );
    run_until(&mut old, 100, |w| all_of(w, P1, SOLDIER).len() == 2);
}
