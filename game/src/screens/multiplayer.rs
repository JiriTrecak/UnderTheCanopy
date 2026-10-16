//! Multiplayer (old `src/ui/menu/multiplayer.ts`): the match server's status, its
//! lobbies, creating and joining one, and the room before its match, through
//! `canopy-net` (D32). Lobby calls are polled, never awaited; the room's socket session
//! is polled every frame while the room waits and moves into the match when the server
//! starts it.
//!
//! The protocol has no lobby chat or ready flags (chat flows only while a match plays),
//! so the room shows seats, races, heroes and measured latency, and the host starts.

use super::{ui_host::UiHost, widgets, widgets::Palette};
use crate::data::{GameData, MapEntry};
use canopy_net::{
    Event, Net, Pending, Phase, ServerAddress, Session,
    wire::{
        CreateRoom, Health, JoinRoom, Joined, MatchConfig, Role, RoomState, RoomView, Seat,
        SeatKind,
    },
};
use canopy_render::Gpu;
use std::time::{Duration, Instant};

/// Rooms refresh this often while the list shows.
const REFRESH: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Find,
    Create,
}

/// What the multiplayer screens ask of the app.
pub enum Outcome {
    /// Joined a room: show the lobby.
    EnterLobby,
    /// The server started the match: load it with this seat.
    Start {
        config: MatchConfig,
        session: Box<Session>,
    },
    /// The room is gone (closed socket, an error before the start).
    LeftLobby,
}

pub struct Multiplayer {
    net: Option<Net>,
    pub server: String,
    health: Option<Pending<Health>>,
    pub health_text: String,
    pub reachable: Option<bool>,
    rooms_pending: Option<Pending<Vec<RoomView>>>,
    pub rooms: Vec<RoomView>,
    pub rooms_loaded: bool,
    last_refresh: Option<Instant>,
    pub tab: Tab,
    pub selected_room: Option<usize>,
    /// Index into the skirmish map list (hosting).
    pub selected_map: usize,
    /// Seats to open when hosting (2 + index).
    pub seats: usize,
    joining: Option<Pending<Joined>>,
    pub session: Option<Session>,
    pub status: String,
}

impl Default for Multiplayer {
    fn default() -> Self {
        Self {
            net: None,
            server: String::new(),
            health: None,
            health_text: String::new(),
            reachable: None,
            rooms_pending: None,
            rooms: Vec::new(),
            rooms_loaded: false,
            last_refresh: None,
            tab: Tab::Find,
            selected_room: None,
            selected_map: 0,
            seats: 0,
            joining: None,
            session: None,
            status: String::new(),
        }
    }
}

/// A short form of a map's identity (its package digest) for `mapRevision`.
pub fn map_revision(bytes: &[u8]) -> String {
    crate::saves::digest(bytes)[..16].to_string()
}

impl Multiplayer {
    /// Connects (or reconnects after a server change) and asks for status and rooms.
    pub fn open(&mut self, setting: Option<&str>) {
        let address = match ServerAddress::resolve(setting) {
            Ok(a) => a,
            Err(e) => {
                self.net = None;
                self.status = format!("Bad server address: {e}");
                return;
            }
        };
        if self.net.as_ref().is_none_or(|n| n.server() != &address) {
            self.net = match Net::new(address.clone()) {
                Ok(net) => Some(net),
                Err(e) => {
                    self.status = format!("Can't start networking: {e}");
                    None
                }
            };
            self.rooms.clear();
            self.rooms_loaded = false;
        }
        self.server = address.http("").trim_end_matches('/').to_string();
        self.status.clear();
        self.refresh();
    }

    pub fn refresh(&mut self) {
        let Some(net) = &self.net else {
            return;
        };
        self.health = Some(net.health());
        self.rooms_pending = Some(net.rooms());
        self.last_refresh = Some(Instant::now());
    }

