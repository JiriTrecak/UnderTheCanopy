//! The ability encounter (the editor's ability stage): fixtures for every aim, a cast that
//! lands, seeking by replay and the presets. Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_content::{Content, placement::Owner};
use canopy_sim::{
    abilities::EventKind,
    encounter::{self, Aim, Fixture, Intervention, PRESETS, Preset, Relation, Settings, Take},
};
use serde_json::json;
use std::sync::Arc;

/// A bolt (unit), a hail (point, channelled) and a ward (self, a status), bound by the
/// caster; a heal bound by nobody.
fn library() -> Arc<Content> {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.bolt",
            "unit",
            &["enemy"],
            12,
            50,
            200,
            10,
            8,
            json!([{"op": "damage", "target": "target", "amount": 30, "damageType": "spell"}]),
        ),
        "bolt",
    );
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
    hail["cast"]["channel"] = json!({"waves": 4, "intervalTicks": 20});
    give(&mut spec, hail, "hail");
    give(
        &mut spec,
        spell(
            "ability.test.ward",
            "self",
            &["ally"],
            0,
            20,
            300,
            6,
            6,
            json!([{"op": "status", "target": "caster", "id": "ward", "amount": 120, "polarity": "positive", "dispel": true}]),
        ),
        "ward",
    );
    spec.abilities.push(spell(
        "ability.test.mend",
        "unit",
        &["ally"],
        10,
        10,
        100,
        8,
        8,
        json!([{"op": "heal", "target": "target", "amount": 40}]),
    ));
    abilities_fixture::content("encounter", &spec)
}

fn settings(relation: Relation, targets: u32) -> Settings {
    Settings {
        caster: "unit.caster".into(),
        target: "unit.warrior".into(),
        relation,
        targets,
        ..Settings::default()
    }
}

#[test]
fn fixtures_stand_up_for_every_aim() {
    let content = library();
    for (ability, relation, targets) in [
        ("ability.test.bolt", Relation::Enemy, 3),
        ("ability.test.hail", Relation::Neutral, 5),
        ("ability.test.ward", Relation::Caster, 4),
        ("ability.test.mend", Relation::Ally, 1),
    ] {
        let fixture = Fixture::new(&content, ability, &settings(relation, targets))
            .unwrap_or_else(|e| panic!("{ability}: {e}"));
        // Free and ready at once, bound on Q by the caster only.
        assert_eq!(
            fixture.ability.cast.cost.amount.at(&Default::default()),
            Some(0)
        );
        let caster = fixture.content.definition(encounter::CASTER).unwrap();
        let bindings = &caster.behaviors.abilities.as_ref().unwrap().bindings;
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].ability.as_str(), ability);
        assert!(caster.behaviors.progression.is_none() && caster.behaviors.combat.is_none());
        let world = fixture.world().unwrap();
        let expected = if relation == Relation::Caster {
            0
        } else {
            targets
        };
        let owner = match relation {
            Relation::Enemy => Owner::Player(1),
            Relation::Neutral => Owner::Nobody,
            _ => Owner::Player(0),
        };
        let standing: Vec<_> = world
            .state()
            .entities
            .iter()
            .filter(|e| {
                e.placement
                    .as_deref()
                    .is_some_and(|p| p.starts_with("target-"))
            })
            .collect();
        assert_eq!(standing.len() as u32, expected, "{ability}");
        assert!(standing.iter().all(|e| e.owner == owner), "{ability}");
        let caster = world
            .state()
            .entities
            .iter()
            .find(|e| e.placement.as_deref() == Some("caster"))
            .unwrap();
        assert_eq!(caster.caster.as_ref().map(|c| c.mana), Some(200));
        assert!(fixture.duration > fixture.ability.cast.prepare_ticks);
    }
    // A unit that does not exist is refused, not guessed.
    assert!(
        Fixture::new(
            &content,
            "ability.test.bolt",
            &Settings {
                caster: "unit.nobody".into(),
                ..settings(Relation::Enemy, 1)
            }
        )
        .is_err()
    );
}

#[test]
fn a_bolt_on_an_enemy_hurts_it() {
    let content = library();
    let fixture = Arc::new(
        Fixture::new(&content, "ability.test.bolt", &settings(Relation::Enemy, 1)).unwrap(),
    );
    let target = fixture
        .world()
        .unwrap()
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("target-0"))
        .map(|e| e.id.0)
        .unwrap();
    let aim = Some(Aim::Unit { entity: target });
    // Stopped before its release, the cast is cancelled and nothing lands.
    let mut stopped = Take::new(fixture.clone(), aim).unwrap();
    stopped.seek(3).unwrap();
    stopped.intervene(Intervention::Interrupt);
    stopped.finish();
    assert!(
        stopped
            .events()
            .iter()
            .any(|e| e.kind == EventKind::Cancelled)
    );
    assert!(
        !stopped
            .events()
            .iter()
            .any(|e| e.kind == EventKind::Damaged)
    );
    let mut take = Take::new(fixture, aim).unwrap();
    take.finish();
    let damaged: Vec<_> = take
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::Damaged)
        .collect();
    assert_eq!(damaged.len(), 1, "{:?}", take.refusals());
    assert_eq!(damaged[0].amount, Some(30));
    // The release comes after the prepare ticks (counted from the take's start).
    let released = take
        .events()
        .iter()
        .find(|e| e.kind == EventKind::Released)
        .unwrap();
    assert!(released.tick >= 10);
    let timeline = take.timeline();
    assert!(
        timeline
            .animation
            .iter()
            .any(|s| s.label.starts_with("Prepare"))
    );
    assert!(timeline.impacts.iter().any(|s| s.label == "damaged · 30"));
    assert!(take.refusals().is_empty());
}

