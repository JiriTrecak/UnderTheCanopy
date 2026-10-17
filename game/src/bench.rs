//! `canopy bench-render`: the renderer's benchmark over fixed scenes (renderer §9).
//!
//! ```text
//! canopy bench-render [--data <folder>] [--frames <n>] [--sizes <w>x<h>,…]
//!                     [--scenes default,spectator,crowd,water,forest,menu,hud]
//!                     [--label <name>] [shot options…]
//! ```
//!
//! Every world scene runs as its own `canopy shot` process (a clean device and the same
//! deterministic tick each time) with `CANOPY_SHOT_BENCH=<frames>`, which measures whole
//! frames and every pass alone from submit to idle (`canopy_render::bench`). Options the
//! benchmark does not know (graphics: `--preset`, `--render-scale`, `--haze`, …) go to
//! every shot. `menu` renders the main menu in this process; `hud` reads the play
//! capture's HUD cost per frame (CPU). Results print as a table and are written to
//! `target/bench-render/<label>.json` and `.md`.

use crate::{paths::Paths, screens, settings::Settings};
use anyhow::{Context, Result, bail};
use canopy_render::{DeviceOptions, Gpu, Renderer, capture::Capture};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A fixed view: name, what it shows and its shot arguments.
struct Scene {
    name: &'static str,
    about: &'static str,
    args: &'static [&'static str],
}

const SCENES: &[Scene] = &[
    Scene {
        name: "default",
        about: "echo-isles mid-game, player 1's base at the default zoom",
        args: &[
            "--map",
            "echo-isles",
            "--tick",
            "2400",
            "--camera",
            "117.5,225.5,1",
        ],
    },
    Scene {
        name: "spectator",
        about: "echo-isles mid-game, the map's centre zoomed out to 3× (spectator)",
        args: &[
            "--map",
            "echo-isles",
            "--tick",
            "2400",
            "--camera",
            "320,320,3",
        ],
    },
    Scene {
        name: "crowd",
        about: "turtle-rock 8 × 50 crowd battle at zoom 1.6 with its effects (256 cues)",
        args: &[
            "--map",
            "turtle-rock",
            "--crowd",
            "50",
            "--tick",
            "120",
            "--camera",
            "325.5,331.5,1.6",
        ],
    },
    Scene {
        name: "water",
        about: "turtle-rock west edge over deep water, 12 wading bodies",
        args: &[
            "--map",
            "turtle-rock",
            "--camera",
            "20,320,1.25",
            "--movers",
            "12",
        ],
    },
    Scene {
        name: "forest",
        about: "echo-isles player 1's base in the deep forest look (bloom and haze)",
        args: &[
            "--map",
            "echo-isles",
            "--biome",
            "deep-forest",
            "--camera",
            "117.5,225.5,1",
        ],
    },
];

const DEFAULT_SCENES: &str = "default,spectator,crowd,water,forest,menu,hud";

/// One measured scene at one size.
struct Result1 {
    scene: String,
    size: [u32; 2],
    value: Value,
}

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut data = None;
    let mut frames = 60u32;
    let mut sizes = vec![[1920, 1080], [2560, 1600]];
    let mut scenes = DEFAULT_SCENES.to_string();
    let mut label = "latest".to_string();
    let mut extra: Vec<String> = Vec::new();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--data" => data = Some(PathBuf::from(value("--data")?)),
            "--frames" => frames = value("--frames")?.parse().context("--frames")?,
            "--scenes" => scenes = value("--scenes")?,
            "--label" => label = value("--label")?,
            "--sizes" => {
                sizes = value("--sizes")?
                    .split(',')
                    .map(|s| {
                        let (w, h) = s.split_once('x').context("--sizes <w>x<h>,…")?;
                        Ok([w.parse()?, h.parse()?])
                    })
                    .collect::<Result<_>>()?;
            }
            _ => extra.push(arg),
        }
    }
    let paths = Paths::resolve(data)?;
    let exe = std::env::current_exe()?;
    let out_dir = paths.data.join("target").join("bench-render");
    std::fs::create_dir_all(&out_dir)?;
    let mut results = Vec::new();
    for name in scenes.split(',').map(str::trim) {
        for &size in &sizes {
            let value = match name {
                "menu" => menu(&paths, size, frames)?,
                "hud" => hud(&exe, &paths, &out_dir, size)?,
                _ => {
                    let scene = SCENES
                        .iter()
                        .find(|s| s.name == name)
                        .with_context(|| format!("no scene {name} (known: {DEFAULT_SCENES})"))?;
                    shot(&exe, &paths, &out_dir, scene, size, frames, &extra)?
                }
            };
            eprintln!("{name} {}×{}: {}", size[0], size[1], summary(&value));
            results.push(Result1 {
                scene: name.to_string(),
                size,
                value,
            });
        }
    }
    let table = table(&results, &extra, frames);
    println!("{table}");
    let json = Value::Array(
        results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "scene": r.scene,
                    "size": r.size,
                    "result": r.value,
                })
            })
            .collect(),
    );
    std::fs::write(
        out_dir.join(format!("{label}.json")),
        serde_json::to_string_pretty(&json)?,
    )?;
    std::fs::write(out_dir.join(format!("{label}.md")), &table)?;
    println!("written to {}", out_dir.join(&label).display());
    Ok(())
}

