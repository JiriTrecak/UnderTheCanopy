//! Sound events (D97) on a test-owned data folder: the events and everything that names
//! them are checked when content loads.

#[path = "../../sim/tests/abilities_fixture/mod.rs"]
mod abilities_fixture;

use abilities_fixture::*;
use canopy_content::testing::{DataFolder, output, record};
use serde_json::{Value, json};

fn clip(id: &str) -> Value {
    let mut clip = record(
        id,
        vec![output("data", &format!("{id}/audio.wav"))],
        json!([]),
        json!([]),
    );
    clip["kind"] = json!("audio");
    clip
}

fn sounds() -> Value {
    json!([
        {"id": "sound.test.knock", "category": "world", "clips": ["asset.test.knock", "asset.test.knock-2"],
         "volume": [0.8, 1.0], "pitch": [0.95, 1.05], "distance": {"reference": 10, "max": 50}},
        {"id": "sound.test.yes", "category": "voice", "clips": ["asset.test.knock"]},
        {"id": "sound.test.click", "category": "interface", "clips": ["asset.test.knock"]},
        {"id": "sound.test.theme", "category": "music", "clips": ["asset.test.knock"], "loop": true},
    ])
}

/// The fixture's game with a warrior that sounds and answers, and the game's own sounds.
fn folder(edit: impl FnOnce(&mut Value, &mut Value)) -> DataFolder {
    let mut game = game(&Spec::default());
    for d in game["definitions"].as_array_mut().unwrap() {
        if d["id"] == "unit.warrior" {
            d["presentation"] = json!({
                "sounds": [
                    {"sound": "sound.test.knock", "event": "hit"},
                    {"sound": "sound.test.knock", "state": "move"},
                    {"sound": "sound.test.knock", "on": "death"}
                ],
                "voice": {"select": "sound.test.yes", "attack": "sound.test.yes"}
            });
        }
    }
    game["rules"]["sounds"] = json!({"menu": ["sound.test.theme"],
        "interface": {"click": "sound.test.click"}});
    let mut sounds = sounds();
    edit(&mut game, &mut sounds);
    let mut records = records();
    records.extend([clip("asset.test.knock"), clip("asset.test.knock-2")]);
    let mut folder = DataFolder::new(game, records);
    folder.sounds = sounds;
    folder
}

/// A change to the folder's game and sounds.
type Edit = Box<dyn FnOnce(&mut Value, &mut Value)>;

fn errors(edit: impl FnOnce(&mut Value, &mut Value)) -> String {
    match folder(edit).load("sounds-broken") {
        Ok(_) => String::new(),
        Err(errors) => errors.to_string(),
    }
}

