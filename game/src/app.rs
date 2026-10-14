//! The window, its event loop and the spectator controls.
//!
//! Controls follow the old game's play camera (arrow keys and screen-edge scrolling at
//! 28 units/s, wheel and PageUp/PageDown zoom with its momentum, middle-drag grab pan);
//! as a spectator the view may also turn (Q/E) and zoom further out.

use crate::{
    gpu::Display,
    options::Options,
    paths::Paths,
    telemetry, view,
    world::{self, Loaded},
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
    stats: (u32, f32, f32, Instant),
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
            stats: (0, 0.0, 0.0, now),
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
                    map,
                    self.options.hour,
                    self.options.biome.as_deref(),
                )?;
                let environment = loaded.environment.clone();
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
        renderer.set_quality(&display.gpu, self.options.quality);
        let camera = match &loaded {
            Some(loaded) => {
                renderer.load_world(&display.gpu, &loaded.pack, &loaded.scene.world)?;
                Some(view::initial(self.options.camera, &loaded.scene))
            }
            None => None,
        };
        window.request_redraw();
        self.window = Some(window);
        self.session = Some(Session {
            display,
            renderer,
            loaded,
            camera,
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
        self.stats.0 += 1;
        self.stats.1 += report.cpu_total_ms;
        self.stats.2 += dt_ms;
        if (now - self.stats.3).as_secs_f32() >= 2.0 {
            let frames = self.stats.0.max(1) as f32;
            let gpu: Option<f32> = report.gpu_ms.as_ref().map(|g| g.iter().sum());
            tracing::info!(
                fps = (1000.0 * frames / self.stats.2.max(1.0)).round(),
                cpu_ms = self.stats.1 / frames,
                gpu_ms = gpu,
                draws = report.draws,
                terrain_triangles = report.terrain_triangles,
                "frame"
            );
            self.stats = (0, 0.0, 0.0, now);
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
