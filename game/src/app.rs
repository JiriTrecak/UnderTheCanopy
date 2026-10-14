//! The window, its event loop and the spectator controls.
//!
//! Controls follow the old game's play camera (arrow keys and screen-edge scrolling at
//! 28 units/s, wheel and PageUp/PageDown zoom with its momentum, middle-drag grab pan);
//! as a spectator the view may also turn (Q/E), zoom further out and jump to the largest
//! fight (B). `--spectate` runs the match on the simulation thread ([`crate::sim`]); the
//! frame interpolates its two latest ticks ([`crate::live`]).

use crate::{
    gpu::Display,
    live::LiveScene,
    options::Options,
    paths::Paths,
    sim::{self, SimThread},
    telemetry, view,
    world::{self, Loaded, MatchSpec},
};
use anyhow::Result;
use canopy_render::{
    Renderer, RtsCamera,
    camera::{EDGE_SCROLL_PIXELS, MAX_STEP_MS},
};
use std::{collections::BTreeSet, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

/// Radians per second the spectator turns with Q and E.
const TURN_SPEED: f32 = 1.2;
/// PageUp/PageDown wheel pixels per millisecond (old `camera.zoomIn`).
const KEY_ZOOM: f32 = 0.7;

struct Session {
    display: Display,
    renderer: Renderer,
    loaded: Option<Loaded>,
    camera: Option<RtsCamera>,
    live: LiveScene,
    sim: Option<SimThread>,
}

/// Measurements gathered between two log lines.
#[derive(Default)]
struct Stats {
    frames: u32,
    cpu_ms: f32,
    cpu_max: f32,
    dt_ms: f32,
    dt_max: f32,
    held: u32,
}

pub struct App {
    options: Options,
    paths: Paths,
    window: Option<Arc<Window>>,
    session: Option<Session>,
    error: Option<anyhow::Error>,
    keys: BTreeSet<KeyCode>,
    cursor: Option<PhysicalPosition<f64>>,
    drag: Option<PhysicalPosition<f64>>,
    focused: bool,
    started: Instant,
    last_frame: Instant,
    stats: Stats,
    stats_since: Instant,
}

impl App {
    pub fn new(options: Options, paths: Paths) -> Self {
        let now = Instant::now();
        Self {
            options,
            paths,
            window: None,
            session: None,
            error: None,
            keys: BTreeSet::new(),
            cursor: None,
            drag: None,
            focused: true,
            started: now,
            last_frame: now,
            stats: Stats::default(),
            stats_since: now,
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
        let (loaded, environment) = match &self.options.map {
            Some(map) => {
                let loaded = world::load(
                    &self.paths,
                    &MatchSpec {
                        map,
                        hour: self.options.hour,
                        players: self.options.players,
                    },
                )?;
                let environment = loaded.0.environment.clone();
                (Some(loaded), environment)
            }
            None => {
                let look =
                    canopy_scene::Look::load(&self.paths.data, canopy_content::map::BIOMES[0])?;
                let environment =
                    canopy_render::Environment::new(&look.lighting, &look.biome, 12.0, None);
                (None, environment)
            }
        };
        let mut renderer =
            Renderer::new(&display.gpu, display.format(), display.size(), environment);
        let mut live = LiveScene::default();
        let mut sim_thread = None;
        let (loaded, camera) = match loaded {
            Some((loaded, mut game)) => {
                renderer.load_world(&display.gpu, &loaded.pack, &loaded.scene.world)?;
                let camera =
                    view::initial(self.options.camera.and_then(|c| c.pose()), &loaded.scene);
                if self.options.spectate {
                    sim_thread = Some(SimThread::spawn(
                        game.world,
                        game.session,
                        game.presenter,
                        sim::TICK,
                        self.options.tick,
                    ));
                } else {
                    // The match's start, still.
                    let start = sim::present(&game.world, &mut game.presenter);
                    live.show(&mut renderer, &display.gpu, &loaded.pack, None, &start, 1.0);
                }
                (Some(loaded), Some(camera))
            }
            None => (None, None),
        };
        window.request_redraw();
        self.window = Some(window);
        self.session = Some(Session {
            display,
            renderer,
            loaded,
            camera,
            live,
            sim: sim_thread,
        });
        Ok(())
    }

    fn update_camera(&mut self, dt_ms: f32) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(camera) = &mut session.camera else {
            return;
        };
        let pressed = |k| self.keys.contains(&k);
        let mut dir = [0.0f32, 0.0];
        if pressed(KeyCode::ArrowLeft) {
            dir[0] -= 1.0;
        }
        if pressed(KeyCode::ArrowRight) {
            dir[0] += 1.0;
        }
        if pressed(KeyCode::ArrowUp) {
            dir[1] += 1.0;
        }
        if pressed(KeyCode::ArrowDown) {
            dir[1] -= 1.0;
        }
        if let (Some(cursor), Some(window), true, None) =
            (self.cursor, &self.window, self.focused, self.drag)
        {
            let size = window.inner_size();
            let edge = f64::from(EDGE_SCROLL_PIXELS) * window.scale_factor();
            if cursor.x <= edge {
                dir[0] -= 1.0;
            } else if cursor.x >= f64::from(size.width) - edge {
                dir[0] += 1.0;
            }
            if cursor.y <= edge {
                dir[1] += 1.0;
            } else if cursor.y >= f64::from(size.height) - edge {
                dir[1] -= 1.0;
            }
        }
        let dir = [dir[0].clamp(-1.0, 1.0), dir[1].clamp(-1.0, 1.0)];
        camera.pan_for(dir, dt_ms);
        let step = dt_ms.min(MAX_STEP_MS);
        if pressed(KeyCode::KeyQ) {
            camera.yaw += TURN_SPEED * step / 1000.0;
        }
        if pressed(KeyCode::KeyE) {
            camera.yaw -= TURN_SPEED * step / 1000.0;
        }
        if pressed(KeyCode::PageUp) {
            camera.wheel(-step * KEY_ZOOM);
        }
        if pressed(KeyCode::PageDown) {
            camera.wheel(step * KEY_ZOOM);
        }
        camera.update(dt_ms);
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let dt_ms = (now - self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        self.update_camera(dt_ms);
        let seconds = (now - self.started).as_secs_f32();
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(frame) = session.display.acquire() else {
            return;
        };
        let output = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let size = session.display.size();
        session.renderer.resize(&session.display.gpu, size);
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        if let (Some(sim), Some(loaded)) = (&session.sim, &session.loaded) {
            session.live.poll(sim.mailbox());
            session.live.frame(
                &mut session.renderer,
                &session.display.gpu,
                &loaded.pack,
                now,
            );
        }
        let camera_view = match (&mut session.camera, &session.loaded) {
            (Some(camera), Some(loaded)) => view::frame(camera, aspect, &loaded.scene),
            _ => canopy_render::RtsCamera::new([0.0, 0.0], None)
                .view(aspect, &canopy_render::camera::FlatGround),
        };
        let report =
            session
                .renderer
                .render(&session.display.gpu, &camera_view, seconds, &output, None);
        session.display.present(frame);
        // A short log line every two seconds.
        let st = &mut self.stats;
        st.frames += 1;
        st.cpu_ms += report.cpu_total_ms;
        st.cpu_max = st.cpu_max.max(report.cpu_total_ms);
        st.dt_ms += dt_ms;
        st.dt_max = st.dt_max.max(dt_ms);
        if (now - self.stats_since).as_secs_f32() >= 2.0 {
            let frames = st.frames.max(1) as f32;
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
                shadow = a.shadow_casters,
                joints = a.joints,
                "frame"
            );
            if let Some(newest) = session.live.newest() {
                let snapshots = session.live.snapshots();
                let n = snapshots.len().max(1) as f32;
                let mean = |f: fn(&sim::TickCost) -> f32| {
                    snapshots.iter().map(|s| f(&s.cost)).sum::<f32>() / n
                };
                let c = newest.presentation.counts;
                tracing::info!(
                    tick = newest.tick(),
                    shown = session.live.shown,
                    step_ms = mean(|c| c.step),
                    view_ms = mean(|c| c.view),
                    present_ms = mean(|c| c.present),
                    late_frames = session.live.held - st.held,
                    slowdowns = newest.slowdowns,
                    units = c.units,
                    buildings = c.buildings,
                    corpses = c.corpses,
                    missiles = c.missiles + c.shells,
                    decided = newest.decided,
                    "simulation"
                );
                st.held = session.live.held;
            }
            if session.sim.as_ref().is_some_and(SimThread::finished) {
                tracing::error!("the simulation thread stopped");
            }
            let held = st.held;
            *st = Stats {
                held,
                ..Stats::default()
            };
            self.stats_since = now;
        }
    }

    /// Jumps the camera to the largest fight of the newest tick.
    fn look_at_battle(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let battle = session.live.newest().and_then(|s| s.presentation.battle);
        if let (Some([x, z]), Some(camera)) = (battle, &mut session.camera) {
            camera.look_at(x, z);
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
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(session) = &mut self.session {
                    session.display.resize(size);
                }
            }
            WindowEvent::Focused(focused) => self.focused = focused,
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            self.keys.insert(code);
                            if code == KeyCode::Escape {
                                event_loop.exit();
                            }
                            if code == KeyCode::KeyB {
                                self.look_at_battle();
                            }
                        }
                        ElementState::Released => {
                            self.keys.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let (Some(from), Some(session)) = (self.drag, &mut self.session)
                    && let (Some(camera), Some(window)) = (&mut session.camera, &self.window)
                {
                    // Grab panning: the ground under the cursor follows it.
                    let size = window.inner_size();
                    let aspect = size.width as f32 / size.height.max(1) as f32;
                    let units_per_pixel = camera
                        .footprint(aspect, 0.0)
                        .map_or(0.1, |[x0, x1, ..]| (x1 - x0) / size.width.max(1) as f32);
                    let dx = (position.x - from.x) as f32 * units_per_pixel;
                    let dy = (position.y - from.y) as f32 * units_per_pixel;
                    camera.pan(-dx, dy);
                    self.drag = Some(position);
                }
                self.cursor = Some(position);
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Middle,
                ..
            } => {
                self.drag = (state == ElementState::Pressed)
                    .then_some(self.cursor)
                    .flatten();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(camera) = self.session.as_mut().and_then(|s| s.camera.as_mut()) {
                    let pixels = match delta {
                        MouseScrollDelta::LineDelta(_, y) => -y * 16.0 * 6.25,
                        MouseScrollDelta::PixelDelta(p) => -p.y as f32,
                    };
                    camera.wheel(pixels);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                telemetry::frame_mark();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
