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

/// Plays the shot's standalone effects at the camera's focus (in a row, 8 m apart), each
/// started `age` ticks before the captured tick, travelling layers from 6 m west.
fn play_effects(options: &Options, loaded: &world::Loaded, game: &mut world::Match) -> Result<()> {
    use canopy_render::water::ripples::WaterField;
    if options.effects.is_empty() {
        return Ok(());
    }
    let [fx, fz] = options
        .camera
        .and_then(CameraChoice::pose)
        .map(|p| [p.x, p.z])
        .or_else(|| loaded.scene.starts.first().copied())
        .unwrap_or([0.0; 2]);
    let n = options.effects.len() as f32;
    let terrain = loaded.scene.terrain();
    for (i, spec) in options.effects.iter().enumerate() {
        let effect = game
            .presenter
            .effects
            .find(&spec.id)
            .with_context(|| format!("--effect {}: no such effect in the pack", spec.id))?;
        let x = fx + (i as f32 - (n - 1.0) / 2.0) * 8.0;
        let y = terrain.walk(x, fz, 0);
        game.presenter.effects.play(canopy_scene::effects::Play {
            effect,
            tick: f64::from(options.tick) - f64::from(spec.age),
            seed: 1 + i as u32,
            at: glam::Vec3::new(x, y, fz),
            from: Some(glam::Vec3::new(x - 6.0, y + 1.0, fz)),
            radius: spec.radius,
        });
    }
    Ok(())
}

/// `--damage`: sets every built building's health to the listed shares in turn (by entity
/// id), so fires can be reviewed at chosen levels. A dev tool: it edits the match state.
fn apply_damage(options: &Options, game: &mut world::Match) {
    use canopy_content::game::Kind;
    if options.damage.is_empty() {
        return;
    }
    let world = &mut game.world;
    let catalog = world.catalog();
    let state = world.state();
    let targets: Vec<(canopy_sim::entity::EntityId, u32)> = state
        .entities
        .iter()
        .filter(|e| e.construction.is_none() && e.resource.is_none())
        .filter(|e| e.hp.is_some_and(|hp| hp > 0))
        .filter(|e| catalog.get(e.definition).kind == Kind::Building)
        .map(|e| {
            let research = canopy_sim::stats::research(state, e.owner);
            (
                e.id,
                canopy_sim::stats::Stats::of(catalog, e, research).max_hp,
            )
        })
        .collect();
    let state = world.state_mut();
    for (i, (id, max_hp)) in targets.into_iter().enumerate() {
        let share = options.damage[i % options.damage.len()] / 100.0;
        if let Some(e) = state.entities.get_mut(id) {
            e.hp = Some(((max_hp as f32 * share).round() as u32).max(1));
        }
    }
}

