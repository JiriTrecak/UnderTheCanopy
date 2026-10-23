//! Hero inventories: picking up, dropping, using, validation, and what equipment and
//! consumables do to stats. Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_sim::{abilities::EventKind, stats::Stats};
use serde_json::{Value, json};

fn pick(actor: u32, target: u32, append: bool) -> Value {
    json!({"type": "pickup", "actor": actor, "target": target, "append": append})
}

fn use_item(actor: u32, slot: u32) -> Value {
    json!({"type": "useItem", "actor": actor, "slot": slot})
}

fn drop_item(actor: u32, slot: u32) -> Value {
    json!({"type": "dropItem", "actor": actor, "slot": slot})
}

fn slots(played: &Played, placement: &str) -> Vec<Option<String>> {
    let catalog = played.world.catalog();
    played
        .entity(placement)
        .equipment
        .as_ref()
        .map(|q| {
            q.slots
                .iter()
                .map(|s| s.map(|d| catalog.id(d).to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn gear_spec() -> Spec {
    let mut spec = Spec::default();
    spec.definitions.push(item("item.sword", json!({"type": "equipment", "damage": 5, "armor": 2, "maxHp": 50,
        "modifiers": {"attackSpeedPermille": 200, "healthRegenPerSecond": 2.0, "moveSpeedPermille": 100}})));
    spec.definitions.push(item(
        "item.banner",
        json!({"type": "equipment", "damage": 0, "armor": 0, "maxHp": 0,
        "aura": {"radius": 6, "target": "allies", "modifiers": {"armor": 3}}}),
    ));
    spec.definitions.push(item(
        "item.gloom",
        json!({"type": "equipment", "damage": 0, "armor": 0, "maxHp": 0,
        "aura": {"radius": 6, "target": "enemies", "modifiers": {"armor": -2}}}),
    ));
    spec
}

#[test]
fn heroes_pick_up_equip_and_drop() {
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "item.sword", 101.0, 100.0, 0),
        placed("c", "item.banner", 103.0, 101.0, 0),
        placed("d", "item.gloom", 100.0, 103.0, 0),
    ];
    let script = Script::new(
        "items-pickup",
        gear_spec(),
        entities,
        120,
        vec![
            (1, 1, pick(1, 2, false)),
            (2, 1, pick(1, 3, true)),
            (3, 1, pick(1, 4, true)),
            (100, 1, drop_item(1, 1)),
        ],
    );
    let base = {
        let world = canopy_sim::World::new(script.content(), &script.map(), setup(false)).unwrap();
        Stats::of(world.catalog(), entity(&world, "a"), &[])
    };
    let played = script.play();
    assert_eq!(
        slots(&played, "a")[..3],
        [Some("item.sword".into()), None, Some("item.gloom".into())]
    );
    let stats = Stats::of(played.world.catalog(), played.entity("a"), &[]);
    assert_eq!(stats.max_hp, base.max_hp + 50);
    assert_eq!(stats.armor, base.armor + 2, "the banner left with the drop");
    assert!(stats.damage_milli > base.damage_milli);
    // The banner lies on the ground again.
    let banner = played
        .world
        .state()
        .entities
        .iter()
        .find(|e| played.world.catalog().id(e.definition).as_str() == "item.banner")
        .expect("dropped banner");
    assert!(banner.hp.is_none() || banner.unit.is_none());
}

#[test]
fn item_auras_reach_allies_and_enemies() {
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("c", "item.banner", 100.0, 101.0, 0),
        placed("d", "item.gloom", 101.0, 100.0, 0),
        placed("e", "unit.warrior", 101.0, 98.0, 1),
        placed("f", "unit.warrior", 105.0, 100.0, 2),
    ];
    let played = Script::new(
        "items-aura",
        gear_spec(),
        entities.clone(),
        30,
        vec![(1, 1, pick(1, 2, false)), (2, 1, pick(1, 3, true))],
    )
    .play();
    let script = Script::new("items-aura", gear_spec(), entities, 1, vec![]);
    let world = canopy_sim::World::new(script.content(), &script.map(), setup(false)).unwrap();
    let ally = Stats::of(world.catalog(), entity(&world, "e"), &[]).armor;
    let enemy = Stats::of(world.catalog(), entity(&world, "f"), &[]).armor;
    assert_eq!(
        Stats::of(played.world.catalog(), played.entity("e"), &[]).armor,
        ally + 3
    );
    assert_eq!(
        Stats::of(played.world.catalog(), played.entity("f"), &[]).armor,
        enemy - 2
    );
}

#[test]
fn an_item_against_a_building_is_reached_from_the_heros_edge() {
    // The item lies just inside a building's footprint: the hero's centre stops a body's
    // radius off the wall, further from the item than its pickup range.
    let entities = vec![
        placed("a", "unit.hero", 110.0, 101.0, 1),
        placed("b", "item.sword", 102.0, 101.0, 0),
        placed("h", "building.barracks", 101.5, 101.5, 0),
    ];
    let played = Script::new(
        "items-wall",
        gear_spec(),
        entities,
        200,
        vec![(1, 1, pick(1, 2, false))],
    )
    .play();
    assert_eq!(slots(&played, "a")[0], Some("item.sword".into()));
    assert!(played.errors().is_empty(), "{:?}", played.errors());
}

#[test]
fn pickups_and_uses_are_validated() {
    let mut spec = gear_spec();
    spec.definitions.push(item(
        "item.potion",
        json!({"type": "consumable", "heal": 0,
        "active": {"cooldownTicks": 40, "charges": 1, "target": "self", "radius": 0, "heal": 40}}),
    ));
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "item.sword", 160.0, 100.0, 0),
        placed("c", "item.potion", 101.0, 100.0, 0),
        placed("d", "unit.warrior", 102.0, 102.0, 1),
        placed("e", "item.banner", 100.0, 101.0, 0),
    ];
    let played = Script::new(
        "items-validation",
        spec,
        entities,
        120,
        vec![
            (1, 1, pick(1, 2, false)),
            (2, 1, pick(4, 5, false)),
            (3, 1, use_item(1, 0)),
            (4, 1, pick(1, 3, false)),
            (30, 1, use_item(1, 0)),
            (40, 1, use_item(1, 0)),
            (50, 1, drop_item(1, 4)),
            (60, 2, pick(1, 5, false)),
        ],
    )
    .set(20, "a", json!({"hp": 50}))
    .play();
    let errors = played.errors();
    for expected in [
        "message.item-is-not-visible",
        "message.this-unit-has-no-inventory",
        "message.inventory-slot-is-empty",
        "message.this-item-cannot-be-used",
        "message.not-your-actors",
    ] {
        assert!(
            errors.iter().any(|e| e == expected),
            "missing {expected:?} in {errors:?}"
        );
    }
    assert!(
        (90..=95).contains(&played.hp("a")),
        "the potion healed 40 (plus regeneration): {}",
        played.hp("a")
    );
    assert!(
        slots(&played, "a").iter().all(Option::is_none),
        "the single charge was used up"
    );
}

