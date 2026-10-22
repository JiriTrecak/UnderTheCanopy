//! The running match as the screens see it: the seam to the in-match play module.
//!
//! The screens start a match from a prepared world and session ([`Playing::start`]),
//! which runs the real [`play::run::InMatch`] (the player's HUD, input, placement and
//! fog through the seat's view; the observer HUD for spectators). They forward window
//! events after the UI saw them ([`Playing::window_event`]), update it before the UI
//! lays out ([`Playing::update`]), hand it the HUD's UI actions ([`Playing::act`]), let
//! it decorate the UI's draw list and render ([`Playing::render`]) and read back
//! [`MatchEvent`]s (the menu was asked for, the outcome, network trouble). Pause, save
//! and chat go to the simulation thread.

use crate::{
    live::LiveScene,
    play::{
        self,
        run::{Host, InMatch, Options},
    },
    saves::{MatchIdentity, Mode, SaveBody},
    sim::{Control, LiveStatus, Notice, Ready, Snapshot},
    world::Prepared,
};
use canopy_render::FrameReport;
use canopy_ui::{DrawList, Ui};
use std::sync::Arc;
use std::time::Duration;
use winit::{event::WindowEvent, window::Window};

/// How long the next beat may be missing before "waiting for players" shows.
pub const STALL_NOTICE: Duration = Duration::from_millis(1500);

/// What kind of match runs (titles, what the game menu offers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Skirmish,
    Campaign,
    Multiplayer,
    /// Watching a replay (D60).
    Replay,
}

impl MatchKind {
    pub fn save_mode(self) -> Option<Mode> {
        match self {
            Self::Skirmish => Some(Mode::Skirmish),
            Self::Campaign => Some(Mode::Campaign),
            Self::Multiplayer | Self::Replay => None,
        }
    }

    /// The replay's word for a recorded match of this kind.
    pub fn replay_mode(self) -> Option<crate::replay::format::Mode> {
        use crate::replay::format::Mode as M;
        match self {
            Self::Skirmish => Some(M::Skirmish),
            Self::Campaign => Some(M::Campaign),
            Self::Multiplayer => Some(M::Multiplayer),
            Self::Replay => None,
        }
    }
}

/// The replay being watched.
#[derive(Debug, Clone)]
pub struct Watching {
    /// Its name in the replays folder (the file name).
    pub name: String,
    /// The file.
    pub path: std::path::PathBuf,
    pub header: crate::replay::format::Header,
}

/// What the screens learn from a running match.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchEvent {
    /// The player asked for the game menu (F10 or Escape by default, or the HUD's menu
    /// button).
    MenuRequested,
    /// The outcome was decided.
    Decided(canopy_sim::state::Outcome),
    /// The match cannot continue: desync, a closed connection, a stopped session.
    Problem { title: String, body: String },
    /// The server ended the match for everyone (another peer reported the outcome).
    Ended(canopy_net::wire::WireOutcome),
}

pub struct Playing {
    pub game: InMatch,
    /// The local player's slot; `None` when watching.
    pub me: Option<u8>,
    pub kind: MatchKind,
    pub map_id: String,
    pub map_name: String,
    pub identity: MatchIdentity,
    pub networked: bool,
    /// Player names by slot, for chat and results.
    pub names: Vec<(u8, String)>,
    /// Player colours by slot (sRGB), for chat names.
    colors: Vec<[u8; 3]>,
    /// Slots the computer plays, and those that already said "gg" on defeat.
    computers: Vec<u8>,
    good_games: Vec<u8>,
    /// The outcome once decided.
    pub outcome: Option<canopy_sim::state::Outcome>,
    events: Vec<MatchEvent>,
    problem: bool,
    /// The match's recording and what a replay of it says about it (D60).
    pub recorder: Option<crate::replay::record::Recorder>,
    /// The match as it started.
    pub setup: canopy_sim::MatchSetup,
    pub info: Option<crate::replay::MatchInfo>,
    /// The replay being watched.
    pub watching: Option<Watching>,
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
    /// The camera's start: `x`, `z`, zoom, yaw in degrees (a loaded save's).
    pub camera: Option<[f32; 4]>,
    /// Control groups to restore (a loaded save's).
    pub groups: Option<Vec<Vec<u32>>>,
    /// The player's name in the chat.
    pub name: String,
    /// What a replay of this match says about it (recorded matches).
    pub info: Option<crate::replay::MatchInfo>,
    /// The replay being watched.
    pub watching: Option<Watching>,
}

