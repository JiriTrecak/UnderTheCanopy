//! The replay viewer's controls and statistics (D60, `layouts/replay.json`), over the
//! observer HUD: a bar with every player's colour, goods, supply and hero; a statistics
//! panel with tabs (resources, army, losses, production); and the transport above the HUD
//! dock: play and pause, the time, the timeline (click or drag to jump; the part the
//! background analysis has reached jumps fastest), speeds and whose vision shows. An out
//! of sync notice shows once playback leaves the recorded match.

use super::{in_match::Playing, ui_host::UiHost, widgets};
use crate::data::GameData;
use crate::replay::clock;
use crate::sim::{ColonyStats, Control, REPLAY_SPEEDS, ReplayLive};
use canopy_render::Gpu;
use canopy_ui::{Action, ActionKind, Color, Tooltip, Ui};

/// Ticks the arrow keys skip (10 s).
pub const SKIP_TICKS: u32 = 400;

/// The statistics panel's tabs.
pub const TABS: [&str; 4] = ["Resources", "Army", "Units lost", "Production"];

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayUi {
    pub details: bool,
    pub tab: usize,
    /// Where the timeline is being dragged, 0–1.
    pub drag: Option<f32>,
    /// The transport's pause, as asked.
    pub held: bool,
    pub speed: f32,
}

impl Default for ReplayUi {
    fn default() -> Self {
        Self {
            details: false,
            tab: 0,
            drag: None,
            held: false,
            speed: 1.0,
        }
    }
}

/// `1,234`.
fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plain(text: &str) -> String {
    text.replace('[', "[[")
}

impl ReplayUi {
    /// Fresh controls for a new replay.
    pub fn reset(&mut self) {
        *self = Self {
            details: self.details,
            tab: self.tab,
            ..Self::default()
        };
    }

    fn live(playing: &Playing) -> Option<ReplayLive> {
        playing.status().replay
    }

    /// Play or pause (Space); at the end, plays again from the start.
    pub fn toggle(&mut self, playing: &Playing) {
        let Some(r) = Self::live(playing) else {
            return;
        };
        if r.at_end {
            playing.game.sim().control(Control::Seek(r.start));
            self.held = false;
        } else {
            self.held = !self.held;
        }
        playing.game.sim().control(Control::Hold(self.held));
    }

    /// Skips `ticks` forward (or back when negative), from the time shown.
    pub fn skip(&mut self, playing: &Playing, ticks: i64) {
        let Some(r) = Self::live(playing) else {
            return;
        };
        let from = r.seeking.unwrap_or(r.tick);
        let to = (i64::from(from) + ticks).clamp(i64::from(r.start), i64::from(r.end)) as u32;
        playing.game.sim().control(Control::Seek(to));
    }

