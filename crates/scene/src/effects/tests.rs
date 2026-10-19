//! Effect timing and evaluation on test-owned effects.

use super::frame::{EffectEvaluator, cue_motion, cue_time, envelope};
use super::*;
use canopy_assets::{
    CookedCueMotion, CookedEffect, CookedEmitter, CookedPulse, Easing, EffectBlend, EffectLayer,
};
use canopy_render::effects::EffectFrame;
use canopy_sim::{
    abilities::{AbilityId, events::EventPoint},
    entity::EntityId,
};
use serde_json::json;

fn layer(motion: Motion, primitive: Primitive) -> EffectLayer {
    EffectLayer {
        id: "layer".into(),
        primitive,
        motion,
        start: 0.1,
        duration: 0.5,
        sustain: false,
        at_source: false,
        follow: false,
        offset: [0.0; 3],
        color: [1.0, 0.5, 0.0],
        accent: [1.0; 3],
        count: 1,
        size: 2.0,
        height: 0.5,
        intensity: None,
        core: 0.0,
        spread: None,
        length: None,
        arc_height: 0.0,
        blend: EffectBlend::Additive,
        ground: false,
        disc: false,
        texture: None,
        flipbook: None,
        model: None,
        attachment: None,
        emitter: None,
        cue_motion: None,
        trail: None,
        rain: None,
    }
}

fn effect(id: &str, looping: bool, layers: Vec<EffectLayer>) -> CookedEffect {
    CookedEffect {
        id: id.into(),
        duration: 1.0,
        looping,
        layers,
    }
}

fn presentation(bindings: serde_json::Value) -> Presentation {
    serde_json::from_value(json!({
        "animations": {"prepare": "cast", "release": "cast", "recover": "cast", "fallback": "idle"},
        "effects": bindings,
    }))
    .unwrap()
}

fn director(effects: Vec<CookedEffect>, bindings: serde_json::Value) -> EffectDirector {
    let mut effects = effects;
    effects.sort_by(|a, b| a.id.cmp(&b.id));
    let table = Arc::new(EffectTable { effects });
    let p = presentation(bindings);
    EffectDirector::bind(table, &[Some(&p)], BTreeMap::new(), |_| None)
}

struct Nowhere;

impl EffectScene for Nowhere {
    fn anchor(&self, _: u32) -> Option<Vec3> {
        None
    }
    fn socket(&self, _: u32, _: &str) -> Option<(Vec3, Quat)> {
        None
    }
    fn floor(&self, _: Milli, _: Option<&str>) -> f32 {
        0.0
    }
    fn missile_pose(&self, _: u32) -> Option<(Vec3, Vec3)> {
        None
    }
}

fn event(id: u32, tick: u32, kind: EventKind, cast: u32) -> AbilityEvent {
    let point = EventPoint {
        x: 10_000,
        y: 20_000,
        height: 1.0,
    };
    AbilityEvent {
        id,
        cast,
        tick,
        ability: AbilityId(0),
        kind,
        caster: EntityId(1),
        target: Some(EntityId(2)),
        origin: point,
        point,
        viewers: Vec::new(),
        amount: None,
        status: None,
        reason: None,
        duration_ticks: None,
        radius: None,
        until_death: false,
        spawned: Vec::new(),
    }
}

fn step(d: &mut EffectDirector, tick: u32, events: &[AbilityEvent]) -> EffectSnapshot {
    d.step(
        &Sightings {
            tick,
            events,
            ..Sightings::default()
        },
        &Nowhere,
    )
}

