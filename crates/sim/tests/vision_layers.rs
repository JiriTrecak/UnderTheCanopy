//! Fog on maps with bridge decks: deck nodes have their own fog after the ground cells,
//! and units on a deck are seen through their deck node.

mod vision_fixture;

use canopy_content::placement::Owner;
use canopy_sim::{World, entity::Milli};
use serde_json::{Value, json};
use vision_fixture::*;

const P0: Owner = Owner::Player(0);

fn bridged(entities: Vec<Value>) -> Value {
    let mut document = flat(entities);
    document["stamps"] = json!([{"id": "bridge.1", "asset": "scenery.asset.bridge-prop", "x": 120, "y": 120, "elevation": 1}]);
    document
}

fn deck_fog(world: &World, player: u8, x: i64, y: i64) -> u8 {
    let terrain = world.terrain();
    let deck = terrain.deck_id("bridge.1").unwrap();
    let node = terrain
        .surfaces()
        .unwrap()
        .node_at(x, y, Some(deck))
        .unwrap();
    world
        .sight()
        .cells(Owner::Player(player))
        .unwrap()
        .get(node)
}

#[test]
fn units_on_a_deck_are_seen_by_the_deck_not_the_ground_below() {
    let mut world = start(
        bridged(vec![
            place("scout", "unit.scout", "player.1", 114.0, 120.0),
            place_on("walker", "unit.mole", "player.2", 120.0, 120.0, "bridge.1"),
            place("under", "unit.mole", "player.2", 120.0, 121.0),
        ]),
        &[None, None],
    );
    let nodes = world.vision().nodes();
    assert!(nodes > (SIZE * SIZE) as usize);
    assert_eq!(world.view(P0).fog.unwrap().cells.len(), nodes);
    // The deck hides the ground beneath it but is itself in sight.
    assert_eq!(deck_fog(&world, 0, 120, 120), 2);
    assert_eq!(fog(&world, 0, 120, 120), 0);
    let view = world.view(P0);
    assert!(view.entity(id(&world, "walker")).is_some(), "on the deck");
    assert!(view.entity(id(&world, "under")).is_none(), "under the deck");
    let fog_view = view.fog.unwrap();
    let deck = fog_view
        .deck_cells
        .iter()
        .position(|&c| c == cell(120, 120))
        .unwrap();
    assert_eq!(fog_view.cells[(SIZE * SIZE) as usize + deck], 2);
    assert_eq!(fog_view.flat(cell(120, 120)), 2);
    verify(&world);
    let scout = id(&world, "scout");
    put(&mut world, scout, milli(100, 120));
    world.step();
    assert!(world.view(P0).entity(id(&world, "walker")).is_none());
    verify(&world);
}

#[test]
fn sensors_crossing_a_bridge_keep_coverage_exact() {
    let document = bridged(vec![
        place_on(
            "deck scout",
            "unit.scout",
            "player.1",
            120.0,
            116.0,
            "bridge.1",
        ),
        place("ground scout", "unit.scout", "player.2", 100.0, 120.0),
        place("flyer", "unit.flyer", "player.2", 110.0, 110.0),
    ]);
    let mut world = start(document.clone(), &[None, None]);
    let (deck, ground, flyer) = (
        id(&world, "deck scout"),
        id(&world, "ground scout"),
        id(&world, "flyer"),
    );
    for step in 0..40 {
        put(
            &mut world,
            deck,
            Milli {
                x: 120_000,
                y: 116_000 + (step % 9) * 1000,
            },
        );
        put(
            &mut world,
            ground,
            Milli {
                x: 100_000 + step * 900,
                y: 120_000,
            },
        );
        put(
            &mut world,
            flyer,
            Milli {
                x: 110_000 + step * 400,
                y: 110_000 + step * 300,
            },
        );
        world.step();
        verify(&world);
    }
    let restored = reload_on(&world, document);
    verify(&restored);
    assert_eq!(restored.state(), world.state());
}
