//! Effect sound cues against the old effect player's rules, with test-owned effects.
//! Simulation and authored ticks are both 25 ms; the test sound lasts one second (40
//! ticks).

use super::*;
use serde_json::{Value, json};

fn sound_layer(extra: Value, sound: Value) -> Value {
    let mut settings = json!({"asset": "asset.test.sound", "role": "audio", "index": 1, "volume": 0.5,
        "pitch": 1, "loop": false, "fadeInTicks": 0, "fadeOutTicks": 0,
        "referenceDistance": 10, "maxDistance": 50});
    merge(&mut settings, sound);
    let mut layer = json!({"id": "sound", "shape": "sound", "sound": settings, "colour": "#ffffff",
        "accent": "#ffffff", "durationTicks": 60, "count": 1, "size": 1, "height": 0});
    merge(&mut layer, extra);
    layer
}

fn merge(base: &mut Value, extra: Value) {
    for (k, v) in extra.as_object().expect("object") {
        base[k] = v.clone();
    }
}

fn effect(duration: u32, looping: bool, layers: Vec<Value>) -> Value {
    json!({"id": "effect.test.sound", "name": "Sound", "durationTicks": duration,
        "loop": looping, "layers": layers})
}

fn binding(event: &str, extra: Value) -> Value {
    let mut b =
        json!({"id": "b", "effect": "effect.test.sound", "event": event, "anchor": "target"});
    merge(&mut b, extra);
    b
}

struct Bench {
    effects: EffectSounds,
    library: SoundLibrary,
    listener: Listener,
}

/// Ability 0 has the bindings; ability 1 has none.
fn bench(bindings: Vec<Value>, effect: Value) -> Bench {
    let presentation: Presentation = serde_json::from_value(json!({
        "animations": {"prepare": "a", "release": "a", "recover": "a",
        "fallback": "a"}, "effects": bindings}))
    .unwrap();
    let effect: VisualEffect = serde_json::from_value(effect).unwrap();
    let mut library = SoundLibrary::new();
    let sound = library.insert_pcm(
        "assets/library/asset.test.sound/audio.wav",
        40,
        1,
        &[1000; 40],
    );
    let mut effects = EffectSounds::new();
    effects.compile(&[Some(&presentation), None], &[effect], |s| {
        (s.asset.as_str() == "asset.test.sound").then_some(sound)
    });
    Bench {
        effects,
        library,
        listener: Listener::new(0.0, 0.0, 1.0, 0.0),
    }
}

impl Bench {
    fn at(&mut self, tick: f64) -> Vec<Request> {
        self.at_speed(tick, 1.0)
    }

    fn at_speed(&mut self, tick: f64, speed: f64) -> Vec<Request> {
        let mut out = Vec::new();
        self.effects
            .requests(tick, speed, &self.listener, &self.library, &mut out);
        out
    }
}

fn event(kind: EffectEvent, tick: u32, cast: u32) -> Trigger<'static> {
    Trigger {
        ability: 0,
        event: kind,
        cast: Some(cast),
        tick,
        caster: Some(1),
        target: Some(2),
        origin: (-20.0, 0.0),
        point: (5.0, 0.0),
        duration_ticks: None,
        status: None,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn a_one_shot_plays_once_from_its_event() {
    let mut b = bench(
        vec![binding("released", json!({}))],
        effect(
            60,
            false,
            vec![sound_layer(json!({"startTick": 4}), json!({}))],
        ),
    );
    b.effects.trigger(&event(EffectEvent::Impact, 100, 1));
    b.effects.trigger(&Trigger {
        ability: 1,
        ..event(EffectEvent::Released, 100, 1)
    });
    assert_eq!(b.effects.cue_count(), 0);
    b.effects.trigger(&event(EffectEvent::Released, 100, 1));
    assert_eq!(b.effects.cue_count(), 1);
    assert!(b.at(103.0).is_empty());
    let r = b.at(104.0);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].offset, r[0].gain, r[0].pan), (0.0, 0.5, 0.5));
    assert_eq!(r[0].category, Category::Effect);
    assert!(close(b.at(124.0)[0].offset, 0.5));
    // The sample ended; the cue lives on silently until the layer's duration.
    assert!(b.at(145.0).is_empty());
    assert_eq!(b.effects.cue_count(), 1);
    assert!(b.at(164.0).is_empty());
    assert_eq!(b.effects.cue_count(), 0);
}

#[test]
fn anchors_offsets_and_follow() {
    let mut b = bench(
        vec![binding("released", json!({}))],
        effect(
            60,
            false,
            vec![sound_layer(
                json!({"anchor": "source", "offset": {"x": 2, "y": 9, "z": 1}, "follow": true}),
                json!({}),
            )],
        ),
    );
    b.effects.trigger(&Trigger {
        origin: (10.0, 10.0),
        ..event(EffectEvent::Released, 0, 1)
    });
    // On the caster's origin plus the offset (the layer's anchor wins over the binding's).
    b.listener = Listener::new(12.0, 11.0, 1.0, 0.0);
    let r = b.at(1.0);
    assert_eq!((r[0].gain, r[0].pan), (0.5, 0.0));
    // The caster (entity 1) moved: the sound follows.
    b.effects
        .set_positions([(1, (40.0, 10.0)), (2, (0.0, 0.0))]);
    let r = b.at(2.0);
    assert!(r[0].pan > 0.9, "{r:?}");
}

