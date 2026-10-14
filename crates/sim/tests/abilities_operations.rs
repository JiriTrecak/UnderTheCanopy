//! Every release operation: damage, heal, mana, drain, status, dispel, summon, resurrect,
//! revive, split, teleport, vision, convert, contain, releaseContained and sacrifice, plus
//! branches, area queries, records and scales. Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_content::placement::Owner;
use canopy_sim::abilities::EventKind;
use serde_json::json;

#[test]
fn branches_heal_allies_and_damage_enemies() {
    let mut spec = Spec::default();
    let mut sap = with(
        spell(
            "ability.test.sap",
            "unit",
            &["ally", "enemy"],
            12,
            30,
            20,
            12,
            8,
            json!([
            {"op": "branch", "condition": {"kind": "relation", "of": "target", "to": "caster", "is": "ally"},
              "then": [{"op": "heal", "target": "target", "amount": {"rankParameter": "heal"}}],
              "else": [{"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"}]}]),
        ),
        json!({"ranks": [{"heal": 120, "damage": 60}]}),
    );
    sap["targeting"]["allowSelf"] = json!(true);
    sap["targeting"]["condition"] = json!({"kind": "any", "conditions": [
        {"kind": "relation", "of": "target", "to": "caster", "is": "enemy"},
        {"kind": "resource", "of": "target", "resource": "health", "measure": "missing", "comparison": "gt", "value": 0}]});
    give(&mut spec, sap, "sap");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 103.0, 100.0, 1),
        placed("c", "unit.warrior", 110.0, 104.0, 2),
    ];
    let played = Script::new(
        "op-branch",
        spec,
        entities,
        120,
        vec![
            (1, 1, cast(1, "sap", 2)),
            (2, 1, cast(1, "sap", 3)),
            (50, 1, cast(1, "sap", 2)),
        ],
    )
    .set(45, "b", json!({"hp": 30}))
    .play();
    // A full-health ally fails the targeting condition.
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "Target does not match spell filters")
    );
    assert_eq!(played.at(EventKind::Damaged, "c")[0].amount, Some(60));
    let healed = played.at(EventKind::Healed, "b");
    assert_eq!(healed.len(), 1);
    assert!(played.hp("b") > 30);
}

#[test]
fn area_queries_exclude_the_primary_and_cap_targets() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.frost",
            "unit",
            &["enemy"],
            14,
            40,
            240,
            10,
            8,
            json!([
            {"op": "damage", "target": "target", "amount": 90, "damageType": "spell"},
            {"op": "damage", "target": "target", "query": {"center": "target", "radius": 5, "relations": ["enemy"], "excludePrimary": true}, "amount": 45, "damageType": "spell"}]),
        ),
        "frost",
    );
    let mut seed = spell(
        "ability.test.seed",
        "point",
        &["enemy"],
        18,
        60,
        400,
        14,
        8,
        json!([{"op": "damage", "target": "target", "amount": 7, "damageType": "spell"}]),
    );
    seed["targeting"]["radius"] = json!(4);
    seed["targeting"]["maxTargets"] = json!(2);
    give(&mut spec, seed, "seed");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
        placed("c", "unit.warrior", 112.0, 101.0, 2),
        placed("d", "unit.archer", 111.0, 98.0, 2),
        placed("e", "unit.warrior", 117.0, 100.0, 2),
    ];
    let played = Script::new(
        "op-area",
        spec.clone(),
        entities.clone(),
        60,
        vec![(1, 1, cast(1, "frost", 2))],
    )
    .play();
    let frost: Vec<_> = played
        .of(EventKind::Damaged)
        .into_iter()
        .filter(|e| e.tick < 60)
        .collect();
    assert_eq!(
        frost.len(),
        3,
        "primary plus two neighbours; e is out of the radius"
    );
    assert_eq!(
        played
            .at(EventKind::Damaged, "b")
            .first()
            .and_then(|e| e.amount),
        Some(90),
        "{}",
        played.dump()
    );
    assert!(played.at(EventKind::Damaged, "e").is_empty());
    let seeded = Script::new(
        "op-area-cap",
        spec,
        entities,
        60,
        vec![(1, 1, cast_at_point(1, "seed", 112, 100))],
    )
    .play();
    assert_eq!(
        seeded.count(EventKind::Damaged),
        2,
        "maxTargets caps the point area: {}",
        seeded.dump()
    );
}