    /// Polls replies (every frame while a multiplayer screen shows).
    pub fn poll(&mut self, on_list: bool) -> Option<Outcome> {
        if let Some(reply) = self.health.as_ref().and_then(Pending::poll) {
            self.health = None;
            match reply {
                Ok(h) if h.ok => {
                    self.reachable = Some(true);
                    self.health_text = format!("MatchHost {} · online", h.version);
                }
                Ok(_) => {
                    self.reachable = Some(false);
                    self.health_text = "MatchHost is unhealthy".into();
                }
                Err(e) => {
                    self.reachable = Some(false);
                    self.health_text = "Can't reach MatchHost".into();
                    tracing::warn!(error = %e, "match server health");
                }
            }
        }
        if let Some(reply) = self.rooms_pending.as_ref().and_then(Pending::poll) {
            self.rooms_pending = None;
            match reply {
                Ok(mut rooms) => {
                    rooms.retain(|r| matches!(r.state, RoomState::Waiting | RoomState::Playing));
                    let selected = self
                        .selected_room
                        .and_then(|i| self.rooms.get(i))
                        .map(|r| r.id.clone());
                    self.rooms = rooms;
                    self.selected_room = selected
                        .and_then(|id| self.rooms.iter().position(|r| r.id == id))
                        .or((!self.rooms.is_empty()).then_some(0));
                    self.rooms_loaded = true;
                }
                Err(e) => {
                    self.rooms_loaded = true;
                    self.status = format!("Can't reach MatchHost: {e}");
                }
            }
        }
        if on_list
            && self.session.is_none()
            && self.last_refresh.is_some_and(|t| t.elapsed() > REFRESH)
        {
            self.refresh();
        }
        if let Some(reply) = self.joining.as_ref().and_then(Pending::poll) {
            self.joining = None;
            match reply {
                Ok(joined) => {
                    let Some(net) = &self.net else {
                        return None;
                    };
                    self.session = Some(net.enter(&joined));
                    self.status.clear();
                    return Some(Outcome::EnterLobby);
                }
                Err(e) => self.status = e.to_string(),
            }
        }
        let session = self.session.as_mut()?;
        let mut left = false;
        let mut started = None;
        for event in session.poll() {
            match event {
                Event::Started { config, .. } => started = Some(config),
                Event::Error { code, message } => {
                    self.status = if message.is_empty() { code } else { message };
                }
                Event::Stopped { reason } => {
                    self.status = reason;
                    left = true;
                }
                Event::Closed { reason } => {
                    self.status = if reason.is_empty() {
                        "The lobby closed.".into()
                    } else {
                        format!("The lobby closed: {reason}")
                    };
                    left = true;
                }
                _ => {}
            }
        }
        if let Some(config) = started {
            let session = self.session.take()?;
            return Some(Outcome::Start {
                config,
                session: Box::new(session),
            });
        }
        if left || matches!(session.phase(), Phase::Closed | Phase::Stopped) {
            self.session = None;
            return Some(Outcome::LeftLobby);
        }
        None
    }

    pub fn busy(&self) -> bool {
        self.joining.is_some()
    }

    /// Joins the selected room or creates one on `maps[selected_map]`, as the tab says.
    pub fn go(&mut self, maps: &[&MapEntry], name: &str) {
        let Some(net) = &self.net else {
            self.status = "Not connected to a match server.".into();
            return;
        };
        if name.trim().is_empty() {
            self.status = "Enter your player name first.".into();
            return;
        }
        match self.tab {
            Tab::Find => {
                let Some(room) = self.selected_room.and_then(|i| self.rooms.get(i)) else {
                    return;
                };
                self.joining = Some(net.join_room(
                    &room.id,
                    &JoinRoom {
                        guest_name: name.into(),
                        role: Role::Player,
                    },
                ));
            }
            Tab::Create => {
                let Some(map) = maps.get(self.selected_map) else {
                    return;
                };
                let bytes = map.identity().unwrap_or_default();
                let seats = (2 + self.seats).min(map.starts.len()).max(2) as u32;
                self.joining = Some(net.create_room(&CreateRoom {
                    name: format!("{name}'s lobby"),
                    map_id: map.id().into(),
                    map_revision: map_revision(&bytes),
                    slot_count: seats,
                    guest_name: name.into(),
                }));
            }
        }
        self.status = "Contacting MatchHost…".into();
    }

    /// Leaves the room (old Back).
    pub fn leave(&mut self) {
        if let Some(session) = self.session.take() {
            session.leave();
        }
    }

    pub fn room(&self) -> Option<&RoomView> {
        self.session.as_ref().and_then(Session::room)
    }

    pub fn is_host(&self) -> bool {
        self.session.as_ref().and_then(Session::player) == Some(0)
    }

