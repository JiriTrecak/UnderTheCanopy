//! From a running simulation to voices: a spell with test-owned effects whose sound
//! layers are bound to its release and to the status it applies.

#[path = "../../sim/tests/abilities_fixture/mod.rs"]
mod abilities_fixture;

use abilities_fixture::*;
use canopy_audio::{Audio, AudioSettings, Category, Frame, Listener};
use canopy_content::testing::{DataFolder, output, record};
use serde_json::{Value, json};
use std::sync::Arc;

const CHIME: &str = "assets/library/asset.test.chime/audio.wav";

fn sound_effect(id: &str, looping: bool) -> Value {
    json!({"schemaVersion": 1, "id": id, "name": id, "durationTicks": 40, "loop": false,
        "layers": [{"id": "sound", "shape": "sound", "colour": "#ffffff", "accent": "#ffffff",
            "durationTicks": 40, "count": 1, "size": 1, "height": 0, "sustain": looping,
            "sound": {"asset": "asset.test.chime", "role": "audio", "index": 1, "volume": 0.8,
                "pitch": 1, "loop": looping, "fadeInTicks": 0, "fadeOutTicks": 4,
                "referenceDistance": 12, "maxDistance": 80}}]})
}

fn content(spec: &Spec) -> Arc<canopy_content::Content> {
    let mut library = library(spec);
    library["presentations"][0]["effects"] = json!([
        {"id": "release", "effect": "effect.test.release", "event": "released", "anchor": "target"},
        {"id": "snared", "effect": "effect.test.snared", "event": "statusApplied",
            "anchor": "target", "lifetime": "status", "statusId": "snared"},
    ]);
    let mut chime = record(
        "asset.test.chime",
        vec![output("data", CHIME)],
        json!([]),
        json!([]),
    );
    chime["kind"] = json!("audio");
    let mut records = records();
    records.push(chime);
    let mut folder = DataFolder::new(game(spec), records);
    folder.abilities = library;
    folder.effects = json!([
        sound_effect("effect.test.release", false),
        sound_effect("effect.test.snared", true)
    ]);
    Arc::new(folder.load("audio-sim").unwrap_or_else(|e| panic!("{e}")))
}

#[test]
fn spell_sounds_follow_events_and_statuses() {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.snare",
            "unit",
            &["enemy"],
            16,
            0,
            100,
            10,
            8,
            json!([{"op": "status", "target": "target", "id": "snared", "amount": 120,
                "polarity": "negative", "dispel": true, "modifiers": {"rooted": true}}]),
        ),
        "snare",
    );
    let content = content(&spec);
    let entities = vec![
        placed("a", "unit.caster", 100.0, 100.0, 1),
        placed("b", "unit.warrior", 110.0, 100.0, 2),
    ];
    let mut world = canopy_sim::World::new(content.clone(), &map(entities), setup(false))
        .unwrap_or_else(|e| panic!("{e}"));

    let mut audio = Audio::headless(AudioSettings::default());
    // Stands in for the pack: a second of sound under its published path.
    audio
        .library_mut()
        .insert_pcm(CHIME, 8000, 1, &[1000; 8000]);
    audio.bind(&content);
    assert_eq!(audio.effects().recipe_count(), 2);

    let (a, b) = (id(&world, "a"), id(&world, "b"));
    cast_at(&mut world, 1, a, "snare", b);
    let listener = Listener::new(105.0, 100.0, 1.0, 0.0);
    let mut effect_voices = Vec::new();
    for _ in 0..200 {
        world.step();
        audio.observe(&world.view(player(1)));
        audio.update(&Frame {
            real_dt: 0.025,
            render_tick: f64::from(world.tick()),
            speed: 1.0,
            listener,
        });
        effect_voices.push(audio.stats().voices[Category::Effect as usize]);
    }
    let released = world
        .drain_events()
        .iter()
        .any(|e| e.kind == canopy_sim::abilities::EventKind::Released);
    assert!(released);
    // The release chime and the snare loop together, then the loop alone while the
    // status lasts (120 ticks), then silence.
    assert!(effect_voices.contains(&2), "{effect_voices:?}");
    let last_two = effect_voices.iter().rposition(|&v| v == 2).unwrap();
    let last_one = effect_voices.iter().rposition(|&v| v >= 1).unwrap();
    assert!(last_one > last_two + 60, "{effect_voices:?}");
    assert_eq!(*effect_voices.last().unwrap(), 0, "{effect_voices:?}");
    assert_eq!(audio.effects().cue_count(), 0);
}
