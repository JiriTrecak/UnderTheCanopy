//! The mixer (kira's renderer, as the audio thread runs it) never allocates or frees
//! memory while sounds start, play, loop, move, fade and stop. Debug builds only:
//! `assert_no_alloc` counts violations there.
#![cfg(debug_assertions)]

use assert_no_alloc::{AllocDisabler, assert_no_alloc, violation_count};
use canopy_audio::{At, Audio, AudioSettings, Category, Frame, Listener, PlayOptions, Playlist};

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

#[test]
fn rendering_never_allocates() {
    let mut audio = Audio::headless(AudioSettings::default());
    let samples: Vec<i16> = (0..4800).map(|i| ((i * 37) % 2000) as i16).collect();
    let short = audio.library_mut().insert_pcm("short", 24_000, 1, &samples);
    let long = audio.library_mut().insert_pcm("long", 48_000, 2, &samples);
    audio.play_music(Playlist::new(vec![long, short]));
    let looping = PlayOptions {
        looping: true,
        category: Category::Effect,
        pitch: 1.3,
        ..PlayOptions::default()
    };
    let handle = audio
        .play_with(long, At::ground(5.0, 0.0), looping)
        .unwrap();
    // Warm up: lazily initialised locks and the first blocks.
    audio.render_headless(2);
    let before = violation_count();
    for frame in 0..240u32 {
        // The main thread may allocate: new sounds, moves, stops, speed changes.
        if frame % 5 == 0 {
            let x = (frame % 60) as f32 - 30.0;
            audio.play_with(
                short,
                At::ground(x, 3.0),
                PlayOptions::category(Category::Effect),
            );
        }
        audio.set_at(handle, At::ground((frame as f32 * 0.3).sin() * 40.0, 0.0));
        if frame == 200 {
            audio.stop(handle, Some(0.1));
        }
        audio.update(&Frame {
            real_dt: 1.0 / 60.0,
            render_tick: f64::from(frame) * 40.0 / 60.0,
            speed: if frame % 50 < 45 { 1.0 } else { 0.0 },
            listener: Listener::new(0.0, 0.0, 1.0, 0.0),
        });
        // The audio thread may not.
        assert_no_alloc(|| audio.render_headless(6));
    }
    assert!(audio.stats().sources > 0);
    assert_eq!(violation_count(), before, "the mixer allocated");
}
