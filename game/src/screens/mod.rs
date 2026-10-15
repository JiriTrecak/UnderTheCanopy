//! The game's screens and app flow: boot, main menu, campaign, skirmish, multiplayer,
//! loading, the match with its menu and result, settings and saves.
//!
//! [`Game`] owns the UI ([`ui_host::UiHost`]), the flow state machine ([`flow::Flow`]), the
//! models of every screen and the running match ([`in_match::Playing`]). The window
//! (`app.rs`) forwards events to [`Game::window_event`], calls [`Game::update`] once per
//! frame and [`Game::render`] to draw. Layouts live in `config/ui/layouts/` (one document
//! per screen), reproducing the old game's menus.

pub mod campaign;
pub mod capture;
pub mod flow;
pub mod in_match;
pub mod loading;
pub mod match_ui;
pub mod menu;
pub mod multiplayer;
#[cfg(test)]
mod net_tests;
pub mod settings_ui;
pub mod skirmish;
pub mod ui_host;
pub mod widgets;

use crate::{
    data::{BootStage, GameData},
    paths::Paths,
    saves::{self, MatchIdentity, NewSave, SaveLibrary, SaveMeta},
    settings::{Chord, Settings, WindowMode, keys},
    sim::Ready,
    world::MatchRequest,
};
use anyhow::Result;
use campaign::Progress;
use canopy_render::{Gpu, Renderer};
use canopy_ui::{Action, ActionKind};
use flow::{Effect, Flow, Intent, Origin, Overlay, Screen};
use in_match::{MatchEvent, MatchKind, Playing, Start};
use loading::{DevSpec, Job, LoadJob};
use match_ui::{Chat, MenuPage, MissionHud, ResultModel};
use multiplayer::Multiplayer;
use settings_ui::{Changed, SettingsUi};
use skirmish::Skirmish;
use std::sync::{Arc, mpsc};
use std::time::Instant;
use ui_host::UiHost;
use widgets::Palette;
use winit::{event::WindowEvent, keyboard::ModifiersState, window::Window};

/// Fade-out and fade-in of a screen change (old `MenuTransition`), seconds.
const FADE_OUT: f32 = 0.18;
const FADE_IN: f32 = 0.30;

/// What the window gives a frame.
pub struct Ctx<'a> {
    pub gpu: &'a Gpu,
    pub renderer: &'a mut Renderer,
    pub window: Option<&'a Window>,
    /// Target size in physical pixels and the display scale.
    pub size: [u32; 2],
    pub scale: f32,
}

/// What the game asks of the window after a frame.
#[derive(Debug, Default)]
pub struct Requests {
    pub exit: bool,
    pub vsync: Option<bool>,
}

/// A match started from the development options (`--map`, `--spectate`).
#[derive(Debug, Clone)]
pub struct DevStart {
    pub spec: DevSpec,
    pub spectate: bool,
    pub tick: u32,
    pub camera: Option<[f32; 4]>,
}

/// What happens once a load finishes.
#[derive(Debug, Clone)]
struct PendingStart {
    kind: MatchKind,
    me: Option<u8>,
    map: String,
    networked: bool,
    camera: Option<[f32; 4]>,
    spectator: bool,
    start_tick: u32,
    /// Shows a still frame instead of running (`--map` without `--spectate`).
    paused: bool,
}

/// A match that can be started again (restart).
#[derive(Debug, Clone)]
struct Rerun {
    map: String,
    setup: canopy_sim::MatchSetup,
    me: Option<u8>,
    kind: MatchKind,
    origin: Origin,
}

struct Loading {
    job: Option<LoadJob>,
    start: PendingStart,
    error: Option<String>,
    started: Instant,
}

/// A question with up to two answers besides Cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    DeleteSave(String),
}

struct Dialog {
    ask: Ask,
    title: String,
    body: String,
    confirm: String,
    danger: bool,
    /// A second answer (Save and exit).
    second: Option<String>,
}

enum BootMessage {
    Stage(BootStage, f32),
    Done(Box<Result<GameData>>),
}

pub struct Game {
    paths: Paths,
    pub settings: Settings,
    settings_dirty: bool,
    catalog: Vec<keys::Entry>,
    library: SaveLibrary,
    progress: Progress,
    flow: Flow,
    pub host: UiHost,
    palette: Palette,
    data: Option<Arc<GameData>>,
    boot: Option<mpsc::Receiver<BootMessage>>,
    boot_stage: (BootStage, f32),
    boot_error: Option<String>,
    dev: Option<DevStart>,
    skirmish: Skirmish,
    mission_index: usize,
    mp: Multiplayer,
    loading: Option<Loading>,
    pub playing: Option<Playing>,
    result: Option<ResultModel>,
    result_hidden: bool,
    menu_page: MenuPage,
    menu_status: String,
    saves_list: Vec<SaveMeta>,
    saves_status: String,
    settings_ui: SettingsUi,
    dialog: Option<Dialog>,
    chat: Chat,
    problem: Option<(String, String)>,
    mission_hud: MissionHud,
    last_match: Option<Rerun>,
    pending: Option<Intent>,
    fade: f32,
    fading_out: bool,
    pub audio: canopy_audio::Audio,
    modifiers: ModifiersState,
    started: Instant,
    last_frame: Instant,
    last_reload: Instant,
    requests: Requests,
    view: [f32; 2],
    apply_all: bool,
    clear_actors: bool,
}

fn seed() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32);
    nanos.max(1)
}

/// Escapes markup in player text (chat).
fn plain(text: &str) -> String {
    text.replace('[', "[[")
}