fn warrior(game: &mut Value) -> &mut Value {
    game["definitions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|d| d["id"] == "unit.warrior")
        .unwrap()
}

#[test]
fn sound_events_and_what_names_them_are_checked() {
    let content = folder(|_, _| {})
        .load("sounds")
        .unwrap_or_else(|e| panic!("{e}"));
    let knock = content.sounds.get("sound.test.knock").expect("loaded");
    assert_eq!(knock.clips.len(), 2);
    assert_eq!(content.rules.sounds.menu.len(), 1);

    let cases: [(&str, Edit); 6] = [
        (
            "is no audio asset",
            Box::new(|_, s| s[0]["clips"] = json!(["asset.test.icon"])),
        ),
        (
            "pitch must be",
            Box::new(|_, s| s[0]["pitch"] = json!([1.2, 0.9])),
        ),
        (
            "only world and ambience sounds have a distance",
            Box::new(|_, s| s[1]["distance"] = json!({"reference": 1, "max": 2})),
        ),
        (
            "sound.test.knock is a World sound",
            Box::new(|g, _| {
                warrior(g)["presentation"]["voice"]["select"] = json!("sound.test.knock")
            }),
        ),
        (
            "names one of a state",
            Box::new(|g, _| {
                warrior(g)["presentation"]["sounds"][0]["on"] = json!("death");
            }),
        ),
        (
            "unknown sound sound.test.none",
            Box::new(|g, _| g["rules"]["sounds"]["interface"]["click"] = json!("sound.test.none")),
        ),
    ];
    for (expected, edit) in cases {
        let found = errors(edit);
        assert!(found.contains(expected), "{expected}: {found}");
    }
}

/// A library with the folder's two test clips: two and three seconds of a quiet tone.
fn library() -> canopy_audio::SoundLibrary {
    let mut library = canopy_audio::SoundLibrary::new();
    for (n, asset) in ["asset.test.knock", "asset.test.knock-2"]
        .iter()
        .enumerate()
    {
        let samples = vec![1000i16; 200 + n * 100];
        library.insert_pcm(&format!("{asset}/audio.wav"), 100, 1, &samples);
    }
    library
}

fn content() -> canopy_content::Content {
    folder(|_, _| {})
        .load("sounds-audio")
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Audio on the mock backend with the folder's events bound to the test clips.
fn audio() -> canopy_audio::Audio {
    let mut audio = canopy_audio::Audio::headless(canopy_audio::AudioSettings::default());
    *audio.library_mut() = library();
    audio.bind(&content());
    audio
}

fn frame(audio: &mut canopy_audio::Audio, tick: f64) {
    audio.update(&canopy_audio::Frame {
        real_dt: 1.0 / 60.0,
        render_tick: tick,
        speed: 1.0,
        listener: canopy_audio::Listener::default(),
    });
}

#[test]
fn events_vary_their_clips_within_their_ranges() {
    let mut bank = canopy_audio::SoundBank::default();
    bank.bind(&content().sounds, &library());
    let knock = bank.find("sound.test.knock").expect("bound");
    assert_eq!(bank.clips(knock).len(), 2);
    let mut last = None;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..50 {
        let start = bank.start(knock, 0.0).expect("no cooldown");
        // A two-clip event never plays the same clip twice running.
        assert_ne!(Some(start.sound), last);
        last = Some(start.sound);
        seen.insert(start.sound);
        assert!((0.8..=1.0).contains(&start.options.volume));
        assert!((0.95..=1.05).contains(&start.options.pitch));
        assert_eq!(start.options.category, canopy_audio::Category::World);
    }
    assert_eq!(seen.len(), 2);
}

#[test]
fn loops_are_held_moved_and_let_go() {
    use canopy_audio::{At, LoopGroup};
    let mut audio = audio();
    let knock = audio.event("sound.test.knock").unwrap();
    audio.hold(
        LoopGroup::Bodies,
        [
            (1, knock, At::ground(0.0, 0.0)),
            (2, knock, At::ground(4.0, 0.0)),
        ],
    );
    frame(&mut audio, 0.0);
    assert_eq!(audio.stats().sources, 2);
    // Held again: the same two, moved; then one is let go and fades away.
    audio.hold(
        LoopGroup::Bodies,
        [
            (1, knock, At::ground(1.0, 0.0)),
            (2, knock, At::ground(4.0, 0.0)),
        ],
    );
    frame(&mut audio, 1.0);
    assert_eq!(audio.stats().sources, 2);
    audio.hold(LoopGroup::Bodies, [(1, knock, At::ground(1.0, 0.0))]);
    // Another group is its own.
    audio.hold(LoopGroup::Ambience, []);
    for tick in 2..40 {
        frame(&mut audio, f64::from(tick));
    }
    assert_eq!(audio.stats().sources, 1);
}

#[test]
fn a_new_answer_cuts_the_one_before_and_a_hush_ends_it() {
    use canopy_audio::{At, Category};
    let mut audio = audio();
    let yes = audio.event("sound.test.yes").unwrap();
    let first = audio.play_event(yes, At::Listener).expect("answers");
    frame(&mut audio, 0.0);
    assert!(audio.speaking(Category::Voice));
    let second = audio.play_event(yes, At::Listener).expect("answers again");
    for tick in 1..30 {
        frame(&mut audio, f64::from(tick));
    }
    assert!(!audio.is_live(first), "cut short");
    assert!(audio.is_live(second));
    audio.hush(Category::Voice, 0.1);
    for tick in 30..60 {
        frame(&mut audio, f64::from(tick));
    }
    assert!(!audio.is_live(second), "hushed");
}

#[test]
fn compressed_sounds_stream() {
    use canopy_audio::{At, Category, PlayOptions};
    let mp3 = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/tone.mp3"))
        .expect("tests/data/tone.mp3 (git lfs pull)");
    let mut audio = canopy_audio::Audio::headless(canopy_audio::AudioSettings::default());
    let tone = audio
        .library_mut()
        .insert_encoded("asset.test.tone/audio.mp3", &mp3)
        .unwrap_or_else(|e| panic!("{e}"));
    let duration = audio.library().clip(tone).duration;
    assert!((0.45..0.6).contains(&duration), "{duration}");
    assert_eq!(audio.library().find_asset("asset.test.tone"), Some(tone));
    audio.play_with(
        tone,
        At::Listener,
        PlayOptions::category(Category::Dialogue),
    );
    frame(&mut audio, 0.0);
    audio.render_headless(8);
    assert_eq!(audio.stats().voices[Category::Dialogue as usize], 1);
    assert_eq!(audio.stats().playing, 1);
    assert!(
        audio
            .library_mut()
            .insert_encoded("broken.mp3", b"not a sound")
            .is_err()
    );
}
