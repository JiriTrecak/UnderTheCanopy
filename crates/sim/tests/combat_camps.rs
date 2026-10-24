//! Neutral camps: aggression, leash and return, loot rolled on the state's generator.

mod combat_fixture;

use canopy_sim::World;
use combat_fixture::*;
use serde_json::json;

fn items(world: &World) -> Vec<(String, i32, i32)> {
    world
        .state()
        .entities
        .iter()
        .filter(|e| e.item.is_some())
        .map(|e| {
            (
                world.catalog().id(e.definition).as_str().to_string(),
                e.position.x / 1000,
                e.position.y / 1000,
            )
        })
        .collect()
}

fn returning(world: &World, placement: &str) -> bool {
    entity(world, placement)
        .unit
        .as_ref()
        .and_then(|u| u.camp.as_ref())
        .is_some_and(|c| c.returning)
}

#[test]
fn a_cleared_camp_pays_its_loot_pool_once() {
    let camps = vec![camp(
        "camp.test",
        &["c1", "c2", "c3"],
        (101.0, 100.0),
        8,
        12,
        json!({"lootPool": "loot.test"}),
    )];
    let map = map_with(
        vec![
            placed("h", "unit.hero", 80.0, 100.0, 1),
            placed("c1", "unit.critter", 100.0, 100.0, 0),
            placed("c2", "unit.critter", 101.0, 101.0, 0),
            placed("c3", "unit.spitter", 102.0, 99.0, 0),
        ],
        camps,
        flat(),
    );
    let mut world = World::new(content(), &map, setup(false)).unwrap();
    let h = id(&world, "h");
    command(
        &mut world,
        1,
        json!({"type": "move", "actors": [h], "destination": {"x": 95, "y": 100}}),
    );
    let seed = world.state().random;
    run_until(&mut world, 1200, |w| !alive(w, "c1"));
    // Nothing drops while a member lives, and nothing is rolled.
    assert!(items(&world).is_empty());
    assert_eq!(world.state().random, seed);
    run_until(&mut world, 1200, |w| !alive(w, "c3"));
    assert_eq!(world.state().cleared_camps, ["camp.test"]);
    // Three weighted rolls with seed 7 (one could have been empty), around the last body,
    // two cells apart.
    assert_eq!(
        items(&world),
        [
            ("item.charm".to_string(), 100, 99),
            ("item.trinket".to_string(), 100, 97),
            ("item.trinket".to_string(), 98, 99),
        ]
    );
    assert_ne!(world.state().random, seed);
    // Loot is neutral and takes part from the next tick.
    let drop = world
        .state()
        .entities
        .iter()
        .find(|e| e.item.is_some())
        .unwrap();
    assert_eq!(drop.owner, canopy_content::placement::Owner::Nobody);
    assert_eq!(drop.ready_tick, world.state().tick);
}

#[test]
fn a_member_past_its_leash_walks_home_and_fights_again() {
    let camps = vec![camp(
        "camp.test",
        &["c1"],
        (100.0, 100.0),
        8,
        10,
        json!({"fixedDrops": ["item.charm"]}),
    )];
    let map = map_with(
        vec![
            placed("s", "unit.swordsman", 92.0, 100.0, 1),
            placed("c1", "unit.critter", 100.0, 100.0, 0),
        ],
        camps,
        flat(),
    );
    let mut world = World::new(content(), &map, setup(false)).unwrap();
    let s = id(&world, "s");
    run(&mut world, 30);
    assert!(entity(&world, "c1").unit.as_ref().unwrap().target.is_some());
    command(
        &mut world,
        1,
        json!({"type": "move", "actors": [s], "destination": {"x": 70, "y": 100}}),
    );
    // It chases past ten cells from home, then gives up and walks back.
    run_until(&mut world, 400, |w| returning(w, "c1"));
    let c = entity(&world, "c1");
    assert!(c.unit.as_ref().unwrap().target.is_none());
    let away = (
        f64::from(c.position.x) / 1000.0 - 100.0,
        f64::from(c.position.y) / 1000.0 - 100.0,
    );
    assert!(
        (away.0 * away.0 + away.1 * away.1).sqrt() >= 9.9,
        "past the leash: {away:?}"
    );
    run_until(&mut world, 600, |w| !returning(w, "c1"));
    let c = entity(&world, "c1");
    let (dx, dy) = (
        f64::from(c.position.x) / 1000.0 - 100.0,
        f64::from(c.position.y) / 1000.0 - 100.0,
    );
    assert!(dx * dx + dy * dy <= 1.0);
    // Back home it fights whoever comes close again.
    command(
        &mut world,
        1,
        json!({"type": "move", "actors": [s], "destination": {"x": 94, "y": 100}}),
    );
    run_until(&mut world, 600, |w| !alive(w, "c1") || !alive(w, "s"));
    if !alive(&world, "c1") {
        // Fixed drops need no roll.
        assert_eq!(items(&world).len(), 1);
        assert_eq!(items(&world)[0].0, "item.charm");
    }
}

