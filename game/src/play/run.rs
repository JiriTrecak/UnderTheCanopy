//! A running match in the window: the simulation thread, the live scene, the camera,
//! the in-match controller, the HUD, the debug overlay and effect sounds, behind a small
//! API for the screens around it:
//!
//! - [`InMatch::start`] from a loaded world and its session,
//! - [`InMatch::window_event`] for every window event,
//! - [`InMatch::frame`] once per redraw: updates, renders, presents and returns
//!   [`Event`]s ("open the game menu").

use super::{
    Event, Play,
    debug::{Debug, Stats as DebugStats},
    hud::Hud,
    input::{self, Input},
    know::Knowledge,
    overlay,
    picking::{Body, Screen},
};
use crate::{
    gpu::Display,
    live::LiveScene,
    sim::{self, Seat, SimThread},
    view,
    world::{Loaded, Match},
};
use anyhow::Result;
use canopy_render::{
    Renderer, RtsCamera,
    camera::{EDGE_SCROLL_PIXELS, MAX_STEP_MS},
};
use canopy_sim::catalog::Catalog;
use canopy_ui::{FontSources, Ui, UiConfig};
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

/// PageUp/PageDown wheel pixels per millisecond (old `camera.zoomIn`).
const KEY_ZOOM: f32 = 0.7;
/// Radians per second a spectator turns with Q and E.
const TURN_SPEED: f32 = 1.2;

/// How a match starts.
pub struct Start {
    /// The local player, or `None` to spectate.
    pub me: Option<u8>,
    /// Fast-forward to this tick first.
    pub tick: u32,
    pub camera: Option<crate::options::CameraSpec>,
}

pub struct InMatch {
    loaded: Loaded,
    sim: SimThread,
    live: LiveScene,
    camera: RtsCamera,
    pub play: Play,
    hud: Hud,
    ui: Ui,
    debug: Debug,
    audio: canopy_audio::Audio,
    content: Arc<canopy_content::Content>,
    catalog: Arc<Catalog>,
    me: Option<u8>,
    started: Instant,
    last_frame: Instant,
    keys: BTreeSet<KeyCode>,
    images: BTreeSet<String>,
    cursor: Option<(f32, f32)>,
    drag: Option<(f32, f32)>,
    focused: bool,
    last_tick: Option<u32>,
    stats: DebugStats,
    frames: u32,
    frame_sum: f32,
    frame_max: f32,
    stats_since: Instant,
    held_before: u32,
}

impl InMatch {
    /// Starts `game` (moves it to the simulation thread) over the loaded scene, which
    /// the renderer has already loaded.
    pub fn start(
        window: &Window,
        display: &Display,
        renderer: &mut Renderer,
        loaded: Loaded,
        game: Match,
        start: Start,
        data: &std::path::Path,
    ) -> Result<Self> {
        let content = game.world.catalog().content.clone();
        let catalog = Arc::new(Catalog::new(content.clone()));
        let terrain = game.world.terrain().clone();
        let seat = start.me.map_or(Seat::Observer, Seat::Player);
        let view0 = Arc::new(seat.view(&game.world));
        let commands = game.session.commands();
        let mut camera = view::initial(start.camera, &loaded.scene);
        if start.me.is_some() {
            // Players keep the game's zoom range.
            camera.zoom_range = canopy_render::camera::game_zoom();
            if start.camera.is_none()
                && let Some(&focus) = start
                    .me
                    .and_then(|p| loaded.scene.starts.get(usize::from(p)))
            {
                camera.look_at(focus[0], focus[1]);
            }
        }
        let sim = SimThread::spawn(
            game.world,
            game.session,
            game.presenter,
            sim::TICK,
            start.tick,
            seat,
        );
        let know = Knowledge::new(content.clone(), catalog.clone(), view0, start.me);
        let play = Play::new(know, terrain, Box::new(commands));
        let config = UiConfig::load(&data.join("config/ui"))?;
        let mut ui = Ui::new(
            config,
            FontSources {
                bundled: Vec::new(),
                system: true,
            },
        )?;
        let mut hud = Hud::new();
        hud.map_size = loaded.scene.size as f32;
        hud.palette = loaded
            .scene
            .colors
            .iter()
            .map(|c| c.map(|v| (srgb(v) * 255.0).round() as u8))
            .collect();
        hud.attach(&mut ui);
        let ui_layer = renderer.ui_mut(&display.gpu);
        for (name, asset) in ui
            .theme_images()
            .map(|(n, a)| (n.to_owned(), a.to_owned()))
            .collect::<Vec<_>>()
        {
            match ui_layer.load_pack_image(&display.gpu, &loaded.pack, &asset) {
                Ok(desc) => {
                    ui.register_image(&name, desc);
                }
                Err(e) => tracing::warn!(image = %asset, error = %e, "ui image not in the pack"),
            }
        }
        let format = renderer.ui_mut(&display.gpu).format();
        let mut debug = Debug::new(window, &display.gpu.device, format);
        // Opens the overlay at start (smoke tests of the window path).
        debug.open = std::env::var_os("CANOPY_DEBUG_OVERLAY").is_some();
        let mut audio = canopy_audio::Audio::new(canopy_audio::AudioSettings::default());
        if let Err(e) = audio.load(&loaded.pack) {
            tracing::warn!(error = %e, "effect sounds not loaded");
        }
        audio.bind(&content);
        let now = Instant::now();
        Ok(Self {
            loaded,
            sim,
            live: LiveScene::default(),
            camera,
            play,
            hud,
            ui,
            debug,
            audio,
            content,
            catalog,
            me: start.me,
            started: now,
            last_frame: now,
            keys: BTreeSet::new(),
            images: BTreeSet::new(),
            cursor: None,
            drag: None,
            focused: true,
            last_tick: None,
            stats: DebugStats::default(),
            frames: 0,
            frame_sum: 0.0,
            frame_max: 0.0,
            stats_since: now,
            held_before: 0,
        })
    }

