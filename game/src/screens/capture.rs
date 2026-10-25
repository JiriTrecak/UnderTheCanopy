//! Offscreen captures of every screen: `canopy capture-screens --out <folder>
//! [--size <w>x<h>] [--scale <s>]` (docs/captures/m4-screens).
//!
//! The real [`Game`] runs headless: the same flow, models, layouts and pack images as in
//! the window, driven through its intents and actions; matches really load and run on
//! the simulation thread and their frame renders behind the UI. Saves go to a temporary
//! user folder. Multiplayer shows a real server when `CANOPY_MATCH_SERVER` names one (a
//! local match server, `just server`): the capture opens a second client's lobby so the
//! list has a room, joins it, then hosts a lobby with computer and closed seats. No shipped map is a mission, so the mission HUD captures
//! feed a sample mission state (marked `-sample` in their names). The `editor` group
//! renders the editor (`canopy_editor`) offscreen in a few states.

use super::{
    Ctx, Game, Intent, Overlay, Screen,
    match_ui::{MenuPage, MissionHud},
};
use crate::{paths::Paths, settings::Settings};
use anyhow::{Context, Result};
use canopy_render::{DeviceOptions, Gpu, Renderer, capture::CAPTURE_FORMAT};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// An sRGB target with a non-sRGB view for the UI, read back as RGBA8.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    ui_view: wgpu::TextureView,
    buffer: wgpu::Buffer,
    size: [u32; 2],
    padded_row: u32,
}

impl Target {
    fn new(gpu: &Gpu, size: [u32; 2]) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen capture"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAPTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[CAPTURE_FORMAT.remove_srgb_suffix()],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(CAPTURE_FORMAT.remove_srgb_suffix()),
            ..Default::default()
        });
        let padded_row = (size[0] * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen capture readback"),
            size: u64::from(padded_row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            ui_view,
            buffer,
            size,
            padded_row,
        }
    }

    fn read(&self, gpu: &Gpu) -> Result<Vec<u8>> {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.size[1]),
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let mut out = Vec::with_capacity((self.size[0] * self.size[1] * 4) as usize);
        {
            let mapped = self.buffer.get_mapped_range(..)?;
            for row in mapped.chunks_exact(self.padded_row as usize) {
                out.extend_from_slice(&row[..self.size[0] as usize * 4]);
            }
        }
        self.buffer.unmap();
        Ok(out)
    }
}

#[path = "capture_ui.rs"]
mod ui;

struct Rig {
    gpu: Gpu,
    renderer: Renderer,
    target: Target,
    game: Game,
    size: [u32; 2],
    scale: f32,
    out: PathBuf,
    written: Vec<String>,
}

impl Rig {
    fn frame(&mut self) {
        let mut ctx = Ctx {
            gpu: &self.gpu,
            renderer: &mut self.renderer,
            window: None,
            size: self.size,
            scale: self.scale,
        };
        self.game.update(&mut ctx);
    }

    /// Runs frames for `seconds` of wall time (at least three, so what is placed from
    /// last frame's boxes settles).
    fn settle(&mut self, seconds: f32) {
        let until = Instant::now() + Duration::from_secs_f32(seconds);
        let mut n = 0;
        while n < 3 || Instant::now() < until {
            self.frame();
            std::thread::sleep(Duration::from_millis(16));
            n += 1;
        }
    }

