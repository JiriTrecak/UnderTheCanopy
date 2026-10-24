//! Binds the match to the HUD layout (`config/ui/layouts/hud.json`, templates in
//! `common.json` and `play.json`): every frame the presentation of [`Play`] and its
//! knowledge is pushed into named nodes, and the UI's interactions come back as calls on
//! [`Play`] (old `settlementHud.ts`, `heroBar.ts`, `tooltips.ts`, `chat.ts`,
//! `observerPanel.ts`).
//!
//! Images are named by their pack entry (`assets/library/<id>/image.png`); the HUD lists
//! the names it used so the game can upload them ([`Hud::take_image_requests`]). Render
//! targets plug in by name: `@minimap` and `@portrait` (registered by the systems that
//! draw them; see [`Hud::live_portrait`] and [`Hud::minimap`]).

use super::{
    Event, Play,
    card::{self, Command, MANA},
    keys::{Keymap, Shortcut},
    know::Knowledge,
};
use crate::settings::Chord;
use canopy_content::{game::Kind, placement::Owner};
use canopy_sim::{Action, economy::lane::Item, view::EntityView};
use canopy_ui::{ActionKind, Color, Cost, NodeId, PointerButton, Tooltip, Ui};
use std::collections::{BTreeMap, BTreeSet};

/// Seconds a chat line shows while the chat is closed.
pub const CHAT_SECONDS: f32 = 10.0;
/// Game seconds of deliveries the observer income covers.
pub const INCOME_TICKS: u32 = 2400;

fn find(ui: &Ui, path: &str) -> Option<NodeId> {
    let id = ui.find(path);
    if id.is_none() {
        tracing::warn!(path, "hud node missing");
    }
    id
}

/// The health ramp: red through amber (half) to green (old `health.ts`).
pub fn health_color(ratio: f32) -> Color {
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
        [0, 1, 2].map(|i| (a[i] + (b[i] - a[i]) * t).round() as u8)
    };
    let (red, amber, green) = (
        [219.0, 57.0, 53.0],
        [232.0, 191.0, 50.0],
        [100.0, 204.0, 57.0],
    );
    let r = ratio.clamp(0.0, 1.0);
    let [r8, g8, b8] = if r < 0.5 {
        lerp(red, amber, r * 2.0)
    } else {
        lerp(amber, green, (r - 0.5) * 2.0)
    };
    Color::rgba(r8, g8, b8, 255)
}

struct Nodes {
    root: NodeId,
    dock: NodeId,
    heroes: NodeId,
    hint: NodeId,
    outcome: NodeId,
    error: NodeId,
    groups: NodeId,
    minimap: NodeId,
    portrait: NodeId,
    experience: NodeId,
    level: NodeId,
    copy: NodeId,
    name: NodeId,
    stats: NodeId,
    task: NodeId,
    task_image: NodeId,
    task_meter: NodeId,
    task_label: NodeId,
    status: NodeId,
    statuses: NodeId,
    health: NodeId,
    health_value: NodeId,
    mana: NodeId,
    mana_value: NodeId,
    cards: NodeId,
    inventory: NodeId,
    queue: NodeId,
    orders: NodeId,
    commands: NodeId,
    banners: NodeId,
    pager: NodeId,
    pager_label: NodeId,
    resources: NodeId,
    chat_log: NodeId,
    chat_input: NodeId,
    idle: NodeId,
    idle_icon: NodeId,
    idle_count: NodeId,
    army: NodeId,
    army_icon: NodeId,
    army_count: NodeId,
    observer: NodeId,
    observer_columns: NodeId,
    observer_rows: NodeId,
    observer_footer: NodeId,
}

impl Nodes {
    fn find(ui: &Ui) -> Option<Self> {
        Some(Self {
            root: find(ui, "hud")?,
            dock: find(ui, "hud/dock")?,
            heroes: find(ui, "hud/heroes")?,
            hint: find(ui, "hud/hint")?,
            outcome: find(ui, "hud/outcome")?,
            error: find(ui, "hud/error")?,
            groups: find(ui, "hud/dock/groups")?,
            minimap: find(ui, "hud/dock/minimap")?,
            portrait: find(ui, "hud/dock/selection/portrait")?,
            experience: find(ui, "hud/dock/selection/experience")?,
            level: find(ui, "hud/dock/selection/level")?,
            copy: find(ui, "hud/dock/selection/copy")?,
            name: find(ui, "hud/dock/selection/copy/name")?,
            stats: find(ui, "hud/dock/selection/copy/stats")?,
            task: find(ui, "hud/dock/selection/copy/task")?,
            task_image: find(ui, "hud/dock/selection/copy/task/icon/image")?,
            task_meter: find(ui, "hud/dock/selection/copy/task/meter")?,
            task_label: find(ui, "hud/dock/selection/copy/task/meter/label")?,
            status: find(ui, "hud/dock/selection/copy/status")?,
            statuses: find(ui, "hud/dock/selection/copy/statuses")?,
            health: find(ui, "hud/dock/selection/copy/health")?,
            health_value: find(ui, "hud/dock/selection/copy/health/value")?,
            mana: find(ui, "hud/dock/selection/copy/mana")?,
            mana_value: find(ui, "hud/dock/selection/copy/mana/value")?,
            cards: find(ui, "hud/dock/selection/cards")?,
            inventory: find(ui, "hud/dock/selection/inventory")?,
            queue: find(ui, "hud/dock/selection/queue")?,
            orders: find(ui, "hud/dock/selection/orders")?,
            commands: find(ui, "hud/dock/actions/commands")?,
            banners: find(ui, "hud/dock/actions/banners")?,
            pager: find(ui, "hud/dock/actions/pager")?,
            pager_label: find(ui, "hud/dock/actions/pager/label")?,
            resources: find(ui, "hud/dock/actions/resources")?,
            chat_log: find(ui, "hud/chat/log")?,
            chat_input: find(ui, "hud/chat/input")?,
            idle: find(ui, "hud/dock/picks/idle")?,
            idle_icon: find(ui, "hud/dock/picks/idle/icon")?,
            idle_count: find(ui, "hud/dock/picks/idle/count")?,
            army: find(ui, "hud/dock/picks/army")?,
            army_icon: find(ui, "hud/dock/picks/army/icon")?,
            army_count: find(ui, "hud/dock/picks/army/count")?,
            observer: find(ui, "hud/observer")?,
            observer_columns: find(ui, "hud/observer/columns")?,
            observer_rows: find(ui, "hud/observer/rows")?,
            observer_footer: find(ui, "hud/observer/footer")?,
        })
    }
}

/// A queue entry the player can cancel by clicking its tile.
#[derive(Debug, Clone)]
struct QueueEntry {
    icon: String,
    cancel: Option<Action>,
    progress: Option<f32>,
    name: String,
}

/// Player colours for chat names and the observer panel (sRGB), by slot.
pub type Palette = Vec<[u8; 3]>;

pub struct Hud {
    nodes: Option<Nodes>,
    /// Hero cards with a skill point to spend, as laid out last frame (physical pixels):
    /// motes rise behind them.
    rising: Vec<canopy_ui::Rect>,
    appearance: [Option<String>; 2],
    requests: BTreeSet<String>,
    /// The portrait shows the live render target `@portrait` instead of the icon.
    pub live_portrait: bool,
    /// The minimap shows the render target `@minimap`.
    pub minimap: bool,
    pub palette: Palette,
    /// The match's players: slot, name and whether a computer plays it.
    pub players: Vec<(u8, String, bool)>,
    /// Map size, for minimap clicks.
    /// The minimap's square on the map: x, z of its corner, its side (cells).
    pub minimap_area: [f32; 3],
    /// A left-button drag on the minimap is moving the camera (a right-click's slight
    /// movement must not).
    minimap_look: bool,
    /// What the current card's cells and banners hold (command ids).
    cells: [Option<String>; card::SLOTS],
    banners: Vec<String>,
    queue: Vec<QueueEntry>,
    active: Option<QueueEntry>,
    chat_was_open: bool,
    /// Observer income: deliveries per tick seen, by owner and item.
    deliveries: BTreeMap<u32, Vec<(Owner, String, u32)>>,
    first_tick: Option<u32>,
    /// Colonies for the observer panel (from the simulation thread).
    pub colonies: Option<std::sync::Arc<Vec<crate::sim::ColonyStats>>>,
    /// A replay shows its own statistics instead of the observer panel (D60).
    pub hide_observer: bool,
}

impl Default for Hud {
    fn default() -> Self {
        Self::new()
    }
}

/// Seconds of one breath of a hero card's skill-point glow.
const MOTE_PULSE: f32 = 1.6;
/// Motes rising behind one card at a time, and how long each lives (s).
const MOTES: u32 = 22;
const MOTE_LIFE: f32 = 1.9;

