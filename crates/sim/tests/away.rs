//! Players who leave a networked match (D107): the server's `away` and `back` commands.
//! A human ally commands the away colony and pays from its stock; without a human ally
//! playing on, the away player is defeated. Test-owned content only.

mod economy_fixture;

use canopy_content::{map::UtcMap, placement::Owner};
use canopy_sim::{MatchSetup, Slot, SlotKind, World};
use economy_fixture::*;
use serde_json::json;

const P3: Owner = Owner::Player(2);

/// Three starts: players 1 and 2 on one team (or as `teams` says), player 3 against them.
fn three(teams: [Option<u8>; 3], kinds: [SlotKind; 3]) -> World {
    let document = json!({
        "v": 3, "size": SIZE, "name": "Away", "stamps": [],
        "initialExploration": "explored",
        "playerStarts": [
            {"player": 1, "x": 43.5, "z": 43.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
            {"player": 2, "x": 43.5, "z": 203.5, "setup": "setup.test", "mainFort": "start.player.2/main-fort"},
            {"player": 3, "x": 203.5, "z": 203.5, "setup": "setup.test", "mainFort": "start.player.3/main-fort"},
        ],
        "entities": [], "camps": [],
        "authoring": {"version": 1, "terrain": flat(), "objects": []},
    });
    let map = UtcMap::from_value(document, "away.utcmap").unwrap_or_else(|e| panic!("{e}"));
    let setup = MatchSetup {
        seed: 3,
        slots: (0..3)
            .map(|player| Slot {
                difficulty: Default::default(),
                player,
                kind: kinds[usize::from(player)],
                name: None,
                team: teams[usize::from(player)],
                hero: None,
                race: None,
            })
            .collect(),
    };
    World::new(content(), &map, setup).unwrap_or_else(|e| panic!("{e}"))
}

fn errors_of(world: &World, owner: Owner, since: u32) -> Vec<String> {
    world
        .state()
        .facts
        .iter()
        .filter(|f| {
            f.id >= since && f.owner == owner && f.kind == canopy_sim::state::FactKind::Error
        })
        .map(|f| f.message.clone())
        .collect()
}

#[test]
fn an_ally_commands_an_away_colony_and_pays_from_its_stock_until_it_returns() {
    let human = SlotKind::Human;
    let mut world = three([Some(1), Some(1), Some(2)], [human; 3]);
    world.step();
    let worker = all_of(&world, P2, WORKER)[0];
    let hall = all_of(&world, P2, HALL)[0];
    let before = get(&world, worker).position;

    // Before player 2 leaves, its units are not player 1's to command.
    let since = world.state().counters.fact;
    command_for(&mut world, 0, json!({"type": "stop", "actors": [worker]}));
    world.step();
    assert_eq!(errors_of(&world, P1, since), ["message.not-your-actors"]);

    command_for(&mut world, 1, json!({"type": "away"}));
    world.step();
    assert!(world.state().away.contains(&1));
    let view = world.view(P1);
    assert_eq!(view.away, [P2]);
    assert_eq!(view.commanded.iter().map(|c| c.0).collect::<Vec<_>>(), [P2]);
    assert!(!view.defeated.contains(&P2), "an ally plays on");
    assert!(
        world.view(P3).commanded.is_empty(),
        "enemies command nothing"
    );

    // Player 1 moves player 2's worker together with its own.
    let own = all_of(&world, P1, WORKER)[0];
    let since = world.state().counters.fact;
    command_for(
        &mut world,
        0,
        json!({"type": "move", "actors": [own, worker], "destination": {"x": 60, "y": 190}}),
    );
    run(&mut world, 80);
    assert!(errors_of(&world, P1, since).is_empty());
    assert_ne!(
        get(&world, worker).position,
        before,
        "the ally's worker walked"
    );

    // Training at player 2's hall is paid from player 2's stock.
    fund(&mut world, P1, AMBER, 500);
    fund(&mut world, P2, AMBER, 100);
    command_for(
        &mut world,
        0,
        json!({"type": "produce", "actor": hall, "definition": WORKER}),
    );
    world.step();
    assert_eq!(wallet(&world, P2, AMBER), 90);
    assert_eq!(wallet(&world, P1, AMBER), 500);

    // Back: the colony is player 2's alone again.
    command_for(&mut world, 1, json!({"type": "back"}));
    world.step();
    assert!(world.state().away.is_empty());
    assert!(world.view(P1).commanded.is_empty());
    let since = world.state().counters.fact;
    command_for(&mut world, 0, json!({"type": "stop", "actors": [worker]}));
    world.step();
    assert_eq!(errors_of(&world, P1, since), ["message.not-your-actors"]);
}

#[test]
fn a_player_away_without_a_human_ally_is_defeated() {
    // One against one: the one who stays wins.
    let mut world = World::new(content(), &skirmish(Vec::new()), setup(2)).unwrap();
    world.step();
    command_for(&mut world, 1, json!({"type": "away"}));
    world.step();
    let outcome = world.state().outcome.clone().expect("the match ended");
    assert_eq!(outcome.winner, Some(P1));
    assert_eq!(outcome.defeated, [P2]);

    // A computer ally does not hold the colony: the away player is out, the team plays on.
    let mut world = three(
        [Some(1), Some(1), Some(2)],
        [SlotKind::Human, SlotKind::Ai, SlotKind::Human],
    );
    world.step();
    command_for(&mut world, 0, json!({"type": "away"}));
    world.step();
    assert!(world.view(P2).defeated.contains(&P1));
    assert!(
        all_of(&world, P1, WORKER).is_empty(),
        "a defeated colony leaves"
    );
    assert!(
        world.state().outcome.is_none(),
        "the computer ally plays on"
    );

    // Allies who all leave are all defeated.
    let mut world = three([Some(1), Some(1), Some(2)], [SlotKind::Human; 3]);
    world.step();
    command_for(&mut world, 0, json!({"type": "away"}));
    command_for(&mut world, 1, json!({"type": "away"}));
    world.step();
    assert_eq!(
        world.state().outcome.as_ref().and_then(|o| o.winner),
        Some(P3)
    );
}
