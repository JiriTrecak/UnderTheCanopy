//! Deliveries and lasting effects: projectiles, chains, lines, charges, swarms, persistent
//! zones and toggles, auras and weapon casts. Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_sim::abilities::EventKind;
use serde_json::json;

#[test]
fn projectiles_travel_and_follow_their_target() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.pebble",
                "unit",
                &["enemy"],
                16,
                45,
                60,
                10,
                8,
                json!([
        {"op": "damage", "target": "target", "amount": 30, "damageType": "spell"},
        {"op": "status", "target": "target", "id": "stunned", "amount": 80, "heroDuration": 40, "polarity": "negative", "dispel": false, "stun": true}]),
            ),
            json!({"delivery": {"kind": "projectile", "speed": 12}}),
        ),
        "pebble",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.hero", 112.0, 100.0, 2),
        placed("c", "unit.warrior", 112.0, 106.0, 2),
    ];
    let played = Script::new(
        "delivery-projectile",
        spec,
        entities,
        160,
        vec![
            (1, 1, cast(1, "pebble", 2)),
            (5, 2, moving(2, 120, 110)),
            (80, 1, cast(1, "pebble", 3)),
        ],
    )
    .play();
    let launched = played.of(EventKind::Projectile);
    let impacts = played.of(EventKind::Impact);
    assert_eq!((launched.len(), impacts.len()), (2, 2));
    assert!(
        impacts[0].tick > launched[0].tick,
        "the projectile takes time"
    );
    let stuns: Vec<_> = played.of(EventKind::StatusApplied);
    assert_eq!(stuns.len(), 2);
    // Heroes take the hero duration.
    assert_eq!(stuns[0].duration_ticks, Some(40));
    assert_eq!(stuns[1].duration_ticks, Some(80));
}

#[test]
fn chains_bounce_with_falloff_and_skip_full_health() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.zap",
                "unit",
                &["enemy"],
                14,
                45,
                60,
                8,
                8,
                json!([
        {"op": "damage", "target": "target", "amount": 80, "damageType": "spell"}]),
            ),
            json!({"delivery": {"kind": "chain", "bounces": 4, "radius": 6, "intervalTicks": 6, "retentionPermille": 750}}),
        ),
        "zap",
    );
    let mut mend = with(
        spell(
            "ability.test.mend",
            "unit",
            &["ally"],
            14,
            45,
            60,
            8,
            8,
            json!([{"op": "heal", "target": "target", "amount": 60}]),
        ),
        json!({"delivery": {"kind": "chain", "bounces": 3, "radius": 6, "intervalTicks": 6, "retentionPermille": 800, "skipFullHealth": true}}),
    );
    mend["targeting"]["allowSelf"] = json!(true);
    give(&mut spec, mend, "mend");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
        placed("c", "unit.warrior", 114.0, 101.0, 2),
        placed("d", "unit.warrior", 112.0, 104.0, 2),
        placed("e", "unit.warrior", 118.0, 104.0, 2),
        placed("f", "unit.archer", 96.0, 100.0, 1),
        placed("g", "unit.archer", 96.0, 104.0, 1),
    ];
    let played = Script::new(
        "delivery-chain",
        spec,
        entities,
        120,
        vec![(1, 1, cast(1, "zap", 2)), (40, 1, cast(1, "mend", 6))],
    )
    .set(30, "f", json!({"hp": 20}))
    .set(30, "g", json!({"hp": 30}))
    .play();
    let zaps: Vec<i64> = played
        .of(EventKind::Damaged)
        .iter()
        .filter_map(|e| e.amount)
        .collect();
    assert!(zaps.len() >= 3, "{}", played.dump());
    assert_eq!(zaps[0], 80);
    assert_eq!(zaps[1], 60, "three quarters retained per bounce");
    let heals = played.of(EventKind::Healed);
    assert_eq!(
        heals.len(),
        2,
        "only the wounded archers are healed: {}",
        played.dump()
    );
}

#[test]
fn lines_sweep_once_per_unit() {
    let mut spec = Spec::default();
    let mut wave = with(
        spell(
            "ability.test.wave",
            "point",
            &["enemy"],
            12,
            45,
            60,
            8,
            8,
            json!([
        {"op": "damage", "target": "target", "amount": 50, "damageType": "spell"}]),
        ),
        json!({"delivery": {"kind": "line", "speed": 16, "length": 14, "width": 1.5}}),
    );
    wave["targeting"]["radius"] = json!(2);
    give(&mut spec, wave, "wave");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 105.0, 100.0, 2),
        placed("c", "unit.warrior", 110.0, 101.0, 2),
        placed("d", "unit.warrior", 108.0, 104.0, 2),
        placed("e", "unit.warrior", 116.0, 100.0, 2),
    ];
    let played = Script::new(
        "delivery-line",
        spec,
        entities,
        80,
        vec![(1, 1, cast_at_point(1, "wave", 106, 100))],
    )
    .play();
    assert_eq!(played.at(EventKind::Damaged, "b").len(), 1);
    assert_eq!(played.at(EventKind::Damaged, "c").len(), 1);
    assert!(
        played.at(EventKind::Damaged, "d").is_empty(),
        "outside the width"
    );
    assert!(
        played.at(EventKind::Damaged, "e").is_empty(),
        "beyond the length"
    );
}

