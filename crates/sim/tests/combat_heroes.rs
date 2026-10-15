//! Experience, levels and fallen heroes on test-owned content.

mod combat_fixture;

use canopy_sim::{World, mission::Systems, state::FactKind, stats::Stats};
use combat_fixture::*;
use serde_json::{Value, json};

fn experience(world: &World, placement: &str) -> u32 {
    entity(world, placement)
        .progression
        .as_ref()
        .map_or(0, |p| p.experience)
}

fn with_experience(mut placement: Value, experience: u32) -> Value {
    placement["initialState"] = json!({"experience": experience});
    placement
}

#[test]
fn kills_pay_experience_with_the_lone_hero_bonus() {
    let map = map(vec![
        placed("h", "unit.hero", 100.0, 100.0, 1),
        placed("s1", "unit.swordsman", 105.0, 100.0, 2),
        placed("s2", "unit.swordsman", 106.0, 102.0, 2),
        placed("s3", "unit.bowman", 112.0, 101.0, 2),
    ]);
    let mut world = world(&map);
    run_until(&mut world, 400, |w| !alive(w, "s1"));
    // A level-1 unit is worth 20; the only hero of a roster with room for two earns 10 % more.
    assert_eq!(experience(&world, "h"), 22);
    run_until(&mut world, 1200, |w| !alive(w, "s3"));
    assert_eq!(experience(&world, "h"), 66);
    // Only the owner sees experience.
    let h = canopy_sim::entity::EntityId(id(&world, "h"));
    let own = world.view(player(1));
    let theirs = world.view(player(2));
    let combat = |view: &canopy_sim::view::View| view.entity(h).and_then(|e| e.combat.clone());
    assert_eq!(combat(&own).unwrap().experience, Some(66));
    assert_eq!(combat(&own).unwrap().stats.level, 1);
    if let Some(seen) = combat(&theirs) {
        assert_eq!(seen.experience, None);
        assert!(seen.hostile);
    }
}

#[test]
fn levelling_raises_stats_and_keeps_damage_taken() {
    let map = map(vec![
        with_experience(placed("h", "unit.hero", 100.0, 100.0, 1), 90),
        placed("s1", "unit.swordsman", 105.0, 100.0, 2),
    ]);
    let mut world = world(&map);
    let level = |w: &World| Stats::of(w.catalog(), entity(w, "h"), &[]);
    assert_eq!(level(&world).level, 1);
    run_until(&mut world, 400, |w| !alive(w, "s1"));
    assert_eq!(experience(&world, "h"), 112);
    let stats = level(&world);
    assert_eq!((stats.level, stats.max_hp, stats.armor), (2, 260, 4));
    assert_eq!(stats.damage_points(), 19);
    // The 60 extra health is added to what was left, not a refill.
    let hp = entity(&world, "h").hp.unwrap();
    assert!(hp < 260 && hp > 60, "{hp}");
    assert!(
        world
            .state()
            .facts
            .iter()
            .any(|f| f.kind == FactKind::Command && f.message == "unit.hero reached level 2")
    );
}

#[test]
fn heroes_nearby_share_a_reward_by_id() {
    let map = map(vec![
        placed("h1", "unit.hero", 100.0, 100.0, 1),
        placed("h2", "unit.hero", 100.0, 103.0, 1),
        placed("s", "unit.swordsman", 98.0, 101.0, 1),
        placed("e1", "unit.swordsman", 105.0, 100.0, 2),
    ]);
    let mut world = world(&map);
    run_until(&mut world, 600, |w| !alive(w, "e1"));
    // 20 split two ways, each with the lone-hero bonus (one hero definition on the roster).
    assert_eq!(
        (experience(&world, "h1"), experience(&world, "h2")),
        (11, 11)
    );
}

#[test]
fn far_away_heroes_learn_from_their_side_s_kills() {
    let map = map(vec![
        placed("h", "unit.hero", 40.0, 40.0, 1),
        placed("a", "unit.knight", 100.0, 100.0, 1),
        placed("e", "unit.swordsman", 105.0, 100.0, 2),
    ]);
    let mut world = world(&map);
    run_until(&mut world, 900, |w| !alive(w, "e"));
    // Nobody with levels was near, so the killer's heroes anywhere share it.
    assert_eq!(experience(&world, "h"), 22);
}

#[test]
fn neutral_kills_pay_less_per_level_and_stop_at_the_cap() {
    let entities = |experience: u32| {
        vec![
            with_experience(placed("h", "unit.hero", 95.0, 100.0, 1), experience),
            placed("c1", "unit.critter", 100.0, 100.0, 0),
        ]
    };
    let camps = vec![camp("camp.a", &["c1"], (100.0, 100.0), 8, 12, json!({}))];
    let mut level_one = World::new(
        content(),
        &map_with(entities(0), camps.clone(), flat()),
        setup(false),
    )
    .unwrap();
    run_until(&mut level_one, 900, |w| !alive(w, "c1"));
    assert_eq!(experience(&level_one, "h"), 22);
    // At level 2 a neutral kill pays 70 %: 20 · 0.7 · 1.1 = 15.4 → 15.
    let mut level_two = World::new(
        content(),
        &map_with(entities(100), camps.clone(), flat()),
        setup(false),
    )
    .unwrap();
    run_until(&mut level_two, 900, |w| !alive(w, "c1"));
    assert_eq!(experience(&level_two, "h"), 115);
    // Neutral experience stops at level 3 (250 here); the reward is clipped to it.
    let mut capped = World::new(
        content(),
        &map_with(entities(245), camps, flat()),
        setup(false),
    )
    .unwrap();
    run_until(&mut capped, 900, |w| !alive(w, "c1"));
    assert_eq!(experience(&capped, "h"), 250);
}

#[test]
fn a_dead_hero_is_kept_as_fallen() {
    let map = map(vec![
        placed("h", "unit.hero", 100.0, 100.0, 1),
        placed("k1", "unit.knight", 104.0, 100.0, 2),
        placed("k2", "unit.knight", 104.0, 102.0, 2),
        placed("k3", "unit.knight", 103.0, 97.0, 2),
        placed("k4", "unit.knight", 98.0, 104.0, 2),
    ]);
    let mut world = world(&map);
    run_until(&mut world, 1500, |w| !alive(w, "h"));
    let hero = entity(&world, "h");
    assert!(hero.fallen && hero.hp == Some(0));
    // It lies on a cell, with a fresh unit and nothing carried, and leaves no corpse.
    assert_eq!(hero.position.x % 1000, 0);
    assert_eq!(hero.position.y % 1000, 0);
    let u = hero.unit.as_ref().unwrap();
    assert!(u.order.is_none() && u.target.is_none() && u.route.is_empty());
    assert!(world.state().combat.corpses.iter().all(|c| c.id != hero.id));
    assert!(canopy_sim::mission::Current.fallen(hero));
    // Only its owner lists it, and keeps it in its view (to revive it, for its portrait);
    // rivals do not see it, and the scene draws no fallen hero.
    let id = hero.id;
    assert_eq!(world.view(player(1)).battle.fallen, [id]);
    assert!(world.view(player(2)).battle.fallen.is_empty());
    assert!(world.view(player(1)).entity(id).is_some());
    assert!(world.view(player(2)).entity(id).is_none());
    // Enemies stop fighting it and the knights earn nothing (no levels).
    run(&mut world, 10);
    assert!(
        world
            .state()
            .entities
            .iter()
            .all(|e| e.unit.as_ref().is_none_or(|u| u.target != Some(id)))
    );
}
