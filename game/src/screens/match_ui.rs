//! What the screens draw over a running match: the mission HUD (objectives, dialogue
//! lines, cinematic letterbox), the match overlay (waiting for players, connection
//! problems), the result and the game menu. Chat is the play HUD's.

use super::{
    in_match::{MatchKind, Playing},
    ui_host::UiHost,
    widgets,
    widgets::Palette,
};
use crate::data::GameData;
use canopy_content::{game::Kind, map::Objective, placement::Owner};
use canopy_render::Gpu;
use canopy_sim::{mission::state::ObjectiveState, state::Outcome, view::MissionView};

/// Letterbox slide time (old .65 s).
const LETTERBOX_SECONDS: f32 = 0.65;
/// The dialogue's bottom edge over the view's height (old `bottom: 31%`), and the least
/// gap it keeps above the play HUD, logical pixels.
const DIALOGUE_BOTTOM: f32 = 0.31;
const DIALOGUE_GAP: f32 = 12.0;

/// The mission HUD's state between frames.
#[derive(Default)]
pub struct MissionHud {
    pub objectives: Vec<Objective>,
    letterbox: f32,
    /// The top of the play HUD's dock (logical pixels), which the dialogue keeps clear.
    pub hud_top: Option<f32>,
}

impl MissionHud {
    pub fn new(objectives: Vec<Objective>) -> Self {
        Self {
            objectives,
            letterbox: 0.0,
            hud_top: None,
        }
    }

    pub fn show(
        &mut self,
        host: &mut UiHost,
        gpu: &Gpu,
        data: &GameData,
        mission: Option<&MissionView>,
        dt: f32,
        view_height: f32,
    ) {
        let Some(m) = mission else {
            widgets::visible(&mut host.ui, "mission-hud/objectives", false);
            widgets::visible(&mut host.ui, "mission-hud/dialogue-wrap", false);
            widgets::visible(&mut host.ui, "mission-hud/top-bar", false);
            widgets::visible(&mut host.ui, "mission-hud/bottom-bar", false);
            return;
        };
        let cinematic = m.scene.is_some() || m.dialogue.as_ref().is_some_and(|d| d.cinematic);
        let target = if cinematic { 1.0 } else { 0.0 };
        let step = dt / LETTERBOX_SECONDS;
        self.letterbox = if self.letterbox < target {
            (self.letterbox + step).min(target)
        } else {
            (self.letterbox - step).max(target)
        };
        let bar = view_height * 0.07 * (1.0 - self.letterbox);
        let ui = &mut host.ui;
        widgets::visible(ui, "mission-hud/top-bar", self.letterbox > 0.0);
        widgets::visible(ui, "mission-hud/bottom-bar", self.letterbox > 0.0);
        if let Some(n) = ui.find("mission-hud/top-bar") {
            ui.set_shift(n, 0.0, -bar);
        }
        if let Some(n) = ui.find("mission-hud/bottom-bar") {
            ui.set_shift(n, 0.0, bar);
        }
        widgets::visible(ui, "mission-hud/objectives", !cinematic);
        widgets::text(
            ui,
            "mission-hud/objectives/caption",
            if m.error.is_some() {
                "Script error"
            } else {
                "Current objective"
            },
        );
        let current = m.error.clone().unwrap_or_else(|| m.objective.clone());
        widgets::text(ui, "mission-hud/objectives/objective", &current);
        let lines: Vec<String> = m
            .objectives
            .iter()
            .map(|(id, state)| {
                let objective = self.objectives.iter().find(|o| &o.id == id);
                let title = objective.map_or(id.as_str(), |o| o.title.as_str());
                let optional = if objective.is_some_and(|o| o.optional) {
                    "Optional: "
                } else {
                    ""
                };
                match state {
                    ObjectiveState::Active => format!("•  {optional}{title}"),
                    ObjectiveState::Completed => {
                        format!("[color=#9fd18b]✓[/color]  {optional}{title}")
                    }
                    ObjectiveState::Failed => {
                        format!("[color=#e28b7a]✕[/color]  {optional}{title}")
                    }
                }
            })
            .collect();
        if let Some(list) = ui.find("mission-hud/objectives/lines") {
            ui.set_count(list, lines.len());
            for (i, line) in lines.iter().enumerate() {
                if let Some(n) = ui.child(list, i) {
                    ui.set_text(n, line);
                }
            }
        }
        widgets::visible(ui, "mission-hud/dialogue-wrap", m.dialogue.is_some());
        // The dialogue sits at 31 % from the bottom (old), lifted clear of a larger HUD
        //
        let lift = self.hud_top.map_or(0.0, |top| {
            (view_height * (1.0 - DIALOGUE_BOTTOM) + DIALOGUE_GAP - top).max(0.0)
        });
        if let Some(n) = ui.find("mission-hud/dialogue-wrap") {
            ui.set_shift(n, 0.0, -lift);
        }
        if let Some(d) = &m.dialogue {
            widgets::text(ui, "mission-hud/dialogue-wrap/speaker", &d.speaker);
            widgets::text(ui, "mission-hud/dialogue-wrap/text", &d.text);
            let portrait = data
                .content
                .definition(d.portrait.as_str())
                .and_then(|def| widgets::asset_image(data, def.icon.as_str()))
                .and_then(|e| host.pack_image(gpu, &data.pack, &e));
            if let Some(n) = host.ui.find("mission-hud/dialogue-wrap/portrait") {
                host.ui.set_image(n, portrait);
            }
        }
    }
}

