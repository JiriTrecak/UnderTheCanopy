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
pub mod debug;
pub mod hud;
pub mod input;
pub mod know;
pub mod orders;
pub mod overlay;
pub mod picking;
pub mod placement;
pub mod run;
pub mod selection;

use canopy_content::game::Kind;
use canopy_render::camera::Ground;
use canopy_sim::{Action, action::CellPoint, terrain::Terrain};
use card::{Activate, Command};
use glam::{Vec2, Vec3};
use input::{Button, Input, Mods};
use know::Knowledge;
use orders::{Marker, Target, Targeting};
use picking::{Body, Screen};
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
    /// The player sent a chat line.
    Chat(String),
}

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
    pointer: Option<Vec2>,
    press: Option<Press>,
    last_click: Option<(Vec2, Duration)>,
    mods: Mods,
    held: Vec<&'static str>,
    hovered: Option<u32>,
    ground_point: Option<Vec3>,
    markers: Vec<(Marker, Duration)>,
    message: Option<(String, Duration)>,
    seen_fact: Option<u32>,
    hero_press: Option<(usize, Duration)>,
    idle_cursor: usize,
    dropoff_cursor: usize,
    alerts: Vec<Alert>,
    alert_cursor: usize,
    known_hp: BTreeMap<u32, (u32, bool)>,
    events: Vec<Event>,
    now: Duration,
    /// Every command, the current menu and its layout, from the newest view.
    pub commands: Vec<Command>,
    pub entries: Vec<Command>,
    pub layout: card::Layout,
    pub chat: Vec<ChatLine>,
    pub chat_open: bool,
    /// Commands sent, newest last (debug overlay).
    pub sent: Vec<(Duration, Action)>,
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
            pointer: None,
            press: None,
            last_click: None,
            mods: Mods::default(),
            held: Vec::new(),
            hovered: None,
            ground_point: None,
            markers: Vec::new(),
            message: None,
            seen_fact: None,
            hero_press: None,
            idle_cursor: 0,
            dropoff_cursor: 0,
            alerts: Vec::new(),
            alert_cursor: 0,
            known_hp: BTreeMap::new(),
            events: Vec::new(),
            now: Duration::ZERO,
            commands: Vec::new(),
            entries: Vec::new(),
            layout: card::Layout::default(),
            chat: Vec::new(),
            chat_open: false,
            sent: Vec::new(),
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

    /// Events since the last call.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
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
        self.entries = card::menu(&self.know, &self.commands, self.menu.as_deref());
        if self.menu.is_some() && !self.entries.iter().any(|c| c.id == "back") {
            self.menu = None;
        }
        self.layout = card::layout(&self.entries, self.page);
        self.page = self.layout.page;
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
        let unseen = facts.iter().rfind(|f| {
            self.seen_fact.is_none_or(|s| f.id > s) && f.kind == canopy_sim::state::FactKind::Error
        });
        if let Some(f) = unseen
            && self.know.view.tick.saturating_sub(f.tick) < ERROR_FACT_TICKS
        {
            self.message = Some((f.message.clone(), self.now));
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
                }
                None if !first && k.is_unit(e) => fresh.push(k.position(e)),
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

    /// Re-picks what is under the cursor.
    fn hover(&mut self, ground: &dyn Ground) {
        let (Some(screen), Some(px)) = (self.screen, self.pointer) else {
            self.hovered = None;
            self.ground_point = None;
            return;
        };
        self.ground_point = screen.ground(px, ground);
        self.hovered = match self.mode {
            Mode::Place { .. } => None,
            _ => self.pick(px, ground).map(|t| t.id),
        };
    }

    /// The entity under a pixel: a unit's capsule, else the building, item or resource
    /// whose footprint holds the ground point (old fallback rule).
    fn pick(&self, px: Vec2, ground: &dyn Ground) -> Option<Target> {
        let screen = self.screen?;
        let k = &self.know;
        if let Some(id) = screen.pick_unit(&self.bodies, px)
            && k.entity(id).is_some_and(|e| k.selectable(e))
        {
            return Target::of(k, id);
        }
        let hit = screen.ground(px, ground)?;
        let reach = |d: &canopy_content::game::Definition| {
            d.footprint
                .as_ref()
                .map_or(0.8, |f| f.width.max(f.depth) as f32 / 2.0)
        };
        let inside = |p: [f32; 2], r: f32| (p[0] - hit.x).abs() <= r && (p[1] - hit.z).abs() <= r;
        for e in k.view.entities.iter() {
            let d = k.def(e);
            if d.kind == Kind::Unit || !k.selectable(e) {
                continue;
            }
            if inside(k.position(e), reach(d)) {
                return Target::of(k, e.id().0);
            }
        }
        // Trees and other known scenery resources.
        k.view
            .scenery
            .iter()
            .find(|s| {
                s.resource.as_ref().is_some_and(|r| r.amount > 0)
                    && inside(
                        [s.position.x as f32 / 1000.0, s.position.y as f32 / 1000.0],
                        reach(k.catalog.get(s.definition)),
                    )
            })
            .and_then(|s| Target::of(k, s.id.0))
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
        for (action, marker) in commands {
            if action.problem().is_some() {
                continue;
            }
            if self.sink.send(action.clone()) {
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

    /// The targeting hint above the HUD (old hint text).
    pub fn hint(&self) -> Option<String> {
        match &self.mode {
            Mode::Idle => None,
            Mode::Target {
                targeting, name, ..
            } => Some(format!(
                "{name}: choose a {}. Esc cancels.",
                targeting.wants()
            )),
            Mode::Place {
                placement, name, ..
            } => {
                let problem = self.ghost().and_then(|g| g.problem);
                Some(format!(
                    "{name}: {} R / Shift+R: rotate ({}°). Esc cancels.",
                    problem.map_or("Click to order construction.".to_owned(), |p| format!(
                        "{p}."
                    )),
                    placement.rotation
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
        let health = if self.held.contains(&"Alt") {
            HealthBars::All
        } else if self.held.contains(&"BracketLeft") {
            HealthBars::Friendly
        } else if self.held.contains(&"BracketRight") {
            HealthBars::Hostile
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
            selected: self.selection.ids().to_vec(),
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
        }
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
            hero: d.is_hero(),
            army: d.selection_class == Some(canopy_content::game::SelectionClass::Army),
            worker: d.behaviors.work.is_some(),
            kind: e.definition().0,
        })
    }

    /// Replaces the selection (clears any mode, as the old `setSelection`).
    pub fn select(&mut self, ids: Vec<u32>, focus: Option<u32>) {
        let k = self.know.clone();
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
        let own_unit = k
            .entity(t.id)
            .is_some_and(|e| k.controlled(e) && k.is_unit(e));
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
        if own_unit && (self.mods.command() || double) {
            // Every own unit of the type on screen.
            let screen = self.screen;
            let mut ids: Vec<u32> = k
                .view
                .entities
                .iter()
                .filter(|e| e.definition() == t.definition && k.controlled(e) && k.is_unit(e))
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
        if own_unit && shift {
            let mut ids: Vec<u32> = self
                .selection
                .ids()
                .iter()
                .copied()
                .filter(|&id| {
                    k.entity(id)
                        .is_some_and(|e| k.controlled(e) && k.is_unit(e))
                })
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
            .filter(|e| !e.remembered && k.is_unit(e) && k.selectable(e))
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
            Input::Button { button, pressed } => self.button(*button, *pressed, ground),
            Input::Key {
                key,
                pressed,
                repeat,
            } => {
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
        let shift = self.mods.shift;
        if key == "Enter" {
            self.chat_open = true;
            return;
        }
        // Control groups.
        if let Some(digit) = key.parse::<u8>().ok().filter(|_| key.len() == 1)
            && let Some(slot) = selection::slot_of_digit(digit)
        {
            if self.mods.command() {
                let ids = self.groupable();
                self.groups.assign(slot, &ids);
            } else if shift {
                let ids = self.groupable();
                self.groups.add(slot, &ids);
            } else if let Some(recall) = self.groups.recall(slot, self.now) {
                match recall {
                    Recall::Select(ids) => self.select(ids, None),
                    Recall::Focus(ids) => {
                        self.select(ids.clone(), None);
                        self.look_at(&ids);
                    }
                }
            }
            return;
        }
        match key {
            "Space" => {
                if !self.alerts.is_empty() {
                    let a = self.alerts[self.alert_cursor % self.alerts.len()];
                    self.alert_cursor += 1;
                    self.events.push(Event::LookAt(a.position));
                }
                return;
            }
            "F1" | "F2" | "F3" => {
                let index = usize::from(key.as_bytes()[1] - b'1');
                self.hero_key(index);
                return;
            }
            "F8" | "Backquote" => {
                self.next_idle_worker();
                return;
            }
            "Backspace" => {
                self.next_dropoff();
                return;
            }
            "Tab" => {
                let cands: Vec<Candidate> = self
                    .selection
                    .ids()
                    .iter()
                    .filter_map(|&id| self.candidate(id))
                    .collect();
                self.selection.cycle(&cands, !shift);
                self.menu = None;
                self.page = 0;
                self.mode = Mode::Idle;
                self.refresh();
                return;
            }
            "Numpad7" | "Numpad8" | "Numpad4" | "Numpad5" => {
                let slot = ["Numpad7", "Numpad8", "Numpad4", "Numpad5"]
                    .iter()
                    .position(|k| *k == key)
                    .unwrap_or(0);
                self.use_item(slot);
                return;
            }
            "F10" => {
                self.events.push(Event::OpenMenu);
                return;
            }
            "Escape" => {
                if self.mode != Mode::Idle {
                    self.mode = Mode::Idle;
                } else if self.menu.is_some() {
                    self.activate_id("back", false);
                } else {
                    self.events.push(Event::OpenMenu);
                }
                return;
            }
            "R" if matches!(self.mode, Mode::Place { .. }) => {
                if let Mode::Place { placement, .. } = &mut self.mode {
                    placement.rotate(shift);
                }
                return;
            }
            "Home" => {
                let ids = self.selection.ids().to_vec();
                if ids.is_empty() {
                    if let Some(hall) = self.home() {
                        self.events.push(Event::LookAt(hall));
                    }
                } else {
                    self.look_at(&ids);
                }
                return;
            }
            _ => {}
        }
        let visible: Vec<Command> = self
            .layout
            .slots
            .iter()
            .flatten()
            .chain(&self.layout.banners)
            .map(|&i| self.entries[i].clone())
            .collect();
        let all = self.commands.clone();
        if let Some((c, queue)) = card::shortcut(&visible, &all, key, shift) {
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

    /// F1–F3 and hero buttons: select; a second press within 350 ms centres.
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

    /// Own workers with nothing to do (old idle rule).
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
                }) && e.work.as_ref().is_none_or(|w| w.cargo.is_none())
            })
            .map(|e| e.id().0)
            .collect()
    }

    fn next_idle_worker(&mut self) {
        let idle = self.idle_workers();
        if idle.is_empty() {
            return;
        }
        let id = idle[self.idle_cursor % idle.len()];
        self.idle_cursor += 1;
        self.select(vec![id], None);
        self.look_at(&[id]);
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
                self.send(vec![(action, None)]);
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

    /// The player's chat line, echoed locally (single-player has no chat server).
    pub fn say(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).take(300).collect();
        let text = text.trim().to_owned();
        self.chat_open = false;
        if text.is_empty() {
            return;
        }
        let name = self
            .know
            .me
            .map_or("Observer".to_owned(), |p| format!("Player {}", p + 1));
        self.chat.push(ChatLine {
            name,
            color: [0xcc, 0xcc, 0xcc],
            text: text.clone(),
            at: self.now.as_secs_f32(),
        });
        if self.chat.len() > 200 {
            self.chat.remove(0);
        }
        self.events.push(Event::Chat(text));
    }

    pub fn now(&self) -> Duration {
        self.now
    }
}

#[cfg(test)]
mod tests;
