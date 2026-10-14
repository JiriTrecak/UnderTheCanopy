//! Duels with exact outcomes: melee, missiles, armour, shells, charges, flying targets.
//! The numbers are the old game's (`settlers-web`) for the same content and map.

mod combat_fixture;

use canopy_sim::{World, entity::EntityId, state::FactKind};
use combat_fixture::*;

/// Ticks at which `placement`'s health changed while it lived, with the new health.
fn health_changes(world: &mut World, placement: &str, ticks: u32) -> Vec<(u32, u32)> {
    let mut changes = Vec::new();
    let mut last = hp(world, placement);
    for _ in 0..ticks {
        let tick = world.state().tick;
        world.step();
        let now = hp(world, placement);
        if now != last {
            changes.push((tick, now));
            last = now;
        }
    }
    changes
}

#[test]
fn simultaneous_blows_kill_both_duellists_on_the_same_tick() {
    let map = map(vec![
        placed("a", "unit.swordsman", 100.0, 100.0, 1),
        placed("b", "unit.swordsman", 106.0, 100.0, 2),
    ]);
    let mut world = world(&map);
    let (a, b) = (id(&world, "a"), id(&world, "b"));
    // Each unit looks around only on ticks where `tick % 8 == id % 8`.
    run(&mut world, 2);
    let target = |w: &World, p| entity(w, p).unit.as_ref().unwrap().target;
    assert_eq!(target(&world, "a"), Some(EntityId(b)));
    assert_eq!(target(&world, "b"), None);
    run(&mut world, 1);
    assert_eq!(target(&world, "b"), Some(EntityId(a)));
    // Blade 10 against light armour 2: 10 / 1.12 = 8.9, rounded to 9 per blow, every 40 ticks
    // from tick 39; both strike at once, so both fall at tick 479.
    let changes = health_changes(&mut world, "a", 497);
    assert_eq!(&changes[..3], &[(39, 91), (79, 82), (119, 73)]);
    assert_eq!(changes.last(), Some(&(479, 0)));
    assert!(find(&world, "a").is_none() && find(&world, "b").is_none());
    let corpses = &world.state().combat.corpses;
    assert_eq!(corpses.len(), 2);
    assert!(corpses.iter().all(|c| c.died == 479));
    let deaths = world
        .state()
        .facts
        .iter()
        .filter(|f| f.kind == FactKind::Death)
        .count();
    assert_eq!(deaths, 2);
    // Both owners saw both deaths.
    for owner in [player(1), player(2)] {
        assert_eq!(world.view(owner).deaths.len(), 2);
    }
}

#[test]
fn missiles_take_their_flight_time_and_hit_where_the_target_is() {
    let map = map(vec![
        placed("a", "unit.bowman", 100.0, 100.0, 1),
        placed("b", "unit.bowman", 113.0, 104.0, 2),
        placed("c", "unit.swordsman", 118.0, 98.0, 2),
    ]);
    let mut world = world(&map);
    let mut launched = None;
    while launched.is_none() {
        world.step();
        launched = world.state().combat.missiles.first().cloned();
    }
    let missile = launched.unwrap();
    let (dx, dy) = (
        f64::from(missile.destination.position.x - missile.origin.position.x) / 1000.0,
        f64::from(missile.destination.position.y - missile.origin.position.y) / 1000.0,
    );
    // Flight time: distance over 20 cells per second, in 40 Hz ticks, rounded up.
    let flight = ((dx * dx + dy * dy).sqrt() / 20.0 * 40.0).ceil() as u32;
    assert_eq!(missile.impact - missile.launched, flight);
    // Pierce 8 against unarmoured light bodies (120 %): 9.6, rounded to 10 a shot.
    let changes = health_changes(&mut world, "a", 400);
    assert_eq!(&changes[..3], &[(60, 60), (110, 50), (160, 40)]);
    assert!(!alive(&world, "a") && !alive(&world, "b"));
    // Landed missiles linger a few ticks for views, then go.
    assert!(world.state().combat.missiles.iter().all(|m| m.resolved));
    run(&mut world, 5);
    assert!(world.state().combat.missiles.is_empty());
}

