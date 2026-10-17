//! The window and its event loop. Everything shown in it, menus and matches alike, is
//! the screens' ([`crate::screens::Game`]): this file opens the window and the renderer,
//! forwards events and presents frames.
//!
//! `--map <id>` jumps through the flow straight into a skirmish (you on the first start
//! against computer players, `--players N` in all); `--spectate <id>` watches computer
//! players on every start. In a match the camera follows the player's camera shortcuts
//! (arrow keys and PageUp/PageDown by default) and screen-edge scrolling, the wheel and
//! middle-drag grab; spectators also turn with Q/E and jump to the largest fight with B;
//! F9 opens the debug overlay.
//!
//! The editor (`canopy_editor`) takes the whole window while it is open: from the main
//! menu's Editor entry or `canopy edit`. The game waits behind it, not updated, and comes
//! back when the editor closes.

use crate::{
    gpu::Display,
    options::{Command, Options},
    paths::Paths,
    screens::{Ctx, DevStart, Game, loading::DevSpec},
    settings::{Settings, WindowMode},
    telemetry,
};
use anyhow::Result;
use canopy_render::Renderer;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How often a hidden window's loop runs: the game keeps going (matches, the network)
/// without drawing.
const HIDDEN_FRAME: Duration = Duration::from_millis(16);
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Fullscreen, Window, WindowId},
};

struct Session {
    display: Display,
    renderer: Renderer,
    game: Game,
    /// Open while editing; it draws the window instead of the game.
    editor: Option<canopy_editor::Editor>,
    /// The editor waiting while Test in game plays its data.
    suspended: Option<canopy_editor::Editor>,
}

/// Measurements gathered between two log lines.
#[derive(Default)]
struct Stats {
    frames: u32,
    cpu_ms: f32,
    cpu_max: f32,
    dt_ms: f32,
    dt_max: f32,
}

pub struct App {
    options: Options,
    paths: Paths,
    window: Option<Arc<Window>>,
    session: Option<Session>,
    error: Option<anyhow::Error>,
    last_frame: Instant,
    stats: Stats,
    stats_since: Instant,
    quality_override: Option<canopy_render::Quality>,
}

impl App {
    pub fn new(options: Options, paths: Paths) -> Self {
        let now = Instant::now();
        Self {
            paths,
            window: None,
            session: None,
            error: None,
            last_frame: now,
            stats: Stats::default(),
            stats_since: now,
            quality_override: (options.quality != canopy_render::Quality::default())
                .then_some(options.quality),
            options,
        }
    }

    /// The error that stopped the game, if any.
    pub fn finish(self) -> Result<()> {
        self.error.map_or(Ok(()), Err)
    }