impl Hud {
    /// A breathing gold halo and motes rising from behind each hero card with a skill
    /// point to spend, drawn under the HUD so the card stands in front of them (`now`:
    /// the match's clock).
    pub fn draw_motes(&self, list: &mut canopy_ui::DrawList, now: std::time::Duration, scale: f32) {
        use canopy_ui::{Batch, Color, Quad, QuadKind, Rect};
        if self.rising.is_empty() {
            return;
        }
        let t = now.as_secs_f32();
        let mut quads = Vec::new();
        let breath = 0.6 + 0.4 * (t * std::f32::consts::TAU / MOTE_PULSE).sin();
        for (c, card) in self.rising.iter().enumerate() {
            // A shadow quad covers the blur's reach around the card it is cast from.
            let (spread, blur) = (3.0 * scale, 16.0 * scale);
            let reach = spread + blur + 1.0;
            let mut halo = Quad::new(
                QuadKind::Shadow,
                Rect::new(
                    card.x - reach,
                    card.y - reach,
                    card.w + 2.0 * reach,
                    card.h + 2.0 * reach,
                ),
                Color::rgba(0xf6, 0xb8, 0x4a, (235.0 * breath) as u8),
            );
            halo.radius = 4.0 * scale;
            halo.border = spread;
            halo.soft = blur;
            halo.uv = [0.0; 4];
            quads.push(halo);
            for m in 0..MOTES {
                // A fixed scatter per mote; each rises through its life and starts again.
                let hash = |k: u32| {
                    let x = (c as u32 * 7919 + m * 104_729 + k * 15_485_863)
                        .wrapping_mul(2_654_435_761);
                    (x >> 8) as f32 / (1u32 << 24) as f32
                };
                let age = (t / MOTE_LIFE + hash(1)).fract();
                let spread = card.w + 14.0 * scale;
                let x = card.x - 7.0 * scale
                    + spread * hash(2)
                    + (age * 6.0 + hash(3) * 6.3).sin() * 3.0 * scale;
                let y = card.bottom() - age * (card.h + 44.0 * scale);
                let size = (3.5 + 5.0 * hash(4)) * scale * (1.0 - 0.45 * age);
                let alpha = ((age * std::f32::consts::PI).sin().powf(0.6) * 255.0) as u8;
                let mut q = Quad::new(
                    QuadKind::Radial,
                    Rect::new(x - size, y - size, size * 2.0, size * 2.0),
                    Color::rgba(0xff, 0xe6, 0xa6, alpha),
                );
                q.color2 = Color::rgba(0xf2, 0xa8, 0x38, 0).to_array();
                q.radius = size;
                quads.push(q);
            }
        }
        // Under the HUD: shift its batches past the motes.
        let n = quads.len() as u32;
        for batch in &mut list.batches {
            batch.quads = batch.quads.start + n..batch.quads.end + n;
        }
        list.batches.insert(
            0,
            Batch {
                texture: None,
                scissor: None,
                quads: 0..n,
            },
        );
        quads.append(&mut list.quads);
        list.quads = quads;
    }

    pub fn new() -> Self {
        Self {
            nodes: None,
            rising: Vec::new(),
            appearance: [None, None],
            requests: BTreeSet::new(),
            live_portrait: false,
            minimap: false,
            palette: Vec::new(),
            players: Vec::new(),
            minimap_area: [0.0, 0.0, 1.0],
            minimap_look: false,
            cells: Default::default(),
            banners: Vec::new(),
            queue: Vec::new(),
            active: None,
            chat_was_open: false,
            deliveries: BTreeMap::new(),
            first_tick: None,
            colonies: None,
            hide_observer: false,
        }
    }