/// The overlay over a running match.
pub fn show_overlay(host: &mut UiHost, playing: &Playing, problem: Option<&(String, String)>) {
    let status = playing.status();
    let ui = &mut host.ui;
    let waiting = playing.networked
        && problem.is_none()
        && playing.outcome.is_none()
        && status
            .status
            .stalled
            .is_some_and(|s| s > super::in_match::STALL_NOTICE);
    let banner = if waiting {
        let seconds = status.status.stalled.map_or(0, |s| s.as_secs());
        Some(format!("Waiting for players…  {seconds} s"))
    } else {
        None
    };
    widgets::visible(ui, "match-overlay/banner", banner.is_some());
    if let Some(text) = banner {
        widgets::text(ui, "match-overlay/banner/text", &text);
    }
    widgets::visible(ui, "match-overlay/problem", problem.is_some());
    if let Some((title, body)) = problem {
        widgets::text(ui, "match-overlay/problem/title", title);
        widgets::text(ui, "match-overlay/problem/body", body);
    }
}

/// One player's line of the result.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultRow {
    pub slot: u8,
    pub name: String,
    pub result: String,
    pub units: u32,
    pub buildings: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResultModel {
    pub eyebrow: String,
    pub title: String,
    pub line: String,
    pub rows: Vec<ResultRow>,
    pub goods: String,
    /// The local player won (campaign progress).
    pub won: bool,
    /// The next mission's map id, after a won mission.
    pub next: Option<String>,
}

fn kind_name(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::Skirmish => "Skirmish",
        MatchKind::Campaign => "Campaign",
        MatchKind::Multiplayer => "Multiplayer",
    }
}

