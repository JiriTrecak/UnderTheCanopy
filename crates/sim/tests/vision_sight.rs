//! What sensors see: radius, height filtering, flight, following moving units, allies and
//! the starting exploration. Test-owned content and testbeds only.

mod vision_fixture;

use canopy_content::placement::Owner;
use canopy_sim::{
    entity::Milli,
    teams::Relation,
    vision::{EXPLORED, UNEXPLORED, VISIBLE},
};
use vision_fixture::*;

const P0: Owner = Owner::Player(0);
const P1: Owner = Owner::Player(1);
const P2: Owner = Owner::Player(2);

/// Ground 5 m high from x = 110 on.
fn plateau(x: i32, _: i32) -> f32 {
    if x >= 110 { 5.0 } else { 0.0 }
}

#[test]
fn sight_is_a_circle_of_the_vision_radius() {
    let world = start(
        flat(vec![place("scout", "unit.scout", "player.1", 100.0, 100.0)]),
        &[None],
    );
    let seen = sight_cells(&world, 0);
    // Lattice points within 6 cells of the centre.
    assert_eq!(seen.len(), 113);
    assert!(visible(&world, 0, 106, 100) && visible(&world, 0, 100, 94));
    assert!(visible(&world, 0, 104, 104) && !visible(&world, 0, 105, 104));
    assert_eq!(fog(&world, 0, 107, 100), UNEXPLORED);
    verify(&world);
}

#[test]
fn higher_ground_hides_unless_the_eye_is_raised() {
    let world = start(
        testbed(
            vec![
                place("low", "unit.scout", "player.1", 106.0, 100.0),
                place("flyer", "unit.flyer", "player.2", 106.0, 110.0),
                place("high", "unit.scout", "player.3", 114.0, 120.0),
            ],
            plateau,
            true,
        ),
        &[None, None, None],
    );
    // A ground scout sees up to 2 m above itself: not the plateau beside it.
    assert!(visible(&world, 0, 109, 100));
    assert!(!visible(&world, 0, 110, 100) && !visible(&world, 0, 112, 100));
    // Flying 6 m up, the plateau is in view.
    assert!(visible(&world, 1, 110, 110));
    // From the plateau, lower ground is visible.
    assert!(visible(&world, 2, 108, 120) && visible(&world, 2, 112, 120));
    verify(&world);
}

#[test]
fn vision_follows_a_moving_unit() {
    let document = flat(vec![
        place("scout", "unit.scout", "player.1", 50.0, 50.0),
        place("mole", "unit.mole", "player.2", 75.0, 50.0),
    ]);
    let mut world = start(document, &[None, None]);
    let scout = id(&world, "scout");
    let mole = id(&world, "mole");
    let mut seen_mole = Vec::new();
    for step in 1..=40 {
        // Half a cell per tick, as movement would.
        put(
            &mut world,
            scout,
            Milli {
                x: 50_000 + step * 500,
                y: 50_000,
            },
        );
        world.step();
        let x = (50_000 + step * 500 + 500) / 1000;
        assert!(visible(&world, 0, x + 6, 50), "tick {step}: ahead");
        assert!(visible(&world, 0, x - 6, 50), "tick {step}: behind");
        assert_eq!(
            fog(&world, 0, x - 7, 50),
            EXPLORED,
            "tick {step}: left behind"
        );
        assert_eq!(
            fog(&world, 0, x + 7, 50),
            UNEXPLORED,
            "tick {step}: not yet seen"
        );
        let view = world.view(P0);
        if view.entity(mole).is_some() {
            seen_mole.push(x);
        }
        verify(&world);
    }
    // The mole at x = 75 is seen exactly while the scout's cell is within 6 of it.
    seen_mole.dedup();
    assert_eq!(seen_mole, [69, 70]);
    // Walking back out of range hides it again.
    put(&mut world, scout, milli(60, 50));
    world.step();
    assert!(world.view(P0).entity(mole).is_none());
    assert_eq!(fog(&world, 0, 75, 50), EXPLORED);
    verify(&world);
}