#[test]
fn mana_drain_and_burn() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.sip",
            "unit",
            &["enemy"],
            12,
            0,
            20,
            4,
            4,
            json!([
        {"op": "drain", "target": "target", "amount": 30, "resource": "mana", "restoreCaster": true, "damageType": "spell"}]),
        ),
        "sip",
    );
    give(
        &mut spec,
        spell(
            "ability.test.burn",
            "unit",
            &["enemy"],
            12,
            0,
            20,
            4,
            4,
            json!([
        {"op": "drain", "target": "target", "amount": 50, "resource": "mana", "restoreCaster": false, "damageType": "spell", "damagePerDrainedPermille": 1000}]),
        ),
        "burn",
    );
    give(
        &mut spec,
        spell(
            "ability.test.gift",
            "unit",
            &["ally"],
            12,
            0,
            20,
            4,
            4,
            json!([{"op": "mana", "target": "target", "amount": 40}]),
        ),
        "gift",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.caster", 108.0, 100.0, 2),
        placed("c", "unit.caster", 100.0, 104.0, 1),
    ];
    let played = Script::new(
        "op-mana",
        spec,
        entities,
        100,
        vec![
            (1, 1, cast(1, "sip", 2)),
            (30, 1, cast(1, "burn", 2)),
            (60, 1, cast(1, "gift", 3)),
        ],
    )
    .set(0, "a", json!({"mana": 50}))
    .set(0, "c", json!({"mana": 20}))
    .play();
    let drained = played.of(EventKind::Drained);
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].amount, Some(30));
    assert_eq!(drained[1].amount, Some(50));
    // Burned mana turns into damage.
    let b_max = played
        .world
        .catalog()
        .get(played.entity("b").definition)
        .body
        .as_ref()
        .unwrap()
        .max_hp;
    assert!(played.hp("b") <= b_max - 50);
    assert_eq!(played.of(EventKind::ManaRestored)[0].amount, Some(40));
    assert!(played.mana("c") >= 60);
}

#[test]
fn summons_replace_expire_and_can_be_dispelled() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.spirits",
                "self",
                &["ally"],
                0,
                75,
                60,
                12,
                8,
                json!([
        {"op": "summon", "target": "caster", "amount": {"rankParameter": "count"}, "definition": "unit.imp", "durationTicks": 150, "replace": true, "radius": 3}]),
            ),
            json!({"ranks": [{"count": 3}]}),
        ),
        "spirits",
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
        {"op": "dispel", "target": "target", "amount": 50, "damageType": "spell", "polarity": "all"}]),
        ),
        "purge",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.caster", 108.0, 100.0, 2),
    ];
    let played = Script::new(
        "op-summon",
        spec,
        entities,
        120,
        vec![
            (1, 1, cast(1, "spirits", 1)),
            (90, 1, cast(1, "spirits", 1)),
        ],
    )
    .play();
    let summoned = played.of(EventKind::Summoned);
    assert_eq!(summoned.len(), 2);
    assert_eq!(summoned[0].spawned.len(), 3);
    // Replacing removes the first group.
    for id in &summoned[0].spawned {
        assert!(played.world.state().entities.get(*id).is_none());
    }
    let imps = played
        .world
        .state()
        .entities
        .iter()
        .filter(|e| e.links.as_ref().is_some_and(|l| l.summoned.is_some()))
        .count();
    assert_eq!(imps, 3);
    let expired = Script::new(
        "op-summon-expiry",
        played_spec_spirits(),
        vec![placed("a", "unit.caster", 100.0, 100.0, 1)],
        200,
        vec![(1, 1, cast(1, "spirits", 1))],
    )
    .play();
    assert_eq!(
        expired
            .world
            .state()
            .entities
            .iter()
            .filter(|e| e.links.as_ref().is_some_and(|l| l.summoned.is_some()))
            .count(),
        0,
        "summons expire"
    );
}