#[test]
fn a_cue_starts_after_its_delay_and_ends_after_its_life() {
    let mut d = director(
        vec![effect(
            "effect.test.ring",
            false,
            vec![layer(Motion::Ring, Primitive::Decal)],
        )],
        json!([{"id": "b", "effect": "effect.test.ring", "event": "released", "anchor": "target"}]),
    );
    let events = [event(1, 10, EventKind::Released, 7)];
    let snapshot = step(&mut d, 10, &events);
    assert_eq!(snapshot.cues.len(), 1);
    let cue = &snapshot.cues[0];
    // 0.1 s = 4 ticks after the event; the root stands on the event point.
    assert_eq!(cue.start, 14.0);
    assert!((cue.root - Vec3::new(10.0, 1.0 + ROOT_LIFT, 20.0)).length() < 1e-5);
    let table = d.effect_table().clone();
    assert!(cue_time(&table, cue, 13.9).is_none());
    assert_eq!(cue_time(&table, cue, 14.0).unwrap().t, 0.0);
    assert!((cue_time(&table, cue, 24.0).unwrap().t - 0.5).abs() < 1e-6);
    // Events already played do not replay.
    assert_eq!(step(&mut d, 20, &events).cues.len(), 1);
    // Its 20-tick life is over at tick 34.
    assert!(cue_time(&table, cue, 34.0).is_none());
    assert!(step(&mut d, 34, &events).cues.is_empty());
}

#[test]
fn finite_bindings_of_looping_effects_end_after_one_period() {
    let mut d = director(
        vec![effect(
            "effect.test.loop",
            true,
            vec![layer(Motion::Glow, Primitive::Decal)],
        )],
        json!([{"id": "b", "effect": "effect.test.loop", "event": "released", "anchor": "caster"}]),
    );
    let snapshot = step(&mut d, 0, &[event(1, 0, EventKind::Released, 3)]);
    let cue = &snapshot.cues[0];
    assert_eq!(cue.finite_end, Some(44.0));
    let table = d.effect_table().clone();
    assert!((cue_time(&table, cue, 14.0).unwrap().t - 0.5).abs() < 1e-6);
    assert!(cue_time(&table, cue, 44.0).is_none());
}

#[test]
fn status_cues_last_while_the_status_does_and_auras_pulse() {
    let mut d = director(
        vec![
            effect(
                "effect.test.aura",
                false,
                vec![layer(Motion::Ring, Primitive::Decal)],
            ),
            effect(
                "effect.test.held",
                false,
                vec![layer(Motion::Glow, Primitive::Decal)],
            ),
        ],
        json!([
            {"id": "a", "effect": "effect.test.aura", "event": "statusApplied", "anchor": "target"},
            {"id": "h", "effect": "effect.test.held", "event": "statusApplied", "anchor": "target",
             "lifetime": "status"},
        ]),
    );
    let status = |aura| StatusSighting {
        entity: 5,
        ability: 0,
        status: "blessed".into(),
        source: 1,
        started: 300,
        expires: 900,
        aura,
        at: Vec3::new(3.0, 0.0, 4.0),
    };
    let seen = |tick, statuses: Vec<StatusSighting>| Sightings {
        tick,
        statuses,
        ..Sightings::default()
    };
    let s = d.step(&seen(410, vec![status(true)]), &Nowhere);
    assert_eq!(s.cues.len(), 2);
    // The finite layer pulses from the last multiple of 200 ticks; the status layer holds.
    let finite = s.cues.iter().find(|c| !c.status_lifetime).unwrap();
    let held = s.cues.iter().find(|c| c.status_lifetime).unwrap();
    assert_eq!(finite.start, 404.0);
    assert_eq!(held.start, 4.0);
    // Still one cue per holder and layer while the status lasts.
    assert_eq!(
        d.step(&seen(411, vec![status(true)]), &Nowhere).cues.len(),
        2
    );
    // The status ends: so do its cues.
    assert!(d.step(&seen(412, vec![]), &Nowhere).cues.is_empty());
}