impl Game {
    /// Builds the UI and starts loading the game's data in the background.
    pub fn new(
        paths: Paths,
        settings: Settings,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        dev: Option<DevStart>,
    ) -> Result<Self> {
        let pack = GameData::open_pack(&paths.data)?;
        let host = UiHost::new(&paths.data, gpu, format, &pack)?;
        let (tx, rx) = mpsc::channel();
        {
            let paths = paths.clone();
            std::thread::Builder::new()
                .name("boot".into())
                .spawn(move || {
                    let progress_tx = tx.clone();
                    let progress = move |stage, value| {
                        let _ = progress_tx.send(BootMessage::Stage(stage, value));
                    };
                    let result = GameData::load(&paths, pack, &progress);
                    let _ = tx.send(BootMessage::Done(Box::new(result)));
                })?;
        }
        let audio = canopy_audio::Audio::new(settings.audio);
        let now = Instant::now();
        let mut game = Self {
            palette: Palette::load(&paths.data),
            library: SaveLibrary::new(&paths.user),
            progress: Progress::load(&paths.user),
            catalog: keys::catalog(),
            paths,
            settings,
            settings_dirty: false,
            flow: Flow::default(),
            host,
            data: None,
            boot: Some(rx),
            boot_stage: (BootStage::Pack, 0.0),
            boot_error: None,
            dev,
            skirmish: Skirmish::default(),
            mission_index: 0,
            mp: Multiplayer::default(),
            loading: None,
            playing: None,
            result: None,
            result_hidden: false,
            menu_page: MenuPage::Main,
            menu_status: String::new(),
            saves_list: Vec::new(),
            saves_status: String::new(),
            settings_ui: SettingsUi::default(),
            dialog: None,
            chat: Chat::default(),
            problem: None,
            mission_hud: MissionHud::default(),
            last_match: None,
            pending: None,
            fade: 1.0,
            fading_out: false,
            audio,
            modifiers: ModifiersState::empty(),
            started: now,
            last_frame: now,
            last_reload: now,
            requests: Requests::default(),
            view: [1.0, 1.0],
            apply_all: true,
            clear_actors: false,
        };
        widgets::text(
            &mut game.host.ui,
            "main-menu/version",
            &format!("v{}", env!("CARGO_PKG_VERSION")),
        );
        Ok(game)
    }

    /// Whether the game is ready for the menus (data loaded).
    pub fn booted(&self) -> bool {
        self.data.is_some()
    }

    // --- Input ---------------------------------------------------------------------