#[test]
fn charges_carry_the_caster_to_the_target() {
    let mut spec = Spec::default();
    let mut rush = with(
        spell(
            "ability.test.rush",
            "unit",
            &["enemy"],
            20,
            55,
            100,
            8,
            8,
            json!([
        {"op": "damage", "target": "target", "amount": 70, "damageType": "damage.blade"},
        {"op": "status", "target": "target", "id": "impact", "amount": 30, "polarity": "negative", "dispel": true, "stun": true}]),
        ),
        json!({"delivery": {"kind": "charge", "speed": 32, "maxTicks": 80}}),
    );
    rush["targeting"]["filter"] = json!({"locomotion": ["ground"]});
    give(&mut spec, rush, "rush");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 114.0, 101.0, 2),
        placed("c", "unit.wasp", 104.0, 104.0, 2),
    ];
    let played = Script::new(
        "delivery-charge",
        spec,
        entities,
        100,
        vec![(1, 1, cast(1, "rush", 3)), (2, 1, cast(1, "rush", 2))],
    )
    .play();
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "Target does not match spell filters")
    );
    assert_eq!(played.count(EventKind::Impact), 1);
    assert_eq!(played.at(EventKind::StatusApplied, "b").len(), 1);
    let a = played.entity("a").position;
    assert!(a.x > 110_000, "the caster arrived next to its target");
}

#[test]
fn swarms_drain_return_and_heal() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.gnats",
                "self",
                &["ally"],
                0,
                150,
                4800,
                10,
                8,
                json!([
        {"op": "drain", "target": "target", "amount": 10, "resource": "health", "restoreCaster": true, "damageType": "spell"}]),
            ),
            json!({"delivery": {"kind": "swarm", "count": 6, "durationTicks": 200, "speed": 18, "radius": 16, "launchIntervalTicks": 8, "attackIntervalTicks": 20,
            "maxPerTarget": 2, "returnThreshold": 20, "returnPermille": 750, "maxHitsPerTrip": 4, "returnTimeoutTicks": 100}}),
        ),
        "gnats",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
        placed("c", "unit.warrior", 108.0, 106.0, 2),
        placed("d", "unit.archer", 104.0, 112.0, 2),
    ];
    let played = Script::new(
        "delivery-swarm",
        spec,
        entities,
        360,
        vec![(1, 1, cast(1, "gnats", 1))],
    )
    .set(0, "a", json!({"hp": 60}))
    .play();
    assert_eq!(played.count(EventKind::Projectile), 6, "six gnats launch");
    assert!(played.count(EventKind::Drained) >= 10);
    assert!(played.count(EventKind::Returned) >= 1);
    assert!(
        played.count(EventKind::Healed) >= 1,
        "returning gnats heal the caster"
    );
    assert!(
        played.world.state().abilities.deliveries.is_empty(),
        "every gnat is gone by the end"
    );
}

#[test]
fn persistent_zones_pulse_and_expire() {
    let mut spec = Spec::default();
    let mut spores = with(
        spell(
            "ability.test.spores",
            "point",
            &["enemy"],
            18,
            65,
            600,
            10,
            8,
            json!([
        {"op": "damage", "target": "target", "amount": 8, "damageType": "spell"},
        {"op": "status", "target": "target", "id": "choke", "amount": 50, "polarity": "negative", "dispel": true, "modifiers": {"moveSpeedPermille": -350}}]),
        ),
        json!({"persistent": {"durationTicks": 160, "intervalTicks": 20, "anchor": "point", "endsWithCaster": false}}),
    );
    spores["targeting"]["radius"] = json!(4);
    give(&mut spec, spores, "spores");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
    ];
    let played = Script::new(
        "delivery-zone",
        spec,
        entities,
        220,
        vec![(1, 1, cast_at_point(1, "spores", 110, 101))],
    )
    .play();
    // Pulses at release and every 20 ticks within the 160-tick lifetime.
    let pulses = played.count(EventKind::Wave);
    assert!((7..=9).contains(&pulses), "{pulses} pulses");
    assert!(
        played.world.state().abilities.instances.is_empty(),
        "the zone expired"
    );
}

