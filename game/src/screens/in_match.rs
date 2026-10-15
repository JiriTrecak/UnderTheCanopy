//! The running match as the screens see it: the seam to the in-match play module.
//!
//! The screens start a match from a prepared world and session ([`Playing::start`]),
//! forward window events the UI did not take ([`Playing::window_event`]), update it each
//! frame ([`Playing::update`]), let it render ([`Playing::render`]) and read back
//! [`MatchEvent`]s (the menu was asked for, the outcome, network trouble, chat). Pause,
//! save and chat go to the simulation thread.
//!
//! Until the play module (`game/src/play/`: selection, orders, HUD) lands, the match is
//! watched with the spectator camera (arrow keys and screen edges, wheel and PageUp/Down,
//! middle-drag grab, Q/E to turn, B for the largest fight) from the observer's
//! presentation. The play module replaces the internals of this type and keeps its API.

use crate::{
    live::LiveScene,
    saves::{MatchIdentity, Mode, SaveBody},
    settings::{Chord, KeyBindings, keys::Entry},
    sim::{self, Control, LiveStatus, Notice, Ready, SimThread, Snapshot},
    view,
    world::{Loaded, Prepared},
};
use canopy_render::{
    FrameReport, Gpu, Renderer, RtsCamera,
    camera::{EDGE_SCROLL_PIXELS, MAX_STEP_MS},
};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
};

/// Radians per second the camera turns with Q and E.
const TURN_SPEED: f32 = 1.2;
/// PageUp/PageDown wheel pixels per millisecond (old `camera.zoomIn`).
const KEY_ZOOM: f32 = 0.7;
/// How long the next beat may be missing before "waiting for players" shows.
pub const STALL_NOTICE: Duration = Duration::from_millis(1500);

/// What kind of match runs (titles, what the game menu offers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Skirmish,
    Campaign,
    Multiplayer,
}

impl MatchKind {
    pub fn save_mode(self) -> Option<Mode> {
        match self {
            Self::Skirmish => Some(Mode::Skirmish),
            Self::Campaign => Some(Mode::Campaign),
            Self::Multiplayer => None,
        }
    }
}

/// What the screens learn from a running match.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchEvent {
    /// The player asked for the game menu (F10 or Escape by default).
    MenuRequested,
    /// The player asked to chat (Enter, networked matches).
    ChatRequested,
    /// The outcome was decided.
    Decided(canopy_sim::state::Outcome),
    /// The match cannot continue: desync, a closed connection, a stopped session.
    Problem {
        title: String,
        body: String,
    },
    Chat(canopy_net::wire::ChatMessage),
    /// The server ended the match for everyone (another peer reported the outcome).
    Ended(canopy_net::wire::WireOutcome),
}

pub struct Playing {
    pub loaded: Loaded,
    pub camera: RtsCamera,
    pub live: LiveScene,
    sim: SimThread,
    /// The local player's slot; `None` when watching.
    pub me: Option<u8>,
    pub kind: MatchKind,
    pub map_id: String,
    pub map_name: String,
    pub identity: MatchIdentity,
    pub networked: bool,
    /// Player names by slot, for chat and results.
    pub names: Vec<(u8, String)>,
    /// The outcome once decided.
    pub outcome: Option<canopy_sim::state::Outcome>,
    keys: BTreeSet<KeyCode>,
    modifiers: ModifiersState,
    cursor: Option<PhysicalPosition<f64>>,
    drag: Option<PhysicalPosition<f64>>,
    focused: bool,
    events: Vec<MatchEvent>,
    problem: bool,
    last_size: [u32; 2],
}

/// How a match starts.
pub struct Start {
    pub me: Option<u8>,
    pub kind: MatchKind,
    pub map_id: String,
    pub identity: MatchIdentity,
    /// Fast-forward before showing (development).
    pub start_tick: u32,
    /// A networked seat reports its identity once the world is built.
    pub ready: Option<Ready>,
    /// The camera's start: `x`, `z`, zoom, yaw (a loaded save's).
    pub camera: Option<[f32; 4]>,
    /// The spectator may zoom further out.
    pub spectator: bool,
}

