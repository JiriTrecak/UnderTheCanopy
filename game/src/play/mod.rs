//! Playing a match: the local player's input, selection, orders and HUD over the live
//! world (milestone M4, in-match part).
//!
//! [`Play`] is the in-match controller. It owns no simulation state: each frame it gets
//! the newest view of the player's seat ([`know::Knowledge`]), what the frame shows
//! (camera and pickable bodies) and the window's input, and it produces protocol
//! [`Action`]s through a [`Sink`] (the session's command handle in the game, a list in
//! tests), camera requests and menu events ([`Event`]), the HUD's contents
//! ([`hud::Hud`]) and what the world UI should show ([`WorldUi`]).
//!
//! Behaviour follows the old game (`session.ts`, `settlementHud.ts`,
//! `presentation/*`); see `docs/play.md`.

pub mod capture;
pub mod card;
pub mod cinematic;
pub mod debug;
pub mod follow;
pub mod hud;
pub mod input;
pub mod keys;
pub mod know;
pub mod orders;
pub mod overlay;
pub mod picking;
pub mod placement;
pub mod presets;
pub mod preview;
pub mod run;
pub mod selection;
pub mod shots;
pub mod sounds;

use crate::settings::Chord;
use canopy_content::game::Kind;
use canopy_render::camera::Ground;
use canopy_sim::{Action, action::CellPoint, terrain::Terrain};
use card::{Activate, Command};
use glam::{Vec2, Vec3};
use input::{Button, Input, Mods};
use keys::{Keymap, Shortcut};
use know::Knowledge;
use orders::{Marker, Target, Targeting};
use picking::{Body, Screen, Trees};
use placement::{Ghost, Placement};
use selection::{Candidate, ControlGroups, DOUBLE_PRESS, Recall, Selection};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// A left drag shorter than this is a click (old `CLICK_PX`), logical pixels.
pub const CLICK_PIXELS: f32 = 5.0;
/// A second click this close (logical pixels) within [`DOUBLE_PRESS`] selects the type.
pub const DOUBLE_CLICK_PIXELS: f32 = 6.0;
/// Seconds an error message shows, and its fade.
pub const ERROR_SECONDS: f32 = 2.5;
pub const ERROR_FADE_SECONDS: f32 = 0.3;
/// Error facts older than this many ticks are not shown when first seen.
pub const ERROR_FACT_TICKS: u32 = 5;
/// Recent alerts kept for Space, and how close (ticks, cells) repeats merge.
pub const MAX_ALERTS: usize = 8;
pub const ALERT_MERGE_TICKS: u32 = 120;
pub const ALERT_MERGE_CELLS: f32 = 10.0;
/// How far behind the ground point (metres along the ray) a tree still picks: its
/// trunk stands on the ground the ray meets.
pub const TREE_GROUND_SLACK: f32 = 0.5;

/// Where commands go.
pub trait Sink {
    /// Queues a command; false when it cannot be sent (observer, stopped session).
    fn send(&mut self, action: Action) -> bool;
}

impl Sink for canopy_net::Commands {
    fn send(&mut self, action: Action) -> bool {
        canopy_net::Commands::send(self, action)
    }
}

impl Sink for Vec<Action> {
    fn send(&mut self, action: Action) -> bool {
        self.push(action);
        true
    }
}

/// What the game around the match should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Centre the camera on this ground point (x, z).
    LookAt([f32; 2]),
    /// Open the game menu (pause menu; the screens own it).
    OpenMenu,
    /// Show or hide the debug overlay (the HUD's debug button).
    ToggleDebug,
    /// The player sent a chat line.
    Chat(String),
}

/// A sound the match asks for (D97); the match plays it from content.
#[derive(Debug, Clone, PartialEq)]
pub enum Cue {
    /// A unit answers its player.
    Answer { unit: u32, answer: Answer },
    /// A message reached the player, by its text key: the player's race may voice it.
    Alert(String),
}

/// What a unit answers to (its `presentation.voice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Select,
    Order,
    Attack,
    /// Selected again and again.
    Annoyed,
    /// Trained or revived.
    Ready,
}

/// Clicks on one unit within this of each other count towards annoying it.
const ANNOY_GAP: Duration = Duration::from_millis(1500);
/// The selection that annoys a unit (Warcraft III: the fourth or so in a row).
const ANNOY_CLICKS: u32 = 4;
/// The least time between two spoken "under attack" alerts of one kind.
const ATTACK_ALERT_GAP: Duration = Duration::from_secs(20);
/// The alert keys the match raises itself.
pub const ALERT_UNDER_ATTACK: &str = "alert.under-attack";
pub const ALERT_BASE_UNDER_ATTACK: &str = "alert.base-under-attack";

/// The armed command mode.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Idle,
    Target {
        targeting: Targeting,
        /// The command that armed it (`id`, actors), dropped if it disappears.
        source: (String, Vec<u32>),
        name: String,
    },
    Place {
        placement: Placement,
        source: (String, Vec<u32>),
        name: String,
    },
}

/// Which health bars show besides selected and damaged units (Alt, `[`, `]` held).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HealthBars {
    #[default]
    Default,
    All,
    Friendly,
    Hostile,
}

/// What the world UI pass draws for the local player: the plain input the renderer's
/// world UI consumes (selection rings, hover, health bars, order markers, the
/// placement ghost and grid, rally points).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldUi {
    pub selected: Vec<u32>,
    pub hovered: Option<u32>,
    pub health: HealthBars,
    /// Markers with their age in seconds.
    pub markers: Vec<(Marker, f32)>,
    pub ghost: Option<Ghost>,
    /// The placement's building definition and owner colour slot.
    pub ghost_definition: Option<String>,
    /// The box being dragged (physical pixels).
    pub drag: Option<[Vec2; 2]>,
    /// The focused producer's rally point (x, z).
    pub rally: Option<[f32; 2]>,
    /// The spell being aimed at the cursor.
    pub aim: Option<Aim>,
    /// A hovered tree the selection can gather from: its ground ring.
    pub tree: Option<Ring>,
    /// The power network's discs (x, z, radius) while placing a building that needs it
    /// or while the debug overlay shows it (D66).
    pub network: Vec<[f32; 3]>,
    /// Deliveries into the viewer's stock rise as "+N" text (a setting).
    pub deliveries: bool,
}

/// A ground ring (world units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    pub position: [f32; 2],
    pub radius: f32,
}

/// A spell aimed at the cursor (old `AbilityAim`): the caster, the aimed cell, the area
/// (a disc, or a line from the caster), the cast range and whether the aim is in range
/// and on ground in sight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aim {
    pub origin: [f32; 2],
    pub point: [f32; 2],
    /// Disc radius at the point (0.8 for unit spells, as the old outline).
    pub radius: f32,
    /// A line delivery's length and half width, from the caster towards the point.
    pub line: Option<(f32, f32)>,
    pub range: f32,
    pub valid: bool,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    at: Vec2,
    button: Button,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Alert {
    tick: u32,
    position: [f32; 2],
}

/// A line of the chat and message log.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatLine {
    pub name: String,
    /// sRGB colour of the name.
    pub color: [u8; 3],
    pub text: String,
    /// Seconds since the match started.
    pub at: f32,
}

