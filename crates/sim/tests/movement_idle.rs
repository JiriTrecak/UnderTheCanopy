//! Idle strolls, separating stacked bodies and placing released units.

mod movement_fixture;

use canopy_sim::entity::{Milli, Place};
use movement_fixture::*;

#[test]
fn idle_workers_stroll_around_home_at_walking_speed() {
    let map = map(
        vec![
            unit("w", "unit.wanderer", 50, 50),
            unit("s", "unit.walker", 60, 50),
        ],
        flat(),
        vec![],
    );
    let mut world = world(&map);
    let mut positions = std::collections::BTreeSet::new();
    let mut fastest = 0.0f64;
    let mut previous = at(&world, "w");
    for _ in 0..2400 {
        run(&mut world, 1);
        let p = at(&world, "w");
        assert!(
            (p.0 - 50.0).abs() <= 1.0 && (p.1 - 50.0).abs() <= 1.0,
            "strayed to {p:?}"
        );
        fastest = fastest.max(distance(previous, p));
        previous = p;
        positions.insert(((p.0 * 1000.0) as i64, (p.1 * 1000.0) as i64));
    }
    assert!(positions.len() > 10, "it strolled");
    // Walk speed 2 cells/s: 50 milli-cells per tick (a little more on a rounded diagonal).
    assert!((0.05..0.052).contains(&fastest), "{fastest}");
    // Units that may not wander stay put.
    assert_eq!(at(&world, "s"), (60.0, 50.0));
}

#[test]
fn stacked_bodies_are_pushed_apart() {
    let map = map(
        vec![
            unit("a", "unit.walker", 50, 50),
            unit("b", "unit.walker", 52, 50),
        ],
        flat(),
        vec![],
    );
    let mut world = world(&map);
    let b = canopy_sim::entity::EntityId(id(&world, "b"));
    world.state_mut().entities.get_mut(b).unwrap().position = Milli {
        x: 50_200,
        y: 50_000,
    };
    run(&mut world, 60);
    assert!(
        distance(at(&world, "a"), at(&world, "b")) >= 0.8,
        "{:?} {:?}",
        at(&world, "a"),
        at(&world, "b")
    );
}

#[test]
fn released_units_appear_on_the_nearest_free_cell() {
    let map = map(
        vec![
            unit("a", "unit.walker", 50, 50),
            unit("b", "unit.walker", 20, 20),
        ],
        flat(),
        vec![],
    );
    let mut world = world(&map);
    let b = canopy_sim::entity::EntityId(id(&world, "b"));
    world
        .state_mut()
        .entities
        .get_mut(b)
        .unwrap()
        .unit
        .as_mut()
        .unwrap()
        .release = Some(Place::new(50, 50));
    run(&mut world, 1);
    let e = entity(&world, "b");
    assert!(e.unit.as_ref().unwrap().release.is_none());
    // (50, 50) is taken by `a`; the first free cell of the diamond is north of it.
    assert_eq!(at(&world, "b"), (50.0, 49.0));
}
