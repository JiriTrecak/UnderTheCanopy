//! Triggers and conditions: interval, weapon release and hit, melee damage, damage, death
//! and kill reactions; status-source executors; combat modifiers; every condition kind;
//! the per-tick reaction budget. Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_sim::abilities::EventKind;
use serde_json::{Value, json};

fn veteran_spec() -> Spec {
    let mut spec = Spec::default();
    let veteran = with(
        passive("ability.test.veteran", json!([])),
        json!({
        "combatModifiers": {"critical": {"chancePermille": 300, "multiplierPermille": {"rankParameter": "crit"}}, "evasionPermille": 250,
            "cleave": {"damagePermille": 300, "radius": 3, "arcDegrees": 120, "maxTargets": 4}},
        "ranks": [{"crit": 1750, "haste": 150, "heal": 30}],
        "triggers": [
            {"id": "bash", "event": "weaponHit", "chancePermille": 300, "operations": [
                {"op": "damage", "target": "target", "amount": 20, "damageType": "spell"},
                {"op": "status", "target": "target", "id": "bashed", "amount": 40, "polarity": "negative", "dispel": false, "stun": true}]},
            {"id": "rhythm", "event": "weaponRelease", "every": 3, "blockedBySilence": false, "operations": [
                {"op": "status", "target": "caster", "id": "momentum", "amount": 160, "polarity": "positive", "dispel": true, "rankedModifiers": {"attackSpeedPermille": {"rankParameter": "haste"}}}]},
            {"id": "retaliate", "event": "meleeDamaged", "cooldownTicks": 80, "operations": [
                {"op": "damage", "target": "target", "amount": 15, "damageType": "spell"}]},
            {"id": "feast", "event": "kill", "filter": {"summoned": false}, "operations": [
                {"op": "heal", "target": "caster", "amount": {"rankParameter": "heal"}}]},
            {"id": "lastStand", "event": "death", "operations": [
                {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"], "maxTargets": 16}, "amount": 60, "damageType": "spell"}]}
        ]}),
    );
    spec.abilities.push(veteran);
    spec.hero.push(bind("veteran", "ability.test.veteran", 1));
    spec.warrior
        .push(bind("veteran", "ability.test.veteran", 1));
    spec
}

#[test]
fn combat_triggers_and_modifiers_fire() {
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 102.0, 100.0, 2),
        placed("c", "unit.warrior", 102.0, 102.0, 2),
        placed("d", "unit.warrior", 101.0, 98.0, 2),
        placed("e", "unit.warrior", 98.0, 101.0, 1),
    ];
    let played = Script::new("trigger-combat", veteran_spec(), entities, 400, vec![]).play();
    for kind in [
        EventKind::CriticalStrike,
        EventKind::Evaded,
        EventKind::Cleaved,
        EventKind::WeaponRelease,
        EventKind::Kill,
        EventKind::Death,
    ] {
        assert!(played.count(kind) > 0, "{kind:?} never fired");
    }
    let statuses: Vec<_> = played
        .of(EventKind::StatusApplied)
        .iter()
        .filter_map(|e| e.status.clone())
        .collect();
    assert!(statuses.iter().any(|s| s == "bashed"), "weapon hits bash");
    assert!(
        statuses.iter().any(|s| s == "momentum"),
        "every third release"
    );
    assert!(
        played
            .of(EventKind::Healed)
            .iter()
            .all(|e| e.amount <= Some(30))
    );
}

#[test]
fn interval_triggers_respect_silence() {
    let mut spec = Spec::default();
    spec.abilities.push(with(passive("ability.test.halo", json!([])), json!({"ranks": [{"damage": 6}],
        "triggers": [{"id": "pulse", "event": "interval", "intervalTicks": 20, "blockedBySilence": true,
            "operations": [{"op": "damage", "target": "target", "query": {"center": "caster", "radius": 4, "relations": ["enemy"]}, "amount": {"rankParameter": "damage"}, "damageType": "spell"}]}]})));
    spec.hero.push(bind("halo", "ability.test.halo", 1));
    give(
        &mut spec,
        spell(
            "ability.test.hush",
            "unit",
            &["enemy"],
            12,
            10,
            100,
            2,
            2,
            json!([
        {"op": "status", "target": "target", "id": "hush", "amount": 60, "polarity": "negative", "dispel": true, "silence": true}]),
        ),
        "hush",
    );
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 103.0, 100.0, 2),
        placed("c", "unit.caster", 106.0, 100.0, 2),
    ];
    let quiet = Script::new(
        "trigger-interval",
        spec.clone(),
        entities.clone(),
        250,
        vec![],
    )
    .play();
    let hushed = Script::new(
        "trigger-interval-silenced",
        spec,
        entities,
        250,
        vec![(50, 2, cast(3, "hush", 1))],
    )
    .play();
    let a = quiet.id("a");
    let pulses = |p: &Played| {
        p.events
            .iter()
            .filter(|e| e.kind == EventKind::Interval && e.caster == a)
            .count()
    };
    assert!(pulses(&quiet) >= 12, "{}", quiet.dump());
    assert!(
        pulses(&hushed) + 2 <= pulses(&quiet),
        "silence skips pulses: {} vs {}",
        pulses(&hushed),
        pulses(&quiet)
    );
}