fn shot(
    exe: &std::path::Path,
    paths: &Paths,
    out_dir: &std::path::Path,
    scene: &Scene,
    size: [u32; 2],
    frames: u32,
    extra: &[String],
) -> Result<Value> {
    let png = out_dir.join(format!("{}-{}x{}.png", scene.name, size[0], size[1]));
    let output = std::process::Command::new(exe)
        .arg("shot")
        .arg("--data")
        .arg(&paths.data)
        .args(scene.args)
        .args(["--size", &format!("{}x{}", size[0], size[1])])
        .args(["--frames", "2"])
        .arg("--out")
        .arg(&png)
        .args(extra)
        .env("CANOPY_SHOT_BENCH", frames.to_string())
        .env("RUST_LOG", "warn")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        bail!(
            "shot {} failed: {}{}",
            scene.name,
            stdout,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("bench-json "))
        .with_context(|| format!("shot {} printed no benchmark", scene.name))?;
    Ok(serde_json::from_str(line)?)
}

/// The play capture's HUD cost (update, bind and paint per frame, CPU).
fn hud(
    exe: &std::path::Path,
    paths: &Paths,
    out_dir: &std::path::Path,
    size: [u32; 2],
) -> Result<Value> {
    let png = out_dir.join(format!("hud-{}x{}.png", size[0], size[1]));
    let output = std::process::Command::new(exe)
        .arg("shot")
        .arg("--data")
        .arg(&paths.data)
        .args(["--map", "echo-isles", "--play", "--input", "select:worker"])
        .args(["--size", &format!("{}x{}", size[0], size[1])])
        .arg("--out")
        .arg(&png)
        .env("RUST_LOG", "warn")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let ms = stdout
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("hud: update + bind + paint ")?
                .split_whitespace()
                .next()?
                .parse::<f64>()
                .ok()
        })
        .with_context(|| format!("the HUD capture printed no cost: {stdout}"))?;
    Ok(serde_json::json!({ "hud_cpu_ms": ms }))
}

