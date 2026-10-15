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
    wire::{CreateRoom, Health, JoinRoom, Joined, MatchConfig, Role, RoomState, RoomView},
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

/// A short form of a map's identity for `mapRevision`.
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
                let bytes = std::fs::read(&map.file.path).unwrap_or_default();
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
            let joined = r.slots.iter().filter(|s| s.name.is_some()).count();
            widgets::text(ui, "lobby/name", &r.name);
            widgets::text(ui, "lobby/host", &format!("Hosted by {}", r.host));
            widgets::text(
                ui,
                "lobby/room-meta",
                &format!(
                    "{joined} / {} players joined · Input buffer {} ms",
                    r.slots.len(),
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
                "Gather your players, then start the match when everyone has arrived."
            } else {
                "Choose your race and hero. The host starts the match."
            },
        );
        let races: Vec<(String, String)> = room
            .and_then(|r| r.races.as_ref())
            .map(|m| {
                m.iter()
                    .map(|(id, r)| (id.clone(), r.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let seats = room.map_or(&[][..], |r| &r.slots[..]);
        if let Some(list) = ui.find("lobby/seats") {
            ui.set_count(list, seats.len());
        }
        let waiting = room.is_some_and(|r| r.state == RoomState::Waiting);
        for (i, seat) in seats.iter().enumerate() {
            let base = format!("lobby/seats/{i}");
            if let Some(badge) = ui.find(&format!("{base}/badge")) {
                ui.set_text(badge, &(seat.player + 1).to_string());
                ui.set_color(badge, Some(palette.badge(seat.player)));
            }
            let mine = me == Some(seat.player);
            let name = match (&seat.name, mine) {
                (Some(n), true) => format!("{n} (you)"),
                (Some(n), false) => n.clone(),
                (None, _) => "Open slot".into(),
            };
            widgets::text(ui, &format!("{base}/name"), &name);
            let sub = match (&seat.name, seat.round_trip_ms) {
                (Some(_), Some(ms)) => format!("Connected · {ms} ms RTT"),
                (Some(_), None) => "Connected".into(),
                (None, _) => "Waiting for a player".into(),
            };
            widgets::text(ui, &format!("{base}/sub"), &sub);
            widgets::visible(ui, &format!("{base}/choices"), seat.name.is_some());
            let race_names: Vec<String> = races.iter().map(|r| r.1.clone()).collect();
            let race = seat
                .race
                .as_ref()
                .and_then(|id| races.iter().position(|r| &r.0 == id))
                .unwrap_or(0);
            widgets::options(ui, &format!("{base}/choices/race"), &race_names, race);
            let heroes = room.and_then(|r| {
                let race_id = seat
                    .race
                    .as_deref()
                    .or(races.first().map(|r| r.0.as_str()))?;
                r.races
                    .as_ref()
                    .and_then(|m| m.get(race_id))
                    .and_then(|c| c.heroes.clone())
                    .or_else(|| r.heroes.clone())
            });
            let hero_ids: Vec<String> = heroes.as_ref().map_or(Vec::new(), |h| h.choices.clone());
            let hero_names: Vec<String> = hero_ids
                .iter()
                .map(|id| {
                    data.content
                        .definition(id)
                        .map_or_else(|| id.clone(), |d| d.name.clone())
                })
                .collect();
            let hero = seat
                .hero
                .as_ref()
                .and_then(|h| hero_ids.iter().position(|x| x == h))
                .unwrap_or(0);
            widgets::options(ui, &format!("{base}/choices/hero"), &hero_names, hero);
            widgets::enabled(
                ui,
                &format!("{base}/choices/race"),
                mine && waiting && race_names.len() > 1,
            );
            widgets::enabled(
                ui,
                &format!("{base}/choices/hero"),
                mine && waiting && hero_names.len() > 1,
            );
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

    /// A race or hero choice in the lobby (own seat).
    pub fn choose(&mut self, field: &str, option: usize) {
        let Some(session) = &self.session else {
            return;
        };
        let Some(room) = session.room() else {
            return;
        };
        let me = session.player();
        let seat = room.slots.iter().find(|s| Some(s.player) == me);
        let races: Vec<String> = room
            .races
            .as_ref()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        match field {
            "race" => {
                if let Some(race) = races.get(option) {
                    session.select_race(race);
                }
            }
            "hero" => {
                let race = seat.and_then(|s| s.race.clone()).or(races.first().cloned());
                let heroes = race
                    .and_then(|r| room.races.as_ref()?.get(&r)?.heroes.clone())
                    .or_else(|| room.heroes.clone());
                if let Some(hero) = heroes.and_then(|h| h.choices.get(option).cloned()) {
                    session.select_hero(&hero);
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