#[test]
fn status_source_triggers_and_while_status_grants() {
    let mut spec = Spec::default();
    let mut rot = with(
        spell(
            "ability.test.rot",
            "unit",
            &["enemy"],
            13,
            50,
            4800,
            12,
            8,
            json!([
        {"op": "status", "target": "target", "id": "rot", "amount": 0, "lifetime": "untilDeath", "polarity": "negative", "dispel": false,
         "periodic": {"intervalTicks": 20, "damage": 20, "damageType": "spell"}}]),
        ),
        json!({"triggers": [{"id": "bloom", "event": "death", "executor": "statusSource", "whileStatus": "rot",
            "operations": [{"op": "heal", "target": "caster", "amount": 100}]}]}),
    );
    rot["targeting"]["filter"] = json!({"heroes": false, "maxLevel": 5});
    give(&mut spec, rot, "rot");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.caster", 109.0, 100.0, 2),
    ];
    let played = Script::new(
        "trigger-source",
        spec,
        entities,
        300,
        vec![(1, 1, cast(1, "rot", 2))],
    )
    .set(0, "a", json!({"hp": 30}))
    .play();
    let rot = played.at(EventKind::StatusApplied, "b");
    assert!(rot[0].until_death);
    assert!(!played.alive("b"), "the rot kills");
    let bloom = played.at(EventKind::Healed, "a");
    assert_eq!(
        bloom.len(),
        1,
        "the caster blooms from its victim's death: {}",
        played.dump()
    );
}

#[test]
fn damage_triggers_answer_attackers_while_a_status_lasts() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.thorns",
                "unit",
                &["ally"],
                12,
                25,
                50,
                4,
                4,
                json!([
        {"op": "status", "target": "target", "id": "thorny", "amount": 200, "polarity": "positive", "dispel": true}]),
            ),
            json!({"triggers": [
            {"id": "prick", "event": "damaged", "whileStatus": "thorny", "operations": [{"op": "damage", "target": "target", "amount": 4, "damageType": "spell"}]},
            {"id": "stab", "event": "weaponHit", "whileStatus": "thorny", "every": 2, "operations": [{"op": "damage", "target": "target", "amount": 7, "damageType": "spell"}]}]}),
        ),
        "thorns",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 103.0, 100.0, 1),
        placed("c", "unit.warrior", 105.0, 100.0, 2),
    ];
    let played = Script::new(
        "trigger-thorns",
        spec,
        entities,
        300,
        vec![(1, 1, cast(1, "thorns", 2))],
    )
    .play();
    let at_c: Vec<i64> = played
        .at(EventKind::Damaged, "c")
        .iter()
        .filter_map(|e| e.amount)
        .collect();
    assert!(at_c.contains(&4), "pricks answer hits: {}", played.dump());
    assert!(at_c.contains(&7), "every second weapon hit stabs");
    let last = played
        .of(EventKind::Damaged)
        .iter()
        .map(|e| e.tick)
        .max()
        .unwrap();
    let applied = played.of(EventKind::StatusApplied)[0].tick;
    assert!(last <= applied + 200, "nothing after the status ends");
}

