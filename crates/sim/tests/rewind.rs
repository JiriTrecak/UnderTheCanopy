//! `World::rewind` (replays, D60): a world moved to a save of another moment of its match,
//! back or ahead, plays on exactly as a world restored from that save.

mod ai_fixture;

use ai_fixture::*;
use canopy_sim::World;

/// Steps both worlds and requires the same fingerprints and, at the end, the same saves.
fn same_from_here(a: &mut World, b: &mut World, ticks: u32) {
    for _ in 0..ticks {
        a.step();
        b.step();
        assert_eq!(a.fingerprint(), b.fingerprint(), "at tick {}", a.tick());
    }
    assert!(a.save() == b.save(), "the same state after {ticks} ticks");
}

#[test]
fn a_rewound_world_plays_on_as_a_restored_one() {
    let map = skirmish(vec![], vec![]);
    let mut world = world(&map, true);
    run(&mut world, 700);
    let early = world.save();
    run(&mut world, 900);
    let late = world.save();

    // Back in time.
    let mut restored = World::restore(content(), &map, early.clone()).unwrap();
    world.rewind(early.clone()).unwrap();
    assert_eq!(world.tick(), restored.tick());
    same_from_here(&mut world, &mut restored, 600);

    // Ahead again, past what it last simulated.
    let mut restored = World::restore(content(), &map, late.clone()).unwrap();
    world.rewind(late).unwrap();
    same_from_here(&mut world, &mut restored, 600);

    // A save of another match is refused.
    let mut other = world_for(&map, 9);
    assert!(other.rewind(early).is_err());
}

fn world_for(map: &canopy_content::map::UtcMap, seed: u32) -> World {
    let mut setup = setup(true);
    setup.seed = seed;
    World::new(content(), map, setup).unwrap()
}