#[test]
fn game_speed_and_pause() {
    let mut b = bench(
        vec![binding("released", json!({}))],
        effect(
            60,
            false,
            vec![sound_layer(json!({}), json!({"pitch": 0.5}))],
        ),
    );
    b.effects.trigger(&event(EffectEvent::Released, 0, 1));
    assert!(b.at_speed(10.0, 0.0).is_empty());
    let r = b.at_speed(10.0, 2.0);
    // Offset follows game time at the layer's pitch; the rate includes game speed.
    assert!(close(r[0].offset, 10.0 / 40.0 * 0.5));
    assert_eq!(r[0].rate, 1.0);
}

#[test]
fn fades_and_distance() {
    let mut b = bench(
        vec![binding("released", json!({}))],
        effect(
            40,
            false,
            vec![sound_layer(
                json!({"durationTicks": 40}),
                json!({"fadeInTicks": 8, "fadeOutTicks": 8}),
            )],
        ),
    );
    b.effects.trigger(&event(EffectEvent::Released, 0, 1));
    assert!((b.at(4.0)[0].gain - 0.25).abs() < 1e-6);
    assert!((b.at(20.0)[0].gain - 0.5).abs() < 1e-6);
    assert!((b.at(36.0)[0].gain - 0.25).abs() < 1e-6);
    b.listener = Listener::new(5.0, 30.0, 1.0, 0.0);
    // 30 cells from a 10/50 sound: half way, so a quarter of the gain.
    assert!((b.at(20.0)[0].gain - 0.125).abs() < 1e-6);
    b.listener = Listener::new(5.0, 60.0, 1.0, 0.0);
    assert!(b.at(21.0).is_empty());
}

#[test]
fn a_looping_sound_on_a_status_lasts_while_the_status_does() {
    let mut b = bench(
        vec![binding(
            "statusApplied",
            json!({"lifetime": "status", "statusId": "snared"}),
        )],
        effect(
            60,
            false,
            vec![sound_layer(json!({}), json!({"loop": true}))],
        ),
    );
    let snared = || StatusSighting {
        entity: 2,
        ability: 0,
        status: "snared",
        source: 1,
        started: 10,
        expires: 400,
        aura: false,
        at: (5.0, 0.0),
    };
    // Another status of the ability does not match the binding.
    b.effects.trigger(&Trigger {
        status: Some("rooted"),
        ..event(EffectEvent::StatusApplied, 10, 1)
    });
    assert_eq!(b.effects.cue_count(), 0);
    b.effects.trigger(&Trigger {
        status: Some("snared"),
        ..event(EffectEvent::StatusApplied, 10, 1)
    });
    b.effects.sync_statuses(20, [snared()]);
    b.effects.sync_statuses(21, [snared()]);
    assert_eq!(b.effects.cue_count(), 1);
    let r = b.at(100.0);
    assert!(r[0].looping);
    assert!(close(r[0].offset, 0.25));
    // Long after the layer's duration it still plays: the status owns its end.
    assert_eq!(b.at(390.0).len(), 1);
    b.effects.sync_statuses(400, []);
    assert_eq!(b.effects.cue_count(), 0);
    // A status first seen in a view (joined late) starts its cue too.
    b.effects.sync_statuses(50, [snared()]);
    assert_eq!(b.effects.cue_count(), 1);
    // A dead or expired holder ends it.
    b.effects.sync_statuses(
        401,
        [StatusSighting {
            expires: 401,
            ..snared()
        }],
    );
    assert_eq!(b.effects.cue_count(), 0);
}

#[test]
fn auras_pulse_finite_layers() {
    let mut b = bench(
        vec![binding("statusApplied", json!({}))],
        effect(
            40,
            false,
            vec![sound_layer(json!({"durationTicks": 40}), json!({}))],
        ),
    );
    let aura = StatusSighting {
        entity: 2,
        ability: 0,
        status: "inspired",
        source: 1,
        started: 0,
        expires: 10_000,
        aura: true,
        at: (0.0, 0.0),
    };
    b.effects.sync_statuses(210, [aura.clone()]);
    assert!(close(b.at(210.0)[0].offset, 10.0 / 40.0));
    // Over: removed, and re-syncing in the same pulse does not play it again.
    assert!(b.at(250.0).is_empty());
    assert_eq!(b.effects.cue_count(), 0);
    b.effects.sync_statuses(260, [aura.clone()]);
    assert!(b.at(260.0).is_empty());
    // The next pulse plays.
    b.effects.sync_statuses(400, [aura]);
    assert_eq!(b.at(400.0)[0].offset, 0.0);
}