    /// Discovery screen.
    pub fn show_list(
        &self,
        host: &mut UiHost,
        gpu: &Gpu,
        data: &GameData,
        palette: &Palette,
        name_focus: bool,
    ) {
        let creating = self.tab == Tab::Create;
        let ui = &mut host.ui;
        widgets::checked(ui, "multiplayer/find", !creating);
        widgets::checked(ui, "multiplayer/create", creating);
        widgets::text(
            ui,
            "multiplayer/list-title",
            if creating { "Battlefields" } else { "Lobbies" },
        );
        widgets::visible(ui, "multiplayer/rooms", !creating);
        widgets::visible(ui, "multiplayer/maps", creating);
        widgets::visible(ui, "multiplayer/refresh", !creating);
        widgets::visible(ui, "multiplayer/slots", creating);
        widgets::visible(ui, "multiplayer/slots-caption", creating);
        widgets::visible(ui, "multiplayer/roster", !creating);
        widgets::visible(ui, "multiplayer/roster-caption", !creating);
        let _ = name_focus;
        let maps: Vec<&MapEntry> = data.skirmish_maps().collect();
        let (map, hint, can_go): (Option<&MapEntry>, String, bool) = if creating {
            let map = maps.get(self.selected_map).copied();
            widgets::map_list(
                host,
                gpu,
                data,
                "multiplayer/maps",
                &maps,
                Some(self.selected_map),
            );
            let n = map.map_or(2, |m| m.starts.len().clamp(2, 8));
            let options: Vec<String> = (2..=n).map(|k| k.to_string()).collect();
            widgets::options(&mut host.ui, "multiplayer/slots", &options, self.seats);
            let ok = map.is_some_and(|m| m.starts.len() >= 2);
            (
                map,
                if ok {
                    "Host a lobby on this battlefield; others join from Find a lobby.".into()
                } else {
                    "This battlefield needs at least two starting positions.".into()
                },
                ok,
            )
        } else {
            let room = self.selected_room.and_then(|i| self.rooms.get(i));
            if let Some(list) = host.ui.find("multiplayer/rooms") {
                host.ui.set_count(list, self.rooms.len());
            }
            for (i, r) in self.rooms.iter().enumerate() {
                let base = format!("multiplayer/rooms/{i}");
                let taken = r.slots.iter().filter(|s| s.name.is_some()).count();
                let total = r.slots.len();
                let name = if r.name.is_empty() {
                    format!("{}'s lobby", r.host)
                } else {
                    r.name.clone()
                };
                let map_name = data
                    .map(&r.map_id)
                    .map_or(r.map_id.as_str(), |m| m.name.as_str());
                let state = match r.state {
                    RoomState::Playing => format!("In game · {taken}/{total}"),
                    _ if taken >= total => format!("Full · {taken}/{total}"),
                    _ => format!("Open · {taken}/{total}"),
                };
                widgets::text(&mut host.ui, &format!("{base}/name"), &name);
                widgets::text(&mut host.ui, &format!("{base}/map"), map_name);
                widgets::text(&mut host.ui, &format!("{base}/state"), &state);
                widgets::checked(&mut host.ui, &base, self.selected_room == Some(i));
            }
            let empty = if !self.rooms_loaded {
                "Looking for lobbies…"
            } else if self.rooms.is_empty() {
                "No lobbies yet. Create one and invite your friends to join."
            } else {
                ""
            };
            widgets::text(&mut host.ui, "multiplayer/empty", empty);
            widgets::visible(&mut host.ui, "multiplayer/empty", !empty.is_empty());
            let map = room.and_then(|r| data.map(&r.map_id));
            if let Some(list) = host.ui.find("multiplayer/roster") {
                host.ui.set_count(list, room.map_or(0, |r| r.slots.len()));
            }
            if let Some(r) = room {
                for (i, seat) in r.slots.iter().enumerate() {
                    let base = format!("multiplayer/roster/{i}");
                    if let Some(badge) = host.ui.find(&format!("{base}/badge")) {
                        host.ui.set_text(badge, &(seat.player + 1).to_string());
                        host.ui.set_color(badge, Some(palette.badge(seat.player)));
                    }
                    widgets::text(
                        &mut host.ui,
                        &format!("{base}/name"),
                        seat.name.as_deref().unwrap_or("Open slot"),
                    );
                    widgets::text(
                        &mut host.ui,
                        &format!("{base}/sub"),
                        if seat.name.is_some() {
                            "Joined"
                        } else {
                            "Waiting for a player"
                        },
                    );
                }
            }
            let free = room.is_some_and(|r| r.slots.iter().any(|s| s.name.is_none()));
            let (hint, ok) = match room {
                None => (String::new(), false),
                Some(r) if r.state == RoomState::Playing => {
                    ("This match is already underway.".to_string(), false)
                }
                Some(_) if map.is_none() => (
                    "This battlefield is not available in your game.".to_string(),
                    false,
                ),
                Some(_) if !free => (
                    "This lobby is full. Try another or create your own.".to_string(),
                    false,
                ),
                Some(r) => (
                    format!(
                        "Hosted by {}. Join an open player slot to enter the lobby.",
                        r.host
                    ),
                    true,
                ),
            };
            (map, hint, ok)
        };
        widgets::map_preview(host, gpu, data, palette, "multiplayer", map, None);
        let ui = &mut host.ui;
        widgets::text(ui, "multiplayer/hint", &hint);
        widgets::text(ui, "multiplayer/status", &self.status);
        widgets::visible(ui, "multiplayer/status", !self.status.is_empty());
        let server = match self.reachable {
            Some(true) => format!("{} · {}", self.health_text, self.server),
            Some(false) => format!("{} · {}", self.health_text, self.server),
            None => format!("Contacting {}…", self.server),
        };
        widgets::text(ui, "multiplayer/server", &server);
        let open = self
            .rooms
            .iter()
            .filter(|r| r.state == RoomState::Waiting)
            .count();
        widgets::text(
            ui,
            "multiplayer/footnote",
            &if creating {
                "Online match · Shared battlefields".to_string()
            } else {
                format!("{open} open lobbies · Refresh to find new matches")
            },
        );
        widgets::text(
            ui,
            "multiplayer/go",
            if creating {
                "Create lobby"
            } else {
                "Join lobby"
            },
        );
        widgets::enabled(
            ui,
            "multiplayer/go",
            can_go && self.net.is_some() && !self.busy(),
        );
    }