fn clock(ticks: u32) -> String {
    let seconds = ticks / 40;
    format!("{}:{:02}", seconds / 60, seconds % 60)
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

impl ResultModel {
    /// The result of a decided match (`save` gives the final counts), or of a surrender.
    pub fn build(
        playing: &Playing,
        data: &GameData,
        outcome: Option<&Outcome>,
        save: Option<&canopy_sim::Save>,
        surrendered: bool,
        next: Option<String>,
    ) -> Self {
        let me = playing.me.map(Owner::Player);
        let winner = outcome.and_then(|o| o.winner);
        let defeated = |o: Owner| outcome.is_some_and(|x| x.defeated.contains(&o));
        let teams = save.map(|s| canopy_sim::teams::Teams::new(&s.setup));
        let ally_of_winner = |o: Owner| match (winner, &teams) {
            (Some(w), Some(t)) => o == w || t.relation(o, w) == canopy_sim::teams::Relation::Ally,
            (Some(w), None) => o == w,
            (None, _) => false,
        };
        let campaign = playing.kind == super::in_match::MatchKind::Campaign;
        let (title, line, won) = if surrendered {
            (
                "[color=#fca5a5]Defeat[/color]".to_string(),
                "You surrendered.".to_string(),
                false,
            )
        } else if campaign && outcome.is_some() {
            // The old mission result.
            if me.is_some_and(ally_of_winner) {
                (
                    "[color=#fcd34d]Mission complete[/color]".into(),
                    if next.is_some() {
                        "Your surviving company is ready to continue. Items and learned abilities travel with you.".into()
                    } else {
                        "The Vanguard has completed its mission.".into()
                    },
                    true,
                )
            } else {
                (
                    "[color=#fca5a5]Mission failed[/color]".into(),
                    "The Vanguard could not complete its mission. Rally and try again.".into(),
                    false,
                )
            }
        } else if outcome.is_some() && winner.is_none() {
            // Old `matchResult`: no winner is a draw for everyone.
            (
                "Draw".into(),
                "The match ended without a winning side.".into(),
                false,
            )
        } else {
            match (me, winner) {
                (Some(me), Some(_)) if ally_of_winner(me) => (
                    "[color=#fcd34d]Victory[/color]".into(),
                    "Your side has won the match.".into(),
                    true,
                ),
                (Some(me), _) if defeated(me) || winner.is_some() => (
                    "[color=#fca5a5]Defeat[/color]".into(),
                    "Your side has been defeated.".into(),
                    false,
                ),
                (_, Some(Owner::Player(w))) => (
                    "Match complete".into(),
                    format!("{}'s side wins the match.", playing.name_of(w)),
                    false,
                ),
                _ => (
                    "Match complete".into(),
                    "Nobody won the match.".into(),
                    false,
                ),
            }
        };
        let tick = save.map_or(0, |s| s.state.tick);
        let mut rows: Vec<ResultRow> = playing
            .names
            .iter()
            .map(|(slot, name)| {
                let owner = Owner::Player(*slot);
                let (mut units, mut buildings) = (0, 0);
                if let Some(s) = save {
                    for e in s.state.entities.iter() {
                        if e.owner != owner || e.hp == Some(0) || e.fallen {
                            continue;
                        }
                        let kind = data
                            .content
                            .definitions
                            .get(usize::from(e.definition.0))
                            .map(|d| d.kind);
                        match kind {
                            Some(Kind::Unit) => units += 1,
                            Some(Kind::Building) => buildings += 1,
                            _ => {}
                        }
                    }
                }
                let result = if surrendered && Some(owner) == me {
                    "Surrendered"
                } else if outcome.is_none() {
                    "Playing"
                } else if ally_of_winner(owner) {
                    "Won"
                } else if defeated(owner) || winner.is_some() {
                    "Defeated"
                } else {
                    "—"
                };
                ResultRow {
                    slot: *slot,
                    name: if Some(owner) == me {
                        format!("{name} (you)")
                    } else {
                        name.clone()
                    },
                    result: result.into(),
                    units,
                    buildings,
                }
            })
            .collect();
        rows.sort_by_key(|r| r.slot);
        let goods = save
            .map(|s| {
                let list = |stock: &canopy_sim::entity::Stock| {
                    stock
                        .iter()
                        .filter(|(_, n)| **n > 0)
                        .map(|(id, n)| {
                            let name = data
                                .content
                                .definition(id.as_str())
                                .map_or_else(|| id.to_string(), |d| d.name.clone());
                            format!("{} {name}", thousands(*n))
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let a = &s.state.accounting;
                let mut parts = Vec::new();
                for (label, stock) in [
                    ("Gathered", &a.produced),
                    ("Spent", &a.consumed),
                    ("Lost", &a.lost),
                ] {
                    let text = list(stock);
                    if !text.is_empty() {
                        parts.push(format!("{label}: {text}."));
                    }
                }
                parts.join("  ")
            })
            .unwrap_or_default();
        Self {
            eyebrow: format!(
                "{} · {} · {}",
                kind_name(playing.kind),
                playing.map_name,
                if tick > 0 {
                    clock(tick)
                } else {
                    "Match complete".into()
                }
            ),
            title,
            line,
            rows,
            goods,
            won,
            next: next.filter(|_| won),
        }
    }

    pub fn show(&self, host: &mut UiHost, palette: &Palette, kind: MatchKind) {
        let ui = &mut host.ui;
        widgets::text(ui, "result/eyebrow", &self.eyebrow);
        widgets::text(ui, "result/title", &self.title);
        widgets::text(ui, "result/line", &self.line);
        if let Some(list) = ui.find("result/rows") {
            ui.set_count(list, self.rows.len());
        }
        for (i, row) in self.rows.iter().enumerate() {
            let base = format!("result/rows/{i}");
            if let Some(n) = ui.find(&format!("{base}/swatch")) {
                ui.set_color(n, Some(palette.solid(row.slot)));
            }
            widgets::text(ui, &format!("{base}/name"), &row.name);
            widgets::text(ui, &format!("{base}/result"), &row.result);
            widgets::text(ui, &format!("{base}/units"), &row.units.to_string());
            widgets::text(ui, &format!("{base}/buildings"), &row.buildings.to_string());
        }
        widgets::text(ui, "result/goods", &self.goods);
        widgets::visible(ui, "result/goods", !self.goods.is_empty());
        widgets::visible(ui, "result/next", self.next.is_some());
        widgets::text(
            ui,
            "result/leave",
            match kind {
                MatchKind::Campaign => "Return to campaign",
                MatchKind::Multiplayer => "Return to multiplayer",
                MatchKind::Skirmish => "Return to menu",
            },
        );
    }
}

/// Which page of the game menu shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuPage {
    #[default]
    Main,
    Save,
    /// Leave this scenario? (old exit page)
    Exit,
    Restart,
    Surrender,
}

pub fn show_game_menu(host: &mut UiHost, playing: &Playing, page: MenuPage, status: &str) {
    let ui = &mut host.ui;
    let local = !playing.networked;
    let decided = playing.outcome.is_some();
    let menu = format!("{} menu", kind_name(playing.kind));
    let (title, sub) = match page {
        MenuPage::Main | MenuPage::Save => (menu.as_str(), playing.map_name.as_str()),
        MenuPage::Exit if local && !decided => {
            ("Leave this scenario?", "Unsaved progress will be lost.")
        }
        MenuPage::Exit => ("Leave this match?", "The others play on without you."),
        MenuPage::Restart => ("Save before restarting?", "Unsaved progress will be lost."),
        MenuPage::Surrender if local => ("Surrender?", "The match ends as a defeat."),
        MenuPage::Surrender => (
            "Surrender?",
            "You leave the match as defeated; the others play on.",
        ),
    };
    widgets::text(ui, "game-menu/title", title);
    widgets::text(ui, "game-menu/sub", sub);
    for (id, shown) in [
        ("main", MenuPage::Main),
        ("save-page", MenuPage::Save),
        ("exit-page", MenuPage::Exit),
        ("restart-page", MenuPage::Restart),
        ("surrender-page", MenuPage::Surrender),
    ] {
        widgets::visible(ui, &format!("game-menu/{id}"), page == shown);
    }
    widgets::visible(ui, "game-menu/exit-page/save-exit", local && !decided);
    widgets::text(
        ui,
        "game-menu/exit-page/leave",
        if local && !decided {
            "Exit without saving"
        } else {
            "Leave the match"
        },
    );
    for id in ["save", "load", "restart"] {
        widgets::enabled(ui, &format!("game-menu/main/{id}"), local);
    }
    widgets::enabled(ui, "game-menu/main/save", local && !decided);
    widgets::enabled(
        ui,
        "game-menu/main/surrender",
        playing.me.is_some() && !decided,
    );
    widgets::text(ui, "game-menu/status", status);
    widgets::visible(ui, "game-menu/status", !status.is_empty());
}