fn played_spec_spirits() -> Spec {
    let mut spec = Spec::default();
    give(
        &mut spec,
        with(
            spell(
                "ability.test.spirits",
                "self",
                &["ally"],
                0,
                75,
                60,
                12,
                8,
                json!([
        {"op": "summon", "target": "caster", "amount": {"rankParameter": "count"}, "definition": "unit.imp", "durationTicks": 150, "replace": true, "radius": 3}]),
            ),
            json!({"ranks": [{"count": 3}]}),
        ),
        "spirits",
    );
    spec
}

#[test]
fn dispel_strips_statuses_and_harms_summons() {
    let mut spec = played_spec_spirits();
    let mut purge = spell(
        "ability.test.purge",
        "unit",
        &["ally", "enemy"],
        16,
        20,
        20,
        4,
        4,
        json!([
        {"op": "dispel", "target": "target", "amount": 50, "damageType": "spell", "polarity": "all"}]),
    );
    purge["targeting"]["allowSelf"] = json!(true);
    give(&mut spec, purge, "purge");
    give(
        &mut spec,
        spell(
            "ability.test.curse",
            "unit",
            &["enemy"],
            16,
            0,
            0,
            2,
            2,
            json!([
        {"op": "status", "target": "target", "id": "curse", "amount": 400, "polarity": "negative", "dispel": true, "modifiers": {"armor": -3}}]),
        ),
        "curse",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.caster", 106.0, 100.0, 2),
    ];
    let played = Script::new(
        "op-dispel",
        spec,
        entities,
        150,
        vec![
            (1, 1, cast(1, "spirits", 1)),
            (40, 2, cast(2, "curse", 1)),
            (60, 1, cast(1, "purge", 1)),
            (70, 2, cast(2, "purge", 3)),
        ],
    )
    .set(50, "a", json!({"mana": 100}))
    .play();
    assert_eq!(
        played.count(EventKind::Dispelled),
        2,
        "{}\n{:?}",
        played.dump(),
        played.errors()
    );
    assert!(statuses(&played.world, "a").is_empty());
    // Dispelling a summon harms it.
    let imp = canopy_sim::entity::EntityId(3);
    let hurt = played
        .events
        .iter()
        .any(|e| e.kind == EventKind::Damaged && e.target == Some(imp));
    assert!(
        hurt || played.world.state().entities.get(imp).is_none(),
        "{}",
        played.dump()
    );
}

#[test]
fn corpses_raise_with_resurrect_and_heroes_revive() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.awaken",
            "self",
            &["ally"],
            0,
            100,
            9600,
            20,
            16,
            json!([
        {"op": "resurrect", "target": "caster", "amount": 6, "corpses": {"radius": 12, "relations": ["ally"], "order": "strongest"}, "healthPermille": 1000, "manaPermille": 0, "placementRadius": 3, "supply": "allow-over-cap"}]),
        ),
        "awaken",
    );
    let entities = vec![
        placed("a", "unit.caster", 96.0, 100.0, 1),
        placed("b", "unit.warrior", 102.0, 100.0, 1),
        placed("c", "unit.archer", 101.0, 103.0, 1),
        placed("d", "unit.warrior", 104.0, 100.0, 2),
        placed("e", "unit.warrior", 104.0, 102.0, 2),
    ];
    let played = Script::new(
        "op-resurrect",
        spec,
        entities,
        300,
        vec![(200, 1, cast(1, "awaken", 1))],
    )
    .set(0, "b", json!({"hp": 15}))
    .set(0, "c", json!({"hp": 10}))
    .play();
    let raised = played.of(EventKind::Resurrected);
    assert_eq!(
        raised.len(),
        2,
        "both fallen allies rise: {}",
        played.dump()
    );
    assert!(
        played
            .world
            .state()
            .combat
            .corpses
            .iter()
            .all(|c| c.owner != Owner::Player(0))
    );

    let mut spec = Spec::default();
    spec.abilities.push(with(passive("ability.test.molt", json!([])), json!({"ranks": [{"delay": 80}],
        "triggers": [{"id": "rebirth", "event": "death", "cooldownTicks": 9600,
            "operations": [{"op": "revive", "id": "molt", "target": "caster", "amount": {"rankParameter": "delay"}, "healthPermille": 1000, "manaPermille": 500, "placementRadius": 3}]}]})));
    spec.hero.push(bind("molt", "ability.test.molt", 1));
    let entities = vec![
        placed("a", "unit.hero", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 102.0, 100.0, 2),
        placed("c", "unit.warrior", 101.0, 102.0, 2),
    ];
    let played = Script::new("op-revive", spec, entities, 400, vec![])
        .set(0, "a", json!({"hp": 40}))
        .play();
    let started = played.of(EventKind::RevivalStarted);
    let revived = played.of(EventKind::Revived);
    assert_eq!((started.len(), revived.len()), (1, 1));
    assert_eq!(revived[0].tick - started[0].tick, 80);
    assert!(!played.entity("a").fallen);
}