    /// A match the command line asks for (`--map`, `--spectate`), skipping the menus.
    fn dev_start(&self) -> Option<DevStart> {
        let o = &self.options;
        let map = o.map.clone()?;
        Some(DevStart {
            spec: DevSpec {
                map,
                hour: o.hour,
                players: o.players,
                biome: o.biome.clone(),
                crowd: o.crowd,
                colors: o.colors.clone(),
                human: (!o.spectate && o.crowd.is_none()).then_some(0),
                encounter: None,
            },
            tick: o.tick,
            camera: o
                .camera
                .and_then(|c| c.pose())
                .map(|p| [p.x, p.z, p.zoom, p.yaw]),
        })
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let (mut settings, note) = Settings::load(&self.paths.user);
        if let Some(error) = note.error {
            tracing::warn!(%error, moved = ?note.moved_aside, "settings could not be read; defaults used");
        }
        let mut attributes = Window::default_attributes()
            .with_title("Under the Canopy")
            .with_inner_size(LogicalSize::new(1600, 900))
            .with_min_inner_size(LogicalSize::new(1024, 600));
        if settings.graphics.window == WindowMode::Fullscreen {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(event_loop.create_window(attributes)?);
        // `--no-vsync` (measuring) wins over the setting.
        let vsync = self.options.vsync && settings.graphics.vsync;
        let display = Display::new(window.clone(), vsync)?;
        let look = canopy_scene::Look::load(&self.paths.data, canopy_content::map::BIOMES[0])?;
        let environment = canopy_render::Environment::new(&look.lighting, &look.biome, 12.0, None);
        let mut renderer =
            Renderer::new(&display.gpu, display.format(), display.size(), environment);
        // `Auto` (the default) takes the adapter's preset (D42).
        settings.graphics.detect(display.gpu.preset());
        tracing::info!(preset = ?settings.graphics.active_preset(), "graphics");
        renderer.set_quality(&display.gpu, settings.graphics.quality(display.size()));
        renderer.shadows = settings.graphics.shadows;
        let mut game = Game::new(self.paths.clone(), settings, &display.gpu, self.dev_start())?;
        if let Command::Replay { file, .. } = &self.options.command {
            game.watch_on_boot(file.clone());
        }
        window.request_redraw();
        self.window = Some(window);
        self.session = Some(Session {
            display,
            renderer,
            game,
            editor: None,
            suspended: None,
        });
        if self.options.command == Command::Edit {
            self.open_editor();
        }
        Ok(())
    }

    fn open_editor(&mut self) {
        let (Some(session), Some(window)) = (&mut self.session, &self.window) else {
            return;
        };
        if session.editor.is_some() {
            return;
        }
        let setup = editor_setup(&self.paths);
        let format = session.display.format().remove_srgb_suffix();
        session.editor = Some(canopy_editor::Editor::new(
            window,
            &session.display.gpu,
            format,
            setup,
        ));
        session.game.audio.set_background(true);
        window.set_title("Under the Canopy · Editor");
    }

    /// A frame while the editor is open.
    fn redraw_editor(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(session), Some(window)) = (&mut self.session, &self.window) else {
            return;
        };
        let Some(editor) = &mut session.editor else {
            return;
        };
        let output = editor.run(window, &session.display.gpu);
        if output.quit {
            session.game.quit();
            event_loop.exit();
            return;
        }
        if let Some(test) = output.test {
            match session.game.test_in_game(
                test.content,
                test.map,
                test.map_package,
                test.encounter,
            ) {
                Ok(()) => {
                    session.suspended = session.editor.take();
                    session.game.audio.set_background(false);
                    session.game.forget_cursor();
                    window.set_title("Under the Canopy · Test in game");
                }
                Err(error) => tracing::warn!(%error, "test in game"),
            }
            return;
        }
        if output.close {
            session.editor = None;
            session.game.audio.set_background(false);
            session.game.forget_cursor();
            window.set_title("Under the Canopy");
            return;
        }
        let Some(frame) = session.display.acquire() else {
            session.display.gpu.queue.submit([]);
            std::thread::sleep(HIDDEN_FRAME);
            return;
        };
        let gpu = &session.display.gpu;
        let view = session.display.ui_view(&frame);
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("editor"),
            });
        editor.paint(&mut encoder, &view);
        gpu.queue.submit([encoder.finish()]);
        session.display.present(frame);
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt_ms = (now - self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        if self.session.as_ref().is_some_and(|s| s.editor.is_some()) {
            self.redraw_editor(event_loop);
            return;
        }
        let (Some(session), Some(window)) = (&mut self.session, &self.window) else {
            return;
        };
        let size = session.display.size();
        session.renderer.resize(&session.display.gpu, size);
        let scale = window.scale_factor() as f32;
        let requests = session.game.update(&mut Ctx {
            gpu: &session.display.gpu,
            renderer: &mut session.renderer,
            window: Some(window),
            size,
            scale,
        });
        // Graphics options on the command line override the settings once.
        if let Some(quality) = self.quality_override.take() {
            session.renderer.set_quality(&session.display.gpu, quality);
        }
        if requests.exit {
            event_loop.exit();
            return;
        }
        if requests.editor {
            self.open_editor();
            return;
        }
        // Test in game ends when its match is left: back to the editor as it was.
        if session.suspended.is_some() && session.game.test_finished() {
            session.editor = session.suspended.take();
            session.game.audio.set_background(true);
            window.set_title("Under the Canopy · Editor");
            return;
        }
        if let Some(vsync) = requests.vsync {
            session.display.set_vsync(vsync && self.options.vsync);
        }
        // The feedback screenshot: this frame (without the form) is drawn into a texture of
        // its own and read back; the window keeps showing the previous frame.
        if session.game.grab_due() {
            let gpu = &session.display.gpu;
            let target = crate::feedback::grab::Target::new(gpu, session.display.format(), size);
            session.game.render(
                &mut Ctx {
                    gpu,
                    renderer: &mut session.renderer,
                    window: Some(window),
                    size,
                    scale,
                },
                &target.view,
                &target.ui_view,
            );
            session.game.grab(gpu, &target.texture);
            return;
        }
        let Some(frame) = session.display.acquire() else {
            // No frame while the window is hidden, minimised or covered. The update still
            // queued its uploads, which only a submit releases, and nothing paces the loop:
            // without these two lines it piled up over a gigabyte a second.
            session.display.gpu.queue.submit([]);
            std::thread::sleep(HIDDEN_FRAME);
            return;
        };
        let output = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = session.display.ui_view(&frame);
        // The frame logged below measures its GPU time from submit to idle (pass
        // timestamps are not trustworthy on Apple GPUs, D34).
        let log_due = (now - self.stats_since).as_secs_f32() >= 2.0;
        session.renderer.sample_gpu_time |= log_due;
        let report = session.game.render(
            &mut Ctx {
                gpu: &session.display.gpu,
                renderer: &mut session.renderer,
                window: Some(window),
                size,
                scale,
            },
            &output,
            &ui_view,
        );
        session.display.present(frame);
        // A short log line every two seconds while a match shows.
        let st = &mut self.stats;
        st.frames += 1;
        st.dt_ms += dt_ms;
        st.dt_max = st.dt_max.max(dt_ms);
        if let Some(report) = &report {
            st.cpu_ms += report.cpu_total_ms;
            st.cpu_max = st.cpu_max.max(report.cpu_total_ms);
        }
        if (now - self.stats_since).as_secs_f32() >= 2.0 {
            let frames = st.frames.max(1) as f32;
            if let Some(report) = report {
                // Submit to idle of this (sampled) frame; `None` outside a match.
                let gpu = report.gpu_frame_ms;
                let a = report.actors;
                tracing::info!(
                    fps = (1000.0 * frames / st.dt_ms.max(1.0)).round(),
                    frame_max_ms = st.dt_max.round(),
                    cpu_ms = st.cpu_ms / frames,
                    cpu_max_ms = st.cpu_max,
                    gpu_ms = gpu,
                    draws = report.draws,
                    actors = a.instances,
                    visible = a.visible,
                    "frame"
                );
            }
            if let Some(live) = session.game.playing.as_ref().map(|p| p.live())
                && let Some(newest) = live.newest()
            {
                let snapshots = live.snapshots();
                let n = snapshots.len().max(1) as f32;
                let mean = |f: fn(&crate::sim::TickCost) -> f32| {
                    snapshots.iter().map(|s| f(&s.cost)).sum::<f32>() / n
                };
                let c = newest.presentation.counts;
                tracing::info!(
                    tick = newest.tick(),
                    step_ms = mean(|c| c.step),
                    view_ms = mean(|c| c.view),
                    present_ms = mean(|c| c.present),
                    late_frames = live.held,
                    slowdowns = newest.slowdowns,
                    units = c.units,
                    buildings = c.buildings,
                    decided = newest.decided,
                    "simulation"
                );
            }
            *st = Stats::default();
            self.stats_since = now;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(error) = self.open(event_loop) {
            self.error = Some(error);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match &event {
            WindowEvent::CloseRequested => {
                if let Some(session) = &mut self.session {
                    // The editor asks about unsaved changes first; it quits once settled.
                    if let Some(editor) = &mut session.editor
                        && !editor.close_requested()
                    {
                        return;
                    }
                    session.game.quit();
                }
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                if let Some(session) = &mut self.session {
                    session.display.resize(*size);
                    let size = session.display.size();
                    session
                        .game
                        .resized(&mut session.renderer, &session.display.gpu, size);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw(event_loop);
                telemetry::frame_mark();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return;
            }
            _ => {}
        }
        let Some(session) = &mut self.session else {
            return;
        };
        match (&mut session.editor, self.window.as_deref()) {
            (Some(editor), Some(window)) => editor.event(window, &event),
            _ => session.game.window_event(self.window.as_deref(), &event),
        }
    }
}

/// What the editor gets from the game: the base package and the interface fonts.
pub fn editor_setup(paths: &Paths) -> canopy_editor::Setup {
    canopy_editor::Setup {
        data: paths.data.clone(),
        base: paths.data.join(canopy_content::BASE_DIR),
        fonts: interface_fonts(&paths.data.join("config/ui/fonts")),
        user: Some(paths.user.clone()),
    }
}

/// The game's interface fonts (`config/ui/fonts`), by file name; Git LFS pointers (CI
/// checkouts) and unreadable files are left out.
fn interface_fonts(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ttf" || e == "otf"))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .filter(|data| matches!(data.get(..4), Some([0, 1, 0, 0] | b"OTTO" | b"true")))
        .collect()
}
