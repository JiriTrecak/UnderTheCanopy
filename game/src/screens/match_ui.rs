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
/// The letterbox bars over the view's height (`mh-bar-top`, `mh-bar-bottom`); the
/// bottom one holds the speaker's portrait and line, as Warcraft III's does.
const TOP_BAR: f32 = 0.10;
const BOTTOM_BAR: f32 = 0.21;
/// A skipped scene's jump hides under black that clears in this many ticks.
const SKIP_FADE_TICKS: f32 = 24.0;
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
    /// The finished objectives are listed (old `<details>`, closed until clicked).
    pub history_open: bool,
    /// The (interpolated) tick the picture shows, for fades.
    pub shown: f32,
    /// The speaker's live model is in the `@portrait` render target.
    pub live_speaker: bool,
    /// Autoplay, when the match is played by its mission's plan (docs/campaign.md
    /// "Autoplay").
    pub autoplay: Option<crate::sim::AutoplayLive>,
}

/// The objectives box as the old mission HUD built it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjectivesModel {
    /// "Objectives" or "Script error".
    pub caption: String,
    /// The script's objective line (or the error); `None` once a required objective is
    /// listed in its place.
    pub text: Option<String>,
    /// Active objectives: title (with "Optional: ") and description, in definition order.
    pub active: Vec<(String, String)>,
    /// Completed and failed objectives as "✓ title" / "✕ title".
    pub finished: Vec<String>,
    /// How many of `finished` were completed.
    pub completed: usize,
    /// The box shows at all: the script set an objective line (or failed).
    pub visible: bool,
}

impl ObjectivesModel {
    /// Old `MissionHud.update`: the line, then the active objectives with descriptions
    /// (a required one replaces the line), then the finished ones in a collapsed list.
    pub fn build(definitions: &[Objective], m: &MissionView) -> Self {
        let line = m.error.clone().unwrap_or_else(|| m.objective.clone());
        let mut model = Self {
            caption: if m.error.is_some() {
                tr!("match.script-error")
            } else {
                tr!("match.objectives")
            },
            visible: !line.is_empty(),
            text: Some(line),
            ..Self::default()
        };
        if m.error.is_some() {
            return model;
        }
        // Definition order first; ids the map does not define (none should) after.
        let mut shown: Vec<(&str, &str, &str, bool, ObjectiveState)> = definitions
            .iter()
            .filter_map(|o| {
                let state = *m.objectives.get(&o.id)?;
                Some((
                    o.id.as_str(),
                    o.title.as_str(),
                    o.description.as_str(),
                    o.optional,
                    state,
                ))
            })
            .collect();
        for (id, state) in &m.objectives {
            if !definitions.iter().any(|o| &o.id == id) {
                shown.push((id.as_str(), id.as_str(), "", false, *state));
            }
        }
        for &(_, title, description, optional, state) in &shown {
            match state {
                ObjectiveState::Active => {
                    let prefix = if optional {
                        tr!("match.optional-prefix")
                    } else {
                        String::new()
                    };
                    model
                        .active
                        .push((format!("{prefix}{title}"), description.to_owned()));
                    if !optional {
                        model.text = None;
                    }
                }
                ObjectiveState::Completed => {
                    model.completed += 1;
                    model.finished.push(format!("✓ {title}"));
                }
                ObjectiveState::Failed => model.finished.push(format!("✕ {title}")),
            }
        }
        model
    }
}

/// A speed as the autoplay line writes it: `4`, `0.5`.
fn speed_text(speed: f32) -> String {
    if speed.fract() == 0.0 {
        format!("{speed:.0}")
    } else {
        format!("{speed}")
    }
}

/// A cinematic holds the match: a scripted scene or a cinematic line still playing, while
/// the match is undecided and the script is running (old `MissionHud.active`).
pub fn cinematic(m: &MissionView, decided: bool) -> bool {
    let line = m
        .dialogue
        .as_ref()
        .is_some_and(|d| d.cinematic && d.remaining > 0);
    (m.scene.is_some() || line || m.cinema.on) && !decided && m.error.is_none()
}