    fn until(&mut self, what: &str, limit: Duration, done: impl Fn(&Game) -> bool) -> Result<()> {
        let start = Instant::now();
        while !done(&self.game) {
            anyhow::ensure!(start.elapsed() < limit, "timed out waiting for {what}");
            self.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    /// Renders the last prepared frame and writes it.
    fn write(&mut self, name: &str) -> Result<()> {
        let (view, ui_view) = (self.target.view.clone(), self.target.ui_view.clone());
        let mut ctx = Ctx {
            gpu: &self.gpu,
            renderer: &mut self.renderer,
            window: None,
            size: self.size,
            scale: self.scale,
        };
        self.game.render(&mut ctx, &view, &ui_view);
        self.save(name)
    }

    /// Writes what the target holds.
    fn save(&mut self, name: &str) -> Result<()> {
        let rgba = self.target.read(&self.gpu)?;
        let path = self.out.join(format!("{name}.png"));
        image::save_buffer(
            &path,
            &rgba,
            self.size[0],
            self.size[1],
            image::ColorType::Rgba8,
        )
        .with_context(|| path.display().to_string())?;
        println!("wrote {}", path.display());
        self.written.push(name.to_string());
        Ok(())
    }

    fn shoot(&mut self, name: &str) -> Result<()> {
        self.frame();
        self.write(name)
    }

    /// Shoots after a UI tweak the game's own frame would undo (sample overlays).
    fn shoot_as_is(&mut self, name: &str) -> Result<()> {
        self.game.prepare_ui(&self.gpu, &mut self.renderer, 1.0);
        self.write(name)
    }

    fn click(&mut self, name: &str) {
        let action = canopy_ui::Action {
            node: None,
            path: None,
            name: Some(name.into()),
            index: None,
            kind: canopy_ui::ActionKind::Click {
                button: canopy_ui::PointerButton::Primary,
                modifiers: canopy_ui::Modifiers::default(),
                count: 1,
            },
        };
        let mut ctx = Ctx {
            gpu: &self.gpu,
            renderer: &mut self.renderer,
            window: None,
            size: self.size,
            scale: self.scale,
        };
        self.game.action(&action, &mut ctx);
    }
}

fn sample_mission(portrait: canopy_content::Id) -> canopy_sim::view::MissionView {
    use canopy_sim::mission::state::{Dialogue, ObjectiveState};
    let mut objectives = std::collections::BTreeMap::new();
    objectives.insert("gather".to_string(), ObjectiveState::Completed);
    objectives.insert("scouts".to_string(), ObjectiveState::Active);
    objectives.insert("hollow".to_string(), ObjectiveState::Active);
    objectives.insert("bridge".to_string(), ObjectiveState::Failed);
    canopy_sim::view::MissionView {
        objective: "Find the lost scouts beyond the river".into(),
        objectives,
        dialogue: Some(Dialogue {
            id: 1,
            actor: None,
            speaker: "Marshal".into(),
            portrait,
            text: "The scouts went north at dawn and never came back. Gather what amber you can, then follow the river.".into(),
            duration_ticks: 200,
            cinematic: false,
            remaining: 0,
            until: 400,
        }),
        scene: None,
        paused: false,
        started: true,
        cinema: Default::default(),
        animations: Vec::new(),
        error: None,
    }
}

fn objective(
    id: &str,
    title: &str,
    description: &str,
    optional: bool,
) -> canopy_content::map::Objective {
    canopy_content::map::Objective {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        optional,
    }
}

/// Renders every screen to PNGs in `out` (`only`: one group, `releases` or `multiplayer`).
pub fn run(
    paths: &Paths,
    out: &Path,
    size: [u32; 2],
    scale: f32,
    only: Option<&str>,
) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
    let user = std::env::temp_dir().join(format!("canopy-capture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&user);
    std::fs::create_dir_all(&user)?;
    let paths = Paths {
        data: paths.data.clone(),
        user: user.clone(),
        cache: user.join("cache"),
        replays: user.join("replays"),
    };
    let look = canopy_scene::Look::load(&paths.data, canopy_content::map::BIOMES[0])?;
    let environment = canopy_render::Environment::new(&look.lighting, &look.biome, 12.0, None);
    let renderer = Renderer::new(&gpu, CAPTURE_FORMAT, size, environment);
    let settings = Settings {
        player_name: "Jiri".into(),
        ..Settings::default()
    };
    let mut game = Game::new(paths.clone(), settings, &gpu, None)?;
    game.offline_updates();
    let target = Target::new(&gpu, size);
    let mut rig = Rig {
        gpu,
        renderer,
        target,
        game,
        size,
        scale,
        out: out.to_path_buf(),
        written: Vec::new(),
    };
    rig.frame();
    rig.shoot("boot")?;
    rig.until("boot", Duration::from_secs(60), Game::booted)?;
    rig.settle(1.2);
    if let Some(group) = only {
        match group {
            "releases" => releases(&mut rig)?,
            "multiplayer" => multiplayer(&mut rig)?,
            "editor" => editor(&mut rig, &paths)?,
            "blocks" => blocks(&mut rig, &paths)?,
            "lab" => lab(&mut rig, &paths)?,
            "neutrals" => neutrals(&mut rig, &paths)?,
            "cameras" => cameras(&mut rig, &paths)?,
            "editor-campaigns" => editor_campaigns(&mut rig, &paths)?,
            "editor-ui" => ui::editor_ui(&mut rig, &paths)?,
            "settings" => settings_tabs(&mut rig)?,
            "campaign" => campaigns(&mut rig, &paths)?,
            "replays" => {
                start_skirmish(&mut rig)?;
                rig.until("the match", Duration::from_secs(180), |g| g.flow.in_match())?;
                // A minute and a half of match time to replay.
                if let Some(p) = &rig.game.playing {
                    p.game.sim().control(crate::sim::Control::Speed(8.0));
                }
                rig.settle(12.0);
                // The result offers the replay.
                let outcome = canopy_sim::state::Outcome {
                    winner: Some(canopy_content::placement::Owner::Player(0)),
                    defeated: vec![canopy_content::placement::Owner::Player(1)],
                };
                if let Some(playing) = &mut rig.game.playing {
                    playing.outcome = Some(outcome.clone());
                }
                rig.game.decided(Some(outcome), false);
                rig.settle(0.2);
                rig.click("result.save-replay");
                rig.settle(0.2);
                rig.shoot("result-replay-saved")?;
                rig.game.apply(Intent::LeaveMatch);
                rig.settle(0.3);
                replays(&mut rig)?;
            }
            "feedback" => {
                feedback_menu(&mut rig)?;
                start_skirmish(&mut rig)?;
                rig.until("the match", Duration::from_secs(180), |g| g.flow.in_match())?;
                rig.settle(3.0);
                rig.game.apply(Intent::Show(Overlay::GameMenu));
                rig.settle(0.2);
                feedback_match(&mut rig)?;
            }
            other => anyhow::bail!(
                "unknown capture group {other} (releases, multiplayer, editor, blocks, lab, neutrals, cameras, settings, feedback, replays, campaign, editor-campaigns)"
            ),
        }
        let _ = std::fs::remove_dir_all(&user);
        println!("{} captures", rig.written.len());
        return Ok(());
    }
    rig.shoot("main-menu")?;
    releases(&mut rig)?;
    settings_tabs(&mut rig)?;
    feedback_menu(&mut rig)?;

    campaigns(&mut rig, &paths)?;

    skirmish(&mut rig)?;
    multiplayer(&mut rig)?;
    a_match(&mut rig)?;

    // Loading a save from the main menu.
    rig.game.open_saves();
    rig.settle(0.2);
    rig.shoot("saves-main-menu")?;
    editor(&mut rig, &paths)?;
    let _ = std::fs::remove_dir_all(&user);
    println!("{} captures", rig.written.len());
    Ok(())
}

/// The campaign screens (docs/campaign.md) on the first shipped campaign: choosing one,
/// its screen fresh, a chapter card, a story page revealing and revealed, a mission's
/// loading screen while loading and once loaded, the mission, a won mission moving on to
/// a cinematic's card, the screen part way through, a lost mission's result with Retry,
/// the screen of a finished campaign, and Reset campaign asking and done.
fn campaigns(rig: &mut Rig, paths: &Paths) -> Result<()> {
    rig.game.apply(Intent::Open(Screen::Campaigns));
    rig.settle(0.4);
    rig.shoot("campaign")?;
    let Some(campaign) = rig
        .game
        .data
        .as_ref()
        .and_then(|d| d.campaigns.first().cloned())
    else {
        rig.game.apply(Intent::Back);
        return Ok(());
    };
    let id = campaign.id().to_string();
    let open = |rig: &mut Rig| {
        rig.game.apply(Intent::Open(Screen::Campaign {
            campaign: id.clone(),
        }));
        rig.settle(0.5);
    };
    open(rig);
    rig.shoot("campaign-new")?;
    // Straight through from the start: the cards, then the first mission.
    rig.game.continue_campaign(&id);
    rig.settle(0.4);
    let steps = &campaign.campaign.steps;
    for _ in 0..steps.len() {
        match rig.game.flow.screen().clone() {
            Screen::Step { step, .. } => {
                let kind = steps[step].kind();
                rig.settle(0.6);
                rig.shoot(&format!("card-{kind}-{step}-reveal"))?;
                rig.settle(2.6);
                rig.shoot(&format!("card-{kind}-{step}"))?;
                // Through every page of a story.
                while matches!(rig.game.flow.screen(), Screen::Step { step: s, .. } if *s == step) {
                    rig.game.card_next();
                    rig.settle(0.4);
                }
            }
            _ => break,
        }
    }
    if matches!(rig.game.flow.screen(), Screen::Loading) {
        rig.frame();
        rig.shoot("loading-mission")?;
        rig.until("the mission", Duration::from_secs(180), |g| {
            g.loading.as_ref().is_some_and(|l| l.prepared.is_some())
        })?;
        rig.settle(0.5);
        rig.shoot("loading-mission-ready")?;
        rig.game.begin_loaded();
        rig.until("the match", Duration::from_secs(60), |g| g.flow.in_match())?;
        rig.settle(3.0);
        rig.shoot("mission")?;
        // Won: progress is saved and the campaign moves on by itself.
        let outcome = canopy_sim::state::Outcome {
            winner: Some(canopy_content::placement::Owner::Player(0)),
            defeated: Vec::new(),
        };
        if let Some(playing) = &mut rig.game.playing {
            playing.outcome = Some(outcome.clone());
        }
        rig.game.decided(Some(outcome), false);
        rig.until("the next step", Duration::from_secs(20), |g| {
            !g.flow.in_match() && !matches!(g.flow.screen(), Screen::Loading)
        })?;
        rig.settle(2.2);
        rig.shoot("after-victory")?;
    }
    // The campaign screen part way through.
    rig.game.apply(Intent::Proceed(Screen::Campaign {
        campaign: id.clone(),
    }));
    rig.settle(0.6);
    rig.shoot("campaign-progress")?;
    // A lost mission: its result offers Retry.
    let first_mission = steps.iter().position(|s| s.mission().is_some());
    if let Some(step) = first_mission {
        rig.game.run_step(&id, step, false);
        rig.until("the mission", Duration::from_secs(180), |g| {
            g.loading.as_ref().is_some_and(|l| l.prepared.is_some())
        })?;
        rig.game.begin_loaded();
        rig.until("the match", Duration::from_secs(60), |g| g.flow.in_match())?;
        rig.settle(1.0);
        let outcome = canopy_sim::state::Outcome {
            winner: None,
            defeated: vec![canopy_content::placement::Owner::Player(0)],
        };
        if let Some(playing) = &mut rig.game.playing {
            playing.outcome = Some(outcome.clone());
        }
        rig.game.decided(Some(outcome), false);
        rig.settle(0.4);
        rig.shoot("result-mission-failed")?;
        rig.game.apply(Intent::LeaveMatch);
        rig.settle(0.4);
    }
    // A finished campaign: every step listed, nothing next.
    let mut done = crate::campaign::Progress::load(paths, &id);
    for step in 0..steps.len() {
        done.finish(&campaign.campaign, step);
    }
    done.save(paths, &id)?;
    rig.game.apply(Intent::Back);
    rig.settle(0.3);
    open(rig);
    rig.shoot("campaign-complete")?;
    // Reset campaign asks first, then leaves the campaign as never played.
    rig.game.ask_reset(&id);
    rig.settle(0.3);
    rig.shoot("campaign-reset-ask")?;
    if let Some(dialog) = rig.game.dialog.take() {
        rig.game.apply(Intent::Back);
        rig.game.answer(dialog.ask, false);
    }
    rig.settle(0.3);
    rig.shoot("campaign-reset")?;
    if crate::campaign::Progress::load(paths, &id).started() {
        anyhow::bail!("reset campaign left progress behind");
    }
    rig.game.apply(Intent::Back);
    rig.game.apply(Intent::Back);
    rig.settle(0.3);
    Ok(())
}

/// The map editor on the neutral bench (D87): Place's catalog, and the panel of a shop, a
/// crate, turned rubble and a spring.
fn neutrals(rig: &mut Rig, paths: &Paths) -> Result<()> {
    use canopy_editor::{Editor, Kind};
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let mut editor = Editor::headless(&rig.gpu, format, crate::app::editor_setup(paths));
    let settle = |editor: &mut Editor, rig: &mut Rig, n: usize| {
        for _ in 0..n {
            editor.run_headless(rig.size, rig.scale, &rig.gpu);
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let shoot = |editor: &mut Editor, rig: &mut Rig, name: &str| -> Result<()> {
        let mut encoder = rig
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        editor.paint(&mut encoder, &rig.target.ui_view);
        rig.gpu.queue.submit([encoder.finish()]);
        rig.save(name)
    };
    editor.select(Kind::Maps);
    editor.open_document("maps/showcase/neutral-bench.utcm/map.json");
    settle(&mut editor, rig, 60);
    editor.set_block_view("generated");
    editor.set_block_tool("place");
    settle(&mut editor, rig, 10);
    let escape = |pressed| egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    for (category, group, name) in [
        ("Buildings", "Neutral", "editor-neutrals-place-buildings"),
        ("Special", "Breakables", "editor-neutrals-place-breakables"),
        ("Items", "Tier 1", "editor-neutrals-place-items"),
    ] {
        editor.set_place_group(category, group);
        editor.open_block_picker();
        settle(&mut editor, rig, 12);
        shoot(&mut editor, rig, name)?;
        editor.run_headless_with(
            rig.size,
            rig.scale,
            &rig.gpu,
            vec![escape(true), escape(false)],
        );
    }
    for id in [
        "bench.merchant",
        "bench.amber-cache",
        "bench.rubble-turned",
        "bench.sap-spring",
        "bench.item.0",
    ] {
        editor.select_placement(id);
        settle(&mut editor, rig, 8);
        shoot(&mut editor, rig, &format!("editor-neutrals-{}", &id[6..]))?;
    }
    Ok(())
}

/// The editor offscreen: an empty editor, documents open, the assistant open, and a
/// library of several collections.
/// The block editor (D80) on Frozen Dream: a raised area, a ramp and a diagonal cut; then
/// Frozen Dream in every view.
/// A mission's saved cameras in the block editor: the cinematic bench's cameras with
/// their frustums, one selected with its panel, and the view through it.
fn cameras(rig: &mut Rig, paths: &Paths) -> Result<()> {
    use canopy_editor::{Editor, Kind};
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let mut editor = Editor::headless(&rig.gpu, format, crate::app::editor_setup(paths));
    let settle = |editor: &mut Editor, rig: &mut Rig, n: usize| {
        for _ in 0..n {
            editor.run_headless(rig.size, rig.scale, &rig.gpu);
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let shoot = |editor: &mut Editor, rig: &mut Rig, name: &str| -> Result<()> {
        let mut encoder = rig
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        editor.paint(&mut encoder, &rig.target.ui_view);
        rig.gpu.queue.submit([encoder.finish()]);
        rig.save(name)
    };
    editor.select(Kind::Maps);
    editor.open_document("maps/showcase/cinematic-bench.utcm/map.json");
    settle(&mut editor, rig, 60);
    editor.set_block_view("generated");
    editor.show_camera("over-tiv", false, [86.0, 112.0], 120.0);
    settle(&mut editor, rig, 40);
    shoot(&mut editor, rig, "editor-cameras")?;
    editor.show_camera("over-tiv", true, [86.0, 112.0], 120.0);
    settle(&mut editor, rig, 40);
    shoot(&mut editor, rig, "editor-cameras-look-through")?;
    Ok(())
}

/// The combat lab (docs/editor.md): prepared scenarios at their start, mid-run with the
/// debug layers, and the unit held up longest inspected.
fn lab(rig: &mut Rig, paths: &Paths) -> Result<()> {
    use canopy_editor::{Editor, Kind};
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let mut editor = Editor::headless(&rig.gpu, format, crate::app::editor_setup(paths));
    let settle = |editor: &mut Editor, rig: &mut Rig, n: usize| {
        for _ in 0..n {
            editor.run_headless(rig.size, rig.scale, &rig.gpu);
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let shoot = |editor: &mut Editor, rig: &mut Rig, name: &str| -> Result<()> {
        let mut encoder = rig
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        editor.paint(&mut encoder, &rig.target.ui_view);
        rig.gpu.queue.submit([encoder.finish()]);
        rig.save(name)
    };
    editor.select(Kind::Lab);
    settle(&mut editor, rig, 20);
    /// A scenario, the ticks run, the camera's cell, distance and pitch, the layers on.
    type LabShot<'a> = (&'a str, u32, f32, f32, f32, f32, &'a [&'a str]);
    let shots: [LabShot; 9] = [
        ("look-check", 1, 62.0, 63.0, 106.0, 0.977, &[]),
        (
            "look-check",
            2,
            62.0,
            63.0,
            106.0,
            std::f32::consts::FRAC_PI_2,
            &["bodies"],
        ),
        ("body-sizes", 1, 88.0, 70.0, 150.0, 1.5, &["bodies"]),
        (
            "ring-still",
            160,
            56.0,
            64.0,
            40.0,
            1.45,
            &["routes", "states", "tried"],
        ),
        ("choke-100", 0, 96.0, 64.0, 150.0, 1.15, &[]),
        (
            "choke-100",
            360,
            96.0,
            64.0,
            70.0,
            1.2,
            &["routes", "vectors", "states"],
        ),
        (
            "crossing",
            300,
            96.0,
            96.0,
            90.0,
            1.2,
            &["routes", "vectors", "states", "tried"],
        ),
        (
            "swarm-base",
            900,
            150.0,
            110.0,
            110.0,
            1.1,
            &["routes", "states", "sectors"],
        ),
        (
            "u-trap",
            120,
            64.0,
            64.0,
            80.0,
            1.25,
            &["routes", "vectors", "detours", "states"],
        ),
    ];
    for (id, ticks, x, z, distance, pitch, layers) in shots {
        let document = format!("@labs/{id}.lab.json");
        editor.open_document(&document);
        settle(&mut editor, rig, 40);
        for layer in [
            "routes", "vectors", "tried", "states", "detours", "sectors", "orders", "bodies",
        ] {
            editor.lab_layer(&document, layer, ticks == 0 || layers.contains(&layer));
        }
        if ticks > 0 {
            editor.lab_layer(&document, "orders", true);
        }
        editor.lab_run(&document, ticks);
        editor.lab_look_at(&document, x, z, distance, pitch);
        if ticks > 0 {
            editor.lab_inspect_held_up(&document);
        }
        settle(&mut editor, rig, 12);
        shoot(&mut editor, rig, &format!("lab-{id}-{ticks}"))?;
    }
    // A Raider walked by hand through a hundred idle allies.
    let document = "@labs/through-a-hundred.lab.json";
    editor.open_document(document);
    settle(&mut editor, rig, 40);
    for layer in ["routes", "vectors", "tried", "states", "detours"] {
        editor.lab_layer(document, layer, true);
    }
    editor.lab_layer(document, "sectors", false);
    editor.lab_hand_move(document, "walker", 112.0, 64.0);
    editor.lab_look_at(document, 64.0, 64.0, 75.0, 1.25);
    let mut ran = 0;
    for ticks in [60, 160] {
        editor.lab_run(document, ticks - ran);
        ran = ticks;
        settle(&mut editor, rig, 10);
        shoot(&mut editor, rig, &format!("lab-through-a-hundred-{ticks}"))?;
    }
    Ok(())
}

fn blocks(rig: &mut Rig, paths: &Paths) -> Result<()> {
    use canopy_editor::{Editor, Kind};
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let mut editor = Editor::headless(&rig.gpu, format, crate::app::editor_setup(paths));
    let ui_scale = rig.scale;
    let frame = |editor: &mut Editor, rig: &mut Rig, events: Vec<egui::Event>| {
        editor.run_headless_with(rig.size, rig.scale, &rig.gpu, events);
        std::thread::sleep(Duration::from_millis(20));
    };
    let settle = |editor: &mut Editor, rig: &mut Rig, n: usize| {
        for _ in 0..n {
            editor.run_headless(rig.size, rig.scale, &rig.gpu);
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let shoot = |editor: &mut Editor, rig: &mut Rig, name: &str| -> Result<()> {
        let mut encoder = rig
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        editor.paint(&mut encoder, &rig.target.ui_view);
        rig.gpu.queue.submit([encoder.finish()]);
        rig.save(name)
    };
    let point = move |x: f32, y: f32| egui::pos2(x / ui_scale, y / ui_scale);
    let press = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let drag = |editor: &mut Editor, rig: &mut Rig, from: (f32, f32), to: (f32, f32)| {
        let a = point(from.0, from.1);
        frame(editor, rig, vec![egui::Event::PointerMoved(a)]);
        frame(editor, rig, vec![press(a, true)]);
        for i in 1..=12 {
            let t = i as f32 / 12.0;
            let p = point(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            frame(editor, rig, vec![egui::Event::PointerMoved(p)]);
        }
        frame(editor, rig, vec![press(point(to.0, to.1), false)]);
        frame(editor, rig, vec![]);
    };
    let click = |editor: &mut Editor, rig: &mut Rig, at: (f32, f32)| {
        let p = point(at.0, at.1);
        frame(editor, rig, vec![egui::Event::PointerMoved(p)]);
        frame(editor, rig, vec![press(p, true)]);
        frame(editor, rig, vec![press(p, false)]);
        frame(editor, rig, vec![]);
    };
    editor.select(Kind::Maps);
    editor.open_document("maps/skirmish/frozen-dream.utcm/map.json");
    settle(&mut editor, rig, 60);
    // Frame time in the block view (the editor draws every frame it is shown).
    let started = Instant::now();
    settle(&mut editor, rig, 30);
    let frame_ms = (started.elapsed().as_secs_f32() * 1000.0 - 30.0 * 25.0) / 30.0;
    tracing::info!(frame_ms, "block view frame");
    shoot(&mut editor, rig, "editor-blocks")?;
    editor.show_problems(true);
    settle(&mut editor, rig, 4);
    shoot(&mut editor, rig, "editor-blocks-problems")?;
    editor.show_problems(false);
    editor.set_block_tool("raise");
    drag(&mut editor, rig, (900.0, 560.0), (1040.0, 640.0));
    editor.set_block_tool("ramp");
    click(&mut editor, rig, (1050.0, 600.0));
    editor.set_block_tool("cut");
    click(&mut editor, rig, (895.0, 555.0));
    settle(&mut editor, rig, 30);
    shoot(&mut editor, rig, "editor-blocks-edited")?;
    editor.undo_all();

    // The blocks, a ramp being dragged, the cell brush, and the world generated from them.
    editor.open_document("maps/skirmish/frozen-dream.utcm/map.json");
    settle(&mut editor, rig, 60);
    editor.set_block_view("blocks");
    settle(&mut editor, rig, 30);
    shoot(&mut editor, rig, "editor-blocks-frost")?;
    editor.set_block_tool("ramp");
    let (a, b) = (point(880.0, 420.0), point(1040.0, 560.0));
    frame(&mut editor, rig, vec![egui::Event::PointerMoved(a)]);
    frame(&mut editor, rig, vec![press(a, true)]);
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        frame(
            &mut editor,
            rig,
            vec![egui::Event::PointerMoved(a + (b - a) * t)],
        );
    }
    shoot(&mut editor, rig, "editor-blocks-ramp-drag")?;
    frame(&mut editor, rig, vec![press(b, false)]);
    settle(&mut editor, rig, 10);
    editor.set_block_tool("cells");
    frame(
        &mut editor,
        rig,
        vec![egui::Event::PointerMoved(point(1300.0, 700.0))],
    );
    settle(&mut editor, rig, 4);
    shoot(&mut editor, rig, "editor-blocks-cells")?;
    // The strip's pickers open: kinds, faces, what Place puts down.
    for (tool, name) in [
        ("paint", "editor-blocks-picker-kinds"),
        ("face", "editor-blocks-picker-faces"),
        ("place", "editor-blocks-picker-place"),
    ] {
        editor.set_block_tool(tool);
        editor.open_block_picker();
        settle(&mut editor, rig, 4);
        shoot(&mut editor, rig, name)?;
        let escape = |pressed| egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut editor, rig, vec![escape(true), escape(false)]);
    }
    // Place shows where it lands: a start's hall footprint on the building lattice.
    editor.set_block_tool("place");
    frame(
        &mut editor,
        rig,
        vec![egui::Event::PointerMoved(point(1180.0, 640.0))],
    );
    settle(&mut editor, rig, 4);
    shoot(&mut editor, rig, "editor-blocks-place-hover")?;
    // A rectangle drawn round several things selects them all.
    editor.set_block_tool("select");
    let (a, b) = (point(800.0, 560.0), point(1060.0, 800.0));
    frame(&mut editor, rig, vec![egui::Event::PointerMoved(a)]);
    frame(&mut editor, rig, vec![press(a, true)]);
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        frame(
            &mut editor,
            rig,
            vec![egui::Event::PointerMoved(a + (b - a) * t)],
        );
    }
    shoot(&mut editor, rig, "editor-blocks-select-drag")?;
    frame(&mut editor, rig, vec![press(b, false)]);
    settle(&mut editor, rig, 6);
    shoot(&mut editor, rig, "editor-blocks-select-many")?;
    // Dragging the selection: it follows the pointer, block by block, red where it
    // may not stand.
    let (a, b) = (point(1050.0, 590.0), point(1250.0, 700.0));
    frame(&mut editor, rig, vec![egui::Event::PointerMoved(a)]);
    frame(&mut editor, rig, vec![press(a, true)]);
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        frame(
            &mut editor,
            rig,
            vec![egui::Event::PointerMoved(a + (b - a) * t)],
        );
    }
    settle(&mut editor, rig, 4);
    shoot(&mut editor, rig, "editor-blocks-move-drag")?;
    frame(
        &mut editor,
        rig,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    frame(&mut editor, rig, vec![press(a, false)]);
    settle(&mut editor, rig, 6);
    editor.undo_all();
    editor.set_block_tool("cells");
    editor.set_block_view("generated");
    let started = Instant::now();
    settle(&mut editor, rig, 1);
    let build_ms = started.elapsed().as_secs_f32() * 1000.0;
    settle(&mut editor, rig, 40);
    tracing::info!(build_ms, "generated view build");
    shoot(&mut editor, rig, "editor-blocks-generated")?;
    // An edit in the generated view, and how long the world takes to follow it.
    editor.set_block_tool("raise");
    let started = Instant::now();
    click(&mut editor, rig, (700.0, 800.0));
    settle(&mut editor, rig, 1);
    tracing::info!(
        edit_ms = started.elapsed().as_secs_f32() * 1000.0,
        "generated view edit"
    );
    settle(&mut editor, rig, 20);
    shoot(&mut editor, rig, "editor-blocks-generated-edited")?;
    editor.undo_all();

    // Frozen Dream with point symmetry: a brush stroke and its mirrored twin.
    editor.open_document("maps/skirmish/frozen-dream.utcm/map.json");
    settle(&mut editor, rig, 60);
    editor.set_block_view("blocks");
    editor.set_block_tool("raise");
    editor.set_block_symmetry("point", true);
    settle(&mut editor, rig, 30);
    let (a, b) = (point(900.0, 500.0), point(1100.0, 640.0));
    frame(&mut editor, rig, vec![egui::Event::PointerMoved(a)]);
    frame(&mut editor, rig, vec![press(a, true)]);
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        frame(
            &mut editor,
            rig,
            vec![egui::Event::PointerMoved(a + (b - a) * t)],
        );
    }
    shoot(&mut editor, rig, "editor-blocks-symmetry")?;
    frame(&mut editor, rig, vec![press(b, false)]);
    settle(&mut editor, rig, 20);
    shoot(&mut editor, rig, "editor-blocks-symmetry-done")?;
    editor.set_block_symmetry("off", false);
    editor.undo_all();
    // A lasso: a loop drawn with Level, the blocks inside it taking the level.
    editor.set_block_tool("level");
    editor.set_block_lasso(true);
    settle(&mut editor, rig, 4);
    let centre = point(1050.0, 640.0);
    frame(
        &mut editor,
        rig,
        vec![egui::Event::PointerMoved(centre + egui::vec2(120.0, 0.0))],
    );
    frame(
        &mut editor,
        rig,
        vec![press(centre + egui::vec2(120.0, 0.0), true)],
    );
    for i in 1..=24 {
        let a = i as f32 / 24.0 * std::f32::consts::TAU;
        let p = centre + egui::vec2(a.cos() * 120.0, a.sin() * 80.0);
        frame(&mut editor, rig, vec![egui::Event::PointerMoved(p)]);
    }
    shoot(&mut editor, rig, "editor-blocks-lasso")?;
    frame(
        &mut editor,
        rig,
        vec![press(centre + egui::vec2(120.0, 0.0), false)],
    );
    settle(&mut editor, rig, 10);
    editor.set_block_lasso(false);
    editor.undo_all();
    editor.set_block_view("generated");
    settle(&mut editor, rig, 40);
    shoot(&mut editor, rig, "editor-blocks-dream-generated")?;
    // Where ground units walk, cell by cell.
    editor.set_block_view("pathing");
    settle(&mut editor, rig, 40);
    shoot(&mut editor, rig, "editor-blocks-dream-pathing")?;
    // The blocks and the generated world side by side, then with the panels hidden.
    editor.set_block_view("split");
    settle(&mut editor, rig, 60);
    shoot(&mut editor, rig, "editor-blocks-dream-split")?;
    // An edit in the blocks: the world follows two seconds after it.
    editor.set_block_tool("raise");
    let (a, b) = (point(560.0, 560.0), point(700.0, 640.0));
    frame(&mut editor, rig, vec![egui::Event::PointerMoved(a)]);
    frame(&mut editor, rig, vec![press(a, true)]);
    frame(&mut editor, rig, vec![egui::Event::PointerMoved(b)]);
    frame(&mut editor, rig, vec![press(b, false)]);
    settle(&mut editor, rig, 10);
    shoot(&mut editor, rig, "editor-blocks-dream-split-edited")?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(2600) {
        settle(&mut editor, rig, 5);
    }
    settle(&mut editor, rig, 10);
    shoot(&mut editor, rig, "editor-blocks-dream-split-followed")?;
    editor.undo_all();
    editor.set_block_tool("level");
    editor.set_map_panels(false, false);
    settle(&mut editor, rig, 40);
    shoot(&mut editor, rig, "editor-blocks-dream-split-wide")?;
    editor.set_map_panels(false, true);
    editor.set_block_view("blocks");
    // The forest biome's look-dev map: its blocks and the world generated from them.
    if paths
        .data
        .join("base/maps/showcase/underleaf-lookdev.utcm")
        .is_dir()
    {
        editor.open_document("maps/showcase/underleaf-lookdev.utcm/map.json");
        settle(&mut editor, rig, 40);
        shoot(&mut editor, rig, "editor-blocks-underleaf")?;
        editor.set_block_view("generated");
        settle(&mut editor, rig, 60);
        shoot(&mut editor, rig, "editor-blocks-underleaf-generated")?;
        editor.set_block_view("blocks");
    }
    Ok(())
}

fn editor(rig: &mut Rig, paths: &Paths) -> Result<()> {
    use canopy_editor::{Editor, Kind};
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let mut editor = Editor::headless(&rig.gpu, format, crate::app::editor_setup(paths));
    let shoot = |editor: &mut Editor, rig: &mut Rig, name: &str| -> Result<()> {
        // A few frames (egui sizes panels and text from the previous frame), and until the
        // check of the latest edits is in.
        let start = Instant::now();
        let mut frames = 0;
        while frames < 4 || (editor.checking() && start.elapsed() < Duration::from_secs(20)) {
            editor.run_headless(rig.size, rig.scale, &rig.gpu);
            std::thread::sleep(Duration::from_millis(10));
            frames += 1;
        }
        let mut encoder = rig
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        editor.paint(&mut encoder, &rig.target.ui_view);
        rig.gpu.queue.submit([encoder.finish()]);
        rig.save(name)
    };
    editor.select(Kind::Abilities);
    shoot(&mut editor, rig, "editor-empty")?;
    editor.open_first(3);
    shoot(&mut editor, rig, "editor-abilities")?;
    // An unsaved edit: marks in the tab, the library and the header (nothing is saved).
    if let Some(document) = editor.active_document() {
        // Its name is in the text library (D96), under its id.
        let id = std::fs::read_to_string(paths.data.join("base").join(&document))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["id"].as_str().map(str::to_string))
            .context("the ability's id")?;
        editor
            .run_step(
                "Rename for the capture",
                vec![canopy_editor::Command::Set {
                    document: "text/en/abilities.json".into(),
                    pointer: format!("/{id}.name"),
                    value: "Renamed in the editor".into(),
                    at: None,
                }],
            )
            .map_err(anyhow::Error::msg)?;
        shoot(&mut editor, rig, "editor-unsaved")?;
        // A broken edit: the check lists it.
        let document = editor.active_document().unwrap_or_default();
        editor
            .run_step(
                "Break for the capture",
                vec![canopy_editor::Command::Set {
                    document,
                    pointer: "/activation".into(),
                    value: "sometimes".into(),
                    at: None,
                }],
            )
            .map_err(anyhow::Error::msg)?;
        editor.show_problems(true);
        shoot(&mut editor, rig, "editor-problems")?;
        editor.show_problems(false);
    }
    editor.set_assistant(true);
    shoot(&mut editor, rig, "editor-assistant")?;
    // A finished conversation: what the user asked, Claude's notes and text, its tool
    // calls (open, with their answers), as a thread holds them.
    let thread = serde_json::json!([
        {"role": "user", "content": [
            {"type": "text", "text": "<editor-context>\nEditor: Units and heroes\n</editor-context>"},
            {"type": "text", "text": "Make the Warrior a little tougher, about 10%."}
        ]},
        {"role": "assistant", "content": [
            {"type": "thinking", "thinking": "Reading the Warrior and the units skill first.", "signature": "s"},
            {"type": "tool_use", "id": "t1", "name": "read_skill", "input": {"name": "canopy-definitions"}},
            {"type": "tool_use", "id": "t2", "name": "read_document", "input": {"document": "units/ants/warrior.json"}}
        ]},
        {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "{ \"skill\": \"canopy-definitions\" }"},
            {"type": "tool_result", "tool_use_id": "t2", "content": "{ \"body\": { \"maxHp\": 185, \"armor\": 1 } }"}
        ]},
        {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t3", "name": "edit", "input": {"label": "Warrior health 185 → 204", "commands": [
                {"op": "set", "document": "units/ants/warrior.json", "pointer": "/body/maxHp", "value": 204}
            ]}},
            {"type": "tool_use", "id": "t4", "name": "problems", "input": {}}
        ]},
        {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t3", "content": "{ \"done\": \"Warrior health 185 → 204\" }"},
            {"type": "tool_result", "tool_use_id": "t4", "content": "{ \"problems\": [] }"}
        ]},
        {"role": "assistant", "content": [
            {"type": "text", "text": "Done: the Warrior's health goes from **185** to **204** (+10%), in one undo step, *Warrior health 185 → 204*.\n\n- Armour stays at `1`; with it, a Warrior now survives one more hit from a Raider.\n- The check finds no problems. Nothing is saved yet."}
        ]}
    ]);
    editor.assistant_thread(
        "Make the Warrior a little tougher",
        thread.as_array().cloned().unwrap_or_default(),
        false,
    );
    shoot(&mut editor, rig, "editor-assistant-thread")?;
    editor.assistant_thread("Settings", Vec::new(), true);
    shoot(&mut editor, rig, "editor-assistant-settings")?;
    editor.assistant_provider("openai");
    shoot(&mut editor, rig, "editor-assistant-settings-openai")?;
    editor.assistant_provider("anthropic");
    editor.set_assistant(false);
    editor.select(Kind::Races);
    editor.open_first(1);
    shoot(&mut editor, rig, "editor-races")?;
    editor.select(Kind::Units);
    editor.open_first(1);
    shoot(&mut editor, rig, "editor-units")?;
    editor.show_sheet("form");
    shoot(&mut editor, rig, "editor-units-form")?;
    editor.show_sheet("json");
    shoot(&mut editor, rig, "editor-units-json")?;
    editor.show_sheet("");
    // The ability stage: a unit's projectile spell cast at an enemy in the editor's own
    // viewport, caught in flight; then a hero's area spell on a few enemies.
    {
        use canopy_content::abilities::{Delivery, TargetingKind};
        let content = editor.checked_content().context("a check passed")?;
        let bound: Vec<_> = content
            .abilities
            .abilities
            .iter()
            .filter(|a| !canopy_sim::encounter::binders(&content, a.id.as_str()).is_empty())
            .filter(|a| a.targeting.condition.is_none() && a.targeting.filter.is_none())
            .collect();
        let projectile = bound
            .iter()
            .find(|a| {
                a.targeting.kind == TargetingKind::Unit
                    && matches!(a.delivery, Some(Delivery::Projectile { .. }))
                    && !a.presentation.effects.is_empty()
            })
            .or(bound.first())
            .context("an ability a unit binds")?;
        let area = bound
            .iter()
            .filter(|a| a.targeting.kind == TargetingKind::Point)
            .max_by_key(|a| a.presentation.effects.len());
        let takes = [
            Some((projectile, 8, "editor-ability-stage")),
            area.map(|a| (a, 12, "editor-ability-stage-area")),
        ];
        editor.select(Kind::Abilities);
        for (ability, after, name) in takes.into_iter().flatten() {
            editor.open_stage(ability.id.as_str());
            for _ in 0..6 {
                editor.run_headless(rig.size, rig.scale, &rig.gpu);
            }
            editor.stage_cast();
            editor.run_headless(rig.size, rig.scale, &rig.gpu);
            editor.stage_seek(ability.cast.prepare_ticks + after);
            shoot(&mut editor, rig, name)?;
        }
    }
    // An asset: its overview, its clip playing in the preview.
    editor.select(Kind::Assets);
    editor.open("asset.models.units.neutral-violet-sporeling");
    for _ in 0..4 {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
    }
    editor.play_clip(3);
    for _ in 0..8 {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
        std::thread::sleep(Duration::from_millis(25));
    }
    shoot(&mut editor, rig, "editor-asset")?;
    editor.select(Kind::Maps);
    editor.open_first(1);
    // The map view builds its scene once the content check has passed.
    for _ in 0..40 {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
        std::thread::sleep(Duration::from_millis(25));
    }
    shoot(&mut editor, rig, "editor-maps")?;
    // Editing the map through its view, in blocks: a raised stroke, a start.
    let frame = |editor: &mut Editor, rig: &mut Rig, events: Vec<egui::Event>| {
        editor.run_headless_with(rig.size, rig.scale, &rig.gpu, events);
        std::thread::sleep(Duration::from_millis(20));
    };
    let ui_scale = rig.scale;
    let point = move |x: f32, y: f32| egui::pos2(x / ui_scale, y / ui_scale);
    let press = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let drag = |editor: &mut Editor, rig: &mut Rig, from: (f32, f32), to: (f32, f32)| {
        let a = point(from.0, from.1);
        frame(editor, rig, vec![egui::Event::PointerMoved(a)]);
        frame(editor, rig, vec![press(a, true)]);
        for i in 1..=20 {
            let t = i as f32 / 20.0;
            let p = point(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            frame(editor, rig, vec![egui::Event::PointerMoved(p)]);
        }
        let b = point(to.0, to.1);
        frame(editor, rig, vec![press(b, false)]);
        frame(editor, rig, vec![]);
    };
    let click = |editor: &mut Editor, rig: &mut Rig, at: (f32, f32)| {
        let p = point(at.0, at.1);
        frame(editor, rig, vec![egui::Event::PointerMoved(p)]);
        frame(editor, rig, vec![press(p, true)]);
        frame(editor, rig, vec![press(p, false)]);
        frame(editor, rig, vec![]);
    };
    editor.undo_all();
    editor.set_block_tool("raise");
    editor.set_block_symmetry("off", true);
    drag(&mut editor, rig, (760.0, 640.0), (1000.0, 700.0));
    // The start moves where its room (turned to the middle) is clear: the first spot
    // that takes it.
    editor.set_block_tool("place");
    for at in [
        (1150.0, 450.0),
        (1300.0, 450.0),
        (1150.0, 600.0),
        (1300.0, 800.0),
        (1150.0, 850.0),
    ] {
        click(&mut editor, rig, at);
        for _ in 0..20 {
            frame(&mut editor, rig, vec![]);
        }
        if editor.unsaved().iter().any(|d| d.ends_with("map.json")) {
            break;
        }
    }
    let unsaved = editor.unsaved();
    anyhow::ensure!(
        unsaved.iter().any(|d| d.ends_with("terrain/blocks.grid"))
            && unsaved.iter().any(|d| d.ends_with("terrain/heights.grid"))
            && unsaved.iter().any(|d| d.ends_with("map.json")),
        "the blocks, the stroke and the start are unsaved edits: {unsaved:?}"
    );
    shoot(&mut editor, rig, "editor-map-edited")?;
    editor.undo_all();
    for _ in 0..5 {
        frame(&mut editor, rig, vec![]);
    }
    anyhow::ensure!(
        editor.unsaved().is_empty(),
        "undo takes every map edit back"
    );

    // Test in game: the editor's checked data (a renamed race: its name is in the text
    // library, D96) plays a skirmish, and the game's own data is back once the match is
    // left.
    editor.undo_all();
    editor
        .run_step(
            "Rename for the test",
            vec![canopy_editor::Command::Set {
                document: "text/en/races.json".into(),
                pointer: "/race.ants.name".into(),
                value: "Ants (editor test)".into(),
                at: None,
            }],
        )
        .map_err(anyhow::Error::msg)?;
    shoot(&mut editor, rig, "editor-before-test")?;
    let content = editor.checked_content().context("a check passed")?;
    anyhow::ensure!(
        content
            .rules
            .races
            .get("ants")
            .is_some_and(|r| r.name == "Ants (editor test)"),
        "the checked data has the unsaved edit"
    );
    rig.game
        .test_in_game(content.clone(), None, None, None, false)
        .map_err(anyhow::Error::msg)?;
    rig.until("the test match", Duration::from_secs(180), |g| {
        g.flow.in_match()
    })?;
    // The world itself plays the editor's data (not the files on disk).
    let played = rig
        .game
        .playing
        .as_ref()
        .context("a match")?
        .game
        .play
        .knowledge()
        .content
        .clone();
    anyhow::ensure!(
        played
            .rules
            .races
            .get("ants")
            .is_some_and(|r| r.name == "Ants (editor test)"),
        "the match plays the editor's data"
    );
    rig.settle(2.0);
    rig.shoot("editor-test-match")?;
    rig.game.apply(Intent::LeaveMatch);
    rig.until("the main menu", Duration::from_secs(30), |g| {
        matches!(g.flow.screen(), Screen::MainMenu)
    })?;
    anyhow::ensure!(rig.game.test_finished(), "the test ends at the main menu");
    // An ability's encounter: a hero near the player's start, allies and creeps around.
    let hero = content
        .definitions
        .iter()
        .find(|d| d.behaviors.progression.is_some() && d.behaviors.abilities.is_some())
        .map(|d| d.id.to_string())
        .context("a hero with abilities")?;
    rig.game
        .test_in_game(content.clone(), None, None, Some(hero.clone()), false)
        .map_err(anyhow::Error::msg)?;
    rig.until("the encounter", Duration::from_secs(180), |g| {
        g.flow.in_match()
    })?;
    rig.settle(2.0);
    let knowledge = rig
        .game
        .playing
        .as_ref()
        .context("a match")?
        .game
        .play
        .knowledge();
    anyhow::ensure!(
        knowledge
            .view
            .entities
            .iter()
            .any(|e| knowledge.catalog.id(e.definition()).as_str() == hero),
        "the encounter's hero is in the match"
    );
    rig.shoot("editor-encounter")?;
    rig.game.apply(Intent::LeaveMatch);
    rig.until("the main menu", Duration::from_secs(30), |g| {
        matches!(g.flow.screen(), Screen::MainMenu)
    })?;
    anyhow::ensure!(
        rig.game.test_finished(),
        "the encounter ends at the main menu"
    );
    let own = rig.game.data.as_ref().context("data")?;
    anyhow::ensure!(
        own.content
            .rules
            .races
            .get("ants")
            .is_some_and(|r| r.name != "Ants (editor test)"),
        "the game's own data is back"
    );
    editor_campaigns(rig, paths)
}

