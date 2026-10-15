//! Statuses: controls (stun, root, silence, disarm, item block, slows, taunt), stacking,
//! periodic damage, shields and mana shields, damage taken, immunities, ethereal bodies,
//! concealment and detection, and forms (combat profile, scale, flight, grounding).
//! Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_sim::{abilities::EventKind, stats::move_speed_permille};
use serde_json::json;

#[test]
fn stuns_halt_units_and_heroes_resist_longer_stuns() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.stomp",
                "self",
                &["ally"],
                0,
                50,
                320,
                10,
                8,
                json!([
        {"op": "status", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"]}, "filter": {"locomotion": ["ground"]}, "id": "stunned",
         "amount": {"rankParameter": "stun"}, "heroDuration": {"rankParameter": "heroStun"}, "polarity": "negative", "dispel": false, "stun": true}]),
            ),
            json!({"ranks": [{"stun": 80, "heroStun": 40}]}),
        ),
        "stomp",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 102.0, 100.0, 2),
        placed("c", "unit.wasp", 100.0, 102.0, 2),
        placed("d", "unit.hero", 98.0, 101.0, 2),
    ];
    let played = Script::new(
        "status-stun",
        spec,
        entities,
        60,
        vec![(3, 1, cast(1, "stomp", 1))],
    )
    .play();
    let b = played.at(EventKind::StatusApplied, "b");
    let d = played.at(EventKind::StatusApplied, "d");
    assert_eq!(b[0].duration_ticks, Some(80));
    assert_eq!(d[0].duration_ticks, Some(40));
    assert!(
        played.at(EventKind::StatusApplied, "c").is_empty(),
        "flyers are filtered out"
    );
    // Stunned units neither attack nor move.
    let stunned_at = b[0].tick;
    let b_unit = played.entity("b").unit.as_ref().unwrap();
    assert!(b_unit.combat.attack.is_none() || stunned_at + 80 <= played.world.tick());
    // The owner's view says so (the idle-worker key skips stunned workers).
    let b_entity = played.entity("b");
    let view = played.world.view(b_entity.owner);
    let seen = view
        .entity(b_entity.id)
        .and_then(|e| e.private.as_ref())
        .expect("own unit");
    assert!(seen.stunned);
    assert!(!seen.contained && !seen.releasing);
}

#[test]
fn roots_disarms_and_periodic_damage_until_dispelled() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.snare",
            "unit",
            &["enemy"],
            16,
            45,
            100,
            10,
            8,
            json!([
        {"op": "status", "target": "target", "id": "snared", "amount": 120, "heroDuration": 80,
         "polarity": "negative", "dispel": true, "modifiers": {"rooted": true}, "disarm": true,
         "periodic": {"intervalTicks": 20, "damage": 12, "damageType": "spell"}}]),
        ),
        "snare",
    );
    give(
        &mut spec,
        spell(
            "ability.test.purge",
            "unit",
            &["ally", "enemy"],
            16,
            20,
            20,
            4,
            4,
            json!([
        {"op": "dispel", "target": "target", "amount": 30, "damageType": "spell", "polarity": "negative"}]),
        ),
        "purge",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
    ];
    let played = Script::new(
        "status-snare",
        spec,
        entities,
        120,
        vec![
            (1, 1, cast(1, "snare", 2)),
            (30, 2, moving(2, 120, 100)),
            (70, 1, cast(1, "purge", 2)),
        ],
    )
    .set(60, "a", json!({"mana": 100}))
    .play();
    // Periodic damage every 20 ticks from 15: two ticks before the dispel at 73.
    let b = played.entity("b");
    let max = played
        .world
        .catalog()
        .get(b.definition)
        .body
        .as_ref()
        .unwrap()
        .max_hp;
    assert!(played.hp("b") <= max - 24, "{} of {max}", played.hp("b"));
    assert_eq!(played.count(EventKind::Dispelled), 1);
    assert!(statuses(&played.world, "b").is_empty());
    // Rooted while snared: the move order waited.
    assert!(played.entity("b").position.x < 120_000);
}

