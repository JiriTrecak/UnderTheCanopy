//! The window and its event loop.
//!
//! `--map <id>` plays a skirmish (you on the first start against computer players on
//! the others, `--players N` in all); `--spectate <id>` watches computer players on
//! every start. Either runs as a [`play::run::InMatch`]: the simulation thread, the live
//! scene, the camera (arrow keys and screen-edge scrolling at 28 units/s, wheel and
//! PageUp/PageDown zoom with the old momentum, middle-drag grab pan; spectators also
//! turn with Q/E, zoom further out and jump to the largest fight with B), input, the
//! HUD and the debug overlay (F9). Without a map the window shows an empty biome.

use crate::{
    gpu::Display,
    options::Options,
    paths::Paths,
    play::{
        Event,
        run::{InMatch, Start},
    },
    telemetry,
    world::{self, MatchSpec},
};
use anyhow::Result;
use canopy_render::Renderer;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

struct Session {
    display: Display,
    renderer: Renderer,
    game: Option<InMatch>,
    started: std::time::Instant,
}

pub struct App {
    options: Options,
    paths: Paths,
    window: Option<Arc<Window>>,
    session: Option<Session>,
    error: Option<anyhow::Error>,
    stopped_logged: bool,
}

impl App {
    pub fn new(options: Options, paths: Paths) -> Self {
        Self {
            options,
            paths,
            window: None,
            session: None,
            error: None,
            stopped_logged: false,
        }
    }

    /// The error that stopped the game, if any.
    pub fn finish(self) -> Result<()> {
        self.error.map_or(Ok(()), Err)
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attributes = Window::default_attributes()
            .with_title("Under the Canopy")
            .with_inner_size(LogicalSize::new(1600, 900))
            .with_min_inner_size(LogicalSize::new(1024, 600));
        let window = Arc::new(event_loop.create_window(attributes)?);
        let display = Display::new(window.clone(), self.options.vsync)?;
        let me = (!self.options.spectate && self.options.crowd.is_none()).then_some(0);
        let loaded = match &self.options.map {
            Some(map) => Some(world::load(
                &self.paths,
                &MatchSpec {
                    map,
                    hour: self.options.hour,
                    players: self.options.players,
                    crowd: self.options.crowd,
                    colors: self.options.colors.clone(),
                    biome: self.options.biome.as_deref(),
                    human: me,
                },
            )?),
            None => None,
        };
        let environment = match &loaded {
            Some((l, _)) => l.environment.clone(),
            None => {
                let look =
                    canopy_scene::Look::load(&self.paths.data, canopy_content::map::BIOMES[0])?;
                canopy_render::Environment::new(&look.lighting, &look.biome, 12.0, None)
            }
        };
        let mut renderer =
            Renderer::new(&display.gpu, display.format(), display.size(), environment);
        renderer.set_quality(&display.gpu, self.options.quality);
        let game = match loaded {
            Some((loaded, game)) => {
                renderer.load_world(&display.gpu, &loaded.pack, &loaded.scene.world)?;
                let start = Start {
                    me,
                    tick: self.options.tick,
                    camera: self.options.camera.and_then(|c| c.pose()),
                };
                Some(InMatch::start(
                    &window,
                    &display,
                    &mut renderer,
                    loaded,
                    game,
                    start,
                    &self.paths.data,
                )?)
            }
            None => None,
        };
        window.request_redraw();
        self.window = Some(window);
        self.session = Some(Session {
            display,
            renderer,
            game,
            started: std::time::Instant::now(),
        });
        Ok(())
    }

    fn redraw(&mut self) {
        let (Some(session), Some(window)) = (&mut self.session, &self.window) else {
            return;
        };
        match &mut session.game {
            Some(game) => {
                for event in game.frame(window, &mut session.display, &mut session.renderer) {
                    match event {
                        // The game menu belongs to the screens (pause, settings, exit).
                        Event::OpenMenu => tracing::info!("game menu requested"),
                        Event::Chat(text) => tracing::info!(%text, "chat"),
                        Event::LookAt(_) => {}
                    }
                }
                if game.stopped() && !self.stopped_logged {
                    tracing::error!("the simulation thread stopped");
                    self.stopped_logged = true;
                }
            }
            None => {
                let Some(frame) = session.display.acquire() else {
                    return;
                };
                let output = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let size = session.display.size();
                session.renderer.resize(&session.display.gpu, size);
                let aspect = size[0] as f32 / size[1].max(1) as f32;
                let camera = canopy_render::RtsCamera::new([0.0, 0.0], None)
                    .view(aspect, &canopy_render::camera::FlatGround);
                session.renderer.render(
                    &session.display.gpu,
                    &camera,
                    session.started.elapsed().as_secs_f32(),
                    &output,
                    None,
                );
                session.display.present(frame);
            }
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
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                if let Some(session) = &mut self.session {
                    session.display.resize(*size);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                telemetry::frame_mark();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return;
            }
            _ => {}
        }
        if let (Some(session), Some(window)) = (&mut self.session, &self.window)
            && let Some(game) = &mut session.game
        {
            game.window_event(window, &event);
        }
    }
}