pub struct Play {
    know: Knowledge,
    terrain: Arc<Terrain>,
    sink: Box<dyn Sink>,
    pub selection: Selection,
    pub groups: ControlGroups,
    pub mode: Mode,
    /// The open command category and page.
    pub menu: Option<String>,
    pub page: usize,
    screen: Option<Screen>,
    bodies: Vec<Body>,
    /// The map's standing trees as drawn, for picking.
    trees: Arc<Trees>,
    pointer: Option<Vec2>,
    press: Option<Press>,
    last_click: Option<(Vec2, Duration)>,
    mods: Mods,
    held: Vec<&'static str>,
    hovered: Option<u32>,
    /// The pointer is over the HUD: nothing in the world is hovered.
    over_ui: bool,
    ground_point: Option<Vec3>,
    markers: Vec<(Marker, Duration)>,
    message: Option<(String, Duration)>,
    seen_fact: Option<u32>,
    /// Refused commands show no message (autoplay plays: its refusals are not the
    /// player's).
    pub quiet_facts: bool,
    hero_press: Option<(usize, Duration)>,
    idle_cursor: usize,
    /// Queued work sent to each building since the view of this tick (`spread`).
    pending: (u32, BTreeMap<u32, u32>),
    dropoff_cursor: usize,
    alerts: Vec<Alert>,
    alert_cursor: usize,
    known_hp: BTreeMap<u32, (u32, bool)>,
    events: Vec<Event>,
    /// Sounds asked for since the match last took them.
    cues: Vec<Cue>,
    /// The unit selected last, how many times running and when (to annoy it).
    answered: Option<(u32, u32, Duration)>,
    /// When "under attack" was last said, for units and for buildings.
    attacked: [Option<Duration>; 2],
    now: Duration,
    /// Every command, the current menu and its layout, from the newest view.
    pub commands: Vec<Command>,
    pub entries: Vec<Command>,
    pub layout: card::Layout,
    pub chat: Vec<ChatLine>,
    pub chat_open: bool,
    /// The player's own lines show at once (no chat server echoes them: single player).
    pub echo_chat: bool,
    /// The player's name in the chat.
    pub name: Option<String>,
    /// The engine's shortcuts as the player bound them.
    pub keymap: Keymap,
    /// Health bars over every unit unless a hold narrows them (a setting).
    pub always_health_bars: bool,
    /// Deliveries into the player's stock rise as "+N" text (a setting).
    pub delivery_text: bool,
    /// The power network shows on the ground at all times (debug overlay, D66).
    pub show_network: bool,
    /// Commands sent, newest last (debug overlay).
    pub sent: Vec<(Duration, Action)>,
    /// The unit camera (J cycles it; Escape and camera jumps return to RTS).
    pub camera_mode: follow::CameraMode,
}

impl Play {
    pub fn new(know: Knowledge, terrain: Arc<Terrain>, sink: Box<dyn Sink>) -> Self {
        let mut play = Self {
            know,
            terrain,
            sink,
            selection: Selection::default(),
            groups: ControlGroups::default(),
            mode: Mode::Idle,
            menu: None,
            page: 0,
            screen: None,
            bodies: Vec::new(),
            trees: Arc::default(),
            pointer: None,
            press: None,
            last_click: None,
            mods: Mods::default(),
            held: Vec::new(),
            hovered: None,
            over_ui: false,
            ground_point: None,
            markers: Vec::new(),
            message: None,
            seen_fact: None,
            quiet_facts: false,
            hero_press: None,
            idle_cursor: 0,
            pending: (0, BTreeMap::new()),
            dropoff_cursor: 0,
            alerts: Vec::new(),
            alert_cursor: 0,
            known_hp: BTreeMap::new(),
            events: Vec::new(),
            cues: Vec::new(),
            answered: None,
            attacked: [None; 2],
            now: Duration::ZERO,
            commands: Vec::new(),
            entries: Vec::new(),
            layout: card::Layout::default(),
            chat: Vec::new(),
            chat_open: false,
            echo_chat: true,
            name: None,
            keymap: Keymap::default(),
            always_health_bars: false,
            delivery_text: true,
            show_network: false,
            sent: Vec::new(),
            camera_mode: follow::CameraMode::Rts,
        };
        // Facts from before the match was shown are not news.
        play.seen_fact = play.know.view.facts.last().map(|f| f.id);
        play.refresh();
        play
    }

    pub fn knowledge(&self) -> &Knowledge {
        &self.know
    }

    pub fn observer(&self) -> bool {
        self.know.me.is_none()
    }

    /// A mission cinematic holds the match (old `MissionHud.active`): a scene, a
    /// cinematic line still playing or cinematic mode, before the outcome and without a
    /// script error. Returns where the scene stands (x, z) when it has a place.
    /// Whether the mission's cinematic mode is on (`mission.cinematic`): bars, no HUD.
    pub fn cinema_on(&self) -> bool {
        let view = &self.know.view;
        view.outcome.is_none()
            && view
                .mission
                .as_ref()
                .is_some_and(|m| m.cinema.on && m.error.is_none())
    }

    pub fn cinematic(&self) -> Option<Option<[f32; 2]>> {
        let v = &self.know.view;
        let m = v.mission.as_ref()?;
        if v.outcome.is_some() || m.error.is_some() {
            return None;
        }
        let line = m
            .dialogue
            .as_ref()
            .is_some_and(|d| d.cinematic && d.remaining > 0);
        match &m.scene {
            Some(scene) => Some(Some([
                scene.position.x as f32 / 1000.0,
                scene.position.y as f32 / 1000.0,
            ])),
            None if line || m.cinema.on => Some(None),
            None => None,
        }
    }

    /// A scene plays that Escape may skip (and no skip is under way).
    pub fn skippable(&self) -> bool {
        let v = &self.know.view;
        v.outcome.is_none()
            && v.mission.as_ref().is_some_and(|m| {
                m.error.is_none()
                    && m.cinema
                        .scene
                        .as_ref()
                        .is_some_and(|s| s.skippable && !s.skipping)
            })
    }

    /// The camera shot of the scene a cinematic plays, if it has one.
    pub fn scene_camera(&self) -> Option<&canopy_sim::mission::state::Camera> {
        self.cinematic()?;
        self.know
            .view
            .mission
            .as_ref()?
            .scene
            .as_ref()?
            .camera
            .as_ref()
    }

    /// The map's trees as the static scene draws them (picked by their shape).
    pub fn set_trees(&mut self, trees: Arc<Trees>) {
        self.trees = trees;
    }

    /// Whether the pointer is over the HUD (no world hover there).
    pub fn set_over_ui(&mut self, over: bool) {
        self.over_ui = over;
    }

    /// Events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// The sounds asked for since the last call.
    pub fn take_cues(&mut self) -> Vec<Cue> {
        std::mem::take(&mut self.cues)
    }

    /// The newest frame: the view, what is drawn and where, and the time.
    pub fn update(
        &mut self,
        know: Knowledge,
        screen: Screen,
        bodies: Vec<Body>,
        ground: &dyn Ground,
        now: Duration,
    ) {
        let tick_changed = know.view.tick != self.know.view.tick;
        self.know = know;
        self.screen = Some(screen);
        self.bodies = bodies;
        self.now = now;
        if tick_changed {
            self.prune();
            self.read_facts();
            self.watch_alerts();
        }
        self.refresh();
        self.hover(ground);
        let seconds = |d: Duration| d.as_secs_f32();
        self.markers
            .retain(|(m, born)| seconds(now.saturating_sub(*born)) < m.lifetime());
        if let Some((_, at)) = &self.message
            && seconds(now.saturating_sub(*at)) > ERROR_SECONDS + ERROR_FADE_SECONDS
        {
            self.message = None;
        }
    }

    /// Drops dead, gone and no longer owned entities from the selection and groups.
    fn prune(&mut self) {
        let k = self.know.clone();
        let keep = |id: u32| k.entity(id).is_some_and(|e| k.selectable(e));
        if self.selection.prune(keep, |id| k.rank_of(id)) {
            self.mode = Mode::Idle;
        }
        self.groups
            .prune(|id| k.entity(id).is_some_and(|e| k.own(e) && k.alive(e)));
    }

    /// Rebuilds the command card from the newest view and drops a mode whose command
    /// went away or changed.
    fn refresh(&mut self) {
        self.commands = card::commands(&self.know, self.selection.ids());
        match card::camera(&self.know, self.selection.ids(), self.camera_mode) {
            Some(c) => self.commands.push(c),
            // The followed unit died, left sight or entered a building.
            None => self.camera_mode = follow::CameraMode::Rts,
        }
        self.entries = card::menu(&self.know, &self.commands, self.menu.as_deref());
        for c in self.commands.iter_mut().chain(self.entries.iter_mut()) {
            c.chord = self.keymap.command(&c.id, c.hotkey.as_deref());
        }
        if self.menu.is_some() && !self.entries.iter().any(|c| c.id == "back") {
            self.menu = None;
        }
        self.layout = card::layout(&self.entries, self.page);
        self.page = self.layout.page;
        card::grid_keys(&mut self.entries, &self.layout);
        let source = match &self.mode {
            Mode::Idle => None,
            Mode::Target { source, .. } | Mode::Place { source, .. } => Some(source.clone()),
        };
        if let Some((id, actors)) = source {
            let alive = self
                .commands
                .iter()
                .any(|c| c.id == id && c.actors == actors && c.enabled());
            if !alive {
                self.mode = Mode::Idle;
            }
        }
    }