    /// Image names used since the last call that the game has to register.
    pub fn take_image_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.requests).into_iter().collect()
    }

    /// Uses the local seat's starting race, independent of selection or fog perspective.
    /// Observers use the default race; missing art falls back to the UI theme.
    pub fn configure(
        &mut self,
        content: &canopy_content::Content,
        setup: &canopy_sim::MatchSetup,
        me: Option<u8>,
    ) {
        let race = me
            .and_then(|me| setup.slots.iter().find(|slot| slot.player == me))
            .and_then(|slot| slot.race.as_deref())
            .unwrap_or(&content.rules.default_race);
        let hud = content
            .rules
            .races
            .get(race)
            .and_then(|race| race.hud.as_ref());
        self.appearance = [
            hud.and_then(|h| h.frame.as_ref()),
            hud.and_then(|h| h.resources.as_ref()),
        ]
        .map(|asset| {
            asset
                .and_then(|id| content.image_path(id.as_str()))
                .map(str::to_owned)
        });
    }

    fn image(&mut self, ui: &mut Ui, k: &Knowledge, icon: &str) -> Option<canopy_ui::ImageKey> {
        let path = k.icon_image(icon)?.to_owned();
        self.requests.insert(path.clone());
        Some(ui.image_key(&path))
    }

    /// Shows the HUD and finds its nodes (again after a hot reload).
    pub fn attach(&mut self, ui: &mut Ui) {
        ui.show("hud", true);
        self.nodes = Nodes::find(ui);
    }

    /// Pushes the frame's state into the HUD.
    pub fn update(&mut self, ui: &mut Ui, play: &Play) {
        if self
            .nodes
            .as_ref()
            .is_none_or(|n| ui.find("hud") != Some(n.root))
        {
            self.attach(ui);
        }
        let Some(n) = self.nodes.take() else {
            return;
        };
        let k = play.knowledge().clone();
        for (node, path) in [n.dock, n.resources].into_iter().zip(&self.appearance) {
            let image = path.as_ref().map(|path| {
                self.requests.insert(path.clone());
                ui.image_key(path)
            });
            ui.set_background_image(node, image);
        }
        self.messages(ui, &n, play, &k);
        self.resources(ui, &n, &k);
        self.heroes(ui, &n, play, &k);
        self.selection(ui, &n, play, &k);
        self.card(ui, &n, play, &k);
        self.groups(ui, &n, play);
        self.army(ui, &n, play, &k);
        self.idle(ui, &n, play, &k);
        self.chat(ui, &n, play);
        self.observer(ui, &n, play, &k);
        if self.minimap {
            let key = ui.image_key("@minimap");
            ui.set_image(n.minimap, Some(key));
        }
        self.nodes = Some(n);
    }

    /// The army button left of the idle worker button: the leading unit's icon and the
    /// army's size; players always see it, dimmed while they have no army.
    fn army(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
        ui.set_visible(n.army, !play.observer());
        let army = play.army();
        ui.set_enabled(n.army, !army.is_empty());
        let icon = match army.first().and_then(|&id| k.entity(id)) {
            Some(first) => k.def(first).icon.as_str(),
            None => k
                .content
                .actions
                .actions
                .get("attack")
                .map_or("", |a| a.icon.as_str()),
        };
        let key = self.image(ui, k, icon);
        ui.set_image(n.army_icon, key);
        let count = if army.is_empty() {
            String::new()
        } else {
            army.len().to_string()
        };
        ui.set_text(n.army_count, &count);
    }

    /// The idle worker button above the minimap: a worker's icon and how many idle.
    fn idle(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
        let idle = play.idle_workers();
        ui.set_visible(n.idle, !idle.is_empty());
        let Some(first) = idle.first().and_then(|&id| k.entity(id)) else {
            return;
        };
        let key = self.image(ui, k, k.def(first).icon.as_str());
        ui.set_image(n.idle_icon, key);
        ui.set_text(n.idle_count, &idle.len().to_string());
    }

    fn messages(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
        match play.hint() {
            Some(h) => {
                ui.set_text(n.hint, &h);
                ui.set_visible(n.hint, true);
            }
            None => ui.set_visible(n.hint, false),
        }
        match play.message() {
            Some((text, opacity)) if opacity > 0.0 => {
                let alpha = (opacity * 255.0) as u8;
                ui.set_text(
                    n.error,
                    &format!("[color=#ff5252{alpha:02x}]{}[/color]", escape(text)),
                );
                ui.set_visible(n.error, true);
            }
            _ => ui.set_visible(n.error, false),
        }
        // Old HUD outcome lines (the result dialog shows the details).
        let mission = k.view.mission.is_some();
        let outcome = k
            .view
            .outcome
            .as_ref()
            .map(|o| match (k.owner(), o.winner) {
                (Some(me), w) if mission => {
                    if w == Some(me) {
                        tr!("hud.mission-complete")
                    } else {
                        tr!("hud.mission-failed")
                    }
                }
                (_, None) => tr!("hud.outcome-draw"),
                (None, Some(Owner::Player(p))) => tr!("hud.outcome-player-wins", player = p + 1),
                (None, Some(_)) => tr!("hud.outcome-over"),
                (Some(me), Some(w)) if w == me => tr!("hud.outcome-victory"),
                _ => tr!("hud.outcome-defeat"),
            });
        match outcome {
            Some(text) => {
                ui.set_text(n.outcome, &text);
                ui.set_visible(n.outcome, true);
            }
            None => ui.set_visible(n.outcome, false),
        }
    }

    fn resources(&mut self, ui: &mut Ui, n: &Nodes, k: &Knowledge) {
        ui.set_visible(n.resources, k.view.colony.is_some());
        let Some(colony) = k.view.colony.as_ref() else {
            ui.set_count(n.resources, 0);
            return;
        };
        let count = colony.goods.len() + 1;
        ui.set_count(n.resources, count);
        for (i, g) in colony.goods.iter().enumerate() {
            let Some(cell) = ui.child(n.resources, i) else {
                continue;
            };
            let d = k.content.definition(g.item.as_str());
            if let (Some(icon), Some(d)) = (ui.find_in(cell, "icon"), d) {
                let key = self.image(ui, k, d.icon.as_str());
                ui.set_image(icon, key);
            }
            if let Some(value) = ui.find_in(cell, "value") {
                ui.set_text(value, &g.available.to_string());
            }
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: d.map_or(g.item.to_string(), |d| d.name.clone()),
                    body: tr!(
                        "hud.resource-tip",
                        description = d.map_or("", |d| d.description.as_str()),
                        available = g.available,
                        reserved = g.reserved,
                        stored = g.stored,
                        carried = g.in_transit
                    ),
                    ..Tooltip::default()
                }),
            );
        }
        let s = &colony.supply;
        if let Some(cell) = ui.child(n.resources, count - 1) {
            if let Some(icon) = ui.find_in(cell, "icon") {
                let key = self.image(ui, k, k.content.rules.supply_icon.as_str());
                ui.set_image(icon, key);
            }
            if let Some(value) = ui.find_in(cell, "value") {
                let text = format!("{}/{}", s.committed, s.capacity);
                if s.capacity == 0 {
                    // No base at all (a mission's party on the road): nothing to be over.
                    ui.set_text(value, &s.committed.to_string());
                } else if s.over_capacity > 0 {
                    ui.set_text(value, &format!("[color=#ed8d77]{text}[/color]"));
                } else {
                    ui.set_text(value, &text);
                }
            }
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: tr!("hud.supply"),
                    body: tr!(
                        "hud.supply-tip",
                        used = s.used,
                        reserved = s.reserved,
                        available = s.available,
                        limit = s.limit
                    ),
                    ..Tooltip::default()
                }),
            );
        }
    }

    fn heroes(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
        self.rising.clear();
        let heroes = play.heroes();
        ui.set_count(n.heroes, heroes.len());
        for (i, &id) in heroes.iter().enumerate() {
            let (Some(cell), Some(e)) = (ui.child(n.heroes, i), k.entity(id)) else {
                continue;
            };
            let d = k.def(e);
            if let Some(p) = ui.find_in(cell, "portrait") {
                let key = self.image(ui, k, d.icon.as_str());
                ui.set_image(p, key);
            }
            let max = e.combat.as_ref().map_or(1, |c| c.stats.max_hp.max(1));
            let ratio = e.public.hp.unwrap_or(0) as f32 / max as f32;
            if let Some(bar) = ui.find_in(cell, "health") {
                ui.set_value(bar, ratio);
                ui.set_color(bar, Some(health_color(ratio)));
            }
            // Its level on the badge; an unspent skill point makes the card glow and
            // motes rise behind it.
            let level = e.combat.as_ref().map_or(1, |c| c.stats.level);
            if let Some(badge) = ui.find_in(cell, "level") {
                ui.set_text(badge, &level.to_string());
            }
            let points = super::card::unspent(k, id);
            if points > 0
                && let Some(rect) = ui.rect(cell)
            {
                self.rising.push(rect);
            }
            let label = if i < 3 {
                play.keymap.label(Shortcut::Hero(i))
            } else {
                String::new()
            };
            if let Some(key) = ui.find_in(cell, "key") {
                ui.set_text(key, &label);
            }
            let available = play.hero_available(id);
            let fallen = k.view.battle.fallen.contains(&e.id());
            let revival = fallen.then(|| revival(k, id)).flatten();
            if let Some(timer) = ui.find_in(cell, "timer") {
                let text = match revival {
                    Some(Some(s)) => s.to_string(),
                    Some(None) => "…".to_string(),
                    None => String::new(),
                };
                ui.set_text(timer, &text);
            }
            ui.set_enabled(cell, available);
            ui.set_checked(cell, play.selection.contains(id));
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: tr!("hud.hero-title", name = &d.name, level = level),
                    hotkey: (!label.is_empty()).then_some(label),
                    note: (points > 0).then(|| tr!("hud.hero-skill-points", count = points)),
                    body: if available {
                        tr!("hud.hero-select-tip")
                    } else if let Some(Some(s)) = revival {
                        tr!("hud.hero-reviving-in", seconds = s)
                    } else if revival.is_some() {
                        tr!("hud.hero-waiting-revival")
                    } else if fallen {
                        tr!("hud.hero-fallen")
                    } else {
                        tr!("hud.hero-unavailable")
                    },
                    ..Tooltip::default()
                }),
            );
        }
    }

    fn selection(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
        let ids = play.selection.ids();
        let focus = ids.first().and_then(|&id| k.entity(id));
        let group = ids.len() >= 2;
        // Unit cards for groups.
        if group {
            ui.set_count(n.cards, ids.len().min(48));
            for (i, &id) in ids.iter().take(48).enumerate() {
                let (Some(cell), Some(e)) = (ui.child(n.cards, i), k.entity(id)) else {
                    continue;
                };
                let d = k.def(e);
                if let Some(icon) = ui.find_in(cell, "icon") {
                    let key = self.image(ui, k, d.icon.as_str());
                    ui.set_image(icon, key);
                }
                let max = max_hp(e);
                let ratio = e.public.hp.unwrap_or(0) as f32 / max.max(1) as f32;
                if let Some(bar) = ui.find_in(cell, "health") {
                    ui.set_value(bar, ratio);
                    ui.set_color(bar, Some(health_color(ratio)));
                }
                ui.set_checked(cell, i == 0);
                ui.set_tooltip(
                    cell,
                    Some(Tooltip {
                        title: d.name.clone(),
                        body: tr!(
                            "hud.unit-card-tip",
                            hp = e.public.hp.unwrap_or(0),
                            max = max
                        ),
                        ..Tooltip::default()
                    }),
                );
            }
        } else {
            ui.set_count(n.cards, 0);
        }
        ui.set_visible(n.copy, !group && focus.is_some());
        let Some(e) = focus else {
            ui.set_image(n.portrait, None);
            ui.set_visible(n.level, false);
            ui.set_visible(n.experience, false);
            ui.set_count(n.inventory, 0);
            ui.set_visible(n.inventory, false);
            ui.set_count(n.queue, 0);
            ui.set_count(n.orders, 0);
            ui.set_count(n.statuses, 0);
            self.queue.clear();
            self.active = None;
            return;
        };
        let d = k.def(e);
        if self.live_portrait {
            let key = ui.image_key("@portrait");
            ui.set_image(n.portrait, Some(key));
        } else {
            let key = self.image(ui, k, d.icon.as_str());
            ui.set_image(n.portrait, key);
        }
        // Old portrait tip: name, the definition's price and supply, description.
        let costs = self.cost_rows(ui, k, &card::definition_costs(k, d));
        ui.set_tooltip(
            n.portrait,
            Some(Tooltip {
                title: d.name.clone(),
                costs,
                body: d.description.clone(),
                ..Tooltip::default()
            }),
        );
        ui.set_text(n.name, &escape(&d.name));
        // Level and experience.
        let level = e.combat.as_ref().map(|c| c.stats.level).or(d.level);
        let building = d.kind == Kind::Building;
        ui.set_visible(n.level, level.is_some() && !building);
        if let Some(l) = level {
            ui.set_text(n.level, &l.to_string());
        }
        let meter = experience(k, e);
        ui.set_visible(n.experience, meter.is_some());
        if let Some((fraction, label, tip)) = meter {
            ui.set_value(n.experience, fraction);
            ui.set_tooltip(
                n.experience,
                Some(Tooltip {
                    title: label,
                    body: tip,
                    ..Tooltip::default()
                }),
            );
        }
        // Stats: damage (units) and armour.
        let mut stats: Vec<(Option<String>, String, String, String)> = Vec::new();
        if let Some(body) = &d.body {
            if !building {
                let combat = d.behaviors.combat.as_ref();
                let damage = e.combat.as_ref().map_or_else(
                    || combat.map_or(0.0, |c| f64::from(c.damage)),
                    |c| c.stats.damage_milli as f64 / 1000.0,
                );
                let tip = match combat {
                    Some(c) => tr!(
                        "hud.damage-tip",
                        kind = k
                            .content
                            .rules
                            .damage_types
                            .get(c.damage_type.as_str())
                            .map_or(c.damage_type.as_str(), |t| t.name.as_str()),
                        seconds = &format!(
                            "{:.3}",
                            f64::from(
                                e.combat
                                    .as_ref()
                                    .map_or(c.cooldown_ticks, |s| s.stats.cooldown_ticks)
                            ) * 0.025
                        )
                    ),
                    None => tr!("hud.no-attack"),
                };
                stats.push((
                    Some("asset.icons.command-attack".into()),
                    tr!("hud.damage"),
                    trim(damage),
                    tip,
                ));
            }
            let armor = e
                .combat
                .as_ref()
                .map_or(f64::from(body.armor), |c| f64::from(c.stats.armor));
            let armor_type = k.content.rules.armor_types.get(body.armor_type.as_str());
            let reduction =
                (1.0 - 1.0 / (1.0 + k.content.rules.armor_coefficient * armor.max(0.0))) * 100.0;
            let mut tip = tr!(
                "hud.armor-tip",
                kind = armor_type.map_or(body.armor_type.as_str(), |t| t.name.as_str()),
                reduction = &format!("{reduction:.1}")
            );
            for (id, ty) in &k.content.rules.damage_types {
                let mult = k
                    .content
                    .rules
                    .damage_multipliers
                    .get(id.as_str())
                    .and_then(|m| m.get(body.armor_type.as_str()))
                    .copied()
                    .unwrap_or(1000);
                tip.push('\n');
                tip.push_str(&if ty.applies_armor {
                    tr!(
                        "hud.armor-class-damage",
                        kind = &ty.name,
                        percent = f64::from(mult) / 10.0
                    )
                } else {
                    tr!(
                        "hud.armor-class-damage-bypass",
                        kind = &ty.name,
                        percent = f64::from(mult) / 10.0
                    )
                });
            }
            stats.push((
                armor_type.map(|t| t.icon.to_string()),
                tr!("hud.armor"),
                trim(armor),
                tip,
            ));
        }
        ui.set_count(n.stats, stats.len());
        for (i, (icon, label, value, tip)) in stats.into_iter().enumerate() {
            let Some(cell) = ui.child(n.stats, i) else {
                continue;
            };
            if let (Some(node), Some(icon)) = (ui.find_in(cell, "icon"), icon) {
                let key = self.image(ui, k, &icon);
                ui.set_image(node, key);
            }
            if let Some(node) = ui.find_in(cell, "label") {
                ui.set_text(node, &label);
            }
            if let Some(node) = ui.find_in(cell, "value") {
                ui.set_text(node, &value);
            }
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: label,
                    body: tip,
                    ..Tooltip::default()
                }),
            );
        }
        // Vitals.
        let max = max_hp(e);
        let hp = e.public.hp.unwrap_or(0);
        ui.set_visible(n.health, e.public.hp.is_some());
        ui.set_value(n.health, hp as f32 / max.max(1) as f32);
        ui.set_text(n.health_value, &format!("{hp} / {max}"));
        let magic = e.magic.as_ref();
        let max_mana = magic.map_or(0, |m| m.max_mana);
        ui.set_visible(n.mana, max_mana > 0);
        if max_mana > 0 {
            let mana = magic.and_then(|m| m.caster.as_ref()).map(|c| c.mana);
            ui.set_value(n.mana, mana.unwrap_or(0) as f32 / max_mana as f32);
            ui.set_text(
                n.mana_value,
                &mana.map_or(format!("? / {max_mana}"), |m| format!("{m} / {max_mana}")),
            );
        }
        self.inventory(ui, n, k, e, &play.keymap);
        self.workplace(ui, n, play, k, e);
        self.order_queue(ui, n, k, e);
        self.statuses(ui, n, k, e);
    }

    fn inventory(
        &mut self,
        ui: &mut Ui,
        n: &Nodes,
        k: &Knowledge,
        e: &EntityView,
        keymap: &Keymap,
    ) {
        let Some(inv) = e.magic.as_ref().and_then(|m| m.inventory.as_ref()) else {
            ui.set_count(n.inventory, 0);
            ui.set_visible(n.inventory, false);
            return;
        };
        let own = k.controlled(e);
        ui.set_visible(n.inventory, !inv.slots.is_empty());
        ui.set_count(n.inventory, inv.slots.len());
        for (i, slot) in inv.slots.iter().enumerate() {
            let Some(cell) = ui.child(n.inventory, i) else {
                continue;
            };
            let item = slot.map(|d| k.catalog.get(d));
            if let Some(icon) = ui.find_in(cell, "icon") {
                let key = item.and_then(|d| self.image(ui, k, d.icon.as_str()));
                ui.set_image(icon, key);
            }
            ui.set_enabled(cell, own && item.is_some());
            let runtime = inv.runtime.get(i).cloned().flatten();
            if let Some(count) = ui.find_in(cell, "count") {
                let ready = runtime
                    .as_ref()
                    .map(|r| r.ready_tick)
                    .filter(|&t| t > k.tick());
                let charges = runtime.as_ref().and_then(|r| r.charges);
                let text = match (ready, charges) {
                    (Some(t), _) => Some(format!("{}s", (t - k.tick()).div_ceil(40))),
                    (None, Some(c)) => Some(c.to_string()),
                    _ => None,
                };
                ui.set_visible(count, text.is_some());
                if let Some(t) = text {
                    ui.set_text(count, &t);
                }
            }
            // Old inventory tooltip: tier, description, charges, cooldown, and how to use
            // or drop it; the slot key only for items that can be used.
            let ready = runtime
                .as_ref()
                .map(|r| r.ready_tick)
                .filter(|&t| t > k.tick())
                .map(|t| (t - k.tick()).div_ceil(40));
            let charges = runtime.as_ref().and_then(|r| r.charges);
            let usable = item.is_some_and(|d| {
                d.item_effect.as_ref().is_some_and(|x| {
                    matches!(x, canopy_content::items::ItemEffect::Consumable { .. })
                        || x.active().is_some()
                })
            });
            let can_use = own && usable && ready.is_none() && k.view.outcome.is_none();
            ui.set_tooltip(
                cell,
                item.map(|d| Tooltip {
                    title: d.name.clone(),
                    hotkey: can_use
                        .then(|| keymap.label(Shortcut::Inventory(i)))
                        .filter(|l| !l.is_empty()),
                    body: format!(
                        "{}{}{}{}",
                        d.item_tier
                            .map_or(String::new(), |t| tr!("hud.item-tier", tier = t) + " "),
                        d.description,
                        charges.map_or(String::new(), |c| format!(
                            " {}",
                            tr!("hud.item-charges", count = c)
                        )),
                        ready.map_or(String::new(), |s| format!(
                            " {}",
                            tr!("hud.item-cooldown", seconds = s)
                        )),
                    ),
                    note: own.then(|| {
                        if usable {
                            tr!("hud.item-use-tip")
                        } else {
                            tr!("hud.item-equipped-tip")
                        }
                    }),
                    ..Tooltip::default()
                }),
            );
        }
    }

    fn workplace(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge, e: &EntityView) {
        self.queue.clear();
        self.active = None;
        let own = k.controlled(e) && k.view.outcome.is_none();
        let fid = e.id().0;
        let d = k.def(e);
        let work = e.private.as_ref().map(|p| &*p.economy);
        let mut status: Option<String> = None;
        if let Some(w) = work {
            // One queue in the building's order (D73): training, research and revivals
            // mixed as queued. Only the first is under way.
            let items = canopy_sim::economy::lane::items(
                w.production.as_ref(),
                w.research.as_ref(),
                w.revival.as_ref(),
            );
            for (i, item) in items.iter().enumerate() {
                let first = i == 0;
                self.queue.push(match item {
                    Item::Training(q) => {
                        let od = k.catalog.get(q.definition);
                        let active = w
                            .production
                            .as_ref()
                            .and_then(|p| p.active.as_ref())
                            .filter(|a| a.queue == Some(q.id));
                        let total = od.creation.as_ref().map_or(1, |c| match c {
                            canopy_content::game::Creation::Train { work_ticks, .. }
                            | canopy_content::game::Creation::Construct { work_ticks, .. }
                            | canopy_content::game::Creation::Harvest { work_ticks, .. }
                            | canopy_content::game::Creation::Plant { work_ticks, .. } => {
                                *work_ticks
                            }
                        });
                        QueueEntry {
                            icon: od.icon.to_string(),
                            name: od.name.clone(),
                            progress: active
                                .filter(|_| first)
                                .map(|a| a.progress as f32 / total.max(1) as f32),
                            cancel: own.then_some(Action::Cancel {
                                actor: fid,
                                queue: Some(q.id),
                            }),
                        }
                    }
                    Item::Research(r) => {
                        let rule = k.content.rules.research.get(r.id.as_str());
                        QueueEntry {
                            icon: rule.map_or(String::new(), |x| x.icon.to_string()),
                            name: rule.map_or(r.id.to_string(), |x| x.name.clone()),
                            progress: rule
                                .filter(|_| first)
                                .map(|x| r.progress as f32 / x.work_ticks.max(1) as f32),
                            cancel: own.then(|| Action::CancelResearch {
                                actor: fid,
                                research: r.id.clone(),
                            }),
                        }
                    }
                    Item::Revival(r) => {
                        let hero = k.entity(r.hero.0).map(|h| k.def(h));
                        let total = d.behaviors.revival.as_ref().map_or(1, |policy| {
                            policy.work_ticks
                                + policy.work_ticks_per_level * r.level.saturating_sub(1)
                        });
                        QueueEntry {
                            icon: hero.map_or(String::new(), |h| h.icon.to_string()),
                            name: match hero {
                                Some(h) => tr!("hud.revive-named", name = &h.name),
                                None => tr!("hud.revive-hero"),
                            },
                            progress: first.then(|| r.progress as f32 / total.max(1) as f32),
                            cancel: own.then_some(Action::CancelRevival {
                                actor: fid,
                                hero: r.hero.0,
                            }),
                        }
                    }
                });
            }
            // The working task: an upgrade (everything queued waits), else the first
            // queued item.
            if let (Some(up), Some(rule)) = (&w.upgrade, &d.upgrade) {
                let target = k.content.definition(rule.target.as_str());
                self.active = Some(QueueEntry {
                    icon: target.map_or(String::new(), |t| t.icon.to_string()),
                    name: tr!(
                        "hud.upgrading-to",
                        name = target.map_or("", |t| t.name.as_str())
                    ),
                    progress: Some(up.progress as f32 / rule.work_ticks.max(1) as f32),
                    cancel: own.then_some(Action::CancelUpgrade { actor: fid }),
                });
            } else if !self.queue.is_empty() {
                self.active = Some(self.queue.remove(0));
            }
            status = w
                .status
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(crate::text::message);
        }
        // A building that makes goods by itself says what it makes (D62: a well).
        if let Some(income) = &d.income
            && e.public.construction.is_none()
        {
            let item = k
                .content
                .definition(income.item.as_str())
                .map_or(income.item.as_str(), |i| i.name.as_str());
            let seconds = f64::from(income.interval_ticks) / 40.0;
            status = Some(tr!(
                "hud.income-status",
                amount = income.amount,
                item = item,
                seconds = seconds
            ));
        }
        if let Some(site) = e
            .work
            .as_ref()
            .and_then(|w| w.site)
            .filter(|s| s.recommended > 0)
        {
            let workers = tr!(
                "hud.site-workers",
                workers = site.workers,
                recommended = site.recommended
            );
            status = Some(match e.public.resource.as_ref() {
                Some(r) => tr!(
                    "hud.site-workers-remaining",
                    workers = &workers,
                    amount = r.amount
                ),
                None => workers,
            });
        }
        // Out of the network its work waits (D66), whatever it is.
        let unpowered = e.work.as_ref().is_some_and(|w| w.unpowered);
        if unpowered {
            status = Some(crate::text::message(
                &canopy_sim::economy::Status::Unpowered.label(None),
            ));
        }
        if let Some(c) = e.public.construction {
            let total = d.creation.as_ref().map_or(1, |c| match c {
                canopy_content::game::Creation::Construct { work_ticks, .. } => *work_ticks,
                _ => 1,
            });
            self.active = Some(QueueEntry {
                icon: d.icon.to_string(),
                name: tr!("hud.under-construction"),
                progress: Some(c as f32 / total.max(1) as f32),
                cancel: None,
            });
        }
        match self.active.clone() {
            Some(a) => {
                ui.set_visible(n.task, true);
                let key = self.image(ui, k, &a.icon);
                ui.set_image(n.task_image, key);
                let p = a.progress.unwrap_or(0.0).clamp(0.0, 1.0);
                ui.set_value(n.task_meter, p);
                let label = match a.progress {
                    _ if unpowered => tr!("hud.task-no-power", name = &a.name),
                    Some(p) => tr!(
                        "hud.task-progress",
                        name = &a.name,
                        percent = (p.clamp(0.0, 1.0) * 100.0).floor()
                    ),
                    None => tr!("hud.task-waiting", name = &a.name),
                };
                ui.set_text(n.task_label, &escape(&label));
                ui.set_tooltip(
                    n.task_meter,
                    status.clone().map(|s| Tooltip {
                        title: a.name.clone(),
                        body: s,
                        ..Tooltip::default()
                    }),
                );
                ui.set_visible(n.status, false);
            }
            None => {
                ui.set_visible(n.task, false);
                match &status {
                    Some(s) => {
                        ui.set_text(n.status, &escape(s));
                        ui.set_visible(n.status, true);
                    }
                    None => ui.set_visible(n.status, false),
                }
            }
        }
        let entries = self.queue.clone();
        let wells = if entries.is_empty() && self.active.is_none() {
            0
        } else {
            entries.len().max(5)
        };
        ui.set_visible(n.queue, wells > 0);
        ui.set_count(n.queue, wells);
        for i in 0..wells {
            let Some(cell) = ui.child(n.queue, i) else {
                continue;
            };
            let entry = entries.get(i);
            if let Some(icon) = ui.find_in(cell, "icon") {
                let key = entry.and_then(|q| self.image(ui, k, &q.icon));
                ui.set_image(icon, key);
            }
            ui.set_enabled(cell, entry.is_some_and(|q| q.cancel.is_some()));
            ui.set_tooltip(
                cell,
                entry.map(|q| Tooltip {
                    title: q.name.clone(),
                    body: tr!("hud.queue-cancel-tip"),
                    ..Tooltip::default()
                }),
            );
        }
        let _ = play;
    }

    fn order_queue(&mut self, ui: &mut Ui, n: &Nodes, k: &Knowledge, e: &EntityView) {
        let queued = e
            .private
            .as_ref()
            .filter(|_| k.own(e))
            .map_or(&[][..], |p| p.queued.as_slice());
        ui.set_count(n.orders, queued.len());
        for (i, order) in queued.iter().enumerate() {
            let Some(cell) = ui.child(n.orders, i) else {
                continue;
            };
            let (verb, icon, target) = describe_order(k, k.def(e), order);
            if let Some(node) = ui.find_in(cell, "icon") {
                let key = self.image(ui, k, &icon);
                ui.set_image(node, key);
            }
            if let Some(node) = ui.find_in(cell, "number") {
                ui.set_text(node, &(i + 1).to_string());
            }
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: if target.is_empty() {
                        verb
                    } else {
                        format!("{verb}: {target}")
                    },
                    body: tr!("hud.queued-order-tip", number = i + 1),
                    ..Tooltip::default()
                }),
            );
        }
    }

    /// Item statuses on the focus (old `itemStatusCard`): auras, active item effects and
    /// rescue, with their modifiers and remaining time.
    fn statuses(&mut self, ui: &mut Ui, n: &Nodes, k: &Knowledge, e: &EntityView) {
        use canopy_content::items::ItemEffect;
        use canopy_sim::items::ItemStatusKind;
        let statuses = match e.magic.as_ref() {
            Some(m) if !e.remembered => m.item_statuses.as_slice(),
            _ => &[],
        };
        ui.set_count(n.statuses, statuses.len());
        for (i, s) in statuses.iter().enumerate() {
            let Some(cell) = ui.child(n.statuses, i) else {
                continue;
            };
            let d = k.catalog.get(s.item);
            if let Some(node) = ui.find_in(cell, "icon") {
                let key = self.image(ui, k, d.icon.as_str());
                ui.set_image(node, key);
            }
            let (aura, active) = match &d.item_effect {
                Some(ItemEffect::Equipment { aura, active, .. })
                | Some(ItemEffect::Consumable { aura, active, .. }) => {
                    (aura.as_ref(), active.as_ref())
                }
                None => (None, None),
            };
            let modifiers = match s.kind {
                ItemStatusKind::Aura => aura.map(|a| modifier_text(&a.modifiers)),
                ItemStatusKind::Rescue => Some(tr!("hud.invulnerable")),
                ItemStatusKind::Active => active
                    .and_then(|a| a.status.as_ref())
                    .map(|st| modifier_text(&st.modifiers)),
            };
            let mut parts: Vec<String> = modifiers.into_iter().filter(|m| !m.is_empty()).collect();
            if let Some(shield) = s.shield {
                parts.push(tr!("hud.shield-remaining", amount = shield));
            }
            parts.push(match (s.kind, aura) {
                (ItemStatusKind::Aura, Some(a)) => tr!("hud.aura-range", radius = a.radius),
                _ => tr!(
                    "hud.seconds-remaining",
                    seconds = s.expires.saturating_sub(k.tick()).div_ceil(40)
                ),
            });
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: if s.kind == ItemStatusKind::Aura {
                        tr!("hud.aura-title", name = &d.name)
                    } else {
                        d.name.clone()
                    },
                    body: escape(&parts.join(" · ")),
                    ..Tooltip::default()
                }),
            );
        }
    }

    fn card(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
        let armed = match &play.mode {
            super::Mode::Target { source, .. } | super::Mode::Place { source, .. } => {
                Some(source.0.clone())
            }
            super::Mode::Idle => None,
        };
        ui.set_count(n.commands, card::SLOTS);
        for slot in 0..card::SLOTS {
            let Some(cell) = ui.child(n.commands, slot) else {
                continue;
            };
            let entry = play.layout.slots[slot].map(|i| &play.entries[i]);
            self.cells[slot] = entry.map(|c| c.id.clone());
            // Empty cells stay in the grid as dark wells (old twelve wells).
            let Some(c) = entry else {
                if let Some(icon) = ui.find_in(cell, "icon") {
                    ui.set_image(icon, None);
                }
                if let Some(key) = ui.find_in(cell, "key") {
                    ui.set_text(key, "");
                }
                if let Some(cd) = ui.find_in(cell, "cooldown") {
                    ui.set_visible(cd, false);
                }
                if let Some(mark) = ui.find_in(cell, "passive") {
                    ui.set_visible(mark, false);
                }
                if let Some(badge) = ui.find_in(cell, "badge") {
                    ui.set_visible(badge, false);
                }
                ui.set_checked(cell, false);
                ui.set_enabled(cell, false);
                ui.set_tooltip(cell, None);
                continue;
            };
            if let Some(icon) = ui.find_in(cell, "icon") {
                let key = self.image(ui, k, c.icon.as_str());
                ui.set_image(icon, key);
            }
            // A passive shows its mark instead of a hotkey: pressing it does nothing.
            if let Some(key) = ui.find_in(cell, "key") {
                let label = c.chord.as_ref().filter(|_| !c.passive).map(Chord::label);
                ui.set_text(key, &label.unwrap_or_default());
            }
            if let Some(mark) = ui.find_in(cell, "passive") {
                ui.set_visible(mark, c.passive);
            }
            if let Some(badge) = ui.find_in(cell, "badge") {
                ui.set_visible(badge, c.badge.is_some());
                if let Some(n) = c.badge {
                    ui.set_text(badge, &n.to_string());
                }
            }
            ui.set_enabled(cell, c.enabled() || c.passive);
            ui.set_checked(
                cell,
                c.autocast == Some(true) || armed.as_deref() == Some(c.id.as_str()),
            );
            if let Some(cd) = ui.find_in(cell, "cooldown") {
                match c.cooldown {
                    Some((rem, total)) => {
                        ui.set_visible(cd, true);
                        ui.set_value(cd, (rem as f32 / total.max(1) as f32).min(1.0));
                        if let Some(s) = ui.find_in(cd, "seconds") {
                            ui.set_text(s, &((rem as f32 * 0.025).ceil() as u32).to_string());
                        }
                    }
                    None => ui.set_visible(cd, false),
                }
            }
            let tip = self.tooltip(ui, k, c);
            ui.set_tooltip(cell, Some(tip));
        }
        self.banners = play
            .layout
            .banners
            .iter()
            .map(|&i| play.entries[i].id.clone())
            .collect();
        ui.set_count(n.banners, self.banners.len());
        for (i, &e) in play.layout.banners.iter().enumerate() {
            let c = &play.entries[e];
            let Some(cell) = ui.child(n.banners, i) else {
                continue;
            };
            if let Some(icon) = ui.find_in(cell, "icon") {
                let key = self.image(ui, k, c.icon.as_str());
                ui.set_image(icon, key);
            }
            if let Some(label) = ui.find_in(cell, "label") {
                ui.set_text(label, &escape(&c.name));
            }
            if let Some(key) = ui.find_in(cell, "key") {
                ui.set_text(key, &c.chord.as_ref().map(Chord::label).unwrap_or_default());
            }
            ui.set_enabled(cell, c.enabled());
            let tip = self.tooltip(ui, k, c);
            ui.set_tooltip(cell, Some(tip));
        }
        ui.set_visible(n.pager, play.layout.pages > 1);
        ui.set_text(
            n.pager_label,
            &format!("{} / {}", play.layout.page + 1, play.layout.pages),
        );
    }

    /// Tooltip cost rows: an icon and the amount, mana as text.
    fn cost_rows(&mut self, ui: &mut Ui, k: &Knowledge, costs: &[card::CostView]) -> Vec<Cost> {
        costs
            .iter()
            .map(|cost| {
                if cost.icon.as_str() == MANA {
                    Cost {
                        image: None,
                        amount: tr!("hud.mana-cost", amount = cost.amount),
                    }
                } else {
                    Cost {
                        image: self.image(ui, k, cost.icon.as_str()),
                        amount: cost.amount.to_string(),
                    }
                }
            })
            .collect()
    }

    fn tooltip(&mut self, ui: &mut Ui, k: &Knowledge, c: &Command) -> Tooltip {
        let costs = self.cost_rows(ui, k, &c.costs);
        let mut body = vec![escape(&c.description)];
        if c.passive {
            body.push(format!("[color=#d9c27a]{}[/color]", tr!("hud.passive")));
        } else if let Some(r) = &c.reason {
            body.push(format!("[color=#ff8a65]{}[/color]", escape(r)));
        }
        if c.actors.len() > 1 {
            body.push(tr!("hud.eligible-actors", count = c.actors.len()));
        }
        Tooltip {
            title: escape(&c.name),
            hotkey: c.chord.as_ref().map(Chord::label),
            costs,
            body: body
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            note: c.alternate.is_some().then(|| tr!("hud.autocast-tip")),
        }
    }

    fn groups(&mut self, ui: &mut Ui, n: &Nodes, play: &Play) {
        let any = (0..10).any(|s| !play.groups.get(s).is_empty());
        ui.set_visible(n.groups, any);
        let mut selected = play.selection.ids().to_vec();
        selected.sort_unstable();
        for slot in 0..10 {
            // Each group keeps its column (old grid columns 1–10), so a slot without
            // members stays as empty, click-through space.
            let Some(slot_node) = ui.child(n.groups, slot) else {
                continue;
            };
            let Some(cell) = ui.find_in(slot_node, "button") else {
                continue;
            };
            let members = play.groups.get(slot);
            ui.set_visible(cell, !members.is_empty());
            if let Some(count) = ui.find_in(cell, "count") {
                ui.set_text(count, &members.len().to_string());
            }
            let digit = (slot + 1) % 10;
            let recall = play.keymap.label(Shortcut::Recall(slot));
            if let Some(key) = ui.find_in(cell, "key") {
                ui.set_text(key, if recall.is_empty() { "—" } else { &recall });
            }
            let mut m = members.to_vec();
            m.sort_unstable();
            ui.set_checked(cell, !m.is_empty() && m == selected);
            // Old group tooltip: members, the bound assign and add keys, double press.
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: tr!("hud.control-group", digit = digit),
                    hotkey: (!recall.is_empty()).then_some(recall),
                    body: tr!(
                        "hud.control-group-tip",
                        count = members.len(),
                        assign = &play.keymap.label(Shortcut::Assign(slot)),
                        add = &play.keymap.label(Shortcut::Add(slot)),
                    ),
                    ..Tooltip::default()
                }),
            );
        }
    }

    fn chat(&mut self, ui: &mut Ui, n: &Nodes, play: &Play) {
        let now = play.now().as_secs_f32();
        let lines: Vec<&super::ChatLine> = play
            .chat
            .iter()
            .filter(|l| play.chat_open || now - l.at < CHAT_SECONDS)
            .collect();
        let lines = &lines[lines
            .len()
            .saturating_sub(if play.chat_open { 40 } else { 8 })..];
        ui.set_count(n.chat_log, lines.len());
        for (i, l) in lines.iter().enumerate() {
            if let Some(cell) = ui.child(n.chat_log, i) {
                let [r, g, b] = l.color;
                ui.set_text(
                    cell,
                    &format!(
                        "[color=#{r:02x}{g:02x}{b:02x}]{}:[/color] {}",
                        escape(&l.name),
                        escape(&l.text)
                    ),
                );
            }
        }
        ui.set_visible(n.chat_input, play.chat_open);
        if play.chat_open && !self.chat_was_open {
            ui.set_text(n.chat_input, "");
            ui.focus(Some(n.chat_input));
        }
        self.chat_was_open = play.chat_open;
    }

    fn observer(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
        let observing = play.observer() && !self.hide_observer;
        ui.set_visible(n.observer, observing);
        if !observing {
            return;
        }
        // Income: gross deliveries over the last 60 s of game time.
        let tick = k.view.tick;
        self.first_tick.get_or_insert(tick);
        if let std::collections::btree_map::Entry::Vacant(slot) = self.deliveries.entry(tick) {
            slot.insert(
                k.view
                    .deliveries
                    .iter()
                    .map(|d| (d.owner, d.item.to_string(), d.amount))
                    .collect(),
            );
        }
        self.deliveries.retain(|&t, _| t + INCOME_TICKS > tick);
        let sampled = tick
            .saturating_sub(self.first_tick.unwrap_or(tick))
            .min(INCOME_TICKS);
        let seconds = sampled as f32 * 0.025;
        let colonies = self.colonies.clone().unwrap_or_default();
        // One column per currency (old `observerResources`, in display order).
        let currencies: Vec<String> = colonies
            .first()
            .map(|c| c.colony.goods.iter().map(|g| g.item.to_string()).collect())
            .unwrap_or_default();
        if let Some(goods) = ui.find_in(n.observer_columns, "goods") {
            ui.set_count(goods, currencies.len());
            for (i, item) in currencies.iter().enumerate() {
                if let Some(cell) = ui.child(goods, i) {
                    ui.set_text(cell, &k.name_of(item));
                }
            }
        }
        ui.set_count(n.observer_rows, colonies.len());
        for (i, stats) in colonies.iter().enumerate() {
            let Some(row) = ui.child(n.observer_rows, i) else {
                continue;
            };
            let player = stats.player;
            let owner = Owner::Player(player);
            let color = self
                .palette
                .get(usize::from(player))
                .copied()
                .unwrap_or([200, 200, 200]);
            if let Some(bar) = ui.find_in(row, "color") {
                ui.set_color(bar, Some(Color::rgba(color[0], color[1], color[2], 255)));
            }
            let defeated = k
                .view
                .outcome
                .as_ref()
                .is_some_and(|o| o.defeated.contains(&owner));
            ui.set_opacity(row, if defeated { 0.58 } else { 1.0 });
            if let Some(cell) = ui.find_in(row, "player") {
                // Old row: the slot badge, the name and "Player N · controller".
                let (name, ai) = self
                    .players
                    .iter()
                    .find(|(slot, ..)| *slot == player)
                    .map_or(
                        (tr!("hud.player", number = player + 1), true),
                        |(_, n, ai)| (n.clone(), *ai),
                    );
                let role = if defeated {
                    tr!("hud.role-defeated")
                } else if ai {
                    tr!("hud.role-computer")
                } else {
                    tr!("hud.role-human")
                };
                ui.set_text(
                    cell,
                    &format!(
                        "[color=#{:02x}{:02x}{:02x}][b]{}[/b][/color]  {}\n[color=#8c9d9b]{}[/color]",
                        color[0],
                        color[1],
                        color[2],
                        player + 1,
                        escape(&name),
                        tr!("hud.player-role", number = player + 1, role = &role),
                    ),
                );
                ui.set_tooltip(
                    cell,
                    Some(Tooltip {
                        title: name,
                        ..Tooltip::default()
                    }),
                );
            }
            if let Some(goods) = ui.find_in(row, "goods") {
                ui.set_count(goods, stats.colony.goods.len());
                for (j, g) in stats.colony.goods.iter().enumerate() {
                    let Some(cell) = ui.child(goods, j) else {
                        continue;
                    };
                    let income: u32 = self
                        .deliveries
                        .values()
                        .flatten()
                        .filter(|(o, item, _)| *o == owner && item == g.item.as_str())
                        .map(|(_, _, a)| a)
                        .sum();
                    let per_minute = if seconds >= 1.0 {
                        (income as f32 * 60.0 / seconds).round() as u32
                    } else {
                        0
                    };
                    let d = k.content.definition(g.item.as_str());
                    if let (Some(icon), Some(d)) = (ui.find_in(cell, "icon"), d) {
                        let key = self.image(ui, k, d.icon.as_str());
                        ui.set_image(icon, key);
                    }
                    if let Some(amount) = ui.find_in(cell, "amount") {
                        ui.set_text(amount, &thousands(g.stored));
                    }
                    if let Some(label) = ui.find_in(cell, "income") {
                        ui.set_text(label, &format!("+{}/min", thousands(per_minute)));
                    }
                    ui.set_tooltip(
                        cell,
                        Some(Tooltip {
                            title: d.map_or_else(|| g.item.to_string(), |d| d.name.clone()),
                            body: tr!(
                                "hud.observer-goods-tip",
                                description = d.map_or("", |d| d.description.as_str()),
                                stored = g.stored,
                                per_minute = per_minute
                            )
                            .trim_start()
                            .to_owned(),
                            ..Tooltip::default()
                        }),
                    );
                }
            }
            if let Some(cell) = ui.find_in(row, "units") {
                ui.set_text(cell, &thousands(stats.units));
                ui.set_tooltip(
                    cell,
                    Some(Tooltip {
                        title: tr!("hud.total-units"),
                        body: tr!(
                            "hud.total-units-tip",
                            units = stats.units,
                            workers = stats.workers,
                            army = stats.army
                        ),
                        ..Tooltip::default()
                    }),
                );
            }
            if let Some(cell) = ui.find_in(row, "workers") {
                let s = &stats.colony.supply;
                ui.set_text(cell, &thousands(stats.workers));
                ui.set_tooltip(
                    cell,
                    Some(Tooltip {
                        title: tr!("hud.total-workers"),
                        body: tr!(
                            "hud.total-workers-tip",
                            available = stats.colony.population.available,
                            committed = s.committed,
                            capacity = s.capacity,
                            used = s.used,
                            reserved = s.reserved
                        ),
                        ..Tooltip::default()
                    }),
                );
            }
            // Every hero: icon, level and, unless alive, "Fallen" or "Reviving".
            if let Some(list) = ui.find_in(row, "heroes") {
                if let Some(none) = ui.find_in(list, "none") {
                    ui.set_visible(none, stats.heroes.is_empty());
                }
                ui.set_count(list, stats.heroes.len());
                for (j, hero) in stats.heroes.iter().enumerate() {
                    let Some(cell) = ui.child(list, j) else {
                        continue;
                    };
                    let d = k.catalog.get(hero.definition);
                    if let Some(icon) = ui.find_in(cell, "icon") {
                        let key = self.image(ui, k, d.icon.as_str());
                        ui.set_image(icon, key);
                    }
                    if let Some(level) = ui.find_in(cell, "level") {
                        ui.set_text(level, &tr!("hud.level-short", level = hero.level));
                    }
                    let status = match hero.status {
                        crate::sim::HeroStatus::Alive => String::new(),
                        crate::sim::HeroStatus::Fallen => tr!("hud.hero-status-fallen"),
                        crate::sim::HeroStatus::Reviving => tr!("hud.hero-status-reviving"),
                    };
                    if let Some(label) = ui.find_in(cell, "status") {
                        ui.set_text(label, &status);
                        ui.set_visible(label, !status.is_empty());
                    }
                    ui.set_enabled(cell, hero.status == crate::sim::HeroStatus::Alive);
                    ui.set_tooltip(
                        cell,
                        Some(Tooltip {
                            title: tr!(
                                "hud.observer-hero",
                                name = &d.name,
                                level = hero.level,
                                status = &hero.status.label()
                            ),
                            ..Tooltip::default()
                        }),
                    );
                }
            }
        }
        ui.set_text(
            n.observer_footer,
            &if sampled >= INCOME_TICKS {
                tr!("hud.income-footer")
            } else {
                tr!("hud.income-footer-sampled", seconds = seconds.floor())
            },
        );
        ui.set_tooltip(
            n.observer_footer,
            Some(Tooltip {
                title: tr!("hud.income"),
                body: tr!("hud.income-tip"),
                ..Tooltip::default()
            }),
        );
    }

    /// Acts on the UI's interactions; returns events for the game (camera, menu).
    pub fn act(
        &mut self,
        ui: &mut Ui,
        play: &mut Play,
        actions: Vec<canopy_ui::Action>,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        for a in actions {
            let name = a.name.as_deref().unwrap_or("");
            let index = a.index.unwrap_or(0);
            match (name, &a.kind) {
                (
                    "command",
                    ActionKind::Click {
                        button, modifiers, ..
                    },
                ) => {
                    if let Some(Some(id)) = self.cells.get(index).cloned() {
                        match button {
                            PointerButton::Secondary => play.alternate(&id),
                            _ => play.activate_id(&id, modifiers.shift),
                        }
                    }
                }
                ("banner", ActionKind::Click { modifiers, .. }) => {
                    if let Some(id) = self.banners.get(index).cloned() {
                        play.activate_id(&id, modifiers.shift);
                    }
                }
                ("page-prev", ActionKind::Click { .. }) => play.turn_page(false),
                ("page-next", ActionKind::Click { .. }) => play.turn_page(true),
                (
                    "unit-card",
                    ActionKind::Click {
                        modifiers, count, ..
                    },
                ) => {
                    if let Some(&id) = play.selection.ids().get(index) {
                        play.card_click(id, modifiers.shift, *count >= 2);
                    }
                }
                ("hero", ActionKind::Click { count, .. }) => {
                    play.hero_key(index);
                    if *count >= 2
                        && let Some(&id) = play.heroes().get(index)
                        && let Some(e) = play.knowledge().entity(id)
                    {
                        events.push(Event::LookAt(play.knowledge().position(e)));
                    }
                }
                ("item", ActionKind::Click { button, .. }) => {
                    play.item(index, *button == PointerButton::Secondary);
                }
                ("queue", ActionKind::Click { .. }) => {
                    if let Some(action) = self.queue.get(index).and_then(|q| q.cancel.clone()) {
                        play.cancel(action);
                    }
                }
                ("task", ActionKind::Click { .. }) => {
                    if let Some(action) = self.active.as_ref().and_then(|q| q.cancel.clone()) {
                        play.cancel(action);
                    }
                }
                ("group", ActionKind::Click { modifiers, .. }) => {
                    play.group_click(
                        index,
                        super::input::Mods {
                            shift: modifiers.shift,
                            ctrl: modifiers.ctrl,
                            alt: modifiers.alt,
                            logo: modifiers.logo,
                        },
                    );
                }
                ("minimap", ActionKind::DragStart { button, at }) => {
                    let (x, z) = (
                        self.minimap_area[0] + at.x * self.minimap_area[2],
                        self.minimap_area[1] + at.y * self.minimap_area[2],
                    );
                    let ping = *button == PointerButton::Primary && play.alt_held();
                    self.minimap_look = *button == PointerButton::Primary && !ping;
                    match button {
                        _ if ping => play.ping(x, z),
                        PointerButton::Secondary => play.minimap_command(x, z),
                        _ => events.push(Event::LookAt([x, z])),
                    }
                }
                ("minimap", ActionKind::Drag { at }) if self.minimap_look => {
                    events.push(Event::LookAt([
                        self.minimap_area[0] + at.x * self.minimap_area[2],
                        self.minimap_area[1] + at.y * self.minimap_area[2],
                    ]));
                }
                ("menu", ActionKind::Click { .. }) => events.push(Event::OpenMenu),
                ("idle", ActionKind::Click { .. }) => play.next_idle_worker(),
                ("army", ActionKind::Click { .. }) => play.select_army(),
                ("debug", ActionKind::Click { .. }) => events.push(Event::ToggleDebug),
                (_, ActionKind::Submitted(text)) => {
                    play.say(text);
                    if let Some(n) = &self.nodes {
                        ui.set_text(n.chat_input, "");
                        ui.focus(None);
                    }
                }
                _ => {}
            }
        }
        // Escape in the chat field leaves it: close the chat.
        if play.chat_open && self.chat_was_open && !ui.wants_keyboard() {
            play.chat_open = false;
        }
        events.extend(play.take_events());
        events
    }
}

