//! Harvesting: the worker loop with its exact timing, the first-come queue at a source,
//! felling, depletion and regrowth, and starting workers gathering.

mod economy_fixture;

use canopy_sim::economy::{JobKind, Phase};
use economy_fixture::*;
use serde_json::json;

/// (tick, phase, cargo, wallet) whenever any of them changes.
fn timeline(
    world: &mut canopy_sim::World,
    worker: u32,
    item: &str,
    ticks: u32,
) -> Vec<(u32, String, u32, u32)> {
    let mut out: Vec<(u32, String, u32, u32)> = Vec::new();
    for _ in 0..ticks {
        let tick = world.state().tick;
        world.step();
        let phase = job(world, worker).map_or("-".to_string(), |j| j.phase.name().to_string());
        let cargo = get(world, worker)
            .unit
            .as_ref()
            .and_then(|u| u.work.cargo.as_ref())
            .map_or(0, |c| c.amount);
        let funds = wallet(world, P1, item);
        if out
            .last()
            .is_none_or(|l| (l.1.as_str(), l.2, l.3) != (phase.as_str(), cargo, funds))
        {
            out.push((tick, phase, cargo, funds));
        }
    }
    out
}

#[test]
fn a_worker_mines_carries_and_delivers_in_cycles() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("mine", MINE, 33.5, 21.5),
        place("worker", WORKER, 26.0, 20.0),
    ]));
    let (worker, mine, hall) = (id(&world, "worker"), id(&world, "mine"), id(&world, "hall"));
    command(
        &mut world,
        json!({"type": "gather", "actors": [worker], "target": mine}),
    );
    let line = timeline(&mut world, worker, AMBER, 700);
    // The exact cycle: walk to the mine's edge, wait-free admission, eight work ticks, a
    // load of five carried to the hall's edge, delivery, and straight back (turning about at
    // each end without stopping to face the way first, D51).
    let expect = [
        (0, "walk", 0, 0),
        (49, "work", 0, 0),
        (56, "return", 5, 0),
        (131, "-", 0, 5),
        (132, "walk", 0, 5),
        (206, "work", 0, 5),
        (213, "return", 5, 5),
        (288, "-", 0, 10),
    ];
    let first: Vec<(u32, &str, u32, u32)> = line[..8]
        .iter()
        .map(|l| (l.0, l.1.as_str(), l.2, l.3))
        .collect();
    assert_eq!(first, expect);
    let at = |t: u32| {
        line.iter()
            .rev()
            .find(|l| l.0 <= t)
            .cloned()
            .expect("entry")
    };
    // Walk out, work exactly eight ticks for one load of five, carry it home.
    let work = line.iter().find(|l| l.1 == "work").expect("works").0;
    let back = line.iter().find(|l| l.1 == "return").expect("returns").0;
    assert_eq!(
        back - work,
        7,
        "eight work ticks: admitted and first progress on the same tick"
    );
    assert_eq!(at(back).2, 5);
    let paid = line.iter().find(|l| l.3 == 5).expect("delivered").0;
    assert!(paid > back);
    // The next trip starts the tick after delivery, from the same source.
    assert_eq!(at(paid + 1).1, "walk");
    let j = job(&world, worker).expect("job");
    assert_eq!(
        (j.kind, j.target.0, j.source.map(|s| s.0)),
        (JobKind::Harvest, hall, Some(mine))
    );
    // Steady state: every cycle is the same length.
    let paid: Vec<u32> = line
        .windows(2)
        .filter(|w| w[1].3 > w[0].3)
        .map(|w| w[1].0)
        .collect();
    assert!(paid.len() >= 3, "{line:?}");
    let periods: Vec<u32> = paid.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(periods.windows(2).all(|w| w[0] == w[1]), "{periods:?}");
    assert_eq!(
        get(&world, mine).resource.as_ref().unwrap().amount,
        100 - wallet(&world, P1, AMBER) - at(1000).2
    );
    world.verify_economy().unwrap();
}

#[test]
fn harvesters_queue_at_a_source_in_arrival_order() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("tree", TREE, 30.0, 20.0),
        place("first", WORKER, 27.0, 20.0),
        place("second", WORKER, 29.0, 23.0),
    ]));
    let (first, second, tree) = (
        id(&world, "first"),
        id(&world, "second"),
        id(&world, "tree"),
    );
    command(
        &mut world,
        json!({"type": "gather", "actors": [first, second], "target": tree}),
    );
    let mut both_working = false;
    let mut waited = None;
    for _ in 0..200 {
        world.step();
        let phases = (
            job(&world, first).map(|j| j.phase),
            job(&world, second).map(|j| j.phase),
        );
        both_working |= phases == (Some(Phase::Work), Some(Phase::Work));
        if phases == (Some(Phase::Work), Some(Phase::Wait)) {
            waited.get_or_insert(job(&world, second).unwrap().arrived_tick.unwrap());
        }
    }
    assert!(!both_working, "the tree admits one worker at a time");
    assert!(waited.is_some(), "the later arrival waits its turn");
    assert_eq!(wallet(&world, P1, WOOD), 10, "both loads delivered");
}