#[test]
fn armour_and_the_damage_matrix_scale_every_blow() {
    let map = map(vec![
        placed("a1", "unit.swordsman", 100.0, 100.0, 1),
        placed("a2", "unit.swordsman", 100.0, 102.0, 1),
        placed("a3", "unit.swordsman", 100.0, 98.0, 1),
        placed("k", "unit.knight", 105.0, 100.0, 2),
        placed("k2", "unit.knight", 106.0, 103.0, 2),
    ]);
    let mut world = world(&map);
    let mut knight = Vec::new();
    let mut swordsman = Vec::new();
    let (mut k, mut s) = (hp(&world, "k"), hp(&world, "a1"));
    for _ in 0..120 {
        world.step();
        let (k2, s2) = (hp(&world, "k"), hp(&world, "a1"));
        if k2 != k {
            knight.push(k - k2);
        }
        if s2 != s {
            swordsman.push(s - s2);
        }
        (k, s) = (k2, s2);
    }
    // Blade 10 on heavy armour 6: 10 · 60 % / (1 + 0.06 · 6) = 4.4 → 4 a blow.
    assert!(
        !knight.is_empty() && knight.iter().all(|&d| d % 4 == 0),
        "{knight:?}"
    );
    // Blade 14 on light armour 2: 14 / 1.12 = 12.5, rounded half up to 13.
    assert_eq!(swordsman, [13, 13]);
}

#[test]
fn shells_splash_and_slow_only_the_enemies_they_were_aimed_at() {
    let map = map(vec![
        placed("m", "unit.mortar", 80.0, 100.0, 1),
        placed("s1", "unit.swordsman", 100.0, 100.0, 2),
        placed("s2", "unit.swordsman", 101.0, 101.0, 2),
        placed("s3", "unit.swordsman", 99.0, 102.0, 2),
        placed("g", "unit.swordsman", 70.0, 100.0, 1),
    ]);
    let mut world = world(&map);
    let m = id(&world, "m");
    command(
        &mut world,
        1,
        serde_json::json!({"type": "move", "actors": [m], "destination": {"x": 90, "y": 100}, "attackMove": true}),
    );
    run(&mut world, 102);
    // The first shell lands on s3 alone: blast 20 on light armour 2 is 17.9 → 18.
    assert_eq!(hp(&world, "s3"), 82);
    let slows = |w: &World, p| entity(w, p).unit.as_ref().unwrap().combat.slows.clone();
    assert_eq!(slows(&world, "s3").len(), 1);
    assert_eq!(slows(&world, "s3")[0].permille, 300);
    // A slowed unit walks at 70 %.
    assert_eq!(
        canopy_sim::stats::move_speed_permille(entity(&world, "s3")),
        700
    );
    let shell = world.state().combat.shells[0].clone();
    assert_eq!(shell.victims, [player(2)]);
    assert_eq!(shell.impact - shell.launched, 30);
    // The slow wears off after 60 ticks.
    run(&mut world, 60);
    assert!(slows(&world, "s3").is_empty());
    // Later shells catch the other two together; the mortar's own side is never hit.
    run(&mut world, 420);
    assert_eq!((hp(&world, "s1"), hp(&world, "s2")), (82, 82));
    assert_eq!(hp(&world, "g"), 100);
}