    /// Plugs a render target another system draws into the HUD: `"@minimap"` or
    /// `"@portrait"` (the image names `hud.json` shows), `id` a texture id below
    /// [`canopy_render::ui::FIRST_PACK_TEXTURE`], `size` its pixel size. The HUD shows
    /// it from the next frame.
    #[allow(dead_code, reason = "the minimap and portrait renderers plug in here")]
    pub fn plug_render_target(
        &mut self,
        gpu: &canopy_render::Gpu,
        renderer: &mut Renderer,
        name: &str,
        id: canopy_ui::TextureId,
        view: &wgpu::TextureView,
        size: [u32; 2],
    ) {
        renderer.ui_mut(gpu).set_texture(gpu, id, view);
        let mut desc = canopy_ui::ImageDesc::new(id, size[0] as f32, size[1] as f32);
        desc.premultiplied = false;
        self.ui.register_image(name, desc);
        match name {
            "@minimap" => self.hud.minimap = true,
            "@portrait" => self.hud.live_portrait = true,
            _ => {}
        }
    }

    /// What the world UI should draw this frame (selection, hover, health bar mode,
    /// order markers, placement ghost and grid, rally point): the input the renderer's
    /// world UI pass reads.
    #[allow(dead_code, reason = "the world UI pass reads it")]
    pub fn world_ui(&self) -> super::WorldUi {
        self.play.world_ui()
    }

    /// Whether the simulation thread has stopped (it only stops when asked, or by
    /// panicking).
    pub fn stopped(&self) -> bool {
        self.sim.finished()
    }