#[test]
fn consumables_heal_restore_and_grant_permanent_bonuses() {
    let mut spec = Spec::default();
    spec.definitions.push(item("item.potion", json!({"type": "consumable", "heal": 0,
        "active": {"cooldownTicks": 40, "charges": 2, "target": "self", "radius": 0, "heal": 40, "mana": 10}})));
    spec.definitions.push(item("item.tome", json!({"type": "consumable", "heal": 0, "onPickup": true, "permanent": {"maxHp": 20, "damage": 2, "armor": 1}})));
    spec.definitions.push(item(
        "item.salve",
        json!({"type": "consumable", "heal": 30, "onPickup": true, "permanent": {"maxMana": 5}}),
    ));
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "item.potion", 101.0, 100.0, 0),
        placed("c", "item.tome", 99.0, 101.0, 0),
        placed("d", "item.salve", 102.0, 102.0, 0),
    ];
    let played = Script::new(
        "items-consumables",
        spec,
        entities,
        200,
        vec![
            (1, 1, pick(1, 2, false)),
            (20, 1, pick(1, 3, false)),
            (40, 1, pick(1, 4, false)),
            (90, 1, use_item(1, 0)),
            (100, 1, use_item(1, 0)),
            (140, 1, use_item(1, 0)),
        ],
    )
    .set(0, "a", json!({"hp": 30}))
    .play();
    let errors = played.errors();
    assert!(
        errors.iter().any(|e| e == "message.item-is-cooling-down"),
        "{errors:?}"
    );
    let bonuses = played
        .entity("a")
        .equipment
        .as_ref()
        .and_then(|q| q.bonuses)
        .expect("bonuses");
    assert_eq!(
        (
            bonuses.max_hp,
            bonuses.damage,
            bonuses.armor,
            bonuses.max_mana
        ),
        (20, 2, 1, 5)
    );
    // Pick-up consumables never take a slot; the potion is gone after two charges.
    assert!(slots(&played, "a").iter().all(Option::is_none));
    // The tome's health bonus heals with it; power-ups ignore the flat heal; two potions.
    assert!((130..=136).contains(&played.hp("a")), "{}", played.hp("a"));
}

#[test]
fn health_regenerates_through_the_accumulator() {
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "item.sword", 101.0, 100.0, 0),
    ];
    let played = Script::new(
        "items-regeneration",
        gear_spec(),
        entities,
        121,
        vec![(1, 1, pick(1, 2, false))],
    )
    .set(1, "a", json!({"hp": 100}))
    .play();
    let stats = Stats::of(played.world.catalog(), played.entity("a"), &[]);
    // 120 ticks at the hero's regeneration plus 2 per second: whole points only.
    let gained = played.hp("a") - 100;
    let expected =
        (stats.health_regen * 1000.0).round() as u32 * 120 / canopy_sim::items::regeneration::POINT;
    assert!(gained.abs_diff(expected) <= 1, "{gained} vs {expected}");
    assert!(
        played.entity("a").unit.as_ref().unwrap().regeneration
            < canopy_sim::items::regeneration::POINT
    );
    let _ = EventKind::Healed;
}