    /// The room before its match.
    pub fn show_lobby(&self, host: &mut UiHost, gpu: &Gpu, data: &GameData, palette: &Palette) {
        let Some(session) = &self.session else {
            return;
        };
        let room = self.room();
        let me = session.player();
        let host_seat = self.is_host();
        let map = room.and_then(|r| data.map(&r.map_id));
        widgets::map_preview(host, gpu, data, palette, "lobby", map, me);
        // Markers follow the seats: their colours, and none where nobody will play.
        if let (Some(r), Some(m), Some(list)) = (room, map, host.ui.find("lobby/preview/markers")) {
            for (i, &player) in m.starts.iter().enumerate() {
                let Some(node) = host.ui.child(list, i) else {
                    continue;
                };
                let seat = r.slots.iter().find(|s| s.player + 1 == player);
                let plays =
                    seat.is_some_and(|s| matches!(seat_kind(s), SeatKind::Human | SeatKind::Ai));
                if let Some(s) = seat.filter(|_| plays) {
                    let color = s.color.unwrap_or(s.player);
                    let shade = if me == Some(s.player) {
                        palette.solid(color)
                    } else {
                        palette.badge(color)
                    };
                    host.ui.set_color(node, Some(shade));
                }
                host.ui.set_visible(
                    node,
                    plays || seat.is_some_and(|s| seat_kind(s) == SeatKind::Open),
                );
            }
        }
        let ui = &mut host.ui;
        widgets::text(
            ui,
            "lobby/map",
            map.map_or_else(
                || room.map_or("", |r| r.map_id.as_str()),
                |m| m.name.as_str(),
            ),
        );
        if let Some(r) = room {
            let count = |kind: SeatKind| r.slots.iter().filter(|s| seat_kind(s) == kind).count();
            let (humans, computers, open) = (
                count(SeatKind::Human),
                count(SeatKind::Ai),
                count(SeatKind::Open),
            );
            widgets::text(ui, "lobby/name", &r.name);
            widgets::text(ui, "lobby/host", &format!("Hosted by {}", r.host));
            let plural = |n: usize, one: &str, many: &str| {
                format!("{n} {}", if n == 1 { one } else { many })
            };
            let mut parts = vec![plural(humans, "player", "players")];
            if computers > 0 {
                parts.push(plural(computers, "computer", "computers"));
            }
            parts.push(format!("{open} open"));
            widgets::text(
                ui,
                "lobby/room-meta",
                &format!(
                    "{} · Input buffer {} ms",
                    parts.join(" · "),
                    r.input_delay_ms.unwrap_or(0)
                ),
            );
        }
        widgets::text(
            ui,
            "lobby/badge",
            if host_seat {
                "You are the host"
            } else {
                "You have joined"
            },
        );
        widgets::text(
            ui,
            "lobby/copy",
            if host_seat {
                "Set each seat open, closed or to the computer, and the teams. Start whenever you are ready: open seats nobody took stay empty."
            } else {
                "Choose your race, hero, team and colour. The host starts the match."
            },
        );
        let races = widgets::races(data);
        let race_names: Vec<String> = races.iter().map(|r| r.1.clone()).collect();
        let seats = room.map_or(&[][..], |r| &r.slots[..]);
        if let Some(list) = ui.find("lobby/seats") {
            ui.set_count(list, seats.len());
        }
        let waiting = room.is_some_and(|r| r.state == RoomState::Waiting);
        let teams: Vec<String> = (1..=TEAMS).map(|t| format!("Team {t}")).collect();
        for (i, seat) in seats.iter().enumerate() {
            let base = format!("lobby/seats/{i}");
            let kind = seat_kind(seat);
            let mine = me == Some(seat.player);
            let plays = matches!(kind, SeatKind::Human | SeatKind::Ai);
            // Your own seat, or a computer's when you host.
            let editable = waiting && (mine || (host_seat && kind == SeatKind::Ai));
            let color = seat.color.unwrap_or(seat.player);
            if let Some(badge) = ui.find(&format!("{base}/badge")) {
                ui.set_text(badge, &(seat.player + 1).to_string());
                ui.set_color(badge, Some(palette.badge(color)));
            }
            let (name, sub) = match kind {
                SeatKind::Human => {
                    let n = seat.name.clone().unwrap_or_default();
                    (
                        if mine { format!("{n} (you)") } else { n },
                        seat.round_trip_ms.map_or_else(
                            || "Connected".to_string(),
                            |ms| format!("Connected · {ms} ms RTT"),
                        ),
                    )
                }
                SeatKind::Ai => ("Computer".into(), "Played by the computer".into()),
                SeatKind::Open => ("Open".into(), "Waiting for a player".into()),
                SeatKind::Closed => ("Closed".into(), "Nobody plays here".into()),
            };
            widgets::text(ui, &format!("{base}/name"), &name);
            widgets::text(ui, &format!("{base}/sub"), &sub);
            // The seat: a joined player's name, locked; otherwise open, closed or computer.
            let field = |f: &str| format!("{base}/choices/{f}");
            match kind {
                SeatKind::Human => {
                    widgets::options(ui, &field("seat"), std::slice::from_ref(&name), 0)
                }
                _ => widgets::options(ui, &field("seat"), &seat_options(), seat_option(kind)),
            }
            widgets::enabled(
                ui,
                &field("seat"),
                host_seat && waiting && kind != SeatKind::Human,
            );
            for f in ["team", "race", "hero", "color"] {
                widgets::visible(ui, &field(f), plays);
            }
            if !plays {
                continue;
            }
            let team = seat.team.unwrap_or(seat.player + 1).clamp(1, TEAMS);
            widgets::options(ui, &field("team"), &teams, usize::from(team - 1));
            let race_id = seat_race(data, seat);
            let race = races.iter().position(|r| r.0 == race_id).unwrap_or(0);
            widgets::options(ui, &field("race"), &race_names, race);
            let (hero_choices, default_hero) = widgets::heroes(data, &race_id);
            let hero_names: Vec<String> = hero_choices.iter().map(|h| h.1.clone()).collect();
            let hero = seat
                .hero
                .clone()
                .or(default_hero)
                .and_then(|h| hero_choices.iter().position(|x| x.0 == h))
                .unwrap_or(0);
            widgets::options(ui, &field("hero"), &hero_names, hero);
            let colors = free_colors(seats, i, palette.len());
            let color_names: Vec<String> = colors.iter().map(|&c| palette.name(c)).collect();
            let selected = colors.iter().position(|&c| c == color).unwrap_or(0);
            widgets::options(ui, &field("color"), &color_names, selected);
            widgets::enabled(ui, &field("team"), editable);
            widgets::enabled(ui, &field("race"), editable && race_names.len() > 1);
            widgets::enabled(ui, &field("hero"), editable && hero_names.len() > 1);
            widgets::enabled(ui, &field("color"), editable && color_names.len() > 1);
        }
        widgets::text(ui, "lobby/status", &self.status);
        widgets::visible(ui, "lobby/status", !self.status.is_empty());
        widgets::visible(ui, "lobby/start", host_seat);
        widgets::visible(ui, "lobby/waiting", !host_seat);
        widgets::enabled(ui, "lobby/start", host_seat && waiting);
        widgets::text(
            ui,
            "lobby/footnote",
            if waiting {
                "Lobby open · Players can join until the match starts"
            } else {
                "Match starting…"
            },
        );
    }