#[test]
fn shields_mana_shields_damage_taken_and_immunities() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.sleep",
            "unit",
            &["enemy"],
            16,
            20,
            20,
            4,
            4,
            json!([
        {"op": "status", "target": "target", "id": "drowse", "amount": 200, "heroDuration": 100, "polarity": "negative", "dispel": true, "stun": true, "breakOnDamage": true}]),
        ),
        "sleep",
    );
    give(
        &mut spec,
        spell(
            "ability.test.bolt",
            "unit",
            &["enemy"],
            16,
            0,
            0,
            2,
            2,
            json!([{"op": "damage", "target": "target", "amount": 40, "damageType": "spell"}]),
        ),
        "bolt",
    );
    let mut shell = spell(
        "ability.test.shell",
        "unit",
        &["ally"],
        16,
        20,
        20,
        4,
        4,
        json!([
        {"op": "status", "target": "target", "id": "amberShell", "amount": 400, "polarity": "positive", "dispel": true, "shield": 30, "onShieldDepleted": "remove"},
        {"op": "status", "target": "target", "id": "bark", "amount": 400, "polarity": "positive", "dispel": true, "damageTakenPermille": {"spell": 1500}}]),
    );
    shell["targeting"]["allowSelf"] = json!(true);
    give(&mut spec, shell, "shell");
    give(
        &mut spec,
        spell(
            "ability.test.ward",
            "self",
            &["ally"],
            0,
            20,
            20,
            4,
            4,
            json!([
        {"op": "status", "target": "caster", "id": "nullify", "amount": 200, "polarity": "positive", "dispel": true, "spellImmunity": "hostile"}]),
        ),
        "ward",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.caster", 108.0, 100.0, 2),
        placed("c", "unit.caster", 108.0, 104.0, 2),
    ];
    let played = Script::new(
        "status-shields",
        spec,
        entities,
        140,
        vec![
            (1, 2, cast(2, "shell", 2)),
            (20, 1, cast(1, "bolt", 2)),
            (30, 1, cast(1, "bolt", 2)),
            (40, 2, cast(3, "ward", 3)),
            (60, 1, cast(1, "bolt", 3)),
            (70, 1, cast(1, "sleep", 2)),
            (80, 1, cast(1, "bolt", 2)),
        ],
    )
    .set(15, "a", json!({"mana": 200}))
    .play();
    let hits: Vec<i64> = played
        .at(EventKind::Damaged, "b")
        .iter()
        .filter_map(|e| e.amount)
        .collect();
    // 40 spell damage × 1.5 = 60; the first 30 go into the shell.
    assert_eq!(hits[0], 30, "{}", played.dump());
    assert_eq!(hits[1], 60);
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "Target is immune to this spell")
    );
    // Sleep breaks on damage.
    let drowse = played
        .at(EventKind::StatusApplied, "b")
        .into_iter()
        .find(|e| e.status.as_deref() == Some("drowse"))
        .expect("drowse");
    assert!(drowse.tick < 82);
    assert!(!statuses(&played.world, "b").contains(&"drowse".to_string()));
}

#[test]
fn stacks_build_per_source() {
    let mut spec = Spec::default();
    spec.abilities.push(with(passive("ability.test.pressure", json!([])), json!({
        "combatModifiers": {"attackBonus": {"status": "pressure", "amount": 0, "manaCost": 0, "weapon": "melee", "blockedBySilence": false}},
        "ranks": [{"bonus": 5, "slow": -50}],
        "statuses": [{"op": "status", "target": "target", "id": "pressure", "amount": 240, "polarity": "negative", "dispel": true,
            "stacking": {"max": 3, "scope": "source"}, "sourceAttackBonus": {"rankParameter": "bonus"}, "rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slow"}}}]})));
    spec.warrior
        .push(bind("pressure", "ability.test.pressure", 1));
    let entities = vec![
        placed("a", "unit.warrior", 100.0, 100.0, 1),
        placed("b", "unit.golem", 102.0, 100.0, 2),
        placed("c", "unit.warrior", 101.0, 103.0, 1),
    ];
    let played = Script::new("status-stacks", spec, entities, 150, vec![])
        .set(0, "b", json!({"hp": 400}))
        .play();
    let b = played.entity("b");
    let stacks: Vec<_> = b.statuses.iter().map(|s| s.stacks.unwrap_or(1)).collect();
    assert_eq!(stacks.len(), 2, "one status per attacker: {:?}", b.statuses);
    assert!(
        stacks.iter().all(|&n| n == 3),
        "capped at three: {stacks:?}"
    );
    assert!(played.count(EventKind::EnhancedHit) > 6);
}