impl Playing {
    /// Uploads the scene, starts the simulation thread and attaches the HUD.
    pub fn start(
        prepared: Prepared,
        start: Start,
        host: &mut Host<'_>,
        options: &Options,
    ) -> anyhow::Result<Self> {
        let Prepared {
            loaded,
            game,
            map_name,
            ..
        } = prepared;
        let names = game
            .world
            .setup()
            .slots
            .iter()
            .map(|s| {
                (
                    s.player,
                    match s.kind {
                        // Lobbies name every computer seat "Computer": number them.
                        canopy_sim::SlotKind::Ai => format!("Computer {}", s.player + 1),
                        canopy_sim::SlotKind::Human => s
                            .name
                            .clone()
                            .unwrap_or_else(|| format!("Player {}", s.player + 1)),
                    },
                )
            })
            .collect();
        let computers = game
            .world
            .setup()
            .slots
            .iter()
            .filter(|s| s.kind == canopy_sim::SlotKind::Ai)
            .map(|s| s.player)
            .collect();
        let colors = loaded
            .scene
            .colors
            .iter()
            .map(|c| c.map(|v| (play::run::srgb(v) * 255.0).round() as u8))
            .collect();
        let networked = game.session.room().is_some();
        let recorder = game.recorder().filter(|_| start.info.is_some());
        let setup = game.world.setup().clone();
        let camera = start
            .camera
            .map(|[x, z, zoom, yaw]| crate::options::CameraSpec { x, z, zoom, yaw });
        let mut game = InMatch::start(
            host.gpu,
            host.renderer,
            host.ui,
            host.audio,
            loaded,
            game,
            play::run::Start {
                me: start.me,
                tick: start.start_tick,
                camera,
                ready: start.ready,
                groups: start.groups,
                name: Some(start.name),
                chat_server: networked,
            },
        )?;
        game.set_options(options);
        if start.watching.is_some() {
            game.set_replay();
        }
        Ok(Self {
            game,
            me: start.me,
            kind: start.kind,
            map_id: start.map_id,
            map_name,
            identity: start.identity,
            networked,
            names,
            colors,
            computers,
            good_games: Vec::new(),
            outcome: None,
            events: Vec::new(),
            problem: false,
            recorder,
            setup,
            info: start.info,
            watching: start.watching,
        })
    }