    /// A lobby choice on seat row `row`: the host's seat dropdown, or a race, hero, team or
    /// colour for your own seat (or, hosting, a computer's).
    pub fn choose(&mut self, data: &GameData, field: &str, row: usize, option: usize) {
        let Some(session) = &self.session else {
            return;
        };
        let Some(room) = session.room() else {
            return;
        };
        let Some(seat) = room.slots.get(row) else {
            return;
        };
        // `None` addresses this connection's own seat.
        let target = (session.player() != Some(seat.player)).then_some(seat.player);
        let races = widgets::races(data);
        match field {
            "seat" if seat_kind(seat) != SeatKind::Human => {
                if let Some(&kind) = SEAT_OPTIONS.get(option) {
                    session.set_seat(seat.player, kind);
                }
            }
            "team" => session.select_team(target, option.min(usize::from(TEAMS) - 1) as u8 + 1),
            "race" => {
                if let Some((race, _)) = races.get(option) {
                    session.select_race(target, race);
                }
            }
            "hero" => {
                let (heroes, _) = widgets::heroes(data, &seat_race(data, seat));
                if let Some((hero, _)) = heroes.get(option) {
                    session.select_hero(target, hero);
                }
            }
            "color" => {
                let colors = free_colors(&room.slots, row, COLORS);
                if let Some(&color) = colors.get(option) {
                    session.select_color(target, color);
                }
            }
            _ => {}
        }
    }

