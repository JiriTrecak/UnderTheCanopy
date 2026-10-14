//! A busy two-player economy runs identically twice and across a save and restore taken
//! mid-job; derived indexes rebuilt on load match the incremental ones.

mod economy_fixture;

use canopy_sim::World;
use economy_fixture::*;
use serde_json::json;

fn map() -> canopy_content::map::UtcMap {
    let mut extra = Vec::new();
    for (i, (x, y)) in [
        (36.0, 46.0),
        (35.0, 49.0),
        (33.0, 47.0),
        (212.0, 206.0),
        (213.0, 209.0),
    ]
    .into_iter()
    .enumerate()
    {
        extra.push(resource(&format!("tree-{i}"), TREE, x, y));
    }
    skirmish(extra)
}

/// Commands issued at fixed ticks: wood, a house, training and a rally.
fn script(world: &mut World, t: u32) {
    let p1 = all_of(world, P1, WORKER);
    let p2 = all_of(world, P2, WORKER);
    let hall1 = all_of(world, P1, HALL)[0];
    let hall2 = all_of(world, P2, HALL)[0];
    let tree = id(world, "tree-0");
    match t {
        2 => {
            command(
                world,
                json!({"type": "gather", "actors": [p1[2]], "target": tree}),
            );
            command_for(
                world,
                1,
                json!({"type": "produce", "actor": hall2, "definition": WORKER}),
            );
            command_for(
                world,
                1,
                json!({"type": "rally", "actor": hall2, "destination": null, "target": id(world, "tree-3")}),
            );
        }
        40 => command(
            world,
            json!({"type": "build", "actors": [p1[0]], "definition": HOUSE,
            "position": {"x": 57.5, "y": 41.5}}),
        ),
        90 => command(
            world,
            json!({"type": "produce", "actor": hall1, "definition": WORKER}),
        ),
        200 => command_for(
            world,
            1,
            json!({"type": "move", "actors": [p2[0]], "destination": {"x": 190, "y": 190}}),
        ),
        _ => {}
    }
}

fn run_scripted(world: &mut World, from: u32, to: u32) -> Vec<u32> {
    (from..to)
        .map(|t| {
            script(world, t);
            world.step();
            world.fingerprint()
        })
        .collect()
}

#[test]
fn the_economy_is_deterministic_and_survives_save_and_restore() {
    let map = map();
    let mut a = world(&map);
    let mut b = world(&map);
    let first = run_scripted(&mut a, 0, 900);
    let second = run_scripted(&mut b, 0, 900);
    assert_eq!(first, second);
    assert_eq!(a.state(), b.state());
    assert!(
        !errors(&a)
            .iter()
            .any(|e| e != "Move destination is unreachable"),
        "{:?}",
        errors(&a)
    );
    assert!(all_of(&a, P1, HOUSE).len() == 1 && all_of(&a, P1, WORKER).len() == 4);
    assert!(wallet(&a, P1, WOOD) > 50);

    let mut original = world(&map);
    run_scripted(&mut original, 0, 450);
    assert!(!original.state().economy.jobs.is_empty(), "saved mid-job");
    let save = original.save();
    let json = serde_json::to_string(&save).unwrap();
    let mut restored = World::restore(content(), &map, serde_json::from_str(&json).unwrap())
        .unwrap_or_else(|e| panic!("{e}"));
    restored.verify_economy().unwrap();
    let after_original = run_scripted(&mut original, 450, 900);
    let after_restored = run_scripted(&mut restored, 450, 900);
    assert_eq!(after_original, after_restored);
    assert_eq!(original.state(), restored.state());
    assert_eq!(&first[450..], &after_original[..]);
    original.verify_economy().unwrap();
    restored.verify_economy().unwrap();
}
