//! What players remember: fog transitions, buildings and scenery under fog, views that
//! hide what is out of sight, fog revisions, deaths and temporary sight. Test-owned
//! content and testbeds only.

mod vision_fixture;

use canopy_content::placement::Owner;
use canopy_sim::{
    catalog::Catalog,
    view::Relation,
    vision::{self, Current, EXPLORED, SpellVision, UNEXPLORED, VISIBLE},
};
use vision_fixture::*;

const P0: Owner = Owner::Player(0);
const P1: Owner = Owner::Player(1);

#[test]
fn fog_goes_from_unexplored_to_visible_to_explored() {
    let mut world = start(
        flat(vec![place("scout", "unit.scout", "player.1", 40.0, 60.0)]),
        &[None],
    );
    let scout = id(&world, "scout");
    assert_eq!(fog(&world, 0, 60, 60), UNEXPLORED);
    put(&mut world, scout, milli(58, 60));
    world.step();
    assert_eq!(fog(&world, 0, 60, 60), VISIBLE);
    put(&mut world, scout, milli(80, 60));
    world.step();
    assert_eq!(fog(&world, 0, 60, 60), EXPLORED);
    assert_eq!(fog(&world, 0, 40, 60), EXPLORED);
    verify(&world);
}

#[test]
fn buildings_are_remembered_as_last_seen() {
    let mut world = start(
        flat(vec![
            place("scout", "unit.scout", "player.1", 100.0, 121.0),
            place("tower", "building.tower", "player.2", 121.5, 121.5),
        ]),
        &[None, None],
    );
    let (scout, tower) = (id(&world, "scout"), id(&world, "tower"));
    assert!(world.view(P0).entity(tower).is_none());
    // Its footprint's nearest cell (120) comes within sight.
    put(&mut world, scout, milli(114, 121));
    world.step();
    let seen = world
        .view(P0)
        .entity(tower)
        .cloned()
        .expect("tower in sight");
    assert!(!seen.remembered && seen.relation == Relation::Enemy);
    assert_eq!(seen.public.hp, Some(100));
    // Out of sight it stays, as it was.
    put(&mut world, scout, milli(100, 121));
    world.step();
    world.state_mut().entities.get_mut(tower).unwrap().hp = Some(50);
    world.step();
    let remembered = world.view(P0).entity(tower).cloned().expect("remembered");
    assert!(remembered.remembered);
    assert_eq!(remembered.public.hp, Some(100));
    verify(&world);
    // Seen again: updated.
    put(&mut world, scout, milli(114, 121));
    world.step();
    assert_eq!(world.view(P0).entity(tower).unwrap().public.hp, Some(50));
    // Destroyed out of sight: still remembered until its place is seen.
    put(&mut world, scout, milli(100, 121));
    world.step();
    world.state_mut().entities.remove(tower);
    world.step();
    assert!(world.view(P0).entity(tower).is_some_and(|e| e.remembered));
    verify(&world);
    put(&mut world, scout, milli(115, 121));
    world.step();
    assert!(world.view(P0).entity(tower).is_none());
    assert!(world.sight().remembered(P0, tower).is_none());
    verify(&world);
}

#[test]
fn scenery_changes_are_learned_only_by_sight() {
    let mut world = start(
        testbed(
            vec![
                place("scout", "unit.scout", "player.1", 20.0, 20.0),
                place("tree", "resource.tree", "none", 60.0, 20.0),
            ],
            |_, _| 0.0,
            false,
        ),
        &[None],
    );
    let (scout, tree) = (id(&world, "scout"), id(&world, "tree"));
    let amount = |world: &canopy_sim::World| {
        world
            .view(P0)
            .scenery
            .iter()
            .find(|s| s.id == tree)
            .map(|s| s.resource.as_ref().unwrap().amount)
    };
    assert_eq!(amount(&world), Some(10));
    world
        .state_mut()
        .entities
        .get_mut(tree)
        .unwrap()
        .resource
        .as_mut()
        .unwrap()
        .amount = 4;
    world.step();
    assert_eq!(amount(&world), Some(10), "harvested out of sight");
    assert_eq!(
        world
            .observer_view()
            .scenery
            .iter()
            .next()
            .unwrap()
            .resource
            .as_ref()
            .unwrap()
            .amount,
        4
    );
    let before = world.view(P0).scenery.clone();
    put(&mut world, scout, milli(56, 20));
    world.step();
    assert_eq!(amount(&world), Some(4));
    // Only the chunk holding the tree was replaced.
    let after = world.view(P0).scenery;
    assert!(after.revision > before.revision);
    // Felled while watched: gone at once.
    world.state_mut().entities.remove(tree);
    world.step();
    assert_eq!(amount(&world), None);
    verify(&world);
}