    /// Shows new error facts (old: the newest unseen error fact, if under 5 ticks old).
    fn read_facts(&mut self) {
        let facts = &self.know.view.facts;
        let newest = facts.last().map(|f| f.id);
        if !self.observer() {
            for f in facts
                .iter()
                .filter(|f| self.seen_fact.is_none_or(|s| f.id > s))
            {
                if self.know.view.tick.saturating_sub(f.tick) < ERROR_FACT_TICKS {
                    self.cues.push(Cue::Alert(crate::text::key_of(&f.message)));
                }
            }
        }
        let unseen = facts.iter().rfind(|f| {
            self.seen_fact.is_none_or(|s| f.id > s) && f.kind == canopy_sim::state::FactKind::Error
        });
        if let Some(f) = unseen
            && !self.quiet_facts
            && self.know.view.tick.saturating_sub(f.tick) < ERROR_FACT_TICKS
        {
            self.message = Some((crate::text::message(&f.message), self.now));
        }
        if newest.is_some() {
            self.seen_fact = newest;
        }
    }

    /// Records alerts for Space: own entities damaged, finished or born.
    fn watch_alerts(&mut self) {
        let k = self.know.clone();
        let first = self.known_hp.is_empty();
        let mut now: BTreeMap<u32, (u32, bool)> = BTreeMap::new();
        let mut fresh = Vec::new();
        for e in k.view.entities.iter().filter(|e| k.own(e) && k.alive(e)) {
            let id = e.id().0;
            let hp = e.public.hp.unwrap_or(0);
            let building = e.public.construction.is_some();
            match self.known_hp.get(&id) {
                Some(&(old, was_building)) => {
                    if hp < old || (was_building && !building) {
                        fresh.push(k.position(e));
                    }
                    if hp < old {
                        self.attacked_at(k.position(e), !k.is_unit(e));
                    }
                }
                None if !first && k.is_unit(e) => {
                    fresh.push(k.position(e));
                    if k.controlled(e) {
                        self.cues.push(Cue::Answer {
                            unit: id,
                            answer: Answer::Ready,
                        });
                    }
                }
                None => {}
            }
            now.insert(id, (hp, building));
        }
        self.known_hp = now;
        let tick = k.view.tick;
        for position in fresh {
            let near = self.alerts.iter().any(|a| {
                tick.saturating_sub(a.tick) <= ALERT_MERGE_TICKS
                    && (a.position[0] - position[0]).hypot(a.position[1] - position[1])
                        <= ALERT_MERGE_CELLS
            });
            if !near {
                self.alerts.insert(0, Alert { tick, position });
                self.alerts.truncate(MAX_ALERTS);
                self.alert_cursor = 0;
            }
        }
    }

    /// Says "under attack" (for a building: "our base") when an own entity is hurt out of
    /// sight of the camera, at most once a while for each.
    fn attacked_at(&mut self, position: [f32; 2], building: bool) {
        if self.observer() {
            return;
        }
        let shown = self.screen.is_some_and(|screen| {
            screen
                .project(Vec3::new(position[0], 0.0, position[1]))
                .is_some_and(|(px, _)| {
                    px.x >= 0.0 && px.y >= 0.0 && px.x <= screen.size[0] && px.y <= screen.size[1]
                })
        });
        let last = &mut self.attacked[usize::from(building)];
        if shown || last.is_some_and(|t| self.now.saturating_sub(t) < ATTACK_ALERT_GAP) {
            return;
        }
        *last = Some(self.now);
        let key = if building {
            ALERT_BASE_UNDER_ATTACK
        } else {
            ALERT_UNDER_ATTACK
        };
        self.cues.push(Cue::Alert(key.into()));
    }

    /// The first of `ids` the player commands that answers (a unit).
    fn answering(&self, ids: &[u32]) -> Option<u32> {
        let k = &self.know;
        ids.iter().copied().find(|&id| {
            k.entity(id)
                .is_some_and(|e| k.controlled(e) && k.is_unit(e))
        })
    }

    /// Re-picks what is under the cursor.
    fn hover(&mut self, ground: &dyn Ground) {
        let (Some(screen), Some(px)) = (self.screen, self.pointer) else {
            self.hovered = None;
            self.ground_point = None;
            return;
        };
        self.ground_point = screen.ground(px, ground);
        self.hovered = match self.mode {
            _ if self.over_ui => None,
            Mode::Place { .. } => None,
            _ => self.pick(px, ground).map(|t| t.id),
        };
    }

    /// The entity under a pixel: a unit's capsule; else the building, item or resource
    /// whose footprint holds the ground point (old fallback rule) or the standing tree
    /// the camera ray enters, whichever the ray meets first; else known scenery without
    /// a drawn shape by its footprint.
    fn pick(&self, px: Vec2, ground: &dyn Ground) -> Option<Target> {
        let screen = self.screen?;
        let k = &self.know;
        if let Some(id) = screen.pick_unit(&self.bodies, px)
            && k.entity(id).is_some_and(|e| k.selectable(e))
        {
            return Target::of(k, id);
        }
        let (origin, dir) = screen.ray(px);
        let hit = screen.ground(px, ground);
        let reach = |d: &canopy_content::game::Definition| {
            d.footprint
                .as_ref()
                .map_or(0.8, |f| f.width.max(f.depth) as f32 / 2.0)
        };
        let inside = |p: [f32; 2], r: f32| {
            hit.is_some_and(|hit| (p[0] - hit.x).abs() <= r && (p[1] - hit.z).abs() <= r)
        };
        let footprint = k
            .view
            .entities
            .iter()
            .find(|e| {
                let d = k.def(e);
                d.kind != Kind::Unit && k.selectable(e) && inside(k.position(e), reach(d))
            })
            .map(|e| e.id().0);
        // Trees are picked by their drawn shape, up to just behind the ground point.
        let far = hit.map_or(screen.camera.far.max(64.0) * 1.5, |h| {
            h.distance(origin) + TREE_GROUND_SLACK
        });
        let tree = self.trees.hit(origin, dir, far, |id| k.standing(id));
        let id = match (footprint, tree) {
            (Some(id), Some((tree, t))) => {
                // Whichever the ray meets first: the building's body or the tree.
                let depth = self
                    .bodies
                    .iter()
                    .find(|b| b.id == id)
                    .and_then(|b| b.enter(origin, dir, far))
                    .or_else(|| hit.map(|h| h.distance(origin)))
                    .unwrap_or(f32::MAX);
                Some(if t < depth { tree } else { id })
            }
            (footprint, tree) => footprint.or(tree.map(|(id, _)| id)),
        };
        if let Some(id) = id {
            return Target::of(k, id);
        }
        // Other known scenery resources (no drawn shape) by their footprint.
        k.view
            .scenery
            .iter()
            .find(|s| {
                self.trees.get(s.id.0).is_none()
                    && s.resource.as_ref().is_some_and(|r| r.amount > 0)
                    && inside(
                        [s.position.x as f32 / 1000.0, s.position.y as f32 / 1000.0],
                        reach(k.catalog.get(s.definition)),
                    )
            })
            .and_then(|s| Target::of(k, s.id.0))
    }

