//! A running match: the simulation thread, the live scene, the camera, the in-match
//! controller, the HUD, the debug overlay and effect sounds, behind a small API for the
//! screens around it (`screens::in_match::Playing`), which own the window, the UI and
//! the audio:
//!
//! - [`InMatch::start`] from a loaded world and its session,
//! - [`InMatch::window_event`] for every window event (after the UI saw it),
//! - [`InMatch::update`] once per frame before the UI lays out,
//! - [`InMatch::act`] with the HUD's UI actions → [`Event`]s ("open the game menu"),
//! - [`InMatch::decorate`] on the UI's draw list, then [`InMatch::render`].

use super::{
    Event, Play, cinematic,
    debug::{Debug, MatchTools, Stats as DebugStats},
    follow::{self, CameraMode},
    hud::Hud,
    input::{self, Input},
    keys::{Keymap, Shortcut},
    know::Knowledge,
    overlay,
    picking::{Body, Screen, Trees},
};
use crate::{
    live::LiveScene,
    settings::{Chord, keys::key_code},
    sim::{self, Seat, SimThread},
    view,
    world::{Loaded, Match},
};
use anyhow::Result;
use canopy_render::{
    CameraView, FrameReport, Gpu, Renderer, RtsCamera,
    camera::{EDGE_SCROLL_PIXELS, MAX_STEP_MS},
};
use canopy_sim::catalog::Catalog;
use canopy_ui::{DrawList, Ui};
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::Window,
};

/// Seconds per simulation tick (D3).
const TICK_SECONDS: f32 = 0.025;
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
    /// A networked seat reports its identity once the world is built.
    pub ready: Option<sim::Ready>,
    /// Control groups to restore (a loaded save), by slot.
    pub groups: Option<Vec<Vec<u32>>>,
    /// The player's name in the chat.
    pub name: Option<String>,
    /// A chat server echoes the player's lines (networked); otherwise they show at once.
    pub chat_server: bool,
}

/// Player settings the match follows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    pub keymap: Keymap,
    /// Health bars over every unit by default.
    pub always_health_bars: bool,
}

/// What the match borrows from the game around it each frame.
pub struct Host<'a> {
    /// The window (`None` offscreen: no debug overlay).
    pub window: Option<&'a Window>,
    pub gpu: &'a Gpu,
    pub renderer: &'a mut Renderer,
    pub ui: &'a mut Ui,
    pub audio: &'a mut canopy_audio::Audio,
    /// Physical pixels.
    pub size: [u32; 2],
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

pub struct InMatch {
    loaded: Loaded,
    sim: SimThread,
    live: LiveScene,
    camera: RtsCamera,
    pub play: Play,
    hud: Hud,
    debug: Option<Debug>,
    debug_open: bool,
    content: Arc<canopy_content::Content>,
    catalog: Arc<Catalog>,
    me: Option<u8>,
    started: Instant,
    last_frame: Instant,
    keys: BTreeSet<KeyCode>,
    modifiers: ModifiersState,
    images: BTreeSet<String>,
    /// Render targets plugged in, registered with the UI on the next frame.
    targets: Vec<(String, canopy_ui::ImageDesc)>,
    cursor: Option<(f32, f32)>,
    drag: Option<(f32, f32)>,
    focused: bool,
    hud_hidden: bool,
    last_tick: Option<u32>,
    camera_view: Option<CameraView>,
    /// The unit camera's blend between poses.
    follow: follow::Follow,
    /// The RTS focus before a mission cinematic moved the view.
    cinematic_focus: Option<[f32; 2]>,
    screen: Option<Screen>,
    stats: DebugStats,
    frames: u32,
    frame_sum: f32,
    frame_max: f32,
    stats_since: Instant,
    /// When the overlay last sampled the GPU time.
    gpu_sampled: Instant,
    held_before: u32,
    /// The debug overlay's speed, reveal and fog perspective.
    tools: MatchTools,
    /// The cinematic lens and the entities a scene's shot names.
    lens: cinematic::Lens,
    scene_targets: Option<sim::SceneTargets>,
    networked: bool,
}