#[test]
fn teleports_move_the_caster_and_recall_allies() {
    let mut spec = Spec::default();
    let mut step = spell(
        "ability.test.step",
        "point",
        &["ally"],
        20,
        25,
        100,
        4,
        8,
        json!([
        {"op": "teleport", "target": "caster", "amount": 3, "destination": "point", "preserveOffset": false}]),
    );
    step["targeting"]["radius"] = json!(1);
    give(&mut spec, step, "step");
    give(
        &mut spec,
        spell(
            "ability.test.recall",
            "unit",
            &["ally"],
            128,
            100,
            400,
            40,
            8,
            json!([
        {"op": "teleport", "target": "target", "query": {"center": "caster", "radius": 6, "relations": ["ally"], "allowSelf": true, "excludePrimary": true, "maxTargets": 12},
         "amount": 8, "destination": "target", "preserveOffset": true}]),
        ),
        "recall",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 102.0, 101.0, 1),
        placed("d", "unit.warrior", 140.0, 120.0, 1),
    ];
    let played = Script::new(
        "op-teleport",
        spec,
        entities,
        150,
        vec![
            (1, 1, cast(1, "recall", 3)),
            (60, 1, cast_at_point(1, "step", 130, 120)),
        ],
    )
    .play();
    let teleported = played.of(EventKind::Teleported);
    assert_eq!(teleported.len(), 3, "{}", played.dump());
    let a = played.entity("a").position;
    assert_eq!((a.x / 1000, a.y / 1000), (130, 120));
    let b = played.entity("b").position;
    assert!((b.x - 140_000).abs() <= 8_000 && (b.y - 120_000).abs() <= 8_000);
}

#[test]
fn vision_reveals_an_area() {
    let mut spec = Spec::default();
    let mut eye = spell(
        "ability.test.eye",
        "point",
        &["ally"],
        128,
        25,
        200,
        8,
        8,
        json!([
        {"op": "vision", "id": "scouting", "target": "point", "amount": 100, "radius": 8, "ignoreTerrain": true, "detectInvisible": true, "endsWithCaster": false}]),
    );
    eye["targeting"]["radius"] = json!(8);
    eye["targeting"]["visible"] = json!(false);
    give(&mut spec, eye, "eye");
    give(
        &mut spec,
        spell(
            "ability.test.zap",
            "unit",
            &["enemy"],
            60,
            10,
            10,
            2,
            2,
            json!([{"op": "damage", "target": "target", "amount": 5, "damageType": "spell"}]),
        ),
        "zap",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 140.0, 100.0, 2),
    ];
    let played = Script::new(
        "op-vision",
        spec,
        entities,
        200,
        vec![
            (1, 1, cast_at_point(1, "eye", 140, 100)),
            (30, 1, cast(1, "zap", 2)),
            (150, 1, cast(1, "zap", 2)),
        ],
    )
    .play();
    assert_eq!(played.count(EventKind::VisionCreated), 1);
    assert_eq!(
        played.at(EventKind::Damaged, "b").len(),
        1,
        "visible while the vision lasts"
    );
    assert!(
        played.errors().iter().any(|e| e == "Target is not visible"),
        "and hidden after"
    );
}