fn max_hp(e: &EntityView) -> u32 {
    e.combat
        .as_ref()
        .map_or(e.public.hp.unwrap_or(0), |c| c.stats.max_hp)
}

/// Experience meter (old `experienceMeter`): fraction, label and tooltip.
fn experience(k: &Knowledge, e: &EntityView) -> Option<(f32, String, String)> {
    let levels = &k.def(e).behaviors.progression.as_ref()?.levels;
    let xp = e.combat.as_ref()?.experience?;
    if levels.is_empty() {
        return None;
    }
    let reached = levels.iter().filter(|l| l.experience <= xp).count().max(1);
    if reached >= levels.len() {
        return Some((
            1.0,
            tr!("hud.maximum-level", level = reached),
            tr!("hud.total-experience", xp = xp),
        ));
    }
    let base = levels[reached - 1].experience;
    let next = levels[reached].experience;
    let fraction = (xp - base) as f32 / (next - base).max(1) as f32;
    Some((
        fraction,
        tr!("hud.experience", gained = xp - base, needed = next - base),
        tr!(
            "hud.experience-tip",
            xp = xp,
            more = next - xp,
            level = reached + 1
        ),
    ))
}

/// The verb, icon and target of a queued order (old `orderQueue.ts`).
fn describe_order(
    k: &Knowledge,
    caster: &canopy_content::game::Definition,
    order: &canopy_sim::orders::Order,
) -> (String, String, String) {
    use canopy_sim::orders::Order;
    let meta = |verb: &str| {
        k.content
            .actions
            .actions
            .get(verb)
            .map(|m| (m.name.clone(), m.icon.to_string()))
            .unwrap_or_else(|| (verb.to_owned(), String::new()))
    };
    let target = |id: canopy_sim::entity::EntityId| {
        k.entity(id.0).map(|e| k.def(e)).or_else(|| {
            k.view
                .scenery
                .iter()
                .find(|s| s.id == id)
                .map(|s| k.catalog.get(s.definition))
        })
    };
    let place = |p: &canopy_sim::entity::Place| format!("{}, {}", p.x / 1000, p.y / 1000);
    match order {
        Order::Move {
            destination,
            attack_move,
        } => {
            let (name, icon) = meta(if *attack_move { "attack" } else { "move" });
            (name, icon, place(destination))
        }
        Order::Hold => {
            let (name, icon) = meta("hold");
            (name, icon, "here".into())
        }
        Order::Patrol { destination, .. } => {
            let (name, icon) = meta("patrol");
            (name, icon, place(destination))
        }
        Order::Follow { target: t, .. }
        | Order::Attack { target: t, .. }
        | Order::Garrison { target: t } => {
            let verb = match order {
                Order::Follow { .. } => "follow",
                Order::Attack { .. } => "attack",
                _ => "garrison",
            };
            let (name, icon) = meta(verb);
            (
                name,
                icon,
                target(*t).map_or_else(|| tr!("hud.unknown"), |d| d.name.clone()),
            )
        }
        Order::Gather { target: t }
        | Order::Construct { target: t }
        | Order::Pickup { target: t } => {
            let d = target(*t);
            let verb = match order {
                Order::Gather { .. } => tr!("hud.order-gather"),
                Order::Construct { .. } => tr!("hud.order-build"),
                _ => tr!("hud.order-pick-up"),
            };
            (
                verb,
                d.map_or(String::new(), |d| d.icon.to_string()),
                d.map_or_else(|| tr!("hud.unknown"), |d| d.name.clone()),
            )
        }
        // Old `orderQueue`: the spell's name and its presentation's icon, else Move's.
        Order::CastAbility { binding, .. } => {
            let spell = caster
                .behaviors
                .abilities
                .as_ref()
                .and_then(|a| a.bindings.iter().find(|b| b.id == *binding))
                .and_then(|b| k.content.abilities.ability(b.ability.as_str()));
            let icon = spell
                .and_then(|s| s.presentation.icon.as_ref())
                .map_or_else(|| meta("move").1, |i| i.to_string());
            (
                tr!("hud.order-cast"),
                icon,
                spell.map_or_else(|| binding.clone(), |s| s.name.clone()),
            )
        }
    }
}