#[test]
fn passive_camps_fight_only_when_attacked_by_force() {
    let camps = vec![camp(
        "camp.calm",
        &["c1"],
        (100.0, 100.0),
        8,
        12,
        json!({"aggression": "passive"}),
    )];
    let map = map_with(
        vec![
            placed("h", "unit.hero", 97.0, 100.0, 1),
            placed("c1", "unit.critter", 100.0, 100.0, 0),
        ],
        camps,
        flat(),
    );
    let mut world = World::new(content(), &map, setup(false)).unwrap();
    run(&mut world, 200);
    assert_eq!(hp(&world, "c1"), 80);
    assert_eq!(hp(&world, "h"), 200);
    // An ordinary attack order is refused; a forced one goes through.
    let (h, c) = (id(&world, "h"), id(&world, "c1"));
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [h], "target": c}),
    );
    run(&mut world, 1);
    assert_eq!(errors(&world), ["message.no-actors-support-that-order"]);
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [h], "target": c, "force": true}),
    );
    run_until(&mut world, 900, |w| !alive(w, "c1"));
    // A passive creature is not an enemy: no experience for it.
    assert_eq!(
        entity(&world, "h").progression.as_ref().unwrap().experience,
        0
    );
    assert_eq!(world.state().cleared_camps, ["camp.calm"]);
}

fn target_of(world: &World, placement: &str) -> Option<u32> {
    entity(world, placement)
        .unit
        .as_ref()
        .and_then(|u| u.target)
        .map(|t| t.0)
}

#[test]
fn a_camp_shot_from_beyond_its_aggro_range_answers_together() {
    // The bowman reaches 10 cells, the camp sees 8: the struck member and the one
    // behind it both turn on the shooter.
    let camps = vec![camp(
        "camp.test",
        &["c1", "c2"],
        (103.0, 100.0),
        8,
        24,
        json!({}),
    )];
    let map = map_with(
        vec![
            placed("b", "unit.bowman", 91.0, 100.0, 1),
            placed("c1", "unit.critter", 100.0, 100.0, 0),
            placed("c2", "unit.critter", 106.0, 100.0, 0),
        ],
        camps,
        flat(),
    );
    let mut world = World::new(content(), &map, setup(false)).unwrap();
    let (b, c1) = (id(&world, "b"), id(&world, "c1"));
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [b], "target": c1}),
    );
    run_until(&mut world, 200, |w| hp(w, "c1") < 80);
    run(&mut world, 5);
    assert_eq!(target_of(&world, "c1"), Some(b));
    assert_eq!(target_of(&world, "c2"), Some(b));
    run_until(&mut world, 400, |w| hp(w, "b") < 70);
    assert!(hp(&world, "b") < 70, "the camp reaches the shooter");
}

#[test]
fn a_member_that_finds_an_enemy_calls_its_whole_camp() {
    // The swordsman comes within reach of the first member only; the far one joins.
    let camps = vec![camp(
        "camp.test",
        &["c1", "c2"],
        (106.0, 100.0),
        8,
        24,
        json!({}),
    )];
    let map = map_with(
        vec![
            placed("s", "unit.swordsman", 80.0, 100.0, 1),
            placed("c1", "unit.critter", 100.0, 100.0, 0),
            placed("c2", "unit.critter", 112.0, 100.0, 0),
        ],
        camps,
        flat(),
    );
    let mut world = World::new(content(), &map, setup(false)).unwrap();
    let s = id(&world, "s");
    command(
        &mut world,
        1,
        json!({"type": "move", "actors": [s], "destination": {"x": 93, "y": 100}}),
    );
    run_until(&mut world, 300, |w| target_of(w, "c1").is_some());
    run(&mut world, 3);
    assert_eq!(target_of(&world, "c1"), Some(s));
    assert_eq!(
        target_of(&world, "c2"),
        Some(s),
        "twenty cells away, it joins"
    );
}

#[test]
fn a_passive_camp_struck_by_force_answers_as_a_whole() {
    let camps = vec![camp(
        "camp.calm",
        &["c1", "c2"],
        (103.0, 100.0),
        8,
        24,
        json!({"aggression": "passive"}),
    )];
    let map = map_with(
        vec![
            placed("h", "unit.hero", 97.0, 100.0, 1),
            placed("c1", "unit.critter", 100.0, 100.0, 0),
            placed("c2", "unit.critter", 112.0, 100.0, 0),
        ],
        camps,
        flat(),
    );
    let mut world = World::new(content(), &map, setup(false)).unwrap();
    run(&mut world, 100);
    assert_eq!(target_of(&world, "c2"), None, "passive until struck");
    let (h, c1) = (id(&world, "h"), id(&world, "c1"));
    command(
        &mut world,
        1,
        json!({"type": "attack", "actors": [h], "target": c1, "force": true}),
    );
    run_until(&mut world, 200, |w| hp(w, "c1") < 80);
    run(&mut world, 5);
    assert_eq!(target_of(&world, "c2"), Some(h), "the far member joins");
    run_until(&mut world, 300, |w| hp(w, "h") < 190);
    assert!(hp(&world, "h") < 190, "the camp fights back");
}