impl InMatch {
    /// Uploads the scene, starts `game` on the simulation thread and attaches the HUD.
    pub fn start(
        gpu: &Gpu,
        renderer: &mut Renderer,
        ui: &mut Ui,
        audio: &mut canopy_audio::Audio,
        loaded: Loaded,
        game: Match,
        start: Start,
    ) -> Result<Self> {
        renderer.set_environment(gpu, loaded.environment.clone());
        renderer.load_world(gpu, &loaded.pack, &loaded.scene.world)?;
        renderer.set_actors(&[]);
        let loaded_sandbox = loaded.sandbox;
        let content = game.world.catalog().content.clone();
        let catalog = Arc::new(Catalog::new(content.clone()));
        // Names and controllers for the observer panel.
        let players: Vec<(u8, String, bool)> = game
            .world
            .setup()
            .slots
            .iter()
            .map(|s| {
                (
                    s.player,
                    s.name
                        .clone()
                        .unwrap_or_else(|| format!("Player {}", s.player + 1)),
                    s.kind == canopy_sim::SlotKind::Ai,
                )
            })
            .collect();
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
        // Effects, world UI, the minimap's background and the portrait image, before the
        // world moves to the simulation thread and before any live model is drawn.
        let mut live = LiveScene::default();
        live.attach_effects(
            renderer,
            gpu,
            &loaded.pack,
            game.presenter.effects.effect_table().clone(),
        );
        live.attach_world_ui(
            &loaded.data,
            &loaded.pack,
            &content,
            loaded.scene.colors.clone(),
        );
        live.world_ui_input.viewer = start.me;
        renderer.bake_minimap(gpu, loaded.scene.size, MINIMAP_PIXELS);
        renderer.set_portrait(gpu, None, PORTRAIT_PIXELS);
        let sim = SimThread::start(
            game.world,
            game.session,
            game.presenter,
            sim::TICK,
            start.tick,
            seat,
            start.ready,
        );
        let know = Knowledge::new(content.clone(), catalog.clone(), view0, start.me);
        let mut play = Play::new(know, terrain, Box::new(commands));
        play.set_trees(Arc::new(Trees::of_scene(
            &loaded.pack,
            &loaded.scene.world.statics,
            &loaded.scene.tree_slots,
        )));
        play.echo_chat = !start.chat_server;
        play.name = start.name;
        for (slot, ids) in start.groups.iter().flatten().enumerate() {
            play.groups.assign(slot, ids);
        }
        let mut hud = Hud::new();
        hud.map_size = loaded.scene.size as f32;
        hud.players = players;
        hud.palette = loaded
            .scene
            .colors
            .iter()
            .map(|c| c.map(|v| (srgb(v) * 255.0).round() as u8))
            .collect();
        hud.attach(ui);
        // Effect sounds: the pack's sounds once, the content's layers per match.
        if audio.library().is_empty()
            && let Err(e) = audio.load(&loaded.pack)
        {
            tracing::warn!(error = %e, "effect sounds not loaded");
        }
        audio.clear_game();
        audio.bind(&content);
        let now = Instant::now();
        let mut this = Self {
            loaded,
            sim,
            live,
            camera,
            play,
            hud,
            debug: None,
            // Opens the overlay at start (smoke tests of the window path).
            debug_open: std::env::var_os("CANOPY_DEBUG_OVERLAY").is_some(),
            content,
            catalog,
            me: start.me,
            started: now,
            last_frame: now,
            keys: BTreeSet::new(),
            modifiers: ModifiersState::empty(),
            images: BTreeSet::new(),
            targets: Vec::new(),
            cursor: None,
            drag: None,
            focused: true,
            hud_hidden: false,
            last_tick: None,
            camera_view: None,
            follow: follow::Follow::default(),
            cinematic_focus: None,
            screen: None,
            stats: DebugStats::default(),
            frames: 0,
            frame_sum: 0.0,
            frame_max: 0.0,
            stats_since: now,
            gpu_sampled: now,
            held_before: 0,
            tools: MatchTools {
                speed: 1.0,
                reveal: start.me.is_none(),
                perspective: start.me,
            },
            networked: start.chat_server,
            lens: cinematic::Lens::default(),
            scene_targets: None,
        };
        this.stats.networked = start.chat_server;
        this.stats.players = this
            .hud
            .players
            .iter()
            .map(|(slot, name, _)| (*slot, name.clone()))
            .collect();
        // Sandbox maps start revealed for a local player, as in the old game.
        if loaded_sandbox && start.me.is_some() && !start.chat_server {
            this.set_tools(MatchTools {
                reveal: true,
                ..this.tools
            });
        }
        // The HUD shows the minimap and the live portrait.
        let targets = [
            (
                "@minimap",
                MINIMAP_TEXTURE,
                MINIMAP_PIXELS,
                renderer.minimap_view().cloned(),
            ),
            (
                "@portrait",
                PORTRAIT_TEXTURE,
                PORTRAIT_PIXELS,
                renderer.portrait_view().cloned(),
            ),
        ];
        for (name, id, pixels, view) in targets {
            if let Some(view) = view {
                this.plug_render_target(
                    gpu,
                    renderer,
                    name,
                    canopy_ui::TextureId(id),
                    &view,
                    [pixels; 2],
                );
            }
        }
        Ok(this)
    }