    /// The ring of the hovered tree when the selection has a unit that could gather it
    /// (a right-click would): trees show they are clickable like mines do.
    pub fn tree_ring(&self) -> Option<Ring> {
        if !matches!(self.mode, Mode::Idle) || self.cinematic().is_some() {
            return None;
        }
        let k = &self.know;
        let id = self.hovered?;
        if !k.standing(id) {
            return None;
        }
        let target = Target::of(k, id)?;
        if !orders::commanded(k, self.selection.ids())
            .iter()
            .any(|&a| orders::harvests(k, a, &target))
        {
            return None;
        }
        let (position, radius) = self
            .trees
            .ring(id)
            .unwrap_or((target.position, orders::GATHER_RADIUS));
        Some(Ring { position, radius })
    }

    /// The ground cell under a pixel, as a command destination.
    fn cell(&self, px: Vec2, ground: &dyn Ground) -> Option<CellPoint> {
        let hit = self.screen?.ground(px, ground)?;
        let size = self.terrain.size() as f32;
        let (x, z) = (hit.x.round(), hit.z.round());
        (x >= 0.0 && z >= 0.0 && x < size && z < size).then_some(CellPoint {
            x: x as u32,
            y: z as u32,
            surface: None,
        })
    }

    fn logical(&self, pixels: f32) -> f32 {
        pixels * self.screen.map_or(1.0, |s| s.scale)
    }

    /// Sends commands, showing each marker once its command is queued.
    fn send(&mut self, commands: Vec<(Action, Option<Marker>)>) {
        let mut answered = false;
        for (action, marker) in commands {
            if action.problem().is_some() {
                continue;
            }
            let answer = match &action {
                Action::Attack { actors, .. } => Some((actors.clone(), Answer::Attack)),
                Action::Move { actors, .. }
                | Action::Patrol { actors, .. }
                | Action::Follow { actors, .. }
                | Action::Gather { actors, .. }
                | Action::Construct { actors, .. }
                | Action::Build { actors, .. }
                | Action::Garrison { actors, .. } => Some((actors.clone(), Answer::Order)),
                Action::CastAbility { actor, .. } => Some((vec![*actor], Answer::Order)),
                _ => None,
            };
            if self.sink.send(action.clone()) {
                if let Some((actors, answer)) = answer
                    && !answered
                    && let Some(unit) = self.answering(&actors)
                {
                    answered = true;
                    self.cues.push(Cue::Answer { unit, answer });
                }
                self.sent.push((self.now, action));
                if self.sent.len() > 64 {
                    self.sent.remove(0);
                }
                if let Some(m) = marker {
                    if self.markers.len() >= orders::MAX_MARKERS {
                        self.markers.remove(0);
                    }
                    self.markers.push((m, self.now));
                }
            }
        }
    }