/// The stat changes of an item, aura or item status (old `modifierText`).
fn modifier_text(m: &canopy_content::items::ItemModifiers) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut number = |value: Option<f64>, label: &str, permille: bool| {
        if let Some(v) = value {
            let shown = if permille { v / 10.0 } else { v };
            let sign = if v >= 0.0 { "+" } else { "" };
            let pct = if permille { "%" } else { "" };
            out.push(crate::text::tr_with(
                "hud.modifier",
                &[
                    (
                        "value",
                        format!("{sign}{}{pct}", trim(shown)).as_str().into(),
                    ),
                    ("label", crate::text::tr(label).as_str().into()),
                ],
            ));
        }
    };
    number(m.max_hp.map(f64::from), "hud.modifier-max-health", false);
    number(m.max_mana.map(f64::from), "hud.modifier-max-mana", false);
    number(m.damage.map(f64::from), "hud.modifier-damage", false);
    number(m.armor.map(f64::from), "hud.modifier-armor", false);
    number(
        m.health_regen_per_second,
        "hud.modifier-health-regen",
        false,
    );
    number(m.mana_regen_per_second, "hud.modifier-mana-regen", false);
    number(
        m.damage_permille.map(f64::from),
        "hud.modifier-attack-damage",
        true,
    );
    number(
        m.attack_speed_permille.map(f64::from),
        "hud.modifier-attack-speed",
        true,
    );
    number(
        m.move_speed_permille.map(f64::from),
        "hud.modifier-move-speed",
        true,
    );
    number(
        m.cooldown_reduction_permille.map(f64::from),
        "hud.modifier-cooldown-reduction",
        true,
    );
    number(
        m.lifesteal_permille.map(f64::from),
        "hud.modifier-lifesteal",
        true,
    );
    number(
        m.melee_reflection_permille.map(f64::from),
        "hud.modifier-melee-reflection",
        true,
    );
    for (on, key) in [
        (m.rooted, "hud.rooted"),
        (m.control_immune, "hud.control-immune"),
        (m.invulnerable, "hud.invulnerable"),
    ] {
        if on == Some(true) {
            out.push(crate::text::tr(key));
        }
    }
    out.join(" · ")
}