#[test]
fn death_triggers_raise_summons_from_marked_victims() {
    let mut spec = Spec::default();
    spec.abilities.push(with(spell("ability.test.dusk", "self", &["ally"], 0, 0, 0, 0, 0, json!([
        {"op": "status", "target": "caster", "id": "duskQuiver", "amount": 800, "polarity": "positive", "dispel": false, "lifetime": "instance",
         "combatModifiers": {"attackBonus": {"status": "duskMark", "amount": 10, "manaCost": 6, "weapon": "projectile", "blockedBySilence": true}}}])),
        json!({"persistent": {"durationTicks": 144000, "intervalTicks": 400, "anchor": "caster", "endsWithCaster": true, "toggle": true},
            "statuses": [{"op": "status", "target": "target", "filter": {"heroes": false, "natures": ["organic"]}, "id": "duskMark", "amount": 80, "polarity": "negative", "dispel": true}],
            "triggers": [{"id": "rise", "event": "death", "executor": "statusSource", "whileStatus": "duskMark",
                "operations": [{"op": "summon", "target": "point", "amount": 1, "definition": "unit.imp", "durationTicks": 400, "replace": false, "radius": 2}]}]})));
    spec.archer.push(bind("dusk", "ability.test.dusk", 1));
    let entities = vec![
        placed("a", "unit.archer", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 108.0, 100.0, 2),
    ];
    let played = Script::new(
        "trigger-dusk",
        spec,
        entities,
        400,
        vec![(2, 1, cast(1, "dusk", 1))],
    )
    .play();
    assert!(played.count(EventKind::EnhancedHit) >= 3);
    assert!(!played.alive("b"));
    let risen = played.of(EventKind::Summoned);
    assert_eq!(risen.len(), 1);
    let imp = played
        .world
        .state()
        .entities
        .get(risen[0].spawned[0])
        .expect("imp");
    assert_eq!(
        imp.owner,
        played.entity("a").owner,
        "the mark's source owns the summon"
    );
}

fn condition_spell(id: &str, condition: Value) -> Value {
    let mut s = spell(
        id,
        "unit",
        &["ally", "enemy"],
        20,
        0,
        0,
        2,
        2,
        json!([{"op": "damage", "target": "target", "amount": 1, "damageType": "spell"}]),
    );
    s["targeting"]["allowSelf"] = json!(true);
    s["targeting"]["condition"] = condition;
    s
}

#[test]
fn every_condition_kind_gates_targets() {
    let mut spec = Spec::default();
    let cases = [
        (
            "wounded",
            json!({"kind": "resource", "of": "target", "resource": "health", "measure": "fractionPermille", "comparison": "lt", "value": 500}),
        ),
        (
            "manaful",
            json!({"kind": "resource", "of": "caster", "resource": "mana", "measure": "current", "comparison": "gte", "value": 10}),
        ),
        (
            "players",
            json!({"kind": "controller", "of": "target", "is": "player"}),
        ),
        (
            "neutral",
            json!({"kind": "controller", "of": "target", "is": "neutral"}),
        ),
        (
            "enemies",
            json!({"kind": "relation", "of": "target", "to": "caster", "is": "enemy"}),
        ),
        (
            "grounded",
            json!({"kind": "matches", "of": "target", "filter": {"locomotion": ["ground"]}}),
        ),
        (
            "both",
            json!({"kind": "all", "conditions": [
            {"kind": "relation", "of": "target", "to": "caster", "is": "enemy"},
            {"kind": "matches", "of": "target", "filter": {"natures": ["mechanical"]}}]}),
        ),
        (
            "either",
            json!({"kind": "any", "conditions": [
            {"kind": "controller", "of": "target", "is": "neutral"},
            {"kind": "matches", "of": "target", "filter": {"heroes": true}}]}),
        ),
    ];
    for (name, condition) in &cases {
        give(
            &mut spec,
            condition_spell(&format!("ability.test.{name}"), condition.clone()),
            name,
        );
    }
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 104.0, 100.0, 2),
        placed("c", "unit.golem", 104.0, 104.0, 2),
        placed("d", "unit.wasp", 100.0, 104.0, 2),
        placed("e", "unit.warrior", 96.0, 104.0, 1),
        placed("f", "unit.hero", 96.0, 100.0, 1),
    ];
    // (binding, target id, accepted)
    let expectations: [(&str, u32, bool); 14] = [
        ("wounded", 2, false),
        ("wounded", 6, true),
        ("manaful", 2, true),
        ("players", 2, true),
        ("players", 6, true),
        ("neutral", 2, false),
        ("enemies", 6, false),
        ("grounded", 4, false),
        ("grounded", 2, true),
        ("both", 3, true),
        ("both", 2, false),
        ("either", 3, false),
        ("either", 6, true),
        ("either", 2, false),
    ];
    for (i, (binding, target, accepted)) in expectations.iter().enumerate() {
        let played = Script::new(
            "conditions",
            spec.clone(),
            entities.clone(),
            4,
            vec![(1, 1, cast(1, binding, *target))],
        )
        .set(0, "f", json!({"hp": 10}))
        .play();
        let ok = played.count(EventKind::Accepted) == 1;
        assert_eq!(
            ok,
            *accepted,
            "case {i}: {binding} at {target}: {:?}",
            played.errors()
        );
    }
}

