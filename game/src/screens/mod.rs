//! The game's screens and app flow: boot, main menu, campaign, skirmish, multiplayer,
//! loading, the match with its menu and result, settings, saves and feedback.
//!
//! [`Game`] owns the one UI ([`ui_host::UiHost`]: menus, overlays and the in-match HUD),
//! the audio, the flow state machine ([`flow::Flow`]), the models of every screen and the
//! running match ([`in_match::Playing`], the play module's `InMatch`). The window
//! (`app.rs`) forwards events to [`Game::window_event`], calls [`Game::update`] once per
//! frame and [`Game::render`] to draw. Layouts live in `config/ui/layouts/` (one document
//! per screen), reproducing the old game's menus.

pub mod campaign;
pub mod capture;
pub mod feedback;
pub mod flow;
pub mod in_match;
pub mod loading;
pub mod match_ui;
pub mod menu;
pub mod multiplayer;
#[cfg(test)]
mod net_tests;
pub mod releases;
pub mod replay_ui;
pub mod replays;
pub mod settings_ui;
pub mod skirmish;
pub mod ui_host;
pub mod widgets;

use crate::play::{keys::Keymap, run::Host, run::Options as PlayOptions};
use crate::{
    data::{BootStage, GameData},
    paths::Paths,
    saves::{self, MatchIdentity, NewSave, SaveLibrary, SaveMeta},
    settings::{Chord, Settings, keys},
    sim::Ready,
    world::MatchRequest,
};
use anyhow::Result;
use canopy_render::{Gpu, Renderer};
use canopy_ui::{Action, ActionKind};
use flow::{Effect, Flow, Intent, Origin, Overlay, Screen};
use in_match::{MatchEvent, MatchKind, Playing, Start};
use loading::{DevSpec, Job, LoadJob};
use match_ui::{MenuPage, MissionHud, ResultModel};
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
    /// Open the editor (the main menu's Editor entry).
    pub editor: bool,
    pub vsync: Option<bool>,
}

/// A match started from the development options (`--map`, `--spectate`).
#[derive(Debug, Clone)]
pub struct DevStart {
    pub spec: DevSpec,
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
    /// Control groups of a loaded save.
    groups: Option<Vec<Vec<u32>>>,
    start_tick: u32,
    /// Palette colours by player number (the lobby's or a replay's), when not their own.
    colors: Option<Vec<usize>>,
    /// The replay this load watches.
    watching: Option<in_match::Watching>,
}

/// A match that can be started again (restart).
#[derive(Debug, Clone)]
struct Rerun {
    map: String,
    setup: canopy_sim::MatchSetup,
    me: Option<u8>,
    kind: MatchKind,
    origin: Origin,
    /// Palette colours by player number, when not their own (skirmish).
    colors: Option<Vec<usize>>,
    /// A campaign mission's start (tech, carried heroes, level cap, variables).
    campaign: Option<canopy_sim::campaign::CampaignStart>,
    /// The mission's autoplay plan plays it (Play with AI); a restart plays it so again.
    autoplay: bool,
}

struct Loading {
    job: Option<LoadJob>,
    start: PendingStart,
    error: Option<String>,
    started: Instant,
    /// A campaign mission waits on its loading screen once loaded, until the player
    /// clicks or presses a key (docs/campaign.md).
    hold: bool,
    prepared: Option<crate::world::Prepared>,
}

/// Seconds a won campaign mission shows its outcome before the campaign moves on.
const VICTORY_SECONDS: f32 = 2.5;