    /// A window event: the UI first, then the match.
    pub fn window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::Focused(focused) => self.audio.set_background(!focused),
            WindowEvent::Occluded(hidden) => self.audio.set_background(*hidden),
            _ => {}
        }
        // A shortcut being rebound takes the next key.
        if self.settings_ui.capturing.is_some()
            && let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == winit::event::ElementState::Pressed
            && !key.repeat
        {
            let chord = match key.physical_key {
                winit::keyboard::PhysicalKey::Code(code) => Chord::of(code, self.modifiers),
                _ => None,
            };
            if self
                .settings_ui
                .capture(&mut self.settings, &self.catalog, chord)
            {
                self.settings_dirty = true;
            }
            return;
        }
        let mut consumed = false;
        for e in canopy_ui::winit::translate(event) {
            consumed |= self.host.ui.input(&e);
        }
        if self.fading_out {
            return;
        }
        let in_match = self.flow.in_match() && self.flow.overlays().is_empty();
        if let (Some(playing), true) = (&mut self.playing, in_match) {
            if consumed {
                if matches!(event, WindowEvent::MouseInput { .. }) {
                    playing.release_input();
                }
            } else {
                playing.window_event(event, &self.settings.keys, &self.catalog);
            }
            return;
        }
        // Escape outside a match: one step back.
        if !consumed
            && let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == winit::event::ElementState::Pressed
            && !key.repeat
            && key.physical_key
                == winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape)
        {
            self.escape();
        }
    }

    fn escape(&mut self) {
        match self.flow.top() {
            Some(Overlay::Confirm) => {
                self.dialog = None;
                self.apply(Intent::Back);
            }
            Some(Overlay::Settings) => self.close_settings(),
            Some(Overlay::GameMenu) if self.menu_page != MenuPage::Main => {
                self.menu_page = MenuPage::Main;
            }
            Some(_) => self.apply(Intent::Back),
            None => {
                if matches!(self.flow.screen(), Screen::Loading) {
                    self.apply(Intent::Back);
                } else {
                    self.navigate(Intent::Back);
                }
            }
        }
    }

    /// The window is closing: autosave a local match, write settings.
    pub fn quit(&mut self) {
        self.autosave();
        self.apply(Intent::Quit);
        self.save_settings();
    }

    fn autosave(&mut self) {
        let Some(playing) = &self.playing else {
            return;
        };
        let Some(mode) = playing.kind.save_mode() else {
            return;
        };
        if playing.outcome.is_some() || self.result.is_some() {
            return;
        }
        if let Some(body) = playing.save() {
            let result = self.library.write(
                &NewSave {
                    name: "Autosave",
                    mode,
                    map_name: &playing.map_name,
                    identity: &playing.identity,
                    player: playing.me,
                    body: &body,
                },
                Some(saves::AUTOSAVE),
            );
            match result {
                Ok(_) => tracing::info!("autosaved"),
                Err(error) => tracing::warn!(%error, "autosave failed"),
            }
        }
    }

    fn save_settings(&mut self) {
        if self.settings_dirty {
            match self.settings.save(&self.paths.user) {
                Ok(()) => self.settings_dirty = false,
                Err(error) => tracing::warn!(%error, "cannot save settings"),
            }
        }
    }

    // --- Flow ----------------------------------------------------------------------

    /// A screen change with the menu fade.
    fn navigate(&mut self, intent: Intent) {
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(intent);
        self.fading_out = true;
    }

    fn apply(&mut self, intent: Intent) {
        let before = self.flow.screen().clone();
        let effects = self.flow.apply(intent);
        for effect in effects {
            match effect {
                Effect::Pause(pause) => {
                    if let Some(p) = &self.playing {
                        p.pause(pause);
                    }
                }
                Effect::EndMatch => self.end_match(),
                Effect::LeaveRoom => self.mp.leave(),
                Effect::Exit => self.requests.exit = true,
            }
        }
        let after = self.flow.screen().clone();
        if before != after {
            self.entered(&after);
        }
    }

    fn end_match(&mut self) {
        self.playing = None;
        self.loading = None;
        self.result = None;
        self.result_hidden = false;
        self.problem = None;
        self.chat.clear();
        self.mission_hud = MissionHud::default();
        self.menu_page = MenuPage::Main;
        self.menu_status.clear();
        self.clear_actors = true;
    }

    /// Hooks when a screen comes up.
    fn entered(&mut self, screen: &Screen) {
        let Some(data) = self.data.clone() else {
            return;
        };
        match screen {
            Screen::Skirmish => {
                if self.skirmish.map_index(&data).is_none() {
                    self.skirmish.choose_map(&data, 0);
                }
            }
            Screen::Multiplayer => {
                self.mp.open(self.settings.multiplayer.server.as_deref());
                widgets::text(
                    &mut self.host.ui,
                    "multiplayer/name",
                    &self.settings.player_name,
                );
            }
            Screen::Missions { campaign } => {
                let missions = data.missions(campaign);
                // The first mission not yet won that is open.
                self.mission_index = (0..missions.len())
                    .find(|&i| {
                        self.progress.unlocked(&missions, i)
                            && !self.progress.completed.contains(missions[i].id())
                    })
                    .unwrap_or(0);
            }
            _ => {}
        }
    }

    fn start_loading(&mut self, job: Job, start: PendingStart, origin: Origin) {
        let Some(data) = self.data.clone() else {
            return;
        };
        // A match already running ends first (load or restart from its menu).
        if self.playing.is_some() {
            self.end_match();
        }
        self.dialog = None;
        self.loading = Some(Loading {
            job: Some(LoadJob::spawn(self.paths.clone(), data, job)),
            start,
            error: None,
            started: Instant::now(),
        });
        self.apply(Intent::StartMatch(origin));
    }

    fn start_rerun(&mut self, rerun: Rerun) {
        let start = PendingStart {
            kind: rerun.kind,
            me: rerun.me,
            map: rerun.map.clone(),
            networked: false,
            camera: None,
            spectator: rerun.me.is_none(),
            start_tick: 0,
            paused: false,
        };
        let mut setup = rerun.setup.clone();
        setup.seed = seed();
        let job = Job::Match(Box::new(MatchRequest {
            map: rerun.map.clone(),
            setup: setup.clone(),
            me: rerun.me,
            resume: None,
            session: None,
        }));
        self.last_match = Some(Rerun {
            setup,
            ..rerun.clone()
        });
        self.start_loading(job, start, rerun.origin);
    }

    fn start_skirmish(&mut self) {
        let Some(map) = self.skirmish.map.clone() else {
            return;
        };
        let setup = self.skirmish.setup(seed(), &self.settings.display_name());
        self.start_rerun(Rerun {
            map,
            me: self.skirmish.human(),
            setup,
            kind: MatchKind::Skirmish,
            origin: Origin::Skirmish,
        });
    }

    fn begin_mission(&mut self, campaign: &str, map_id: &str) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(map) = data.map(map_id) else {
            return;
        };
        let setup = canopy_sim::MatchSetup {
            seed: seed(),
            slots: map
                .starts
                .iter()
                .enumerate()
                .map(|(i, &player)| canopy_sim::Slot {
                    player: player.saturating_sub(1),
                    kind: if i == 0 {
                        canopy_sim::SlotKind::Human
                    } else {
                        canopy_sim::SlotKind::Ai
                    },
                    name: (i == 0).then(|| self.settings.display_name()),
                    team: None,
                    hero: None,
                    race: None,
                })
                .collect(),
        };
        let me = map.starts.first().map(|p| p.saturating_sub(1));
        self.start_rerun(Rerun {
            map: map_id.to_string(),
            setup,
            me,
            kind: MatchKind::Campaign,
            origin: Origin::Missions {
                campaign: campaign.to_string(),
            },
        });
    }

    fn load_save(&mut self, id: &str) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let attempt = (|| -> Result<(SaveMeta, saves::SaveBody)> {
            let meta = self.library.meta(id)?;
            let map = data.map(&meta.identity.map_id).ok_or_else(|| {
                anyhow::anyhow!("the map {} is not in this game", meta.identity.map_id)
            })?;
            let bytes = std::fs::read(&map.file.path)?;
            let here = data.match_identity(map, &bytes);
            self.library.read(id, Some(&here))
        })();
        let (meta, body) = match attempt {
            Ok(x) => x,
            Err(error) => {
                self.saves_status = format!("Cannot load: {error:#}");
                return;
            }
        };
        let kind = match meta.mode {
            saves::Mode::Skirmish => MatchKind::Skirmish,
            saves::Mode::Campaign => MatchKind::Campaign,
        };
        let origin = if self.flow.in_match() {
            self.flow.origin().clone()
        } else {
            Origin::MainMenu
        };
        let setup = body.world.setup.clone();
        self.last_match = Some(Rerun {
            map: meta.identity.map_id.clone(),
            setup: setup.clone(),
            me: meta.player,
            kind,
            origin: origin.clone(),
        });
        let start = PendingStart {
            kind,
            me: meta.player,
            map: meta.identity.map_id.clone(),
            networked: false,
            camera: body.camera,
            spectator: meta.player.is_none(),
            start_tick: 0,
            paused: false,
        };
        let job = Job::Match(Box::new(MatchRequest {
            map: meta.identity.map_id.clone(),
            setup,
            me: meta.player,
            resume: Some(body.world),
            session: None,
        }));
        self.saves_status.clear();
        self.start_loading(job, start, origin);
    }

    fn start_dev(&mut self, dev: DevStart) {
        let start = PendingStart {
            kind: MatchKind::Skirmish,
            me: None,
            map: dev.spec.map.clone(),
            networked: false,
            camera: dev.camera,
            spectator: true,
            start_tick: dev.tick,
            paused: !dev.spectate,
        };
        self.start_loading(Job::Dev(dev.spec), start, Origin::MainMenu);
    }

    fn finish_loading(&mut self, ctx: &mut Ctx<'_>) {
        let Some(loading) = &mut self.loading else {
            return;
        };
        let Some(job) = &mut loading.job else {
            return;
        };
        let Some(result) = job.poll() else {
            return;
        };
        loading.job = None;
        let start = loading.start.clone();
        let prepared = match result {
            Ok(p) => p,
            Err(error) => {
                tracing::error!(error = %format!("{error:#}"), "match failed to load");
                loading.error = Some(format!("{error:#}"));
                return;
            }
        };
        let Some(data) = self.data.clone() else {
            return;
        };
        let identity = data.map(&start.map).map_or_else(
            || MatchIdentity {
                map_id: start.map.clone(),
                map_digest: saves::digest(&prepared.map_bytes),
                content_digest: data.content_digest.clone(),
            },
            |m| data.match_identity(m, &prepared.map_bytes),
        );
        let ready = start.networked.then(|| Ready {
            content: data.network_identity(&prepared.map_bytes),
            build: canopy_net::wire::BUILD.into(),
        });
        self.mission_hud = MissionHud::new(prepared.objectives.clone());
        let started = Playing::start(
            prepared,
            Start {
                me: start.me,
                kind: start.kind,
                map_id: start.map.clone(),
                identity,
                start_tick: start.start_tick,
                ready,
                camera: start.camera,
                spectator: start.spectator,
            },
            ctx.renderer,
            ctx.gpu,
        );
        match started {
            Ok(playing) => {
                if start.paused {
                    playing.pause(true);
                }
                tracing::info!(
                    map = %start.map,
                    ms = loading.started.elapsed().as_millis() as u64,
                    "match started"
                );
                self.playing = Some(playing);
                self.apply(Intent::Loaded);
                self.apply_hud();
            }
            Err(error) => {
                if let Some(loading) = &mut self.loading {
                    loading.error = Some(format!("{error:#}"));
                }
            }
        }
    }

    // --- Per frame -----------------------------------------------------------------

    /// One frame: boot, network, loading, the match, the UI's actions, then the UI's
    /// models. Returns what the window should do.
    pub fn update(&mut self, ctx: &mut Ctx<'_>) -> Requests {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;
        self.host
            .ui
            .set_viewport(ctx.size[0], ctx.size[1], ctx.scale);
        self.host.set_scale(ctx.scale);
        self.view = [
            ctx.size[0] as f32 / ctx.scale.max(0.1),
            ctx.size[1] as f32 / ctx.scale.max(0.1),
        ];
        if cfg!(debug_assertions) && (now - self.last_reload).as_secs_f32() > 0.5 {
            self.last_reload = now;
            if let Err(error) = self.host.ui.reload_if_changed() {
                tracing::warn!(%error, "ui reload");
            }
        }
        if std::mem::take(&mut self.apply_all) {
            self.apply_settings(
                ctx,
                Changed {
                    graphics: true,
                    vsync: true,
                    window: true,
                    audio: true,
                    hud: true,
                    server: false,
                    any: false,
                },
            );
        }
        if std::mem::take(&mut self.clear_actors) {
            ctx.renderer.set_actors(&[]);
        }
        self.poll_boot();
        self.poll_network();
        self.finish_loading(ctx);
        self.update_match(dt, ctx);
        for action in self.host.ui.take_actions() {
            if !self.fading_out {
                self.action(&action, ctx);
            }
        }
        self.update_fade(dt);
        self.sync(ctx);
        self.audio.update(&canopy_audio::Frame {
            real_dt: dt,
            render_tick: 0.0,
            speed: 0.0,
            listener: canopy_audio::Listener::new(0.0, 0.0, 1.0, 0.0),
        });
        let time = (now - self.started).as_secs_f64();
        self.host.prepare(ctx.gpu, time);
        std::mem::take(&mut self.requests)
    }

    fn poll_boot(&mut self) {
        let Some(rx) = &self.boot else {
            return;
        };
        let mut done = None;
        while let Ok(message) = rx.try_recv() {
            match message {
                BootMessage::Stage(stage, value) => self.boot_stage = (stage, value),
                BootMessage::Done(result) => done = Some(*result),
            }
        }
        match done {
            Some(Ok(data)) => {
                self.boot = None;
                self.data = Some(Arc::new(data));
                self.apply(Intent::Booted);
                if let Some(dev) = self.dev.take() {
                    self.start_dev(dev);
                }
            }
            Some(Err(error)) => {
                self.boot = None;
                tracing::error!(error = %format!("{error:#}"), "cannot load the game's data");
                self.boot_error = Some(format!("{error:#}"));
            }
            None => {}
        }
    }

    fn poll_network(&mut self) {
        let screen = self.flow.screen().clone();
        if !matches!(screen, Screen::Multiplayer | Screen::Lobby) {
            return;
        }
        let Some(data) = self.data.clone() else {
            return;
        };
        match self.mp.poll(screen == Screen::Multiplayer) {
            Some(multiplayer::Outcome::EnterLobby) => self.apply(Intent::Open(Screen::Lobby)),
            Some(multiplayer::Outcome::Start { config, session }) => {
                if data.map(&config.map_id).is_none() {
                    self.mp.status = format!(
                        "The match uses the battlefield {}, which is not in your game.",
                        config.map_id
                    );
                    drop(session);
                    self.apply(Intent::Back);
                    return;
                }
                let me = session.player();
                let start = PendingStart {
                    kind: MatchKind::Multiplayer,
                    me,
                    map: config.map_id.clone(),
                    networked: true,
                    camera: None,
                    spectator: me.is_none(),
                    start_tick: 0,
                    paused: false,
                };
                self.last_match = None;
                let job = Job::Match(Box::new(MatchRequest {
                    map: config.map_id.clone(),
                    setup: config.setup(),
                    me,
                    resume: None,
                    session: Some(*session),
                }));
                self.start_loading(job, start, Origin::Multiplayer);
            }
            Some(multiplayer::Outcome::LeftLobby) if screen == Screen::Lobby => {
                self.apply(Intent::Back);
            }
            Some(multiplayer::Outcome::LeftLobby) => {}
            None => {}
        }
    }

    fn update_match(&mut self, dt: f32, ctx: &mut Ctx<'_>) {
        let Some(playing) = &mut self.playing else {
            return;
        };
        let size = ctx.size;
        let free = self.flow.overlays().is_empty() && !self.chat.open;
        playing.update(
            dt * 1000.0,
            size,
            f64::from(ctx.scale),
            &self.settings.keys,
            &self.catalog,
            free,
        );
        let events = playing.take_events();
        for event in events {
            match event {
                MatchEvent::MenuRequested => {
                    if self.flow.overlays().is_empty() {
                        self.menu_page = MenuPage::Main;
                        self.menu_status.clear();
                        self.apply(Intent::Show(Overlay::GameMenu));
                    }
                }
                MatchEvent::ChatRequested => {
                    self.chat.open = true;
                    let field = self.host.ui.find("match-overlay/chat/field");
                    self.host.ui.focus(field);
                }
                MatchEvent::Decided(outcome) => self.decided(Some(outcome), false),
                MatchEvent::Ended(wire) => {
                    if self.result.is_none() {
                        let outcome = canopy_sim::state::Outcome {
                            winner: wire.winner.map(canopy_content::placement::Owner::Player),
                            defeated: wire
                                .defeated
                                .iter()
                                .map(|p| canopy_content::placement::Owner::Player(*p))
                                .collect(),
                        };
                        self.decided(Some(outcome), false);
                    }
                }
                MatchEvent::Problem { title, body } => self.problem = Some((title, body)),
                MatchEvent::Chat(message) => {
                    let name = if message.name.is_empty() {
                        "Spectator".to_string()
                    } else {
                        message.name.clone()
                    };
                    self.chat
                        .push(format!("[b]{}[/b]: {}", plain(&name), plain(&message.text)));
                }
            }
        }
    }

    /// Shows the result (an outcome, or a surrender).
    fn decided(&mut self, outcome: Option<canopy_sim::state::Outcome>, surrendered: bool) {
        let (Some(playing), Some(data)) = (&self.playing, &self.data) else {
            return;
        };
        let save = playing.save();
        let next = data.map(&playing.map_id).and_then(|m| {
            let mission = m.mission.as_ref()?;
            let missions = data.missions(&mission.campaign);
            let i = missions.iter().position(|x| x.id() == m.id())?;
            missions.get(i + 1).map(|x| x.id().to_string())
        });
        let model = ResultModel::build(
            playing,
            data,
            outcome.as_ref(),
            save.as_ref().map(|b| &b.world),
            surrendered,
            next,
        );
        if model.won && playing.kind == MatchKind::Campaign {
            self.progress.completed.insert(playing.map_id.clone());
            if let Err(error) = self.progress.save(&self.paths.user) {
                tracing::warn!(%error, "cannot save campaign progress");
            }
        }
        if surrendered && !playing.networked {
            playing.pause(true);
        }
        self.result = Some(model);
        self.result_hidden = false;
        self.dialog = None;
        self.apply(Intent::Decided);
    }

    fn update_fade(&mut self, dt: f32) {
        if self.fading_out {
            self.fade = (self.fade + dt / FADE_OUT).min(1.0);
            if self.fade >= 1.0 {
                self.fading_out = false;
                if let Some(intent) = self.pending.take() {
                    self.apply(intent);
                }
            }
        } else if self.booted() || self.boot_error.is_some() {
            self.fade = (self.fade - dt / FADE_IN).max(0.0);
        } else {
            // The boot screen shows at once.
            self.fade = 0.0;
        }
    }

    // --- Actions -------------------------------------------------------------------

    fn action(&mut self, action: &Action, ctx: &mut Ctx<'_>) {
        let name = action.name.as_deref().unwrap_or("");
        let path = action.path.as_deref().unwrap_or("");
        let clicked = matches!(
            action.kind,
            ActionKind::Click { .. } | ActionKind::Hotkey { .. }
        );
        let selected = match action.kind {
            ActionKind::Selected(i) => Some(i),
            _ => None,
        };
        let Some(data) = self.data.clone() else {
            return;
        };
        if let Some(field) = name.strip_prefix("settings.") {
            if field == "done" {
                if clicked {
                    self.close_settings();
                }
                return;
            }
            let changed = self
                .settings_ui
                .action(action, &mut self.settings, &self.catalog);
            if changed.any || name == "settings.name" {
                self.settings_dirty = true;
            }
            self.apply_settings(ctx, changed);
            return;
        }
        match name {
            // Main menu.
            "menu.campaign" if clicked => self.navigate(Intent::Open(Screen::Campaign)),
            "menu.skirmish" if clicked => self.navigate(Intent::Open(Screen::Skirmish)),
            "menu.multiplayer" if clicked => self.navigate(Intent::Open(Screen::Multiplayer)),
            "menu.load" if clicked => self.open_saves(),
            "menu.settings" if clicked => self.open_settings(),
            "menu.exit" if clicked => self.apply(Intent::Quit),
            // Campaign.
            "campaign.play" if clicked => {
                if let Some(id) = action.index.and_then(|i| campaign::card_id(&data, i)) {
                    self.navigate(Intent::Open(Screen::Missions { campaign: id }));
                }
            }
            "campaign.back" | "missions.back" | "skirmish.back" | "mp.back" if clicked => {
                self.navigate(Intent::Back)
            }
            "missions.select" if clicked => {
                if let Some(i) = action.index {
                    self.mission_index = i;
                }
            }
            "missions.begin" if clicked => {
                if let Screen::Missions { campaign } = self.flow.screen().clone() {
                    let missions = data.missions(&campaign);
                    if self.progress.unlocked(&missions, self.mission_index)
                        && let Some(m) = missions.get(self.mission_index)
                    {
                        let id = m.id().to_string();
                        self.begin_mission(&campaign, &id);
                    }
                }
            }
            // Skirmish (the map list is shared with hosting a lobby).
            "skirmish.map" if clicked => {
                if let Some(i) = action.index {
                    if path.starts_with("multiplayer/") {
                        self.mp.selected_map = i;
                        self.mp.seats = 0;
                    } else {
                        self.skirmish.choose_map(&data, i);
                    }
                }
            }
            "skirmish.controller" | "skirmish.team" | "skirmish.race" | "skirmish.hero" => {
                if let (Some(slot), Some(option)) = (action.index, selected) {
                    let field = name.trim_start_matches("skirmish.");
                    self.skirmish.choose(&data, field, slot, option);
                }
            }
            "skirmish.start" if clicked => self.start_skirmish(),
            // Multiplayer.
            "mp.find" if clicked => {
                self.mp.tab = multiplayer::Tab::Find;
                self.mp.refresh();
            }
            "mp.create" if clicked => self.mp.tab = multiplayer::Tab::Create,
            "mp.refresh" if clicked => self.mp.refresh(),
            "mp.room" if clicked => self.mp.selected_room = action.index,
            "mp.name" => {
                if let ActionKind::TextChanged(text) | ActionKind::Submitted(text) = &action.kind {
                    self.settings.player_name =
                        text.chars().take(crate::settings::MAX_NAME).collect();
                    self.settings_dirty = true;
                }
            }
            "mp.slots" => {
                if let Some(i) = selected {
                    self.mp.seats = i;
                }
            }
            "mp.go" if clicked => {
                let name = self.settings.display_name();
                let maps: Vec<&crate::data::MapEntry> = data.skirmish_maps().collect();
                self.mp.go(&maps, &name);
            }
            "lobby.leave" if clicked => self.navigate(Intent::Back),
            "lobby.race" | "lobby.hero" => {
                if let Some(option) = selected {
                    self.mp.choose(name.trim_start_matches("lobby."), option);
                }
            }
            "lobby.start" if clicked => self.mp.start(),
            // Loading.
            "loading.back" if clicked => {
                let failed = self.loading.as_ref().is_some_and(|l| l.error.is_some());
                self.apply(if failed {
                    Intent::LoadFailed
                } else {
                    Intent::Back
                });
            }
            // Game menu.
            "gm.resume" if clicked => self.apply(Intent::Back),
            "gm.save" if clicked => {
                self.menu_page = MenuPage::Save;
                let default = self.playing.as_ref().map_or_else(String::new, |p| {
                    format!("{} {}", p.map_name, saves::format_time(saves::now_ms()))
                });
                widgets::text(&mut self.host.ui, "game-menu/save-page/name", &default);
                let field = self.host.ui.find("game-menu/save-page/name");
                self.host.ui.focus(field);
            }
            "gm.save-confirm" if clicked => self.save_game(),
            "gm.save-name" => {
                if matches!(action.kind, ActionKind::Submitted(_)) {
                    self.save_game();
                }
            }
            "gm.page-back" if clicked => self.menu_page = MenuPage::Main,
            "gm.load" if clicked => self.open_saves(),
            "gm.restart" | "gm.surrender" | "gm.exit" if clicked => {
                self.menu_status.clear();
                self.menu_page = match name {
                    "gm.restart" => MenuPage::Restart,
                    "gm.surrender" => MenuPage::Surrender,
                    _ => MenuPage::Exit,
                };
            }
            "gm.settings" if clicked => self.open_settings(),
            "gm.save-exit" if clicked => {
                self.save_game_named("Before leaving");
                self.apply(Intent::LeaveMatch);
            }
            "gm.leave" if clicked => self.apply(Intent::LeaveMatch),
            "gm.save-restart" | "gm.restart-now" if clicked => {
                if name == "gm.save-restart" {
                    self.save_game_named("Before restarting");
                }
                if let Some(rerun) = self.last_match.clone() {
                    self.start_rerun(rerun);
                }
            }
            "gm.surrender-now" if clicked => self.decided(None, true),
            // Saves.
            "saves.load" if clicked => {
                if let Some(meta) = action.index.and_then(|i| self.saves_list.get(i)).cloned() {
                    self.load_save(&meta.id);
                }
            }
            "saves.delete" if clicked => {
                if let Some(meta) = action.index.and_then(|i| self.saves_list.get(i)).cloned() {
                    self.ask(Dialog {
                        ask: Ask::DeleteSave(meta.id.clone()),
                        title: "Delete this save?".into(),
                        body: format!("“{}” is removed for good.", plain(&meta.name)),
                        confirm: "Delete".into(),
                        danger: true,
                        second: None,
                    });
                }
            }
            "saves.back" if clicked => self.apply(Intent::Back),
            // Dialog.
            "dialog.cancel" if clicked => {
                self.dialog = None;
                self.apply(Intent::Back);
            }
            "dialog.confirm" | "dialog.second" if clicked => {
                if let Some(dialog) = self.dialog.take() {
                    self.apply(Intent::Back);
                    self.answer(dialog.ask, name == "dialog.second");
                }
            }
            // Result and match overlay.
            "result.watch" if clicked => self.result_hidden = true,
            "result.leave" | "overlay.leave" if clicked => self.apply(Intent::LeaveMatch),
            "result.next" if clicked => {
                let next = self.result.as_ref().and_then(|r| r.next.clone());
                if let (Some(next), Origin::Missions { campaign }) =
                    (next, self.flow.origin().clone())
                {
                    self.begin_mission(&campaign, &next);
                }
            }
            "chat.field" => {
                if let ActionKind::Submitted(text) = &action.kind {
                    let text = text.trim();
                    if let (Some(p), false) = (&self.playing, text.is_empty()) {
                        p.chat(text);
                    }
                    self.chat.open = false;
                    widgets::text(&mut self.host.ui, "match-overlay/chat/field", "");
                    self.host.ui.focus(None);
                }
            }
            _ => {}
        }
    }

    fn ask(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.apply(Intent::Show(Overlay::Confirm));
    }

    fn answer(&mut self, ask: Ask, _second: bool) {
        match ask {
            Ask::DeleteSave(id) => {
                if let Err(error) = self.library.delete(&id) {
                    self.saves_status = format!("Cannot delete: {error:#}");
                }
                self.saves_list = self.library.list();
            }
        }
    }

    fn open_saves(&mut self) {
        self.saves_list = self.library.list();
        self.saves_status.clear();
        self.apply(Intent::Show(Overlay::Saves));
    }

    fn open_settings(&mut self) {
        self.settings_ui.opened();
        self.apply(Intent::Show(Overlay::Settings));
    }

    fn close_settings(&mut self) {
        if self.settings_ui.capturing.take().is_some() {
            return;
        }
        self.save_settings();
        self.apply(Intent::Back);
    }

    fn save_game(&mut self) {
        let name = self
            .host
            .ui
            .find("game-menu/save-page/name")
            .and_then(|n| self.host.ui.text(n).map(str::to_owned))
            .unwrap_or_default();
        self.save_game_named(&name);
        self.menu_page = MenuPage::Main;
    }

    fn save_game_named(&mut self, name: &str) {
        let Some(playing) = &self.playing else {
            return;
        };
        let Some(mode) = playing.kind.save_mode() else {
            self.menu_status = "Multiplayer matches cannot be saved.".into();
            return;
        };
        let Some(body) = playing.save() else {
            self.menu_status = "The match did not answer; nothing was saved.".into();
            return;
        };
        let result = self.library.write(
            &NewSave {
                name,
                mode,
                map_name: &playing.map_name,
                identity: &playing.identity,
                player: playing.me,
                body: &body,
            },
            None,
        );
        self.menu_status = match result {
            Ok(meta) => format!("Saved “{}”.", plain(&meta.name)),
            Err(error) => format!("Cannot save: {error:#}"),
        };
    }

    /// Applies changed settings to the renderer, window, audio and HUD.
    fn apply_settings(&mut self, ctx: &mut Ctx<'_>, changed: Changed) {
        let g = &self.settings.graphics;
        if changed.graphics {
            ctx.renderer.set_quality(ctx.gpu, g.quality(ctx.size));
            ctx.renderer.shadows = g.shadows;
        }
        if changed.vsync {
            self.requests.vsync = Some(g.vsync);
        }
        if changed.window
            && let Some(window) = ctx.window
        {
            let fullscreen = g.window == WindowMode::Fullscreen;
            if fullscreen != window.fullscreen().is_some() {
                window.set_fullscreen(
                    fullscreen.then_some(winit::window::Fullscreen::Borderless(None)),
                );
            }
        }
        if changed.audio {
            self.audio.apply_settings(self.settings.audio);
        }
        if changed.hud {
            self.apply_hud();
        }
        if changed.server && matches!(self.flow.screen(), Screen::Multiplayer) {
            self.mp.open(self.settings.multiplayer.server.as_deref());
        }
    }

    /// The HUD size setting on the in-match HUD (`hud.json`'s dock).
    fn apply_hud(&mut self) {
        if let Some(dock) = self.host.ui.find("hud/dock") {
            self.host
                .ui
                .set_scale(dock, Some(self.settings.hud.scale()));
        }
    }

    /// Keeps the render quality's resolution cap in step with the window size.
    pub fn resized(&mut self, renderer: &mut Renderer, gpu: &Gpu, size: [u32; 2]) {
        if self.settings.graphics.resolution.is_some() {
            renderer.set_quality(gpu, self.settings.graphics.quality(size));
        }
    }

    // --- Models into the UI ----------------------------------------------------------

    fn sync(&mut self, ctx: &mut Ctx<'_>) {
        let screen = self.flow.screen().clone();
        let top = self.flow.top();
        let in_match = self.flow.in_match();
        let mission = self
            .playing
            .as_ref()
            .is_some_and(|p| p.newest().is_some_and(|s| s.mission.is_some()));
        let shows = [
            ("main-menu", screen == Screen::MainMenu),
            ("campaign", screen == Screen::Campaign),
            ("missions", matches!(screen, Screen::Missions { .. })),
            ("skirmish", screen == Screen::Skirmish),
            ("multiplayer", screen == Screen::Multiplayer),
            ("lobby", screen == Screen::Lobby),
            ("loading", matches!(screen, Screen::Loading | Screen::Boot)),
            ("match-overlay", in_match),
            ("mission-hud", in_match && mission),
            ("game-menu", top == Some(Overlay::GameMenu)),
            (
                "result",
                top == Some(Overlay::Result) && !self.result_hidden,
            ),
            ("settings", top == Some(Overlay::Settings)),
            ("saves", top == Some(Overlay::Saves)),
            ("dialog", top == Some(Overlay::Confirm)),
            ("fade", true),
        ];
        for (name, show) in shows {
            self.host.ui.show(name, show);
        }
        if let Some(fade) = self.host.ui.find("fade") {
            self.host.ui.set_opacity(fade, self.fade);
        }
        let data = self.data.clone();
        let gpu = ctx.gpu;
        let t = (Instant::now() - self.started).as_secs_f32();
        match &screen {
            Screen::Boot => self.show_boot(),
            Screen::MainMenu => menu::animate(&mut self.host, t, self.view),
            Screen::Campaign => {
                if let Some(data) = &data {
                    campaign::show_cards(&mut self.host, gpu, data, &self.progress);
                }
            }
            Screen::Missions { campaign } => {
                if let Some(data) = &data {
                    campaign::show_missions(
                        &mut self.host,
                        gpu,
                        data,
                        &self.progress,
                        campaign,
                        self.mission_index,
                    );
                }
            }
            Screen::Skirmish => {
                if let Some(data) = &data {
                    let name = self.settings.player_name.trim().to_string();
                    self.skirmish
                        .show(&mut self.host, gpu, data, &self.palette, &name);
                }
            }
            Screen::Multiplayer => {
                if let Some(data) = &data {
                    self.mp
                        .show_list(&mut self.host, gpu, data, &self.palette, false);
                }
            }
            Screen::Lobby => {
                if let Some(data) = &data {
                    self.mp.show_lobby(&mut self.host, gpu, data, &self.palette);
                }
            }
            Screen::Loading => self.show_loading(gpu),
            Screen::Match => {}
        }
        if let (Some(playing), true) = (&self.playing, in_match) {
            match_ui::show_overlay(&mut self.host, playing, &self.chat, self.problem.as_ref());
            if let Some(data) = &data {
                let mission = playing.newest().and_then(|s| s.mission.clone());
                let dt = 1.0 / 60.0;
                self.mission_hud.show(
                    &mut self.host,
                    gpu,
                    data,
                    mission.as_ref(),
                    dt,
                    self.view[1],
                );
            }
            if top == Some(Overlay::GameMenu) {
                match_ui::show_game_menu(
                    &mut self.host,
                    playing,
                    self.menu_page,
                    &self.menu_status,
                );
            }
            if let (Some(result), Some(Overlay::Result)) = (&self.result, top) {
                result.show(&mut self.host, &self.palette, playing.kind);
            }
        }
        match top {
            Some(Overlay::Settings) => {
                self.settings_ui
                    .show(&mut self.host, &self.settings, &self.catalog)
            }
            Some(Overlay::Saves) => self.show_saves(),
            Some(Overlay::Confirm) => self.show_dialog(),
            _ => {}
        }
    }

    fn show_boot(&mut self) {
        let ui = &mut self.host.ui;
        widgets::visible(ui, "loading/art-view", true);
        widgets::visible(ui, "loading/panel-view", false);
        widgets::visible(ui, "loading/art-view/copy", true);
        widgets::visible(ui, "loading/art-view/copy/crest", false);
        widgets::text(ui, "loading/art-view/copy/chapter", "");
        widgets::text(ui, "loading/art-view/copy/title", "Under the Canopy");
        widgets::text(ui, "loading/art-view/copy/briefing", "");
        widgets::visible(ui, "loading/art-view/back", false);
        widgets::text(ui, "loading/art-view/tip", "");
        match &self.boot_error {
            Some(error) => {
                widgets::visible(ui, "loading/art-view/error", true);
                widgets::text(
                    ui,
                    "loading/art-view/error",
                    &format!("The game could not start: {}", plain(error)),
                );
                widgets::text(ui, "loading/art-view/stage", "");
            }
            None => {
                widgets::visible(ui, "loading/art-view/error", false);
                widgets::text(ui, "loading/art-view/stage", self.boot_stage.0.label());
                if let Some(bar) = ui.find("loading/art-view/progress") {
                    ui.set_value(bar, self.boot_stage.1);
                }
            }
        }
    }

    fn show_loading(&mut self, gpu: &Gpu) {
        let Some(loading) = &self.loading else {
            return;
        };
        let data = self.data.clone();
        let entry = data.as_ref().and_then(|d| d.map(&loading.start.map));
        let presentation = entry
            .and_then(|m| m.mission.as_ref())
            .and_then(|m| m.presentation.as_ref());
        let art = presentation
            .and_then(|p| p.loading_background.as_deref())
            .and_then(|id| data.as_ref().and_then(|d| widgets::asset_image(d, id)));
        let (stage, progress, error) = (
            loading.job.as_ref().map(|j| (j.stage, j.progress)),
            loading.job.as_ref().map_or(1.0, |j| j.progress),
            loading.error.clone(),
        );
        let mission_title = entry
            .and_then(|m| m.mission.as_ref())
            .map(|m| m.title.clone());
        let map_name = entry.map(|m| m.name.clone()).unwrap_or_default();
        let tip_list = presentation
            .and_then(|p| p.tips.clone())
            .unwrap_or_default();
        let elapsed = loading.started.elapsed().as_secs();
        let art_key = match (art, &data) {
            (Some(path), Some(d)) => self.host.pack_image(gpu, &d.pack, &path),
            _ => None,
        };
        let ui = &mut self.host.ui;
        if let Some(node) = ui.find("loading/art-view/art") {
            let key = art_key.or_else(|| Some(ui.image_key("menu-aftermath")));
            ui.set_image(node, key);
        }
        widgets::visible(ui, "loading/art-view/back", true);
        widgets::text(
            ui,
            "loading/art-view/copy/chapter",
            &presentation
                .and_then(|p| p.chapter.clone())
                .unwrap_or_else(|| {
                    if mission_title.is_some() {
                        String::new()
                    } else {
                        map_name.clone()
                    }
                }),
        );
        widgets::text(
            ui,
            "loading/art-view/copy/title",
            mission_title
                .as_deref()
                .unwrap_or("Preparing the battlefield"),
        );
        widgets::text(
            ui,
            "loading/art-view/copy/briefing",
            &presentation
                .and_then(|p| p.briefing.clone())
                .unwrap_or_default(),
        );
        let tip = if tip_list.is_empty() {
            String::new()
        } else {
            tip_list[(elapsed / 9) as usize % tip_list.len()].clone()
        };
        widgets::text(ui, "loading/art-view/tip", &tip);
        // Missions load over their art; other matches show the old plain panel.
        let mission = mission_title.is_some();
        widgets::visible(ui, "loading/art-view", mission);
        widgets::visible(ui, "loading/panel-view", !mission);
        let label = stage.map_or("Starting the match", |(s, _)| s.label());
        widgets::text(
            ui,
            "loading/panel-view/title",
            if error.is_some() {
                "The battlefield could not be loaded"
            } else {
                "Preparing the battlefield"
            },
        );
        widgets::text(
            ui,
            "loading/panel-view/stage",
            if error.is_some() { "" } else { label },
        );
        widgets::visible(ui, "loading/panel-view/progress", error.is_none());
        if let Some(bar) = ui.find("loading/panel-view/progress") {
            ui.set_value(bar, progress);
        }
        widgets::text(
            ui,
            "loading/panel-view/detail",
            &error.as_deref().map_or_else(
                || "The match starts when preparation is complete.".to_string(),
                plain,
            ),
        );
        match error {
            Some(error) => {
                widgets::visible(ui, "loading/art-view/error", true);
                widgets::text(
                    ui,
                    "loading/art-view/error",
                    &format!("The battlefield could not be loaded: {}", plain(&error)),
                );
                widgets::text(ui, "loading/art-view/stage", "");
            }
            None => {
                widgets::visible(ui, "loading/art-view/error", false);
                widgets::text(ui, "loading/art-view/stage", label);
                if let Some(bar) = ui.find("loading/art-view/progress") {
                    ui.set_value(bar, progress);
                }
            }
        }
    }

    fn show_saves(&mut self) {
        let ui = &mut self.host.ui;
        if let Some(list) = ui.find("saves/list") {
            ui.set_count(list, self.saves_list.len());
        }
        for (i, meta) in self.saves_list.iter().enumerate() {
            let base = format!("saves/list/{i}");
            widgets::text(ui, &format!("{base}/name"), &plain(&meta.name));
            let mode = match meta.mode {
                saves::Mode::Skirmish => "Skirmish",
                saves::Mode::Campaign => "Campaign",
            };
            widgets::text(
                ui,
                &format!("{base}/meta"),
                &format!(
                    "{} · {mode} · {} · {}",
                    plain(&meta.map_name),
                    saves::format_time(meta.saved_at),
                    match_ui_clock(meta.tick)
                ),
            );
        }
        widgets::visible(ui, "saves/empty", self.saves_list.is_empty());
        widgets::text(ui, "saves/status", &self.saves_status);
        widgets::visible(ui, "saves/status", !self.saves_status.is_empty());
    }

    fn show_dialog(&mut self) {
        let Some(d) = &self.dialog else {
            return;
        };
        let ui = &mut self.host.ui;
        widgets::text(ui, "dialog/title", &d.title);
        widgets::text(ui, "dialog/body", &d.body);
        widgets::text(ui, "dialog/confirm", &d.confirm);
        if let Some(n) = ui.find("dialog/confirm") {
            // Danger answers read red on the ghost style; the primary style otherwise.
            ui.set_color(
                n,
                Some(if d.danger {
                    canopy_ui::Color::rgba(0x7f, 0x1d, 0x1d, 0xe0)
                } else {
                    canopy_ui::Color::rgba(0xe8, 0xe0, 0xd0, 0xff)
                }),
            );
        }
        widgets::visible(ui, "dialog/second", d.second.is_some());
        if let Some(second) = &d.second {
            widgets::text(ui, "dialog/second", second);
        }
    }

    // --- Drawing -------------------------------------------------------------------

    /// Records the frame: the match (when one shows) into `output`, then the UI into
    /// `ui_view` (a non-sRGB view of the same target).
    pub fn render(
        &mut self,
        ctx: &mut Ctx<'_>,
        output: &wgpu::TextureView,
        ui_view: &wgpu::TextureView,
    ) -> Option<canopy_render::FrameReport> {
        let now = Instant::now();
        let seconds = (now - self.started).as_secs_f32();
        let report = match (&mut self.playing, self.flow.in_match()) {
            (Some(playing), true) => {
                Some(playing.render(ctx.renderer, ctx.gpu, output, ctx.size, seconds, now))
            }
            _ => None,
        };
        let mut encoder = ctx
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ui") });
        let clear = report.is_none().then_some(wgpu::Color {
            r: 0.0306,
            g: 0.0509,
            b: 0.0588,
            a: 1.0,
        });
        self.host.render(&mut encoder, ui_view, clear);
        ctx.gpu.queue.submit([encoder.finish()]);
        report
    }
}

fn match_ui_clock(tick: u32) -> String {
    let seconds = tick / 40;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