#[test]
fn toggles_pay_upkeep_and_switch_off() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.mantle",
                "self",
                &["ally"],
                0,
                10,
                40,
                4,
                4,
                json!([
        {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 4, "relations": ["enemy"]}, "amount": 10, "damageType": "spell"},
        {"op": "status", "target": "caster", "id": "mantle", "amount": 80, "polarity": "positive", "dispel": false, "lifetime": "instance", "modifiers": {"armor": 2}}]),
            ),
            json!({"persistent": {"durationTicks": 144000, "intervalTicks": 20, "anchor": "caster", "endsWithCaster": true, "upkeepMana": 25, "toggle": true}}),
        ),
        "mantle",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 103.0, 100.0, 2),
    ];
    let off = Script::new(
        "delivery-toggle",
        spec.clone(),
        entities.clone(),
        200,
        vec![(1, 1, cast(1, "mantle", 1)), (60, 1, cast(1, "mantle", 1))],
    )
    .play();
    assert!(
        off.world.state().abilities.instances.is_empty(),
        "toggled off"
    );
    assert!(
        statuses(&off.world, "a").is_empty(),
        "instance statuses end with it"
    );
    let starved = Script::new(
        "delivery-upkeep",
        spec,
        entities,
        400,
        vec![(1, 1, cast(1, "mantle", 1))],
    )
    .play();
    assert!(
        starved.world.state().abilities.instances.is_empty(),
        "upkeep exhausts the mana"
    );
    assert!(starved.mana("a") < 25);
}

#[test]
fn auras_follow_their_bearer() {
    let mut spec = Spec::default();
    spec.abilities.push(with(passive("ability.test.thorn", json!([
        {"op": "status", "target": "target", "id": "thornward", "amount": 1, "polarity": "positive", "dispel": false, "rankedModifiers": {"armor": {"rankParameter": "armor"}}}])),
        json!({"aura": {"radius": 6, "meleeOnly": false}, "ranks": [{"armor": 3}]})));
    spec.hero.push(bind("thorn", "ability.test.thorn", 1));
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 103.0, 100.0, 1),
        placed("e", "unit.archer", 100.0, 120.0, 1),
        placed("f", "unit.warrior", 104.0, 102.0, 2),
    ];
    let played = Script::new("delivery-aura", spec.clone(), entities.clone(), 30, vec![]).play();
    assert_eq!(
        statuses(&played.world, "a"),
        vec!["thornward"],
        "{:?}",
        played.entity("a").statuses
    );
    assert_eq!(statuses(&played.world, "b"), vec!["thornward"]);
    assert!(statuses(&played.world, "e").is_empty(), "out of the radius");
    assert!(
        statuses(&played.world, "f").is_empty(),
        "enemies are not allies"
    );
    let walked = Script::new(
        "delivery-aura-walk",
        spec,
        entities,
        200,
        vec![(1, 1, moving(3, 99, 103))],
    )
    .play();
    assert_eq!(
        statuses(&walked.world, "e"),
        vec!["thornward"],
        "{:?}",
        walked.entity("e").position
    );
}

#[test]
fn weapon_casts_enhance_the_next_shot() {
    let mut spec = Spec::default();
    spec.abilities.push(with(spell("ability.test.frostshot", "unit", &["enemy"], 30, 10, 0, 0, 0, json!([])), json!({
        "weaponCast": {"bonus": {"rankParameter": "bonus"}, "weapon": "projectile", "status": "chilled"},
        "autocast": {"intervalTicks": 1, "enabledByDefault": false},
        "ranks": [{"bonus": 5, "slow": -300}],
        "statuses": [{"op": "status", "target": "target", "id": "chilled", "amount": 200, "heroDuration": 60, "polarity": "negative", "dispel": true,
            "rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slow"}}}]})));
    spec.archer.push(bind("frost", "ability.test.frostshot", 1));
    spec.warrior
        .push(bind("frost", "ability.test.frostshot", 1));
    let entities = vec![
        placed("a", "unit.archer", 100.0, 100.0, 1),
        placed("b", "unit.golem", 112.0, 100.0, 2),
        placed("c", "unit.warrior", 101.0, 103.0, 1),
    ];
    let played = Script::new(
        "delivery-weapon",
        spec,
        entities,
        200,
        vec![
            (2, 1, cast(1, "frost", 2)),
            (3, 1, cast(3, "frost", 2)),
            (
                100,
                1,
                json!({"type": "abilityAutocast", "actor": 1, "binding": "frost", "enabled": true}),
            ),
        ],
    )
    .play();
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "This unit cannot use that weapon ability"
                || e == "Ability requires a different weapon kind"),
        "{:?}",
        played.errors()
    );
    assert_eq!(played.count(EventKind::WeaponOrdered), 1);
    let enhanced = played.count(EventKind::EnhancedHit);
    assert!(
        enhanced >= 2,
        "manual then autocast shots: {}",
        played.dump()
    );
    assert!(
        played
            .at(EventKind::StatusApplied, "b")
            .iter()
            .all(|e| e.status.as_deref() == Some("chilled"))
    );
}