pub fn run(options: &Options, paths: &Paths, out: &Path, size: [u32; 2]) -> Result<()> {
    if options.play || options.observe {
        return crate::play::capture::run(
            options,
            paths,
            out,
            size,
            options.observe,
            &options.input,
        );
    }
    let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
    let map = options.map.as_deref().context("shot needs --map")?;
    let (loaded, mut game) = world::load(
        paths,
        &MatchSpec {
            map,
            hour: options.hour,
            players: options.players,
            crowd: options.crowd,
            gallery: options.gallery,
            colors: options.colors.clone(),
            biome: options.biome.as_deref(),
            human: None,
        },
    )?;

    // Simulate to the requested tick, presenting every tick (animation clocks run on ticks).
    let started = std::time::Instant::now();
    let seat = options
        .viewer
        .map_or(sim::Seat::Observer, sim::Seat::Player);
    if options.tick == 0 {
        play_effects(options, &loaded, &mut game)?;
        apply_damage(options, &mut game);
    }
    let (mut newest, _) = sim::present(&game.world, &mut game.presenter, seat);
    let mut previous = None;
    let mut costs = Vec::new();
    while game.world.tick() < options.tick {
        if game.world.tick() + 1 == options.tick {
            play_effects(options, &loaded, &mut game)?;
            apply_damage(options, &mut game);
        }
        let Some(advanced) = sim::advance(
            &mut game.world,
            &mut game.session,
            &mut game.presenter,
            seat,
            None,
        ) else {
            anyhow::bail!("the local session stopped at tick {}", game.world.tick());
        };
        costs.push(advanced.cost);
        previous = Some(std::mem::replace(&mut newest, advanced.presentation));
    }
    let simulate_ms = started.elapsed().as_secs_f32() * 1000.0;

    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, size, loaded.environment.clone());
    renderer.shadows = options.shadows;
    renderer.set_quality(&gpu, options.quality);
    let started = std::time::Instant::now();
    renderer.load_world(&gpu, &loaded.pack, &loaded.scene.world)?;
    // The HUD's images, written next to the shot (`<out>-minimap.png`, `-portrait.png`).
    let hud_images = std::env::var_os("CANOPY_SHOT_HUD").is_some();
    if hud_images {
        renderer.bake_minimap(&gpu, loaded.scene.size, 256);
        renderer.set_portrait(&gpu, None, 256);
    }
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    let upload_ms = started.elapsed().as_secs_f32() * 1000.0;
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
    let camera_view = match options.pitch {
        // A free pitch for looking past the border: no map fit (the fit assumes the
        // game's pitch).
        Some(pitch) => {
            camera.pitch = pitch.to_radians();
            camera.view(aspect, loaded.scene.terrain())
        }
        None => view::frame(&mut camera, aspect, &loaded.scene),
    };
    let mut live = LiveScene::default();
    live.eye = Some(camera_view.eye);
    live.attach_world_ui(
        &paths.data,
        &loaded.pack,
        &game.world.catalog().content,
        loaded.scene.colors.clone(),
    );
    live.world_ui_input.health = options.health;
    live.world_ui_input.viewer = options.viewer;
    live.world_ui_input.selected = match &options.select {
        crate::options::Selection::None => Vec::new(),
        crate::options::Selection::All => newest.bodies.iter().map(|b| b.id).collect(),
        crate::options::Selection::Player(slot) => newest
            .bodies
            .iter()
            .filter(|b| b.owner == Some(*slot))
            .map(|b| b.id)
            .collect(),
        crate::options::Selection::Ids(ids) => ids.clone(),
    };
    live.attach_effects(
        &mut renderer,
        &gpu,
        &loaded.pack,
        game.presenter.effects.effect_table().clone(),
    );
    live.show(
        &mut renderer,
        &gpu,
        &loaded.pack,
        previous.as_ref(),
        &newest,
        1.0,
    );
    // The render thread's cost of a frame of the live scene (interpolation, effects,
    // world UI), measured on the shown pair.
    let live_started = std::time::Instant::now();
    for _ in 0..20 {
        live.show(
            &mut renderer,
            &gpu,
            &loaded.pack,
            previous.as_ref(),
            &newest,
            0.5,
        );
        live.draw_world_ui(
            &mut renderer,
            &gpu,
            &camera_view,
            &loaded.scene,
            options.time,
        );
    }
    let live_ms = live_started.elapsed().as_secs_f32() * 1000.0 / 20.0;
    live.show(
        &mut renderer,
        &gpu,
        &loaded.pack,
        previous.as_ref(),
        &newest,
        1.0,
    );
    renderer.set_cutaway(live.cutaway_subjects());

    let capture = Capture::new(&gpu.device, size[0], size[1]);

    // Warm-up frames (pipelines, first-use costs), then measured ones.
    let frames = options.frames.max(1);
    let mut reports = Vec::new();
    // Submit-to-idle time of each measured frame (Apple GPUs time passes coarsely).
    let mut walls: Vec<f32> = Vec::new();
    let copy = |encoder: &mut wgpu::CommandEncoder| capture.copy(encoder);
    for i in 0..frames {
        let hook: Option<&dyn Fn(&mut wgpu::CommandEncoder)> =
            if i + 1 == frames { Some(&copy) } else { None };
        if options.movers == 0 {
            // The match's ground units (still: a shot shows one tick).
            live.feed_ripples(
                &mut renderer,
                &loaded.scene,
                options.time - (frames - 1 - i) as f32 / 60.0,
                [camera_view.target.x, camera_view.target.z],
            );
        } else {
            // Synthetic bodies walking east through the focus at 2.5 units/s over the
            // frames before the capture, one 60 Hz frame apart, to show their wakes.
            let t = options.time - (frames - 1 - i) as f32 / 60.0;
            let focus = [camera_view.target.x, camera_view.target.z];
            let movers: Vec<canopy_render::water::ripples::Mover> = (0..options.movers)
                .map(|m| {
                    let row = m as f32 - options.movers as f32 / 2.0;
                    canopy_render::water::ripples::Mover {
                        id: m,
                        x: focus[0] - 20.0 + 2.5 * t + (m % 3) as f32 * 1.5,
                        z: focus[1] + row * 2.0,
                        surface: 0,
                        elevation: 0.0,
                    }
                })
                .collect();
            renderer
                .ripples_mut()
                .update(&movers, loaded.scene.terrain(), t, focus);
        }
        let frame_start = std::time::Instant::now();
        live.eye = Some(camera_view.eye);
        live.draw_world_ui(
            &mut renderer,
            &gpu,
            &camera_view,
            &loaded.scene,
            options.time - (frames - 1 - i) as f32 / 60.0,
        );
        if hud_images {
            live.draw_minimap(&mut renderer, &camera_view, &loaded.scene, 256, &[]);
            let subject = live
                .world_ui_input
                .selected
                .first()
                .and_then(|&id| live.portrait(id, options.time));
            renderer.set_portrait(&gpu, subject, 256);
        }
        let report = renderer.render(&gpu, &camera_view, options.time, &capture.view, hook);
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        if i > 0 {
            walls.push(frame_start.elapsed().as_secs_f32() * 1000.0);
        }
        reports.push(report);
    }
    // One more frame lets the last timestamps arrive.
    let pixels = capture.read(&gpu.device)?;
    if hud_images {
        for (name, texture) in [
            ("minimap", renderer.minimap_texture()),
            ("portrait", renderer.portrait_texture()),
        ] {
            let Some(texture) = texture else { continue };
            let image = Capture::new(&gpu.device, 256, 256);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            image.copy_from(&mut encoder, texture);
            gpu.queue.submit([encoder.finish()]);
            let rgba = image.read(&gpu.device)?;
            let path = out.with_file_name(format!(
                "{}-{name}.png",
                out.file_stem().and_then(|s| s.to_str()).unwrap_or("shot")
            ));
            image::save_buffer(&path, &rgba, 256, 256, image::ExtendedColorType::Rgba8)?;
        }
    }
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
        "  draws {} | terrain chunks {} triangles {} | foundations {} | statics {:?} | actors {:?}",
        last.draws,
        last.terrain_chunks,
        last.terrain_triangles,
        last.foundations,
        renderer.statics().map(|s| s.stats),
        last.actors,
    );
    println!(
        "  effects: {} cues, {:?} | live scene {live_ms:.3} ms/frame (interpolation, effects, world UI)",
        newest.effects.cues.len(),
        last.effects
    );
    if std::env::var_os("CANOPY_SHOT_EFFECTS").is_some() {
        let table = game.presenter.effects.effect_table();
        let mut counts: std::collections::BTreeMap<String, u32> = Default::default();
        for cue in &newest.effects.cues {
            let e = &table.effects[usize::from(cue.effect)];
            *counts
                .entry(format!("{}/{}", e.id, e.layers[usize::from(cue.layer)].id))
                .or_default() += 1;
        }
        for (name, n) in counts {
            println!("    {n:4} {name}");
        }
    }
    if std::env::var_os("CANOPY_SHOT_MODELS").is_some()
        && let Some(actors) = renderer.actors()
    {
        for (name, count) in actors.model_counts().filter(|(_, n)| *n > 0) {
            println!("    {count:4} {name}");
        }
    }
    let scene = renderer.scene_size();
    println!(
        "  scene {}×{} | water {:?} | forest {:?}",
        scene[0],
        scene[1],
        last.water,
        renderer.forest().map(|f| f.stats)
    );
    println!(
        "  cpu record {cpu:.2} ms/frame over {} frames",
        measured.len()
    );
    if !walls.is_empty() {
        walls.sort_by(f32::total_cmp);
        println!(
            "  frame to idle: median {:.2} ms, best {:.2} ms",
            walls[walls.len() / 2],
            walls[0]
        );
    }
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
    // `canopy bench-render` runs shots with `CANOPY_SHOT_BENCH=<frames>` and reads this
    // line: whole frames and isolated passes, submit to idle (canopy_render::bench).
    if let Some(frames) = std::env::var("CANOPY_SHOT_BENCH")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        let bench = renderer.benchmark(&gpu, &camera_view, options.time, &capture.view, frames);
        let line = serde_json::json!({
            "bench": bench,
            "live_ms": live_ms,
            "visible_models": last.actors.visible,
            "shadow_casters": last.actors.shadow_casters,
            "joints": last.actors.joints,
            "particles": last.effects.parts,
            "scene": renderer.scene_size(),
            "gpu": gpu.name(),
        });
        println!("bench-json {line}");
    }
    Ok(())
}