#[test]
fn a_cancelled_cast_ends_its_cues() {
    let mut d = director(
        vec![effect(
            "effect.test.charge",
            false,
            vec![EffectLayer {
                sustain: true,
                ..layer(Motion::Billboard, Primitive::Particle)
            }],
        )],
        json!([{"id": "b", "effect": "effect.test.charge", "event": "accepted", "anchor": "caster"}]),
    );
    let events = [
        event(1, 0, EventKind::Accepted, 9),
        event(2, 0, EventKind::Accepted, 10),
    ];
    assert_eq!(step(&mut d, 0, &events).cues.len(), 2);
    let mut later = events.to_vec();
    later.push(event(3, 5, EventKind::Cancelled, 9));
    let s = step(&mut d, 5, &later);
    assert_eq!(s.cues.len(), 1);
    assert_eq!(s.cues[0].seed, 10);
}

#[test]
fn effect_lights_and_cues_are_budgeted() {
    let mut d = director(
        vec![effect(
            "effect.test.lamp",
            false,
            vec![layer(Motion::Light, Primitive::Light)],
        )],
        json!([{"id": "b", "effect": "effect.test.lamp", "event": "released", "anchor": "target"}]),
    );
    let events: Vec<AbilityEvent> = (1..=10)
        .map(|i| event(i, 0, EventKind::Released, i))
        .collect();
    assert_eq!(step(&mut d, 0, &events).cues.len(), MAX_LIGHTS);
}