/// The fade filter at `shown` (opacity 0 to 1, colour): the script's fade, black before
/// `on_start` has run, and a moment of black after a skip.
pub fn fade(m: &MissionView, shown: f32) -> (f32, [u8; 3]) {
    if !m.started {
        return (1.0, [0; 3]);
    }
    let (mut opacity, color) = m
        .cinema
        .fade
        .as_ref()
        .map_or((0.0, [0; 3]), |f| (f.opacity(shown), f.color));
    if let Some(at) = m.cinema.skipped {
        let since = shown - at as f32;
        if since >= 0.0 {
            opacity = opacity.max(1.0 - since / SKIP_FADE_TICKS);
        }
    }
    (opacity.clamp(0.0, 1.0), color)
}

/// Whether the dialogue line shows at `tick`: a cinematic line until its paused time
/// runs out, another until its end tick; never once the match is decided.
pub fn dialogue_visible(m: &MissionView, tick: u32, decided: bool) -> bool {
    m.dialogue.as_ref().is_some_and(|d| {
        !decided
            && if d.cinematic {
                d.remaining > 0
            } else {
                tick < d.until
            }
    })
}

impl MissionHud {
    pub fn new(objectives: Vec<Objective>) -> Self {
        Self {
            objectives,
            ..Self::default()
        }
    }