    /// The replay's UI actions (`replay.*`).
    pub fn act(&mut self, action: &Action, playing: &mut Playing) {
        let name = action.name.as_deref().unwrap_or("");
        let clicked = matches!(
            action.kind,
            ActionKind::Click { .. } | ActionKind::Hotkey { .. } | ActionKind::Toggled(_)
        );
        match name {
            "replay.play" if clicked => self.toggle(playing),
            "replay.details" if clicked => self.details = !self.details,
            "replay.tab" if clicked => {
                if let Some(i) = action.index {
                    self.tab = i.min(TABS.len() - 1);
                }
            }
            "replay.speed" if clicked => {
                if let Some(&speed) = action.index.and_then(|i| REPLAY_SPEEDS.get(i)) {
                    self.speed = speed;
                    playing.game.sim().control(Control::Speed(speed));
                }
            }
            "replay.vision" => {
                if let ActionKind::Selected(i) = action.kind {
                    let player = (i > 0)
                        .then(|| playing.names.get(i - 1).map(|(slot, _)| *slot))
                        .flatten();
                    playing.game.set_vision(player);
                }
            }
            "replay.scrub" => match action.kind {
                ActionKind::DragStart { at, .. } | ActionKind::Drag { at } => {
                    self.drag = Some(at.x.clamp(0.0, 1.0));
                }
                ActionKind::DragEnd { at } => {
                    self.drag = None;
                    if let Some(r) = Self::live(playing) {
                        let span = r.end.saturating_sub(r.start);
                        let to = r.start + (at.x.clamp(0.0, 1.0) * span as f32).round() as u32;
                        playing.game.sim().control(Control::Seek(to));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Fills the replay's controls and statistics; `view` is the window's logical size.
    pub fn show(
        &self,
        host: &mut UiHost,
        gpu: &Gpu,
        data: &GameData,
        playing: &Playing,
        view: [f32; 2],
        scale: f32,
    ) {
        let Some(r) = Self::live(playing) else {
            return;
        };
        let colonies = playing
            .newest()
            .and_then(|s| s.colonies.clone())
            .unwrap_or_default();
        self.chips(host, gpu, data, playing, &colonies);
        let ui = &mut host.ui;
        widgets::checked(ui, "replay/top/details", self.details);
        widgets::visible(ui, "replay/panel", self.details);
        if self.details {
            self.panel(ui, data, playing, &colonies, &r);
        }
        self.transport(ui, playing, &r, view, scale);
    }

    fn chips(
        &self,
        host: &mut UiHost,
        gpu: &Gpu,
        data: &GameData,
        playing: &Playing,
        colonies: &[ColonyStats],
    ) {
        if let Some(list) = host.ui.find("replay/top/chips") {
            host.ui.set_count(list, colonies.len());
        }
        for (i, row) in colonies.iter().enumerate() {
            let base = format!("replay/top/chips/{i}");
            // Icons first (they borrow the host to load images).
            let goods: Vec<_> = row
                .colony
                .goods
                .iter()
                .map(|g| {
                    let def = data.content.definition(g.item.as_str());
                    let icon = def
                        .and_then(|d| widgets::asset_image(data, d.icon.as_str()))
                        .and_then(|p| host.pack_image(gpu, &data.pack, &p));
                    (
                        icon,
                        g.stored,
                        def.map_or_else(|| g.item.to_string(), |d| d.name.clone()),
                    )
                })
                .collect();
            let hero = row.heroes.first().map(|h| {
                let d = data.content.definitions.get(usize::from(h.definition.0));
                let icon = d
                    .and_then(|d| widgets::asset_image(data, d.icon.as_str()))
                    .and_then(|p| host.pack_image(gpu, &data.pack, &p));
                (
                    icon,
                    h.level,
                    h.status,
                    d.map_or(String::new(), |d| d.name.clone()),
                )
            });
            let ui = &mut host.ui;
            let [cr, cg, cb] = playing.color_of(row.player);
            if let Some(n) = ui.find(&format!("{base}/stripe")) {
                ui.set_color(n, Some(Color::rgba(cr, cg, cb, 255)));
            }
            let defeated = playing.newest().is_some_and(|s| {
                s.view
                    .defeated
                    .contains(&canopy_content::placement::Owner::Player(row.player))
            });
            if let Some(n) = ui.find(&base) {
                ui.set_opacity(n, if defeated { 0.55 } else { 1.0 });
            }
            widgets::text(
                ui,
                &format!("{base}/name"),
                &plain(&playing.name_of(row.player)),
            );
            if let Some(list) = ui.find(&format!("{base}/goods")) {
                ui.set_count(list, goods.len());
            }
            for (j, (icon, stored, name)) in goods.iter().enumerate() {
                let g = format!("{base}/goods/{j}");
                if let Some(n) = ui.find(&format!("{g}/icon")) {
                    ui.set_image(n, *icon);
                }
                widgets::text(ui, &format!("{g}/amount"), &thousands(*stored));
                let income = row.income.get(j).copied().unwrap_or(0);
                if let Some(n) = ui.find(&g) {
                    ui.set_tooltip(
                        n,
                        Some(Tooltip {
                            title: name.clone(),
                            body: format!(
                                "{} stored · +{}/min delivered",
                                thousands(*stored),
                                thousands(income)
                            ),
                            ..Tooltip::default()
                        }),
                    );
                }
            }
            let s = &row.colony.supply;
            widgets::text(
                ui,
                &format!("{base}/supply"),
                &format!("{}/{}", s.committed, s.capacity),
            );
            widgets::visible(ui, &format!("{base}/hero"), hero.is_some());
            if let Some((icon, level, status, name)) = hero {
                if let Some(n) = ui.find(&format!("{base}/hero/icon")) {
                    ui.set_image(n, icon);
                }
                let state = match status {
                    crate::sim::HeroStatus::Alive => String::new(),
                    other => format!(" {}", other.name()),
                };
                widgets::text(
                    ui,
                    &format!("{base}/hero/level"),
                    &format!("Lv {level}{state}"),
                );
                if let Some(n) = ui.find(&format!("{base}/hero")) {
                    ui.set_tooltip(
                        n,
                        Some(Tooltip {
                            title: format!("{name} · Level {level}"),
                            body: format!(
                                "{} hero{} in all.",
                                row.heroes.len(),
                                if row.heroes.len() == 1 { "" } else { "es" }
                            ),
                            ..Tooltip::default()
                        }),
                    );
                }
            }
        }
    }

    fn panel(
        &self,
        ui: &mut Ui,
        data: &GameData,
        playing: &Playing,
        colonies: &[ColonyStats],
        r: &ReplayLive,
    ) {
        if let Some(tabs) = ui.find("replay/panel/tabs") {
            ui.set_count(tabs, TABS.len());
            for (i, label) in TABS.iter().enumerate() {
                if let Some(t) = ui.child(tabs, i) {
                    ui.set_text(t, label);
                    ui.set_checked(t, i == self.tab);
                }
            }
        }
        widgets::text(
            ui,
            "replay/panel/clock",
            &format!("{} of {}", clock(r.tick - r.start), clock(r.end - r.start)),
        );
        let goods: Vec<String> = colonies
            .first()
            .map(|c| {
                c.colony
                    .goods
                    .iter()
                    .map(|g| {
                        data.content
                            .definition(g.item.as_str())
                            .map_or_else(|| g.item.to_string(), |d| d.name.clone())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let wide = self.tab == 3;
        let (head, footer): (Vec<String>, &str) = match self.tab {
            0 => {
                let mut h: Vec<String> = goods.clone();
                h.extend(["Gathered".into(), "Supply".into(), "Workers".into()]);
                (
                    h,
                    "Stored, and in green the income: goods delivered to drop-offs over the last minute of match time. Gathered: every good delivered since the start.",
                )
            }
            1 => (
                [
                    "Army",
                    "Army value",
                    "Workers",
                    "Buildings",
                    "Heroes",
                    "Supply",
                ]
                .map(String::from)
                .to_vec(),
                "Army value: what the living army units cost to make, every good added up.",
            ),
            2 => (
                [
                    "Units lost",
                    "Buildings lost",
                    "Value lost",
                    "Units killed",
                    "Buildings killed",
                    "Value killed",
                ]
                .map(String::from)
                .to_vec(),
                "Value: what the units and buildings cost to make. A kill counts for the player whose blow was lethal.",
            ),
            _ => (
                Vec::new(),
                "What every player's buildings are making or researching now, the furthest along first.",
            ),
        };
        if let Some(cols) = ui.find("replay/panel/columns") {
            ui.set_count(cols, head.len());
            for (i, label) in head.iter().enumerate() {
                if let Some(c) = ui.child(cols, i) {
                    ui.set_text(c, &plain(label));
                }
            }
        }
        widgets::visible(ui, "replay/panel/wide-head", wide);
        widgets::visible(ui, "replay/panel/columns", !wide);
        widgets::text(ui, "replay/panel/wide-head", "In production");
        widgets::text(ui, "replay/panel/footer", footer);
        if let Some(rows) = ui.find("replay/panel/rows") {
            ui.set_count(rows, colonies.len());
        }
        for (i, row) in colonies.iter().enumerate() {
            let base = format!("replay/panel/rows/{i}");
            let [cr, cg, cb] = playing.color_of(row.player);
            if let Some(n) = ui.find(&format!("{base}/swatch")) {
                ui.set_color(n, Some(Color::rgba(cr, cg, cb, 255)));
            }
            widgets::text(
                ui,
                &format!("{base}/name"),
                &plain(&playing.name_of(row.player)),
            );
            let t = &row.tally;
            let cells: Vec<String> = match self.tab {
                0 => {
                    let mut c: Vec<String> = row
                        .colony
                        .goods
                        .iter()
                        .enumerate()
                        .map(|(j, g)| {
                            format!(
                                "{} [color=#a3bf8a]+{}[/color]",
                                thousands(g.stored),
                                thousands(row.income.get(j).copied().unwrap_or(0))
                            )
                        })
                        .collect();
                    c.push(thousands(t.gathered.values().sum()));
                    c.push(format!(
                        "{}/{}",
                        row.colony.supply.committed, row.colony.supply.capacity
                    ));
                    c.push(thousands(row.workers));
                    c
                }
                1 => vec![
                    thousands(row.army),
                    thousands(row.army_value),
                    thousands(row.workers),
                    thousands(row.buildings),
                    if row.heroes.is_empty() {
                        "—".into()
                    } else {
                        row.heroes
                            .iter()
                            .map(|h| format!("Lv {}", h.level))
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    format!(
                        "{}/{}",
                        row.colony.supply.committed, row.colony.supply.capacity
                    ),
                ],
                2 => vec![
                    thousands(t.units_lost),
                    thousands(t.buildings_lost),
                    thousands(t.value_lost),
                    thousands(t.units_killed),
                    thousands(t.buildings_killed),
                    thousands(t.value_killed),
                ],
                _ => Vec::new(),
            };
            if let Some(list) = ui.find(&format!("{base}/cells")) {
                ui.set_visible(list, !wide);
                ui.set_count(list, cells.len());
                for (j, text) in cells.iter().enumerate() {
                    if let Some(c) = ui.child(list, j) {
                        ui.set_text(c, text);
                    }
                }
            }
            widgets::visible(ui, &format!("{base}/wide"), wide);
            if wide {
                let text = if row.production.is_empty() {
                    "[color=#869591]Nothing in production[/color]".to_string()
                } else {
                    row.production
                        .iter()
                        .map(|p| {
                            let count = if p.count > 1 {
                                format!(" ×{}", p.count)
                            } else {
                                String::new()
                            };
                            let progress = p.progress.map_or(String::new(), |v| {
                                format!(" [color=#a3bf8a]{:.0}%[/color]", v * 100.0)
                            });
                            format!("{}{count}{progress}", plain(&p.name))
                        })
                        .collect::<Vec<_>>()
                        .join("  ·  ")
                };
                widgets::text(ui, &format!("{base}/wide"), &text);
            }
        }
    }

    fn transport(
        &self,
        ui: &mut Ui,
        playing: &Playing,
        r: &ReplayLive,
        view: [f32; 2],
        scale: f32,
    ) {
        let span = r.end.saturating_sub(r.start).max(1);
        let shown = r.seeking.unwrap_or(r.tick);
        let at = |tick: u32| (tick.saturating_sub(r.start) as f32 / span as f32).clamp(0.0, 1.0);
        let fraction = self.drag.unwrap_or(at(shown));
        let target = r.start + (fraction * span as f32).round() as u32;
        widgets::text(
            ui,
            "replay/dock/transport/play",
            if r.at_end {
                "Replay"
            } else if self.held {
                "Play"
            } else {
                "Pause"
            },
        );
        widgets::text(
            ui,
            "replay/dock/transport/time",
            &clock(target.saturating_sub(r.start)),
        );
        widgets::text(ui, "replay/dock/transport/end", &clock(span));
        if let Some(n) = ui.find("replay/dock/transport/timeline/played") {
            ui.set_value(n, fraction);
        }
        if let Some(n) = ui.find("replay/dock/transport/timeline/analysed") {
            ui.set_value(n, at(r.analysed));
        }
        let width = ui
            .find("replay/dock/transport/timeline")
            .and_then(|n| ui.rect(n))
            .map_or(0.0, |rect| rect.w / scale.max(0.1));
        if let Some(n) = ui.find("replay/dock/transport/timeline/thumb") {
            ui.set_shift(n, fraction * width - 3.0, 0.0);
        }
        widgets::visible(
            ui,
            "replay/dock/transport/timeline/desync-mark",
            r.desync.is_some(),
        );
        if let (Some(d), Some(n)) = (
            r.desync,
            ui.find("replay/dock/transport/timeline/desync-mark"),
        ) {
            ui.set_shift(n, at(d) * width - 1.0, 0.0);
        }
        if let Some(speeds) = ui.find("replay/dock/transport/speeds") {
            ui.set_count(speeds, REPLAY_SPEEDS.len());
            for (i, s) in REPLAY_SPEEDS.iter().enumerate() {
                if let Some(b) = ui.child(speeds, i) {
                    let label = if *s < 1.0 {
                        "½×".to_string()
                    } else {
                        format!("{s}×")
                    };
                    ui.set_text(b, &label);
                    ui.set_checked(b, (s - r.speed).abs() < 0.01);
                }
            }
        }
        let mut vision = vec!["Everyone".to_string()];
        vision.extend(playing.names.iter().map(|(_, n)| plain(n)));
        let selected = playing
            .game
            .vision()
            .and_then(|p| playing.names.iter().position(|(slot, _)| *slot == p))
            .map_or(0, |i| i + 1);
        widgets::options(ui, "replay/dock/transport/vision", &vision, selected);
        widgets::visible(ui, "replay/dock/desync", r.desync.is_some());
        if let Some(d) = r.desync {
            widgets::text(
                ui,
                "replay/dock/desync",
                &format!(
                    "This replay is out of sync from {}",
                    clock(d.saturating_sub(r.start))
                ),
            );
            if let Some(n) = ui.find("replay/dock/desync") {
                ui.set_tooltip(
                    n,
                    Some(Tooltip {
                        title: "Out of sync".into(),
                        body: "From here the match no longer plays as it was recorded (another game version, or a bug). It keeps playing; the log has the details.".into(),
                        ..Tooltip::default()
                    }),
                );
            }
        }
        // Above the HUD dock.
        let dock_top = ui
            .find("hud/dock")
            .and_then(|d| ui.rect(d))
            .filter(|rect| rect.h > 0.0)
            .map_or(view[1], |rect| rect.y / scale.max(0.1));
        if let Some(n) = ui.find("replay/dock") {
            ui.set_shift(n, 0.0, -(view[1] - dock_top + 10.0).max(10.0));
        }
    }
}
