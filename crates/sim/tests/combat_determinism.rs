//! Battles replay identically, and a save taken mid-fight (swings, missiles and shells in
//! flight, slows, charges, a lookout, a camp) continues exactly as the original.

mod combat_fixture;

use canopy_sim::World;
use combat_fixture::*;
use serde_json::{Value, json};

const KINDS: [&str; 6] = [
    "unit.swordsman",
    "unit.knight",
    "unit.bowman",
    "unit.lancer",
    "unit.mortar",
    "unit.wasp",
];

fn battle() -> (canopy_content::map::UtcMap, Vec<Value>) {
    let mut entities = Vec::new();
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for i in 0..24u32 {
        let (row, col) = (i / 6, i % 6);
        let (na, nb) = (format!("a{i:02}"), format!("b{i:02}"));
        entities.push(placed(
            &na,
            KINDS[(i as usize * 7) % 6],
            f64::from(80 - row * 2),
            f64::from(94 + col * 2),
            1,
        ));
        entities.push(placed(
            &nb,
            KINDS[(i as usize * 5 + 1) % 6],
            f64::from(120 + row * 2),
            f64::from(94 + col * 2),
            2,
        ));
        a.push(na);
        b.push(nb);
    }
    entities.push(placed("tower", "building.tower", 93.5, 113.5, 1));
    entities.push(placed("hero", "unit.hero", 90.0, 110.0, 1));
    entities.push(placed("c1", "unit.critter", 100.0, 130.0, 0));
    entities.push(placed("c2", "unit.spitter", 102.0, 131.0, 0));
    let camps = vec![camp(
        "camp.d",
        &["c1", "c2"],
        (101.0, 130.0),
        8,
        12,
        json!({"lootPool": "loot.test"}),
    )];
    let map = map_with(entities, camps, flat());
    let world = World::new(content(), &map, setup(false)).unwrap();
    let ids = |names: &[String]| names.iter().map(|n| id(&world, n)).collect::<Vec<_>>();
    let orders = vec![
        json!({"type": "move", "actors": ids(&a), "destination": {"x": 122, "y": 100}, "attackMove": true}),
        json!({"type": "move", "actors": ids(&b), "destination": {"x": 78, "y": 100}, "attackMove": true}),
        json!({"type": "garrison", "actors": [id(&world, "a02")], "target": id(&world, "tower")}),
        json!({"type": "move", "actors": [id(&world, "hero")], "destination": {"x": 98, "y": 126}, "attackMove": true}),
    ];
    (map, orders)
}

fn start(map: &canopy_content::map::UtcMap, orders: &[Value]) -> World {
    let mut world = World::new(content(), map, setup(false)).unwrap();
    for (i, order) in orders.iter().enumerate() {
        let player = if i == 1 { 2 } else { 1 };
        command(&mut world, player, order.clone());
    }
    world
}

#[test]
fn the_same_battle_twice_is_the_same_battle() {
    let (map, orders) = battle();
    let mut one = start(&map, &orders);
    let mut two = start(&map, &orders);
    for _ in 0..1200 {
        one.step();
        two.step();
        assert_eq!(one.fingerprint(), two.fingerprint());
    }
    assert_eq!(one.state(), two.state());
    // The fight happened: deaths, corpses and projectiles.
    assert!(!one.state().combat.corpses.is_empty());
    assert!(one.state().counters.missile > 1 && one.state().counters.shell > 1);
}

#[test]
fn a_save_mid_fight_continues_exactly() {
    let (map, orders) = battle();
    let mut original = start(&map, &orders);
    // Run until projectiles are in flight and swings are under way.
    let mut ticks = 0;
    loop {
        original.step();
        ticks += 1;
        let s = original.state();
        let in_flight = s.combat.missiles.iter().any(|m| !m.resolved)
            && s.combat.shells.iter().any(|m| !m.resolved);
        let swinging = s.entities.iter().any(|e| {
            e.unit
                .as_ref()
                .is_some_and(|u| u.combat.attack.as_ref().is_some_and(|a| !a.released))
        });
        let slowed = s
            .entities
            .iter()
            .any(|e| e.unit.as_ref().is_some_and(|u| !u.combat.slows.is_empty()));
        if in_flight && swinging && slowed {
            break;
        }
        assert!(ticks < 2000, "no mid-fight moment found");
    }
    let mut restored = reload(&original, &map);
    assert_eq!(restored.state(), original.state());
    for _ in 0..800 {
        original.step();
        restored.step();
        assert_eq!(restored.fingerprint(), original.fingerprint());
    }
    assert_eq!(restored.state(), original.state());
    // Derived sight still equals a cold rebuild after deaths and lookouts.
    original
        .vision()
        .verify(
            original.state(),
            original.catalog(),
            original.terrain(),
            &canopy_sim::vision::Current,
        )
        .unwrap();
    // Views agree too, combat included.
    for owner in [player(1), player(2)] {
        let (a, b) = (original.view(owner), restored.view(owner));
        assert_eq!(a.entities, b.entities);
        assert_eq!(a.battle, b.battle);
    }
}