#[test]
fn a_point_cast_and_a_self_cast_play() {
    let content = library();
    let hail = Arc::new(
        Fixture::new(&content, "ability.test.hail", &settings(Relation::Enemy, 4)).unwrap(),
    );
    let [x, z] = encounter::ORIGIN;
    let at = Aim::Point {
        x: (x + 8.0) as u32,
        y: z as u32,
    };
    let mut take = Take::new(hail, Some(at)).unwrap();
    take.finish();
    let waves = take
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::Wave)
        .count();
    assert_eq!(waves, 4, "{:?}", take.refusals());
    assert!(
        take.timeline()
            .animation
            .iter()
            .any(|s| s.label.starts_with("Channel"))
    );
    let ward = Arc::new(
        Fixture::new(
            &content,
            "ability.test.ward",
            &settings(Relation::Caster, 0),
        )
        .unwrap(),
    );
    let mut take = Take::new(ward, Some(Aim::Caster)).unwrap();
    take.finish();
    assert!(
        take.events()
            .iter()
            .any(|e| e.kind == EventKind::StatusApplied),
        "{:?}",
        take.refusals()
    );
    assert_eq!(take.timeline().statuses.len(), 1);
}

#[test]
fn seeking_replays_to_the_same_world() {
    let content = library();
    let fixture = Arc::new(
        Fixture::new(
            &content,
            "ability.test.hail",
            &Settings {
                combat: true,
                ..settings(Relation::Enemy, 5)
            },
        )
        .unwrap(),
    );
    let [x, z] = encounter::ORIGIN;
    let at = Some(Aim::Point {
        x: (x + 8.0) as u32,
        y: z as u32,
    });
    // Straight to tick 60 with an interruption at 30.
    let mut straight = Take::new(fixture.clone(), at).unwrap();
    straight.seek(30).unwrap();
    straight.intervene(Intervention::Interrupt);
    straight.seek(60).unwrap();
    // Further, back to 45 and on to 60 again: the same world and events.
    let mut seeking = Take::new(fixture.clone(), at).unwrap();
    seeking.seek(30).unwrap();
    seeking.intervene(Intervention::Interrupt);
    seeking.seek(100).unwrap();
    seeking.seek(45).unwrap();
    assert_eq!(seeking.tick(), 45);
    seeking.seek(60).unwrap();
    assert_eq!(seeking.fingerprint(), straight.fingerprint());
    assert_eq!(seeking.events(), straight.events());
    // The interruption cancelled the channel.
    assert!(
        straight
            .events()
            .iter()
            .any(|e| e.kind == EventKind::Cancelled || e.kind == EventKind::Finished),
        "{:?}",
        straight
            .events()
            .iter()
            .map(|e| (e.tick, e.kind))
            .collect::<Vec<_>>()
    );
    let waves = straight
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::Wave)
        .count();
    assert!(waves < 4, "interrupted after {waves} waves");
}

#[test]
fn presets_and_suggestions_set_the_scene() {
    let content = library();
    let base = settings(Relation::Enemy, 1);
    for preset in &PRESETS {
        let s = preset.apply(&base);
        let fixture = Fixture::new(&content, "ability.test.bolt", &s).unwrap();
        let world = fixture.world().unwrap();
        let targets = world
            .state()
            .entities
            .iter()
            .filter(|e| {
                e.placement
                    .as_deref()
                    .is_some_and(|p| p.starts_with("target-"))
            })
            .count() as u32;
        let corpses = world.state().combat.corpses.len() as u32;
        // Fallen targets lie as corpses; the others stand.
        if s.fallen {
            assert_eq!((targets, corpses), (0, s.targets), "{}", preset.id);
        } else {
            assert_eq!(targets, s.targets, "{}", preset.id);
        }
    }
    // The caster that binds the ability casts it; the scene follows its aim.
    let bolt = encounter::suggest(&content, "ability.test.bolt");
    assert_eq!(bolt.caster, "unit.caster");
    assert_eq!(Preset::of(&bolt).map(|p| p.id), Some("enemy-single"));
    let ward = encounter::suggest(&content, "ability.test.ward");
    assert_eq!(Preset::of(&ward).map(|p| p.id), Some("self"));
    let mend = encounter::suggest(&content, "ability.test.mend");
    assert_eq!(Preset::of(&mend).map(|p| p.id), Some("ally-single"));
    assert!(!mend.caster.is_empty());
}