#[test]
fn cancelling_a_cast_ends_its_cues() {
    let mut b = bench(
        vec![binding("accepted", json!({}))],
        effect(60, false, vec![sound_layer(json!({}), json!({}))]),
    );
    b.effects.trigger(&event(EffectEvent::Accepted, 0, 1));
    b.effects.trigger(&event(EffectEvent::Accepted, 0, 2));
    b.effects.trigger(&event(EffectEvent::Cancelled, 5, 1));
    assert_eq!(b.effects.cue_count(), 1);
    assert_eq!(b.at(6.0).len(), 1);
}

#[test]
fn looping_effects_repeat_one_shots() {
    let layers = || vec![sound_layer(json!({"durationTicks": 80}), json!({}))];
    // A finite binding of a looping effect ends after one period.
    let mut b = bench(
        vec![binding("released", json!({}))],
        effect(80, true, layers()),
    );
    b.effects.trigger(&event(EffectEvent::Released, 0, 1));
    assert!(close(b.at(10.0)[0].offset, 0.25));
    assert!(b.at(50.0).is_empty());
    assert!(b.at(80.0).is_empty());
    assert_eq!(b.effects.cue_count(), 0);
    // A status binding repeats it every period while the status lasts.
    let mut b = bench(
        vec![binding("statusApplied", json!({"lifetime": "status"}))],
        effect(80, true, layers()),
    );
    b.effects.trigger(&Trigger {
        status: Some("x"),
        ..event(EffectEvent::StatusApplied, 0, 1)
    });
    assert!(b.at(50.0).is_empty());
    assert!(close(b.at(90.0)[0].offset, 0.25));
    assert!(close(b.at(250.0)[0].offset, 0.25));
}

#[test]
fn delivery_durations_bound_sustained_layers() {
    let mut b = bench(
        vec![binding("waveStarted", json!({"durationFrom": "delivery"}))],
        effect(
            20,
            false,
            vec![sound_layer(json!({"sustain": true}), json!({"loop": true}))],
        ),
    );
    b.effects.trigger(&Trigger {
        duration_ticks: Some(100),
        ..event(EffectEvent::WaveStarted, 0, 1)
    });
    assert_eq!(b.at(99.0).len(), 1);
    assert!(b.at(100.0).is_empty());
    assert_eq!(b.effects.cue_count(), 0);
}

#[test]
fn projectiles_sound_once_per_cast() {
    let mut b = bench(
        vec![binding("projectile", json!({}))],
        effect(60, false, vec![sound_layer(json!({}), json!({}))]),
    );
    b.effects.trigger(&event(EffectEvent::Projectile, 0, 5));
    let flying = |cast| DeliverySighting {
        cast,
        ability: 0,
        at: (1.0, 1.0),
    };
    b.effects.sync_deliveries(1, [flying(5)]);
    assert_eq!(b.effects.cue_count(), 1);
    // A delivery first seen in flight starts its sound; seeing it again does not.
    b.effects.sync_deliveries(2, [flying(5), flying(6)]);
    b.effects.sync_deliveries(3, [flying(6)]);
    b.effects.sync_deliveries(4, []);
    b.effects.sync_deliveries(5, [flying(6)]);
    assert_eq!(b.effects.cue_count(), 2);
}

#[test]
fn revival_cues_last_while_the_hero_returns() {
    let mut b = bench(
        vec![binding(
            "revivalStarted",
            json!({"durationFrom": "delivery"}),
        )],
        effect(
            60,
            false,
            vec![sound_layer(json!({"sustain": true}), json!({"loop": true}))],
        ),
    );
    let returning = ReturnSighting {
        hero: 3,
        ability: 0,
        started: 10,
        due: 500,
        at: Some((0.0, 0.0)),
    };
    b.effects.sync_returns(20, [returning.clone()]);
    b.effects.sync_returns(21, [returning.clone()]);
    assert_eq!(b.effects.cue_count(), 1);
    assert_eq!(b.at(300.0).len(), 1);
    b.effects.sync_returns(22, []);
    assert_eq!(b.effects.cue_count(), 0);
    // Unknown position: nothing to place it at.
    b.effects.sync_returns(
        23,
        [ReturnSighting {
            at: None,
            ..returning
        }],
    );
    assert_eq!(b.effects.cue_count(), 0);
}

#[test]
fn layers_without_their_sound_are_skipped() {
    let mut b = bench(
        vec![binding("released", json!({}))],
        effect(
            60,
            false,
            vec![
                sound_layer(json!({}), json!({"asset": "asset.test.missing"})),
                sound_layer(json!({"id": "off", "enabled": false}), json!({})),
            ],
        ),
    );
    assert_eq!(b.effects.recipe_count(), 0);
    b.effects.trigger(&event(EffectEvent::Released, 0, 1));
    assert!(b.at(1.0).is_empty());
}