#[test]
fn trees_are_felled_fall_and_the_load_leaves_after_the_fall() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("tree", TREE, 30.0, 20.0),
        place("worker", WORKER, 26.0, 20.0),
    ]));
    let (worker, tree) = (id(&world, "worker"), id(&world, "tree"));
    let cell = world
        .spatial()
        .node(canopy_sim::spatial::Spot::new(30, 20, None))
        .unwrap();
    assert!(
        !world.spatial().walkable(cell),
        "a standing tree blocks its cell"
    );
    command(
        &mut world,
        json!({"type": "gather", "actors": [worker], "target": tree}),
    );
    let mut hits = Vec::new();
    let mut fell_at = None;
    let mut left_at = None;
    for _ in 0..400 {
        let tick = world.state().tick;
        world.step();
        let f = get(&world, tree)
            .resource
            .as_ref()
            .unwrap()
            .felling
            .clone()
            .unwrap();
        if f.last_hit_tick == Some(tick) {
            hits.push(tick);
        }
        if fell_at.is_none() && f.fall_tick.is_some() {
            fell_at = f.fall_tick;
            assert_eq!(f.hp, 0);
        }
        if left_at.is_none()
            && job(&world, worker).is_some_and(|j| j.phase == Phase::Return)
            && fell_at.is_some()
        {
            left_at = Some(tick);
        }
    }
    // Strikes land on tick 3 of every 10-tick swing (progress 3 and 13 of a 20-tick
    // load); the felling blow marks the fall.
    let fell = fell_at.expect("felled");
    assert_eq!(hits.len(), 5, "{hits:?}");
    assert_eq!((hits[1] - hits[0], hits[3] - hits[2]), (10, 10), "{hits:?}");
    assert_eq!(hits[4], fell);
    assert_eq!(fell - hits[3], 7, "progress 13 to 20");
    // Two loads of five: the second load fells the tree; it falls for 6 ticks.
    assert_eq!(left_at.expect("left after the fall"), fell + 6);
    assert_eq!(get(&world, tree).resource.as_ref().unwrap().amount, 0);
    assert!(
        world.spatial().walkable(cell),
        "a felled tree leaves occupancy at once"
    );
    assert_eq!(wallet(&world, P1, WOOD), 10);
    let accounting = &world.state().accounting.produced;
    assert_eq!(accounting.get(WOOD).copied(), Some(10));
    world.verify_economy().unwrap();
}

#[test]
fn research_speeds_up_felling() {
    let base = {
        let mut world = world(&sandbox(vec![
            place("hall", HALL, 19.5, 19.5),
            resource("tree", TREE, 30.0, 20.0),
            place("worker", WORKER, 26.0, 20.0),
        ]));
        let (worker, tree) = (id(&world, "worker"), id(&world, "tree"));
        command(
            &mut world,
            json!({"type": "gather", "actors": [worker], "target": tree}),
        );
        run_until(&mut world, 400, |w| wallet(w, P1, WOOD) == 5)
    };
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("tree", TREE, 30.0, 20.0),
        place("worker", WORKER, 26.0, 20.0),
    ]));
    world
        .state_mut()
        .economy
        .research
        .insert(P1, vec![canopy_content::Id::new(TOOLS).unwrap()]);
    let (worker, tree) = (id(&world, "worker"), id(&world, "tree"));
    command(
        &mut world,
        json!({"type": "gather", "actors": [worker], "target": tree}),
    );
    let fast = run_until(&mut world, 400, |w| wallet(w, P1, WOOD) == 5);
    assert_eq!(base - fast, 10, "twenty work ticks at double rate take ten");
}

