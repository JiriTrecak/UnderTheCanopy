//! Map pings: the pinging player's team sees them for four seconds, rivals never.

mod economy_fixture;

use economy_fixture::*;
use serde_json::json;

#[test]
fn a_ping_shows_to_its_team_for_four_seconds() {
    let mut world = world(&sandbox(vec![place("hall", HALL, 19.5, 19.5)]));
    command(
        &mut world,
        json!({"type": "ping", "position": {"x": 40, "y": 12}}),
    );
    world.step();
    let mine = world.view(P1).pings;
    assert_eq!(
        mine.iter().map(|p| (p.owner, p.x, p.y)).collect::<Vec<_>>(),
        [(P1, 40, 12)]
    );
    assert!(world.view(P2).pings.is_empty(), "rivals do not see it");
    assert_eq!(world.observer_view().pings.len(), 1);
    run(&mut world, canopy_sim::state::PING_TICKS);
    assert!(world.view(P1).pings.is_empty(), "gone after four seconds");
    // A ping without a position (older games) marks nothing.
    command(&mut world, json!({"type": "ping"}));
    world.step();
    assert!(world.view(P1).pings.is_empty());
}