    /// Applies the debug overlay's match tools (single player only, as the old ones):
    /// the simulation's pace, and whose view the picture shows. A new perspective clears
    /// the selection (old `onVision`).
    fn set_tools(&mut self, mut tools: MatchTools) {
        if self.networked {
            return;
        }
        if !tools.reveal && tools.perspective.is_none() {
            // An observer leaving the revealed map looks through the first player's fog.
            tools.perspective = self.stats.players.first().map(|&(slot, _)| slot);
        }
        if tools.speed != self.tools.speed {
            self.sim.control(sim::Control::Speed(tools.speed));
        }
        let shown = |t: &MatchTools| {
            if t.reveal {
                Some(Seat::Observer)
            } else {
                t.perspective.map(Seat::Player)
            }
        };
        let (before, after) = (shown(&self.tools), shown(&tools));
        if before != after {
            let own = self.me.map_or(Seat::Observer, Seat::Player);
            self.sim
                .control(sim::Control::Shown(after.filter(|&s| s != own)));
            if tools.perspective != self.tools.perspective {
                self.play.select(Vec::new(), None);
            }
        }
        self.tools = tools;
    }

    /// Follows the player's settings (shortcuts, health bars).
    pub fn set_options(&mut self, options: &Options) {
        self.play.keymap = options.keymap.clone();
        self.play.always_health_bars = options.always_health_bars;
    }

    /// Plugs a render target another system draws into the HUD: `"@minimap"` or
    /// `"@portrait"` (the image names `hud.json` shows), `id` a texture id below
    /// [`canopy_render::ui::FIRST_PACK_TEXTURE`], `size` its pixel size. The HUD shows
    /// it from the next frame.
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
        // The renderer's images are sRGB textures with premultiplied alpha.
        desc.premultiplied = true;
        desc.srgb_view = true;
        self.targets.push((name.to_owned(), desc));
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

    /// The simulation thread (pause, save, chat, notices).
    pub fn sim(&self) -> &SimThread {
        &self.sim
    }

    /// The presented ticks at hand.
    pub fn live(&self) -> &LiveScene {
        &self.live
    }

    pub fn camera(&self) -> &RtsCamera {
        &self.camera
    }

    /// Whether the simulation thread has stopped (it only stops when asked, or by
    /// panicking).
    pub fn stopped(&self) -> bool {
        self.sim.finished()
    }

    /// Releases held keys and drags (an overlay took the input).
    pub fn release_input(&mut self) {
        self.keys.clear();
        self.drag = None;
        self.play
            .input(&Input::FocusLost, self.loaded.scene.terrain());
    }