#[test]
fn allies_share_sight_and_enemies_do_not() {
    let mut world = start(
        flat(vec![
            place("ally scout", "unit.scout", "player.2", 80.0, 80.0),
            place("enemy", "unit.mole", "player.3", 83.0, 80.0),
            place("far", "unit.mole", "player.3", 120.0, 80.0),
        ]),
        &[Some(5), Some(5), None],
    );
    let scout = id(&world, "ally scout");
    let enemy = id(&world, "enemy");
    assert_eq!(sight_cells(&world, 0), sight_cells(&world, 1));
    assert!(visible(&world, 0, 80, 80) && !visible(&world, 2, 80, 80));
    let view = world.view(P0);
    let ally = view.entity(scout).expect("ally scout");
    assert_eq!(ally.relation, Relation::Ally);
    assert!(ally.private.is_none());
    assert_eq!(view.entity(enemy).expect("enemy").relation, Relation::Enemy);
    assert!(view.entity(id(&world, "far")).is_none());
    // The enemy sees the scout only where its own units look; the mole has no sight.
    assert!(world.view(P2).entity(scout).is_none());
    assert!(world.view(P1).entity(scout).unwrap().private.is_some());
    // Exact sight agrees.
    let sight = world.sight();
    let e = world.state().entities.get(enemy).unwrap();
    assert!(sight.visible(P0, e) && sight.visible(P1, e));
    put(&mut world, scout, milli(30, 30));
    world.step();
    let sight = world.sight();
    let e = world.state().entities.get(enemy).unwrap();
    assert!(!sight.visible(P0, e) && !sight.seen(P1, e));
    verify(&world);
}

#[test]
fn explored_maps_start_surveyed_and_hidden_maps_dark() {
    let entities = || {
        vec![
            place("scout", "unit.scout", "player.1", 30.0, 30.0),
            place("tree.near", "resource.tree", "none", 33.0, 30.0),
            place("tree.far", "resource.tree", "none", 200.0, 200.0),
            place("enemy", "unit.mole", "player.2", 200.0, 190.0),
        ]
    };
    let explored = start(testbed(entities(), |_, _| 0.0, false), &[None, None]);
    let hidden = start(testbed(entities(), |_, _| 0.0, true), &[None, None]);
    let cells = |world: &canopy_sim::World| world.sight().cells(P0).unwrap().0.clone();
    assert!(
        cells(&explored)
            .iter()
            .all(|&c| c == EXPLORED || c == VISIBLE)
    );
    assert!(
        cells(&hidden)
            .iter()
            .all(|&c| c == UNEXPLORED || c == VISIBLE)
    );
    assert_eq!(sight_cells(&explored, 0), sight_cells(&hidden, 0));
    let trees = |world: &canopy_sim::World, owner| -> Vec<String> {
        world
            .view(owner)
            .scenery
            .iter()
            .map(|s| {
                world.catalog().id(s.definition).to_string() + &format!("@{}", s.position.x / 1000)
            })
            .collect()
    };
    // Explored: both players know every tree (in id order: placements are created in
    // placement-id order), but not the enemy's units.
    assert_eq!(
        trees(&explored, P0),
        ["resource.tree@200", "resource.tree@33"]
    );
    assert_eq!(
        trees(&explored, P1),
        ["resource.tree@200", "resource.tree@33"]
    );
    // Hidden: only what is in sight.
    assert_eq!(trees(&hidden, P0), ["resource.tree@33"]);
    assert!(trees(&hidden, P1).is_empty());
    assert!(explored.view(P0).entity(id(&explored, "enemy")).is_none());
    // Observers see everything.
    let all = hidden.observer_view();
    assert_eq!(all.scenery.len(), 2);
    assert_eq!(all.entities.len(), 2);
    assert!(all.fog.is_none());
    verify(&explored);
    verify(&hidden);
}