    /// The replay of this match so far (recorded matches), its result as `result`.
    pub fn replay(&self, result: &str) -> Option<crate::replay::format::Replay> {
        let recording = self.recorder.as_ref()?.snapshot();
        let info = self.info.as_ref()?;
        match crate::replay::build(&recording, info, result, crate::saves::now_ms()) {
            Ok(replay) => Some(replay),
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "the replay could not be made");
                None
            }
        }
    }

    /// How the match stands for the local player, in a word (a replay's result).
    pub fn result_word(&self, surrendered: bool) -> &'static str {
        crate::replay::result_word(self.outcome.as_ref(), self.me, &self.setup, surrendered)
    }

    /// A computer player whose colony falls says "gg" in the chat, on every seat (each
    /// seat's simulation sees the same defeat; nothing crosses the network).
    fn good_game(&mut self) {
        let Some(newest) = self.game.live().newest().cloned() else {
            return;
        };
        for owner in &newest.view.defeated {
            let canopy_content::placement::Owner::Player(slot) = *owner else {
                continue;
            };
            if !self.computers.contains(&slot) || self.good_games.contains(&slot) {
                continue;
            }
            self.good_games.push(slot);
            const LINES: [&str; 3] = ["gg", "gg wp", "gg, well played!"];
            let line = LINES[usize::from(slot) % LINES.len()];
            let color = self
                .colors
                .get(usize::from(slot))
                .copied()
                .unwrap_or([0xcc, 0xcc, 0xcc]);
            self.game.play.hear(self.name_of(slot), color, line);
        }
    }

    /// Player `player`'s colour (sRGB).
    pub fn color_of(&self, player: u8) -> [u8; 3] {
        self.colors
            .get(usize::from(player))
            .copied()
            .unwrap_or([0xcc, 0xcc, 0xcc])
    }

    pub fn name_of(&self, player: u8) -> String {
        self.names
            .iter()
            .find(|(p, _)| *p == player)
            .map_or_else(|| format!("Player {}", player + 1), |(_, n)| n.clone())
    }

    /// A window event, after the UI saw it (`consumed`: the UI took it).
    pub fn window_event(
        &mut self,
        window: Option<&Window>,
        event: &WindowEvent,
        ui: &Ui,
        consumed: bool,
    ) {
        self.game.window_event(window, event, ui, consumed);
    }

    /// Releases held keys and drags (an overlay took the input).
    pub fn release_input(&mut self) {
        self.game.release_input();
    }

    /// Advances the match up to the HUD (`free`: no overlay holds the input) and takes
    /// what the simulation reported.
    pub fn update(&mut self, host: &mut Host<'_>, free: bool) {
        self.game.update(host, free);
        for notice in self.game.sim().notices() {
            self.notice(notice);
        }
        self.good_game();
        if self.game.stopped() && !self.problem {
            self.problem = true;
            self.events.push(MatchEvent::Problem {
                title: "The match stopped".into(),
                body: "The simulation stopped unexpectedly. The log has the details.".into(),
            });
        }
    }

    /// The HUD's actions: the menu goes to the screens, chat lines to the match server.
    pub fn act(&mut self, ui: &mut Ui, actions: Vec<canopy_ui::Action>) {
        for event in self.game.act(ui, actions) {
            match event {
                play::Event::OpenMenu => self.events.push(MatchEvent::MenuRequested),
                play::Event::Chat(text) => {
                    if self.networked {
                        self.chat(&text);
                    }
                }
                play::Event::LookAt(_) | play::Event::ToggleDebug => {}
            }
        }
    }

    /// The world UI stand-in under the HUD.
    pub fn decorate(&self, list: &mut DrawList) {
        self.game.decorate(list);
    }

    /// Records the frame into `output`, the UI into `ui_view`.
    pub fn render(
        &mut self,
        host: &mut Host<'_>,
        output: &wgpu::TextureView,
        ui_view: &wgpu::TextureView,
    ) -> FrameReport {
        self.game.render(host, output, ui_view)
    }

    /// Follows changed settings.
    pub fn set_options(&mut self, options: &Options) {
        self.game.set_options(options);
    }

    fn notice(&mut self, notice: Notice) {
        use canopy_net::Event;
        match notice {
            Notice::Decided { outcome, tick } => {
                if self.outcome.is_none() {
                    tracing::info!(tick, winner = ?outcome.winner, "match decided");
                    self.outcome = Some(outcome.clone());
                    // A replay plays on through its end; it has no result dialog.
                    if self.watching.is_none() {
                        self.events.push(MatchEvent::Decided(outcome));
                    }
                }
            }

            Notice::Net(Event::Chat(message)) => self.hear(&message),
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

    /// A chat line from the match server into the HUD's chat.
    pub fn hear(&mut self, message: &canopy_net::wire::ChatMessage) {
        let name = if message.name.is_empty() {
            "Spectator".to_string()
        } else {
            message.name.clone()
        };
        let color = message
            .player
            .and_then(|p| self.colors.get(usize::from(p)).copied())
            .unwrap_or([0xcc, 0xcc, 0xcc]);
        self.game.play.hear(name, color, &message.text);
    }

    /// Takes the match's events since the last call.
    pub fn take_events(&mut self) -> Vec<MatchEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn live(&self) -> &LiveScene {
        self.game.live()
    }

    /// The newest presented tick (mission state, outcome).
    pub fn newest(&self) -> Option<&Arc<Snapshot>> {
        self.game.live().newest()
    }

    /// Autoplay's speed, or the player taking over.
    pub fn autoplay(&self, control: crate::sim::AutoplayControl) {
        self.game.sim().control(Control::Autoplay(control));
    }

    pub fn status(&self) -> LiveStatus {
        self.game.sim().live()
    }

    pub fn pause(&self, pause: bool) {
        self.game.sim().control(Control::Pause(pause));
    }

    pub fn chat(&self, text: &str) {
        self.game.sim().control(Control::Chat(text.to_owned()));
    }

    /// The match as a save body: the world (waits briefly for the simulation thread),
    /// the camera and the control groups.
    pub fn save(&self) -> Option<SaveBody> {
        let world = self.game.sim().save(Duration::from_secs(5))?;
        let camera = self.game.camera();
        let groups = &self.game.play.groups;
        Some(SaveBody {
            world,
            camera: Some([
                camera.focus[0],
                camera.focus[1],
                camera.zoom(),
                camera.yaw.to_degrees(),
            ]),
            groups: Some(
                (0..play::selection::GROUPS)
                    .map(|slot| groups.get(slot).to_vec())
                    .collect(),
            ),
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