#[test]
fn the_envelope_and_cue_motion_follow_the_old_curves() {
    assert!((envelope(0.0, false) - 0.2).abs() < 1e-6);
    assert!((envelope(0.5, false) - 0.5f32.powf(1.2)).abs() < 1e-6);
    assert_eq!(envelope(1.0, false), 0.0);
    assert_eq!(envelope(0.7, true), 1.0);
    let motion = CookedCueMotion {
        rotation: Some((2.0, -1.0, 0.0)),
        scale: Some(CookedPulse {
            period: 1.0,
            min: 1.0,
            max: 2.0,
            easing: Easing::Sine,
            phase: 0.0,
        }),
        opacity: None,
    };
    let m = cue_motion(Some(&motion), 0.5);
    // Half a turn clockwise, and the sine pulse at its peak.
    assert!((m.rotation + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert!((m.scale - 2.0).abs() < 1e-5);
    assert_eq!(m.opacity, 1.0);
}

#[test]
fn draws_follow_textures_not_effect_counts() {
    let textured = |id: &str, texture: &str| {
        let mut l = layer(Motion::Burst, Primitive::Particle);
        l.texture = Some(texture.into());
        l.count = 9;
        effect(id, false, vec![l])
    };
    let mut emitter = layer(Motion::Emitter, Primitive::Particle);
    emitter.count = 24;
    emitter.emitter = Some(CookedEmitter {
        burst: false,
        lifetime: 0.5,
        rate: 5.0,
        speed: 1.0,
        speed_variation: 0.0,
        direction: [0.0, 1.0, 0.0],
        cone: 0.5,
        gravity: 0.0,
        drag: 0.0,
        spawn_radius: 0.0,
        start_size: 0.2,
        end_size: 0.0,
        spin: 0.0,
        fade_in: 0.0,
        fade_out: 0.0,
        stretch: 0.0,
    });
    let mut d = director(
        vec![
            textured("effect.test.a", "spark.png"),
            textured("effect.test.b", "spark.png"),
            effect("effect.test.c", false, vec![emitter]),
        ],
        json!([
            {"id": "a", "effect": "effect.test.a", "event": "released", "anchor": "target"},
            {"id": "b", "effect": "effect.test.b", "event": "released", "anchor": "target"},
            {"id": "c", "effect": "effect.test.c", "event": "released", "anchor": "target"},
        ]),
    );
    let events: Vec<AbilityEvent> = (1..=30)
        .map(|i| event(i, 0, EventKind::Released, i))
        .collect();
    let snapshot = step(&mut d, 0, &events);
    assert_eq!(snapshot.cues.len(), 90);
    let mut evaluator = EffectEvaluator::new(d.effect_table().clone());
    assert_eq!(evaluator.textures(), ["spark.png".to_owned()]);
    let mut frame = EffectFrame::default();
    let mut models = Vec::new();
    evaluator.frame(None, &snapshot, 1.0, 10.0, None, &mut frame, &mut models);
    // 60 textured bursts share one draw; 30 untextured emitters another.
    assert_eq!(frame.draws.len(), 2);
    assert_eq!(frame.emitters.len(), 90);
    assert_eq!(frame.parts.len(), 60 * 9 + 30 * 24);
    // A continuous emitter 6 ticks (0.15 s) in has emitted ⌊0.15 · 5⌋ = 0 particles; at
    // 1 s, 2 (its 0.5 s duration caps emission).
    let emitter = frame
        .emitters
        .iter()
        .find(|e| e.ints[0] == canopy_render::effects::kind::EMITTER)
        .unwrap();
    assert_eq!(emitter.times[3], 0.0);
    evaluator.frame(None, &snapshot, 1.0, 40.0, None, &mut frame, &mut models);
    let emitter = frame
        .emitters
        .iter()
        .find(|e| e.ints[0] == canopy_render::effects::kind::EMITTER)
        .unwrap();
    assert_eq!(emitter.times[3], 2.0);
}

#[test]
fn damage_effects_take_a_capped_share_after_the_cues() {
    use super::frame::{
        DAMAGE_PARTS, DRAWN_FIRES, DRAWN_HIT_DUST, FLAME_PARTS, HIT_DUST_PARTS, SMOKE_PARTS,
    };
    use canopy_render::effects::kind;
    let mut d = director(
        vec![effect(
            "effect.test.a",
            false,
            vec![layer(Motion::Burst, Primitive::Particle)],
        )],
        json!([{"id": "a", "effect": "effect.test.a", "event": "released", "anchor": "target"}]),
    );
    // Far more burning spots and blows than are drawn, all over the map.
    for i in 0..200u32 {
        d.hit_dust(HitDust {
            position: Vec3::new(i as f32, 1.0, 0.0),
            tick: 0.0,
            direction: Vec3::X,
            lift: 1.0,
            power: 1.0,
            seed: i,
        });
    }
    d.burn((0..600u32).map(|i| Fire {
        entity: i / 4,
        position: Vec3::new(0.0, 3.0, i as f32),
        size: 1.0,
        severity: (i % 10) as f32 / 9.0,
        seed: i,
        rank: (i % 4) as u8,
    }));
    let events = [event(1, 0, EventKind::Released, 1)];
    let snapshot = step(&mut d, 5, &events);
    assert_eq!(snapshot.fires.len(), MAX_FIRES);
    let mut evaluator = EffectEvaluator::new(d.effect_table().clone());
    let mut frame = EffectFrame::default();
    let mut models = Vec::new();
    let eye = Some(Vec3::new(0.0, 30.0, 0.0));
    evaluator.frame(None, &snapshot, 1.0, 6.0, eye, &mut frame, &mut models);
    let of = |k: u32| frame.emitters.iter().filter(|e| e.ints[0] == k).count();
    // The cue's burst is drawn first and whole.
    assert_eq!(of(kind::BURST), 1);
    let dust = of(kind::DUST);
    let flames = of(kind::FLAME);
    assert_eq!(dust, DRAWN_HIT_DUST);
    assert_eq!(flames, of(kind::SMOKE));
    assert!(flames <= DRAWN_FIRES);
    let damage = dust * HIT_DUST_PARTS as usize + flames * (FLAME_PARTS + SMOKE_PARTS) as usize;
    assert!(damage <= DAMAGE_PARTS);
    assert_eq!(frame.parts.len(), damage + 1);
    // The nearest are drawn: the 64 bursts nearest the eye.
    assert!(
        frame
            .emitters
            .iter()
            .filter(|e| e.ints[0] == kind::DUST)
            .all(|e| e.position[0] < DRAWN_HIT_DUST as f32)
    );
    // More burn than are drawn: each building's first flame, the nearest buildings first.
    let drawn: Vec<f32> = frame
        .emitters
        .iter()
        .filter(|e| e.ints[0] == kind::FLAME)
        .map(|e| e.position[2])
        .collect();
    assert_eq!(drawn.len(), DRAWN_FIRES);
    assert!(
        drawn
            .iter()
            .all(|&z| z % 4.0 == 0.0 && z < 4.0 * DRAWN_FIRES as f32)
    );
    // Flames come last, over the smoke; one fire light at most.
    let last = frame.draws.last().unwrap();
    let first = frame.parts[last.first as usize];
    assert_eq!(frame.emitters[(first >> 8) as usize].ints[0], kind::FLAME);
    assert!(frame.lights.len() <= 1);
}

#[test]
fn rain_falls_its_duration_over_its_speed_and_lands_with_the_delivery() {
    let mut rain = layer(Motion::Rain, Primitive::Particle);
    rain.start = 0.0;
    rain.duration = 0.5;
    rain.count = 6;
    rain.texture = Some("shard.png".into());
    rain.spread = Some(3.0);
    rain.rain = Some(canopy_assets::CookedRain {
        speed: 2.0,
        launch_delay: 0.1,
        back: [0.0, 1.0, 0.0],
        impact: Some(canopy_assets::CookedImpact {
            duration: 0.25,
            count: 4,
            size: 0.2,
            spread: 1.0,
            height: 0.5,
            color: [1.0; 3],
            accent: [1.0; 3],
        }),
    });
    let mut d = director(
        vec![effect("effect.test.rain", false, vec![rain])],
        json!([
            {"id": "a", "effect": "effect.test.rain", "event": "released", "anchor": "target"},
            {"id": "b", "effect": "effect.test.rain", "event": "projectile", "anchor": "target",
             "durationFrom": "delivery"},
        ]),
    );
    let mut delivered = event(2, 0, EventKind::Projectile, 8);
    delivered.duration_ticks = Some(40);
    let events = [event(1, 0, EventKind::Released, 7), delivered];
    let snapshot = step(&mut d, 0, &events);
    let (plain, timed) = (&snapshot.cues[0], &snapshot.cues[1]);
    // 20 ticks over a fall speed of 2: a 10-tick fall from the event.
    assert_eq!((plain.start, plain.duration), (0.0, 10.0));
    // The delivery's 40 ticks fall in 20, starting late so the drops land as it ends.
    assert_eq!((timed.start, timed.duration), (20.0, 20.0));
    // A drop may launch 4 ticks late and splashes for 10 more.
    let table = d.effect_table().clone();
    assert!(cue_time(&table, plain, 23.9).is_some());
    assert!(cue_time(&table, plain, 24.0).is_none());
    // Shards, flashes and splashes: three emitters, a part per drop and per fragment.
    let mut evaluator = EffectEvaluator::new(table);
    let mut frame = EffectFrame::default();
    let mut models = Vec::new();
    let only = EffectSnapshot {
        cues: vec![plain.clone()],
        ..snapshot.clone()
    };
    evaluator.frame(None, &only, 1.0, 5.0, None, &mut frame, &mut models);
    use canopy_render::effects::kind;
    let kinds: Vec<u32> = frame.emitters.iter().map(|e| e.ints[0]).collect();
    assert_eq!(kinds, [kind::RAIN, kind::RAIN_FLASH, kind::RAIN_SPLASH]);
    assert_eq!(frame.parts.len(), 6 + 6 + 24);
}

#[test]
fn rain_drops_land_on_their_disc() {
    for i in 0..16 {
        let (landing, delay) = super::frame::rain_drop(77, i, 16, 4.0, 0.2);
        assert!(landing.x.hypot(landing.z) <= 4.0 + 1e-4, "{landing:?}");
        assert!((0.0..=0.2).contains(&delay));
    }
    // The same seed lands the same way (the shader hashes alike).
    assert_eq!(
        super::frame::rain_drop(77, 3, 16, 4.0, 0.2),
        super::frame::rain_drop(77, 3, 16, 4.0, 0.2)
    );
}