#[test]
fn a_forester_replants_and_trees_regrow_when_clear() {
    let mut world = world(&sandbox(vec![
        place("forester", FORESTER, 25.5, 13.5),
        with_state(resource("stump", TREE, 30.0, 20.0), json!({"amount": 0})),
        place("planter", WORKER, 24.0, 20.0),
        place("sitter", SOLDIER, 30.0, 20.0),
    ]));
    let (forester, stump) = (id(&world, "forester"), id(&world, "stump"));
    let (planter, sitter) = (id(&world, "planter"), id(&world, "sitter"));
    let cell = world
        .spatial()
        .node(canopy_sim::spatial::Spot::new(30, 20, None))
        .unwrap();
    assert!(world.spatial().walkable(cell), "a stump does not block");
    run_until(&mut world, 400, |w| {
        get(w, stump)
            .resource
            .as_ref()
            .unwrap()
            .growing_until
            .is_some()
    });
    let planted = world.state().tick - 1;
    let p = get(&world, forester).production.as_ref().unwrap();
    assert_eq!(p.produced, 1);
    assert!(p.active.is_none());
    assert_eq!(
        get(&world, planter)
            .unit
            .as_ref()
            .unwrap()
            .work
            .employment
            .map(|e| e.0),
        Some(forester),
        "the planter stays on staff"
    );
    let until = get(&world, stump)
        .resource
        .as_ref()
        .unwrap()
        .growing_until
        .unwrap();
    assert_eq!(until, planted + 50);
    // The soldier parked on the stump holds the regrowth until it has been walked out.
    let now = world.state().tick;
    run(&mut world, until - now);
    assert_eq!(get(&world, sitter).position.cell(), (30, 20));
    run_until(&mut world, 300, |w| {
        get(w, stump).resource.as_ref().unwrap().amount == 10
    });
    assert!(world.state().tick - 1 > until, "the sitter delayed it");
    let r = get(&world, stump).resource.as_ref().unwrap();
    assert!(r.growing_until.is_none());
    assert_eq!(r.felling.as_ref().unwrap().hp, 10);
    assert!(
        !world.spatial().walkable(cell),
        "the regrown tree blocks again"
    );
    assert_ne!(get(&world, sitter).position.cell(), (30, 20));
    world.verify_economy().unwrap();
}

#[test]
fn starting_workers_gather_from_the_start() {
    let mut world = world(&skirmish(vec![]));
    let workers = all_of(&world, P1, WORKER);
    assert_eq!(workers.len(), 3);
    let gathering: Vec<bool> = workers
        .iter()
        .map(|&w| {
            matches!(
                get(&world, w).unit.as_ref().unwrap().order,
                Some(canopy_sim::orders::Order::Gather { .. })
            )
        })
        .collect();
    // The setup sends two of the three.
    assert_eq!(gathering, [true, true, false]);
    assert_eq!(
        wallet(&world, P1, AMBER),
        100,
        "the hall's authored stock became funds"
    );
    assert!(
        get(&world, id(&world, "start.player.1/main-fort"))
            .inventory
            .is_empty()
    );
    run(&mut world, 600);
    assert!(wallet(&world, P1, AMBER) > 100);
    assert!(wallet(&world, P2, AMBER) > 100);
    world.verify_economy().unwrap();
}

#[test]
fn gather_command_validation() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("mine", MINE, 33.5, 21.5),
        with_state(resource("empty", TREE, 40.0, 30.0), json!({"amount": 0})),
        place("worker", WORKER, 26.0, 20.0),
        place("soldier", SOLDIER, 26.0, 24.0),
        place_for("theirs", WORKER, "player.2", 28.0, 28.0),
    ]));
    let (worker, soldier, theirs) = (
        id(&world, "worker"),
        id(&world, "soldier"),
        id(&world, "theirs"),
    );
    let (mine, empty) = (id(&world, "mine"), id(&world, "empty"));
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "gather", "actors": [worker], "target": empty})
        ),
        Some("message.resource-is-not-visible".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "gather", "actors": [soldier], "target": mine})
        ),
        Some("message.no-eligible-workers-can-gather-this".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "gather", "actors": [theirs], "target": mine})
        ),
        Some("message.not-your-actors".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "gather", "actors": [worker, soldier], "target": mine})
        ),
        None
    );
}

#[test]
fn loads_go_to_the_store_nearest_the_source() {
    // The worker stands at the far hall, as after delivering there; a second hall stands
    // by the mine (a new base): every trip carries to the near one.
    let mut world = world(&sandbox(vec![
        place("far-hall", HALL, 19.5, 19.5),
        place("near-hall", HALL, 71.5, 19.5),
        resource("mine", MINE, 61.5, 21.5),
        place("worker", WORKER, 24.0, 20.0),
    ]));
    let (worker, mine, near) = (
        id(&world, "worker"),
        id(&world, "mine"),
        id(&world, "near-hall"),
    );
    command(
        &mut world,
        json!({"type": "gather", "actors": [worker], "target": mine}),
    );
    let mut stores = Vec::new();
    for _ in 0..1200 {
        world.step();
        if let Some(j) = job(&world, worker)
            && stores.last() != Some(&j.target.0)
        {
            stores.push(j.target.0);
        }
    }
    assert_eq!(stores, [near], "every trip ends at the hall by the mine");
    assert!(wallet(&world, P1, AMBER) >= 10, "several loads delivered");
}
