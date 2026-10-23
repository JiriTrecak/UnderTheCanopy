//! What the HUD and renderer see of the economy: goods, supply and workers, queue and
//! construction progress, a worker's job and work pose, and a mine's workforce.

mod economy_fixture;

use canopy_content::game::WorkAnimation;
use canopy_sim::economy::{JobKind, Phase};
use economy_fixture::*;
use serde_json::json;

#[test]
fn the_owner_sees_its_ledger_queues_and_workers() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("mine", MINE, 33.5, 21.5),
        resource("tree", TREE, 22.0, 30.0),
        place("miner", WORKER, 28.0, 20.0),
        place("feller", WORKER, 20.0, 28.0),
        place("idle", WORKER, 14.0, 28.0),
        place_for("enemy", WORKER, "player.2", 26.0, 26.0),
    ]));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 7);
    let (hall, mine, tree) = (id(&world, "hall"), id(&world, "mine"), id(&world, "tree"));
    let (miner, feller) = (id(&world, "miner"), id(&world, "feller"));
    command(
        &mut world,
        json!({"type": "gather", "actors": [miner], "target": mine}),
    );
    command(
        &mut world,
        json!({"type": "gather", "actors": [feller], "target": tree}),
    );
    command(
        &mut world,
        json!({"type": "produce", "actor": hall, "definition": WORKER}),
    );
    command(
        &mut world,
        json!({"type": "produce", "actor": hall, "definition": WORKER}),
    );
    run_until(&mut world, 200, |w| {
        job(w, feller).is_some_and(|j| j.phase == Phase::Work)
    });
    let view = world.view(P1);
    let colony = view.colony.as_ref().expect("own colony");
    let goods: Vec<(&str, u32, u32, u32)> = colony
        .goods
        .iter()
        .map(|g| (g.item.as_str(), g.stored, g.available, g.reserved))
        .collect();
    // Display order: amber, then wood. Queued units hold their bill in the hall: stored,
    // not available.
    assert_eq!(goods[0].0, AMBER);
    assert_eq!(goods[1], (WOOD, 7, 7, 0));
    assert_eq!(goods[0].1 - goods[0].2, goods[0].3);
    assert_eq!(
        colony.population.workers,
        all_of(&world, P1, WORKER).len() as u32
    );
    assert!(colony.population.available >= 1, "the idle worker");
    assert_eq!(colony.supply.capacity, 6);
    let hall_view = view.entity(canopy_sim::entity::EntityId(hall)).unwrap();
    let private = hall_view.private.as_ref().unwrap();
    let training = private.economy.training.expect("training");
    assert_eq!(training.total, 20);
    assert_eq!(private.economy.status.as_deref(), Some("status.training"));
    assert_eq!(private.economy.production.as_ref().unwrap().queue.len(), 2);
    // The feller's job, label and chopping pose facing the tree.
    let feller_view = view.entity(canopy_sim::entity::EntityId(feller)).unwrap();
    let job = feller_view
        .private
        .as_ref()
        .unwrap()
        .economy
        .job
        .clone()
        .unwrap();
    assert_eq!((job.kind, job.phase), (JobKind::Harvest, Phase::Work));
    assert_eq!(
        feller_view.private.as_ref().unwrap().economy.label,
        "harvest"
    );
    let pose = feller_view.work.as_ref().unwrap().pose.expect("working");
    assert_eq!(pose.animation, WorkAnimation::Chop);
    assert_eq!(pose.target, get(&world, tree).position);
    assert_eq!(pose.cycle.map(|c| c.0), Some(10));
    // The mine shows its workforce; enemies see none of the private state.
    let mine_view = view.entity(canopy_sim::entity::EntityId(mine)).unwrap();
    assert_eq!(
        mine_view
            .work
            .as_ref()
            .unwrap()
            .site
            .map(|s| (s.workers, s.recommended)),
        Some((1, 2))
    );
    let enemy = world.view(P2);
    assert!(
        enemy
            .colony
            .as_ref()
            .unwrap()
            .goods
            .iter()
            .all(|g| g.stored == 0)
    );
    assert!(
        enemy
            .entity(canopy_sim::entity::EntityId(hall))
            .is_none_or(|h| h.private.is_none())
    );
}

#[test]
fn a_mine_works_while_workers_gather_from_it() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("mine", MINE, 33.5, 21.5),
        place("miner", WORKER, 28.0, 20.0),
    ]));
    let (mine, miner) = (id(&world, "mine"), id(&world, "miner"));
    let busy = |world: &canopy_sim::World| {
        world
            .view(P1)
            .entity(canopy_sim::entity::EntityId(mine))
            .and_then(|e| e.work.as_ref())
            .is_some_and(|w| w.busy)
    };
    run(&mut world, 2);
    assert!(!busy(&world), "an idle mine rests");
    command(
        &mut world,
        json!({"type": "gather", "actors": [miner], "target": mine}),
    );
    run(&mut world, 2);
    // Its work clip and effects play while it is gathered from (D65).
    assert!(busy(&world));
    command(&mut world, json!({"type": "stop", "actors": [miner]}));
    run_until(&mut world, 200, |w| !busy(w));
}

#[test]
fn construction_progress_and_carried_goods_are_shown() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        resource("mine", MINE, 33.5, 21.5),
        place("builder", WORKER, 26.0, 26.0),
        place("miner", WORKER, 28.0, 20.0),
    ]));
    fund(&mut world, P1, AMBER, 100);
    fund(&mut world, P1, WOOD, 50);
    let (builder, miner, mine) = (
        id(&world, "builder"),
        id(&world, "miner"),
        id(&world, "mine"),
    );
    command(
        &mut world,
        json!({"type": "build", "actors": [builder], "definition": HOUSE,
        "position": {"x": 29.5, "y": 29.5}}),
    );
    command(
        &mut world,
        json!({"type": "gather", "actors": [miner], "target": mine}),
    );
    run_until(&mut world, 300, |w| {
        job(w, miner).is_some_and(|j| j.phase == Phase::Return)
    });
    let site = all_of(&world, P1, HOUSE)[0];
    let view = world.view(P1);
    let site_view = view.entity(canopy_sim::entity::EntityId(site)).unwrap();
    let progress = site_view
        .private
        .as_ref()
        .unwrap()
        .economy
        .construction
        .unwrap();
    assert_eq!(progress.total, 40);
    assert_eq!(site_view.public.construction, Some(progress.done));
    let miner_view = view.entity(canopy_sim::entity::EntityId(miner)).unwrap();
    assert_eq!(
        miner_view
            .work
            .as_ref()
            .unwrap()
            .cargo
            .as_ref()
            .map(|c| c.amount),
        Some(5)
    );
}