    /// Shows a red message above the HUD.
    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), self.now));
    }

    /// The message to show and its opacity.
    pub fn message(&self) -> Option<(&str, f32)> {
        let (text, at) = self.message.as_ref()?;
        let age = self.now.saturating_sub(*at).as_secs_f32();
        let fade =
            ((ERROR_SECONDS + ERROR_FADE_SECONDS - age) / ERROR_FADE_SECONDS).clamp(0.0, 1.0);
        Some((text, fade))
    }

    /// The targeting hint above the HUD (old hint text, with the player's keys).
    pub fn hint(&self) -> Option<String> {
        let cancel = match self.keymap.label(Shortcut::Cancel) {
            "" => tr!("keys.esc"),
            label => label.to_owned(),
        };
        match &self.mode {
            Mode::Idle => None,
            Mode::Target {
                targeting, name, ..
            } => Some(tr!(
                "play.target-hint",
                name = name.as_str(),
                wants = &targeting.wants(),
                cancel = &cancel
            )),
            Mode::Place {
                placement, name, ..
            } => {
                let problem = self.ghost().and_then(|g| g.problem);
                Some(tr!(
                    "play.place-hint",
                    name = name.as_str(),
                    problem =
                        &problem.map_or_else(|| tr!("play.click-to-build"), |p| format!("{p}.")),
                    clockwise = self.keymap.label(Shortcut::RotateClockwise),
                    counterclockwise = self.keymap.label(Shortcut::RotateCounterclockwise),
                    degrees = placement.rotation,
                    cancel = &cancel
                ))
            }
        }
    }

    /// The placement ghost at the cursor.
    pub fn ghost(&self) -> Option<Ghost> {
        let Mode::Place { placement, .. } = &self.mode else {
            return None;
        };
        let p = self.ground_point?;
        placement::ghost(&self.know, &self.terrain, placement, p.x, p.z)
    }

    pub fn world_ui(&self) -> WorldUi {
        let held = |shortcut| {
            self.held
                .iter()
                .any(|k| self.keymap.holds(shortcut, &keys::code_of(k)))
        };
        let health = if held(Shortcut::HealthAll) {
            HealthBars::All
        } else if held(Shortcut::HealthFriendly) {
            HealthBars::Friendly
        } else if held(Shortcut::HealthEnemy) {
            HealthBars::Hostile
        } else if self.always_health_bars {
            HealthBars::All
        } else {
            HealthBars::Default
        };
        let drag = match (self.press, self.pointer) {
            (
                Some(Press {
                    at,
                    button: Button::Left,
                }),
                Some(p),
            ) if at.distance(p) >= self.logical(CLICK_PIXELS) => Some([at, p]),
            _ => None,
        };
        let rally = self
            .selection
            .focus()
            .and_then(|id| self.know.entity(id))
            .filter(|e| self.know.own(e))
            .and_then(|e| e.private.as_ref())
            .and_then(|p| p.economy.production.as_ref())
            .and_then(|p| p.rally.as_ref())
            .map(|r| [r.point.x as f32 / 1000.0, r.point.y as f32 / 1000.0]);
        WorldUi {
            // No selection rings during a cinematic or in cinematic mode; the selection
            // itself stays (old `gameSelect([])`).
            selected: if self.cinematic().is_some() || self.cinema_on() {
                Vec::new()
            } else {
                self.selection.ids().to_vec()
            },
            hovered: self.hovered,
            health,
            markers: self
                .markers
                .iter()
                .map(|(m, born)| (*m, self.now.saturating_sub(*born).as_secs_f32()))
                .collect(),
            ghost: self.ghost(),
            ghost_definition: match &self.mode {
                Mode::Place { placement, .. } => Some(placement.definition.clone()),
                _ => None,
            },
            drag,
            rally,
            aim: self.aim(),
            tree: self.tree_ring(),
            network: self.network_discs(),
            deliveries: self.delivery_text,
        }
    }

    /// The viewer's power network (D66), shown only while a building that needs it is
    /// being placed, or when the debug overlay asks for it.
    fn network_discs(&self) -> Vec<[f32; 3]> {
        let placing = match &self.mode {
            Mode::Place { placement, .. } => self
                .know
                .content
                .definition(&placement.definition)
                .is_some_and(|d| d.power.is_some()),
            _ => false,
        };
        if !placing && !self.show_network {
            return Vec::new();
        }
        self.know
            .view
            .network
            .fields
            .iter()
            .map(|f| {
                [
                    f.x as f32 / 1000.0,
                    f.y as f32 / 1000.0,
                    f.radius as f32 / 1000.0,
                ]
            })
            .collect()
    }

    /// The spell aimed at the cursor (old `onHover` → `gameAbilityTarget`).
    pub fn aim(&self) -> Option<Aim> {
        let Mode::Target {
            targeting: orders::Targeting::Cast { actor, binding, .. },
            ..
        } = &self.mode
        else {
            return None;
        };
        let k = &self.know;
        let hit = self.ground_point?;
        let caster = k.entity(*actor)?;
        let policy = k.catalog.spells.caster(caster.definition())?;
        let index = policy.bindings.iter().position(|b| &b.id == binding)?;
        let rank = caster
            .magic
            .as_ref()
            .and_then(|m| m.caster.as_ref())
            .and_then(|c| c.ranks.get(index).copied())
            .unwrap_or(0);
        if rank == 0 {
            return None;
        }
        let ability = &k.content.abilities.abilities[usize::from(policy.bindings[index].ability.0)];
        let value = |a: &canopy_content::abilities::Amount| {
            canopy_sim::abilities::library::value(ability, rank, a) as f32
        };
        let origin = k.position(caster);
        let point = [hit.x.round(), hit.z.round()];
        let range = value(&ability.targeting.range);
        let radius = match (&ability.targeting.kind, &ability.targeting.radius) {
            (canopy_content::abilities::TargetingKind::Point, Some(r)) => value(r),
            _ => 0.8,
        };
        let line = match &ability.delivery {
            Some(canopy_content::abilities::Delivery::Line { length, width, .. }) => {
                Some((*length as f32, *width as f32))
            }
            _ => None,
        };
        let size = self.terrain.size() as f32;
        let in_sight = point[0] >= 0.0
            && point[1] >= 0.0
            && point[0] < size
            && point[1] < size
            && k.view.fog.as_ref().is_none_or(|f| {
                f.cells
                    .get((point[1] * size + point[0]) as usize)
                    .is_some_and(|&c| c == 2)
            });
        let valid = (point[0] - origin[0]).hypot(point[1] - origin[1]) <= range && in_sight;
        Some(Aim {
            origin,
            point,
            radius,
            line,
            range,
            valid,
        })
    }

    // --- Selection ---------------------------------------------------------------

    fn candidate(&self, id: u32) -> Option<Candidate> {
        let k = &self.know;
        let e = k.entity(id)?;
        let d = k.def(e);
        Some(Candidate {
            id,
            rank: know::rank(d),
            controlled: k.controlled(e),
            unit: d.kind == Kind::Unit,
            building: d.kind == Kind::Building,
            hero: d.is_hero(),
            army: d.selection_class == Some(canopy_content::game::SelectionClass::Army),
            worker: d.behaviors.work.is_some(),
            kind: e.definition().0,
        })
    }

    /// Replaces the selection (clears any mode, as the old `setSelection`).
    pub fn select(&mut self, ids: Vec<u32>, focus: Option<u32>) {
        let k = self.know.clone();
        if let Some(unit) = self.answering(&ids) {
            let clicks = match self.answered {
                Some((last, n, at)) if last == unit && self.now.saturating_sub(at) < ANNOY_GAP => {
                    n + 1
                }
                _ => 1,
            };
            let answer = if clicks >= ANNOY_CLICKS {
                Answer::Annoyed
            } else {
                Answer::Select
            };
            self.answered = Some((
                unit,
                if clicks >= ANNOY_CLICKS { 0 } else { clicks },
                self.now,
            ));
            self.cues.push(Cue::Answer { unit, answer });
        }
        self.selection.set(ids, focus, |id| k.rank_of(id));
        self.mode = Mode::Idle;
        self.menu = None;
        self.page = 0;
        self.refresh();
    }

    /// Centres the camera on the visible members of `ids`.
    fn look_at(&mut self, ids: &[u32]) {
        let k = &self.know;
        let points: Vec<[f32; 2]> = ids
            .iter()
            .filter_map(|&id| k.entity(id))
            .filter(|e| !e.remembered && !k.held(e.id().0))
            .map(|e| k.position(e))
            .collect();
        if points.is_empty() {
            return;
        }
        let n = points.len() as f32;
        let x = points.iter().map(|p| p[0]).sum::<f32>() / n;
        let z = points.iter().map(|p| p[1]).sum::<f32>() / n;
        self.events.push(Event::LookAt([x, z]));
    }

    fn click_select(&mut self, px: Vec2, target: Option<Target>) {
        let k = self.know.clone();
        let shift = self.mods.shift;
        let double = self.last_click.is_some_and(|(at, t)| {
            self.now.saturating_sub(t) <= DOUBLE_PRESS
                && at.distance(px) <= self.logical(DOUBLE_CLICK_PIXELS)
        });
        self.last_click = Some((px, self.now));
        let Some(t) = target else {
            if !shift {
                self.select(Vec::new(), None);
            }
            return;
        };
        // Missions count the player's clicks on placed units (`mission.clicks`).
        if !self.observer() && self.know.view.mission.is_some() {
            self.send(vec![(Action::Clicked { actor: t.id }, None)]);
        }
        // Own units and own buildings gather into a selection; anything else is
        // selected alone.
        let groupable = |e: &canopy_sim::view::EntityView| {
            k.controlled(e) && (k.is_unit(e) || k.def(e).kind == Kind::Building)
        };
        let own = k.entity(t.id).is_some_and(groupable);
        if self.observer() {
            let mut ids = if shift {
                self.selection.ids().to_vec()
            } else {
                Vec::new()
            };
            ids.retain(|&x| x != t.id);
            ids.push(t.id);
            self.select(ids, None);
            return;
        }
        if own && (self.mods.command() || double) {
            // Every own unit or building of the type on screen.
            let screen = self.screen;
            let mut ids: Vec<u32> = k
                .view
                .entities
                .iter()
                .filter(|e| e.definition() == t.definition && groupable(e))
                .filter(|e| {
                    let [x, z] = k.position(e);
                    screen.is_some_and(|s| s.on_screen(Vec3::new(x, 1.0, z)))
                })
                .map(|e| e.id().0)
                .collect();
            if shift {
                let mut all = self.selection.ids().to_vec();
                all.append(&mut ids);
                ids = all;
            }
            self.select(ids, None);
            return;
        }
        if own && shift {
            let mut ids: Vec<u32> = self
                .selection
                .ids()
                .iter()
                .copied()
                .filter(|&id| k.entity(id).is_some_and(groupable))
                .collect();
            if ids.contains(&t.id) {
                ids.retain(|&x| x != t.id);
            } else {
                ids.push(t.id);
            }
            self.select(ids, None);
            return;
        }
        self.select(vec![t.id], None);
    }

    fn box_select(&mut self, a: Vec2, b: Vec2) {
        let Some(screen) = self.screen else {
            return;
        };
        let k = self.know.clone();
        let inside: Vec<Candidate> = k
            .view
            .entities
            .iter()
            .filter(|e| {
                !e.remembered
                    && (k.is_unit(e) || k.def(e).kind == Kind::Building)
                    && k.selectable(e)
            })
            .filter(|e| {
                let [x, z] = k.position(e);
                let y = self
                    .bodies
                    .iter()
                    .find(|b| b.id == e.id().0)
                    .map_or(0.0, |b| b.foot.y);
                screen.inside(Vec3::new(x, y + 1.0, z), a, b)
            })
            .filter_map(|e| self.candidate(e.id().0))
            .collect();
        let mut ids = selection::area(&inside, self.observer());
        if self.mods.shift {
            let mut all = self.selection.ids().to_vec();
            all.append(&mut ids);
            ids = all;
        }
        self.select(ids, None);
    }

    // --- Input -------------------------------------------------------------------

    /// Handles a world input event (the UI declined it).
    pub fn input(&mut self, event: &Input, ground: &dyn Ground) {
        match event {
            Input::PointerMoved(p) => {
                self.pointer = Some(*p);
                self.hover(ground);
            }
            Input::PointerLeft => {
                self.pointer = None;
                self.hovered = None;
            }
            Input::Modifiers(m) => self.mods = *m,
            Input::FocusLost => {
                self.press = None;
                self.held.clear();
                self.mods = Mods::default();
            }
            // A cinematic takes no world clicks (old: input reset every frame).
            Input::Button { pressed: true, .. } if self.cinematic().is_some() => {
                self.press = None;
            }
            Input::Button { button, pressed } => self.button(*button, *pressed, ground),
            Input::Key {
                key,
                pressed,
                repeat,
            } => {
                if *pressed && self.cinematic().is_some() {
                    // Only the game menu opens during a cinematic (old `blockKey`);
                    // Escape still reaches it (D39).
                    let chord = Chord {
                        code: keys::code_of(key),
                        ctrl: self.mods.ctrl,
                        alt: self.mods.alt && *key != "Alt",
                        shift: self.mods.shift,
                        meta: self.mods.logo,
                    };
                    let menu = self.keymap.pressed(&chord).contains(&Shortcut::GameMenu);
                    if !*repeat && *key == "Escape" && !menu && self.skippable() {
                        // Escape skips a scene, as in Warcraft III; the menu key still
                        // opens the menu.
                        self.send(vec![(Action::SkipScene, None)]);
                    } else if !*repeat && (*key == "Escape" || menu) {
                        self.events.push(Event::OpenMenu);
                    }
                    return;
                }
                if *pressed {
                    if !self.held.contains(key) {
                        self.held.push(key);
                    }
                    if !repeat {
                        self.key(key);
                    }
                } else {
                    self.held.retain(|k| k != key);
                }
            }
        }
    }

    fn button(&mut self, button: Button, pressed: bool, ground: &dyn Ground) {
        let Some(px) = self.pointer else {
            return;
        };
        match (button, pressed) {
            (Button::Left, true) => self.press = Some(Press { at: px, button }),
            (Button::Left, false) => {
                let Some(press) = self.press.take() else {
                    return;
                };
                if press.at.distance(px) >= self.logical(CLICK_PIXELS) {
                    self.box_select(press.at, px);
                } else {
                    self.left_click(px, ground);
                }
            }
            // Right-click acts on press (old pointer-down dispatch).
            (Button::Right, true) => {
                if self.mode != Mode::Idle {
                    self.mode = Mode::Idle;
                    return;
                }
                if self.observer() {
                    return;
                }
                let target = self.pick(px, ground);
                let target = self.lookout_redirect(target);
                let ground_cell = self.cell(px, ground);
                let commands = orders::smart(
                    &self.know,
                    self.selection.ids(),
                    target.as_ref(),
                    ground_cell.as_ref(),
                    self.mods.shift,
                );
                self.send(commands);
            }
            _ => {}
        }
    }

    /// An enemy inside a lookout is attacked through its tower (old redirect).
    fn lookout_redirect(&self, target: Option<Target>) -> Option<Target> {
        let t = target?;
        let k = &self.know;
        let tower = k
            .entity(t.id)
            .filter(|e| !k.own(e))
            .and_then(|e| e.combat.as_ref())
            .and_then(|c| c.garrison);
        match tower {
            Some(b) => Target::of(k, b.0).or(Some(t)),
            None => Some(t),
        }
    }

    /// Completes the aim on unit `id`: a hero button or a group card clicked while an
    /// ability or an attack is aimed targets that unit, as in Warcraft III. Whether an aim
    /// took the click (the selection then stays as it is).
    pub fn target_unit(&mut self, id: u32) -> bool {
        let Mode::Target { targeting, .. } = self.mode.clone() else {
            return false;
        };
        let k = self.know.clone();
        let Some(target) = Target::of(&k, id) else {
            return false;
        };
        let cell = CellPoint {
            x: target.position[0].round().max(0.0) as u32,
            y: target.position[1].round().max(0.0) as u32,
            surface: None,
        };
        let (commands, keep) =
            orders::complete(&k, &targeting, Some(&target), Some(&cell), self.mods.shift);
        self.send(commands);
        if !keep {
            self.mode = Mode::Idle;
        }
        true
    }

    fn left_click(&mut self, px: Vec2, ground: &dyn Ground) {
        match self.mode.clone() {
            Mode::Place { placement, .. } => {
                let Some(ghost) = self.ghost() else {
                    return;
                };
                if let Some(problem) = &ghost.problem {
                    self.error(problem.clone());
                    return;
                }
                let append = self.mods.shift;
                self.send(vec![(
                    Action::Build {
                        actors: placement.actors.clone(),
                        definition: canopy_content::Id::new(placement.definition.clone())
                            .expect("a content id"),
                        position: ghost.point.clone(),
                        rotation: Some(placement.rotation),
                        append: append.then_some(true),
                    },
                    None,
                )]);
                // Shift keeps placing (with the same rotation).
                if !append {
                    self.mode = Mode::Idle;
                }
            }
            Mode::Target { targeting, .. } => {
                let target = self.pick(px, ground);
                let target = if matches!(targeting, Targeting::Attack { .. }) {
                    self.lookout_redirect(target)
                } else {
                    target
                };
                let cell = self.cell(px, ground);
                let (commands, keep) = orders::complete(
                    &self.know,
                    &targeting,
                    target.as_ref(),
                    cell.as_ref(),
                    self.mods.shift,
                );
                self.send(commands);
                if !keep {
                    self.mode = Mode::Idle;
                }
            }
            Mode::Idle => {
                let target = self.pick(px, ground);
                self.click_select(px, target);
            }
        }
    }

    fn key(&mut self, key: &'static str) {
        if key == "Enter" {
            self.chat_open = true;
            return;
        }
        // The engine's shortcuts from the player's bindings, then the content's command
        // hotkeys.
        let chord = Chord {
            code: keys::code_of(key),
            ctrl: self.mods.ctrl,
            alt: self.mods.alt && key != "Alt",
            shift: self.mods.shift,
            meta: self.mods.logo,
        };
        for shortcut in self.keymap.pressed(&chord) {
            if self.shortcut(shortcut) {
                return;
            }
        }
        // Escape always leaves a mode, a menu or reaches the game menu, whatever the
        // bindings (it cannot be lost to a rebinding).
        if key == "Escape" {
            self.shortcut(Shortcut::Cancel);
            return;
        }
        self.content_key(key);
    }

    /// Runs an engine shortcut; false when it does not apply now (the key then goes on
    /// to the command card).
    fn shortcut(&mut self, shortcut: Shortcut) -> bool {
        match shortcut {
            Shortcut::Recall(slot) => {
                if let Some(recall) = self.groups.recall(slot, self.now) {
                    match recall {
                        Recall::Select(ids) => self.select(ids, None),
                        Recall::Focus(ids) => {
                            self.select(ids.clone(), None);
                            self.look_at(&ids);
                        }
                    }
                }
            }
            Shortcut::Assign(slot) => {
                let ids = self.groupable();
                self.groups.assign(slot, &ids);
            }
            Shortcut::Add(slot) => {
                let ids = self.groupable();
                self.groups.add(slot, &ids);
            }
            Shortcut::Alerts => {
                if !self.alerts.is_empty() {
                    let a = self.alerts[self.alert_cursor % self.alerts.len()];
                    self.alert_cursor += 1;
                    self.events.push(Event::LookAt(a.position));
                }
            }
            Shortcut::Hero(index) => self.hero_key(index),
            Shortcut::IdleWorker => self.next_idle_worker(),
            Shortcut::Army => self.select_army(),
            Shortcut::NextHall => self.next_dropoff(),
            Shortcut::NextSubgroup | Shortcut::PreviousSubgroup => {
                let cands: Vec<Candidate> = self
                    .selection
                    .ids()
                    .iter()
                    .filter_map(|&id| self.candidate(id))
                    .collect();
                self.selection
                    .cycle(&cands, shortcut == Shortcut::NextSubgroup);
                self.menu = None;
                self.page = 0;
                self.mode = Mode::Idle;
                self.refresh();
            }
            Shortcut::Inventory(slot) => self.use_item(slot),
            Shortcut::GameMenu => self.events.push(Event::OpenMenu),
            Shortcut::Cancel => {
                if self.mode != Mode::Idle {
                    self.mode = Mode::Idle;
                } else if self.camera_mode != follow::CameraMode::Rts {
                    self.camera_mode = follow::CameraMode::Rts;
                    self.refresh();
                } else if self.menu.is_some() {
                    self.activate_id("back", false);
                } else {
                    self.events.push(Event::OpenMenu);
                }
            }
            Shortcut::RotateClockwise | Shortcut::RotateCounterclockwise => {
                let Mode::Place { placement, .. } = &mut self.mode else {
                    return false;
                };
                placement.rotate(shortcut == Shortcut::RotateCounterclockwise);
            }
            Shortcut::CenterSelection => {
                let ids = self.selection.ids().to_vec();
                if ids.is_empty() {
                    if let Some(hall) = self.home() {
                        self.events.push(Event::LookAt(hall));
                    }
                } else {
                    self.look_at(&ids);
                }
            }
            // Held keys (camera, health bars) and window-level switches act elsewhere;
            // the press is still theirs.
            Shortcut::ZoomIn
            | Shortcut::ZoomOut
            | Shortcut::PanLeft
            | Shortcut::PanRight
            | Shortcut::PanUp
            | Shortcut::PanDown
            | Shortcut::HealthAll
            | Shortcut::HealthFriendly
            | Shortcut::HealthEnemy
            | Shortcut::ToggleHud
            | Shortcut::Debug
            | Shortcut::TakeOver
            | Shortcut::AutoplayFaster
            | Shortcut::AutoplaySlower => {}
        }
        true
    }

    /// A key no engine shortcut took: the command card's hotkeys.
    fn content_key(&mut self, key: &'static str) {
        let shift = self.mods.shift;
        let visible: Vec<Command> = self
            .layout
            .slots
            .iter()
            .flatten()
            .chain(&self.layout.banners)
            .map(|&i| self.entries[i].clone())
            .collect();
        let all = self.commands.clone();
        let pressed = Chord {
            code: keys::code_of(key),
            ctrl: self.mods.ctrl,
            alt: self.mods.alt,
            shift,
            meta: self.mods.logo,
        };
        if let Some((c, queue)) = card::shortcut(&visible, &all, &pressed) {
            let id = c.id.clone();
            self.activate_id(&id, queue);
        }
    }

    /// Own controlled entities of the selection (what a group may hold).
    fn groupable(&self) -> Vec<u32> {
        let k = &self.know;
        self.selection
            .ids()
            .iter()
            .copied()
            .filter(|&id| k.entity(id).is_some_and(|e| k.controlled(e)))
            .collect()
    }

    /// Own heroes, fallen ones included, by id.
    pub fn heroes(&self) -> Vec<u32> {
        let k = &self.know;
        let mut ids: Vec<u32> = k
            .view
            .entities
            .iter()
            .filter(|e| !e.remembered && k.owner() == Some(e.public.owner) && k.def(e).is_hero())
            .map(|e| e.id().0)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Whether a hero can be selected now.
    pub fn hero_available(&self, id: u32) -> bool {
        self.know
            .entity(id)
            .is_some_and(|e| self.know.selectable(e))
    }

    /// F2–F4 and hero buttons: select; a second press within 350 ms centres.
    pub fn hero_key(&mut self, index: usize) {
        let Some(&id) = self.heroes().get(index) else {
            return;
        };
        if !self.hero_available(id) {
            return;
        }
        let double = self
            .hero_press
            .is_some_and(|(i, t)| i == index && self.now.saturating_sub(t) <= DOUBLE_PRESS);
        self.hero_press = Some((index, self.now));
        self.select(vec![id], None);
        if double {
            self.look_at(&[id]);
        }
    }

    /// Own workers with nothing to do (old idle rule: no order, queue, job, workplace or
    /// pending move, no cargo, not held, not being released, not stunned).
    pub fn idle_workers(&self) -> Vec<u32> {
        let k = &self.know;
        k.view
            .entities
            .iter()
            .filter(|e| k.controlled(e) && k.is_unit(e) && k.def(e).behaviors.work.is_some())
            .filter(|e| {
                e.private.as_ref().is_some_and(|p| {
                    p.order.is_none()
                        && p.queued.is_empty()
                        && p.economy.job.is_none()
                        && p.economy.employment.is_none()
                        && p.economy.pending_move.is_none()
                        && !p.contained
                        && !p.releasing
                        && !p.stunned
                }) && e.work.as_ref().is_none_or(|w| w.cargo.is_none())
            })
            .map(|e| e.id().0)
            .collect()
    }

    /// Selects the next idle worker and looks at it (F8, the idle worker button).
    pub fn next_idle_worker(&mut self) {
        let idle = self.idle_workers();
        if idle.is_empty() {
            return;
        }
        let id = idle[self.idle_cursor % idle.len()];
        self.idle_cursor += 1;
        self.select(vec![id], None);
        self.look_at(&[id]);
    }

    /// Own army units and heroes that can be selected, in selection order.
    pub fn army(&self) -> Vec<u32> {
        let k = &self.know;
        let mut ids: Vec<u32> = k
            .view
            .entities
            .iter()
            .filter(|e| k.controlled(e) && k.is_unit(e) && k.selectable(e))
            .filter(|e| know::rank(k.def(e)) >= 2)
            .map(|e| e.id().0)
            .collect();
        ids.sort_by_key(|&id| std::cmp::Reverse(k.rank_of(id)));
        ids
    }

    /// Selects the whole army (F1, the army button).
    pub fn select_army(&mut self) {
        let army = self.army();
        if !army.is_empty() {
            self.select(army, None);
        }
    }

    fn dropoffs(&self) -> Vec<u32> {
        let k = &self.know;
        k.view
            .entities
            .iter()
            .filter(|e| {
                k.own(e)
                    && k.alive(e)
                    && e.public.construction.is_none()
                    && k.def(e)
                        .behaviors
                        .storage
                        .as_ref()
                        .is_some_and(|s| s.dropoff == Some(true))
            })
            .map(|e| e.id().0)
            .collect()
    }

    /// Backspace: centres on the next drop-off building (without selecting it).
    fn next_dropoff(&mut self) {
        let all = self.dropoffs();
        if all.is_empty() {
            return;
        }
        let id = all[self.dropoff_cursor % all.len()];
        self.dropoff_cursor += 1;
        self.look_at(&[id]);
    }

    /// The colony's main building (objective fort, else the first drop-off).
    fn home(&self) -> Option<[f32; 2]> {
        let k = &self.know;
        let fort = k
            .owner()
            .and_then(|o| k.view.objectives.get(&o))
            .and_then(|&id| k.entity(id));
        fort.map(|e| k.position(e)).or_else(|| {
            self.dropoffs()
                .first()
                .and_then(|&id| k.entity(id))
                .map(|e| k.position(e))
        })
    }

    // --- Commands -----------------------------------------------------------------

    /// A building command for a group of one type (`actors`, `card::building_group`):
    /// queued work goes to the building with the shortest queue, counting what was sent
    /// since the last view, so repeated presses alternate; pause, cancel and cancelling an
    /// upgrade go to each building they apply to.
    fn spread(&mut self, action: Action, actors: &[u32]) -> Vec<Action> {
        if actors.len() < 2 {
            return vec![action];
        }
        let k = &self.know;
        match &action {
            Action::Produce { .. }
            | Action::Research { .. }
            | Action::Revive { .. }
            | Action::Upgrade { .. } => {
                if self.pending.0 != k.view.tick {
                    self.pending = (k.view.tick, BTreeMap::new());
                }
                let Some(id) = card::next_in_queue(k, actors, &self.pending.1) else {
                    return vec![action];
                };
                // An upgrade holds the building: further presses go elsewhere.
                let weight = if matches!(action, Action::Upgrade { .. }) {
                    u32::MAX / 2
                } else {
                    1
                };
                *self.pending.1.entry(id).or_default() += weight;
                vec![card::with_actor(&action, id)]
            }
            Action::Pause { .. } | Action::Cancel { queue: None, .. } => actors
                .iter()
                .map(|&id| card::with_actor(&action, id))
                .collect(),
            Action::CancelUpgrade { .. } => actors
                .iter()
                .filter(|&&id| {
                    k.entity(id)
                        .and_then(|e| e.private.as_ref())
                        .is_some_and(|p| p.economy.upgrade.is_some())
                })
                .map(|&id| card::with_actor(&action, id))
                .collect(),
            _ => vec![action],
        }
    }

    /// Presses the command `id` of the current card (button or hotkey).
    pub fn activate_id(&mut self, id: &str, queue: bool) {
        let Some(c) = self
            .entries
            .iter()
            .chain(&self.commands)
            .find(|c| c.id == id)
            .cloned()
        else {
            return;
        };
        self.activate(&c, queue);
    }

    pub fn activate(&mut self, c: &Command, queue: bool) {
        if c.passive {
            return;
        }
        if let Activate::Menu(category) = &c.activate {
            self.menu = category.clone();
            self.page = 0;
            self.mode = Mode::Idle;
            self.refresh();
            return;
        }
        if let Some(reason) = &c.reason {
            let reason = reason.clone();
            self.error(reason);
            return;
        }
        let shift = queue || self.mods.shift;
        match &c.activate {
            Activate::Immediate {
                action,
                queue: queues,
            } => {
                let mut action = action.clone();
                if *queues
                    && shift
                    && let Action::Hold { append, .. } = &mut action
                {
                    *append = Some(true);
                }
                let learned = matches!(action, Action::LearnAbility { .. });
                let actions = self.spread(action, &c.actors);
                self.send(actions.into_iter().map(|a| (a, None)).collect());
                if learned {
                    // The old HUD returns to the root card once learning is accepted.
                    self.menu = None;
                }
            }
            Activate::Target(targeting) => {
                self.mode = Mode::Target {
                    targeting: targeting.clone(),
                    source: (c.id.clone(), c.actors.clone()),
                    name: c.name.clone(),
                };
            }
            Activate::Place { definition, actors } => {
                self.mode = Mode::Place {
                    placement: Placement {
                        definition: definition.clone(),
                        actors: actors.clone(),
                        rotation: 0,
                    },
                    source: (c.id.clone(), c.actors.clone()),
                    name: c.name.clone(),
                };
            }
            Activate::Camera => {
                self.mode = Mode::Idle;
                self.camera_mode = self.camera_mode.next();
            }
            Activate::Menu(_) => {}
        }
        self.refresh();
    }

    /// Right-click on a command: its alternate (autocast toggle).
    pub fn alternate(&mut self, id: &str) {
        if let Some(action) = self
            .entries
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.alternate.clone())
        {
            self.send(vec![(action, None)]);
        }
    }

    /// Cycles the command card's pages.
    pub fn turn_page(&mut self, forward: bool) {
        let n = self.layout.pages.max(1);
        self.page = if forward {
            (self.page + 1) % n
        } else {
            (self.page + n - 1) % n
        };
        self.refresh();
    }

    /// Uses (or, with `drop`, drops) the focused hero's item in `slot`.
    pub fn use_item(&mut self, slot: usize) {
        self.item(slot, false);
    }

    pub fn item(&mut self, slot: usize, drop: bool) {
        let k = &self.know;
        let Some(focus) = self.selection.focus().and_then(|id| k.entity(id)) else {
            return;
        };
        if !k.controlled(focus) {
            return;
        }
        let Some(inventory) = focus.magic.as_ref().and_then(|m| m.inventory.as_ref()) else {
            return;
        };
        if inventory.slots.get(slot).copied().flatten().is_none() {
            return;
        }
        let actor = focus.id().0;
        let action = if drop {
            Action::DropItem {
                actor,
                slot: slot as u32,
            }
        } else {
            Action::UseItem {
                actor,
                slot: slot as u32,
            }
        };
        self.send(vec![(action, None)]);
    }

    /// Cancels a production, research or revival entry of the focused building.
    pub fn cancel(&mut self, action: Action) {
        self.send(vec![(action, None)]);
    }

    /// A selection card click: focus, isolate (double click) or remove (Shift).
    pub fn card_click(&mut self, id: u32, shift: bool, double: bool) {
        if shift {
            self.selection.remove(id);
            self.mode = Mode::Idle;
            self.refresh();
        } else if double {
            self.select(vec![id], None);
        } else {
            self.selection.focus_on(id);
            self.mode = Mode::Idle;
            self.menu = None;
            self.refresh();
        }
    }

    /// A control group button: recall, Ctrl assigns, Shift adds.
    pub fn group_click(&mut self, slot: usize, mods: Mods) {
        if mods.command() {
            let ids = self.groupable();
            self.groups.assign(slot, &ids);
        } else if mods.shift {
            let ids = self.groupable();
            self.groups.add(slot, &ids);
        } else if let Some(r) = self.groups.recall(slot, self.now) {
            match r {
                Recall::Select(ids) => self.select(ids, None),
                Recall::Focus(ids) => {
                    self.select(ids.clone(), None);
                    self.look_at(&ids);
                }
            }
        }
    }

    /// The team's recent pings for the minimap, in their players' colours (`colors`,
    /// linear, by slot).
    pub fn minimap_pings(&self, colors: &[[f32; 3]]) -> Vec<canopy_scene::world_ui::MinimapPing> {
        let view = &self.know.view;
        view.pings
            .iter()
            .map(|p| canopy_scene::world_ui::MinimapPing {
                x: p.x as f32 + 0.5,
                z: p.y as f32 + 0.5,
                age: view.tick.saturating_sub(p.tick) as f32 * 0.025,
                color: match p.owner {
                    canopy_content::placement::Owner::Player(slot) => colors
                        .get(usize::from(slot) % colors.len().max(1))
                        .copied()
                        .unwrap_or([1.0; 3]),
                    canopy_content::placement::Owner::Nobody => [1.0; 3],
                },
            })
            .collect()
    }

    /// Whether Alt is held: a minimap click pings instead of looking.
    pub fn alt_held(&self) -> bool {
        self.mods.alt
    }

    /// Pings ground point (x, z) for the player's team (Alt-click on the minimap).
    pub fn ping(&mut self, x: f32, z: f32) {
        if self.know.me.is_none() {
            return;
        }
        let size = self.terrain.size() as f32;
        let position = CellPoint {
            x: x.floor().clamp(0.0, size - 1.0) as u32,
            y: z.floor().clamp(0.0, size - 1.0) as u32,
            surface: None,
        };
        self.send(vec![(
            Action::Ping {
                position: Some(position),
            },
            None,
        )]);
    }

    /// The minimap: right-click moves the selection (or completes a mode) at a ground
    /// point (x, z).
    pub fn minimap_command(&mut self, x: f32, z: f32) {
        let size = self.terrain.size() as f32;
        let cell = CellPoint {
            x: x.floor().clamp(0.0, size - 1.0) as u32,
            y: z.floor().clamp(0.0, size - 1.0) as u32,
            surface: None,
        };
        match self.mode.clone() {
            Mode::Idle => {
                let k = self.know.clone();
                let units = orders::commanded(&k, self.selection.ids());
                if units.is_empty() {
                    let commands = orders::producers(&k, self.selection.ids())
                        .into_iter()
                        .filter_map(|p| orders::rally(&k, p, None, Some(&cell)))
                        .collect();
                    self.send(commands);
                } else {
                    let commands =
                        orders::smart(&k, self.selection.ids(), None, Some(&cell), self.mods.shift);
                    self.send(commands);
                }
            }
            Mode::Target { targeting, .. } => match targeting {
                Targeting::Move { .. }
                | Targeting::Attack { .. }
                | Targeting::Patrol { .. }
                | Targeting::Rally { .. } => {
                    let (commands, _) = orders::complete(
                        &self.know,
                        &targeting,
                        None,
                        Some(&cell),
                        self.mods.shift,
                    );
                    self.send(commands);
                    self.mode = Mode::Idle;
                }
                _ => {}
            },
            Mode::Place { .. } => {}
        }
    }

    /// The player's chat line: sent on ([`Event::Chat`]), and shown at once when no
    /// chat server echoes it ([`Self::echo_chat`]).
    pub fn say(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).take(300).collect();
        let text = text.trim().to_owned();
        self.chat_open = false;
        if text.is_empty() {
            return;
        }
        if self.echo_chat {
            let name = self.name.clone().unwrap_or_else(|| {
                self.know.me.map_or_else(
                    || tr!("play.observer"),
                    |p| tr!("hud.player", number = p + 1),
                )
            });
            self.hear(name, [0xcc, 0xcc, 0xcc], &text);
        }
        self.events.push(Event::Chat(text));
    }

    /// A chat line from the match server (another seat's, or our own echoed).
    pub fn hear(&mut self, name: String, color: [u8; 3], text: &str) {
        self.chat.push(ChatLine {
            name,
            color,
            text: text.to_owned(),
            at: self.now.as_secs_f32(),
        });
        if self.chat.len() > 200 {
            self.chat.remove(0);
        }
    }

    pub fn now(&self) -> Duration {
        self.now
    }
}

#[cfg(test)]
mod tests;