#[test]
fn a_charge_doubles_speed_and_the_first_blow() {
    let map = map(vec![
        placed("l", "unit.lancer", 80.0, 100.0, 1),
        placed("s", "unit.swordsman", 95.0, 100.0, 2),
        placed("l2", "unit.lancer", 80.0, 120.0, 1),
        placed("s2", "unit.knight", 92.0, 126.0, 2),
    ]);
    let mut world = world(&map);
    let mut charged = false;
    let mut fastest = 0;
    let mut last = entity(&world, "l").position;
    for _ in 0..64 {
        world.step();
        let e = entity(&world, "l");
        let step = (e.position.x - last.x)
            .abs()
            .max((e.position.y - last.y).abs());
        fastest = fastest.max(step);
        last = e.position;
        charged |= canopy_sim::combat::charge::charging(e, world.state().tick - 1);
    }
    assert!(charged);
    // Speed 5 cells per second is 125 milli-cells a tick; charging doubles it.
    assert!(fastest > 125, "{fastest}");
    // Blade 12 doubled on light armour 2: 24 / 1.12 = 21.4 → 21; later blows 11.
    let changes = health_changes(&mut world, "s", 60);
    assert_eq!(changes[0].1, 79, "{changes:?}");
    assert_eq!(changes[1].1, 68, "{changes:?}");
    // The charge's cooldown stays after the blow.
    let charge = entity(&world, "l")
        .unit
        .as_ref()
        .unwrap()
        .combat
        .charge
        .clone()
        .unwrap();
    assert_eq!(charge.target, None);
    assert!(charge.ready_tick > world.state().tick);
}

#[test]
fn ground_weapons_cannot_reach_flying_units() {
    let map = map(vec![
        placed("w", "unit.wasp", 100.0, 100.0, 2),
        placed("s", "unit.swordsman", 104.0, 100.0, 1),
        placed("b", "unit.bowman", 90.0, 100.0, 1),
    ]);
    let mut world = world(&map);
    let w = EntityId(id(&world, "w"));
    let mut swordsman_targeted = false;
    for _ in 0..400 {
        world.step();
        swordsman_targeted |= find(&world, "s")
            .and_then(|e| e.unit.as_ref()?.target)
            .is_some_and(|t| t == w);
    }
    // The bowman shot the wasp down; the swordsman never engaged it.
    assert!(!alive(&world, "w"));
    assert!(!swordsman_targeted);
    assert_eq!(hp(&world, "s"), 45);
}

#[test]
fn views_show_swings_projectiles_and_corpses_to_those_who_see_them() {
    let map = map(vec![
        placed("a", "unit.bowman", 100.0, 100.0, 1),
        placed("b", "unit.swordsman", 106.0, 100.0, 2),
        placed("far", "unit.swordsman", 200.0, 200.0, 2),
    ]);
    let mut world = world(&map);
    let (a, b) = (EntityId(id(&world, "a")), EntityId(id(&world, "b")));
    run_until(&mut world, 200, |w| !w.state().combat.missiles.is_empty());
    let own = world.view(player(1));
    let theirs = world.view(player(2));
    // Both sides saw the shot leave and see where it flies.
    assert_eq!(own.battle.missiles.len(), 1);
    assert_eq!(theirs.battle.missiles.len(), 1);
    let mine = own.entity(a).unwrap().combat.clone().unwrap();
    assert_eq!(mine.target, Some(b));
    let seen = theirs.entity(a).unwrap().combat.clone().unwrap();
    // The swing is public, its target too while the viewer sees it; orders are private.
    assert!(seen.hostile && !mine.hostile);
    assert_eq!(seen.target, None);
    assert_eq!(
        seen.attack.as_ref().map(|s| s.target),
        mine.attack.as_ref().map(|s| s.target)
    );
    assert_eq!(seen.stats.max_hp, 70);
    run_until(&mut world, 900, |w| !alive(w, "b") || !alive(w, "a"));
    run(&mut world, 1);
    // The survivor's side sees the body on the field.
    let survivor = if alive(&world, "a") { 1 } else { 2 };
    assert_eq!(world.view(player(survivor)).battle.corpses.len(), 1);
    // An observer sees everything.
    assert!(!world.observer_view().battle.corpses.is_empty());
}