/// Escapes markup brackets in plain text.
pub fn escape(text: &str) -> String {
    text.replace('[', "[[")
}

fn trim(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}

/// A count with thousands separators (old `toLocaleString("en-US")`).
fn thousands(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A fallen hero's revival: the whole seconds left while a building works on it, `None`
/// while it waits in a revival queue; `None` outside when nobody revives it.
pub(super) fn revival(k: &Knowledge, hero: u32) -> Option<Option<u32>> {
    k.view.entities.iter().find_map(|e| {
        let queue = e.private.as_ref()?.economy.revival.as_ref()?;
        let policy = k.def(e).behaviors.revival.as_ref()?;
        let entry = queue.queue.iter().find(|r| r.hero.0 == hero)?;
        if entry.progress == 0 {
            return Some(None);
        }
        let total = policy.work_ticks + policy.work_ticks_per_level * entry.level.saturating_sub(1);
        let left = total.saturating_sub(entry.progress);
        Some(Some((f64::from(left) * 0.025).ceil() as u32))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::ai_fixture as fx;
    use serde_json::json;

    #[test]
    fn hud_art_follows_the_local_slot_and_resets_between_matches() {
        let world = fx::world(&fx::skirmish(vec![], vec![]), false);
        let mut content = (*world.catalog().content).clone();
        for name in ["wood", "metal"] {
            for piece in ["frame", "resources"] {
                content.manifest.records.push(
                    serde_json::from_value(canopy_content::testing::image(&format!(
                        "test.{name}.{piece}"
                    )))
                    .unwrap(),
                );
            }
            content.rules.races.insert(name.into(), serde_json::from_value(json!({
                "name": name, "description": "Fixture", "roster": [],
                "hud": {"frame": format!("test.{name}.frame"), "resources": format!("test.{name}.resources")}
            })).unwrap());
        }
        content.rules.default_race = "wood".into();
        // Slot numbers deliberately differ from vector indices.
        let setup = serde_json::from_value(json!({"seed": 1, "slots": [
            {"player": 7, "kind": "human", "race": "metal"},
            {"player": 3, "kind": "ai"}
        ]}))
        .unwrap();
        let mut hud = Hud::new();
        hud.configure(&content, &setup, Some(7));
        let expected = |name: &str| {
            ["frame", "resources"]
                .map(|piece| Some(format!("assets/library/test.{name}.{piece}/image.png")))
        };
        assert_eq!(hud.appearance, expected("metal"));
        hud.configure(&content, &setup, Some(3));
        assert_eq!(hud.appearance, expected("wood"));
        hud.configure(&content, &setup, None);
        assert_eq!(hud.appearance, expected("wood"));
        content.rules.races.get_mut("wood").unwrap().hud = None;
        hud.configure(&content, &setup, Some(3));
        assert_eq!(hud.appearance, [None, None]);
    }
}
