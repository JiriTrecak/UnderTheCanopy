//! Plays every sound of the cooked pack through the default output device: centred,
//! then sweeping from left to right in front of the listener, then moving away until it
//! is out of range. Listen for even panning and a smooth fall-off.
//!
//! ```text
//! just cook
//! cargo run -p canopy-audio --example play_pack [-- <pack>] [--headless] [--volume 0.5]
//! ```
//!
//! `--headless` runs the same steps without a device (and without waiting);
//! `--volume <0..1>` sets the master volume.

use canopy_audio::{
    At, Audio, AudioSettings, Category, Distances, Frame, Listener, PlayOptions, SoundId,
};
use std::{path::PathBuf, time::Duration};

const FPS: f32 = 60.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let headless = args.iter().any(|a| a == "--headless");
    let master = args
        .iter()
        .position(|a| a == "--volume")
        .and_then(|i| args.get(i + 1)?.parse::<f32>().ok())
        .unwrap_or(1.0);
    let pack_path = args
        .iter()
        .enumerate()
        .find(|&(i, a)| !a.starts_with("--") && (i == 0 || args[i - 1] != "--volume"))
        .map(|(_, a)| a)
        .map(PathBuf::from)
        .or_else(|| {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            canopy_assets::find_pack(&root)
        })
        .unwrap_or_else(|| {
            eprintln!("no pack: run `just cook` or pass the path of a canopy.pack");
            std::process::exit(1);
        });
    let pack = canopy_assets::Pack::load(&pack_path).unwrap_or_else(|e| {
        eprintln!("{}: {e}", pack_path.display());
        std::process::exit(1);
    });
    let settings = AudioSettings {
        master,
        ..AudioSettings::default()
    };
    let mut audio = if headless {
        Audio::headless(settings)
    } else {
        Audio::new(settings)
    };
    let count = audio.load(&pack).expect("sounds load");
    println!(
        "{count} sounds in {} ({})",
        pack_path.display(),
        if audio.has_output() && !headless {
            "playing on the default device"
        } else {
            "no device"
        }
    );
    let sounds: Vec<(SoundId, String, f64)> = audio
        .library()
        .iter()
        .map(|(id, clip)| (id, clip.name.clone(), clip.duration))
        .collect();
    let mut player = Player {
        audio,
        tick: 0.0,
        headless,
    };
    for (sound, name, duration) in sounds {
        println!("{name}: {duration:.2} s");
        let options = PlayOptions {
            category: Category::Effect,
            distances: Distances::new(12.0, 80.0),
            ..PlayOptions::default()
        };
        println!("  centred");
        player.play(sound, At::ground(0.0, 0.0), options, duration);
        println!("  left to right");
        for step in 0..5 {
            let x = -24.0 + 12.0 * step as f32;
            player.play(sound, At::ground(x, 0.0), options, duration);
        }
        println!("  moving away");
        for distance in [12.0, 30.0, 50.0, 70.0, 90.0] {
            let stats = player.play(sound, At::ground(0.0, distance), options, duration);
            println!("    {distance:>4} cells: {} voice(s)", stats);
        }
    }
}

struct Player {
    audio: Audio,
    tick: f64,
    headless: bool,
}

impl Player {
    /// Plays a sound and runs frames for `seconds`; returns the most voices seen.
    fn play(&mut self, sound: SoundId, at: At, options: PlayOptions, seconds: f64) -> u16 {
        self.audio.play_with(sound, at, options);
        let frames = (seconds * f64::from(FPS)).ceil() as usize + 6;
        let mut most = 0;
        for _ in 0..frames {
            self.tick += 40.0 / f64::from(FPS);
            self.audio.update(&Frame {
                real_dt: 1.0 / FPS,
                render_tick: self.tick,
                speed: 1.0,
                listener: Listener::new(0.0, 0.0, 1.0, 0.0),
            });
            most = most.max(self.audio.stats().voices[Category::Effect as usize]);
            if !self.headless {
                std::thread::sleep(Duration::from_secs_f32(1.0 / FPS));
            }
        }
        most
    }
}