#[test]
fn views_show_only_what_is_in_sight_and_fog_patches_add_up() {
    let mut world = start(
        flat(vec![
            place("scout", "unit.scout", "player.1", 50.0, 50.0),
            place("mine", "unit.mole", "player.1", 200.0, 200.0),
            place("enemy", "unit.mole", "player.2", 54.0, 50.0),
            place("hidden", "unit.mole", "player.2", 84.0, 50.0),
        ]),
        &[None, None],
    );
    let scout = id(&world, "scout");
    let view = world.view(P0);
    let ids: Vec<u32> = view.entities.iter().map(|e| e.id().0).collect();
    let expected: Vec<u32> = ["enemy", "mine", "scout"]
        .map(|p| id(&world, p).0)
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(ids, expected, "own units anywhere, enemies in sight");
    assert!(view.entity(id(&world, "mine")).unwrap().private.is_some());
    assert!(view.entity(id(&world, "enemy")).unwrap().private.is_none());
    // The enemy's view: its own units only (they have no sight).
    let theirs = world.view(P1);
    assert_eq!(theirs.entities.len(), 2);
    assert!(
        theirs
            .fog
            .as_ref()
            .unwrap()
            .cells
            .iter()
            .all(|&c| c == UNEXPLORED)
    );
    // A renderer patches its copy of the fog from the revisions it missed.
    let mut copy = view.fog.clone().unwrap();
    let mut cells = copy.cells.as_ref().clone();
    for step in 1..=30 {
        put(&mut world, scout, milli(50 + step, 50));
        world.step();
        if step % 7 == 0 {
            let fog = world.view(P0).fog.unwrap();
            for node in fog.changes_since(copy.revision).expect("recent revision") {
                cells[node as usize] = fog.cells[node as usize];
            }
            assert_eq!(&cells, fog.cells.as_ref());
            copy = fog;
        }
    }
    assert!(world.view(P0).entity(id(&world, "hidden")).is_some());
    // Unchanged fog is shared, not copied.
    let (a, b) = (world.view(P0).fog.unwrap(), world.view(P0).fog.unwrap());
    assert!(std::sync::Arc::ptr_eq(&a.cells, &b.cells));
    assert_eq!(a.changes_since(a.revision), Some(Vec::new()));
    assert_eq!(a.changes_since(0), None);
}

#[test]
fn deaths_are_witnessed_by_those_who_saw_them() {
    let mut world = start(
        flat(vec![
            place("scout", "unit.scout", "player.1", 50.0, 50.0),
            place("near", "unit.mole", "player.2", 53.0, 50.0),
            place("far", "unit.mole", "player.2", 150.0, 50.0),
        ]),
        &[None, None],
    );
    let near = id(&world, "near");
    let catalog = Catalog::new(content());
    let terrain = world.terrain().clone();
    let vision = world.vision().clone();
    for victim in ["near", "far"] {
        let victim = id(&world, victim);
        vision.record_death(world.state_mut(), &catalog, &terrain, &Current, victim);
        world.state_mut().entities.remove(victim);
    }
    world.step();
    let cues = world.view(P0).deaths;
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].sighting.id, near);
    assert_eq!(
        world.view(P1).deaths.len(),
        2,
        "the owner sees its own losses"
    );
    assert_eq!(world.sight().observed_deaths(P0).len(), 1);
    for _ in 0..81 {
        world.step();
    }
    assert!(world.view(P0).deaths.is_empty(), "cues fade");
    assert_eq!(
        world.sight().observed_deaths(P0).len(),
        1,
        "reports stay longer"
    );
    for _ in 0..320 {
        world.step();
    }
    assert!(world.sight().observed_deaths(P0).is_empty());
    verify(&world);
}

#[test]
fn spell_vision_reveals_for_a_while() {
    let mut world = start(
        testbed(
            vec![place("scout", "unit.scout", "player.1", 20.0, 20.0)],
            |x, _| if x >= 150 { 5.0 } else { 0.0 },
            true,
        ),
        &[None, None],
    );
    let tick = world.state().tick;
    let spell = |point, ignore_terrain, owner| SpellVision {
        id: 0,
        owner,
        point,
        surface: None,
        elevation: 0,
        radius: 4000,
        ignore_terrain,
        detect_invisible: false,
        expires: tick + 10,
        caster: None,
    };
    let ground = vision::add_spell_vision(world.state_mut(), spell(milli(147, 100), false, P0));
    let eye = vision::add_spell_vision(world.state_mut(), spell(milli(147, 120), true, P0));
    assert_eq!((ground, eye), (Some(1), Some(2)));
    world.step();
    assert!(visible(&world, 0, 149, 100) && !visible(&world, 0, 150, 100));
    assert!(visible(&world, 0, 150, 120) && visible(&world, 0, 151, 120));
    assert_eq!(
        fog(&world, 1, 147, 100),
        UNEXPLORED,
        "not shared with enemies"
    );
    verify(&world);
    for _ in 0..10 {
        world.step();
    }
    assert_eq!(fog(&world, 0, 147, 100), EXPLORED);
    assert!(world.state().knowledge.spells.is_empty());
    verify(&world);
}
