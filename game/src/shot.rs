//! `canopy shot`: renders one view offscreen at a fixed size and writes a PNG, with
//! timings for the captured view. With `--tick N` the match (computer players) is first
//! simulated to tick N on this thread, deterministically, so mid-match states can be
//! captured; the picture shows tick N exactly.

use crate::{
    live::LiveScene,
    options::{CameraChoice, CameraSpec, Options},
    paths::Paths,
    sim, view,
    world::{self, MatchSpec},
};
use anyhow::{Context, Result};
use canopy_render::{
    DeviceOptions, Gpu, Renderer,
    capture::{CAPTURE_FORMAT, Capture},
};
use std::path::Path;

pub fn run(options: &Options, paths: &Paths, out: &Path, size: [u32; 2]) -> Result<()> {
    let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
    let map = options.map.as_deref().context("shot needs --map")?;
    let (loaded, mut game) = world::load(
        paths,
        &MatchSpec {
            map,
            hour: options.hour,
            players: options.players,
            crowd: options.crowd,
            colors: options.colors.clone(),
        },
    )?;

    // Simulate to the requested tick, presenting every tick (animation clocks run on ticks).
    let started = std::time::Instant::now();
    let mut newest = sim::present(&game.world, &mut game.presenter);
    let mut previous = None;
    let mut costs = Vec::new();
    while game.world.tick() < options.tick {
        let Some((presentation, cost, _)) =
            sim::advance(&mut game.world, &mut game.session, &mut game.presenter)
        else {
            anyhow::bail!("the local session stopped at tick {}", game.world.tick());
        };
        costs.push(cost);
        previous = Some(std::mem::replace(&mut newest, presentation));
    }
    let simulate_ms = started.elapsed().as_secs_f32() * 1000.0;

    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, size, loaded.environment.clone());
    renderer.shadows = options.shadows;
    let started = std::time::Instant::now();
    renderer.load_world(&gpu, &loaded.pack, &loaded.scene.world)?;
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    let upload_ms = started.elapsed().as_secs_f32() * 1000.0;
    let mut live = LiveScene::default();
    live.show(
        &mut renderer,
        &gpu,
        &loaded.pack,
        previous.as_ref(),
        &newest,
        1.0,
    );

    let spec = match options.camera {
        Some(CameraChoice::Battle(zoom)) => {
            let [x, z] = newest.battle.unwrap_or_else(|| {
                eprintln!(
                    "no fight at tick {}: looking at the first start",
                    newest.tick
                );
                loaded.scene.starts.first().copied().unwrap_or([0.0; 2])
            });
            Some(CameraSpec {
                x,
                z,
                zoom,
                yaw: 0.0,
            })
        }
        other => other.and_then(CameraChoice::pose),
    };
    let mut camera = view::initial(spec, &loaded.scene);
    let aspect = size[0] as f32 / size[1] as f32;
    let camera_view = view::frame(&mut camera, aspect, &loaded.scene);
    let capture = Capture::new(&gpu.device, size[0], size[1]);

    // Warm-up frames (pipelines, first-use costs), then measured ones.
    let frames = options.frames.max(1);
    let mut reports = Vec::new();
    let copy = |encoder: &mut wgpu::CommandEncoder| capture.copy(encoder);
    for i in 0..frames {
        let hook: Option<&dyn Fn(&mut wgpu::CommandEncoder)> =
            if i + 1 == frames { Some(&copy) } else { None };
        let report = renderer.render(&gpu, &camera_view, options.time, &capture.view, hook);
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        reports.push(report);
    }
    // One more frame lets the last timestamps arrive.
    let pixels = capture.read(&gpu.device)?;
    let late = renderer.render(&gpu, &camera_view, options.time, &capture.view, None);
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    image::save_buffer(
        out,
        &pixels,
        size[0],
        size[1],
        image::ExtendedColorType::Rgba8,
    )
    .with_context(|| out.display().to_string())?;

    let spec = CameraSpec {
        x: camera.focus[0],
        z: camera.focus[1],
        zoom: camera.zoom(),
        yaw: camera.yaw.to_degrees(),
    };
    let measured = if reports.len() > 1 {
        &reports[1..]
    } else {
        &reports[..]
    };
    let cpu: f32 = measured.iter().map(|r| r.cpu_total_ms).sum::<f32>() / measured.len() as f32;
    let last = reports.last().expect("a frame");
    println!(
        "{}: {map} tick {} at {:.1},{:.1} zoom {:.3} yaw {:.0} ({}×{}), hour {:.1}",
        out.display(),
        newest.tick,
        spec.x,
        spec.z,
        spec.zoom,
        spec.yaw,
        size[0],
        size[1],
        options.hour.unwrap_or(loaded.scene.hour)
    );
    if !costs.is_empty() {
        let n = costs.len() as f32;
        let mean = |f: fn(&sim::TickCost) -> f32| costs.iter().map(f).sum::<f32>() / n;
        let max = costs.iter().map(sim::TickCost::total).fold(0.0, f32::max);
        println!(
            "  simulated {} ticks in {simulate_ms:.0} ms: step {:.2} + view {:.2} + present {:.2} ms/tick (max {max:.1})",
            costs.len(),
            mean(|c| c.step),
            mean(|c| c.view),
            mean(|c| c.present),
        );
    }
    let c = newest.counts;
    println!(
        "  live: {} units, {} buildings, {} corpses, {} felling, {} missiles, {} shells; {} models listed",
        c.units,
        c.buildings,
        c.corpses,
        c.felling,
        c.missiles,
        c.shells,
        newest.models.len()
    );
    for (part, name) in [
        (canopy_scene::live::Part::Tree, "felling"),
        (canopy_scene::live::Part::Corpse, "corpse"),
        (canopy_scene::live::Part::Missile, "missile"),
        (canopy_scene::live::Part::Scaffold, "scaffold"),
    ] {
        let at: Vec<String> = newest
            .actors
            .iter()
            .filter(|a| a.key >> 40 == part as u64)
            .take(8)
            .map(|a| {
                format!(
                    "{:.0},{:.0},{:.1} clip {} t {:.2}",
                    a.position.x,
                    a.position.z,
                    a.position.y,
                    a.poses.poses[0].clip,
                    a.poses.poses[0].time
                )
            })
            .collect();
        if !at.is_empty() {
            println!("  {name} at {}", at.join(" "));
        }
    }
    println!(
        "  gpu {} | world upload {upload_ms:.0} ms | gpu memory {:.1} MiB",
        gpu.name(),
        renderer.gpu_bytes() as f64 / (1024.0 * 1024.0)
    );
    println!(
        "  draws {} | terrain chunks {} triangles {} | statics {:?} | actors {:?}",
        last.draws,
        last.terrain_chunks,
        last.terrain_triangles,
        renderer.statics().map(|s| s.stats),
        last.actors,
    );
    if std::env::var_os("CANOPY_SHOT_MODELS").is_some()
        && let Some(actors) = renderer.actors()
    {
        for (name, count) in actors.model_counts().filter(|(_, n)| *n > 0) {
            println!("    {count:4} {name}");
        }
    }
    println!(
        "  cpu record {cpu:.2} ms/frame over {} frames",
        measured.len()
    );
    if let Some(gpu_ms) = &late.gpu_ms {
        let total: f32 = gpu_ms.iter().sum();
        let passes: Vec<String> = late
            .passes
            .iter()
            .zip(gpu_ms)
            .map(|(name, ms)| format!("{name} {ms:.2}"))
            .collect();
        println!("  gpu {total:.2} ms: {}", passes.join(", "));
    }
    Ok(())
}