#[test]
fn slows_change_movement_speed() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.chill",
            "unit",
            &["enemy"],
            16,
            0,
            0,
            2,
            2,
            json!([
        {"op": "status", "target": "target", "id": "chill", "amount": 400, "polarity": "negative", "dispel": true, "modifiers": {"moveSpeedPermille": -300}}]),
        ),
        "chill",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
    ];
    let script = Script::new(
        "status-slow",
        spec,
        entities,
        30,
        vec![(1, 1, cast(1, "chill", 2))],
    );
    let before = {
        let world = canopy_sim::World::new(script.content(), &script.map(), setup(false)).unwrap();
        move_speed_permille(world.catalog(), entity(&world, "b"))
    };
    let played = script.play();
    assert_eq!(
        move_speed_permille(played.world.catalog(), played.entity("b")),
        before * 700 / 1000
    );
}

#[test]
fn taunts_force_attacks_on_the_challenger() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.hiss",
                "self",
                &["ally"],
                0,
                65,
                520,
                16,
                20,
                json!([
        {"op": "status", "target": "target", "query": {"center": "caster", "radius": 12, "relations": ["enemy"], "maxTargets": 32}, "id": "challenged",
         "amount": 100, "heroDuration": 50, "polarity": "negative", "dispel": true, "taunt": true}]),
            ),
            json!({}),
        ),
        "hiss",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 108.0, 100.0, 2),
        placed("d", "unit.warrior", 104.0, 102.0, 1),
    ];
    let played = Script::new(
        "status-taunt",
        spec,
        entities,
        60,
        vec![(1, 1, cast(1, "hiss", 1)), (25, 2, moving(2, 120, 100))],
    )
    .play();
    let b = played.entity("b");
    let taunt = b.unit.as_ref().unwrap().taunt.as_ref().expect("taunted");
    assert_eq!(taunt.source, played.id("a"));
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "No eligible controlled actors" || e.contains("taunt")),
        "{:?}",
        played.errors()
    );
}

#[test]
fn ethereal_bodies_take_no_weapon_damage_but_more_spell_damage() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.banish",
            "unit",
            &["enemy", "ally"],
            12,
            25,
            50,
            4,
            4,
            json!([
        {"op": "status", "target": "target", "id": "banished", "amount": 160, "heroDuration": 80, "polarity": "negative", "dispel": true,
         "ethereal": true, "damageTakenPermille": {"spell": 1660}}]),
        ),
        "banish",
    );
    give(
        &mut spec,
        spell(
            "ability.test.zap",
            "unit",
            &["enemy"],
            12,
            10,
            10,
            2,
            2,
            json!([{"op": "damage", "target": "target", "amount": 20, "damageType": "spell"}]),
        ),
        "zap",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 106.0, 100.0, 2),
        placed("c", "unit.warrior", 104.0, 100.0, 1),
    ];
    let played = Script::new(
        "status-ethereal",
        spec,
        entities,
        120,
        vec![(1, 1, cast(1, "banish", 2)), (20, 1, cast(1, "zap", 2))],
    )
    .play();
    let zap = played.at(EventKind::Damaged, "b");
    assert_eq!(zap[0].amount, Some(33), "20 × 1.66");
    let b = played.entity("b");
    let max = played
        .world
        .catalog()
        .get(b.definition)
        .body
        .as_ref()
        .unwrap()
        .max_hp;
    assert_eq!(
        played.hp("b"),
        max - 33,
        "the ally warrior's blows pass through"
    );
}

