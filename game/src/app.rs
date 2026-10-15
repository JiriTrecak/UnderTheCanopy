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

use crate::{
    gpu::Display,
    options::Options,
    paths::Paths,
    screens::{Ctx, DevStart, Game, loading::DevSpec},
    settings::{Settings, WindowMode},
    telemetry,
};
use anyhow::Result;
use canopy_render::Renderer;
use std::{sync::Arc, time::Instant};
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
            },
            tick: o.tick,
            camera: o
                .camera
                .and_then(|c| c.pose())
                .map(|p| [p.x, p.z, p.zoom, p.yaw]),
        })
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let (settings, note) = Settings::load(&self.paths.user);
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
        renderer.set_quality(&display.gpu, settings.graphics.quality(display.size()));
        renderer.shadows = settings.graphics.shadows;
        let game = Game::new(self.paths.clone(), settings, &display.gpu, self.dev_start())?;
        window.request_redraw();
        self.window = Some(window);
        self.session = Some(Session {
            display,
            renderer,
            game,
        });
        Ok(())
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt_ms = (now - self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
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
        if let Some(vsync) = requests.vsync {
            session.display.set_vsync(vsync && self.options.vsync);
        }
        let Some(frame) = session.display.acquire() else {
            return;
        };
        let output = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = session.display.ui_view(&frame);
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
                let gpu: Option<f32> = report.gpu_ms.as_ref().map(|g| g.iter().sum());
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
        if let Some(session) = &mut self.session {
            session.game.window_event(self.window.as_deref(), &event);
        }
    }
}