    /// Routes a window event: the debug overlay, then the HUD, then the world.
    pub fn window_event(&mut self, window: &Window, event: &WindowEvent) {
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == ElementState::Pressed
            && !key.repeat
            && let PhysicalKey::Code(code) = key.physical_key
        {
            let ctrl = self.keys.contains(&KeyCode::ControlLeft)
                || self.keys.contains(&KeyCode::ControlRight);
            if code == KeyCode::F9 || (code == KeyCode::F3 && ctrl) {
                self.debug.open = !self.debug.open;
                return;
            }
            // Spectators jump to the largest fight.
            if code == KeyCode::KeyB
                && self.me.is_none()
                && !self.ui.wants_keyboard()
                && let Some([x, z]) = self.live.newest().and_then(|s| s.presentation.battle)
            {
                self.camera.look_at(x, z);
            }
        }
        if self.debug.event(window, event) {
            return;
        }
        let mut consumed = false;
        for e in canopy_ui::winit::translate(event) {
            consumed |= self.ui.input(&e);
        }
        // The world keeps track of keys and modifiers even when the UI took the event.
        match event {
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let PhysicalKey::Code(code) = key.physical_key {
                    match key.state {
                        ElementState::Pressed => {
                            self.keys.insert(code);
                        }
                        ElementState::Released => {
                            self.keys.remove(&code);
                        }
                    }
                }
                if self.ui.wants_keyboard() {
                    return;
                }
            }
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                if !focused {
                    self.keys.clear();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32, position.y as f32);
                if let Some(from) = self.drag {
                    // Grab panning: the ground under the cursor follows it.
                    let size = window.inner_size();
                    let aspect = size.width as f32 / size.height.max(1) as f32;
                    let per_pixel = self
                        .camera
                        .footprint(aspect, 0.0)
                        .map_or(0.1, |[x0, x1, ..]| (x1 - x0) / size.width.max(1) as f32);
                    self.camera
                        .pan(-(p.0 - from.0) * per_pixel, (p.1 - from.1) * per_pixel);
                    self.drag = Some(p);
                }
                self.cursor = Some(p);
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Middle,
                ..
            } => {
                self.drag = (*state == ElementState::Pressed)
                    .then_some(self.cursor)
                    .flatten();
            }
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                let pixels = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 16.0 * 6.25,
                    MouseScrollDelta::PixelDelta(p) => -p.y as f32,
                };
                self.camera.wheel(pixels);
            }
            _ => {}
        }
        let Some(input) = input::translate(event) else {
            return;
        };
        let terrain = self.loaded.scene.terrain();
        match input {
            // The world follows the pointer everywhere (a box drag may cross the HUD) but
            // hovers nothing under the HUD.
            Input::PointerMoved(_) => {
                self.play.set_over_ui(consumed);
                self.play.input(&input, terrain);
                return;
            }
            // A release always reaches the world: it ends a press that began there.
            Input::Button { pressed: false, .. } => {
                self.play.input(&input, terrain);
                return;
            }
            Input::Button { .. } if consumed => return,
            _ => {}
        }
        if consumed && !matches!(input, Input::Modifiers(_) | Input::FocusLost) {
            return;
        }
        self.play.input(&input, self.loaded.scene.terrain());
    }

    fn update_camera(&mut self, window: &Window, dt_ms: f32) {
        let pressed = |k| self.keys.contains(&k);
        let typing = self.ui.wants_keyboard();
        let mut dir = [0.0f32, 0.0];
        if !typing {
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
        }
        if let (Some((x, y)), true, None, false) = (self.cursor, self.focused, self.drag, typing) {
            let size = window.inner_size();
            let edge = EDGE_SCROLL_PIXELS * window.scale_factor() as f32;
            if x <= edge {
                dir[0] -= 1.0;
            } else if x >= size.width as f32 - edge {
                dir[0] += 1.0;
            }
            if y <= edge {
                dir[1] += 1.0;
            } else if y >= size.height as f32 - edge {
                dir[1] -= 1.0;
            }
        }
        let dir = [dir[0].clamp(-1.0, 1.0), dir[1].clamp(-1.0, 1.0)];
        self.camera.pan_for(dir, dt_ms);
        let step = dt_ms.min(MAX_STEP_MS);
        if self.me.is_none() && !typing {
            // Spectators may turn the view.
            if pressed(KeyCode::KeyQ) {
                self.camera.yaw += TURN_SPEED * step / 1000.0;
            }
            if pressed(KeyCode::KeyE) {
                self.camera.yaw -= TURN_SPEED * step / 1000.0;
            }
        }
        if !typing && pressed(KeyCode::PageUp) {
            self.camera.wheel(-step * KEY_ZOOM);
        }
        if !typing && pressed(KeyCode::PageDown) {
            self.camera.wheel(step * KEY_ZOOM);
        }
        self.camera.update(dt_ms);
    }

    /// Updates, renders and presents one frame.
    pub fn frame(
        &mut self,
        window: &Window,
        display: &mut Display,
        renderer: &mut Renderer,
    ) -> Vec<Event> {
        let now = Instant::now();
        let dt_ms = (now - self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        let since_start = now - self.started;
        self.update_camera(window, dt_ms);
        let Some(frame) = display.acquire() else {
            return Vec::new();
        };
        let output = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(display.format().remove_srgb_suffix()),
            ..Default::default()
        });
        let size = display.size();
        let gpu = &display.gpu;
        renderer.resize(gpu, size);
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let scale = window.scale_factor() as f32;

        // The newest view and the interpolated scene.
        let live_started = Instant::now();
        self.live.poll(self.sim.mailbox());
        self.live.frame(renderer, gpu, &self.loaded.pack, now);
        let newest = self.live.newest().cloned();
        if let Some(s) = &newest {
            // Effect sounds hear every tick once, in order.
            for snap in self.live.snapshots() {
                if self.last_tick.is_none_or(|t| snap.tick() > t) {
                    self.audio.observe(&snap.view);
                }
            }
            self.last_tick = Some(s.tick());
            self.hud.colonies = s.colonies.clone();
        }
        let live_ms = live_started.elapsed().as_secs_f32() * 1000.0;
        let camera_view = view::frame(&mut self.camera, aspect, &self.loaded.scene);
        let screen = Screen {
            camera: camera_view,
            size: [size[0] as f32, size[1] as f32],
            scale,
        };
        let terrain = self.loaded.scene.terrain();
        if let Some(s) = &newest {
            let mut know = Knowledge::new(
                self.content.clone(),
                self.catalog.clone(),
                s.view.clone(),
                self.me,
            );
            know.held = s.held.clone();
            let bodies: Vec<Body> = self
                .live
                .bodies()
                .iter()
                .map(|&(id, [radius, height], foot)| Body {
                    id,
                    foot,
                    height,
                    radius: (radius - 0.32).max(0.2),
                    unit: know.entity(id).is_some_and(|e| know.is_unit(e)),
                })
                .collect();
            self.play.update(know, screen, bodies, terrain, since_start);
        }
        // The placement ghost joins the live models.
        let world_ui = self.play.world_ui();
        if let (Some(ghost), Some(def), Some(s)) =
            (&world_ui.ghost, &world_ui.ghost_definition, &newest)
        {
            let mut instances = self.live.instances().to_vec();
            if let Some(instance) = ghost_instance(
                &self.content,
                &s.presentation,
                ghost,
                def,
                terrain,
                self.me,
                &self.loaded.scene.colors,
            ) {
                instances.push(instance);
            }
            renderer.set_actors(&instances);
        }
        let focus = [camera_view.target.x, camera_view.target.z];
        self.live.feed_ripples(
            renderer,
            &self.loaded.scene,
            since_start.as_secs_f32(),
            focus,
        );

        // The HUD.
        let ui_started = Instant::now();
        self.ui.set_viewport(size[0], size[1], scale);
        self.hud.update(&mut self.ui, &self.play);
        #[cfg(debug_assertions)]
        if let Ok(true) = self.ui.reload_if_changed() {
            self.hud.attach(&mut self.ui);
        }
        let mut list = self.ui.frame(since_start.as_secs_f64()).clone();
        let bodies: Vec<Body> = self
            .live
            .bodies()
            .iter()
            .map(|&(id, [radius, height], foot)| Body {
                id,
                foot,
                height,
                radius: (radius - 0.32).max(0.2),
                unit: true,
            })
            .collect();
        overlay::draw(
            &mut list,
            &screen,
            &world_ui,
            self.play.knowledge(),
            &bodies,
            terrain,
        );
        for name in self.hud.take_image_requests() {
            // Each image is uploaded and registered once (missing ones are not retried).
            if !self.images.insert(name.clone()) {
                continue;
            }
            let layer = renderer.ui_mut(gpu);
            match layer.load_pack_image(gpu, &self.loaded.pack, &name) {
                Ok(desc) => {
                    self.ui.register_image(&name, desc);
                }
                Err(e) => tracing::debug!(image = %name, error = %e, "hud image not in the pack"),
            }
        }
        let layer = renderer.ui_mut(gpu);
        layer.submit(gpu, &list);
        layer.set_target(Some(ui_view.clone()));
        let actions = self.ui.take_actions();
        let mut events = self.hud.act(&mut self.ui, &mut self.play, actions);
        let ui_ms = ui_started.elapsed().as_secs_f32() * 1000.0;

        // Debug overlay.
        self.stats.live_ms = live_ms;
        self.stats.ui_ms = ui_ms;
        self.stats.selected = self.play.selection.ids().len();
        if let Some(s) = &newest {
            let n = self.live.snapshots().len().max(1) as f32;
            let mean = |f: fn(&sim::TickCost) -> f32| {
                self.live
                    .snapshots()
                    .iter()
                    .map(|s| f(&s.cost))
                    .sum::<f32>()
                    / n
            };
            self.stats.tick = s.tick();
            self.stats.tick_cost = [mean(|c| c.step), mean(|c| c.view), mean(|c| c.present)];
            self.stats.slowdowns = s.slowdowns;
            self.stats.decided = s.decided;
            self.stats.late_frames = self.live.held;
            self.stats.units = s.presentation.counts.units;
            self.stats.buildings = s.presentation.counts.buildings;
            self.stats.ai =
                s.ai.clone()
                    .or_else(|| (!s.view.ai.is_empty()).then(|| Arc::new(s.view.ai.clone())));
        }
        self.stats.sent = self
            .play
            .sent
            .iter()
            .map(|(t, a)| format!("{:.1}s {a:?}", t.as_secs_f32()))
            .collect();
        let (quality, wants_ai) = self.debug.run(window, gpu, &self.stats, renderer.quality());
        if let Some(q) = quality {
            renderer.set_quality(gpu, q);
        }
        self.sim.want_ai_summaries(wants_ai);

        let debug = &self.debug;
        let paint = |encoder: &mut wgpu::CommandEncoder| debug.paint(encoder, &ui_view);
        let report = renderer.render(
            gpu,
            &camera_view,
            since_start.as_secs_f32(),
            &output,
            Some(&paint),
        );
        display.present(frame);

        // Sounds.
        let right = (camera_view.view.inverse().x_axis.truncate()).normalize_or_zero();
        let render_tick = self.live.shown;
        self.audio.update(&canopy_audio::Frame {
            real_dt: dt_ms / 1000.0,
            render_tick,
            speed: 1.0,
            listener: canopy_audio::Listener::new(focus[0], focus[1], right.x, right.z),
        });

        // Camera requests and frame statistics.
        for e in self.play.take_events() {
            events.push(e);
        }
        events.retain(|e| match e {
            Event::LookAt([x, z]) => {
                self.camera.look_at(*x, *z);
                false
            }
            _ => true,
        });
        self.frames += 1;
        self.frame_sum += dt_ms;
        self.frame_max = self.frame_max.max(dt_ms);
        self.stats.report = report;
        if (now - self.stats_since) >= Duration::from_millis(500) {
            let frames = self.frames.max(1) as f32;
            self.stats.fps = 1000.0 * frames / self.frame_sum.max(1.0);
            self.stats.frame_ms = self.frame_sum / frames;
            self.stats.frame_max_ms = self.frame_max;
            if (now - self.stats_since) >= Duration::from_secs(2) || self.frames > 0 {
                tracing::debug!(
                    fps = self.stats.fps.round(),
                    frame_max_ms = self.frame_max.round(),
                    late = self.live.held - self.held_before,
                    "frame"
                );
            }
            self.held_before = self.live.held;
            self.frames = 0;
            self.frame_sum = 0.0;
            self.frame_max = 0.0;
            self.stats_since = now;
        }
        events
    }
}

