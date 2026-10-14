//! The whole pipeline on kira's mock backend: no audio device needed. Test-owned sounds.

use canopy_assets::{BlobBuilder, EntryKind, Pack, PackWriter, SoundMeta};
use canopy_audio::{
    At, Audio, AudioSettings, Category, Frame, Listener, PlayOptions, Playlist, SoundId,
};

fn frame(render_tick: f64, speed: f32) -> Frame {
    Frame {
        real_dt: 1.0 / 60.0,
        render_tick,
        speed,
        listener: Listener::new(0.0, 0.0, 1.0, 0.0),
    }
}

/// One second of a quiet tone at 8 kHz.
fn tone(audio: &mut Audio, name: &str) -> SoundId {
    let samples: Vec<i16> = (0..8000).map(|i| ((i % 40) * 100) as i16).collect();
    audio.library_mut().insert_pcm(name, 8000, 1, &samples)
}

#[test]
fn sounds_load_from_a_pack() {
    let path = std::env::temp_dir().join(format!("canopy-audio-{}.pack", std::process::id()));
    {
        let mut writer = PackWriter::new(std::fs::File::create(&path).unwrap()).unwrap();
        for (name, channels) in [
            ("assets/library/a/audio.wav", 1u16),
            ("assets/library/b/audio_2.wav", 2),
        ] {
            let mut builder = BlobBuilder::new();
            let samples = vec![100i16; 64 * usize::from(channels)];
            let meta = SoundMeta {
                sample_rate: 32,
                channels,
                frames: 64,
                samples: builder.push(&samples),
            };
            let blob = writer
                .write_blob(&builder.finish(EntryKind::Sound, &meta))
                .unwrap();
            writer.add_entry(name.to_owned(), EntryKind::Sound, blob);
        }
        let mut builder = BlobBuilder::new();
        let bytes = builder.push_bytes(b"not a sound");
        let blob = writer
            .write_blob(&builder.finish(EntryKind::Data, &canopy_assets::DataMeta { bytes }))
            .unwrap();
        writer.add_entry(
            "assets/library/c/data.bin".to_owned(),
            EntryKind::Data,
            blob,
        );
        writer.finish([0; 32]).unwrap();
    }
    let pack = Pack::load(&path).unwrap();
    let mut audio = Audio::headless(AudioSettings::default());
    assert_eq!(audio.load(&pack).unwrap(), 2);
    let b = audio.sound("assets/library/b/audio_2.wav").unwrap();
    assert_eq!(audio.library().clip(b).duration, 2.0);
    assert!(audio.sound("assets/library/c/data.bin").is_none());
    std::fs::remove_file(path).ok();
}

#[test]
fn a_crowd_of_identical_sounds_takes_few_voices() {
    let mut audio = Audio::headless(AudioSettings::default());
    assert!(audio.has_output());
    let hit = tone(&mut audio, "hit");
    let other = tone(&mut audio, "other");
    // Four hundred units strike within one frame: world one-shots of one sound merge.
    let handles: Vec<_> = (0..400)
        .filter_map(|i| audio.play(hit, At::ground(i as f32 * 0.1, 0.0)))
        .collect();
    assert_eq!(handles.len(), 1);
    // Effect layers do not merge, but only the loudest few of a sound get voices.
    let effect = PlayOptions::category(Category::Effect);
    for i in 0..400 {
        audio.play_with(hit, At::ground(i as f32 * 0.2, 0.0), effect);
        audio.play_with(other, At::ground(-(i as f32) * 0.2, 0.0), effect);
    }
    audio.update(&frame(1.0, 1.0));
    let stats = audio.stats();
    let budget = audio.limits().budget(Category::Effect);
    assert_eq!(
        stats.voices[Category::Effect as usize],
        2 * budget.per_sound
    );
    assert_eq!(stats.voices[Category::World as usize], 1);
    assert_eq!(
        usize::from(stats.playing),
        2 * usize::from(budget.per_sound) + 1
    );
    // The farthest are inaudible; the rest wait as virtual voices.
    assert!(stats.virtual_voices > 500);
    assert_eq!(stats.sources, 801);
}

#[test]
fn volumes_pause_and_background() {
    let mut audio = Audio::headless(AudioSettings::default());
    let sound = tone(&mut audio, "s");
    let looping = PlayOptions {
        looping: true,
        ..PlayOptions::default()
    };
    audio.play_with(sound, At::ground(1.0, 0.0), looping);
    let ui = PlayOptions {
        looping: true,
        ..PlayOptions::category(Category::Interface)
    };
    audio.play_with(sound, At::Listener, ui);
    let voices = |audio: &Audio| {
        let s = audio.stats();
        (
            s.voices[Category::World as usize],
            s.voices[Category::Interface as usize],
        )
    };
    audio.update(&frame(1.0, 1.0));
    assert_eq!(voices(&audio), (1, 1));
    // Paused: game sounds stop, the interface goes on.
    audio.update(&frame(1.0, 0.0));
    assert_eq!(voices(&audio), (0, 1));
    audio.update(&frame(2.0, 1.0));
    assert_eq!(voices(&audio), (1, 1));
    // A bus at zero takes no voices.
    audio.apply_settings(AudioSettings {
        effects: 0.0,
        ..AudioSettings::default()
    });
    audio.update(&frame(3.0, 1.0));
    assert_eq!(voices(&audio), (0, 1));
    audio.apply_settings(AudioSettings::default());
    audio.set_background(true);
    audio.update(&frame(4.0, 1.0));
    assert_eq!(voices(&audio), (0, 0));
    audio.set_background(false);
    audio.update(&frame(5.0, 1.0));
    assert_eq!(voices(&audio), (1, 1));
    assert_eq!(audio.stats().playing, 2);
    // A new match ends game sounds only.
    audio.clear_game();
    audio.update(&frame(0.0, 1.0));
    assert_eq!(voices(&audio), (0, 1));
}

#[test]
fn music_crossfades_between_playlists() {
    let mut audio = Audio::headless(AudioSettings::default());
    let a = tone(&mut audio, "a");
    let b = tone(&mut audio, "b");
    let mut first = Playlist::new(vec![a]);
    first.crossfade = 0.5;
    audio.play_music(first);
    audio.update(&frame(0.0, 0.0));
    assert_eq!(audio.stats().voices[Category::Music as usize], 1);
    let mut second = Playlist::new(vec![b]);
    second.crossfade = 0.5;
    audio.play_music(second);
    audio.update(&frame(0.0, 0.0));
    assert_eq!(audio.current_music(), Some(b));
    // Both play while crossfading.
    audio.update(&frame(0.0, 0.0));
    assert_eq!(audio.stats().voices[Category::Music as usize], 2);
    audio.stop_music(0.0);
    audio.update(&frame(0.0, 0.0));
    assert_eq!(audio.stats().voices[Category::Music as usize], 0);
}

#[test]
fn stopped_loops_fade_and_end() {
    let mut audio = Audio::headless(AudioSettings::default());
    let sound = tone(&mut audio, "loop");
    let handle = audio
        .play_with(
            sound,
            At::Listener,
            PlayOptions {
                looping: true,
                category: Category::Interface,
                ..PlayOptions::default()
            },
        )
        .unwrap();
    audio.update(&frame(0.0, 1.0));
    audio.stop(handle, Some(0.05));
    for _ in 0..5 {
        audio.update(&frame(0.0, 1.0));
    }
    assert!(!audio.is_live(handle));
    assert_eq!(audio.stats().playing, 0);
}
