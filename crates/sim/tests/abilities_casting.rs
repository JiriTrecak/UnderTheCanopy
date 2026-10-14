//! Casting: command validation, phases, mana and cooldowns, autocast, learning, channels,
//! determinism and save/restore in the middle of a cast. Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_sim::{World, abilities::EventKind};
use serde_json::json;

fn bolt_spec() -> Spec {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.bolt",
            "unit",
            &["enemy"],
            8,
            30,
            100,
            10,
            8,
            json!([{"op": "damage", "target": "target", "amount": 40, "damageType": "spell"}]),
        ),
        "bolt",
    );
    spec
}

fn duel() -> Vec<serde_json::Value> {
    vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 106.0, 100.0, 2),
    ]
}

#[test]
fn a_cast_prepares_releases_and_recovers_on_schedule() {
    let script = Script::new(
        "cast-phases",
        bolt_spec(),
        duel(),
        60,
        vec![(2, 1, cast(1, "bolt", 2))],
    );
    let mana_before = {
        let mut world = World::new(script.content(), &script.map(), setup(false)).unwrap();
        world.step();
        mana(&world, "a")
    };
    let played = script.play();
    let accepted = played.of(EventKind::Accepted);
    let released = played.of(EventKind::Released);
    let finished = played.of(EventKind::Finished);
    assert_eq!((accepted.len(), released.len(), finished.len()), (1, 1, 1));
    // Commands see the last completed tick: prepare 10 and recover 8 count from there.
    // Turning to face the target comes before the prepare time.
    assert!(released[0].tick - accepted[0].tick >= 10);
    assert_eq!(finished[0].tick - released[0].tick, 8);
    let damaged = played.at(EventKind::Damaged, "b");
    assert_eq!(damaged.len(), 1);
    assert_eq!(damaged[0].amount, Some(40));
    assert_eq!(damaged[0].tick, released[0].tick);
    assert!(played.mana("a") < mana_before, "mana is spent at release");
    let caster = played.entity("a").caster.as_ref().unwrap();
    assert!(caster.pending.is_none());
}

#[test]
fn commands_are_validated() {
    let mut spec = bolt_spec();
    give(
        &mut spec,
        spell(
            "ability.test.mend",
            "unit",
            &["ally"],
            8,
            10,
            0,
            2,
            2,
            json!([{"op": "heal", "target": "target", "amount": 10}]),
        ),
        "mend",
    );
    give(
        &mut spec,
        spell(
            "ability.test.costly",
            "self",
            &["ally"],
            0,
            5000,
            0,
            2,
            2,
            json!([{"op": "heal", "target": "caster", "amount": 1}]),
        ),
        "costly",
    );
    let mut filtered = spell(
        "ability.test.organic",
        "unit",
        &["enemy"],
        30,
        0,
        0,
        2,
        2,
        json!([{"op": "damage", "target": "target", "amount": 1, "damageType": "spell"}]),
    );
    filtered["targeting"]["filter"] = json!({"natures": ["organic"]});
    give(&mut spec, filtered, "organic");
    spec.hero
        .push(learnable("hidden", "ability.test.mend", &[1]));
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 106.0, 100.0, 2),
        placed("c", "unit.warrior", 112.0, 98.0, 2),
        placed("d", "unit.warrior", 102.0, 100.0, 1),
        placed("e", "unit.golem", 104.0, 104.0, 2),
        placed("f", "unit.hero", 96.0, 100.0, 1),
    ];
    let script = Script::new(
        "cast-validation",
        spec,
        entities,
        80,
        vec![
            (1, 1, cast(1, "bolt", 3)),
            (2, 1, cast(1, "bolt", 4)),
            (3, 1, cast(1, "mend", 2)),
            (4, 1, cast(1, "costly", 1)),
            (5, 1, cast(1, "organic", 5)),
            (6, 2, cast(1, "bolt", 2)),
            (7, 1, cast(6, "hidden", 4)),
            (8, 1, cast(1, "bolt", 2)),
            (9, 1, cast(1, "mend", 4)),
            (40, 1, cast(1, "bolt", 2)),
        ],
    );
    let played = script.play();
    let errors = played.errors();
    for expected in [
        "Invalid target relationship",
        "Not enough mana",
        "Target does not match spell filters",
        "Not your actors",
        "Learn this ability first",
        "Caster is busy",
        "Ability is cooling down",
    ] {
        assert!(
            errors.iter().any(|e| e == expected),
            "missing {expected:?} in {errors:?}"
        );
    }
    assert_eq!(played.count(EventKind::Released), 1);
}

