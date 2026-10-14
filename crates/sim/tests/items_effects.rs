//! Item effects in combat: actives (damage, heal, statuses with shields, cooldown
//! refunds), rescue, on-hit procs, lifesteal and reflection, item blocking. Test-owned
//! content only.

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

fn max_hp(played: &Played, placement: &str) -> u32 {
    Stats::of(played.world.catalog(), played.entity(placement), &[]).max_hp
}

fn wand_spec() -> Spec {
    let mut spec = Spec::default();
    spec.definitions.push(item("item.wand", json!({"type": "equipment", "damage": 0, "armor": 0, "maxHp": 0,
        "active": {"cooldownTicks": 60, "target": "enemies", "radius": 8, "damage": 30, "damageType": "spell"}})));
    spec.definitions.push(item("item.aegis", json!({"type": "equipment", "damage": 0, "armor": 0, "maxHp": 0,
        "active": {"cooldownTicks": 200, "target": "self", "radius": 0, "status": {"durationTicks": 100, "shield": 50, "modifiers": {"controlImmune": true}}}})));
    spec.definitions.push(item(
        "item.amulet",
        json!({"type": "equipment", "damage": 0, "armor": 0, "maxHp": 0,
        "rescue": {"healMaxPermille": 300, "invulnerableTicks": 40, "charges": 1}}),
    ));
    spec.definitions.push(item(
        "item.fang",
        json!({"type": "equipment", "damage": 0, "armor": 0, "maxHp": 0,
        "onHit": {"every": 2, "radius": 2, "targets": 2, "damage": 10, "damageType": "spell"}}),
    ));
    spec.definitions.push(item(
        "item.leech",
        json!({"type": "equipment", "damage": 0, "armor": 0, "maxHp": 0,
        "modifiers": {"lifestealPermille": 500, "meleeReflectionPermille": 500}}),
    ));
    spec
}

#[test]
fn active_items_strike_enemies_and_shield_the_hero() {
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "item.wand", 100.0, 101.0, 0),
        placed("c", "item.aegis", 101.0, 101.0, 0),
        placed("f", "unit.warrior", 106.0, 100.0, 2),
        placed("g", "unit.warrior", 107.0, 102.0, 2),
        placed("h", "unit.warrior", 120.0, 100.0, 2),
    ];
    let played = Script::new(
        "items-actives",
        wand_spec(),
        entities,
        40,
        vec![
            (1, 1, pick(1, 2, false)),
            (2, 1, pick(1, 3, true)),
            (20, 1, use_item(1, 0)),
            (22, 1, use_item(1, 1)),
            (24, 1, use_item(1, 0)),
        ],
    )
    .play();
    assert!(played.errors().iter().any(|e| e == "Item is cooling down"));
    for (placement, hit) in [("f", true), ("g", true), ("h", false)] {
        let max = max_hp(&played, placement);
        assert_eq!(played.hp(placement) < max, hit, "{placement}");
    }
    let equipment = played.entity("a").equipment.as_ref().unwrap();
    let shield = equipment
        .statuses
        .iter()
        .find_map(|s| s.shield)
        .expect("aegis status");
    assert!(shield <= 50);
}

#[test]
fn rescue_saves_the_hero_once() {
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("d", "item.amulet", 99.0, 100.0, 0),
        placed("f", "unit.warrior", 103.0, 100.0, 2),
        placed("g", "unit.warrior", 103.0, 102.0, 2),
    ];
    let played = Script::new(
        "items-rescue",
        wand_spec(),
        entities,
        160,
        vec![(1, 1, pick(1, 2, false))],
    )
    .set(40, "a", json!({"hp": 5}))
    .play();
    let a = played.entity("a");
    assert!(!a.fallen, "the amulet caught the killing blow");
    assert!(
        a.equipment
            .as_ref()
            .is_none_or(|q| q.slots.iter().all(Option::is_none)),
        "and was used up"
    );
}

#[test]
fn on_hit_procs_lifesteal_and_reflection() {
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "item.fang", 100.0, 101.0, 0),
        placed("c", "item.leech", 101.0, 101.0, 0),
        placed("f", "unit.warrior", 104.0, 100.0, 2),
        placed("g", "unit.warrior", 104.0, 101.0, 2),
    ];
    let script = Script::new(
        "items-on-hit",
        wand_spec(),
        entities,
        200,
        vec![(1, 1, pick(1, 2, false)), (2, 1, pick(1, 3, true))],
    )
    .set(30, "a", json!({"hp": 100}));
    let played = script.play();
    // Without the items, the hero only loses health to the two warriors.
    let mut bare = Script::new(
        "items-on-hit-bare",
        wand_spec(),
        script.entities.clone(),
        200,
        vec![],
    );
    bare.mutations = script.mutations.clone();
    let bare = bare.play();
    let damage_dealt = |p: &Played| {
        (max_hp(p, "f") - p.hp("f"))
            + (max_hp(p, "g") - p.hp("g"))
            + u32::from(!p.alive("f")) * 1000
            + u32::from(!p.alive("g")) * 1000
    };
    assert!(
        damage_dealt(&played) > damage_dealt(&bare),
        "procs and reflection hurt more"
    );
    assert!(
        played.hp("a") > bare.hp("a"),
        "lifesteal keeps the hero healthier"
    );
    let _ = EventKind::Damaged;
}

#[test]
fn item_use_is_blocked_by_statuses() {
    let mut spec = wand_spec();
    give(
        &mut spec,
        spell(
            "ability.test.frog",
            "unit",
            &["enemy"],
            12,
            20,
            300,
            4,
            4,
            json!([
        {"op": "status", "target": "target", "id": "frogged", "amount": 120, "polarity": "negative", "dispel": true, "itemBlocked": true}]),
        ),
        "frog",
    );
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "item.wand", 100.0, 101.0, 0),
        placed("c", "unit.caster", 106.0, 100.0, 2),
    ];
    let played = Script::new(
        "items-blocked",
        spec,
        entities,
        60,
        vec![
            (1, 1, pick(1, 2, false)),
            (10, 2, cast(3, "frog", 1)),
            (40, 1, use_item(1, 0)),
        ],
    )
    .play();
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "Cannot use items while busy or stunned"
                || e == "This item cannot be used"),
        "{:?}",
        played.errors()
    );
}