/// The editor's campaign tools (docs/editor.md "Campaigns") on the package's first
/// campaign, filled in memory through the campaign tools agents use (nothing is saved):
/// its steps with art, a mission selected, a story selected, a mission's Tech view (all of
/// it, a contradiction in red, and only its changes), its Players, and the warnings.
fn editor_campaigns(rig: &mut Rig, paths: &Paths) -> Result<()> {
    use canopy_editor::{Editor, Kind};
    use serde_json::{Value, json};
    let format = CAPTURE_FORMAT.remove_srgb_suffix();
    let mut editor = Editor::headless(&rig.gpu, format, crate::app::editor_setup(paths));
    let shoot = |editor: &mut Editor, rig: &mut Rig, name: &str| -> Result<()> {
        // Frames until the check is in, and time for the art to decode.
        let start = Instant::now();
        let mut frames = 0;
        while frames < 24 || (editor.checking() && start.elapsed() < Duration::from_secs(30)) {
            editor.run_headless(rig.size, rig.scale, &rig.gpu);
            std::thread::sleep(Duration::from_millis(15));
            frames += 1;
        }
        let mut encoder = rig
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        editor.paint(&mut encoder, &rig.target.ui_view);
        rig.gpu.queue.submit([encoder.finish()]);
        rig.save(name)
    };
    let tool = |editor: &mut Editor, name: &str, args: Value| {
        editor
            .campaign_tool(name, args)
            .map_err(|e| anyhow::anyhow!("{name}: {e}"))
    };
    editor.select(Kind::Campaigns);
    editor.open_first(1);
    // The first check passes before missions can make their maps.
    for _ in 0..4 {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
    }
    let start = Instant::now();
    while editor.checking() && start.elapsed() < Duration::from_secs(30) {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
        std::thread::sleep(Duration::from_millis(15));
    }
    let document = editor.active_document().context("a campaign")?;
    let root = document
        .strip_suffix("/campaign.json")
        .context("a campaign's document")?
        .to_string();
    let id = root
        .rsplit('/')
        .next()
        .and_then(|n| n.strip_suffix(".utcc"))
        .context("a campaign id")?
        .to_string();
    // Its art, whatever the campaign's interface images are.
    let art = editor.interface_images(&format!("campaign.{id}"));
    let pick = |words: &[&str], n: usize| -> Value {
        art.iter()
            .filter(|a| words.iter().any(|w| a.contains(w)))
            .nth(n)
            .or_else(|| art.get(n))
            .map_or(Value::Null, |a| json!(a))
    };
    // More steps after the campaign's own: a story, two missions with new maps, a chapter
    // between them and a cinematic.
    let added = [
        (
            "story",
            "What the Hollow Sang",
            json!({"pages": [
                {"art": pick(&["story"], 2), "text": "Ashes, and a vow."},
                {"art": pick(&["story"], 3), "text": "No ant will change again."},
            ]}),
        ),
        (
            "mission",
            "Founding",
            json!({"art": pick(&["founding"], 0), "chapterLine": "Prologue · Mission Two",
                   "story": "A quiet glade, and the first walls of a new colony."}),
        ),
        (
            "chapter",
            "Act I: The Glass",
            json!({"art": pick(&["chapter"], 1)}),
        ),
        (
            "mission",
            "The Glass Road",
            json!({"art": pick(&["glass"], 0), "chapterLine": "Act I · Mission Three"}),
        ),
        (
            "cinematic",
            "The Condenser Hums",
            json!({"note": "The condenser wakes; for a moment, something answers."}),
        ),
    ];
    for (kind, title, fields) in added {
        let step = tool(
            &mut editor,
            "add_campaign_step",
            json!({"campaign": id, "kind": kind, "title": title}),
        )?["index"]
            .clone();
        let fields: serde_json::Map<String, Value> = fields
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, v)| !v.is_null())
            .collect();
        tool(
            &mut editor,
            "set_campaign_step",
            json!({"campaign": id, "step": step, "fields": fields}),
        )?;
    }
    let listed = tool(&mut editor, "campaign_steps", json!({"campaign": id}))?;
    let missions: Vec<(usize, String)> = listed["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["kind"] == "mission")
        .filter_map(|s| {
            Some((
                s["index"].as_u64()? as usize,
                format!("{root}/missions/{}.utcm", s["map"].as_str()?),
            ))
        })
        .collect();
    anyhow::ensure!(missions.len() >= 3, "three missions: {missions:?}");
    // Tech, from the race's own tree: the first hall, what it trains and two buildings
    // with their units in the first mission; the next hall and its research in the
    // second; in the third, a building blocked though a unit it trains stays new (a
    // contradiction, in red).
    let (first, second, third) = (missions[0].0, missions[1].0, missions[2].0);
    let tree = tool(
        &mut editor,
        "mission_tech_tree",
        json!({"campaign": id, "step": first}),
    )?;
    let nodes = tree["nodes"].as_array().cloned().unwrap_or_default();
    let halls: Vec<String> = tree["halls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| h.as_str().map(str::to_string))
        .collect();
    let under = |card: &str| -> Vec<String> {
        nodes
            .iter()
            .filter(|n| n["under"] == card && n["id"] != card)
            .filter_map(|n| n["id"].as_str().map(str::to_string))
            .collect()
    };
    let buildings: Vec<String> = nodes
        .iter()
        .filter(|n| {
            n["kind"] == "building"
                && n["tier"] == 1
                && !under(n["id"].as_str().unwrap_or("")).is_empty()
        })
        .filter_map(|n| n["id"].as_str().map(str::to_string))
        .collect();
    let set = |editor: &mut Editor, step: usize, ids: Vec<(String, &str)>| -> Result<()> {
        let set: serde_json::Map<String, Value> =
            ids.into_iter().map(|(id, s)| (id, json!(s))).collect();
        tool(
            editor,
            "set_mission_tech",
            json!({"campaign": id, "step": step, "set": set}),
        )?;
        Ok(())
    };
    let mut one = Vec::new();
    if let Some(hall) = halls.first() {
        one.push((hall.clone(), "new"));
        one.extend(under(hall).into_iter().map(|u| (u, "new")));
    }
    for b in buildings.iter().take(2) {
        one.push((b.clone(), "new"));
        one.extend(under(b).into_iter().take(2).map(|u| (u, "new")));
    }
    set(&mut editor, first, one)?;
    let mut two = Vec::new();
    if let Some(hall) = halls.get(1) {
        two.push((hall.clone(), "new"));
        two.extend(under(hall).into_iter().map(|u| (u, "new")));
    }
    set(&mut editor, second, two)?;
    if let Some(b) = buildings.get(2).or(buildings.first()) {
        let mut three = vec![(b.clone(), "blocked")];
        three.extend(under(b).into_iter().rev().take(1).map(|u| (u, "new")));
        set(&mut editor, third, three)?;
    }
    // The first mission's computer players: an enemy sending waves of the race's
    // fighters, an ally holding its base.
    let fighters: Vec<String> = nodes
        .iter()
        .filter(|n| n["kind"] == "unit")
        .filter_map(|n| n["id"].as_str().map(str::to_string))
        .skip(1)
        .take(3)
        .collect();
    let race = tool(&mut editor, "campaign_steps", json!({"campaign": id}))?["race"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let wave = |after: u32, every: u32, counts: &[u32]| {
        json!({"after": after, "every": every,
               "units": fighters.iter().zip(counts).map(|(u, c)| json!({"unit": u, "count": c})).collect::<Vec<_>>(),
               "from": "trained", "target": "base"})
    };
    let mut first_wave = wave(120, 90, &[2, 1]);
    if let Some(unit) = fighters.first() {
        first_wave["grow"] = json!({"every": 3, "unit": unit, "count": 1});
    }
    first_wave["hero"] = json!(true);
    let mut late = wave(600, 120, &[3, 2, 2]);
    late["target"] = json!("nearest");
    tool(
        &mut editor,
        "set_mission_players",
        json!({"map": missions[0].1.clone(), "players": [
            {"player": 2, "race": race, "team": "enemy",
             "ai": {"behaviour": "waves", "waves": [first_wave, late]}},
            {"player": 3, "race": race, "team": "ally", "vision": true,
             "ai": {"behaviour": "base", "rebuild": false}},
        ]}),
    )?;

    editor.select(Kind::Campaigns);
    editor.open_document(&document);
    editor.select_campaign_step(&document, None);
    shoot(&mut editor, rig, "editor-campaign")?;
    editor.select_campaign_step(&document, Some(first));
    shoot(&mut editor, rig, "editor-campaign-mission")?;
    editor.select_campaign_step(&document, Some(1));
    shoot(&mut editor, rig, "editor-campaign-story")?;
    editor.show_problems(true);
    shoot(&mut editor, rig, "editor-campaign-problems")?;
    editor.show_problems(false);
    // The Tech view of the first mission's map, then the third mission's (its blocked
    // building and the contradiction), then only what the third changes.
    editor.open_mission(&missions[0].1, true);
    for _ in 0..40 {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
        std::thread::sleep(Duration::from_millis(25));
    }
    shoot(&mut editor, rig, "editor-campaign-tech")?;
    editor.set_tech_view(Some(third), false);
    shoot(&mut editor, rig, "editor-campaign-tech-contradiction")?;
    editor.set_tech_view(Some(third), true);
    shoot(&mut editor, rig, "editor-campaign-tech-changes")?;
    editor.set_tech_view(None, false);
    // The map itself with its Players in the panel.
    editor.set_block_view("blocks");
    for _ in 0..20 {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
        std::thread::sleep(Duration::from_millis(25));
    }
    // The panel scrolled down to the players and their waves.
    let over = egui::pos2(
        (rig.size[0] as f32 - 150.0) / rig.scale,
        (rig.size[1] as f32 * 0.6) / rig.scale,
    );
    for _ in 0..6 {
        editor.run_headless_with(
            rig.size,
            rig.scale,
            &rig.gpu,
            vec![
                egui::Event::PointerMoved(over),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -120.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    shoot(&mut editor, rig, "editor-campaign-players")?;
    let unsaved = editor.unsaved();
    anyhow::ensure!(
        unsaved.iter().any(|d| d.ends_with("campaign.json"))
            && unsaved
                .iter()
                .any(|d| d.contains("/missions/") && d.ends_with("terrain/blocks.grid")),
        "the steps and the missions' maps are unsaved edits: {unsaved:?}"
    );
    editor.undo_all();
    for _ in 0..4 {
        editor.run_headless(rig.size, rig.scale, &rig.gpu);
    }
    anyhow::ensure!(
        editor.unsaved().is_empty(),
        "undo takes every campaign edit back"
    );
    Ok(())
}

/// Answers the updater's requests from a script (the captures never reach the network).
struct Scripted(std::rc::Rc<std::cell::RefCell<std::collections::VecDeque<crate::update::Event>>>);

impl crate::update::Backend for Scripted {
    fn check(&mut self) {}
    fn changelog(&mut self, _: canopy_release::Version) {}
    fn download(&mut self, _: &crate::update::Offer) {}
    fn install(&mut self, _: &Path) {}
    fn relaunch(&mut self) -> Result<(), String> {
        Err("captures do not restart".into())
    }
    fn blocker(&self, _: &crate::update::Offer) -> Option<String> {
        None
    }
    fn poll(&mut self) -> Option<crate::update::Event> {
        self.0.borrow_mut().pop_front()
    }
}

/// The release plaque in each update state, What's new with a short and a long history
/// (the whole bundled log, as an installed copy of its newest release shows it), its
/// gallery, and the update details. Updates are scripted (`-sample`: an offered version
/// one patch above the installed one, with the newest logged release's notes).
fn releases(rig: &mut Rig) -> Result<()> {
    use super::releases::View;
    use crate::update::{Event, Offer, Updater, history};
    let queue = std::rc::Rc::new(std::cell::RefCell::new(std::collections::VecDeque::new()));
    rig.game.updater = Updater::new(Box::new(Scripted(queue.clone())));
    let send = |rig: &mut Rig, event: Event| {
        queue.borrow_mut().push_back(event);
        rig.settle(0.1);
    };
    // The plaque: checking, latest, check unavailable.
    rig.game.updater.check();
    rig.settle(0.1);
    rig.shoot("main-menu-checking")?;
    send(rig, Event::Checked(Ok(None)));
    rig.shoot("main-menu-latest")?;
    rig.game.updater.check();
    send(rig, Event::Checked(Err("offline".into())));
    rig.shoot("main-menu-check-unavailable")?;

    // What's new: this copy's history.
    rig.game.open_releases(View::Notes);
    rig.settle(0.3);
    rig.shoot("whats-new")?;
    rig.game.close_releases();
    // The whole log as its newest release's copy shows it, then the release with images.
    let log = history::log();
    let newest = log.releases.iter().map(|r| r.version).max();
    if let Some(newest) = newest {
        rig.game.releases.set_history(log.history(newest), newest);
        rig.game.open_releases(View::Notes);
        rig.settle(0.3);
        rig.shoot("whats-new-history")?;
        let illustrated = log
            .history(newest)
            .iter()
            .position(|r| r.changes.images.as_ref().is_some_and(|i| !i.is_empty()));
        if let Some(i) = illustrated {
            rig.game.releases.select(i);
            rig.settle(1.5);
            rig.shoot("whats-new-gallery")?;
            // Scrolled into the notes.
            wheel(rig, "releases/main", -12.0);
            rig.shoot("whats-new-notes")?;
            rig.game.releases.open_viewer(&rig.game.updater, 0);
            rig.settle(0.3);
            rig.shoot("whats-new-image")?;
        }
        rig.game.close_releases();
        rig.game.close_releases();
        rig.game
            .releases
            .set_history(history::installed_history(), history::installed());
    }

    // Updates: the details while checking, up to date, failed, offered, downloading and
    // ready; the plaque with an update.
    rig.game.updater.check();
    rig.game.open_releases(View::Updates);
    rig.settle(0.2);
    rig.shoot("updates-checking")?;
    send(rig, Event::Checked(Ok(None)));
    rig.shoot("updates-current")?;
    rig.game.updater.check();
    send(rig, Event::Checked(Err("offline".into())));
    rig.shoot("updates-check-unavailable")?;
    let installed = history::installed();
    let offered = canopy_release::Version {
        patch: installed.patch + 1,
        ..installed
    };
    let offer = Offer {
        version: offered,
        notes: log
            .releases
            .first()
            .map(|r| r.plain_notes())
            .unwrap_or_default(),
        package: None,
    };
    rig.game.updater.check();
    send(rig, Event::Checked(Ok(Some(offer.clone()))));
    rig.shoot("updates-available-plain-sample")?;
    if let Some(entry) = log.releases.first() {
        let mut entry = entry.clone();
        entry.version = offered;
        entry.title = format!("Sample update: {}", entry.title);
        send(rig, Event::Changelog(offered, Some(entry)));
        rig.settle(0.3);
        rig.shoot("updates-available-sample")?;
    }
    rig.game.updater.download();
    send(rig, Event::Started(Some(412 * 1024 * 1024)));
    send(rig, Event::Received(157 * 1024 * 1024));
    rig.shoot("updates-downloading-sample")?;
    send(
        rig,
        Event::Downloaded(Ok(PathBuf::from("/nonexistent/sample.tar.gz"))),
    );
    rig.shoot("updates-ready-sample")?;
    rig.game.close_releases();
    rig.settle(0.1);
    rig.shoot("main-menu-update-available-sample")?;
    // Back to a quiet updater for the remaining captures.
    rig.game.offline_updates();
    Ok(())
}

/// Turns the wheel over a node's centre.
fn wheel(rig: &mut Rig, path: &str, lines: f32) {
    let ui = &mut rig.game.host.ui;
    if let Some(r) = ui.find(path).and_then(|id| ui.rect(id)) {
        ui.input(&canopy_ui::InputEvent::PointerMoved {
            x: r.x + r.w * 0.5,
            y: r.y + r.h * 0.5,
        });
        ui.input(&canopy_ui::InputEvent::Wheel { x: 0.0, y: lines });
    }
    rig.settle(0.1);
}

fn settings_tabs(rig: &mut Rig) -> Result<()> {
    use super::settings_ui::Tab;
    rig.game.open_settings();
    rig.settle(0.2);
    rig.shoot("settings-graphics")?;
    for (tab, name) in [
        (Tab::Audio, "settings-audio"),
        (Tab::Interface, "settings-interface"),
        (Tab::Gameplay, "settings-gameplay"),
    ] {
        rig.game.settings_ui.tab = tab;
        rig.settle(0.1);
        rig.shoot(name)?;
    }
    // A refused rebinding (the key is taken), then one waiting for a key.
    rig.game.settings_ui.tab = Tab::Keys;
    let catalog = crate::settings::keys::catalog();
    rig.game.settings_ui.capturing = Some("interface.gameMenu".into());
    let _ = rig.game.settings_ui.capture(
        &mut rig.game.settings,
        &catalog,
        crate::settings::Chord::parse("KeyR"),
    );
    rig.game.settings_ui.capturing = Some("interface.toggleHud".into());
    rig.settle(0.1);
    rig.shoot("settings-keys")?;
    rig.game.settings_ui.capturing = None;
    rig.game.settings_ui.tab = Tab::Network;
    rig.settle(0.1);
    rig.shoot("settings-network")?;
    rig.game.settings_ui.tab = Tab::Graphics;
    rig.game.close_settings();
    Ok(())
}

fn skirmish(rig: &mut Rig) -> Result<()> {
    // The largest map, a team game with the player on the second start.
    rig.game.apply(Intent::Open(Screen::Skirmish));
    let data = rig.game.data.clone().context("booted")?;
    let maps = super::skirmish::Skirmish::maps(&data);
    let largest = maps
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.starts.len())
        .map_or(0, |(i, _)| i);
    rig.game.skirmish.choose_map(&data, largest);
    if rig.game.skirmish.slots.len() >= 4 {
        rig.game.skirmish.set_human(1, true);
        for (i, team) in [(0, 0), (1, 0), (2, 1), (3, 1)] {
            rig.game.skirmish.slots[i].team = Some(team);
        }
        rig.game.skirmish.slots[3].difficulty = canopy_sim::Difficulty::Hard;
    }
    rig.settle(0.4);
    rig.shoot("skirmish")?;
    // A dropdown open.
    if let Some(id) = rig.game.host.ui.find("skirmish/players/0/race")
        && let Some(r) = rig.game.host.ui.rect(id)
    {
        let (x, y) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        rig.game
            .host
            .ui
            .input(&canopy_ui::InputEvent::PointerMoved { x, y });
        for pressed in [true, false] {
            rig.game
                .host
                .ui
                .input(&canopy_ui::InputEvent::PointerButton {
                    button: canopy_ui::PointerButton::Primary,
                    pressed,
                });
        }
        rig.settle(0.1);
        rig.shoot("skirmish-race-select")?;
        for pressed in [true, false] {
            rig.game.host.ui.input(&canopy_ui::InputEvent::Key {
                key: canopy_ui::Key::Escape,
                pressed,
                repeat: false,
            });
        }
    }
    rig.game.apply(Intent::Back);
    Ok(())
}

/// The multiplayer list and a lobby against a real server, when one is named.
fn multiplayer(rig: &mut Rig) -> Result<()> {
    use canopy_net::wire::CreateRoom;
    let server = std::env::var(canopy_net::server::ENV).ok();
    let mut other = None;
    if let Some(text) = &server {
        let address = canopy_net::ServerAddress::parse(text)?;
        let net = canopy_net::Net::new(address)?;
        let data = rig.game.data.clone().context("booted")?;
        let map = data
            .skirmish_maps()
            .max_by_key(|m| m.starts.len())
            .context("a map")?;
        let bytes = map.identity()?;
        let joined = net
            .create_room(&CreateRoom {
                name: "Bo's lobby".into(),
                map_id: map.id().into(),
                map_revision: super::multiplayer::map_revision(&bytes),
                slot_count: 3,
                guest_name: "Bo".into(),
                player_id: "Bo".into(),
                protocol: canopy_net::wire::PROTOCOL,
            })
            .wait()?;
        let session = net.enter(&joined);
        other = Some((net, session));
    }
    rig.game.apply(Intent::Open(Screen::Multiplayer));
    rig.settle(if server.is_some() { 1.5 } else { 3.0 });
    rig.shoot("multiplayer")?;
    rig.game.mp.tab = super::multiplayer::Tab::Create;
    rig.settle(0.2);
    rig.shoot("multiplayer-create")?;
    rig.game.mp.tab = super::multiplayer::Tab::Find;
    rig.settle(0.2);
    if server.is_some() && !rig.game.mp.rooms.is_empty() {
        let data = rig.game.data.clone().context("booted")?;
        rig.game.mp.go(
            &data.skirmish_maps().collect::<Vec<_>>(),
            "Jiri",
            "Jiri1",
            canopy_net::wire::Role::Player,
        );
        rig.until("the lobby", Duration::from_secs(20), |g| {
            *g.flow.screen() == Screen::Lobby
        })?;
        rig.settle(2.5);
        rig.shoot("lobby")?;
        rig.game.apply(Intent::Back);
        rig.settle(0.3);
    }
    if let Some((net, mut session)) = other {
        session.close();
        lobby_host(rig, &net)?;
    }
    rig.game.apply(Intent::Back);
    rig.settle(0.2);
    Ok(())
}

/// The host's lobby on the largest map: a computer seat on team 1 with the host, a
/// closed seat, and a second player who joined.
fn lobby_host(rig: &mut Rig, net: &canopy_net::Net) -> Result<()> {
    use canopy_net::wire::{JoinRoom, Role};
    let data = rig.game.data.clone().context("booted")?;
    let maps: Vec<_> = data.skirmish_maps().collect();
    let (index, map) = maps
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.starts.len())
        .context("a map")?;
    rig.game.mp.tab = super::multiplayer::Tab::Create;
    rig.game.mp.selected_map = index;
    rig.game.mp.seats = map.starts.len().saturating_sub(2);
    rig.game.mp.go(&maps, "Jiri", "Jiri1", Role::Player);
    rig.until("the hosted lobby", Duration::from_secs(20), |g| {
        *g.flow.screen() == Screen::Lobby && g.mp.room().is_some()
    })?;
    let id = rig.game.mp.room().context("a room")?.id.clone();
    let data = rig.game.data.clone().context("game data")?;
    rig.game.mp.choose(&data, "seat", 1, 2);
    rig.game.mp.choose(&data, "seat", 3, 1);
    rig.settle(0.5);
    rig.game.mp.choose(&data, "team", 1, 0);
    let joined = net
        .join_room(
            &id,
            &JoinRoom {
                guest_name: "Bo".into(),
                role: Role::Player,
                player_id: "Bo".into(),
                protocol: canopy_net::wire::PROTOCOL,
            },
        )
        .wait()?;
    let mut guest = net.enter(&joined);
    rig.settle(2.5);
    rig.shoot("lobby-host")?;
    guest.close();
    rig.game.apply(Intent::Back);
    rig.settle(0.3);
    Ok(())
}

/// A skirmish match: loading, the match, overlays, its menu, saves, a confirmation and
/// the result.
/// Starts a skirmish on the first map, the player against computer players.
fn start_skirmish(rig: &mut Rig) -> Result<()> {
    let data = rig.game.data.clone().context("booted")?;
    rig.game.apply(Intent::Open(Screen::Skirmish));
    rig.game.skirmish.choose_map(&data, 0);
    rig.game.skirmish.set_human(0, true);
    for slot in &mut rig.game.skirmish.slots {
        slot.team = None;
    }
    rig.game.start_skirmish();
    rig.settle(0.15);
    Ok(())
}

/// Opens the feedback form as the window does: the next frame, drawn without the form
/// and the game menu, becomes its screenshot (also written as `<name>.jpg`).
fn open_feedback(rig: &mut Rig, action: &str, name: &str) -> Result<()> {
    rig.click(action);
    rig.frame();
    anyhow::ensure!(rig.game.grab_due(), "the form asks for a screenshot");
    let (view, ui_view) = (rig.target.view.clone(), rig.target.ui_view.clone());
    let mut ctx = Ctx {
        gpu: &rig.gpu,
        renderer: &mut rig.renderer,
        window: None,
        size: rig.size,
        scale: rig.scale,
    };
    rig.game.render(&mut ctx, &view, &ui_view);
    rig.game.grab(&rig.gpu, &rig.target.texture);
    rig.until("the screenshot", Duration::from_secs(20), |g| {
        !g.grab.busy()
    })?;
    let shot = rig.game.feedback.shot().context("a screenshot")?;
    let path = rig.out.join(format!("{name}.jpg"));
    std::fs::write(&path, &shot.jpeg)?;
    println!("wrote {}", path.display());
    Ok(())
}

fn describe(rig: &mut Rig, category: crate::feedback::Category, text: &str) {
    rig.game.feedback.category = category;
    rig.game.feedback.description = text.into();
    rig.game.feedback.fill(&mut rig.game.host.ui);
}

/// The feedback form over the main menu, and a report going out: saved for later when
/// the server cannot be reached, sent when `CANOPY_MATCH_SERVER` names a local server.
/// Captures never send to the deployed server.
fn feedback_menu(rig: &mut Rig) -> Result<()> {
    use super::feedback::Phase;
    use crate::feedback::Category;
    let local = std::env::var(canopy_net::server::ENV)
        .ok()
        .filter(|s| !s.trim().is_empty());
    open_feedback(rig, "menu.feedback", "feedback-screenshot-menu")?;
    rig.settle(0.2);
    rig.shoot("feedback-main-menu")?;
    describe(
        rig,
        Category::Idea,
        "Let me queue research from the building's tooltip",
    );
    rig.settle(0.1);
    rig.shoot("feedback-main-menu-filled")?;
    // Nothing listens there: the report is saved for later.
    rig.game.feedback_server = Some("http://127.0.0.1:9".into());
    rig.click("fb.submit");
    anyhow::ensure!(
        matches!(rig.game.feedback.phase, Phase::Sending { .. }),
        "the report goes out"
    );
    rig.shoot("feedback-sending")?;
    rig.until("the report saved for later", Duration::from_secs(30), |g| {
        g.feedback.phase == Phase::Queued
    })?;
    rig.settle(0.1);
    rig.shoot("feedback-queued")?;
    anyhow::ensure!(
        crate::feedback::Outbox::new(&rig.game.paths.user)
            .waiting()
            .len()
            == 1,
        "the report waits in the outbox"
    );
    rig.click("fb.close");
    if let Some(server) = local {
        rig.game.feedback_server = Some(server);
        open_feedback(rig, "menu.feedback", "feedback-screenshot-menu-2")?;
        describe(rig, Category::Bug, "The menu music stops after a match");
        rig.click("fb.submit");
        rig.until("the report sent", Duration::from_secs(30), |g| {
            matches!(g.feedback.phase, Phase::Sent { .. })
        })?;
        rig.settle(0.1);
        rig.shoot("feedback-sent")?;
        rig.click("fb.close");
    }
    rig.game.feedback_server = None;
    rig.settle(0.2);
    Ok(())
}

/// The feedback form over a match (opened from the game menu, which stays under it).
fn feedback_match(rig: &mut Rig) -> Result<()> {
    open_feedback(rig, "gm.feedback", "feedback-screenshot-match")?;
    rig.until("the match's files", Duration::from_secs(30), |g| {
        g.feedback.files_ready()
    })?;
    anyhow::ensure!(
        rig.game.feedback.files().replay.is_some(),
        "the replay of the match is attached"
    );
    describe(
        rig,
        crate::feedback::Category::Bug,
        "Workers stop gathering when the tree next to the hall falls",
    );
    rig.settle(0.2);
    rig.shoot("feedback-match")?;
    rig.click("fb.cancel");
    anyhow::ensure!(
        rig.game.flow.top() == Some(Overlay::GameMenu),
        "back to the game menu"
    );
    rig.settle(0.1);
    Ok(())
}

fn a_match(rig: &mut Rig) -> Result<()> {
    let data = rig.game.data.clone().context("booted")?;
    start_skirmish(rig)?;
    rig.shoot("loading")?;
    rig.until("the match", Duration::from_secs(180), |g| g.flow.in_match())?;
    rig.settle(3.0);
    rig.shoot("match")?;
    // The play HUD with the player's units selected and kept as control group 1.
    let mine: Vec<u32> = rig.game.playing.as_ref().map_or_else(Vec::new, |p| {
        let k = p.game.play.knowledge();
        k.view
            .entities
            .iter()
            .filter(|e| k.controlled(e) && k.is_unit(e))
            .map(|e| e.id().0)
            .collect()
    });
    anyhow::ensure!(!mine.is_empty(), "the player has units");
    if let Some(p) = &mut rig.game.playing {
        p.game.play.select(mine.clone(), None);
        p.game.play.groups.assign(0, &mine);
    }
    rig.settle(0.3);
    rig.shoot("match-selection")?;
    if let Some(playing) = &mut rig.game.playing {
        for (player, name, text) in [
            (1, "Bo", "glhf!"),
            (0, "Jiri", "you too, see you at the river"),
        ] {
            playing.hear(&canopy_net::wire::ChatMessage {
                name: name.into(),
                player: Some(player),
                text: text.into(),
            });
        }
    }
    rig.settle(0.1);
    rig.shoot("match-chat")?;

    // The mission HUD with a sample mission state.
    let mut hud = MissionHud::new(vec![
        objective("gather", "Gather 200 amber", "", false),
        objective(
            "scouts",
            "Find the lost scouts",
            "They went north at dawn. Follow the river past the old ford.",
            false,
        ),
        objective(
            "hollow",
            "Clear the beetle hollow",
            "A nest east of camp.",
            true,
        ),
        objective("bridge", "Keep the bridge standing", "", false),
    ]);
    // The finished objectives listed (the old `<details>` opened).
    hud.history_open = true;
    // The portrait of the content's default starting hero.
    let rules = &data.content.rules;
    let hero = rules
        .race(&rules.default_race)
        .map_or(&rules.starting_setup, |p| p.starting_setup)
        .hero
        .as_ref()
        .map(|h| h.default.clone())
        .context("a starting hero")?;
    let mission = sample_mission(hero);
    let view_h = rig.size[1] as f32 / rig.scale;
    rig.frame();
    let hud_top = rig
        .game
        .host
        .ui
        .find("hud/dock")
        .and_then(|d| rig.game.host.ui.rect(d))
        .map(|r| r.y / rig.scale);
    hud.hud_top = hud_top;
    rig.game.host.ui.show("mission-hud", true);
    for _ in 0..3 {
        hud.show(
            &mut rig.game.host,
            &rig.gpu,
            &data,
            Some(&mission),
            0,
            false,
            0.016,
            view_h,
        );
        rig.game.prepare_ui(&rig.gpu, &mut rig.renderer, 1.0);
    }
    rig.shoot_as_is("mission-hud-sample")?;
    let mut cinematic = mission.clone();
    if let Some(d) = &mut cinematic.dialogue {
        d.cinematic = true;
        d.remaining = 80;
    }
    for _ in 0..60 {
        hud.show(
            &mut rig.game.host,
            &rig.gpu,
            &data,
            Some(&cinematic),
            0,
            false,
            0.016,
            view_h,
        );
        rig.game.prepare_ui(&rig.gpu, &mut rig.renderer, 1.0);
    }
    rig.shoot_as_is("mission-cinematic-sample")?;

    // A connection problem as a networked match shows it.
    rig.game.problem = Some((
        "Out of sync".into(),
        "The players' games disagreed at beat 4120, so the match cannot continue. This is a bug; the log has the details.".into(),
    ));
    rig.settle(0.1);
    rig.shoot("match-desync")?;
    rig.game.problem = None;

    rig.game.apply(Intent::Show(Overlay::GameMenu));
    rig.game.menu_page = MenuPage::Main;
    rig.settle(0.2);
    rig.shoot("game-menu")?;
    feedback_match(rig)?;
    rig.click("gm.save");
    super::widgets::text(
        &mut rig.game.host.ui,
        "game-menu/save-page/name",
        "Before the river crossing",
    );
    rig.settle(0.1);
    rig.shoot("game-menu-save")?;
    rig.game.save_game();
    rig.game.save_game_named("An opening");
    rig.settle(0.1);
    rig.shoot("game-menu-saved")?;
    rig.game.open_saves();
    rig.settle(0.1);
    rig.shoot("saves")?;
    // Loading the save continues the match with its camera and control groups.
    let saved = rig
        .game
        .saves_list
        .iter()
        .find(|m| m.name == "An opening")
        .map(|m| m.id.clone())
        .context("the save is listed")?;
    rig.game.load_save(&saved);
    rig.until("the loaded match", Duration::from_secs(180), |g| {
        g.flow.in_match() && g.playing.is_some()
    })?;
    rig.settle(1.5);
    let groups = rig
        .game
        .playing
        .as_ref()
        .map(|p| p.game.play.groups.get(0).to_vec());
    anyhow::ensure!(
        groups.as_deref() == Some(mine.as_slice()),
        "control group 1 comes back with the save: {groups:?}"
    );
    rig.shoot("match-loaded")?;
    rig.game.apply(Intent::Show(Overlay::GameMenu));
    rig.game.menu_page = MenuPage::Main;
    rig.click("gm.exit");
    rig.settle(0.1);
    rig.shoot("game-menu-leave")?;
    rig.click("gm.page-back");
    rig.game.apply(Intent::Back);
    // The generic confirmation, as deleting a save asks it.
    rig.game.open_saves();
    let first = rig.game.saves_list.first().cloned();
    if let Some(meta) = first {
        rig.game.ask(super::Dialog {
            ask: super::Ask::DeleteSave(meta.id.clone()),
            title: "Delete this save?".into(),
            body: format!("“{}” is removed for good.", meta.name),
            confirm: "Delete".into(),
            danger: true,
            second: None,
        });
        rig.settle(0.1);
        rig.shoot("confirm-delete")?;
        rig.game.dialog = None;
        rig.game.apply(Intent::Back);
    }
    rig.game.apply(Intent::Back);

    // The result: a surrender, then a decided match.
    rig.game.decided(None, true);
    rig.settle(0.3);
    rig.shoot("result-defeat")?;
    let outcome = canopy_sim::state::Outcome {
        winner: Some(canopy_content::placement::Owner::Player(0)),
        defeated: vec![canopy_content::placement::Owner::Player(1)],
    };
    if let Some(playing) = &mut rig.game.playing {
        playing.outcome = Some(outcome.clone());
    }
    rig.game.decided(Some(outcome), false);
    rig.settle(0.2);
    rig.shoot("result-victory")?;
    rig.click("result.save-replay");
    rig.settle(0.2);
    anyhow::ensure!(
        matches!(rig.game.replay_saved, Some(Ok(_))),
        "the replay is saved: {:?}",
        rig.game.replay_saved
    );
    rig.shoot("result-replay-saved")?;
    rig.game.apply(Intent::LeaveMatch);
    rig.settle(0.2);
    replays(rig)?;
    rig.game.apply(Intent::Back);
    rig.settle(0.2);
    rig.game.apply(Intent::Back);
    rig.settle(0.2);
    Ok(())
}

/// The replays (D60): the list with an entry from another game version, the viewer with
/// its bar, the statistics panel, one player's vision, the timeline dragged and a jump,
/// and a replay out of sync. Starts on the skirmish screen after a match was left (its
/// replay is the last match), ends there.
fn replays(rig: &mut Rig) -> Result<()> {
    use crate::replay::library::LAST_MATCH;
    let library = rig.game.replay_library.clone();
    let last = library
        .list()
        .into_iter()
        .find(|e| e.name == LAST_MATCH)
        .context("the last match is kept")?;
    let mut old = crate::replay::format::Replay::read(&last.path)?;
    // As another version of the game recorded it.
    old.header.game_version = "0.3.5".into();
    old.header.identity.content_digest = "0".repeat(64);
    old.header.recorded_at -= 86_400_000;
    let old_name = library.write(&old, &format!("{} from last week", old.header.map_name))?;
    // A replay whose recorded fingerprints no longer match from its second one.
    let mut changed = crate::replay::format::Replay::read(&last.path)?;
    match changed.body.fingerprints.get_mut(1) {
        Some(f) => f.1 ^= 1,
        // A short match: a fingerprint of its own that cannot match.
        None => changed.body.fingerprints = vec![(changed.header.start_tick + 40, 0)],
    }
    changed.header.recorded_at -= 3_600_000;
    let changed_name = library.write(&changed, "Out of sync sample")?;
    rig.game.apply(Intent::Open(Screen::Replays));
    rig.settle(0.4);
    let index = |g: &Game, name: &str| g.replays.entries.iter().position(|e| e.name == name);
    rig.game.replays.selected = index(&rig.game, LAST_MATCH).context("listed")?;
    rig.settle(0.2);
    rig.shoot("replays")?;
    rig.game.replays.selected = index(&rig.game, &old_name).context("listed")?;
    rig.settle(0.2);
    rig.shoot("replays-incompatible")?;
    anyhow::ensure!(
        rig.game
            .replays
            .watchable(rig.game.data.as_ref().context("data")?)
            .is_err(),
        "another version's replay cannot be watched"
    );
    // Watching the last match.
    rig.game.replays.selected = index(&rig.game, LAST_MATCH).context("listed")?;
    rig.game.watch_selected();
    rig.until("the replay", Duration::from_secs(180), |g| {
        g.flow.in_match() && g.playing.as_ref().is_some_and(|p| p.watching.is_some())
    })?;
    rig.settle(4.0);
    rig.shoot("replay-viewer")?;
    rig.game.replay_ui.details = true;
    rig.settle(0.3);
    rig.shoot("replay-statistics")?;
    rig.game.replay_ui.tab = 2;
    rig.settle(0.2);
    rig.shoot("replay-units-lost")?;
    rig.game.replay_ui.tab = 3;
    rig.settle(0.2);
    rig.shoot("replay-production")?;
    rig.game.replay_ui.details = false;
    let first = rig
        .game
        .playing
        .as_ref()
        .and_then(|p| p.names.first().map(|(slot, _)| *slot));
    if let Some(p) = &mut rig.game.playing {
        p.game.set_vision(first);
    }
    rig.settle(1.0);
    rig.shoot("replay-vision-player")?;
    if let Some(p) = &mut rig.game.playing {
        p.game.set_vision(None);
    }
    // The timeline held at 60 % (its preview), then the jump made.
    rig.game.replay_ui.drag = Some(0.6);
    rig.settle(0.2);
    rig.shoot("replay-scrub")?;
    rig.game.replay_ui.drag = None;
    let live = rig
        .game
        .playing
        .as_ref()
        .and_then(|p| p.status().replay)
        .context("a replay's transport")?;
    let target = live.start + (live.end - live.start) * 3 / 5;
    if let Some(p) = &rig.game.playing {
        p.game.sim().control(crate::sim::Control::Seek(target));
    }
    rig.until("the jump", Duration::from_secs(30), |g| {
        g.playing
            .as_ref()
            .and_then(|p| p.status().replay)
            .and_then(|r| r.last_seek)
            .is_some()
    })?;
    rig.settle(1.0);
    rig.shoot("replay-seeked")?;
    rig.game.apply(Intent::LeaveMatch);
    rig.until("the replays", Duration::from_secs(30), |g| {
        matches!(g.flow.screen(), Screen::Replays)
    })?;
    // The replay out of sync: the notice and the timeline's mark.
    rig.game.replays.refresh(&library);
    rig.game.replays.selected = index(&rig.game, &changed_name).context("listed")?;
    rig.game.watch_selected();
    rig.until("the replay out of sync", Duration::from_secs(180), |g| {
        g.playing
            .as_ref()
            .and_then(|p| p.status().replay)
            .is_some_and(|r| r.desync.is_some())
    })?;
    rig.settle(1.0);
    rig.shoot("replay-out-of-sync")?;
    rig.game.apply(Intent::LeaveMatch);
    rig.until("the replays", Duration::from_secs(30), |g| {
        matches!(g.flow.screen(), Screen::Replays)
    })?;
    rig.settle(0.3);
    Ok(())
}