/// Linear → sRGB for a channel.
pub(crate) fn srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The ghost of the building being placed: its model at the snapped site, translucent,
/// in the owner's colour, or tinted toward red where the site looks invalid (old 70 %
/// toward `#f26960` at 0.48 opacity; the tint here reaches the team-coloured parts).
pub(crate) fn ghost_instance(
    content: &canopy_content::Content,
    presentation: &canopy_scene::live::Presentation,
    ghost: &super::placement::Ghost,
    definition: &str,
    terrain: &canopy_render::terrain::TerrainDesc,
    me: Option<u8>,
    colors: &[[f32; 3]],
) -> Option<canopy_render::actors::ActorInstance> {
    use canopy_render::camera::Ground;
    let d = content.definition(definition)?;
    let asset = content.asset(d.asset.as_str())?;
    let file = asset.file.as_deref()?;
    let model = presentation.models.iter().position(|m| m == file)? as u32;
    let scale = (asset.scale.unwrap_or(1.0) * d.model_scale.unwrap_or(1.0)) as f32;
    let [x, z] = ghost.center;
    let y = terrain.height(x, z).max(0.0);
    let rotation = glam::Quat::from_rotation_y((ghost.rotation as f32).to_radians());
    let transform = glam::Mat4::from_scale_rotation_translation(
        glam::Vec3::splat(scale),
        rotation,
        glam::Vec3::new(x, y, z),
    );
    let mut instance = canopy_render::actors::ActorInstance::rest(model, transform);
    instance.opacity = 0.48;
    let own = me
        .and_then(|p| colors.get(usize::from(p)).copied())
        .unwrap_or([1.0; 3]);
    instance.owner_color = Some(if ghost.problem.is_some() {
        let red = [0.887, 0.141, 0.117];
        [0, 1, 2].map(|i| own[i] * 0.3 + red[i] * 0.7)
    } else {
        own
    });
    Some(instance)
}