#[test]
fn lifecycle_reactions_are_bounded_per_tick() {
    // 160 warriors die to one blast, each with a death reaction: at most 128 reactions run
    // per tick and the rest wait for the next.
    let mut spec = Spec::default();
    spec.abilities.push(with(passive("ability.test.spite", json!([])), json!({
        "triggers": [{"id": "spite", "event": "death", "operations": [
            {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 1, "relations": ["enemy"]}, "amount": 1, "damageType": "spell"}]}]})));
    spec.warrior.push(bind("spite", "ability.test.spite", 1));
    let mut blast = spell(
        "ability.test.blast",
        "point",
        &["enemy"],
        30,
        0,
        0,
        2,
        2,
        json!([{"op": "damage", "target": "target", "amount": 5000, "damageType": "spell"}]),
    );
    blast["targeting"]["radius"] = json!(12);
    blast["targeting"]["maxTargets"] = json!(200);
    give(&mut spec, blast, "blast");
    let mut entities = vec![placed("a", "unit.caster", 96.0, 108.0, 1)];
    let names: Vec<&'static str> = (0..160)
        .map(|i| &*Box::leak(format!("w{i:03}").into_boxed_str()))
        .collect();
    for (i, name) in names.iter().enumerate() {
        entities.push(placed(
            name,
            "unit.warrior",
            100.0 + (i % 16) as f64,
            104.0 + (i / 16) as f64,
            2,
        ));
    }
    let played = Script::new(
        "trigger-budget",
        spec,
        entities,
        20,
        vec![(1, 1, cast_at_point(1, "blast", 108, 108))],
    )
    .play();
    let mut per_tick = std::collections::BTreeMap::<u32, usize>::new();
    for e in played.of(EventKind::Death) {
        *per_tick.entry(e.tick).or_default() += 1;
    }
    assert_eq!(
        per_tick.values().sum::<usize>(),
        160,
        "{:?}",
        played.errors()
    );
    assert_eq!(
        per_tick.values().copied().collect::<Vec<_>>(),
        vec![128, 32],
        "{per_tick:?}"
    );
}

#[test]
fn auras_may_carry_damage_triggers() {
    // The old rule scanned trigger operations too, so an aura could not also pulse damage.
    let mut spec = Spec::default();
    spec.abilities.push(with(
        passive(
            "ability.test.ember",
            json!([{"op": "status", "target": "target", "id": "warmth", "amount": 1, "polarity": "positive", "dispel": false, "modifiers": {"armor": 1}}]),
        ),
        json!({"aura": {"radius": 4, "meleeOnly": false},
            "triggers": [{"id": "flare", "event": "interval", "intervalTicks": 20, "operations": [
                {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 4, "relations": ["enemy"]}, "amount": 3, "damageType": "spell"}]}]}),
    ));
    spec.hero.push(bind("ember", "ability.test.ember", 1));
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 102.0, 100.0, 1),
        placed("c", "unit.critter", 140.0, 140.0, 0),
    ];
    let played = Script::new("trigger-aura", spec, entities, 60, vec![]).play();
    assert_eq!(statuses(&played.world, "b"), vec!["warmth"]);
    assert!(played.count(EventKind::Interval) >= 2);
}
