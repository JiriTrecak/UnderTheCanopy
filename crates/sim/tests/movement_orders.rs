//! Move, stop, hold, patrol, follow and queued orders on test-owned content and maps.

mod movement_fixture;

use movement_fixture::*;
use serde_json::json;

#[test]
fn a_walker_crosses_open_ground_and_stops_on_the_cell() {
    let map = map(vec![unit("a", "unit.walker", 20, 20)], flat(), vec![]);
    let mut world = world(&map);
    let a = id(&world, "a");
    command(&mut world, json!({"type": "move", "actors": [a], "destination": {"x": 60, "y": 30}}));
    run(&mut world, 1);
    assert!(entity(&world, "a").unit.as_ref().unwrap().order.is_some());
    // Speed 4 cells/s: 41.2 cells take a little over 10 s.
    run(&mut world, 460);
    assert_eq!(at(&world, "a"), (60.0, 30.0));
    let u = entity(&world, "a").unit.as_ref().unwrap();
    assert!(u.order.is_none() && u.route.is_empty(), "{u:?}");
    assert!(errors(&world).is_empty(), "{:?}", errors(&world));
}