impl Playing {
    /// Uploads the scene and starts the simulation thread.
    pub fn start(
        prepared: Prepared,
        start: Start,
        renderer: &mut Renderer,
        gpu: &Gpu,
    ) -> anyhow::Result<Self> {
        let Prepared {
            loaded,
            game,
            map_name,
            ..
        } = prepared;
        renderer.set_environment(gpu, loaded.environment.clone());
        renderer.load_world(gpu, &loaded.pack, &loaded.scene.world)?;
        renderer.set_actors(&[]);
        let names = game
            .world
            .setup()
            .slots
            .iter()
            .map(|s| {
                (
                    s.player,
                    s.name.clone().unwrap_or_else(|| match s.kind {
                        canopy_sim::SlotKind::Ai => format!("Computer {}", s.player + 1),
                        canopy_sim::SlotKind::Human => format!("Player {}", s.player + 1),
                    }),
                )
            })
            .collect();
        let networked = game.session.room().is_some();
        let mut camera = match start.camera {
            Some([x, z, zoom, yaw]) => view::initial(
                Some(crate::options::CameraSpec { x, z, zoom, yaw }),
                &loaded.scene,
            ),
            None => {
                let mut camera = view::initial(None, &loaded.scene);
                if let Some(focus) = start
                    .me
                    .and_then(|me| loaded.scene.starts.get(usize::from(me)))
                {
                    camera.look_at(focus[0], focus[1]);
                }
                camera
            }
        };
        if !start.spectator && start.me.is_some() {
            camera.zoom_range = canopy_render::camera::game_zoom();
        }
        let sim = SimThread::start(
            game.world,
            game.session,
            game.presenter,
            sim::TICK,
            start.start_tick,
            start.ready,
        );
        Ok(Self {
            loaded,
            camera,
            live: LiveScene::default(),
            sim,
            me: start.me,
            kind: start.kind,
            map_id: start.map_id,
            map_name,
            identity: start.identity,
            networked,
            names,
            outcome: None,
            keys: BTreeSet::new(),
            modifiers: ModifiersState::empty(),
            cursor: None,
            drag: None,
            focused: true,
            events: Vec::new(),
            problem: false,
            last_size: [1, 1],
        })
    }

    pub fn name_of(&self, player: u8) -> String {
        self.names
            .iter()
            .find(|(p, _)| *p == player)
            .map_or_else(|| format!("Player {}", player + 1), |(_, n)| n.clone())
    }