    /// Routes a window event the UI has seen (`consumed`: it took it): the debug
    /// overlay, then the world.
    pub fn window_event(
        &mut self,
        window: Option<&Window>,
        event: &WindowEvent,
        ui: &Ui,
        consumed: bool,
    ) {
        if let WindowEvent::ModifiersChanged(m) = event {
            self.modifiers = m.state();
        }
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == ElementState::Pressed
            && !key.repeat
            && let PhysicalKey::Code(code) = key.physical_key
            && !ui.wants_keyboard()
        {
            let shortcuts = Chord::of(code, self.modifiers)
                .map(|c| self.play.keymap.pressed(&c))
                .unwrap_or_default();
            if code == KeyCode::F9 || shortcuts.contains(&Shortcut::Debug) {
                self.debug_open = !self.debug_open;
                return;
            }
            // Escape also brings a hidden HUD back (old `enableHudToggle`).
            if shortcuts.contains(&Shortcut::ToggleHud)
                || (self.hud_hidden && code == KeyCode::Escape)
            {
                self.hud_hidden = !self.hud_hidden;
                return;
            }
            // Spectators jump to the largest fight.
            if code == KeyCode::KeyB
                && self.me.is_none()
                && !consumed
                && let Some([x, z]) = self.live.newest().and_then(|s| s.presentation.battle)
            {
                self.camera.look_at(x, z);
            }
        }
        if let (Some(debug), Some(window)) = (&mut self.debug, window)
            && debug.event(window, event)
        {
            return;
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
                if ui.wants_keyboard() {
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
                    // Grab panning from the centre ray (old `panPersp`), clamped at once.
                    let size = self.screen.map_or([1.0, 1.0], |s| s.size);
                    let aspect = size[0] / size[1].max(1.0);
                    let rts = view::frame(&mut self.camera, aspect, &self.loaded.scene);
                    let [mx, mz] = view::grab_pan(&rts, size, p.0 - from.0, p.1 - from.1);
                    self.camera.focus[0] += mx;
                    self.camera.focus[1] += mz;
                    view::frame(&mut self.camera, aspect, &self.loaded.scene);
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
                self.drag = (*state == ElementState::Pressed
                    && !consumed
                    && self.play.camera_mode == CameraMode::Rts)
                    .then_some(self.cursor)
                    .flatten();
            }
            WindowEvent::MouseWheel { delta, .. }
                if !consumed && self.play.camera_mode == CameraMode::Rts =>
            {
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
            Input::Key { pressed: false, .. } => {
                self.play.input(&input, terrain);
                return;
            }
            Input::Button { .. } if consumed => return,
            _ => {}
        }
        if consumed && !matches!(input, Input::Modifiers(_) | Input::FocusLost) {
            return;
        }
        self.play.input(&input, terrain);
    }

    /// A close view: a mission scene's unit shot while a cinematic plays, else the unit
    /// camera on the focused unit; `None` for the RTS view.
    fn close_view(&mut self, aspect: f32, dt: f32) -> Option<CameraView> {
        if self.play.cinematic().is_some() {
            let camera = self.play.scene_camera()?.clone();
            let targets = self.scene_targets?;
            let (live, content, catalog) = (&self.live, &self.content, &self.catalog);
            let subject = |t: &sim::SceneTarget| {
                follow::subject_of(live, content, catalog.get(t.definition), t.id)
            };
            let obstruction =
                |o, d, far| live.camera_obstruction(&self.loaded.obstacles, o, d, far);
            let (key, pose) = cinematic::pose(
                &camera,
                &targets,
                subject,
                self.loaded.scene.terrain(),
                &obstruction,
            )?;
            return Some(self.follow.view(key, &pose, dt, aspect));
        }
        let mode = self.play.camera_mode;
        if mode == CameraMode::Rts {
            return None;
        }
        let id = self.play.selection.focus()?;
        let subject = follow::subject(&self.live, self.play.knowledge(), id)?;
        let (live, obstacles) = (&self.live, &self.loaded.obstacles);
        let obstruction = |o, d, far| live.camera_obstruction(obstacles, o, d, far);
        let pose = follow::ground_pose(&subject, mode, self.loaded.scene.terrain(), &obstruction)?;
        Some(
            self.follow
                .view(follow::Key::unit(mode, id), &pose, dt, aspect),
        )
    }

    /// The unit whose eyes the camera looks through (its body is not drawn).
    fn first_person_body(&self) -> Option<u32> {
        if self.play.cinematic().is_some() {
            let camera = self.play.scene_camera()?;
            return (cinematic::mode(camera) == Some(CameraMode::FirstPerson))
                .then(|| self.scene_targets.and_then(|t| t.subject).map(|s| s.id))
                .flatten();
        }
        (self.play.camera_mode == CameraMode::FirstPerson)
            .then(|| self.play.selection.focus())
            .flatten()
    }

    /// Whether a held key holds `shortcut` (camera keys from the player's bindings).
    fn held(&self, shortcut: Shortcut) -> bool {
        self.play
            .keymap
            .codes(shortcut)
            .filter_map(key_code)
            .any(|k| self.keys.contains(&k))
    }

    fn update_camera(&mut self, size: [u32; 2], scale: f32, dt_ms: f32, free: bool, typing: bool) {
        // A unit camera ignores panning and zoom (old `followingUnit`); so does a
        // cinematic, which looks at its scene and gives the view back afterwards.
        let cinematic = self.play.cinematic();
        self.lens.update(cinematic.is_some(), dt_ms);
        match (cinematic, self.cinematic_focus) {
            (Some(scene), saved) => {
                if saved.is_none() {
                    self.cinematic_focus = Some(self.camera.focus);
                }
                // A unit shot looks through its unit; otherwise the RTS view looks at
                // an RTS shot's unit or the scene.
                let camera = self.play.scene_camera();
                let unit_shot = camera.is_some_and(|c| cinematic::mode(c).is_some());
                if let (Some(scene), false) = (scene, unit_shot) {
                    let [x, z] = cinematic::focus(scene, camera, self.scene_targets.as_ref());
                    self.camera.look_at(x, z);
                }
            }
            (None, Some([x, z])) => {
                self.camera.look_at(x, z);
                self.cinematic_focus = None;
            }
            (None, None) => {}
        }
        let steer =
            free && !typing && cinematic.is_none() && self.play.camera_mode == CameraMode::Rts;
        let mut dir = [0.0f32, 0.0];
        if steer {
            if self.held(Shortcut::PanLeft) {
                dir[0] -= 1.0;
            }
            if self.held(Shortcut::PanRight) {
                dir[0] += 1.0;
            }
            if self.held(Shortcut::PanUp) {
                dir[1] += 1.0;
            }
            if self.held(Shortcut::PanDown) {
                dir[1] -= 1.0;
            }
        }
        if let (Some((x, y)), true, None, true) = (self.cursor, self.focused, self.drag, steer) {
            let edge = EDGE_SCROLL_PIXELS * scale;
            if x <= edge {
                dir[0] -= 1.0;
            } else if x >= size[0] as f32 - edge {
                dir[0] += 1.0;
            }
            if y <= edge {
                dir[1] += 1.0;
            } else if y >= size[1] as f32 - edge {
                dir[1] -= 1.0;
            }
        }
        let dir = [dir[0].clamp(-1.0, 1.0), dir[1].clamp(-1.0, 1.0)];
        self.camera.pan_for(dir, dt_ms);
        let step = dt_ms.min(MAX_STEP_MS);
        if self.me.is_none() && steer {
            // Spectators may turn the view.
            if self.keys.contains(&KeyCode::KeyQ) {
                self.camera.yaw += TURN_SPEED * step / 1000.0;
            }
            if self.keys.contains(&KeyCode::KeyE) {
                self.camera.yaw -= TURN_SPEED * step / 1000.0;
            }
        }
        if steer && self.held(Shortcut::ZoomIn) {
            self.camera.wheel(-step * KEY_ZOOM);
        }
        if steer && self.held(Shortcut::ZoomOut) {
            self.camera.wheel(step * KEY_ZOOM);
        }
        self.camera.update(dt_ms);
    }

    /// Advances the frame up to the HUD's contents: the camera (`free`: no overlay holds
    /// the input), the newest view and the interpolated scene, the controller, the
    /// placement ghost, the HUD and effect sounds. The UI lays out after this.
    pub fn update(&mut self, host: &mut Host<'_>, free: bool) {
        let now = Instant::now();
        let dt_ms = (now - self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        let since_start = now - self.started;
        let size = host.size;
        let scale = host.scale;
        let gpu = host.gpu;
        if self.debug.is_none()
            && let Some(window) = host.window
        {
            let format = host.renderer.ui_mut(gpu).format();
            self.debug = Some(Debug::new(window, &gpu.device, format));
        }
        if let Some(debug) = &mut self.debug {
            debug.open = self.debug_open;
        }
        let typing = host.ui.wants_keyboard();
        self.update_camera(size, scale, dt_ms, free, typing);
        let aspect = size[0] as f32 / size[1].max(1) as f32;

        // The newest view and the interpolated scene.
        let live_started = Instant::now();
        // The first-person camera does not draw its own body (old `hideCameraBody`).
        self.live.hidden_entity = self.first_person_body();
        self.live.poll(self.sim.mailbox());
        self.live.frame(host.renderer, gpu, &self.loaded.pack, now);
        let newest = self.live.newest().cloned();
        if let Some(s) = &newest {
            // Effect sounds hear every tick once, in order.
            for snap in self.live.snapshots() {
                if self.last_tick.is_none_or(|t| snap.tick() > t) {
                    host.audio.observe(&snap.view);
                }
            }
            if self.last_tick != Some(s.tick())
                && let Some(day) = &self.loaded.day
            {
                // The old sky's day cycle, on match time (it stops while paused).
                host.renderer
                    .set_lighting(day.lighting(s.tick() as f32 * TICK_SECONDS));
            }
            self.last_tick = Some(s.tick());
            self.hud.colonies = s.colonies.clone();
            self.scene_targets = s.scene;
        }
        let live_ms = live_started.elapsed().as_secs_f32() * 1000.0;
        let rts_view = self
            .lens
            .apply(view::frame(&mut self.camera, aspect, &self.loaded.scene));
        let close = self.close_view(aspect, dt_ms / 1000.0);
        // Giant canopies open around units in the overhead view only (a close camera
        // would cut holes at eye level).
        host.renderer.set_cutaway(
            close
                .is_none()
                .then(|| self.live.cutaway_subjects())
                .into_iter()
                .flatten(),
        );
        let camera_view = close.unwrap_or_else(|| {
            self.follow.shown(&rts_view);
            rts_view
        });
        let screen = Screen {
            camera: camera_view,
            size: [size[0] as f32, size[1] as f32],
            scale,
        };
        self.camera_view = Some(camera_view);
        self.screen = Some(screen);
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
            host.renderer.set_actors(&instances);
        }
        // The world UI pass, the minimap and the portrait from what play shows.
        self.live.world_ui_input = world_ui_input(&world_ui, scale, self.me);
        let seconds = since_start.as_secs_f32();
        self.live.eye = Some(camera_view.eye);
        self.live.draw_world_ui(
            host.renderer,
            gpu,
            &camera_view,
            &self.loaded.scene,
            seconds,
        );
        self.live.draw_minimap(
            host.renderer,
            &camera_view,
            &self.loaded.scene,
            MINIMAP_PIXELS,
        );
        let subject = world_ui
            .selected
            .first()
            .and_then(|&id| self.live.portrait(id, seconds));
        host.renderer.set_portrait(gpu, subject, PORTRAIT_PIXELS);
        let focus = [camera_view.target.x, camera_view.target.z];
        self.live.feed_ripples(
            host.renderer,
            &self.loaded.scene,
            since_start.as_secs_f32(),
            focus,
        );

        // The HUD.
        let ui_started = Instant::now();
        let ui = &mut *host.ui;
        for (name, desc) in self.targets.drain(..) {
            ui.register_image(&name, desc);
        }
        self.hud.update(ui, &self.play);
        ui.show("hud", !self.hud_hidden);
        for name in self.hud.take_image_requests() {
            // Each image is uploaded and registered once (missing ones are not retried).
            if !self.images.insert(name.clone()) {
                continue;
            }
            let layer = host.renderer.ui_mut(gpu);
            match layer.load_pack_image(gpu, &self.loaded.pack, &name) {
                Ok(desc) => {
                    ui.register_image(&name, desc);
                }
                Err(e) => tracing::debug!(image = %name, error = %e, "hud image not in the pack"),
            }
        }
        self.stats.ui_ms = ui_started.elapsed().as_secs_f32() * 1000.0;

        // Sounds.
        let right = (camera_view.view.inverse().x_axis.truncate()).normalize_or_zero();
        host.audio.update(&canopy_audio::Frame {
            real_dt: dt_ms / 1000.0,
            render_tick: self.live.shown,
            speed: 1.0,
            listener: canopy_audio::Listener::new(focus[0], focus[1], right.x, right.z),
        });

        // Debug statistics.
        self.stats.live_ms = live_ms;
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
        self.frames += 1;
        self.frame_sum += dt_ms;
        self.frame_max = self.frame_max.max(dt_ms);
        if (now - self.stats_since) >= Duration::from_millis(500) {
            let frames = self.frames.max(1) as f32;
            self.stats.fps = 1000.0 * frames / self.frame_sum.max(1.0);
            self.stats.frame_ms = self.frame_sum / frames;
            self.stats.frame_max_ms = self.frame_max;
            tracing::debug!(
                fps = self.stats.fps.round(),
                frame_max_ms = self.frame_max.round(),
                late = self.live.held - self.held_before,
                "frame"
            );
            self.held_before = self.live.held;
            self.frames = 0;
            self.frame_sum = 0.0;
            self.frame_max = 0.0;
            self.stats_since = now;
        }
    }

    /// The HUD's UI actions since the last frame (call every frame: it also closes the
    /// chat when its field lost focus). Camera requests are handled here; the rest is
    /// for the screens.
    pub fn act(&mut self, ui: &mut Ui, actions: Vec<canopy_ui::Action>) -> Vec<Event> {
        let mut events = self.hud.act(ui, &mut self.play, actions);
        events.extend(self.play.take_events());
        events.retain(|e| match e {
            Event::LookAt([x, z]) => {
                // A camera jump returns to the RTS view (old `lookAt` hook).
                self.play.camera_mode = CameraMode::Rts;
                self.camera.look_at(*x, *z);
                false
            }
            Event::ToggleDebug => {
                self.debug_open = !self.debug_open;
                false
            }
            _ => true,
        });
        events
    }

    /// Adds the drag box under the HUD in the UI's draw list (everything else in the
    /// world is the renderer's world UI pass).
    pub fn decorate(&self, list: &mut DrawList) {
        let Some(screen) = &self.screen else {
            return;
        };
        overlay::draw(list, screen.scale, &self.play.world_ui());
    }

    /// Renders the frame into `output` with the UI (already submitted to the renderer's
    /// UI pass) into `ui_view`, a non-sRGB view of it, and the debug overlay on top.
    pub fn render(
        &mut self,
        host: &mut Host<'_>,
        output: &wgpu::TextureView,
        ui_view: &wgpu::TextureView,
    ) -> FrameReport {
        let gpu = host.gpu;
        let aspect = host.size[0] as f32 / host.size[1].max(1) as f32;
        let camera_view = match self.camera_view {
            Some(v) => v,
            None => self
                .lens
                .apply(view::frame(&mut self.camera, aspect, &self.loaded.scene)),
        };
        let seconds = self.started.elapsed().as_secs_f32();
        host.renderer.ui_mut(gpu).set_target(Some(ui_view.clone()));
        if let (Some(debug), Some(window)) = (&mut self.debug, host.window) {
            let changes = debug.run(
                window,
                gpu,
                &self.stats,
                host.renderer.quality(),
                self.tools,
            );
            if let Some(q) = changes.quality {
                host.renderer.set_quality(gpu, q);
            }
            self.sim.want_ai_summaries(changes.wants_ai);
            if let Some(tools) = changes.tools {
                self.set_tools(tools);
            }
        }
        // While the overlay shows, sample the frame's GPU time (submit to idle) twice a
        // second; pass timestamps are only hints on Apple GPUs (D34).
        if self.debug.as_ref().is_some_and(|d| d.open)
            && self.gpu_sampled.elapsed() >= Duration::from_millis(500)
        {
            host.renderer.sample_gpu_time = true;
            self.gpu_sampled = Instant::now();
        }
        let debug = &self.debug;
        let paint = |encoder: &mut wgpu::CommandEncoder| {
            if let Some(debug) = debug {
                debug.paint(encoder, ui_view);
            }
        };
        let report = host
            .renderer
            .render(gpu, &camera_view, seconds, output, Some(&paint));
        self.stats.report = report.clone();
        if report.gpu_frame_ms.is_some() {
            self.stats.gpu_frame_ms = report.gpu_frame_ms;
        }
        report
    }
}

/// The minimap and portrait images and their UI texture ids (render targets sit below
/// `canopy_render::ui::FIRST_PACK_TEXTURE`).
pub(crate) const MINIMAP_PIXELS: u32 = 256;
pub(crate) const PORTRAIT_PIXELS: u32 = 256;
const MINIMAP_TEXTURE: u32 = 1;
const PORTRAIT_TEXTURE: u32 = 2;

/// What the renderer's world UI pass draws from what play shows.
pub(crate) fn world_ui_input(
    world_ui: &super::WorldUi,
    scale: f32,
    viewer: Option<u8>,
) -> canopy_scene::world_ui::WorldUiInput {
    use canopy_scene::world_ui::{
        AimMark, GroundMarker, HealthBars, PlacementMark, WorldUiInput, linear,
    };
    WorldUiInput {
        selected: world_ui.selected.clone(),
        hovered: world_ui.hovered,
        health: match world_ui.health {
            super::HealthBars::Default => HealthBars::Damaged,
            super::HealthBars::All => HealthBars::Always,
            super::HealthBars::Friendly => HealthBars::Friendly,
            super::HealthBars::Hostile => HealthBars::Hostile,
        },
        viewer,
        scale,
        markers: world_ui
            .markers
            .iter()
            .map(|(m, age)| {
                let [r, g, b] = m.kind.color();
                GroundMarker {
                    position: m.position,
                    radius: m.radius,
                    color: linear(u32::from_be_bytes([0, r, g, b])),
                    opacity: m.opacity(*age),
                }
            })
            .collect(),
        aim: world_ui.aim.map(|a| AimMark {
            origin: a.origin,
            point: a.point,
            radius: a.radius,
            line: a.line,
            range: a.range,
            valid: a.valid,
        }),
        placement: world_ui.ghost.as_ref().map(|g| PlacementMark {
            cells: [g.cells.min_x, g.cells.min_y, g.cells.max_x, g.cells.max_y],
            valid: g.problem.is_none(),
            entrance: g.entrance,
        }),
        tree: world_ui
            .tree
            .map(|r| [r.position[0], r.position[1], r.radius]),
        ..WorldUiInput::default()
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
    // Translucent like the old ghost (0.48 opacity), not screen-doored.
    instance.tint = [1.0, 1.0, 1.0, 0.48];
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
