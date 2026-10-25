//! Multiplayer (old `src/ui/menu/multiplayer.ts`): the match server's status, its
//! lobbies, creating and joining one, and the room before its match, through
//! `canopy-net` (D32). Lobby calls are polled, never awaited; the room's socket session
//! is polled every frame while the room waits and moves into the match when the server
//! starts it.
//!
//! The protocol has no lobby chat or ready flags (chat flows only while a match plays),
//! so the room shows seats, races, heroes and measured latency, and the host starts.
//!
//! Protocol 2 (D107): a room is joined to play or to spectate (a match under way only to
//! spectate); the lobby lists its spectators, whose places and delay the host sets; the
//! host can remove anyone and sets each computer's level; people move between seats and
//! spectator places. The seat of a match under way is kept in the user folder, so a game
//! that restarted can rejoin it.

use super::{ui_host::UiHost, widgets, widgets::Palette};
use crate::data::{GameData, MapEntry};
use canopy_net::queue::{EnterQueue, QueueInfo, QueueMode, Ticket, TicketState};
use canopy_net::{
    Event, Net, Pending, Phase, ServerAddress, Session,
    wire::{
        CreateRoom, Health, JoinRoom, Joined, MatchConfig, OBSERVER_DELAYS, PROTOCOL, Role,
        RoomState, RoomView, SPECTATOR_PLACES, Seat, SeatKind,
    },
};
use canopy_render::Gpu;
use canopy_sim::Difficulty;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// The seat of a networked match under way, kept so a game that restarted can rejoin it
/// (D107). Removed once that match is over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSeat {
    /// The server's address (`http://…`), so another server's rooms are not asked.
    pub server: String,
    pub room: String,
    pub token: String,
}

impl SavedSeat {
    pub fn load(paths: &crate::paths::Paths) -> Option<Self> {
        let bytes = std::fs::read(paths.multiplayer_seat()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, paths: &crate::paths::Paths) {
        let written = serde_json::to_vec(self)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| crate::settings::write_atomic(&paths.multiplayer_seat(), &bytes));
        if let Err(error) = written {
            tracing::warn!(%error, "cannot keep the match's seat");
        }
    }

    pub fn clear(paths: &crate::paths::Paths) {
        let _ = std::fs::remove_file(paths.multiplayer_seat());
    }
}

/// Rooms refresh this often while the list shows.
const REFRESH: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Ranked and unranked 1v1 matchmaking (D107).
    Queue,
    Find,
    Create,
}