    /// A window event the UI did not consume.
    pub fn window_event(&mut self, event: &WindowEvent, keys: &KeyBindings, catalog: &[Entry]) {
        match event {
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                if !focused {
                    self.keys.clear();
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => {
                        self.keys.insert(code);
                        if event.repeat {
                            return;
                        }
                        let chord = Chord::of(code, self.modifiers);
                        let is =
                            |id: &str| chord.as_ref().is_some_and(|c| keys.pressed(catalog, id, c));
                        if is("interface.gameMenu") || code == KeyCode::Escape {
                            self.events.push(MatchEvent::MenuRequested);
                        } else if code == KeyCode::Enter && self.networked {
                            self.events.push(MatchEvent::ChatRequested);
                        } else if code == KeyCode::KeyB {
                            self.look_at_battle();
                        }
                    }
                    ElementState::Released => {
                        self.keys.remove(&code);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(from) = self.drag {
                    self.grab(from, *position);
                    self.drag = Some(*position);
                }
                self.cursor = Some(*position);
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
            WindowEvent::MouseWheel { delta, .. } => {
                let pixels = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 16.0 * 6.25,
                    MouseScrollDelta::PixelDelta(p) => -p.y as f32,
                };
                self.camera.wheel(pixels);
            }
            _ => {}
        }
    }

    /// Releases held keys and drags (the UI took the pointer or keyboard).
    pub fn release_input(&mut self) {
        self.keys.clear();
        self.drag = None;
    }

    /// Grab panning: the ground under the cursor follows it.
    fn grab(&mut self, from: PhysicalPosition<f64>, to: PhysicalPosition<f64>) {
        let size = self.last_size;
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let units_per_pixel = self
            .camera
            .footprint(aspect, 0.0)
            .map_or(0.1, |[x0, x1, ..]| (x1 - x0) / size[0].max(1) as f32);
        let dx = (to.x - from.x) as f32 * units_per_pixel;
        let dy = (to.y - from.y) as f32 * units_per_pixel;
        self.camera.pan(-dx, dy);
    }

    fn look_at_battle(&mut self) {
        let battle = self.live.newest().and_then(|s| s.presentation.battle);
        if let Some([x, z]) = battle {
            self.camera.look_at(x, z);
        }
    }

    /// Advances the camera and takes what the simulation reported.
    pub fn update(
        &mut self,
        dt_ms: f32,
        size: [u32; 2],
        scale: f64,
        keys: &KeyBindings,
        catalog: &[Entry],
        edge_scroll: bool,
    ) {
        self.last_size = size;
        let held = |id: &str| {
            keys.chord(catalog, id)
                .and_then(|c| crate::settings::keys::key_code(&c.code))
                .is_some_and(|k| self.keys.contains(&k))
        };
        let mut dir = [0.0f32, 0.0];
        if held("camera.panLeft") {
            dir[0] -= 1.0;
        }
        if held("camera.panRight") {
            dir[0] += 1.0;
        }
        if held("camera.panUp") {
            dir[1] += 1.0;
        }
        if held("camera.panDown") {
            dir[1] -= 1.0;
        }
        if let (Some(cursor), true, None, true) =
            (self.cursor, self.focused, self.drag, edge_scroll)
        {
            let edge = f64::from(EDGE_SCROLL_PIXELS) * scale;
            if cursor.x <= edge {
                dir[0] -= 1.0;
            } else if cursor.x >= f64::from(size[0]) - edge {
                dir[0] += 1.0;
            }
            if cursor.y <= edge {
                dir[1] += 1.0;
            } else if cursor.y >= f64::from(size[1]) - edge {
                dir[1] -= 1.0;
            }
        }
        let dir = [dir[0].clamp(-1.0, 1.0), dir[1].clamp(-1.0, 1.0)];
        self.camera.pan_for(dir, dt_ms);
        let step = dt_ms.min(MAX_STEP_MS);
        if self.keys.contains(&KeyCode::KeyQ) {
            self.camera.yaw += TURN_SPEED * step / 1000.0;
        }
        if self.keys.contains(&KeyCode::KeyE) {
            self.camera.yaw -= TURN_SPEED * step / 1000.0;
        }
        if held("camera.zoomIn") {
            self.camera.wheel(-step * KEY_ZOOM);
        }
        if held("camera.zoomOut") {
            self.camera.wheel(step * KEY_ZOOM);
        }
        self.camera.update(dt_ms);
        for notice in self.sim.notices() {
            self.notice(notice);
        }
        if self.sim.finished() && !self.problem {
            self.problem = true;
            self.events.push(MatchEvent::Problem {
                title: "The match stopped".into(),
                body: "The simulation stopped unexpectedly. The log has the details.".into(),
            });
        }
    }

    fn notice(&mut self, notice: Notice) {
        use canopy_net::Event;
        match notice {
            Notice::Decided { outcome, tick } => {
                if self.outcome.is_none() {
                    tracing::info!(tick, winner = ?outcome.winner, "match decided");
                    self.outcome = Some(outcome.clone());
                    self.events.push(MatchEvent::Decided(outcome));
                }
            }
            Notice::Net(Event::Chat(message)) => self.events.push(MatchEvent::Chat(message)),
            Notice::Net(Event::Desync { tick, .. }) => {
                self.problem = true;
                self.events.push(MatchEvent::Problem {
                    title: "Out of sync".into(),
                    body: format!(
                        "The players' games disagreed at beat {tick}, so the match cannot continue. \
                         This is a bug; the log has the details."
                    ),
                });
            }
            Notice::Net(Event::Stopped { reason }) => {
                self.problem = true;
                self.events.push(MatchEvent::Problem {
                    title: "The match stopped".into(),
                    body: capitalise(&reason),
                });
            }
            Notice::Net(Event::Closed { reason }) => {
                if self.outcome.is_none() && !self.problem {
                    self.problem = true;
                    self.events.push(MatchEvent::Problem {
                        title: "Connection lost".into(),
                        body: if reason.is_empty() {
                            "The connection to the match server closed.".into()
                        } else {
                            format!("The connection to the match server closed: {reason}.")
                        },
                    });
                }
            }
            Notice::Net(Event::Ended { outcome, .. }) => {
                self.events.push(MatchEvent::Ended(outcome));
            }
            Notice::Net(Event::Error { code, message }) => {
                tracing::warn!(%code, %message, "match server error");
            }
            Notice::Net(Event::Skipped {
                beat,
                player,
                reason,
                ..
            }) => {
                tracing::warn!(beat, player, %reason, "skipped a committed command");
            }
            Notice::Net(_) => {}
        }
    }

    /// Takes the match's events since the last call.
    pub fn take_events(&mut self) -> Vec<MatchEvent> {
        std::mem::take(&mut self.events)
    }

    /// Prepares and records the frame into `output`.
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        output: &wgpu::TextureView,
        size: [u32; 2],
        seconds: f32,
        now: Instant,
    ) -> FrameReport {
        self.live.poll(self.sim.mailbox());
        self.live.frame(renderer, gpu, &self.loaded.pack, now);
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let camera_view = view::frame(&mut self.camera, aspect, &self.loaded.scene);
        let focus = [camera_view.target.x, camera_view.target.z];
        self.live
            .feed_ripples(renderer, &self.loaded.scene, seconds, focus);
        renderer.render(gpu, &camera_view, seconds, output, None)
    }

    /// The newest presented tick (mission state, outcome).
    pub fn newest(&self) -> Option<&Arc<Snapshot>> {
        self.live.newest()
    }

    pub fn status(&self) -> LiveStatus {
        self.sim.live()
    }

    pub fn pause(&self, pause: bool) {
        self.sim.control(Control::Pause(pause));
    }

    pub fn chat(&self, text: &str) {
        self.sim.control(Control::Chat(text.to_owned()));
    }

    /// The match as a save body (waits briefly for the simulation thread).
    pub fn save(&self) -> Option<SaveBody> {
        let world = self.sim.save(Duration::from_secs(5))?;
        Some(SaveBody {
            world,
            camera: Some([
                self.camera.focus[0],
                self.camera.focus[1],
                self.camera.zoom(),
                self.camera.yaw.to_degrees(),
            ]),
        })
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect::<String>() + ".",
        None => String::new(),
    }
}