/// The main menu (UI only): CPU of a frame's update and recording, and to idle.
fn menu(paths: &Paths, size: [u32; 2], frames: u32) -> Result<Value> {
    let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
    let user = std::env::temp_dir().join(format!("canopy-bench-{}", std::process::id()));
    std::fs::create_dir_all(&user)?;
    let paths = Paths {
        data: paths.data.clone(),
        user: user.clone(),
        cache: user.join("cache"),
        replays: user.join("replays"),
    };
    let look = canopy_scene::Look::load(&paths.data, canopy_content::map::BIOMES[0])?;
    let environment = canopy_render::Environment::new(&look.lighting, &look.biome, 12.0, None);
    let mut renderer = Renderer::new(
        &gpu,
        canopy_render::capture::CAPTURE_FORMAT,
        size,
        environment,
    );
    let mut game = screens::Game::new(paths, Settings::default(), &gpu, None)?;
    game.offline_updates();
    let target = Capture::new(&gpu.device, size[0], size[1]);
    let frame = |game: &mut screens::Game, renderer: &mut Renderer| {
        let mut ctx = screens::Ctx {
            gpu: &gpu,
            renderer,
            window: None,
            size,
            scale: 1.0,
        };
        let start = Instant::now();
        game.update(&mut ctx);
        let updated = start.elapsed();
        game.render(&mut ctx, &target.view, &target.ui_view);
        let recorded = start.elapsed();
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        (updated, recorded, start.elapsed())
    };
    let limit = Instant::now() + Duration::from_secs(60);
    while !game.booted() {
        anyhow::ensure!(Instant::now() < limit, "the game did not boot");
        frame(&mut game, &mut renderer);
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..60 {
        frame(&mut game, &mut renderer);
    }
    let (mut update, mut cpu, mut wall) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..frames.max(3) {
        let (u, r, w) = frame(&mut game, &mut renderer);
        update.push(u.as_secs_f32() * 1000.0);
        cpu.push(r.as_secs_f32() * 1000.0);
        wall.push(w.as_secs_f32() * 1000.0);
        // The menu animates with wall time; pace it like a 60 Hz display.
        std::thread::sleep(Duration::from_millis(4));
    }
    let _ = std::fs::remove_dir_all(&user);
    let median = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let (u, c, w) = (median(&mut update), median(&mut cpu), median(&mut wall));
    Ok(serde_json::json!({
        "menu": { "update_ms": u, "cpu_ms": c, "frame_ms": w, "gpu_ms": (w - c).max(0.0) },
        "gpu": gpu.name(),
    }))
}

fn f(value: &Value, path: &[&str]) -> f64 {
    path.iter()
        .try_fold(value, |v, k| v.get(*k))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

fn summary(value: &Value) -> String {
    if value.get("hud_cpu_ms").is_some() {
        return format!("HUD {:.3} ms CPU", f(value, &["hud_cpu_ms"]));
    }
    if value.get("menu").is_some() {
        return format!(
            "frame {:.2} ms (cpu {:.2})",
            f(value, &["menu", "frame_ms"]),
            f(value, &["menu", "cpu_ms"])
        );
    }
    format!(
        "frame {:.2} ms (cpu {:.2}, gpu {:.2})",
        f(value, &["bench", "frame_ms"]),
        f(value, &["bench", "cpu_ms"]),
        f(value, &["bench", "gpu_ms"])
    )
}

/// The results as Markdown: one row per scene and size, passes in the last column.
fn table(results: &[Result1], extra: &[String], frames: u32) -> String {
    let gpu = results
        .iter()
        .find_map(|r| r.value.get("gpu").and_then(Value::as_str))
        .unwrap_or("?");
    let mut s = String::new();
    let _ = writeln!(
        s,
        "GPU {gpu}; {frames} frames per scene, fastest fifth (canopy_render::bench::QUANTILE); options: {}\n",
        if extra.is_empty() {
            "defaults".to_string()
        } else {
            extra.join(" ")
        }
    );
    let _ = writeln!(
        s,
        "| Scene | Size | Scene px | Frame ms | CPU rec | GPU | Live CPU | Draws | Fwd tris | Shadow tris | Passes alone (ms, submit to idle − floor) |"
    );
    let _ = writeln!(s, "|---|---|---|---|---|---|---|---|---|---|---|");
    for r in results {
        let v = &r.value;
        let size = format!("{}×{}", r.size[0], r.size[1]);
        if let Some(ms) = v.get("hud_cpu_ms").and_then(Value::as_f64) {
            let _ = writeln!(
                s,
                "| hud | {size} | | | {ms:.3} | | | | | | match HUD update + bind + paint (CPU) |"
            );
            continue;
        }
        if v.get("menu").is_some() {
            let _ = writeln!(
                s,
                "| menu | {size} | | {:.2} | {:.2} | {:.2} | | | | | UI update {:.2} ms |",
                f(v, &["menu", "frame_ms"]),
                f(v, &["menu", "cpu_ms"]),
                f(v, &["menu", "gpu_ms"]),
                f(v, &["menu", "update_ms"]),
            );
            continue;
        }
        let b = &v["bench"];
        let passes: Vec<String> = b["passes"]
            .as_array()
            .into_iter()
            .flatten()
            .zip(b["pass_ms"].as_array().into_iter().flatten())
            .filter_map(|(n, ms)| {
                let ms = ms.as_f64()?;
                (ms >= 0.005).then(|| format!("{} {ms:.2}", n.as_str().unwrap_or("?")))
            })
            .collect();
        let scene = v["scene"]
            .as_array()
            .map(|a| {
                format!(
                    "{}×{}",
                    a.first().and_then(Value::as_u64).unwrap_or(0),
                    a.get(1).and_then(Value::as_u64).unwrap_or(0)
                )
            })
            .unwrap_or_default();
        let _ = writeln!(
            s,
            "| {} | {size} | {scene} | {:.2} | {:.2} | {:.2} | {:.3} | {} | {:.0}k | {:.0}k | {} (floor {:.2}) |",
            r.scene,
            f(b, &["frame_ms"]),
            f(b, &["cpu_ms"]),
            f(b, &["gpu_ms"]),
            f(v, &["live_ms"]),
            f(b, &["draws"]),
            f(b, &["forward_triangles"]) / 1000.0,
            f(b, &["triangles", "shadow"]) / 1000.0,
            passes.join(", "),
            f(b, &["floor_ms"]),
        );
    }
    let _ = writeln!(s, "\nScenes:");
    for scene in SCENES {
        let _ = writeln!(
            s,
            "- `{}`: {} (`{}`)",
            scene.name,
            scene.about,
            scene.args.join(" ")
        );
    }
    let _ = writeln!(
        s,
        "- `menu`: the main menu (UI only), rendered in process\n- `hud`: the play capture's HUD on echo-isles (`shot --play --input select:worker`), CPU only"
    );
    s
}