/// A question with up to two answers besides Cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    DeleteSave(String),
    DeleteReplay(String),
    /// Clears a campaign's progress, by campaign id.
    ResetCampaign(String),
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
    /// The player's progress through each campaign, by campaign id.
    progress: std::collections::BTreeMap<String, crate::campaign::Progress>,
    /// The campaign step being played (a card or a mission), if any.
    campaign_play: Option<crate::campaign::Playing>,
    /// Steps run from now on play with their missions' autoplay plans (Play with AI);
    /// until the campaign screen shows again.
    campaign_autoplay: bool,
    /// The story, chapter or cinematic card on screen.
    card: Option<campaign::Card>,
    /// The campaign screen's list, which its buttons index.
    campaign_entries: Vec<campaign::Entry>,
    /// Frames the campaign screen keeps its list scrolled to the newest step.
    scroll_steps: u32,
    /// A won campaign mission moves on at this moment.
    advance_at: Option<Instant>,
    flow: Flow,
    pub host: UiHost,
    palette: Palette,
    data: Option<Arc<GameData>>,
    boot: Option<mpsc::Receiver<BootMessage>>,
    boot_stage: (BootStage, f32),
    boot_error: Option<String>,
    dev: Option<DevStart>,
    skirmish: Skirmish,
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
    problem: Option<(String, String)>,
    mission_hud: MissionHud,
    last_match: Option<Rerun>,
    pending: Option<Intent>,
    fade: f32,
    fading_out: bool,
    /// The one audio output: interface, music and the match's effect sounds.
    pub audio: canopy_audio::Audio,
    /// The game's sound events (the base's and every campaign's) are bound (D97).
    sounds_bound: bool,
    /// What the match follows of the settings (shortcuts, health bars).
    play_options: PlayOptions,
    /// The cursor last set on the window.
    cursor: Option<winit::window::CursorIcon>,
    /// Desktop updates (D43): checked when the main menu comes up.
    updater: crate::update::Updater,
    /// What's new and the update details.
    releases: releases::Releases,
    /// The window's width in logical pixels (the compact HUD's dock scale).
    hud_width: f32,
    modifiers: ModifiersState,
    started: Instant,
    last_frame: Instant,
    last_reload: Instant,
    requests: Requests,
    view: [f32; 2],
    apply_all: bool,
    clear_actors: bool,
    /// Test in game: the game's own data while a match plays the editor's.
    testing: Option<Arc<GameData>>,
    /// The feedback form, its screenshot and the reports going out (D59).
    feedback: feedback::FeedbackForm,
    grab: crate::feedback::grab::Grab,
    courier: crate::feedback::Courier,
    /// Where feedback goes instead of the multiplayer server (captures).
    feedback_server: Option<String>,
    /// The replays folder and screen, and the viewer's controls (D60).
    replay_library: crate::replay::library::Library,
    replays: replays::Replays,
    replay_ui: replay_ui::ReplayUi,
    /// A replay the command line asks to watch once the game has loaded.
    boot_replay: Option<std::path::PathBuf>,
    /// The local player surrendered the running match.
    surrendered: bool,
    /// The result screen's replay was saved under this name.
    replay_saved: Option<Result<String, String>>,
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
    pub fn new(paths: Paths, settings: Settings, gpu: &Gpu, dev: Option<DevStart>) -> Result<Self> {
        let pack = GameData::open_pack(&paths.data)?;
        crate::text::install(crate::text::load(
            &paths.data,
            canopy_content::text::SOURCE_LANGUAGE,
        ));
        let host = UiHost::new(&paths.data, gpu, &pack)?;
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
        let catalog = keys::catalog();
        let play_options = play_options(&settings, &catalog);
        let now = Instant::now();
        let updater = crate::update::Updater::new(Box::new(crate::update::remote::Remote::new(
            crate::update::history::installed(),
            paths.cache.join("updates"),
        )));
        let releases = releases::Releases::new(&paths.data);
        let mut game = Self {
            updater,
            releases,
            palette: Palette::load(&paths.data),
            library: SaveLibrary::new(&paths.user),
            replay_library: crate::replay::library::Library::new(&paths.replays),
            replays: replays::Replays::default(),
            replay_ui: replay_ui::ReplayUi::default(),
            boot_replay: None,
            surrendered: false,
            replay_saved: None,
            courier: crate::feedback::Courier::new(&paths.user),
            progress: std::collections::BTreeMap::new(),
            campaign_play: None,
            campaign_autoplay: false,
            card: None,
            campaign_entries: Vec::new(),
            scroll_steps: 0,
            advance_at: None,
            catalog,
            play_options,
            cursor: None,
            hud_width: 0.0,
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
            problem: None,
            mission_hud: MissionHud::default(),
            last_match: None,
            pending: None,
            fade: 1.0,
            fading_out: false,
            audio,
            sounds_bound: false,
            modifiers: ModifiersState::empty(),
            started: now,
            last_frame: now,
            last_reload: now,
            requests: Requests::default(),
            view: [1.0, 1.0],
            apply_all: true,
            clear_actors: false,
            testing: None,
            feedback: feedback::FeedbackForm::default(),
            grab: crate::feedback::grab::Grab::default(),
            feedback_server: None,
        };
        // The editor edits the repository; installed games show its entry disabled.
        if game.paths.repository().is_none()
            && let Some(entry) = game.host.ui.find("main-menu/editor")
        {
            game.host.ui.set_enabled(entry, false);
        }
        Ok(game)
    }

    /// Watches the replay at `path` once the game has loaded (`canopy replay <file>`).
    pub fn watch_on_boot(&mut self, path: std::path::PathBuf) {
        self.boot_replay = Some(path);
    }

    /// Updates never reach the network (benchmarks, captures).
    pub fn offline_updates(&mut self) {
        self.updater = crate::update::Updater::new(Box::new(crate::update::remote::Offline));
        self.releases.gallery.offline = true;
    }

    /// Test in game (docs/editor.md): a skirmish against computer players on `map` (the
    /// last map played, or the first skirmish map) with `content` in place of the game's
    /// own until the match is left; with `encounter`, that unit tries its abilities near
    /// the player's start instead (no computer players).
    pub fn test_in_game(
        &mut self,
        content: Arc<canopy_content::Content>,
        map: Option<String>,
        map_package: Option<std::path::PathBuf>,
        encounter: Option<String>,
        autoplay: bool,
    ) -> Result<(), String> {
        let data = self.data.clone().ok_or("the game is still loading")?;
        let mut testing = data.testing(content);
        // A map with unsaved edits plays from the editor's copy of its package.
        if let (Some(id), Some(path)) = (&map, map_package) {
            testing.add_test_map(id, path).map_err(|e| e.to_string())?;
        }
        let map = map
            .filter(|m| testing.map(m).is_some_and(|e| e.playable))
            .or_else(|| self.last_match.as_ref().map(|r| r.map.clone()))
            .or_else(|| data.skirmish_maps().next().map(|m| m.id().to_string()))
            .ok_or("no playable map")?;
        self.testing = Some(data.clone());
        self.data = Some(Arc::new(testing));
        self.start_dev(DevStart {
            spec: DevSpec {
                map,
                hour: None,
                players: Some(2),
                biome: None,
                crowd: None,
                colors: None,
                races: None,
                human: Some(0),
                encounter,
                autoplay,
            },
            tick: 0,
            camera: None,
        });
        Ok(())
    }

    /// Test in game from the editor at a campaign's step (Play from here): the campaign
    /// package at `package` (the editor's copy, unsaved edits included) stands in for its
    /// campaign and step `step` plays as a replay does, back to the campaign screen after.
    pub fn test_campaign(
        &mut self,
        content: Arc<canopy_content::Content>,
        package: &std::path::Path,
        step: usize,
    ) -> Result<(), String> {
        let data = self.data.clone().ok_or("the game is still loading")?;
        let mut testing = data.testing(content);
        let id = testing
            .add_test_campaign(package)
            .map_err(|e| e.to_string())?;
        if testing
            .campaign(&id)
            .is_none_or(|c| step >= c.campaign.steps.len())
        {
            return Err(format!("the campaign {id} has no step {step}"));
        }
        self.testing = Some(data.clone());
        self.data = Some(Arc::new(testing));
        self.run_step(&id, step, false);
        Ok(())
    }

    /// The test match was left: the game's own data is back. True once, when it happens.
    pub fn test_finished(&mut self) -> bool {
        if self.testing.is_none() || !matches!(self.flow.screen(), Screen::MainMenu) {
            return false;
        }
        self.data = self.testing.take();
        true
    }

    /// The window's cursor changed behind the game's back (the editor): set it again.
    pub fn forget_cursor(&mut self) {
        self.cursor = None;
    }

    /// Whether the game is ready for the menus (data loaded).
    pub fn booted(&self) -> bool {
        self.data.is_some()
    }

    /// Starts a skirmish on battlefield `map` (its index in the list) with the human in
    /// the first seat and computers in the rest, as the skirmish screen would: for
    /// benchmarks of the match as the window plays it. False before boot.
    pub fn start_benchmark_skirmish(&mut self, map: usize) -> bool {
        let Some(data) = self.data.clone() else {
            return false;
        };
        self.apply(Intent::Open(Screen::Skirmish));
        self.skirmish.choose_map(&data, map);
        self.skirmish.set_human(0, true);
        for slot in &mut self.skirmish.slots {
            slot.team = None;
        }
        self.start_skirmish();
        true
    }

    /// Whether a match is on screen.
    pub fn in_match(&self) -> bool {
        self.flow.in_match()
    }

    // --- Input ---------------------------------------------------------------------

    /// A window event: the UI first, then the match.
    pub fn window_event(&mut self, window: Option<&Window>, event: &WindowEvent) {
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
        if let (Some(playing), true) = (&mut self.playing, self.flow.in_match()) {
            if self.flow.overlays().is_empty() {
                // A replay's keys: Space plays and pauses, the arrows skip 10 s.
                if playing.watching.is_some()
                    && !consumed
                    && !self.host.ui.wants_keyboard()
                    && let WindowEvent::KeyboardInput { event: key, .. } = event
                    && let winit::keyboard::PhysicalKey::Code(code) = key.physical_key
                {
                    use winit::keyboard::KeyCode;
                    let skip = replay_ui::SKIP_TICKS as i64;
                    let handled = matches!(
                        code,
                        KeyCode::Space | KeyCode::ArrowLeft | KeyCode::ArrowRight
                    );
                    if handled && key.state == winit::event::ElementState::Pressed {
                        match code {
                            KeyCode::Space if !key.repeat => self.replay_ui.toggle(playing),
                            KeyCode::ArrowLeft => self.replay_ui.skip(playing, -skip),
                            KeyCode::ArrowRight => self.replay_ui.skip(playing, skip),
                            _ => {}
                        }
                    }
                    if handled {
                        return;
                    }
                }
                playing.window_event(window, event, &self.host.ui, consumed);
                return;
            }
            // An overlay holds the input: the match only follows releases and focus.
            playing.window_event(window, event, &self.host.ui, true);
        }
        // A card or a loaded mission moves on with any key but Escape.
        if !consumed
            && let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == winit::event::ElementState::Pressed
            && !key.repeat
            && key.physical_key
                != winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::Escape)
            && self.flow.overlays().is_empty()
        {
            match self.flow.screen() {
                Screen::Step { .. } => {
                    self.card_next();
                    return;
                }
                Screen::Loading if self.loading.as_ref().is_some_and(|l| l.prepared.is_some()) => {
                    self.begin_loaded();
                    return;
                }
                _ => {}
            }
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
            Some(Overlay::Releases) => self.close_releases(),
            Some(Overlay::Feedback) => self.close_feedback(),
            Some(Overlay::GameMenu) if self.menu_page != MenuPage::Main => {
                self.menu_page = MenuPage::Main;
            }
            Some(_) => self.apply(Intent::Back),
            None => {
                if matches!(self.flow.screen(), Screen::Loading) {
                    self.apply(Intent::Back);
                } else if matches!(self.flow.screen(), Screen::Step { .. }) {
                    // Escape skips a card or a story, as Skip does.
                    self.finish_step();
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
                    name: &tr!("menu.autosave"),
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
        self.keep_last_match();
        self.playing = None;
        self.surrendered = false;
        self.replay_saved = None;
        self.host.ui.show("hud", false);
        self.loading = None;
        self.result = None;
        self.result_hidden = false;
        self.problem = None;
        self.mission_hud = MissionHud::default();
        self.menu_page = MenuPage::Main;
        self.menu_status.clear();
        self.clear_actors = true;
        self.advance_at = None;
    }

    /// Hooks when a screen comes up.
    fn entered(&mut self, screen: &Screen) {
        let Some(data) = self.data.clone() else {
            return;
        };
        match screen {
            // Desktop apps check for updates on entering the main menu (old behaviour).
            Screen::MainMenu => {
                self.updater.menu_entered();
                self.play_menu_music(&data);
            }
            Screen::Skirmish => {
                if self.skirmish.map_index(&data).is_none() {
                    self.skirmish.choose_map(&data, 0);
                }
            }
            Screen::Replays => {
                self.replays.status.clear();
                self.replays.refresh(&self.replay_library);
            }
            Screen::Multiplayer => {
                self.mp.open(self.settings.multiplayer.server.as_deref());
                widgets::text(
                    &mut self.host.ui,
                    "multiplayer/name",
                    &self.settings.player_name,
                );
            }
            Screen::Campaigns => {
                for c in &data.campaigns {
                    self.progress.insert(
                        c.id().to_string(),
                        crate::campaign::Progress::load(&self.paths, c.id()),
                    );
                }
            }
            Screen::Campaign { campaign } => {
                let progress = crate::campaign::Progress::load(&self.paths, campaign);
                self.progress.insert(campaign.clone(), progress);
                self.campaign_play = None;
                self.campaign_autoplay = false;
                self.card = None;
                self.scroll_steps = 20;
                self.audio.stop_music(1.5);
            }
            Screen::Step { campaign, step } => {
                self.card = Some(campaign::Card::new(campaign, *step));
                if let Some(c) = data.campaign(campaign)
                    && let Some(s) = c.campaign.steps.get(*step)
                {
                    self.play_step_music(&data, s);
                }
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
        // Autoplay begins at once: it is for watching a mission play, fast.
        let hold = start.kind == MatchKind::Campaign
            && self.campaign_play.as_ref().is_some_and(|p| {
                !p.autoplay
                    || !self
                        .data
                        .as_ref()
                        .is_some_and(|d| d.map(&start.map).is_some_and(|m| m.autoplay))
            });
        self.loading = Some(Loading {
            job: Some(LoadJob::spawn(self.paths.clone(), data, job)),
            start,
            error: None,
            started: Instant::now(),
            hold,
            prepared: None,
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
            groups: None,
            start_tick: 0,
            colors: rerun.colors.clone(),
            watching: None,
        };
        // A restart plays the same scenario again: map, players, teams and seed (old
        // "Restart scenario"); new matches bring their own seed.
        let setup = rerun.setup.clone();
        let job = Job::Match(Box::new(MatchRequest {
            map: rerun.map.clone(),
            setup: setup.clone(),
            me: rerun.me,
            resume: None,
            session: None,
            colors: rerun.colors.clone(),
            replay: None,
            map_path: None,
            campaign: rerun.campaign.clone(),
            autoplay: rerun.autoplay,
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
            colors: self.skirmish.colors(),
            campaign: None,
            autoplay: false,
        });
    }

    // --- Campaigns ------------------------------------------------------------------

    /// The campaign's progress as last read.
    fn campaign_progress(&self, campaign: &str) -> crate::campaign::Progress {
        self.progress.get(campaign).cloned().unwrap_or_default()
    }

    /// Continue on the campaign screen: the next step, straight through.
    fn continue_campaign(&mut self, campaign: &str) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(c) = data.campaign(campaign) else {
            return;
        };
        let next = self.campaign_progress(campaign).next(&c.campaign);
        if next < c.campaign.steps.len() {
            self.run_step(campaign, next, true);
        }
    }

    /// Plays step `step`: a mission loads, a story, chapter or cinematic shows its card.
    /// `straight`: it is the next step, and finishing it moves the campaign on.
    fn run_step(&mut self, campaign: &str, step: usize, straight: bool) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(c) = data.campaign(campaign).cloned() else {
            return;
        };
        let Some(kind) = c.campaign.steps.get(step) else {
            return;
        };
        self.campaign_play = Some(crate::campaign::Playing {
            campaign: campaign.to_string(),
            step,
            straight,
            autoplay: self.campaign_autoplay,
        });
        let screen = Screen::Step {
            campaign: campaign.to_string(),
            step,
        };
        match kind {
            canopy_content::campaign::Step::Mission(_) => self.begin_mission(&data, &c, step),
            _ if matches!(self.flow.screen(), Screen::Campaign { .. }) => {
                self.navigate(Intent::Open(screen));
            }
            _ => {
                self.navigate(Intent::Proceed(screen));
            }
        }
    }

    /// The step being played is done: progress moves on (straight through), then the next
    /// step runs, or the campaign screen comes back.
    fn finish_step(&mut self) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(play) = self.campaign_play.clone() else {
            // A card without a play (should not happen): back to the campaign.
            self.navigate(Intent::Back);
            return;
        };
        let Some(c) = data.campaign(&play.campaign).cloned() else {
            return;
        };
        if play.straight {
            let mut progress = self.campaign_progress(&play.campaign);
            progress.finish(&c.campaign, play.step);
            self.store_progress(&play.campaign, progress);
        }
        self.after_step(&c, &play);
    }

    /// Where the campaign goes after `play`'s step.
    fn after_step(
        &mut self,
        c: &canopy_content::campaign::CampaignPackage,
        play: &crate::campaign::Playing,
    ) {
        match crate::campaign::after(&c.campaign, play.step, play.straight) {
            crate::campaign::After::Step(next) => self.run_step(&play.campaign, next, true),
            crate::campaign::After::Screen => {
                self.campaign_play = None;
                self.audio.stop_music(1.5);
                self.navigate(Intent::Proceed(Screen::Campaign {
                    campaign: play.campaign.clone(),
                }));
            }
        }
    }

    fn store_progress(&mut self, campaign: &str, progress: crate::campaign::Progress) {
        if let Err(error) = progress.save(&self.paths, campaign) {
            tracing::warn!(%error, "cannot save campaign progress");
        }
        self.progress.insert(campaign.to_string(), progress);
    }

    /// Continue on a card: the rest of the page, the next page, or the next step.
    fn card_next(&mut self) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(card) = &mut self.card else {
            return;
        };
        let Some(step) = data
            .campaign(&card.campaign)
            .and_then(|c| c.campaign.steps.get(card.step))
        else {
            return;
        };
        if card.advance(step) {
            self.finish_step();
        }
    }

    /// The pack's sounds and the game's sound events, once: the base's, then every
    /// campaign's own (D97). A match binds its own content over them.
    fn bind_sounds(&mut self, data: &GameData) {
        if self.audio.library().is_empty()
            && let Err(error) = self.audio.load(&data.pack)
        {
            tracing::warn!(%error, "sounds not loaded");
        }
        if !self.sounds_bound {
            self.sounds_bound = true;
            self.audio.bind_sounds(&data.content.sounds);
            for c in &data.campaigns {
                self.audio.bind_sounds(&c.sounds());
            }
        }
    }

    /// The main menu's music (`rules/sounds.json` → `menu`), unless it already plays.
    fn play_menu_music(&mut self, data: &GameData) {
        self.bind_sounds(data);
        let menu = &data.content.rules.sounds.menu;
        let playing = self.audio.current_music();
        let ours = menu
            .iter()
            .filter_map(|id| self.audio.event(id.as_str()))
            .any(|e| playing.is_some_and(|p| self.audio.bank().clips(e).contains(&p)));
        if ours {
            return;
        }
        if let Some(playlist) = self.audio.playlist(menu.iter().map(|id| id.as_str())) {
            self.audio.play_music(playlist);
        }
    }

    /// A step's music, when it names one (a music sound event).
    fn play_step_music(&mut self, data: &GameData, step: &canopy_content::campaign::Step) {
        let Some(music) = step.music() else {
            return;
        };
        self.bind_sounds(data);
        if let Some(playlist) = self.audio.playlist([music.as_str()]) {
            self.audio.play_music(playlist);
        }
    }

    /// Story narration for the page on screen, once per page (a dialogue sound event).
    fn play_narration(&mut self, data: &GameData, event: &str) {
        self.bind_sounds(data);
        if let Some(event) = self.audio.event(event) {
            self.audio.play_event(event, canopy_audio::At::Listener);
        }
    }

    /// An interface sound by what it marks (`rules/sounds.json` → `interface`).
    fn interface_sound(&mut self, name: &str) {
        let Some(data) = self.data.clone() else {
            return;
        };
        self.bind_sounds(&data);
        if let Some(event) = data
            .content
            .rules
            .sounds
            .interface
            .get(name)
            .and_then(|id| self.audio.event(id.as_str()))
        {
            self.audio.play_event(event, canopy_audio::At::Listener);
        }
    }

    /// Loads the mission at `step` of campaign `c`: player 1 plays the campaign's race,
    /// computer players take the other starts, and the campaign hands it its tech, carried
    /// heroes, level cap and variables.
    fn begin_mission(
        &mut self,
        data: &GameData,
        c: &canopy_content::campaign::CampaignPackage,
        step: usize,
    ) {
        let Some(mission) = c.campaign.steps.get(step).and_then(|s| s.mission()) else {
            return;
        };
        let Some(map) = data.map(mission.map.as_str()) else {
            return;
        };
        let mut slots = vec![canopy_sim::Slot {
            player: 0,
            kind: canopy_sim::SlotKind::Human,
            name: Some(self.settings.display_name()),
            team: None,
            hero: None,
            race: Some(c.campaign.race.clone()),
        }];
        // Players with a start are computer players (`mission.players` refines this).
        for &player in map.starts.iter().filter(|&&p| p > 1) {
            slots.push(canopy_sim::Slot {
                player: player - 1,
                kind: canopy_sim::SlotKind::Ai,
                name: None,
                team: None,
                hero: None,
                race: None,
            });
        }
        let setup = canopy_sim::MatchSetup {
            seed: seed(),
            slots,
        };
        let start = self
            .campaign_progress(c.id())
            .start(&c.campaign, &data.content, step);
        self.play_step_music(data, &c.campaign.steps[step]);
        self.start_rerun(Rerun {
            map: mission.map.to_string(),
            setup,
            me: Some(0),
            kind: MatchKind::Campaign,
            origin: Origin::Campaign {
                campaign: c.id().to_string(),
            },
            colors: None,
            campaign: Some(start),
            autoplay: self.campaign_play.as_ref().is_some_and(|p| p.autoplay),
        });
    }

    /// The loaded mission begins (a click or a key on its loading screen).
    fn begin_loaded(&mut self) {
        if let Some(loading) = &mut self.loading {
            loading.hold = false;
        }
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
            let bytes = map.identity()?;
            let here = data.match_identity(map, &bytes);
            self.library.read(id, Some(&here))
        })();
        let (meta, body) = match attempt {
            Ok(x) => x,
            Err(error) => {
                self.saves_status = tr!("menu.cannot-load", error = &format!("{error:#}"));
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
        // A campaign save continues its step: Retry starts it as the campaign would.
        let (origin, campaign) = match self.campaign_of_save(&data, &meta.identity.map_id) {
            Some((play, start)) => {
                let origin = Origin::Campaign {
                    campaign: play.campaign.clone(),
                };
                self.campaign_play = Some(play);
                (origin, Some(start))
            }
            None => (origin, None),
        };
        self.last_match = Some(Rerun {
            map: meta.identity.map_id.clone(),
            setup: setup.clone(),
            me: meta.player,
            kind,
            origin: origin.clone(),
            colors: None,
            campaign,
            autoplay: false,
        });
        let start = PendingStart {
            kind,
            me: meta.player,
            map: meta.identity.map_id.clone(),
            networked: false,
            camera: body.camera,
            groups: body.groups.clone(),
            start_tick: 0,
            colors: None,
            watching: None,
        };
        let job = Job::Match(Box::new(MatchRequest {
            map: meta.identity.map_id.clone(),
            setup,
            me: meta.player,
            resume: Some(body.world),
            session: None,
            colors: None,
            replay: None,
            map_path: None,
            campaign: None,
            autoplay: false,
        }));
        self.saves_status.clear();
        self.start_loading(job, start, origin);
    }

    /// The campaign step a save on `map` belongs to (the furthest step reached that plays
    /// it), and what it starts with.
    fn campaign_of_save(
        &mut self,
        data: &GameData,
        map: &str,
    ) -> Option<(
        crate::campaign::Playing,
        canopy_sim::campaign::CampaignStart,
    )> {
        let c = data.campaign(data.map(map)?.campaign.as_deref()?)?.clone();
        let progress = crate::campaign::Progress::load(&self.paths, c.id());
        let next = progress.next(&c.campaign);
        let step = (0..c.campaign.steps.len())
            .rev()
            .filter(|&i| i <= next)
            .find(|&i| {
                c.campaign.steps[i]
                    .mission()
                    .is_some_and(|m| m.map.as_str() == map)
            })?;
        let start = progress.start(&c.campaign, &data.content, step);
        self.progress.insert(c.id().to_string(), progress);
        Some((
            crate::campaign::Playing {
                campaign: c.id().to_string(),
                step,
                straight: step == next,
                autoplay: false,
            },
            start,
        ))
    }

    /// The campaign step that plays `map`, when a campaign holds it.
    fn mission_step(&self, map: &str) -> Option<(String, usize)> {
        let data = self.data.as_ref()?;
        let campaign = data.campaign(data.map(map)?.campaign.as_deref()?)?;
        let step = campaign
            .campaign
            .steps
            .iter()
            .position(|s| s.mission().is_some_and(|m| m.map.as_str() == map))?;
        Some((campaign.id().to_string(), step))
    }

    fn start_dev(&mut self, dev: DevStart) {
        // Autoplay plays a campaign's mission as its step: its tech, level cap and HUD.
        if dev.spec.autoplay
            && dev.spec.human.is_some()
            && let Some((campaign, step)) = self.mission_step(&dev.spec.map)
        {
            self.campaign_autoplay = true;
            self.run_step(&campaign, step, false);
            return;
        }
        let start = PendingStart {
            kind: MatchKind::Skirmish,
            me: dev.spec.human,
            map: dev.spec.map.clone(),
            networked: false,
            camera: dev.camera,
            groups: None,
            start_tick: dev.tick,
            colors: dev.spec.colors.clone(),
            watching: None,
        };
        self.start_loading(Job::Dev(dev.spec), start, Origin::MainMenu);
    }

    fn finish_loading(&mut self, ctx: &mut Ctx<'_>) {
        let Some(loading) = &mut self.loading else {
            return;
        };
        if let Some(job) = &mut loading.job {
            let Some(result) = job.poll() else {
                return;
            };
            loading.job = None;
            match result {
                Ok(p) => loading.prepared = Some(p),
                Err(error) => {
                    tracing::error!(error = %format!("{error:#}"), "match failed to load");
                    loading.error = Some(format!("{error:#}"));
                    return;
                }
            }
        }
        // A campaign mission waits for the player on its loading screen.
        if loading.hold {
            return;
        }
        let Some(prepared) = loading.prepared.take() else {
            return;
        };
        let start = loading.start.clone();
        let Some(data) = self.data.clone() else {
            return;
        };
        let identity = data.map(&start.map).map_or_else(
            || MatchIdentity {
                map_id: start.map.clone(),
                map_digest: saves::digest(&prepared.map_identity),
                content_digest: data.content_digest.clone(),
            },
            |m| data.match_identity(m, &prepared.map_identity),
        );
        let ready = start.networked.then(|| Ready {
            content: data.network_identity(&prepared.map_identity),
            build: canopy_net::wire::BUILD.into(),
        });
        // What a replay of this match says about it; matches of the editor's content
        // (Test in game) and replays themselves are not recorded.
        let info = start
            .kind
            .replay_mode()
            .filter(|_| self.testing.is_none() && start.watching.is_none())
            .map(|mode| {
                let setup = prepared.game.world.setup();
                let map_package = data
                    .map(&start.map)
                    .filter(|m| !m.file.path.starts_with(&self.paths.data))
                    .map(|m| m.file.path.clone());
                crate::replay::MatchInfo {
                    mode,
                    map_name: prepared.map_name.clone(),
                    identity: identity.clone(),
                    map_package,
                    players: setup
                        .slots
                        .iter()
                        .map(|s| crate::replay::format::Player {
                            slot: s.player,
                            name: match s.kind {
                                canopy_sim::SlotKind::Ai => {
                                    tr!("menu.computer", number = s.player + 1)
                                }
                                canopy_sim::SlotKind::Human => s
                                    .name
                                    .clone()
                                    .unwrap_or_else(|| tr!("hud.player", number = s.player + 1)),
                            },
                            computer: s.kind == canopy_sim::SlotKind::Ai,
                            race: s.race.clone(),
                            hero: s.hero.clone(),
                            team: s.team,
                            color: start
                                .colors
                                .as_ref()
                                .and_then(|c| c.get(usize::from(s.player)).copied())
                                .unwrap_or(usize::from(s.player)),
                        })
                        .collect(),
                    recorder: start.me,
                    colors: start.colors.clone(),
                }
            });
        self.mission_hud = MissionHud::new(prepared.objectives.clone());
        let mut host = Host {
            window: ctx.window,
            gpu: ctx.gpu,
            renderer: ctx.renderer,
            ui: &mut self.host.ui,
            audio: &mut self.audio,
            size: ctx.size,
            scale: ctx.scale,
        };
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
                groups: start.groups.clone(),
                name: self.settings.display_name(),
                info,
                watching: start.watching.clone(),
            },
            &mut host,
            &self.play_options,
        );
        match started {
            Ok(playing) => {
                tracing::info!(
                    map = %start.map,
                    ms = loading.started.elapsed().as_millis() as u64,
                    "match started"
                );
                self.playing = Some(playing);
                self.surrendered = false;
                self.replay_saved = None;
                if start.watching.is_some() {
                    self.replay_ui.reset();
                }
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
        // The compact HUD depends on the window's width.
        let width = ctx.size[0] as f32 / ctx.scale.max(0.1);
        if (width - self.hud_width).abs() > 0.5 {
            self.hud_width = width;
            self.apply_hud();
        }
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
        self.poll_updates();
        self.poll_feedback(ctx.gpu);
        self.finish_loading(ctx);
        // Text fields reach the system clipboard (copy, cut, paste) for every screen.
        let mut taken = self.host.ui.take_actions();
        taken.retain(|a| match &a.kind {
            ActionKind::Copy(text) => {
                crate::clipboard::set(text);
                false
            }
            ActionKind::PasteRequest => {
                if let Some(text) = crate::clipboard::get() {
                    self.host.ui.paste(&text);
                }
                false
            }
            _ => true,
        });
        // A click sounds, wherever it lands (D97).
        if taken
            .iter()
            .any(|a| matches!(a.kind, ActionKind::Click { .. }))
        {
            self.interface_sound("click");
        }
        // The HUD's actions go to the match, the rest to the screens.
        let (hud, actions): (Vec<Action>, Vec<Action>) = taken
            .into_iter()
            .partition(|a| a.path.as_deref().is_some_and(|p| p.starts_with("hud/")));
        self.update_match(ctx, hud);
        for action in actions {
            if !self.fading_out {
                self.action(&action, ctx);
            }
        }
        self.update_campaign(dt);
        self.update_fade(dt);
        self.sync(ctx);
        self.update_cursor(ctx);
        if self.playing.is_none() {
            // A match updates the audio itself (its listener and effect sounds).
            self.audio.update(&canopy_audio::Frame {
                real_dt: dt,
                render_tick: 0.0,
                speed: 0.0,
                listener: canopy_audio::Listener::new(0.0, 0.0, 1.0, 0.0),
            });
        }
        let time = (now - self.started).as_secs_f64();
        self.prepare_ui(ctx.gpu, ctx.renderer, time);
        std::mem::take(&mut self.requests)
    }

    /// A card's clock (timed cards move on by themselves) and a won mission's pause
    /// before the campaign moves on.
    fn update_campaign(&mut self, dt: f32) {
        if let Some(card) = &mut self.card
            && matches!(self.flow.screen(), Screen::Step { .. })
            && self.flow.overlays().is_empty()
            && !self.fading_out
        {
            let before = card.time;
            card.time += dt;
            let page = card.page;
            let narration = self.data.clone().and_then(|data| {
                let c = data.campaign(&card.campaign)?;
                let step = c.campaign.steps.get(card.step)?;
                let narration = match step {
                    canopy_content::campaign::Step::Story(s) if before == 0.0 => {
                        s.pages.get(page)?.narration.clone()
                    }
                    _ => None,
                };
                Some((card.due(step), narration, data.clone()))
            });
            if let Some((due, narration, data)) = narration {
                if let Some(asset) = narration {
                    self.play_narration(&data, asset.as_str());
                }
                if due {
                    self.finish_step();
                }
            }
        }
        if self
            .advance_at
            .is_some_and(|at| Instant::now() >= at && !self.fading_out)
        {
            self.advance_at = None;
            self.after_victory();
        }
    }

    /// A won campaign mission's pause is over: the campaign moves on.
    fn after_victory(&mut self) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(play) = self.campaign_play.clone() else {
            self.apply(Intent::LeaveMatch);
            return;
        };
        let Some(c) = data.campaign(&play.campaign).cloned() else {
            return;
        };
        self.after_step(&c, &play);
    }

    /// The mouse cursor: the UI's (pointer over controls, text over fields), else the
    /// old canvas cursor (a crosshair while choosing an attack target).
    fn update_cursor(&mut self, ctx: &Ctx<'_>) {
        use winit::window::CursorIcon;
        let Some(window) = ctx.window else {
            return;
        };
        let attack = self.flow.in_match()
            && self.playing.as_ref().is_some_and(|p| {
                matches!(
                    &p.game.play.mode,
                    crate::play::Mode::Target {
                        targeting: crate::play::orders::Targeting::Attack { .. },
                        ..
                    }
                )
            });
        let icon = match self.host.ui.cursor() {
            canopy_ui::Cursor::Pointer => CursorIcon::Pointer,
            canopy_ui::Cursor::Text => CursorIcon::Text,
            canopy_ui::Cursor::Grab => CursorIcon::Grab,
            canopy_ui::Cursor::Grabbing => CursorIcon::Grabbing,
            canopy_ui::Cursor::Default if attack => CursorIcon::Crosshair,
            canopy_ui::Cursor::Default => CursorIcon::Default,
        };
        if self.cursor != Some(icon) {
            window.set_cursor(icon);
            self.cursor = Some(icon);
        }
    }

    /// Lays out and paints the UI (with the match's world UI stand-in under the HUD) and
    /// hands the draw list to the renderer's UI pass.
    pub fn prepare_ui(&mut self, gpu: &Gpu, renderer: &mut Renderer, time: f64) {
        let layer = renderer.ui_mut(gpu);
        self.host.flush(gpu, layer);
        match (&self.playing, self.flow.in_match()) {
            (Some(playing), true) => {
                let mut list = self.host.ui.frame(time).clone();
                playing.decorate(&mut list);
                layer.submit(gpu, &list);
            }
            _ => layer.submit(gpu, self.host.ui.frame(time)),
        }
    }

    /// Takes in the game's data once it has loaded (also while the editor has the window,
    /// so Test in game works when the game opened straight into it).
    pub fn poll_boot(&mut self) {
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
                // The content's command shortcuts join the rebindable catalogue.
                self.catalog = keys::full_catalog(Some(&data.content));
                self.play_options = play_options(&self.settings, &self.catalog);
                self.data = Some(Arc::new(data));
                self.apply(Intent::Booted);
                if let Some(dev) = self.dev.take() {
                    self.start_dev(dev);
                } else if let Some(path) = self.boot_replay.take() {
                    self.watch_replay(&path, None);
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
                    self.mp.status = tr!("menu.missing-battlefield", map = config.map_id.as_str());
                    drop(session);
                    self.apply(Intent::Back);
                    return;
                }
                let me = session.player();
                self.last_match = None;
                // The lobby's colours, by player number; the others keep their own.
                let colors = config.colors.as_ref().map(|colors| {
                    let mut by_player: Vec<usize> = (0..8).collect();
                    for (slot, &color) in config.slots.iter().zip(colors) {
                        if let Some(c) = by_player.get_mut(usize::from(slot.player)) {
                            *c = usize::from(color);
                        }
                    }
                    by_player
                });
                let start = PendingStart {
                    kind: MatchKind::Multiplayer,
                    me,
                    map: config.map_id.clone(),
                    networked: true,
                    camera: None,
                    groups: None,
                    start_tick: 0,
                    colors: colors.clone(),
                    watching: None,
                };
                let job = Job::Match(Box::new(MatchRequest {
                    map: config.map_id.clone(),
                    setup: config.setup(),
                    me,
                    resume: None,
                    session: Some(*session),
                    colors,
                    replay: None,
                    map_path: None,
                    campaign: None,
                    autoplay: false,
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

    fn update_match(&mut self, ctx: &mut Ctx<'_>, hud: Vec<Action>) {
        let Some(playing) = &mut self.playing else {
            return;
        };
        if self.settings_dirty {
            // Shortcuts or HUD settings may have changed.
            let options = play_options(&self.settings, &self.catalog);
            if options != self.play_options {
                playing.set_options(&options);
                self.play_options = options;
            }
        }
        let free = self.flow.in_match() && self.flow.overlays().is_empty();
        let mut host = Host {
            window: ctx.window,
            gpu: ctx.gpu,
            renderer: ctx.renderer,
            ui: &mut self.host.ui,
            audio: &mut self.audio,
            size: ctx.size,
            scale: ctx.scale,
        };
        playing.update(&mut host, free);
        playing.act(&mut self.host.ui, hud);
        let events = playing.take_events();
        for event in events {
            match event {
                MatchEvent::MenuRequested => {
                    if self.flow.overlays().is_empty() {
                        if let Some(playing) = &mut self.playing {
                            playing.release_input();
                        }
                        self.menu_page = MenuPage::Main;
                        self.menu_status.clear();
                        self.apply(Intent::Show(Overlay::GameMenu));
                    }
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
            }
        }
    }

    /// Shows the result (an outcome, or a surrender).
    fn decided(&mut self, outcome: Option<canopy_sim::state::Outcome>, surrendered: bool) {
        let (Some(playing), Some(data)) = (&self.playing, self.data.clone()) else {
            return;
        };
        let save = playing.save();
        let model = ResultModel::build(
            playing,
            &data,
            outcome.as_ref(),
            save.as_ref().map(|b| &b.world),
            surrendered,
        );
        let campaign = (playing.kind == MatchKind::Campaign)
            .then(|| self.campaign_play.clone())
            .flatten();
        let map_id = playing.map_id.clone();
        if model.won
            && let Some(play) = campaign
        {
            // The heroes and campaign variables carry on (docs/campaign.md).
            if play.straight
                && let (Some(c), Some(body)) = (data.campaign(&play.campaign), &save)
            {
                let content = data
                    .content_for(&self.paths, &map_id)
                    .unwrap_or_else(|_| data.content.clone());
                let catalog = canopy_sim::catalog::Catalog::new(content);
                let state = &body.world.state;
                let heroes = canopy_sim::campaign::carried(state, &catalog, 0);
                let variables = state
                    .campaign
                    .as_ref()
                    .map(|c| c.variables.clone())
                    .unwrap_or_default();
                let mut progress = self.campaign_progress(&play.campaign);
                progress.won(&c.campaign, play.step, heroes, &variables);
                self.store_progress(&play.campaign, progress);
            }
            if play.straight {
                // Straight through: the victory shows a moment, then the next step.
                self.result = None;
                self.advance_at =
                    Some(Instant::now() + std::time::Duration::from_secs_f32(VICTORY_SECONDS));
                return;
            }
        }
        let Some(playing) = &self.playing else {
            return;
        };
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
            "menu.campaign" if clicked => self.navigate(Intent::Open(Screen::Campaigns)),
            "menu.skirmish" if clicked => self.navigate(Intent::Open(Screen::Skirmish)),
            "menu.multiplayer" if clicked => self.navigate(Intent::Open(Screen::Multiplayer)),
            "menu.load" if clicked => self.open_saves(),
            "menu.settings" if clicked => self.open_settings(),
            "menu.editor" if clicked && self.paths.repository().is_some() => {
                self.requests.editor = true
            }
            "menu.exit" if clicked => self.apply(Intent::Quit),
            // Feedback, from the main menu or the game menu.
            "menu.feedback" | "gm.feedback" if clicked => self.open_feedback(),
            "fb.category" => {
                if let Some(&category) =
                    selected.and_then(|i| crate::feedback::Category::ALL.get(i))
                {
                    self.feedback.category = category;
                }
            }
            "fb.description" => match &action.kind {
                ActionKind::TextChanged(text) => self.feedback.description = text.clone(),
                ActionKind::Submitted(text) => {
                    self.feedback.description = text.clone();
                    self.submit_feedback(ctx.gpu);
                }
                _ => {}
            },
            "fb.submit" if clicked => self.submit_feedback(ctx.gpu),
            "fb.cancel" | "fb.close" if clicked => self.close_feedback(),
            // The release plaque and dialog.
            "releases.notes" if clicked => self.open_releases(releases::View::Notes),
            "releases.updates" if clicked => {
                self.open_releases(releases::View::Updates);
                // Opening the details retries a failed check (old behaviour).
                if matches!(
                    self.updater.state().phase,
                    crate::update::Phase::Idle | crate::update::Phase::Error
                ) {
                    self.updater.check();
                }
            }
            "releases.select" if clicked => {
                if let Some(i) = action.index {
                    self.releases.select(i);
                }
            }
            "releases.primary" if clicked => {
                use crate::update::Phase;
                match self.updater.state().phase {
                    Phase::Available => self.updater.download(),
                    Phase::Ready => self.updater.install(),
                    Phase::Restart => self.updater.request_restart(),
                    Phase::Current | Phase::Error => self.updater.check(),
                    _ => {}
                }
            }
            "releases.close" if clicked => self.close_releases(),
            "releases.image" if clicked => {
                if let Some(i) = action.index {
                    self.releases.open_viewer(&self.updater, i);
                }
            }
            "releases.previous" if clicked => self.releases.step_viewer(&self.updater, -1),
            "releases.next" if clicked => self.releases.step_viewer(&self.updater, 1),
            "releases.viewer-close" if clicked => self.releases.viewer = None,
            // Campaign.
            "campaign.play" if clicked => {
                if let Some(id) = action.index.and_then(|i| campaign::card_id(&data, i)) {
                    self.navigate(Intent::Open(Screen::Campaign { campaign: id }));
                }
            }
            "campaign.back" | "steps.back" | "skirmish.back" | "mp.back" if clicked => {
                self.navigate(Intent::Back)
            }
            "steps.continue" if clicked => {
                if let Screen::Campaign { campaign } = self.flow.screen().clone() {
                    self.continue_campaign(&campaign);
                }
            }
            "steps.reset" if clicked => {
                if let Screen::Campaign { campaign } = self.flow.screen().clone() {
                    self.ask_reset(&campaign);
                }
            }
            "steps.act" if clicked => {
                if let (Screen::Campaign { campaign }, Some(step)) = (
                    self.flow.screen().clone(),
                    action
                        .index
                        .and_then(|i| self.campaign_entries.get(i))
                        .and_then(campaign::Entry::step),
                ) && let Some(c) = data.campaign(&campaign)
                {
                    let next = self.campaign_progress(&campaign).next(&c.campaign);
                    self.run_step(&campaign, step, step == next);
                }
            }
            "steps.autoplay" if clicked => {
                if let (Screen::Campaign { campaign }, Some(step)) = (
                    self.flow.screen().clone(),
                    action
                        .index
                        .and_then(|i| self.campaign_entries.get(i))
                        .and_then(campaign::Entry::step),
                ) && let Some(c) = data.campaign(&campaign)
                {
                    let next = self.campaign_progress(&campaign).next(&c.campaign);
                    self.campaign_autoplay = true;
                    self.run_step(&campaign, step, step == next);
                }
            }
            "card.next" if clicked => self.card_next(),
            "card.skip" if clicked => self.finish_step(),
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
            "skirmish.controller"
            | "skirmish.team"
            | "skirmish.race"
            | "skirmish.hero"
            | "skirmish.color" => {
                if let (Some(slot), Some(option)) = (action.index, selected) {
                    let field = name.trim_start_matches("skirmish.");
                    let colors = self.palette.len();
                    self.skirmish.choose(&data, field, slot, option, colors);
                }
            }
            "skirmish.start" if clicked => self.start_skirmish(),
            "skirmish.replays" if clicked => self.navigate(Intent::Open(Screen::Replays)),
            // Replays (D60).
            "replays.back" if clicked => self.navigate(Intent::Back),
            "replays.select" if clicked => {
                if let Some(i) = action.index {
                    self.replays.selected = i;
                    self.replays.renaming = false;
                    self.replays.status.clear();
                    if matches!(action.kind, ActionKind::Click { count: 2, .. }) {
                        self.watch_selected();
                    }
                }
            }
            "replays.watch" if clicked => self.watch_selected(),
            "replays.rename" if clicked => {
                if let Some(entry) = self.replays.selected() {
                    let name = entry.name.clone();
                    self.replays.renaming = true;
                    self.replays.rename_text = name.clone();
                    widgets::text(&mut self.host.ui, "replays/rename/field", &name);
                    let field = self.host.ui.find("replays/rename/field");
                    self.host.ui.focus(field);
                }
            }
            "replays.rename-name" => match &action.kind {
                ActionKind::TextChanged(text) => self.replays.rename_text = text.clone(),
                ActionKind::Submitted(text) => {
                    self.replays.rename_text = text.clone();
                    self.rename_replay();
                }
                _ => {}
            },
            "replays.rename-confirm" if clicked => self.rename_replay(),
            "replays.rename-cancel" if clicked => self.replays.renaming = false,
            "replays.delete" if clicked => {
                if let Some(entry) = self.replays.selected() {
                    self.ask(Dialog {
                        ask: Ask::DeleteReplay(entry.name.clone()),
                        title: tr!("menu.delete-replay-q"),
                        body: tr!("menu.removed-for-good", name = &plain(&entry.name)),
                        confirm: tr!("menu.delete"),
                        danger: true,
                        second: None,
                    });
                }
            }
            "replays.reveal" if clicked => {
                let dir = self.replay_library.dir().to_path_buf();
                if let Err(error) = std::fs::create_dir_all(&dir)
                    .map_err(anyhow::Error::from)
                    .and_then(|()| crate::files::reveal(&dir))
                {
                    self.replays.status =
                        tr!("menu.cannot-open-folder", error = &format!("{error:#}"));
                }
            }
            _ if name.starts_with("replay.") => {
                if let Some(playing) = &mut self.playing {
                    self.replay_ui.act(action, playing);
                }
            }
            "result.save-replay" if clicked => self.save_result_replay(),
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
            "lobby.seat" | "lobby.team" | "lobby.race" | "lobby.hero" | "lobby.color" => {
                if let (Some(row), Some(option)) = (action.index, selected) {
                    self.mp
                        .choose(&data, name.trim_start_matches("lobby."), row, option);
                }
            }
            "lobby.start" if clicked => self.mp.start(),
            // Loading.
            "loading.begin" if clicked => {
                if self.loading.as_ref().is_some_and(|l| l.prepared.is_some()) {
                    self.begin_loaded();
                }
            }
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
                self.save_game_named(&tr!("menu.save-before-leaving"));
                self.apply(Intent::LeaveMatch);
            }
            "gm.leave" if clicked => self.apply(Intent::LeaveMatch),
            "gm.save-restart" | "gm.restart-now" if clicked => {
                if name == "gm.save-restart" {
                    self.save_game_named(&tr!("menu.save-before-restarting"));
                }
                if let Some(rerun) = self.last_match.clone() {
                    self.start_rerun(rerun);
                }
            }
            "gm.surrender-now" if clicked => {
                self.surrendered = true;
                self.decided(None, true);
            }
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
                        title: tr!("menu.delete-save-q"),
                        body: tr!("menu.removed-for-good", name = &plain(&meta.name)),
                        confirm: tr!("menu.delete"),
                        danger: true,
                        second: None,
                    });
                }
            }
            "mission.history" if clicked => {
                self.mission_hud.history_open = !self.mission_hud.history_open;
            }
            "autoplay.take" if clicked => {
                if let Some(playing) = &self.playing {
                    playing.autoplay(crate::sim::AutoplayControl::TakeOver);
                }
            }
            "autoplay.speed1" | "autoplay.speed2" | "autoplay.speed4" | "autoplay.speed8"
                if clicked =>
            {
                if let (Some(playing), Some(speed)) = (
                    &self.playing,
                    name.strip_prefix("autoplay.speed")
                        .and_then(|n| n.parse::<f32>().ok()),
                ) {
                    playing.autoplay(crate::sim::AutoplayControl::Speed(speed));
                }
            }
            "saves.export" if clicked => {
                if let Some(meta) = action.index.and_then(|i| self.saves_list.get(i)).cloned() {
                    self.export_save(&meta);
                }
            }
            "saves.import" if clicked => self.import_save(),
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
            "result.retry" if clicked => {
                if let Some(rerun) = self.last_match.clone() {
                    self.start_rerun(rerun);
                }
            }
            _ => {}
        }
    }

    /// Opens the feedback form; the frame drawn next, without it, is its screenshot.
    fn open_feedback(&mut self) {
        use crate::feedback::Place;
        let place = match (&self.playing, self.flow.in_match()) {
            (Some(playing), true) => Place {
                in_match: true,
                map: Some(playing.map_id.clone()),
                mode: Some(match playing.kind {
                    MatchKind::Skirmish => "skirmish",
                    MatchKind::Campaign => "campaign",
                    MatchKind::Multiplayer => "multiplayer",
                    MatchKind::Replay => "replay",
                }),
                tick: playing.newest().map(|s| s.view.tick),
            },
            _ => Place::default(),
        };
        self.apply(Intent::Show(Overlay::Feedback));
        if self.flow.top() != Some(Overlay::Feedback) {
            return;
        }
        self.feedback.open(place);
        self.feedback.fill(&mut self.host.ui);
        self.grab.request();
        self.attach_match_files();
    }

    /// From inside a match, the report carries the replay of the match so far and a save
    /// of this moment (D60); watching a replay, the replay itself. Encoded on a worker
    /// thread; the form waits for them as for the screenshot.
    fn attach_match_files(&mut self) {
        let Some(playing) = self.playing.as_ref().filter(|_| self.flow.in_match()) else {
            return;
        };
        if let Some(watching) = &playing.watching {
            let path = watching.path.clone();
            let name = format!("{}.{}", watching.name, crate::replay::format::EXTENSION);
            self.feedback
                .prepare_files(move || crate::feedback::Attachments {
                    replay: std::fs::read(&path).ok().map(|bytes| (name, bytes)),
                    save: None,
                });
            return;
        }
        let result = if playing.outcome.is_some() || self.surrendered {
            playing.result_word(self.surrendered)
        } else {
            "In progress"
        };
        let replay = playing.replay(result);
        let save = playing.kind.save_mode().and_then(|mode| {
            let body = playing.save()?;
            let meta = saves::SaveMeta {
                format: saves::FORMAT.into(),
                version: saves::VERSION,
                id: "feedback".into(),
                name: format!("{} for feedback", playing.map_name),
                saved_at: saves::now_ms(),
                mode,
                map_name: playing.map_name.clone(),
                identity: playing.identity.clone(),
                player: playing.me,
                tick: body.world.state.tick,
            };
            Some((meta, body))
        });
        self.feedback.prepare_files(move || {
            let replay = replay.and_then(|r| {
                let name = format!(
                    "{}.{}",
                    crate::replay::library::clean_name(&crate::replay::default_name(&r.header)),
                    crate::replay::format::EXTENSION
                );
                r.to_bytes().ok().map(|bytes| (name, bytes))
            });
            let save = save.and_then(|(meta, body)| {
                let name = saves::SaveLibrary::export_name(&meta);
                saves::exported_bytes(&meta, &body)
                    .ok()
                    .map(|bytes| (name, bytes))
            });
            crate::feedback::Attachments { replay, save }
        });
    }

    fn submit_feedback(&mut self, gpu: &Gpu) {
        use crate::feedback::{self as fb, System};
        if !self.feedback.can_submit() {
            return;
        }
        let system = System {
            version: env!("CARGO_PKG_VERSION").into(),
            os: fb::os_description(),
            gpu: gpu.name(),
            preset: self
                .settings
                .graphics
                .active_preset()
                .map_or_else(|| "Custom".into(), |p| format!("{p:?}")),
        };
        let mut report = fb::build(
            &fb::install_id(&self.paths.user),
            self.feedback.category,
            &self.feedback.description,
            &system,
            &self.feedback.place,
            self.feedback.shot(),
        );
        fb::attach(&mut report, &self.feedback.files());
        let id = report.report_id.clone();
        if let Err(error) = self.courier.submit(report) {
            tracing::warn!(error = %format!("{error:#}"), "feedback could not be kept for later");
        }
        self.feedback.sending(id);
    }

    fn close_feedback(&mut self) {
        if self.flow.top() == Some(Overlay::Feedback) {
            self.apply(Intent::Back);
        }
    }

    /// The screenshot being taken and the reports going out.
    fn poll_feedback(&mut self, gpu: &Gpu) {
        if let Some(result) = self.grab.poll(gpu) {
            self.feedback.screenshot(&mut self.host, gpu, result);
        }
        self.feedback.poll_files();
        let (setting, chosen) = (&self.settings.multiplayer.server, &self.feedback_server);
        let server = || match chosen {
            Some(address) => canopy_net::ServerAddress::parse(address),
            None => canopy_net::ServerAddress::resolve(setting.as_deref()),
        };
        for delivery in self.courier.poll(server) {
            self.feedback.delivered(&delivery);
        }
    }

    /// Whether the next frame is the feedback screenshot: the window then draws it into a
    /// texture of its own ([`crate::feedback::grab::Target`]) and calls [`Game::grab`]
    /// instead of presenting it.
    pub fn grab_due(&self) -> bool {
        self.grab.due()
    }

    /// Reads back the frame just drawn into `texture` as the feedback screenshot.
    pub fn grab(&mut self, gpu: &Gpu, texture: &wgpu::Texture) {
        self.grab.record(gpu, texture);
    }

    fn open_releases(&mut self, view: releases::View) {
        self.releases.open(view);
        self.apply(Intent::Show(Overlay::Releases));
    }

    /// Escape or Close: the enlarged image first, then the dialog unless the update is
    /// busy or restarting (old behaviour).
    fn close_releases(&mut self) {
        if self.releases.viewer.take().is_some() {
            return;
        }
        if releases::Releases::closable(&self.updater) && self.flow.top() == Some(Overlay::Releases)
        {
            self.apply(Intent::Back);
        }
    }

    /// The update's outcomes; a finished install restarts into the new version once the
    /// settings are written.
    fn poll_updates(&mut self) {
        self.updater.poll();
        if self.updater.restart_due() {
            self.save_settings();
            self.updater.restart();
            if self.updater.relaunched() {
                tracing::info!("restarting into the update");
                self.apply(Intent::Quit);
            }
        }
    }

    /// Reset campaign on the campaign screen: asks first, then clears its progress.
    fn ask_reset(&mut self, campaign: &str) {
        let Some(name) = self
            .data
            .as_ref()
            .and_then(|d| d.campaign(campaign))
            .map(|c| plain(c.name()))
        else {
            return;
        };
        self.ask(Dialog {
            ask: Ask::ResetCampaign(campaign.to_string()),
            title: tr!("campaign.reset-q"),
            body: tr!("campaign.reset-body", name = &name),
            confirm: tr!("campaign.reset"),
            danger: true,
            second: None,
        });
    }

    fn ask(&mut self, dialog: Dialog) {
        self.dialog = Some(dialog);
        self.apply(Intent::Show(Overlay::Confirm));
    }

    fn answer(&mut self, ask: Ask, _second: bool) {
        match ask {
            Ask::DeleteReplay(name) => {
                self.replays.status = match self.replay_library.delete(&name) {
                    Ok(()) => tr!("menu.deleted", name = &plain(&name)),
                    Err(error) => tr!("menu.cannot-delete", error = &format!("{error:#}")),
                };
                self.replays.refresh(&self.replay_library);
            }
            Ask::DeleteSave(id) => {
                if let Err(error) = self.library.delete(&id) {
                    self.saves_status = tr!("menu.cannot-delete", error = &format!("{error:#}"));
                }
                self.list_saves();
            }
            Ask::ResetCampaign(campaign) => {
                self.store_progress(&campaign, crate::campaign::Progress::default());
            }
        }
    }

    /// Keeps the running match as the last match's replay (overwritten every match), so
    /// it is never lost: when it ends, is left, restarted or the game closes (D60).
    fn keep_last_match(&mut self) {
        let Some(playing) = &self.playing else {
            return;
        };
        if playing.watching.is_some() || playing.recorder.is_none() {
            return;
        }
        let result = if playing.outcome.is_some() || self.surrendered {
            playing.result_word(self.surrendered)
        } else {
            "Left"
        };
        if let Some(replay) = playing.replay(result) {
            match self
                .replay_library
                .write(&replay, crate::replay::library::LAST_MATCH)
            {
                Ok(_) => tracing::info!(
                    commands = replay.body.commands.len(),
                    ticks = replay.header.duration(),
                    "kept the last match's replay"
                ),
                Err(error) => {
                    tracing::warn!(error = %format!("{error:#}"), "the last match's replay could not be kept")
                }
            }
        }
    }

    /// The result screen's Save replay: the match as `<Map> <date> <time> <result>`.
    fn save_result_replay(&mut self) {
        let Some(playing) = &self.playing else {
            return;
        };
        let result = playing.result_word(self.surrendered);
        let saved = match playing.replay(result) {
            Some(replay) => {
                let name = crate::replay::default_name(&replay.header);
                self.replay_library
                    .write(&replay, &name)
                    .map_err(|e| format!("{e:#}"))
            }
            None => Err("this match was not recorded".into()),
        };
        self.replay_saved = Some(saved);
    }

    fn rename_replay(&mut self) {
        let Some(entry) = self.replays.selected() else {
            return;
        };
        let from = entry.name.clone();
        let to = self.replays.rename_text.clone();
        match self.replay_library.rename(&from, &to) {
            Ok(name) => {
                self.replays.status = tr!("menu.renamed", name = &plain(&name));
                self.replays.refresh(&self.replay_library);
                if let Some(i) = self.replays.entries.iter().position(|e| e.name == name) {
                    self.replays.selected = i;
                }
            }
            Err(error) => {
                self.replays.status = tr!("menu.cannot-rename", error = &format!("{error:#}"))
            }
        }
    }

    fn watch_selected(&mut self) {
        let Some(data) = self.data.clone() else {
            return;
        };
        if let Err(reason) = self.replays.watchable(&data) {
            self.replays.status = reason;
            return;
        }
        if let Some(entry) = self.replays.selected() {
            let path = entry.path.clone();
            let name = entry.name.clone();
            self.watch_replay(&path, Some(name));
        }
    }

    /// Opens the replay at `path` in the viewer; leaving it returns to the replays.
    fn watch_replay(&mut self, path: &std::path::Path, name: Option<String>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let attempt =
            (|| -> Result<(crate::replay::format::Replay, Option<std::path::PathBuf>)> {
                let replay = crate::replay::format::Replay::read(path)?;
                if let Some(reason) = replays::unplayable(&replay.header, &data) {
                    anyhow::bail!("{reason}");
                }
                let map_path = match &replay.body.map {
                    Some(package)
                        if data.map(&replay.header.identity.map_id).is_none_or(|m| {
                            m.identity().map(|d| saves::digest(&d)).ok().as_deref()
                                != Some(replay.header.identity.map_digest.as_str())
                        }) =>
                    {
                        Some(crate::replay::unpack_map(
                            &replay.header,
                            package,
                            &self.paths.cache,
                        )?)
                    }
                    _ => None,
                };
                Ok((replay, map_path))
            })();
        let (replay, map_path) = match attempt {
            Ok(x) => x,
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "cannot watch the replay");
                self.replays.status = tr!("menu.cannot-watch", error = &format!("{error:#}"));
                return;
            }
        };
        let name = name.unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
        });
        let header = replay.header.clone();
        let colors = replay.body.colors.clone();
        let start = PendingStart {
            kind: MatchKind::Replay,
            me: None,
            map: header.identity.map_id.clone(),
            networked: false,
            camera: None,
            groups: None,
            start_tick: 0,
            colors: colors.clone(),
            watching: Some(in_match::Watching {
                name,
                path: path.to_path_buf(),
                header: header.clone(),
            }),
        };
        let job = Job::Match(Box::new(MatchRequest {
            map: header.identity.map_id.clone(),
            setup: replay.body.setup.clone(),
            me: None,
            resume: None,
            session: None,
            colors,
            replay: Some(Arc::new(replay)),
            map_path,
            campaign: None,
            autoplay: false,
        }));
        self.last_match = None;
        self.start_loading(job, start, Origin::Replays);
    }

    /// Exports a save to a file the player picks (old `Export`).
    fn export_save(&mut self, meta: &SaveMeta) {
        let Some(path) = crate::files::save_as(&SaveLibrary::export_name(meta)) else {
            return;
        };
        self.saves_status = match self.library.export(&meta.id, &path) {
            Ok(meta) => tr!("menu.exported", name = &plain(&meta.name)),
            Err(error) => tr!("menu.cannot-export", error = &format!("{error:#}")),
        };
    }

    /// Imports an exported save the player picks (old `Import save…`): checked like a
    /// load (the map and content here, the match's mode in a match), then listed.
    fn import_save(&mut self) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let Some(path) = crate::files::open() else {
            return;
        };
        let mode = if self.flow.in_match() {
            self.playing.as_ref().and_then(|p| p.kind.save_mode())
        } else {
            None
        };
        let here = |identity: &MatchIdentity| -> Result<MatchIdentity> {
            let map = data.map(&identity.map_id).ok_or_else(|| {
                anyhow::anyhow!("the map {} is not in this game", identity.map_id)
            })?;
            let bytes = map.identity()?;
            Ok(data.match_identity(map, &bytes))
        };
        match self.library.import(&path, mode, here) {
            Ok(meta) => {
                self.list_saves();
                self.saves_status = tr!("menu.imported", name = &plain(&meta.name));
            }
            Err(error) => {
                self.saves_status = tr!("menu.cannot-import", error = &format!("{error:#}"))
            }
        }
    }

    fn open_saves(&mut self) {
        self.list_saves();
        self.saves_status.clear();
        self.apply(Intent::Show(Overlay::Saves));
    }

    fn list_saves(&mut self) {
        self.saves_list = self.library.list();
        // In a match only saves of its mode are listed (old `savesForMode`: campaign and
        // skirmish saves are kept apart); the main menu lists every save.
        if self.flow.in_match()
            && let Some(mode) = self.playing.as_ref().and_then(|p| p.kind.save_mode())
        {
            self.saves_list.retain(|s| s.mode == mode);
        }
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
            self.menu_status = tr!("menu.multiplayer-no-save");
            return;
        };
        let Some(body) = playing.save() else {
            self.menu_status = tr!("menu.match-no-answer");
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
            Ok(meta) => tr!("menu.saved", name = &plain(&meta.name)),
            Err(error) => tr!("menu.cannot-save", error = &format!("{error:#}")),
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
            window.set_fullscreen(crate::app::fullscreen(g.window, window.current_monitor()));
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

    /// The HUD size and layout settings on the in-match HUD (`hud.json`'s dock).
    fn apply_hud(&mut self) {
        let width = self.hud_width;
        if let Some(dock) = self.host.ui.find("hud/dock") {
            self.host
                .ui
                .set_scale(dock, Some(self.settings.hud.dock_scale(width)));
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
            .is_some_and(|p| p.newest().is_some_and(|s| s.view.mission.is_some()));
        let shows = [
            ("main-menu", screen == Screen::MainMenu),
            ("campaign", screen == Screen::Campaigns),
            ("campaign-steps", matches!(screen, Screen::Campaign { .. })),
            ("card", matches!(screen, Screen::Step { .. })),
            ("skirmish", screen == Screen::Skirmish),
            ("replays", screen == Screen::Replays),
            (
                "replay",
                in_match && self.playing.as_ref().is_some_and(|p| p.watching.is_some()),
            ),
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
            ("releases", top == Some(Overlay::Releases)),
            // Not in the frame that becomes its screenshot.
            (
                "feedback",
                top == Some(Overlay::Feedback) && !self.grab.due(),
            ),
            ("fade", true),
        ];
        for (name, show) in shows {
            self.host.ui.show(name, show);
        }
        // The result dialog says the outcome; the HUD's banner returns when it is hidden.
        if top == Some(Overlay::Result) && !self.result_hidden {
            widgets::visible(&mut self.host.ui, "hud/outcome", false);
        }
        // Tooltips off (a setting) applies to the HUD while it has the input.
        let hud_active = in_match && top.is_none();
        self.host
            .ui
            .set_tooltips(self.settings.hud.tooltips || !hud_active);
        if let Some(fade) = self.host.ui.find("fade") {
            self.host.ui.set_opacity(fade, self.fade);
        }
        let data = self.data.clone();
        let gpu = ctx.gpu;
        let t = held_clock().unwrap_or_else(|| (Instant::now() - self.started).as_secs_f32());
        match &screen {
            Screen::Boot => self.show_boot(),
            Screen::MainMenu => {
                menu::animate(&mut self.host, t, self.view);
                self.releases.show_plaque(&mut self.host.ui, &self.updater);
            }
            Screen::Campaigns => {
                if let Some(data) = &data {
                    campaign::show_cards(&mut self.host, gpu, data, &self.progress);
                }
            }
            Screen::Campaign { campaign } => {
                if let Some(data) = &data
                    && let Some(c) = data.campaign(campaign)
                {
                    let progress = self.campaign_progress(campaign);
                    self.campaign_entries =
                        campaign::show_steps(&mut self.host, gpu, data, c, &progress);
                    // The newest step is at the bottom: keep it in view as the list
                    // settles.
                    if self.scroll_steps > 0 {
                        self.scroll_steps -= 1;
                        if let Some(list) = self.host.ui.find("campaign-steps/steps") {
                            self.host.ui.scroll_to_end(list);
                        }
                    }
                }
            }
            Screen::Step { .. } => {
                if let (Some(data), Some(card)) = (&data, &self.card) {
                    campaign::show_card(&mut self.host, gpu, data, card);
                }
            }
            Screen::Skirmish => {
                if let Some(data) = &data {
                    let name = self.settings.player_name.trim().to_string();
                    self.skirmish
                        .show(&mut self.host, gpu, data, &self.palette, &name);
                }
            }
            Screen::Replays => {
                if let Some(data) = &data {
                    self.replays.show(&mut self.host, gpu, data, &self.palette);
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
            match_ui::show_overlay(&mut self.host, playing, self.problem.as_ref());
            if let Some(data) = &data {
                let newest = playing.newest();
                let mission = newest.and_then(|s| s.view.mission.clone());
                let tick = newest.map_or(0, |s| s.view.tick);
                let decided = newest.is_some_and(|s| s.view.outcome.is_some());
                let dt = 1.0 / 60.0;
                let hud_top = self
                    .host
                    .ui
                    .find("hud/dock")
                    .and_then(|d| self.host.ui.rect(d))
                    .map(|r| r.y / ctx.scale.max(0.1));
                self.mission_hud.hud_top = hud_top;
                self.mission_hud.shown = playing.game.shown_tick();
                self.mission_hud.live_speaker = playing.game.live_speaker;
                self.mission_hud.autoplay = playing.status().autoplay;
                self.mission_hud.show(
                    &mut self.host,
                    gpu,
                    data,
                    mission.as_ref(),
                    tick,
                    decided,
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
                let ui = &mut self.host.ui;
                let recorded = playing.recorder.is_some() && playing.info.is_some();
                widgets::visible(ui, "result/save-replay", recorded);
                let saved = matches!(self.replay_saved, Some(Ok(_)));
                widgets::enabled(ui, "result/save-replay", !saved);
                widgets::text(
                    ui,
                    "result/save-replay",
                    &if saved {
                        tr!("menu.replay-saved")
                    } else {
                        tr!("menu.save-replay")
                    },
                );
                let status = match &self.replay_saved {
                    Some(Ok(name)) => tr!("menu.replay-saved-as", name = &plain(name)),
                    Some(Err(error)) => {
                        format!(
                            "[color=#fca5a5]{}[/color]",
                            tr!("menu.replay-not-saved", error = &plain(error))
                        )
                    }
                    None => String::new(),
                };
                widgets::text(ui, "result/replay-status", &status);
                widgets::visible(ui, "result/replay-status", !status.is_empty());
            }
            if playing.watching.is_some()
                && let Some(data) = &data
            {
                self.replay_ui
                    .show(&mut self.host, gpu, data, playing, self.view, ctx.scale);
            }
        }
        match top {
            Some(Overlay::Settings) => {
                self.settings_ui
                    .show(&mut self.host, &self.settings, &self.catalog)
            }
            Some(Overlay::Saves) => self.show_saves(),
            Some(Overlay::Confirm) => self.show_dialog(),
            Some(Overlay::Releases) => self.releases.show(&mut self.host, gpu, &self.updater),
            Some(Overlay::Feedback) => self.feedback.show(&mut self.host.ui),
            _ => {}
        }
    }

    fn show_boot(&mut self) {
        let ui = &mut self.host.ui;
        widgets::visible(ui, "loading/mission-view", false);
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
                    &tr!("menu.game-could-not-start", error = &plain(error)),
                );
                widgets::text(ui, "loading/art-view/stage", "");
            }
            None => {
                widgets::visible(ui, "loading/art-view/error", false);
                widgets::text(ui, "loading/art-view/stage", &self.boot_stage.0.label());
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
        let (stage, progress, error) = (
            loading.job.as_ref().map(|j| (j.stage, j.progress)),
            loading.job.as_ref().map_or(1.0, |j| j.progress),
            loading.error.clone(),
        );
        // A campaign mission loads over its art with its story (option A, docs/campaign.md).
        let mission = self
            .campaign_play
            .clone()
            .filter(|_| loading.start.kind == MatchKind::Campaign);
        if let (Some(play), Some(data)) = (mission, &data)
            && let Some(c) = data.campaign(&play.campaign)
        {
            let state = campaign::Loading {
                progress,
                ready: loading.prepared.is_some(),
                error: error.as_deref(),
                time: loading.started.elapsed().as_secs_f32(),
            };
            campaign::show_loading(&mut self.host, gpu, data, c, play.step, &state);
            let ui = &mut self.host.ui;
            widgets::visible(ui, "loading/mission-view", true);
            widgets::visible(ui, "loading/art-view", false);
            widgets::visible(ui, "loading/panel-view", false);
            return;
        }
        let ui = &mut self.host.ui;
        // Other matches show the old plain panel.
        widgets::visible(ui, "loading/mission-view", false);
        widgets::visible(ui, "loading/art-view", false);
        widgets::visible(ui, "loading/panel-view", true);
        let label = stage.map_or_else(|| tr!("loading.starting-match"), |(s, _)| s.label());
        widgets::text(
            ui,
            "loading/panel-view/title",
            &if error.is_some() {
                tr!("loading.battlefield-failed")
            } else {
                tr!("loading.preparing")
            },
        );
        widgets::text(
            ui,
            "loading/panel-view/stage",
            if error.is_some() { "" } else { &label },
        );
        widgets::visible(ui, "loading/panel-view/progress", error.is_none());
        if let Some(bar) = ui.find("loading/panel-view/progress") {
            ui.set_value(bar, progress);
        }
        widgets::text(
            ui,
            "loading/panel-view/detail",
            &error
                .as_deref()
                .map_or_else(|| tr!("loading.starts-when-ready"), plain),
        );
        match error {
            Some(error) => {
                widgets::visible(ui, "loading/art-view/error", true);
                widgets::text(
                    ui,
                    "loading/art-view/error",
                    &tr!("loading.battlefield-failed-detail", error = &plain(&error)),
                );
                widgets::text(ui, "loading/art-view/stage", "");
            }
            None => {
                widgets::visible(ui, "loading/art-view/error", false);
                widgets::text(ui, "loading/art-view/stage", &label);
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
                saves::Mode::Skirmish => tr!("match.kind-skirmish"),
                saves::Mode::Campaign => tr!("match.kind-campaign"),
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
    ) -> Option<&canopy_render::FrameReport> {
        if let (Some(playing), true) = (&mut self.playing, self.flow.in_match()) {
            let mut host = Host {
                window: ctx.window,
                gpu: ctx.gpu,
                renderer: ctx.renderer,
                ui: &mut self.host.ui,
                audio: &mut self.audio,
                size: ctx.size,
                scale: ctx.scale,
            };
            return Some(playing.render(&mut host, output, ui_view));
        }
        let mut encoder = ctx
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ui") });
        let clear = wgpu::Color {
            r: 0.0306,
            g: 0.0509,
            b: 0.0588,
            a: 1.0,
        };
        ctx.renderer
            .ui_mut(ctx.gpu)
            .encode_alone(&mut encoder, ui_view, Some(clear));
        ctx.gpu.queue.submit([encoder.finish()]);
        None
    }
}

/// `CANOPY_MENU_TIME=<seconds>` holds the screens' animation clock there, so captures of
/// animated screens can be compared pixel for pixel.
fn held_clock() -> Option<f32> {
    static HELD: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    *HELD.get_or_init(|| std::env::var("CANOPY_MENU_TIME").ok()?.parse().ok())
}

/// What the match follows of the settings.
fn play_options(settings: &Settings, catalog: &[keys::Entry]) -> PlayOptions {
    PlayOptions {
        keymap: Keymap::new(&settings.keys, catalog),
        always_health_bars: settings.hud.always_show_health_bars,
        delivery_text: settings.gameplay.delivery_text,
    }
}

fn match_ui_clock(tick: u32) -> String {
    let seconds = tick / 40;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
