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
use canopy_sim::{Action, view::EntityView};
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
    observer: NodeId,
    observer_columns: NodeId,
    observer_rows: NodeId,
    observer_footer: NodeId,
}

impl Nodes {
    fn find(ui: &Ui) -> Option<Self> {
        Some(Self {
            root: find(ui, "hud")?,
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
    requests: BTreeSet<String>,
    /// The portrait shows the live render target `@portrait` instead of the icon.
    pub live_portrait: bool,
    /// The minimap shows the render target `@minimap`.
    pub minimap: bool,
    pub palette: Palette,
    /// The match's players: slot, name and whether a computer plays it.
    pub players: Vec<(u8, String, bool)>,
    /// Map size, for minimap clicks.
    pub map_size: f32,
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
    pub colonies: Option<std::sync::Arc<Vec<(u8, canopy_sim::economy::view::Colony)>>>,
}

impl Default for Hud {
    fn default() -> Self {
        Self::new()
    }
}

impl Hud {
    pub fn new() -> Self {
        Self {
            nodes: None,
            requests: BTreeSet::new(),
            live_portrait: false,
            minimap: false,
            palette: Vec::new(),
            players: Vec::new(),
            map_size: 1.0,
            cells: Default::default(),
            banners: Vec::new(),
            queue: Vec::new(),
            active: None,
            chat_was_open: false,
            deliveries: BTreeMap::new(),
            first_tick: None,
            colonies: None,
        }
    }

    /// Image names used since the last call that the game has to register.
    pub fn take_image_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.requests).into_iter().collect()
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
        self.messages(ui, &n, play, &k);
        self.resources(ui, &n, &k);
        self.heroes(ui, &n, play, &k);
        self.selection(ui, &n, play, &k);
        self.card(ui, &n, play, &k);
        self.groups(ui, &n, play);
        self.chat(ui, &n, play);
        self.observer(ui, &n, play, &k);
        if self.minimap {
            let key = ui.image_key("@minimap");
            ui.set_image(n.minimap, Some(key));
        }
        self.nodes = Some(n);
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
                        "Mission complete".to_owned()
                    } else {
                        "Mission failed".to_owned()
                    }
                }
                (_, None) => "Draw — both main forts fell.".to_owned(),
                (None, Some(Owner::Player(p))) => {
                    format!("Player {} wins — the rival Mound fell.", p + 1)
                }
                (None, Some(_)) => "Match over".to_owned(),
                (Some(me), Some(w)) if w == me => "Victory — the enemy main fort fell.".to_owned(),
                _ => "Defeat — your main fort fell.".to_owned(),
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
                    body: format!(
                        "{}\n{} available · {} reserved\n{} stored · {} being carried",
                        d.map_or("", |d| d.description.as_str()),
                        g.available,
                        g.reserved,
                        g.stored,
                        g.in_transit
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
                if s.over_capacity > 0 {
                    ui.set_text(value, &format!("[color=#ed8d77]{text}[/color]"));
                } else {
                    ui.set_text(value, &text);
                }
            }
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: "Supply".into(),
                    body: format!(
                        "{} used by living units. {} reserved for queued units.\n{} available; maximum {}.\nCompleted halls and mounds provide supply. Started training finishes even if capacity is lost; waiting units need enough supply to start.",
                        s.used, s.reserved, s.available, s.limit
                    ),
                    ..Tooltip::default()
                }),
            );
        }
    }

    fn heroes(&mut self, ui: &mut Ui, n: &Nodes, play: &Play, k: &Knowledge) {
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
            ui.set_enabled(cell, available);
            ui.set_checked(cell, play.selection.contains(id));
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: d.name.clone(),
                    hotkey: (!label.is_empty()).then_some(label),
                    body: if available {
                        "Click to select. Double-click to center the camera.".into()
                    } else if fallen {
                        "Fallen. Revive this hero at a Sanctuary.".into()
                    } else {
                        "Hero is currently unavailable.".into()
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
                        body: format!(
                            "{} / {max}\nClick to focus. Double-click to isolate. Shift-click to remove.",
                            e.public.hp.unwrap_or(0)
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
        ui.set_tooltip(
            n.portrait,
            Some(Tooltip {
                title: d.name.clone(),
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
        let mut stats: Vec<(Option<String>, &str, String, String)> = Vec::new();
        if let Some(body) = &d.body {
            if !building {
                let combat = d.behaviors.combat.as_ref();
                let damage = e.combat.as_ref().map_or_else(
                    || combat.map_or(0.0, |c| f64::from(c.damage)),
                    |c| c.stats.damage_milli as f64 / 1000.0,
                );
                let tip = match combat {
                    Some(c) => format!(
                        "{}. {:.3}s between attacks. Damage shown before target armor and resistance.",
                        k.content
                            .rules
                            .damage_types
                            .get(c.damage_type.as_str())
                            .map_or(c.damage_type.as_str(), |t| t.name.as_str()),
                        f64::from(
                            e.combat
                                .as_ref()
                                .map_or(c.cooldown_ticks, |s| s.stats.cooldown_ticks)
                        ) * 0.025
                    ),
                    None => "This unit has no attack.".into(),
                };
                stats.push((
                    Some("icon.action.attack".into()),
                    "Damage",
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
            let mut tip = format!(
                "{}. Armor points reduce ordinary attack damage by {reduction:.1}%.",
                armor_type.map_or(body.armor_type.as_str(), |t| t.name.as_str())
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
                tip.push_str(&format!(
                    "\n{}: {}% class damage{}",
                    ty.name,
                    f64::from(mult) / 10.0,
                    if ty.applies_armor {
                        " before armor"
                    } else {
                        "; bypasses armor points"
                    }
                ));
            }
            stats.push((
                armor_type.map(|t| t.icon.to_string()),
                "Armor",
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
                ui.set_text(node, label);
            }
            if let Some(node) = ui.find_in(cell, "value") {
                ui.set_text(node, &value);
            }
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: label.into(),
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
                        d.item_tier.map_or(String::new(), |t| format!("Tier {t}. ")),
                        d.description,
                        charges.map_or(String::new(), |c| format!(" {c} charges remaining.")),
                        ready.map_or(String::new(), |s| format!(" Cooldown: {s}s.")),
                    ),
                    note: own.then(|| {
                        if usable {
                            "Click to use. Right-click to drop.".into()
                        } else {
                            "Equipped. Right-click to drop.".into()
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
            if let Some(research) = &w.research {
                for r in &research.queue {
                    let rule = k.content.rules.research.get(r.id.as_str());
                    self.queue.push(QueueEntry {
                        icon: rule.map_or(String::new(), |x| x.icon.to_string()),
                        name: rule.map_or(r.id.to_string(), |x| x.name.clone()),
                        progress: rule.map(|x| r.progress as f32 / x.work_ticks.max(1) as f32),
                        cancel: own.then(|| Action::CancelResearch {
                            actor: fid,
                            research: r.id.clone(),
                        }),
                    });
                }
            } else if let Some(p) = &w.production {
                for q in &p.queue {
                    let od = k.catalog.get(q.definition);
                    let active = p.active.as_ref().filter(|a| a.queue == Some(q.id));
                    let total = od.creation.as_ref().map_or(1, |c| match c {
                        canopy_content::game::Creation::Train { work_ticks, .. }
                        | canopy_content::game::Creation::Construct { work_ticks, .. }
                        | canopy_content::game::Creation::Harvest { work_ticks, .. }
                        | canopy_content::game::Creation::Plant { work_ticks, .. } => *work_ticks,
                    });
                    self.queue.push(QueueEntry {
                        icon: od.icon.to_string(),
                        name: od.name.clone(),
                        progress: active.map(|a| a.progress as f32 / total.max(1) as f32),
                        cancel: own.then_some(Action::Cancel {
                            actor: fid,
                            queue: Some(q.id),
                        }),
                    });
                }
            }
            if let (Some(revival), Some(policy)) = (&w.revival, &d.behaviors.revival) {
                for r in &revival.queue {
                    let hero = k.entity(r.hero.0).map(|h| k.def(h));
                    let total =
                        policy.work_ticks + policy.work_ticks_per_level * r.level.saturating_sub(1);
                    self.queue.push(QueueEntry {
                        icon: hero.map_or(String::new(), |h| h.icon.to_string()),
                        name: format!("Revive {}", hero.map_or("hero", |h| h.name.as_str())),
                        progress: Some(r.progress as f32 / total.max(1) as f32),
                        cancel: own.then_some(Action::CancelRevival {
                            actor: fid,
                            hero: r.hero.0,
                        }),
                    });
                }
            }
            // The active task: an upgrade, else the active entry, else the first.
            if let (Some(up), Some(rule)) = (&w.upgrade, &d.upgrade) {
                let target = k.content.definition(rule.target.as_str());
                self.active = Some(QueueEntry {
                    icon: target.map_or(String::new(), |t| t.icon.to_string()),
                    name: format!("Upgrading to {}", target.map_or("", |t| t.name.as_str())),
                    progress: Some(up.progress as f32 / rule.work_ticks.max(1) as f32),
                    cancel: own.then_some(Action::CancelUpgrade { actor: fid }),
                });
            } else if !self.queue.is_empty() {
                let i = self
                    .queue
                    .iter()
                    .position(|q| q.progress.is_some())
                    .unwrap_or(0);
                self.active = Some(self.queue.remove(i));
            }
            status = w.status.clone().filter(|s| !s.is_empty());
        }
        if let Some(site) = e.work.as_ref().and_then(|w| w.site) {
            status = Some(format!(
                "{} workers · {} recommended{}",
                site.workers,
                site.recommended,
                e.public
                    .resource
                    .as_ref()
                    .map_or(String::new(), |r| format!(" · {} remaining", r.amount))
            ));
        }
        if let Some(c) = e.public.construction {
            let total = d.creation.as_ref().map_or(1, |c| match c {
                canopy_content::game::Creation::Construct { work_ticks, .. } => *work_ticks,
                _ => 1,
            });
            self.active = Some(QueueEntry {
                icon: d.icon.to_string(),
                name: "Under construction".into(),
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
                    Some(p) => format!("{} · {}%", a.name, (p.clamp(0.0, 1.0) * 100.0).floor()),
                    None => format!("{} · waiting", a.name),
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
                    body: "Click to cancel and refund the reserved resources.".into(),
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
            let (verb, icon, target) = describe_order(k, order);
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
                    body: format!(
                        "Queued order {}. Runs after the current task. A normal order or Stop clears pending orders.",
                        i + 1
                    ),
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
                ItemStatusKind::Rescue => Some("Invulnerable".to_owned()),
                ItemStatusKind::Active => active
                    .and_then(|a| a.status.as_ref())
                    .map(|st| modifier_text(&st.modifiers)),
            };
            let mut parts: Vec<String> = modifiers.into_iter().filter(|m| !m.is_empty()).collect();
            if let Some(shield) = s.shield {
                parts.push(format!("{shield} shield remaining"));
            }
            parts.push(match (s.kind, aura) {
                (ItemStatusKind::Aura, Some(a)) => format!(
                    "While within {} tiles of the bearer. Identical auras do not stack.",
                    a.radius
                ),
                _ => format!(
                    "{}s remaining",
                    s.expires.saturating_sub(k.tick()).div_ceil(40)
                ),
            });
            let aura_suffix = if s.kind == ItemStatusKind::Aura {
                " aura"
            } else {
                ""
            };
            ui.set_tooltip(
                cell,
                Some(Tooltip {
                    title: format!("{}{aura_suffix}", d.name),
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
                ui.set_checked(cell, false);
                ui.set_enabled(cell, false);
                ui.set_tooltip(cell, None);
                continue;
            };
            if let Some(icon) = ui.find_in(cell, "icon") {
                let key = self.image(ui, k, c.icon.as_str());
                ui.set_image(icon, key);
            }
            if let Some(key) = ui.find_in(cell, "key") {
                ui.set_text(key, &c.chord.as_ref().map(Chord::label).unwrap_or_default());
            }
            ui.set_enabled(cell, c.enabled());
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

    fn tooltip(&mut self, ui: &mut Ui, k: &Knowledge, c: &Command) -> Tooltip {
        let costs = c
            .costs
            .iter()
            .map(|cost| {
                if cost.icon.as_str() == MANA {
                    Cost {
                        image: None,
                        amount: format!("{} mana", cost.amount),
                    }
                } else {
                    Cost {
                        image: self.image(ui, k, cost.icon.as_str()),
                        amount: cost.amount.to_string(),
                    }
                }
            })
            .collect();
        let mut body = vec![escape(&c.description)];
        if let Some(r) = &c.reason {
            body.push(format!("[color=#ff8a65]{}[/color]", escape(r)));
        }
        if c.actors.len() > 1 {
            body.push(format!("{} eligible selected actors", c.actors.len()));
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
            note: c
                .alternate
                .is_some()
                .then(|| "Right-click to toggle autocast.".into()),
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
                    title: format!("Control group {digit}"),
                    hotkey: (!recall.is_empty()).then_some(recall),
                    body: format!(
                        "{} members. {}: assign. {}: add selection. Double press or double click to focus.",
                        members.len(),
                        play.keymap.label(Shortcut::Assign(slot)),
                        play.keymap.label(Shortcut::Add(slot)),
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
        let observing = play.observer();
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
        let currencies: Vec<(String, String)> = colonies
            .first()
            .map(|(_, c)| {
                c.goods
                    .iter()
                    .map(|g| (g.item.to_string(), k.name_of(g.item.as_str())))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(goods) = ui.find_in(n.observer_columns, "goods") {
            let names: Vec<&str> = currencies.iter().map(|(_, name)| name.as_str()).collect();
            ui.set_text(goods, &names.join(" · "));
        }
        ui.set_count(n.observer_rows, colonies.len());
        for (i, (player, colony)) in colonies.iter().enumerate() {
            let Some(row) = ui.child(n.observer_rows, i) else {
                continue;
            };
            let owner = Owner::Player(*player);
            let color = self
                .palette
                .get(usize::from(*player))
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
            if let Some(cell) = ui.find_in(row, "player") {
                // Old row: the slot badge, the name and "Player N · controller".
                let (name, ai) = self
                    .players
                    .iter()
                    .find(|(slot, ..)| slot == player)
                    .map_or((format!("Player {}", player + 1), true), |(_, n, ai)| {
                        (n.clone(), *ai)
                    });
                let role = if defeated {
                    "Defeated"
                } else if ai {
                    "Computer"
                } else {
                    "Human"
                };
                ui.set_text(
                    cell,
                    &format!(
                        "[color=#{:02x}{:02x}{:02x}][b]{}[/b][/color]  {}\n[color=#93afb5]Player {} · {role}[/color]",
                        color[0],
                        color[1],
                        color[2],
                        player + 1,
                        escape(&name),
                        player + 1,
                    ),
                );
            }
            if let Some(cell) = ui.find_in(row, "goods") {
                let text: Vec<String> = colony
                    .goods
                    .iter()
                    .map(|g| {
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
                        format!("{} [color=#93afb5]+{per_minute}/min[/color]", g.stored)
                    })
                    .collect();
                ui.set_text(cell, &text.join("   "));
            }
            let units = k
                .view
                .entities
                .iter()
                .filter(|e| e.public.owner == owner && k.is_unit(e) && k.alive(e))
                .count();
            if let Some(cell) = ui.find_in(row, "units") {
                ui.set_text(cell, &units.to_string());
            }
            if let Some(cell) = ui.find_in(row, "workers") {
                ui.set_text(cell, &colony.population.workers.to_string());
            }
            // Every hero (old: one entry each, fallen ones marked).
            let heroes: Vec<String> = k
                .view
                .entities
                .iter()
                .filter(|e| e.public.owner == owner && k.def(e).is_hero())
                .map(|h| {
                    let level = h.combat.as_ref().map_or(1, |c| c.stats.level);
                    if k.view.battle.fallen.contains(&h.id()) {
                        format!("Lv. {level} Fallen")
                    } else {
                        format!("Lv. {level}")
                    }
                })
                .collect();
            if let Some(cell) = ui.find_in(row, "hero") {
                let text = if heroes.is_empty() {
                    "—".to_owned()
                } else {
                    heroes.join("\n")
                };
                ui.set_text(cell, &text);
            }
        }
        ui.set_text(
            n.observer_footer,
            &if sampled >= INCOME_TICKS {
                "Income / min · last 60s of game time".to_owned()
            } else {
                format!("Income / min · {}s of game time sampled", seconds.floor())
            },
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
                    let (x, z) = (at.x * self.map_size, at.y * self.map_size);
                    match button {
                        PointerButton::Secondary => play.minimap_command(x, z),
                        _ => events.push(Event::LookAt([x, z])),
                    }
                }
                ("minimap", ActionKind::Drag { at }) => {
                    events.push(Event::LookAt([at.x * self.map_size, at.y * self.map_size]));
                }
                ("menu", ActionKind::Click { .. }) => events.push(Event::OpenMenu),
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
            format!("Maximum level {reached}"),
            format!("{xp} total experience."),
        ));
    }
    let base = levels[reached - 1].experience;
    let next = levels[reached].experience;
    let fraction = (xp - base) as f32 / (next - base).max(1) as f32;
    Some((
        fraction,
        format!("Experience: {} / {}", xp - base, next - base),
        format!(
            "{xp} total experience. {} more to reach level {}.",
            next - xp,
            reached + 1
        ),
    ))
}

/// The verb, icon and target of a queued order (old `orderQueue.ts`).
fn describe_order(k: &Knowledge, order: &canopy_sim::orders::Order) -> (String, String, String) {
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
                target(*t).map_or("unknown".into(), |d| d.name.clone()),
            )
        }
        Order::Gather { target: t }
        | Order::Construct { target: t }
        | Order::Pickup { target: t } => {
            let d = target(*t);
            let verb = match order {
                Order::Gather { .. } => "Gather",
                Order::Construct { .. } => "Build",
                _ => "Pick up",
            };
            (
                verb.into(),
                d.map_or(String::new(), |d| d.icon.to_string()),
                d.map_or("unknown".into(), |d| d.name.clone()),
            )
        }
        Order::CastAbility { binding, .. } => ("Cast".into(), String::new(), binding.clone()),
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
            out.push(format!("{sign}{}{pct} {label}", trim(shown)));
        }
    };
    number(m.max_hp.map(f64::from), "maximum health", false);
    number(m.max_mana.map(f64::from), "maximum mana", false);
    number(m.damage.map(f64::from), "damage", false);
    number(m.armor.map(f64::from), "armor", false);
    number(m.health_regen_per_second, "health/sec", false);
    number(m.mana_regen_per_second, "mana/sec", false);
    number(m.damage_permille.map(f64::from), "attack damage", true);
    number(m.attack_speed_permille.map(f64::from), "attack speed", true);
    number(m.move_speed_permille.map(f64::from), "movement speed", true);
    number(
        m.cooldown_reduction_permille.map(f64::from),
        "ability cooldown reduction",
        true,
    );
    number(m.lifesteal_permille.map(f64::from), "lifesteal", true);
    number(
        m.melee_reflection_permille.map(f64::from),
        "meleeReflectionPermille",
        true,
    );
    for (on, text) in [
        (m.rooted, "Rooted: cannot move"),
        (m.control_immune, "Immune to roots and stuns"),
        (m.invulnerable, "Invulnerable"),
    ] {
        if on == Some(true) {
            out.push(text.to_owned());
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