#[test]
fn out_of_range_casters_walk_into_range() {
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 112.0, 98.0, 2),
    ];
    let played = Script::new(
        "cast-approach",
        bolt_spec(),
        entities,
        80,
        vec![(1, 1, cast(1, "bolt", 2))],
    )
    .play();
    assert!(played.errors().is_empty(), "{:?}", played.errors());
    assert_eq!(played.at(EventKind::Damaged, "b").len(), 1);
    assert!(
        played.entity("a").position.x > 101_000,
        "the caster approached"
    );
}

#[test]
fn a_cancelled_cast_refunds_and_skips_the_cooldown() {
    let script = Script::new(
        "cast-cancel",
        bolt_spec(),
        duel(),
        40,
        vec![(2, 1, cast(1, "bolt", 2)), (5, 1, moving(1, 90, 100))],
    );
    let played = script.play();
    assert_eq!(played.count(EventKind::Cancelled), 1);
    assert_eq!(played.count(EventKind::Released), 0);
    let caster = played.entity("a").caster.as_ref().unwrap();
    assert!(caster.pending.is_none());
    assert!(caster.cooldowns.is_empty(), "no cooldown before release");
    assert_eq!(
        played.hp("b"),
        played
            .world
            .catalog()
            .get(played.entity("b").definition)
            .body
            .as_ref()
            .unwrap()
            .max_hp
    );
}

#[test]
fn silence_blocks_casting() {
    let mut spec = bolt_spec();
    give(
        &mut spec,
        spell(
            "ability.test.hush",
            "unit",
            &["enemy"],
            12,
            0,
            0,
            2,
            2,
            json!([
            {"op": "status", "target": "target", "id": "hush", "amount": 100, "polarity": "negative", "dispel": true, "silence": true}]),
        ),
        "hush",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.caster", 106.0, 100.0, 2),
    ];
    let played = Script::new(
        "cast-silence",
        spec,
        entities,
        40,
        vec![(1, 1, cast(1, "hush", 2)), (10, 2, cast(2, "bolt", 1))],
    )
    .play();
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "Caster is unable to cast"),
        "{:?}",
        played.errors()
    );
    assert_eq!(played.at(EventKind::Damaged, "a").len(), 0);
}

#[test]
fn autocast_follows_the_toggle() {
    let mut spec = Spec::default();
    let mut frenzy = with(
        spell(
            "ability.test.frenzy",
            "unit",
            &["ally"],
            14,
            40,
            40,
            8,
            8,
            json!([
            {"op": "status", "target": "target", "id": "frenzy", "amount": 300, "polarity": "positive", "dispel": true,
             "rankedModifiers": {"attackSpeedPermille": {"rankParameter": "haste"}}}]),
        ),
        json!({"autocast": {"intervalTicks": 12, "enabledByDefault": false}, "ranks": [{"haste": 300}]}),
    );
    frenzy["targeting"]["allowSelf"] = json!(true);
    give(&mut spec, frenzy, "frenzy");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 103.0, 100.0, 1),
        placed("c", "unit.warrior", 108.0, 100.0, 2),
    ];
    let off = Script::new("autocast-off", spec.clone(), entities.clone(), 150, vec![]).play();
    assert_eq!(off.count(EventKind::Accepted), 0);
    let on = Script::new(
        "autocast-on",
        spec,
        entities,
        300,
        vec![(
            3,
            1,
            json!({"type": "abilityAutocast", "actor": 1, "binding": "frenzy", "enabled": true}),
        )],
    )
    .play();
    assert_eq!(on.count(EventKind::StatusApplied), 2);
    assert!(
        on.of(EventKind::StatusApplied)
            .iter()
            .all(|e| e.status.as_deref() == Some("frenzy"))
    );
}