#[test]
fn concealment_hides_until_detected_or_broken() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.veil",
            "self",
            &["ally"],
            0,
            25,
            50,
            4,
            4,
            json!([
        {"op": "status", "target": "caster", "id": "veil", "amount": 300, "polarity": "positive", "dispel": true,
         "concealment": {"fadeTicks": 16, "breakOnAttack": true, "breakOnCast": true, "attackBonus": 60}}]),
        ),
        "veil",
    );
    give(
        &mut spec,
        spell(
            "ability.test.zap",
            "unit",
            &["enemy"],
            16,
            0,
            0,
            2,
            2,
            json!([{"op": "damage", "target": "target", "amount": 5, "damageType": "spell"}]),
        ),
        "zap",
    );
    spec.abilities.push(with(passive("ability.test.keen", json!([
        {"op": "status", "target": "caster", "id": "detector", "amount": 1, "polarity": "positive", "dispel": false, "detectionRadius": 6}])),
        json!({"aura": {"radius": 1, "meleeOnly": false}})));
    spec.warrior.push(bind("keen", "ability.test.keen", 1));
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.caster", 110.0, 100.0, 2),
        placed("c", "unit.warrior", 112.0, 120.0, 2),
    ];
    let hidden = Script::new(
        "status-veil",
        spec.clone(),
        entities.clone(),
        60,
        vec![(1, 1, cast(1, "veil", 1)), (40, 2, cast(2, "zap", 1))],
    )
    .play();
    assert!(
        hidden.errors().iter().any(|e| e == "Target is not visible"),
        "{:?}",
        hidden.errors()
    );
    let detected = Script::new(
        "status-detect",
        spec.clone(),
        entities.clone(),
        160,
        vec![
            (1, 1, cast(1, "veil", 1)),
            (30, 2, moving(3, 103, 100)),
            (120, 2, cast(2, "zap", 1)),
        ],
    )
    .play();
    assert_eq!(
        detected.at(EventKind::Damaged, "a").len(),
        1,
        "the detector reveals: {:?}",
        detected.errors()
    );
    let broken = Script::new(
        "status-break",
        spec,
        entities,
        80,
        vec![
            (1, 1, cast(1, "veil", 1)),
            (30, 1, cast(1, "zap", 2)),
            (60, 2, cast(2, "zap", 1)),
        ],
    )
    .play();
    assert!(
        !statuses(&broken.world, "a").contains(&"veil".to_string()),
        "casting breaks the veil"
    );
    assert_eq!(broken.at(EventKind::Damaged, "a").len(), 1);
}

#[test]
fn forms_change_profile_and_flight() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.hunter",
                "self",
                &["ally"],
                0,
                75,
                400,
                12,
                8,
                json!([
        {"op": "status", "target": "caster", "id": "form", "amount": 200, "polarity": "positive", "dispel": false,
         "form": {"combatProfile": "unit.archer", "scale": 1.3}, "rankedModifiers": {"maxHp": {"rankParameter": "health"}}}]),
            ),
            json!({"ranks": [{"health": 150}]}),
        ),
        "hunter",
    );
    give(
        &mut spec,
        with(
            spell(
                "ability.test.wings",
                "self",
                &["ally"],
                0,
                25,
                0,
                10,
                8,
                json!([
        {"op": "status", "target": "caster", "id": "soaring", "amount": 80, "polarity": "positive", "dispel": true, "lifetime": "instance",
         "form": {"movement": {"locomotion": "air", "flightHeight": 7}}}]),
            ),
            json!({"persistent": {"durationTicks": 144000, "intervalTicks": 40, "anchor": "caster", "endsWithCaster": true, "toggle": true}}),
        ),
        "wings",
    );
    give(
        &mut spec,
        spell(
            "ability.test.ground",
            "unit",
            &["enemy"],
            12,
            20,
            200,
            4,
            4,
            json!([
        {"op": "status", "target": "target", "id": "grounded", "amount": 100, "polarity": "negative", "dispel": true, "form": {"movement": {"locomotion": "ground"}}}]),
        ),
        "ground",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.wasp", 108.0, 104.0, 2),
    ];
    let played = Script::new(
        "status-forms",
        spec.clone(),
        entities.clone(),
        40,
        vec![(1, 1, cast(1, "hunter", 1))],
    )
    .play();
    let caster_max = played
        .world
        .catalog()
        .get(played.entity("a").definition)
        .body
        .as_ref()
        .unwrap()
        .max_hp;
    let stats = canopy_sim::stats::Stats::of(played.world.catalog(), played.entity("a"), &[]);
    assert_eq!(stats.max_hp, caster_max + 150);
    let flying = Script::new(
        "status-wings",
        spec.clone(),
        entities.clone(),
        80,
        vec![(1, 1, cast(1, "wings", 1))],
    )
    .play();
    assert!(
        flying.entity("a").unit.as_ref().unwrap().flight.is_some(),
        "the caster takes off"
    );
    let grounded = Script::new(
        "status-grounded",
        spec,
        entities,
        80,
        vec![(1, 1, cast(1, "ground", 2))],
    )
    .play();
    assert!(grounded.at(EventKind::StatusApplied, "b").len() == 1);
    assert!(
        !canopy_sim::abilities::forms::airborne(grounded.world.catalog(), grounded.entity("b")),
        "the wasp is grounded"
    );
}