    pub fn start(&self) {
        if let Some(session) = &self.session {
            session.start_match();
        }
    }
}

/// Teams and colours a lobby offers (the server's `TEAMS`, `COLORS`).
const TEAMS: u8 = 8;
const COLORS: usize = 8;

/// The host's seat dropdown, in order.
const SEAT_OPTIONS: [SeatKind; 3] = [SeatKind::Open, SeatKind::Closed, SeatKind::Ai];

fn seat_options() -> Vec<String> {
    vec!["Open".into(), "Closed".into(), "Computer".into()]
}

fn seat_option(kind: SeatKind) -> usize {
    SEAT_OPTIONS.iter().position(|&k| k == kind).unwrap_or(0)
}

/// What holds a seat; servers before lobby seats only name the joined ones.
fn seat_kind(seat: &Seat) -> SeatKind {
    seat.kind.unwrap_or(if seat.name.is_some() {
        SeatKind::Human
    } else {
        SeatKind::Open
    })
}

/// The race a seat plays: its own choice, else the content's default race.
fn seat_race(data: &GameData, seat: &Seat) -> String {
    seat.race
        .clone()
        .unwrap_or_else(|| data.content.rules.default_race.clone())
}

/// Colours seat `row` may take: its own and those no other seat taking part has.
fn free_colors(seats: &[Seat], row: usize, count: usize) -> Vec<u8> {
    let taken: Vec<u8> = seats
        .iter()
        .enumerate()
        .filter(|&(i, s)| i != row && matches!(seat_kind(s), SeatKind::Human | SeatKind::Ai))
        .map(|(_, s)| s.color.unwrap_or(s.player))
        .collect();
    (0..count.min(usize::from(u8::MAX)) as u8)
        .filter(|c| !taken.contains(c))
        .collect()
}
