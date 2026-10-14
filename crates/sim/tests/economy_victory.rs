//! Defeat and victory in skirmish: a colony without buildings is out and its units leave;
//! the match ends when one team remains.

mod economy_fixture;

use canopy_sim::{MatchSetup, World, entity::EntityId, state::FactKind};
use economy_fixture::*;
use serde_json::json;

fn halls(world: &World, owner: canopy_content::placement::Owner) -> Vec<u32> {
    all_of(world, owner, HALL)
}

#[test]
fn a_colony_without_buildings_is_defeated_and_the_last_team_wins() {
    let mut world = world(&skirmish(vec![]));
    run(&mut world, 5);
    assert!(world.state().outcome.is_none());
    for hall in halls(&world, P2) {
        world.remove_entity(EntityId(hall));
    }
    world.step();
    let outcome = world.state().outcome.clone().expect("decided");
    assert_eq!((outcome.winner, outcome.defeated), (Some(P1), vec![P2]));
    assert!(
        world.state().entities.iter().all(|e| e.owner != P2),
        "the defeated colony leaves"
    );
    assert!(
        world
            .state()
            .facts
            .iter()
            .any(|f| f.owner == P2 && f.kind == FactKind::Death && f.message == "Colony defeated")
    );
    let tick = world.state().tick;
    let fingerprint = world.fingerprint();
    run(&mut world, 3);
    assert_eq!(world.state().tick, tick + 3, "time runs on");
    let _ = fingerprint;
    let worker = all_of(&world, P1, WORKER)[0];
    assert_eq!(
        try_command(&mut world, json!({"type": "stop", "actors": [worker]})),
        Some("Match has ended".into())
    );
}

#[test]
fn an_unfinished_site_keeps_a_colony_alive() {
    let mut world = world(&skirmish(vec![]));
    fund(&mut world, P2, AMBER, 100);
    fund(&mut world, P2, WOOD, 100);
    let worker = all_of(&world, P2, WORKER)[2];
    command_for(
        &mut world,
        1,
        json!({"type": "build", "actors": [worker], "definition": HOUSE,
        "position": {"x": 193.5, "y": 193.5}}),
    );
    world.step();
    assert_eq!(all_of(&world, P2, HOUSE).len(), 1, "{:?}", errors(&world));
    for hall in halls(&world, P2) {
        world.remove_entity(EntityId(hall));
    }
    run(&mut world, 3);
    assert!(world.state().outcome.is_none());
    assert!(!all_of(&world, P2, WORKER).is_empty());
}

#[test]
fn allies_win_together_and_a_defeated_player_is_refused() {
    let map = skirmish(vec![
        place_for("third-hall", HALL, "player.3", 123.5, 123.5),
        place_for("third-worker", WORKER, "player.3", 130.0, 130.0),
    ]);
    let mut setup: MatchSetup = setup(3);
    setup.slots[0].team = Some(1);
    setup.slots[1].team = Some(1);
    let mut world = World::new(content(), &map, setup).unwrap_or_else(|e| panic!("{e}"));
    let p3 = canopy_content::placement::Owner::Player(2);
    world.step();
    for hall in halls(&world, P2) {
        world.remove_entity(EntityId(hall));
    }
    world.step();
    assert!(world.state().outcome.is_none(), "two teams remain");
    let worker = all_of(&world, P1, WORKER)[0];
    command_for(&mut world, 1, json!({"type": "stop", "actors": [worker]}));
    world.step();
    assert!(
        world
            .state()
            .facts
            .iter()
            .any(|f| f.owner == P2 && f.message == "Your colony has been defeated")
    );
    for hall in halls(&world, p3) {
        world.remove_entity(EntityId(hall));
    }
    world.step();
    let outcome = world.state().outcome.clone().expect("decided");
    assert_eq!(
        outcome.winner,
        Some(P1),
        "the first remaining player of the winning team"
    );
    assert_eq!(outcome.defeated, vec![P2, p3]);
}