    /// Shows the mission's state at the view's `tick` (`decided`: the match has an
    /// outcome).
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        host: &mut UiHost,
        gpu: &Gpu,
        data: &GameData,
        mission: Option<&MissionView>,
        tick: u32,
        decided: bool,
        dt: f32,
        view_height: f32,
    ) {
        // The speaker's icon, for a line without a live portrait.
        let icon = mission
            .and_then(|m| m.dialogue.as_ref())
            .and_then(|d| data.content.definition(d.portrait.as_str()))
            .and_then(|def| widgets::asset_image(data, def.icon.as_str()))
            .and_then(|e| host.pack_image(gpu, &data.pack, &e));
        self.present(&mut host.ui, icon, mission, tick, decided, dt, view_height);
    }

    /// The autoplay line at the top: its speed (1× while a cinematic plays), its step, the
    /// speeds to pick and Take over. Gone once the player took over.
    fn show_autoplay(&self, ui: &mut canopy_ui::Ui) {
        let Some(a) = self.autoplay.as_ref().filter(|a| a.on) else {
            widgets::visible(ui, "mission-hud/autoplay", false);
            return;
        };
        widgets::visible(ui, "mission-hud/autoplay", true);
        let label = if a.running < a.speed {
            tr!("match.autoplay-scene", speed = &speed_text(a.running))
        } else {
            tr!("match.autoplay", speed = &speed_text(a.speed))
        };
        widgets::text(ui, "mission-hud/autoplay/label", &label);
        widgets::text(ui, "mission-hud/autoplay/step", &a.label);
        for speed in crate::sim::AUTOPLAY_SPEEDS {
            let text = format!("{}×", speed_text(speed));
            let text = if speed == a.speed {
                format!("[color=#f0e3c3]{text}[/color]")
            } else {
                text
            };
            widgets::text(
                ui,
                &format!("mission-hud/autoplay/speeds/s{}", speed_text(speed)),
                &text,
            );
        }
    }

    /// [`Self::show`] with the speaker's icon at hand (captures bind their own images).
    #[allow(clippy::too_many_arguments)]
    pub fn present(
        &mut self,
        ui: &mut canopy_ui::Ui,
        icon: Option<canopy_ui::ImageKey>,
        mission: Option<&MissionView>,
        tick: u32,
        decided: bool,
        dt: f32,
        view_height: f32,
    ) {
        self.show_autoplay(ui);
        let Some(m) = mission else {
            widgets::visible(ui, "mission-hud/objectives", false);
            widgets::visible(ui, "mission-hud/dialogue-wrap", false);
            widgets::visible(ui, "mission-hud/top-bar", false);
            widgets::visible(ui, "mission-hud/bottom-bar", false);
            widgets::visible(ui, "mission-hud/fade", false);
            return;
        };
        let cinematic = cinematic(m, decided);
        let target = if cinematic { 1.0 } else { 0.0 };
        let step = dt / LETTERBOX_SECONDS;
        self.letterbox = if self.letterbox < target {
            (self.letterbox + step).min(target)
        } else {
            (self.letterbox - step).max(target)
        };
        // The bars slide in eased, as the camera blends.
        let hidden = 1.0 - canopy_sim::mission::cinema::ease(self.letterbox);
        widgets::visible(ui, "mission-hud/top-bar", self.letterbox > 0.0);
        widgets::visible(ui, "mission-hud/bottom-bar", self.letterbox > 0.0);
        if let Some(n) = ui.find("mission-hud/top-bar") {
            ui.set_shift(n, 0.0, -view_height * TOP_BAR * hidden);
        }
        if let Some(n) = ui.find("mission-hud/bottom-bar") {
            ui.set_shift(n, 0.0, view_height * BOTTOM_BAR * hidden);
        }
        let skippable = m
            .cinema
            .scene
            .as_ref()
            .is_some_and(|s| s.skippable && !s.skipping);
        widgets::visible(ui, "mission-hud/top-bar/skip", cinematic && skippable);
        let (opacity, [r, g, b]) = fade(m, self.shown);
        widgets::visible(ui, "mission-hud/fade", opacity > 0.001 && !decided);
        if let Some(n) = ui.find("mission-hud/fade") {
            ui.set_opacity(n, opacity);
            ui.set_color(n, Some(canopy_ui::Color::rgba(r, g, b, 255)));
        }
        let model = ObjectivesModel::build(&self.objectives, m);
        widgets::visible(ui, "mission-hud/objectives", model.visible && !cinematic);
        widgets::text(ui, "mission-hud/objectives/caption", &model.caption);
        widgets::visible(ui, "mission-hud/objectives/objective", model.text.is_some());
        widgets::text(
            ui,
            "mission-hud/objectives/objective",
            model.text.as_deref().unwrap_or(""),
        );
        if let Some(list) = ui.find("mission-hud/objectives/active") {
            ui.set_count(list, model.active.len());
            for (i, (title, description)) in model.active.iter().enumerate() {
                let Some(n) = ui.child(list, i) else {
                    continue;
                };
                // Optional objectives are quieter: a muted mark and a muted "Optional".
                let prefix = tr!("match.optional-prefix");
                let optional = title.strip_prefix(prefix.as_str());
                if let Some(b) = ui.find_in(n, "bullet") {
                    ui.set_color(
                        b,
                        optional.map(|_| canopy_ui::Color::rgba(0x8f, 0xa0, 0x8b, 0xd0)),
                    );
                }
                if let Some(t) = ui.find_in(n, "copy/title") {
                    match optional {
                        Some(rest) => ui.set_text(t, &tr!("match.optional-title", title = rest)),
                        None => ui.set_text(t, title),
                    }
                }
                if let Some(d) = ui.find_in(n, "copy/description") {
                    ui.set_text(d, description);
                    ui.set_visible(d, !description.is_empty());
                }
            }
        }
        widgets::visible(
            ui,
            "mission-hud/objectives/history",
            !model.finished.is_empty(),
        );
        widgets::text(
            ui,
            "mission-hud/objectives/history/summary",
            &format!(
                "{} {}",
                if self.history_open { "▾" } else { "▸" },
                tr!("match.objectives-completed", count = model.completed)
            ),
        );
        widgets::visible(
            ui,
            "mission-hud/objectives/history/lines",
            self.history_open,
        );
        if let Some(list) = ui.find("mission-hud/objectives/history/lines") {
            ui.set_count(list, model.finished.len());
            for (i, line) in model.finished.iter().enumerate() {
                if let Some(n) = ui.child(list, i) {
                    ui.set_text(n, line);
                }
            }
        }
        let speaking = dialogue_visible(m, tick, decided);
        // In cinematic mode the line is a transmission in the lower bar.
        let transmission = speaking && m.cinema.on;
        widgets::visible(ui, "mission-hud/bottom-bar/transmission", transmission);
        widgets::visible(ui, "mission-hud/dialogue-wrap", speaking && !transmission);
        if let (Some(d), true) = (&m.dialogue, transmission) {
            widgets::text(
                ui,
                "mission-hud/bottom-bar/transmission/speaker",
                &d.speaker,
            );
            widgets::text(ui, "mission-hud/bottom-bar/transmission/text", &d.text);
            let portrait = if self.live_speaker {
                Some(ui.image_key("@portrait"))
            } else {
                icon
            };
            if let Some(n) = ui.find("mission-hud/bottom-bar/transmission/portrait") {
                ui.set_image(n, portrait);
            }
            return;
        }
        // The dialogue sits at 31 % from the bottom (old), lifted clear of a larger HUD
        //
        let lift = self.hud_top.map_or(0.0, |top| {
            (view_height * (1.0 - DIALOGUE_BOTTOM) + DIALOGUE_GAP - top).max(0.0)
        });
        if let Some(n) = ui.find("mission-hud/dialogue-wrap") {
            ui.set_shift(n, 0.0, -lift);
        }
        if let (Some(d), true) = (&m.dialogue, speaking) {
            widgets::text(ui, "mission-hud/dialogue-wrap/speaker", &d.speaker);
            widgets::text(ui, "mission-hud/dialogue-wrap/text", &d.text);
            if let Some(n) = ui.find("mission-hud/dialogue-wrap/portrait") {
                ui.set_image(n, icon);
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
        Some(tr!("match.waiting-for-players", seconds = seconds))
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
    /// A lost campaign mission offers Retry.
    pub retry: bool,
}

fn kind_name(kind: MatchKind) -> String {
    match kind {
        MatchKind::Skirmish => tr!("match.kind-skirmish"),
        MatchKind::Campaign => tr!("match.kind-campaign"),
        MatchKind::Multiplayer => tr!("match.kind-multiplayer"),
        MatchKind::Replay => tr!("match.kind-replay"),
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
                format!("[color=#fca5a5]{}[/color]", tr!("match.defeat")),
                tr!("match.you-surrendered"),
                false,
            )
        } else if campaign && outcome.is_some() {
            // The old mission result.
            if me.is_some_and(ally_of_winner) {
                (
                    format!("[color=#fcd34d]{}[/color]", tr!("hud.mission-complete")),
                    tr!("match.mission-won"),
                    true,
                )
            } else {
                (
                    format!("[color=#fca5a5]{}[/color]", tr!("hud.mission-failed")),
                    tr!("match.mission-lost"),
                    false,
                )
            }
        } else if outcome.is_some() && winner.is_none() {
            // Old `matchResult`: no winner is a draw for everyone.
            (tr!("match.draw"), tr!("match.draw-line"), false)
        } else {
            match (me, winner) {
                (Some(me), Some(_)) if ally_of_winner(me) => (
                    format!("[color=#fcd34d]{}[/color]", tr!("match.victory")),
                    tr!("match.victory-line"),
                    true,
                ),
                (Some(me), _) if defeated(me) || winner.is_some() => (
                    format!("[color=#fca5a5]{}[/color]", tr!("match.defeat")),
                    tr!("match.defeat-line"),
                    false,
                ),
                (_, Some(Owner::Player(w))) => (
                    tr!("match.complete"),
                    tr!("match.side-wins", name = &playing.name_of(w)),
                    false,
                ),
                _ => (tr!("match.complete"), tr!("match.nobody-won"), false),
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
                    tr!("match.result-surrendered")
                } else if outcome.is_none() {
                    tr!("match.result-playing")
                } else if ally_of_winner(owner) {
                    tr!("match.result-won")
                } else if defeated(owner) || winner.is_some() {
                    tr!("match.result-defeated")
                } else {
                    "—".into()
                };
                ResultRow {
                    slot: *slot,
                    name: if Some(owner) == me {
                        tr!("match.you", name = name)
                    } else {
                        name.clone()
                    },
                    result,
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
                for (key, stock) in [
                    ("match.goods-gathered", &a.produced),
                    ("match.goods-spent", &a.consumed),
                    ("match.goods-lost", &a.lost),
                ] {
                    let text = list(stock);
                    if !text.is_empty() {
                        parts.push(crate::text::tr_with(
                            key,
                            &[("goods", text.as_str().into())],
                        ));
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
                    tr!("match.complete")
                }
            ),
            title,
            line,
            rows,
            goods,
            won,
            retry: campaign && !won,
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
        widgets::visible(ui, "result/retry", self.retry);
        widgets::text(
            ui,
            "result/leave",
            &match kind {
                MatchKind::Campaign => tr!("match.return-campaign"),
                MatchKind::Multiplayer => tr!("match.return-multiplayer"),
                MatchKind::Skirmish => tr!("match.return-menu"),
                MatchKind::Replay => tr!("match.return-replays"),
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
    let menu = tr!("match.menu-title", kind = &kind_name(playing.kind));
    let watching = playing.watching.is_some();
    let replay_sub = playing.watching.as_ref().map(|w| {
        tr!(
            "match.replay-sub",
            name = &w.name.replace('[', "[["),
            time = &crate::saves::format_time(w.header.recorded_at),
            version = &w.header.game_version
        )
    });
    let (title, sub) = match page {
        MenuPage::Main if let Some(sub) = &replay_sub => (menu.clone(), sub.clone()),
        MenuPage::Main | MenuPage::Save => (menu.clone(), playing.map_name.clone()),
        MenuPage::Exit if watching => (tr!("match.leave-replay-q"), tr!("match.back-to-replays")),
        MenuPage::Exit if local && !decided => {
            (tr!("match.leave-scenario-q"), tr!("match.unsaved-lost"))
        }
        MenuPage::Exit => (tr!("match.leave-match-q"), tr!("match.others-play-on")),
        MenuPage::Restart => (
            tr!("match.save-before-restart-q"),
            tr!("match.unsaved-lost"),
        ),
        MenuPage::Surrender if local => (tr!("match.surrender-q"), tr!("match.surrender-defeat")),
        MenuPage::Surrender => (tr!("match.surrender-q"), tr!("match.surrender-leave")),
    };
    widgets::text(ui, "game-menu/title", &title);
    widgets::text(ui, "game-menu/sub", &sub);
    for (id, shown) in [
        ("main", MenuPage::Main),
        ("save-page", MenuPage::Save),
        ("exit-page", MenuPage::Exit),
        ("restart-page", MenuPage::Restart),
        ("surrender-page", MenuPage::Surrender),
    ] {
        widgets::visible(ui, &format!("game-menu/{id}"), page == shown);
    }
    widgets::visible(
        ui,
        "game-menu/exit-page/save-exit",
        local && !decided && !watching,
    );
    widgets::text(
        ui,
        "game-menu/exit-page/leave",
        &if watching {
            tr!("match.leave-replay")
        } else if local && !decided {
            tr!("match.exit-without-saving")
        } else {
            tr!("match.leave-match")
        },
    );
    for id in ["save", "load", "restart"] {
        widgets::enabled(ui, &format!("game-menu/main/{id}"), local && !watching);
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

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_sim::mission::state::Dialogue;
    use std::collections::BTreeMap;

    fn objective(id: &str, title: &str, description: &str, optional: bool) -> Objective {
        Objective {
            id: id.into(),
            title: title.into(),
            description: description.into(),
            optional,
        }
    }

    fn mission(states: &[(&str, ObjectiveState)]) -> MissionView {
        MissionView {
            objective: "Hold the ford".into(),
            objectives: states
                .iter()
                .map(|(id, s)| (id.to_string(), *s))
                .collect::<BTreeMap<_, _>>(),
            dialogue: None,
            scene: None,
            paused: false,
            started: true,
            cinema: Default::default(),
            animations: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn objectives_read_like_the_old_mission_hud() {
        let defs = [
            objective("z-gather", "Gather amber", "200 of it.", false),
            objective("a-scouts", "Find the scouts", "North of the river.", false),
            objective("hollow", "Clear the hollow", "", true),
            objective("bridge", "Keep the bridge", "", false),
        ];
        // Only the line until objectives are given.
        let m = mission(&[]);
        let model = ObjectivesModel::build(&defs, &m);
        assert_eq!(model.text.as_deref(), Some("Hold the ford"));
        assert!(model.visible && model.active.is_empty() && model.finished.is_empty());

        // An optional objective keeps the line; definition order, not id order.
        let m = mission(&[
            ("hollow", ObjectiveState::Active),
            ("z-gather", ObjectiveState::Completed),
            ("bridge", ObjectiveState::Failed),
        ]);
        let model = ObjectivesModel::build(&defs, &m);
        assert_eq!(model.text.as_deref(), Some("Hold the ford"));
        assert_eq!(
            model.active,
            [("Optional: Clear the hollow".into(), String::new())]
        );
        assert_eq!(model.finished, ["✓ Gather amber", "✕ Keep the bridge"]);
        assert_eq!(model.completed, 1);

        // A required active objective replaces the line with its title and description.
        let m = mission(&[
            ("a-scouts", ObjectiveState::Active),
            ("z-gather", ObjectiveState::Completed),
        ]);
        let model = ObjectivesModel::build(&defs, &m);
        assert_eq!(model.text, None);
        assert_eq!(
            model.active,
            [("Find the scouts".into(), "North of the river.".into())]
        );

        // A script error shows alone.
        let mut m = mission(&[("a-scouts", ObjectiveState::Active)]);
        m.error = Some("attempt to index a nil value".into());
        let model = ObjectivesModel::build(&defs, &m);
        assert_eq!(model.caption, "Script error");
        assert_eq!(model.text.as_deref(), Some("attempt to index a nil value"));
        assert!(model.active.is_empty());
        // No line: no box.
        let mut m = mission(&[]);
        m.objective.clear();
        assert!(!ObjectivesModel::build(&defs, &m).visible);
    }

    #[test]
    fn lines_end_and_cinematics_hold_only_while_they_play() {
        let mut m = mission(&[]);
        m.dialogue = Some(Dialogue {
            id: 1,
            actor: None,
            speaker: "Marshal".into(),
            portrait: canopy_content::Id::new("unit.test.hero").unwrap(),
            text: "Forward.".into(),
            duration_ticks: 100,
            cinematic: false,
            remaining: 0,
            until: 400,
        });
        assert!(dialogue_visible(&m, 399, false));
        assert!(!dialogue_visible(&m, 400, false), "the line has ended");
        assert!(!dialogue_visible(&m, 10, true), "a decided match hides it");
        assert!(!cinematic(&m, false));
        let d = m.dialogue.as_mut().unwrap();
        d.cinematic = true;
        d.remaining = 20;
        assert!(dialogue_visible(&m, 999, false) && cinematic(&m, false));
        m.dialogue.as_mut().unwrap().remaining = 0;
        assert!(!dialogue_visible(&m, 0, false) && !cinematic(&m, false));
        m.scene = Some(canopy_sim::mission::state::Scene {
            position: canopy_sim::entity::Milli { x: 0, y: 0 },
            camera: None,
        });
        assert!(cinematic(&m, false));
        assert!(!cinematic(&m, true));
    }
}