/// A search in a matchmaking queue.
#[derive(Debug, Clone)]
pub struct Search {
    pub ticket: String,
    pub since: Instant,
    /// The server's count, else the time since searching began.
    pub seconds: u32,
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
    /// This connection created the room (the host may also spectate).
    hosting: bool,
    /// A kept seat of a match under way, and the check whether it still plays.
    pub saved: Option<SavedSeat>,
    saved_check: Option<Pending<RoomView>>,
    /// The kept seat's match plays on: it can be rejoined.
    pub rejoinable: Option<RoomView>,
    /// Matchmaking: what the server's queues offer, the choices, and a search under way.
    pub queue: Option<QueueInfo>,
    queue_pending: Option<Pending<QueueInfo>>,
    pub queue_mode: QueueMode,
    /// A race id of the queue's, or [`canopy_net::queue::RANDOM`].
    pub queue_race: String,
    pub queue_veto: Option<String>,
    pub search: Option<Search>,
    entering: Option<Pending<Ticket>>,
    asking: Option<Pending<TicketState>>,
    asked: Option<Instant>,
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
            tab: Tab::Queue,
            selected_room: None,
            selected_map: 0,
            seats: 0,
            joining: None,
            session: None,
            status: String::new(),
            hosting: false,
            saved: None,
            saved_check: None,
            rejoinable: None,
            queue: None,
            queue_pending: None,
            queue_mode: QueueMode::Ranked,
            queue_race: canopy_net::queue::RANDOM.into(),
            queue_veto: None,
            search: None,
            entering: None,
            asking: None,
            asked: None,
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
                self.status = tr!("mp.bad-address", error = &e.to_string());
                return;
            }
        };
        if self.net.as_ref().is_none_or(|n| n.server() != &address) {
            self.net = match Net::new(address.clone()) {
                Ok(net) => Some(net),
                Err(e) => {
                    self.status = tr!("mp.no-networking", error = &e.to_string());
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

    /// Looks whether the kept seat's match still plays (`saved` read from the user folder).
    pub fn check_saved(&mut self, saved: Option<SavedSeat>) {
        self.rejoinable = None;
        self.saved = saved.filter(|s| s.server == self.server);
        self.saved_check = match (&self.net, &self.saved) {
            (Some(net), Some(s)) => Some(net.room(&s.room)),
            _ => None,
        };
    }

    /// Rejoins the kept seat's match: the server sends the match and the session catches
    /// up with it.
    pub fn rejoin(&mut self) -> Option<Outcome> {
        let (Some(net), Some(saved)) = (&self.net, &self.saved) else {
            return None;
        };
        self.session = Some(net.rejoin(&saved.room, &saved.token));
        self.hosting = false;
        self.rejoinable = None;
        self.status = tr!("mp.contacting-host");
        None
    }

    pub fn refresh(&mut self) {
        let Some(net) = &self.net else {
            return;
        };
        self.health = Some(net.health());
        self.rooms_pending = Some(net.rooms());
        self.queue_pending = Some(net.queue_info());
        self.last_refresh = Some(Instant::now());
    }

    /// Starts searching the chosen queue, or stops the search under way.
    pub fn toggle_search(&mut self, name: &str, player_id: &str) {
        if self.search.is_some() || self.entering.is_some() {
            self.stop_search();
            return;
        }
        let Some(net) = &self.net else {
            self.status = tr!("mp.not-connected");
            return;
        };
        let body = EnterQueue::new(
            player_id,
            name,
            self.queue_mode,
            &self.queue_race,
            self.queue_veto.as_deref(),
        );
        self.entering = Some(net.enter_queue(&body));
        self.status = tr!("mp.contacting-host");
    }

    /// Stops searching (leaving the screen, or Cancel).
    pub fn stop_search(&mut self) {
        if let (Some(net), Some(search)) = (&self.net, self.search.take()) {
            drop(net.leave_queue(&search.ticket));
        }
        self.entering = None;
        self.asking = None;
        self.status.clear();
    }

    /// A queue choice: the mode, race or veto dropdown's option.
    pub fn choose_queue(&mut self, field: &str, option: usize, data: &GameData) {
        match field {
            "mode" => {
                self.queue_mode = if option == 1 {
                    QueueMode::Unranked
                } else {
                    QueueMode::Ranked
                }
            }
            "race" => {
                self.queue_race = option
                    .checked_sub(1)
                    .and_then(|i| self.queue.as_ref()?.races.get(i))
                    .map_or_else(|| canopy_net::queue::RANDOM.into(), |r| r.id.clone());
            }
            "veto" => {
                self.queue_veto = option
                    .checked_sub(1)
                    .and_then(|i| self.vetoable(data).get(i).map(|m| m.id.clone()));
            }
            _ => {}
        }
    }

    /// The pool maps this game has, which may be vetoed.
    fn vetoable(&self, data: &GameData) -> Vec<canopy_net::queue::Named> {
        self.queue.as_ref().map_or_else(Vec::new, |q| {
            q.pool
                .iter()
                .filter(|m| data.map(&m.id).is_some())
                .cloned()
                .collect()
        })
    }

    /// Follows the search: asks for the ticket about once a second and enters the room a
    /// match puts this player in.
    fn follow_search(&mut self) {
        if let Some(reply) = self.entering.as_ref().and_then(Pending::poll) {
            self.entering = None;
            match reply {
                Ok(t) => {
                    self.search = Some(Search {
                        ticket: t.ticket,
                        since: Instant::now(),
                        seconds: 0,
                    });
                    self.status.clear();
                }
                Err(e) => self.status = e.to_string(),
            }
        }
        if let Some(reply) = self.asking.as_ref().and_then(Pending::poll) {
            self.asking = None;
            match reply {
                Ok(TicketState::Searching { seconds, .. }) => {
                    if let Some(s) = &mut self.search {
                        s.seconds = seconds;
                    }
                }
                Ok(TicketState::Matched(joined)) => {
                    self.search = None;
                    if let Some(net) = &self.net {
                        self.session = Some(net.enter(&joined));
                        self.hosting = false;
                        self.status = tr!("mp.match-found");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "matchmaking ticket");
                    self.search = None;
                    self.status = tr!("mp.search-ended");
                }
            }
        }
        let due = self
            .asked
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
        if let (Some(net), Some(search), None, true) = (&self.net, &self.search, &self.asking, due)
        {
            self.asking = Some(net.ticket(&search.ticket));
            self.asked = Some(Instant::now());
        }
    }

    /// Polls replies (every frame while a multiplayer screen shows).
    pub fn poll(&mut self, on_list: bool) -> Option<Outcome> {
        if let Some(reply) = self.health.as_ref().and_then(Pending::poll) {
            self.health = None;
            match reply {
                Ok(h) if h.ok => {
                    self.reachable = Some(true);
                    self.health_text = tr!("mp.host-online", version = h.version.as_str());
                }
                Ok(_) => {
                    self.reachable = Some(false);
                    self.health_text = tr!("mp.host-unhealthy");
                }
                Err(e) => {
                    self.reachable = Some(false);
                    self.health_text = tr!("mp.host-unreachable");
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
                    self.status = tr!("mp.host-unreachable-error", error = &e.to_string());
                }
            }
        }
        if on_list
            && self.session.is_none()
            && self.last_refresh.is_some_and(|t| t.elapsed() > REFRESH)
        {
            self.refresh();
        }
        if let Some(reply) = self.queue_pending.as_ref().and_then(Pending::poll) {
            self.queue_pending = None;
            self.queue = reply.ok();
        }
        self.follow_search();
        if let Some(reply) = self.saved_check.as_ref().and_then(Pending::poll) {
            self.saved_check = None;
            match reply {
                Ok(room) if room.state == RoomState::Playing => self.rejoinable = Some(room),
                // Over, or gone: nothing to rejoin.
                _ => self.saved = None,
            }
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
                Event::Kicked => {
                    self.status = tr!("mp.kicked");
                    left = true;
                }
                Event::Stopped { reason } => {
                    self.status = reason;
                    left = true;
                }
                Event::Closed { reason } => {
                    self.status = if reason.is_empty() {
                        tr!("mp.lobby-closed")
                    } else {
                        tr!("mp.lobby-closed-reason", reason = &reason)
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

    /// Joins the selected room (to play, or as a spectator) or creates one on
    /// `maps[selected_map]`, as the tab says.
    pub fn go(&mut self, maps: &[&MapEntry], name: &str, player_id: &str, role: Role) {
        let Some(net) = &self.net else {
            self.status = tr!("mp.not-connected");
            return;
        };
        if name.trim().is_empty() {
            self.status = tr!("mp.enter-name");
            return;
        }
        match self.tab {
            Tab::Queue => {}
            Tab::Find => {
                let Some(room) = self.selected_room.and_then(|i| self.rooms.get(i)) else {
                    return;
                };
                self.hosting = false;
                self.joining = Some(net.join_room(
                    &room.id,
                    &JoinRoom {
                        guest_name: name.into(),
                        role,
                        player_id: player_id.into(),
                        protocol: PROTOCOL,
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
                    name: tr!("mp.lobby-of", name = name),
                    map_id: map.id().into(),
                    map_revision: map_revision(&bytes),
                    slot_count: seats,
                    guest_name: name.into(),
                    player_id: player_id.into(),
                    protocol: PROTOCOL,
                }));
                self.hosting = true;
            }
        }
        self.status = tr!("mp.contacting-host");
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
        self.session.is_some() && self.hosting
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
        let queueing = self.tab == Tab::Queue;
        let ui = &mut host.ui;
        widgets::checked(ui, "multiplayer/queue", queueing);
        widgets::checked(ui, "multiplayer/find", self.tab == Tab::Find);
        widgets::checked(ui, "multiplayer/create", creating);
        widgets::visible(ui, "multiplayer/queue-panel", queueing);
        for id in ["list-title", "hint", "spectate"] {
            widgets::visible(ui, &format!("multiplayer/{id}"), !queueing);
        }
        if queueing {
            self.show_queue(host, gpu, data, palette);
            return;
        }
        widgets::text(
            ui,
            "multiplayer/list-title",
            &if creating {
                tr!("mp.battlefields")
            } else {
                tr!("mp.lobbies")
            },
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
                    tr!("mp.host-hint")
                } else {
                    tr!("mp.needs-two-starts")
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
                    tr!("mp.lobby-of", name = r.host.as_str())
                } else {
                    r.name.clone()
                };
                let map_name = data
                    .map(&r.map_id)
                    .map_or(r.map_id.as_str(), |m| m.name.as_str());
                let state = match r.state {
                    RoomState::Playing => tr!("mp.room-in-game", taken = taken, total = total),
                    _ if taken >= total => tr!("mp.room-full", taken = taken, total = total),
                    _ => tr!("mp.room-open", taken = taken, total = total),
                };
                widgets::text(&mut host.ui, &format!("{base}/name"), &name);
                widgets::text(&mut host.ui, &format!("{base}/map"), map_name);
                widgets::text(&mut host.ui, &format!("{base}/state"), &state);
                widgets::checked(&mut host.ui, &base, self.selected_room == Some(i));
            }
            let empty = if !self.rooms_loaded {
                tr!("mp.looking")
            } else if self.rooms.is_empty() {
                tr!("mp.no-lobbies")
            } else {
                String::new()
            };
            widgets::text(&mut host.ui, "multiplayer/empty", &empty);
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
                        &seat.name.clone().unwrap_or_else(|| tr!("mp.open-slot")),
                    );
                    widgets::text(
                        &mut host.ui,
                        &format!("{base}/sub"),
                        &if seat.name.is_some() {
                            tr!("mp.joined")
                        } else {
                            tr!("mp.waiting-for-player")
                        },
                    );
                }
            }
            let free = room.is_some_and(|r| r.slots.iter().any(|s| s.name.is_none()));
            let (hint, ok) = match room {
                None => (String::new(), false),
                Some(r) if r.state == RoomState::Playing => (tr!("mp.underway-spectate"), false),
                Some(_) if map.is_none() => (tr!("mp.map-unavailable"), false),
                Some(_) if !free => (tr!("mp.lobby-full"), false),
                Some(r) => (tr!("mp.hosted-by-join", host = r.host.as_str()), true),
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
            None => tr!("mp.contacting", server = &self.server.to_string()),
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
                tr!("mp.footnote-create")
            } else {
                tr!("mp.footnote-find", open = open)
            },
        );
        widgets::text(
            ui,
            "multiplayer/go",
            &if creating {
                tr!("mp.create-lobby")
            } else {
                tr!("mp.join-lobby")
            },
        );
        widgets::enabled(
            ui,
            "multiplayer/go",
            can_go && self.net.is_some() && !self.busy(),
        );
        // Spectating: a lobby with a free spectator place, or a match under way.
        let room = self.selected_room.and_then(|i| self.rooms.get(i));
        let watchable = room.is_some_and(|r| {
            r.state == RoomState::Playing
                || (r.state == RoomState::Waiting
                    && (r.observers.len() as u32) < r.spectator_places)
        });
        widgets::visible(ui, "multiplayer/spectate", !creating);
        widgets::enabled(
            ui,
            "multiplayer/spectate",
            watchable && map_known(data, room) && self.net.is_some() && !self.busy(),
        );
        widgets::visible(ui, "multiplayer/rejoin", self.rejoinable.is_some());
        if let Some(r) = &self.rejoinable {
            let map_name = data
                .map(&r.map_id)
                .map_or(r.map_id.as_str(), |m| m.name.as_str());
            widgets::text(ui, "multiplayer/rejoin", &tr!("mp.rejoin", map = map_name));
        }
    }

    /// Matchmaking: the queue, race and veto, and the search.
    fn show_queue(&self, host: &mut UiHost, gpu: &Gpu, data: &GameData, palette: &Palette) {
        let ui = &mut host.ui;
        for id in [
            "rooms",
            "maps",
            "refresh",
            "empty",
            "roster",
            "roster-caption",
            "slots",
            "slots-caption",
        ] {
            widgets::visible(ui, &format!("multiplayer/{id}"), false);
        }
        let searching = self.search.is_some() || self.entering.is_some();
        let modes = [tr!("mp.ranked"), tr!("mp.unranked")];
        widgets::options(
            ui,
            "multiplayer/queue-panel/mode",
            &modes,
            usize::from(self.queue_mode == QueueMode::Unranked),
        );
        let mut races = vec![tr!("mp.random")];
        let mut race = 0;
        if let Some(q) = &self.queue {
            for (i, r) in q.races.iter().enumerate() {
                // The game's own name for the race when it has it.
                let name = widgets::races(data)
                    .into_iter()
                    .find(|(id, _)| *id == r.id)
                    .map_or_else(|| r.name.clone(), |(_, n)| n);
                races.push(name);
                if r.id == self.queue_race {
                    race = i + 1;
                }
            }
        }
        widgets::options(ui, "multiplayer/queue-panel/race", &races, race);
        let vetoable = self.vetoable(data);
        let mut vetoes = vec![tr!("mp.no-veto")];
        vetoes.extend(vetoable.iter().map(|m| {
            data.map(&m.id)
                .map_or_else(|| m.name.clone(), |e| e.name.clone())
        }));
        let veto = self
            .queue_veto
            .as_ref()
            .and_then(|v| vetoable.iter().position(|m| &m.id == v))
            .map_or(0, |i| i + 1);
        widgets::options(ui, "multiplayer/queue-panel/veto", &vetoes, veto);
        for id in ["mode", "race", "veto"] {
            widgets::enabled(ui, &format!("multiplayer/queue-panel/{id}"), !searching);
        }
        let pool: Vec<String> = self.queue.as_ref().map_or_else(Vec::new, |q| {
            q.pool
                .iter()
                .map(|m| {
                    data.map(&m.id)
                        .map_or_else(|| m.name.clone(), |e| e.name.clone())
                })
                .collect()
        });
        let available = self.queue.as_ref().is_some_and(|q| !q.pool.is_empty());
        widgets::text(
            ui,
            "multiplayer/queue-panel/pool",
            &if available {
                tr!("mp.pool", maps = &pool.join(", "))
            } else {
                tr!("mp.queue-unavailable")
            },
        );
        let state = match &self.search {
            Some(s) => {
                let seconds = s.seconds.max(s.since.elapsed().as_secs() as u32);
                tr!(
                    "mp.searching",
                    time = &format!("{}:{:02}", seconds / 60, seconds % 60)
                )
            }
            None if self.session.is_some() => tr!("mp.match-found"),
            None => self.queue.as_ref().map_or_else(String::new, |q| {
                let n = match self.queue_mode {
                    QueueMode::Ranked => q.searching.ranked,
                    QueueMode::Unranked => q.searching.unranked,
                };
                tr!("mp.searching-count", count = n)
            }),
        };
        widgets::text(ui, "multiplayer/queue-panel/state", &state);
        // The vetoed map in the preview, so the choice is seen.
        let shown = self.queue_veto.as_deref().and_then(|v| data.map(v));
        widgets::map_preview(host, gpu, data, palette, "multiplayer", shown, None);
        let ui = &mut host.ui;
        widgets::text(ui, "multiplayer/status", &self.status);
        widgets::visible(ui, "multiplayer/status", !self.status.is_empty());
        widgets::text(
            ui,
            "multiplayer/footnote",
            &match self.queue_mode {
                QueueMode::Ranked => tr!("mp.footnote-ranked"),
                QueueMode::Unranked => tr!("mp.footnote-unranked"),
            },
        );
        widgets::text(
            ui,
            "multiplayer/go",
            &if searching {
                tr!("mp.cancel-search")
            } else {
                tr!("mp.find-match")
            },
        );
        widgets::enabled(
            ui,
            "multiplayer/go",
            self.net.is_some() && self.session.is_none() && (available || searching),
        );
        widgets::visible(ui, "multiplayer/rejoin", self.rejoinable.is_some());
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
            widgets::text(
                ui,
                "lobby/host",
                &tr!("mp.hosted-by", host = r.host.as_str()),
            );
            let mut parts = vec![tr!("mp.players-count", count = humans)];
            if computers > 0 {
                parts.push(tr!("mp.computers-count", count = computers));
            }
            parts.push(tr!("mp.open-count", count = open));
            widgets::text(
                ui,
                "lobby/room-meta",
                &tr!(
                    "mp.room-meta",
                    parts = &parts.join(" · "),
                    delay = r.input_delay_ms.unwrap_or(0)
                ),
            );
        }
        widgets::text(
            ui,
            "lobby/badge",
            &if host_seat {
                tr!("mp.you-host")
            } else {
                tr!("mp.you-joined")
            },
        );
        widgets::text(
            ui,
            "lobby/copy",
            &if host_seat {
                tr!("mp.host-copy")
            } else {
                tr!("mp.guest-copy")
            },
        );
        let races = widgets::races(data);
        let race_names: Vec<String> = races.iter().map(|r| r.1.clone()).collect();
        let seats = room.map_or(&[][..], |r| &r.slots[..]);
        if let Some(list) = ui.find("lobby/seats") {
            ui.set_count(list, seats.len());
        }
        let waiting = room.is_some_and(|r| r.state == RoomState::Waiting);
        let teams: Vec<String> = (1..=TEAMS).map(|t| tr!("mp.team", number = t)).collect();
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
                        if mine { tr!("match.you", name = &n) } else { n },
                        seat.round_trip_ms.map_or_else(
                            || tr!("mp.connected"),
                            |ms| tr!("mp.connected-rtt", ms = ms),
                        ),
                    )
                }
                SeatKind::Ai => (
                    crate::text::tr(&format!("skirmish.ai-{}", seat.difficulty.name())),
                    tr!("mp.played-by-computer"),
                ),
                SeatKind::Open => (tr!("mp.seat-open"), tr!("mp.waiting-for-player")),
                SeatKind::Closed => (tr!("mp.seat-closed"), tr!("mp.nobody-plays")),
            };
            widgets::text(ui, &format!("{base}/name"), &name);
            widgets::text(ui, &format!("{base}/sub"), &sub);
            // The seat: a joined player's name, locked; otherwise open, closed or computer.
            let field = |f: &str| format!("{base}/choices/{f}");
            match kind {
                SeatKind::Human if host_seat && !mine => widgets::options(
                    ui,
                    &field("seat"),
                    &[name.clone(), tr!("mp.remove-from-lobby")],
                    0,
                ),
                SeatKind::Human => {
                    widgets::options(ui, &field("seat"), std::slice::from_ref(&name), 0)
                }
                _ => widgets::options(ui, &field("seat"), &seat_options(), seat_option(seat)),
            }
            widgets::enabled(ui, &field("seat"), host_seat && waiting && !mine);
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
        // Spectators: who watches, the places and delay (the host's), moving between a seat
        // and a spectator place.
        let observers = room.map_or(&[][..], |r| &r.observers[..]);
        if let Some(list) = ui.find("lobby/observers") {
            ui.set_count(list, observers.len());
        }
        for (i, o) in observers.iter().enumerate() {
            let base = format!("lobby/observers/{i}");
            widgets::text(ui, &format!("{base}/name"), &o.name);
            widgets::visible(ui, &format!("{base}/kick"), host_seat && waiting);
        }
        let places = room.map_or(0, |r| r.spectator_places);
        widgets::text(
            ui,
            "lobby/observers-empty",
            &if observers.is_empty() {
                tr!("mp.no-spectators", places = places)
            } else {
                tr!("mp.spectators-of", count = observers.len(), places = places)
            },
        );
        let place_options: Vec<String> = (0..=SPECTATOR_PLACES)
            .map(|n| tr!("mp.spectator-places", count = n))
            .collect();
        widgets::options(ui, "lobby/places", &place_options, places as usize);
        let delays: Vec<String> = OBSERVER_DELAYS
            .iter()
            .map(|&s| {
                if s == 0 {
                    tr!("mp.observer-delay-off")
                } else {
                    tr!("mp.observer-delay", minutes = s / 60)
                }
            })
            .collect();
        let delay = room.map_or(0, |r| r.observer_delay);
        let delay = OBSERVER_DELAYS
            .iter()
            .position(|&s| s == delay)
            .unwrap_or(0);
        widgets::options(ui, "lobby/delay", &delays, delay);
        for id in ["lobby/places", "lobby/delay"] {
            widgets::enabled(ui, id, host_seat && waiting);
        }
        let spectating = me.is_none();
        let open_seat = seats.iter().any(|s| seat_kind(s) == SeatKind::Open);
        let free_place = (observers.len() as u32) < places;
        widgets::text(
            ui,
            "lobby/move",
            &if spectating {
                tr!("mp.take-seat")
            } else {
                tr!("mp.spectate")
            },
        );
        widgets::enabled(
            ui,
            "lobby/move",
            waiting && if spectating { open_seat } else { free_place },
        );
        widgets::text(ui, "lobby/status", &self.status);
        widgets::visible(ui, "lobby/status", !self.status.is_empty());
        widgets::visible(ui, "lobby/start", host_seat);
        widgets::visible(ui, "lobby/waiting", !host_seat);
        widgets::enabled(ui, "lobby/start", host_seat && waiting);
        widgets::text(
            ui,
            "lobby/footnote",
            &if waiting {
                tr!("mp.lobby-open")
            } else {
                tr!("mp.match-starting")
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
                if let Some(&(kind, level)) = SEAT_OPTIONS.get(option) {
                    session.set_seat(seat.player, kind, level);
                }
            }
            // The host's dropdown on another player's seat: their name, or remove them.
            "seat" if option == 1 && self.hosting && session.player() != Some(seat.player) => {
                session.kick_player(seat.player);
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

    /// A spectator-places or observer-delay choice (host), or removing spectator `row`.
    pub fn lobby_setting(&self, field: &str, option: usize) {
        let Some(session) = &self.session else {
            return;
        };
        match field {
            "places" => session.set_spectator_places(option as u32),
            "delay" => {
                if let Some(&seconds) = OBSERVER_DELAYS.get(option) {
                    session.set_observer_delay(seconds);
                }
            }
            "kick-observer" => {
                if let Some(o) = session.room().and_then(|r| r.observers.get(option)) {
                    session.kick_observer(&o.id);
                }
            }
            _ => {}
        }
    }

    /// Moves this connection between a seat and a spectator place.
    pub fn move_seat(&self) {
        let Some(session) = &self.session else {
            return;
        };
        if session.player().is_some() {
            session.spectate();
        } else if let Some(seat) = session
            .room()
            .and_then(|r| r.slots.iter().find(|s| seat_kind(s) == SeatKind::Open))
        {
            session.take_seat(seat.player);
        }
    }

    pub fn start(&self) {
        if let Some(session) = &self.session {
            session.start_match();
        }
    }
}

/// Whether this game has the room's battlefield.
fn map_known(data: &GameData, room: Option<&RoomView>) -> bool {
    room.is_some_and(|r| data.map(&r.map_id).is_some())
}

/// Teams and colours a lobby offers (the server's `TEAMS`, `COLORS`).
const TEAMS: u8 = 8;
const COLORS: usize = 8;

/// The host's seat dropdown, in order: open, closed, then a computer of each level.
const SEAT_OPTIONS: [(SeatKind, Difficulty); 5] = [
    (SeatKind::Open, Difficulty::Normal),
    (SeatKind::Closed, Difficulty::Normal),
    (SeatKind::Ai, Difficulty::Easy),
    (SeatKind::Ai, Difficulty::Normal),
    (SeatKind::Ai, Difficulty::Hard),
];

fn seat_options() -> Vec<String> {
    let mut options = vec![tr!("mp.seat-open"), tr!("mp.seat-closed")];
    options.extend(
        Difficulty::ALL
            .iter()
            .map(|d| crate::text::tr(&format!("skirmish.ai-{}", d.name()))),
    );
    options
}

fn seat_option(seat: &Seat) -> usize {
    let kind = seat_kind(seat);
    let level = if kind == SeatKind::Ai {
        seat.difficulty
    } else {
        Difficulty::Normal
    };
    SEAT_OPTIONS
        .iter()
        .position(|&o| o == (kind, level))
        .unwrap_or(0)
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