#[test]
fn heroes_learn_ranks_with_skill_points() {
    let mut spec = Spec::default();
    spec.abilities.push(with(
        spell("ability.test.nova", "self", &["ally"], 0, 30, 60, 4, 4, json!([
            {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"]}, "amount": {"rankParameter": "damage"}, "damageType": "spell"}])),
        json!({"ranks": [{"damage": 20}, {"damage": 40}]}),
    ));
    spec.hero
        .push(learnable("nova", "ability.test.nova", &[1, 3]));
    let learn = json!({"type": "learnAbility", "actor": 1, "ability": "nova"});
    let played = Script::new(
        "learn",
        spec,
        vec![
            placed("a", "unit.hero", 100.0, 100.0, 1),
            placed("b", "unit.warrior", 104.0, 100.0, 2),
        ],
        200,
        vec![
            (1, 1, cast(1, "nova", 1)),
            (2, 1, learn.clone()),
            (3, 1, learn.clone()),
            (5, 1, cast(1, "nova", 1)),
            (60, 1, learn),
            (100, 1, cast(1, "nova", 1)),
        ],
    )
    .set(50, "a", json!({"experience": 300}))
    .play();
    let errors = played.errors();
    assert!(errors.iter().any(|e| e == "Learn this ability first"));
    assert!(errors.iter().any(|e| e == "Hero level is too low"));
    let damage: Vec<i64> = played
        .of(EventKind::Damaged)
        .iter()
        .filter_map(|e| e.amount)
        .collect();
    assert_eq!(damage.len(), 2);
    assert!(damage[1] > damage[0], "rank two hits harder: {damage:?}");
    assert_eq!(played.entity("a").caster.as_ref().unwrap().ranks, vec![2]);
}

#[test]
fn channels_release_waves_until_interrupted() {
    let mut spec = Spec::default();
    let mut hail = spell(
        "ability.test.hail",
        "point",
        &["enemy"],
        18,
        75,
        480,
        12,
        8,
        json!([{"op": "damage", "target": "target", "amount": 25, "damageType": "spell"}]),
    );
    hail["targeting"]["radius"] = json!(5);
    hail["targeting"]["maxTargets"] = json!(64);
    hail["cast"]["channel"] = json!({"waves": 6, "intervalTicks": 20});
    give(&mut spec, hail, "hail");
    let mut siphon = spell(
        "ability.test.siphon",
        "unit",
        &["enemy"],
        12,
        40,
        100,
        10,
        8,
        json!([
        {"op": "drain", "target": "target", "amount": 25, "resource": "health", "restoreCaster": true, "damageType": "spell"}]),
    );
    siphon["cast"]["channel"] = json!({"waves": 5, "intervalTicks": 20, "tetherRange": 10});
    give(&mut spec, siphon, "siphon");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
        placed("c", "unit.archer", 108.0, 104.0, 2),
    ];
    let full = Script::new(
        "channel-full",
        spec.clone(),
        entities.clone(),
        220,
        vec![(1, 1, cast_at_point(1, "hail", 111, 100))],
    )
    .play();
    assert_eq!(full.count(EventKind::WaveStarted), 6);
    assert_eq!(full.count(EventKind::Wave), 6);
    assert_eq!(full.count(EventKind::Finished), 1);
    let broken = Script::new(
        "channel-tether",
        spec,
        entities,
        200,
        vec![(1, 1, cast(1, "siphon", 3)), (40, 2, moving(3, 114, 104))],
    )
    .set(0, "a", json!({"hp": 60}))
    .play();
    assert_eq!(broken.count(EventKind::Cancelled), 1);
    assert_eq!(
        broken.of(EventKind::Cancelled)[0].reason.as_deref(),
        Some("Channel target lost")
    );
    assert!(broken.count(EventKind::Drained) >= 1);
    assert!(broken.total(EventKind::Drained) >= 25);
}

/// A script exercising casts, channels, deliveries, persistent zones, statuses, summons and
/// items at once.
fn busy_script(name: &'static str, ticks: u32) -> Script {
    let mut spec = bolt_spec();
    let mut hail = spell(
        "ability.test.hail",
        "point",
        &["enemy"],
        18,
        30,
        60,
        12,
        8,
        json!([
        {"op": "damage", "target": "target", "amount": 5, "damageType": "spell"},
        {"op": "status", "target": "target", "id": "chill", "amount": 60, "polarity": "negative", "dispel": true, "modifiers": {"moveSpeedPermille": -300},
         "periodic": {"intervalTicks": 10, "damage": 2, "damageType": "spell"}}]),
    );
    hail["targeting"]["radius"] = json!(5);
    hail["cast"]["channel"] = json!({"waves": 4, "intervalTicks": 15});
    give(&mut spec, hail, "hail");
    give(
        &mut spec,
        with(
            spell(
                "ability.test.pebble",
                "unit",
                &["enemy"],
                16,
                20,
                30,
                6,
                6,
                json!([{"op": "damage", "target": "target", "amount": 15, "damageType": "spell"}]),
            ),
            json!({"delivery": {"kind": "projectile", "speed": 6}}),
        ),
        "pebble",
    );
    give(
        &mut spec,
        spell(
            "ability.test.imps",
            "self",
            &["ally"],
            0,
            30,
            60,
            4,
            4,
            json!([
            {"op": "summon", "target": "caster", "amount": 2, "definition": "unit.imp", "durationTicks": 90, "replace": true, "radius": 3}]),
        ),
        "imps",
    );
    let mut spores = with(
        spell(
            "ability.test.spores",
            "point",
            &["enemy"],
            18,
            30,
            200,
            6,
            6,
            json!([{"op": "damage", "target": "target", "amount": 3, "damageType": "spell"}]),
        ),
        json!({"persistent": {"durationTicks": 120, "intervalTicks": 20, "anchor": "point", "endsWithCaster": false}}),
    );
    spores["targeting"]["radius"] = json!(4);
    give(&mut spec, spores, "spores");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
        placed("c", "unit.warrior", 112.0, 103.0, 2),
        placed("d", "unit.archer", 111.0, 97.0, 2),
        placed("e", "unit.warrior", 102.0, 101.0, 1),
    ];
    Script::new(
        name,
        spec,
        entities,
        ticks,
        vec![
            (1, 1, cast_at_point(1, "hail", 111, 100)),
            (80, 1, cast(1, "pebble", 2)),
            (95, 1, cast(1, "imps", 1)),
            (110, 1, cast_at_point(1, "spores", 111, 101)),
            (130, 1, cast(1, "bolt", 3)),
        ],
    )
}

#[test]
fn runs_are_deterministic() {
    let a = busy_script("determinism", 260).play();
    let b = busy_script("determinism", 260).play();
    assert_eq!(a.prints, b.prints);
    assert!(
        a.count(EventKind::Released) >= 4,
        "the script casts: {:?}",
        a.errors()
    );
}

#[test]
fn save_and_restore_mid_cast_resume_identically() {
    let straight = busy_script("reload", 260).play();
    // Mid-channel, a projectile in flight, summons alive and a zone pulsing.
    for at in [10, 84, 100, 125, 140] {
        let reloaded = busy_script("reload", 260).play_reloading(Some(at));
        assert_eq!(
            straight.prints, reloaded.prints,
            "save at tick {at} diverged"
        );
        assert_eq!(straight.world.fingerprint(), reloaded.world.fingerprint());
    }
}

#[test]
fn camp_creatures_cast_on_their_own() {
    let mut spec = Spec::default();
    spec.abilities.push(spell(
        "ability.test.spit",
        "unit",
        &["enemy"],
        10,
        20,
        60,
        6,
        6,
        json!([{"op": "damage", "target": "target", "amount": 15, "damageType": "spell"}]),
    ));
    let mut lick = spell(
        "ability.test.lick",
        "unit",
        &["ally"],
        10,
        20,
        60,
        6,
        6,
        json!([{"op": "heal", "target": "target", "amount": 25}]),
    );
    lick["targeting"]["allowSelf"] = json!(true);
    spec.abilities.push(lick);
    spec.critter.push(json!({"id": "spit", "ability": "ability.test.spit", "initialRank": 1, "controls": ["ai"], "ai": {"preference": "enemy", "intervalTicks": 10}}));
    spec.critter.push(json!({"id": "lick", "ability": "ability.test.lick", "initialRank": 1, "controls": ["ai"], "ai": {"preference": "wounded-ally", "intervalTicks": 10}}));
    let mut script = Script::new(
        "camp-casters",
        spec,
        vec![
            placed("a", "unit.critter", 100.0, 100.0, 0),
            placed("b", "unit.critter", 101.0, 102.0, 0),
            placed("c", "unit.warrior", 112.0, 100.0, 1),
        ],
        300,
        vec![(2, 1, moving(3, 104, 100))],
    )
    .set(0, "b", json!({"hp": 40}));
    script.camps = vec![camp("camp.test", &["a", "b"], (100.0, 101.0))];
    let played = script.play();
    assert!(
        played.at(EventKind::Damaged, "c").len() >= 2,
        "{}",
        played.dump()
    );
    assert!(
        !played.at(EventKind::Healed, "b").is_empty(),
        "the wounded camp member is healed"
    );
}
