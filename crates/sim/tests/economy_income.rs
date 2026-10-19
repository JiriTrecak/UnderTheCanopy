//! Buildings that earn by themselves (`income`; D62, a Substance Well pumping its deposit
//! with no workers): nothing while a site stands, a steady amount every interval once
//! finished, accounted and shown to its owner only as a delivery at the building, held
//! while the building is unpowered, stopped when it falls, and resumed exactly from a
//! save.

mod economy_fixture;

use canopy_sim::{World, entity::EntityId, state::FactKind};
use economy_fixture::*;
use serde_json::{Value, json};

const AT: (f64, f64) = (41.5, 41.5);

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

/// Steps `ticks` and counts the deliveries `owner` is shown from `store`.
fn shown(
    world: &mut World,
    ticks: u32,
    owner: canopy_content::placement::Owner,
    store: u32,
) -> u32 {
    let mut count = 0;
    for _ in 0..ticks {
        world.step();
        count += world
            .view(owner)
            .deliveries
            .iter()
            .filter(|d| d.store.0 == store && d.carrier.is_none())
            .count() as u32;
    }
    count
}

#[test]
fn a_pump_on_its_deposit_earns_by_itself_once_finished() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        place("worker", WORKER, 34.0, 34.0),
        resource("spring", SPRING, AT.0, AT.1),
        place_for("rival", HALL, "player.2", 99.5, 99.5),
    ]));
    fund(&mut world, P1, AMBER, 100);
    let worker = id(&world, "worker");
    assert_eq!(
        try_command(&mut world, build(worker, PUMP, AT.0, AT.1)),
        None
    );
    let pump = all_of(&world, P1, PUMP)[0];
    run_until(&mut world, 600, |w| {
        get(w, pump).construction.is_none() || wallet(w, P1, ESSENCE) > 0
    });
    assert_eq!(wallet(&world, P1, ESSENCE), 0, "a site earns nothing");
    assert!(get(&world, pump).construction.is_none());
    // The worker that built it goes idle: nobody works the pump.
    run(&mut world, 5);
    assert!(job(&world, worker).is_none());
    let produced = |w: &World| {
        w.state()
            .accounting
            .produced
            .get(ESSENCE)
            .copied()
            .unwrap_or(0)
    };
    let before = (wallet(&world, P1, ESSENCE), produced(&world));
    let count = shown(&mut world, 4 * PUMP_TICKS, P1, pump);
    assert_eq!(count, 4, "one delivery per interval, at the pump");
    assert_eq!(wallet(&world, P1, ESSENCE) - before.0, 4 * PUMP_AMOUNT);
    assert_eq!(produced(&world) - before.1, 4 * PUMP_AMOUNT, "accounted");
    let income_fact = |f: &&canopy_sim::state::Fact| {
        f.owner == P1
            && f.kind == FactKind::Produced
            && f.item.as_ref().is_some_and(|i| i.as_str() == ESSENCE)
    };
    assert!(
        world.state().facts.iter().any(|f| income_fact(&f)),
        "a produced fact for the owner"
    );
    assert_eq!(
        shown(&mut world, 2 * PUMP_TICKS, P2, pump),
        0,
        "the rival is never shown another's income"
    );
    assert_eq!(
        get(&world, id(&world, "spring"))
            .resource
            .as_ref()
            .unwrap()
            .amount,
        50,
        "the deposit is left as it was"
    );
    // A fallen pump earns nothing more.
    destroy(&mut world, pump);
    let after = wallet(&world, P1, ESSENCE);
    run(&mut world, 3 * PUMP_TICKS);
    assert_eq!(wallet(&world, P1, ESSENCE), after);
    world.verify_economy().unwrap();
}

#[test]
fn an_earner_that_needs_power_holds_its_count_while_unpowered() {
    let map = sandbox(vec![
        place("nexus", NEXUS, 19.5, 19.5),
        // Inside the nexus's disc, and one far outside any network.
        place("engine", ENGINE, 29.5, 21.5),
        place("stray", ENGINE, 121.5, 121.5),
    ]);
    let mut world = world(&map);
    let (engine, stray, nexus) = (
        id(&world, "engine"),
        id(&world, "stray"),
        id(&world, "nexus"),
    );
    let start = wallet(&world, P1, AMBER);
    assert_eq!(
        shown(&mut world, 3 * ENGINE_TICKS, P1, engine),
        3,
        "powered, it earns every interval"
    );
    assert_eq!(wallet(&world, P1, AMBER) - start, 3 * ENGINE_AMOUNT);
    assert_eq!(
        shown(&mut world, 3 * ENGINE_TICKS, P1, stray),
        0,
        "unpowered, it earns nothing"
    );
    // Part-way through an interval the power fails; the count holds until it returns.
    run(&mut world, ENGINE_TICKS / 2);
    let held = get(&world, engine).income;
    assert!(held > 0 && held < ENGINE_TICKS);
    let save = world.save();
    destroy(&mut world, nexus);
    let at = get(&world, engine).income;
    assert_eq!(shown(&mut world, 4 * ENGINE_TICKS, P1, engine), 0);
    assert_eq!(get(&world, engine).income, at, "held while unpowered");
    // A restored save resumes the count exactly where it was.
    let mut restored = World::restore(content(), &map, save).unwrap();
    assert_eq!(get(&restored, engine).income, held);
    let left = ENGINE_TICKS - held;
    assert_eq!(shown(&mut restored, left - 1, P1, engine), 0);
    assert_eq!(shown(&mut restored, 1, P1, engine), 1);
}