#[test]
fn convert_respects_filters_and_conditions() {
    let mut spec = Spec::default();
    let mut charm = spell(
        "ability.test.charm",
        "unit",
        &["enemy"],
        14,
        50,
        100,
        12,
        8,
        json!([{"op": "convert", "target": "target", "amount": 1, "supply": "allow-over-cap"}]),
    );
    charm["targeting"]["filter"] = json!({"heroes": false, "natures": ["organic", "undead"]});
    give(&mut spec, charm, "charm");
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 109.0, 100.0, 2),
        placed("d", "unit.golem", 108.0, 97.0, 2),
    ];
    let played = Script::new(
        "op-convert",
        spec,
        entities,
        100,
        vec![(1, 1, cast(1, "charm", 3)), (5, 1, cast(1, "charm", 2))],
    )
    .play();
    assert!(
        played
            .errors()
            .iter()
            .any(|e| e == "Target does not match spell filters")
    );
    assert_eq!(played.count(EventKind::Converted), 1);
    assert_eq!(played.entity("b").owner, Owner::Player(0));
    assert_eq!(played.entity("d").owner, Owner::Player(1));
}

#[test]
fn contain_digests_and_releases() {
    let mut spec = Spec::default();
    let mut gullet = spell(
        "ability.test.gullet",
        "unit",
        &["enemy"],
        8,
        0,
        0,
        12,
        8,
        json!([
        {"op": "contain", "id": "stomach", "target": "target", "shareVision": true, "amount": 2400, "lifetime": "untilDeath", "capacity": 1,
         "digestion": {"intervalTicks": 20, "damage": 5, "damageType": "spell"}}]),
    );
    gullet["targeting"]["filter"] = json!({"heroes": false, "locomotion": ["ground"]});
    give(&mut spec, gullet, "gullet");
    give(
        &mut spec,
        spell(
            "ability.test.spit",
            "self",
            &["ally"],
            0,
            0,
            0,
            4,
            4,
            json!([{"op": "releaseContained", "target": "caster", "amount": 1}]),
        ),
        "spit",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 104.0, 100.0, 2),
        placed("c", "unit.warrior", 106.0, 103.0, 2),
    ];
    let played = Script::new(
        "op-contain",
        spec,
        entities,
        200,
        vec![
            (1, 1, cast(1, "gullet", 2)),
            (40, 1, cast(1, "gullet", 3)),
            (120, 1, cast(1, "spit", 1)),
        ],
    )
    .play();
    assert!(played.errors().iter().any(|e| e == "Carrier is full"));
    assert_eq!(played.count(EventKind::Contained), 1);
    assert!(played.count(EventKind::Digested) >= 4);
    assert_eq!(played.count(EventKind::ReleasedContained), 1);
    let b = played.entity("b");
    assert!(b.unit.as_ref().unwrap().contained.is_none());
    assert!(
        b.links
            .as_ref()
            .and_then(|l| l.containment.as_ref())
            .is_none()
    );
}

#[test]
fn sacrifice_records_scale_later_operations() {
    let mut spec = Spec::default();
    let mut offering = spell(
        "ability.test.offering",
        "unit",
        &["ally"],
        12,
        50,
        100,
        10,
        10,
        json!([
        {"op": "sacrifice", "target": "target", "amount": 1, "record": "offering"},
        {"op": "heal", "target": "caster", "amount": 0, "scale": {"source": "result", "id": "offering", "stat": "maxHealth", "permille": 500}}]),
    );
    offering["targeting"]["filter"] = json!({"heroes": false});
    give(&mut spec, offering, "offering");
    give(
        &mut spec,
        spell(
            "ability.test.scaled",
            "unit",
            &["enemy"],
            12,
            10,
            20,
            4,
            4,
            json!([
        {"op": "damage", "target": "target", "query": {"center": "target", "radius": 4, "relations": ["enemy"]}, "amount": 5, "damageType": "spell", "record": "struck",
         "scale": {"source": "recipient", "stat": "maxHealth", "permille": 40}},
        {"op": "heal", "target": "caster", "amount": 0, "scale": {"source": "result", "id": "struck", "stat": "count", "permille": 10000}},
        {"op": "mana", "target": "caster", "amount": 0, "scale": {"source": "caster", "stat": "missingMana", "permille": 250}},
        {"op": "damage", "target": "target", "amount": 0, "damageType": "spell", "scale": {"source": "target", "stat": "missingHealth", "permille": 100}}]),
        ),
        "scaled",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 103.0, 100.0, 1),
        placed("c", "unit.warrior", 110.0, 100.0, 2),
        placed("d", "unit.archer", 111.0, 102.0, 2),
    ];
    let played = Script::new(
        "op-sacrifice",
        spec,
        entities,
        120,
        vec![
            (1, 1, cast(1, "offering", 2)),
            (50, 1, cast(1, "scaled", 3)),
        ],
    )
    .set(0, "a", json!({"hp": 40}))
    .play();
    assert_eq!(played.count(EventKind::Sacrificed), 1);
    assert!(!played.alive("b"));
    let warrior_max = played
        .world
        .catalog()
        .get(played.entity("c").definition)
        .body
        .as_ref()
        .unwrap()
        .max_hp;
    let heals = played.of(EventKind::Healed);
    assert_eq!(
        heals[0].amount,
        Some(i64::from(warrior_max / 2)),
        "half the sacrificed maximum health"
    );
    assert_eq!(
        heals[1].amount,
        Some(10),
        "ten per struck recipient, two struck"
    );
    let damage: Vec<_> = played
        .at(EventKind::Damaged, "c")
        .iter()
        .filter_map(|e| e.amount)
        .collect();
    assert_eq!(damage[0], 5 + i64::from(warrior_max) * 40 / 1000);
    assert_eq!(damage[1], damage[0] / 10, "a tenth of the missing health");
}

#[test]
fn split_forms_return_the_parent() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.trinity",
            "self",
            &["ally"],
            0,
            50,
            100,
            8,
            8,
            json!([
        {"op": "split", "id": "trinity", "target": "caster", "amount": 160, "members": ["unit.sprite", "unit.imp", "unit.sprite"],
         "radius": 3, "returnHealthPermille": 800, "returnManaPermille": 500, "onAllLost": "kill"}]),
        ),
        "trinity",
    );
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 108.0, 100.0, 2),
    ];
    let played = Script::new(
        "op-split",
        spec.clone(),
        entities,
        200,
        vec![(1, 1, cast(1, "trinity", 1))],
    )
    .play();
    let started = played.of(EventKind::SplitStarted);
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].spawned.len(), 3);
    assert_eq!(played.count(EventKind::SplitEnded), 1);
    let a = played.entity("a");
    assert!(a.unit.as_ref().unwrap().contained.is_none());
    let max_hp = played
        .world
        .catalog()
        .get(a.definition)
        .body
        .as_ref()
        .unwrap()
        .max_hp;
    assert!(played.hp("a") <= max_hp * 800 / 1000 + 5);
    for id in &started[0].spawned {
        assert!(
            played.world.state().entities.get(*id).is_none(),
            "members leave"
        );
    }
    // Losing every member kills the parent.
    let entities = vec![placed("a", "unit.caster", 100.0, 100.0, 1)];
    let mut lost = Script::new(
        "op-split-lost",
        spec,
        entities,
        60,
        vec![(1, 1, cast(1, "trinity", 1))],
    )
    .play_reloading(None);
    let members = lost.of(EventKind::SplitStarted)[0].spawned.clone();
    for id in members {
        lost.world.remove_entity(id);
    }
    lost.world.step();
    assert!(
        !lost
            .world
            .state()
            .entities
            .iter()
            .any(|e| e.placement.as_deref() == Some("a") && e.hp.is_some_and(|h| h > 0))
    );
}
