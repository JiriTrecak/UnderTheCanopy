//! One seat in one match: what the game drives its simulation with.
//!
//! A networked session starts in the room's lobby (choices, start), turns into a match
//! when the server sends `start`, and from then on hands out the server's commits one beat
//! at a time. A local session (single-player, the old `MemoryChannel`) runs the same
//! mailbox in-process with an input delay of one beat, so local and networked play feed the
//! simulation identically.
//!
//! Protocol 2 (D107): a seat that binds to a match under way (a rejoin after a crash, a
//! spectator arriving late, or this session reconnecting by itself after its socket
//! closed) receives the missed commits in `history`, plays them as fast as the game steps,
//! and only then reports `ready` and confirms again.
//!
//! Threading: a `Session` lives on the simulation thread and never blocks. Socket I/O runs
//! on the network thread ([`crate::Net`]); [`Commands`] lets any thread (input, UI) hand
//! commands in without waiting for the simulation.
//!
//! Driving a match:
//!
//! ```no_run
//! # fn example(session: &mut canopy_net::Session, world: &mut canopy_sim::World) {
//! // Each simulation beat (every 25 ms, as the game's scheduler decides):
//! for event in session.poll() { /* chat, desync, ended, … */ }
//! if session.advance(world).is_none() {
//!     // The next beat is not committed yet: waiting for players.
//! }
//! # }
//! ```

use crate::link::{FromIo, Link, ToIo};
use crate::lockstep::Outbox;
use crate::mailbox::Mailbox;
use crate::net::Net;
use crate::wire::{
    ChatMessage, ClientIdentity, ClientMsg, Commit, ContentIdentity, Joined, MatchConfig,
    PlayerHash, Role, RoomView, SeatKind, ServerMsg, TICK_MS, WireAction, WireOutcome,
};
use canopy_sim::{Action, MatchSetup, Queued, World};
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

/// Where a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Opening the room socket.
    Connecting,
    /// In the room before the match: choices and start.
    Lobby,
    /// The match is set (`start`); the game builds its world, then calls
    /// [`Session::ready`].
    Loading,
    /// Ready; waiting for every player to load (`go`).
    Starting,
    /// Commits flow.
    Playing,
    /// The server ended the match (`ended`).
    Ended,
    /// Peers disagreed on a hash; nothing more is simulated or sent.
    Desynced,
    /// The match cannot continue on this client (see [`Event::Stopped`]).
    Stopped,
    /// The socket closed during the match; the session is opening it again (it keeps its
    /// world and continues from the commits it missed).
    Reconnecting,
    /// The socket closed for good.
    Closed,
}

/// Something that happened, in arrival order.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The room socket is open.
    Connected,
    /// The room as it is now (`welcome` and `room`).
    Room(RoomView),
    /// The match is set: build the world from [`MatchConfig::setup`].
    Started {
        config: MatchConfig,
        you: ClientIdentity,
    },
    /// Every player is loaded; commits follow from `tick`.
    Go {
        tick: u32,
    },
    Chat(ChatMessage),
    /// Every connected player reported the same hash for beat `tick`.
    HashOk {
        tick: u32,
    },
    Desync {
        tick: u32,
        hashes: Vec<PlayerHash>,
    },
    Ended {
        outcome: WireOutcome,
        replay_id: String,
    },
    /// A server error message (`START_REJECTED`, `INVALID_RACE`, `INVALID_HERO`,
    /// `CONTENT_MISMATCH`, `bad_json`). The old lobby treated any error before the start as
    /// fatal; the game decides.
    Error {
        code: String,
        message: String,
    },
    /// The socket closed during the match; the session reconnects by itself.
    Reconnecting {
        reason: String,
    },
    /// The socket is back: the match continues where this seat left it.
    Reconnected,
    /// The beats wait for these seats (`[]` once they flow again), for `seconds`.
    Waiting {
        players: Vec<u8>,
        seconds: u32,
    },
    /// A player (`None`: a referee) paused the match.
    Paused {
        player: Option<u8>,
        name: String,
        pauses_left: u32,
    },
    /// The match resumes in `seconds`.
    Resuming {
        seconds: u32,
    },
    Resumed,
    /// The host removed this connection from the lobby.
    Kicked,
    /// A committed command no peer can apply (unreadable or malformed); every Rust peer
    /// skips it identically.
    Skipped {
        beat: u32,
        player: u8,
        seq: u32,
        reason: String,
    },
    /// The match cannot continue here: a restart or save load by the host (not supported
    /// by this client, nor handled by the old one), a content mismatch, or commits missed
    /// by joining a running match.
    Stopped {
        reason: String,
    },
    Closed {
        reason: String,
    },
    /// A server message this client cannot read.
    Malformed {
        text: String,
        error: String,
    },
}

/// The commands of one beat, ready for `World::enqueue`.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// The server's beat number (from 1).
    pub beat: u32,
    /// In `(player, seq)` order; each scheduled for `World::tick() == beat - 1`.
    pub commands: Vec<Queued>,
}

/// What the game shows about the connection.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub phase: Phase,
    /// Input delay in beats (frozen at start; 1 in single-player).
    pub delay: u32,
    /// This seat's server-measured round trip, as last published in the room view (the
    /// server publishes it while the room waits).
    pub round_trip_ms: Option<u32>,
    /// The highest beat received.
    pub received: u32,
    /// The last beat simulated.
    pub clock: u32,
    /// How long the next beat has been missing while playing: waiting for other players
    /// once it exceeds a few beats.
    pub stalled: Option<Duration>,
    pub last_hash_ok: Option<u32>,
    pub desync: Option<u32>,
    /// Catching up with a match under way: how far, 0 to 1 (a rejoin or a late spectator).
    pub catching_up: Option<f32>,
    /// Who paused the match, while it is paused.
    pub paused: Option<String>,
    /// The seats the beats wait for, and for how long (seconds).
    pub waiting: (Vec<u8>, u32),
}

impl Status {
    /// The input buffer in milliseconds.
    pub fn input_delay_ms(&self) -> u32 {
        self.delay * TICK_MS
    }
}

/// Hands commands to a session from any thread.
#[derive(Clone)]
pub struct Commands(Sink);

#[derive(Clone)]
enum Sink {
    Local(mpsc::Sender<Action>),
    Remote(UnboundedSender<ToIo>),
    Inert,
}

impl Commands {
    /// Queues a command for this seat. Ignored for observers, before the match starts,
    /// and after a desync (the socket task checks).
    pub fn send(&self, action: Action) -> bool {
        match &self.0 {
            Sink::Local(tx) => tx.send(action).is_ok(),
            Sink::Remote(tx) => tx.send(ToIo::Command(action)).is_ok(),
            Sink::Inert => false,
        }
    }
}

struct Local {
    mailbox: Mailbox,
    outboxes: BTreeMap<u8, Outbox>,
    commands: mpsc::Receiver<Action>,
    sender: mpsc::Sender<Action>,
}

struct Remote {
    net: Net,
    link: Link,
    room: String,
    token: String,
    /// The session closed the socket itself: no reconnecting.
    closing: bool,
    /// Reconnection attempts since the socket closed, and when the next one is due.
    attempts: u32,
    retry_at: Option<Instant>,
}

/// How long a session keeps trying to reconnect, and how often.
const RECONNECT_ATTEMPTS: u32 = 60;
const RECONNECT_EVERY: Duration = Duration::from_secs(2);
/// A seat this many beats behind the last commit is still catching up.
const CAUGHT_UP_BEATS: u32 = 8;

enum Transport {
    Local(Local),
    Remote(Remote),
}

pub struct Session {
    transport: Transport,
    you: Option<ClientIdentity>,
    room: Option<RoomView>,
    config: Option<MatchConfig>,
    setup: Option<MatchSetup>,
    phase: Phase,
    commits: BTreeMap<u32, Commit>,
    received: u32,
    clock: u32,
    stalled_since: Option<Instant>,
    last_hash_ok: Option<u32>,
    desync: Option<u32>,
    events: Vec<Event>,
    /// Beats up to this one that have no stored commit were empty for every slot
    /// (`history` lists only commits with actions).
    empty_through: u32,
    /// The engine build and content `ready` reports, once the world is built.
    identity: Option<(ContentIdentity, String)>,
    /// `ready` went to the server for this connection.
    ready_sent: bool,
    /// The clock when a catch-up began.
    catch_up_from: Option<u32>,
    paused: Option<String>,
    waiting: (Vec<u8>, u32),
}

impl Session {
    /// A single-player match through the in-process mailbox: every slot of `setup` takes
    /// part (AI players command from inside the simulation), `me` is the local player or
    /// `None` for an observer. Commands apply on the beat after they are sent.
    pub fn local(setup: MatchSetup, me: Option<u8>) -> Self {
        let players: Vec<u8> = setup.slots.iter().map(|s| s.player).collect();
        let (sender, commands) = mpsc::channel();
        let me = me.filter(|p| players.contains(p));
        let you = me.map(|player| ClientIdentity {
            role: Role::Player,
            player: Some(player),
            name: setup
                .slots
                .iter()
                .find(|s| s.player == player)
                .and_then(|s| s.name.clone())
                .unwrap_or_default(),
        });
        let local = Local {
            mailbox: Mailbox::new(players.iter().copied(), 1, true),
            outboxes: players.iter().map(|p| (*p, Outbox::default())).collect(),
            commands,
            sender,
        };
        Self::new(
            Transport::Local(local),
            you,
            None,
            Some(setup),
            Phase::Playing,
        )
    }

    /// A single-player match resumed from a save: like [`Session::local`], with beats up
    /// to `clock` (`World::tick()` of the restored world) already played.
    pub fn local_from(setup: MatchSetup, me: Option<u8>, clock: u32) -> Self {
        let mut session = Self::local(setup, me);
        if let Transport::Local(local) = &mut session.transport {
            local.mailbox.resume(clock);
            for outbox in local.outboxes.values_mut() {
                outbox.resume(clock);
            }
        }
        session.received = clock;
        session.clock = clock;
        session
    }

    pub(crate) fn remote(net: Net, link: Link, joined: &Joined) -> Self {
        let remote = Remote {
            net,
            link,
            room: joined.room.id.clone(),
            token: joined.token.clone(),
            closing: false,
            attempts: 0,
            retry_at: None,
        };
        Self::new(
            Transport::Remote(remote),
            Some(joined.you.clone()),
            Some(joined.room.clone()),
            None,
            Phase::Connecting,
        )
    }

    /// A seat kept from an earlier session: who it is and the room arrive with `welcome`.
    pub(crate) fn rejoining(net: Net, link: Link, room: &str, token: &str) -> Self {
        let remote = Remote {
            net,
            link,
            room: room.into(),
            token: token.into(),
            closing: false,
            attempts: 0,
            retry_at: None,
        };
        Self::new(
            Transport::Remote(remote),
            None,
            None,
            None,
            Phase::Connecting,
        )
    }

    /// The room id and this seat's token (networked sessions), to rejoin after a restart.
    pub fn seat(&self) -> Option<(&str, &str)> {
        match &self.transport {
            Transport::Remote(remote) => Some((&remote.room, &remote.token)),
            Transport::Local(_) => None,
        }
    }

    fn new(
        transport: Transport,
        you: Option<ClientIdentity>,
        room: Option<RoomView>,
        setup: Option<MatchSetup>,
        phase: Phase,
    ) -> Self {
        Self {
            transport,
            you,
            room,
            config: None,
            setup,
            phase,
            commits: BTreeMap::new(),
            received: 0,
            clock: 0,
            stalled_since: None,
            last_hash_ok: None,
            desync: None,
            events: Vec::new(),
            empty_through: 0,
            identity: None,
            ready_sent: false,
            catch_up_from: None,
            paused: None,
            waiting: (Vec::new(), 0),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Who this seat is (updated by the server's `welcome` and `start`).
    pub fn you(&self) -> Option<&ClientIdentity> {
        self.you.as_ref()
    }

    /// The local player's slot; `None` for spectators and observers.
    pub fn player(&self) -> Option<u8> {
        self.you
            .as_ref()
            .filter(|y| y.role == Role::Player)
            .and_then(|y| y.player)
    }

    /// The room as last published (networked sessions).
    pub fn room(&self) -> Option<&RoomView> {
        self.room.as_ref()
    }

    /// The frozen match configuration (networked sessions, once started).
    pub fn config(&self) -> Option<&MatchConfig> {
        self.config.as_ref()
    }

    /// The simulation's start, once the match is set.
    pub fn setup(&self) -> Option<&MatchSetup> {
        self.setup.as_ref()
    }

    /// Takes what happened since the last poll, reading the socket first.
    pub fn poll(&mut self) -> Vec<Event> {
        self.pump();
        std::mem::take(&mut self.events)
    }

    /// A handle for queuing this seat's commands from other threads.
    pub fn commands(&self) -> Commands {
        if self.player().is_none() {
            return Commands(Sink::Inert);
        }
        Commands(match &self.transport {
            Transport::Local(local) => Sink::Local(local.sender.clone()),
            Transport::Remote(remote) => Sink::Remote(remote.link.to_io.clone()),
        })
    }

    /// Queues a command for this seat (old `send`): refused for observers, before the
    /// match, and after it stopped.
    pub fn send(&mut self, action: Action) -> bool {
        if !matches!(
            self.phase,
            Phase::Loading | Phase::Starting | Phase::Playing
        ) {
            return false;
        }
        self.commands().send(action)
    }

    // Lobby.

    // `seat` is `None` for this connection's own seat, or a computer seat the host sets up.

    /// `selectHero`: a seat's starting hero (while the room waits).
    pub fn select_hero(&self, seat: Option<u8>, hero: &str) {
        self.message(ClientMsg::SelectHero {
            hero: hero.into(),
            player: seat,
        });
    }

    /// `selectRace`: a seat's race; the hero resets to the race's default.
    pub fn select_race(&self, seat: Option<u8>, race: &str) {
        self.message(ClientMsg::SelectRace {
            race: race.into(),
            player: seat,
        });
    }

    /// `selectTeam`: a seat's team, 1 to 8.
    pub fn select_team(&self, seat: Option<u8>, team: u8) {
        self.message(ClientMsg::SelectTeam { team, player: seat });
    }

    /// `selectColor`: a seat's palette colour; refused when another seat has it.
    pub fn select_color(&self, seat: Option<u8>, color: u8) {
        self.message(ClientMsg::SelectColor {
            color,
            player: seat,
        });
    }

    /// `setSeat` (host): opens, closes or hands seat `player` to the computer of
    /// `difficulty`.
    pub fn set_seat(&self, player: u8, seat: SeatKind, difficulty: canopy_sim::Difficulty) {
        self.message(ClientMsg::SetSeat {
            player,
            seat,
            difficulty,
        });
    }

    /// `setSpectatorPlaces` (host): how many spectators the lobby takes (0 to 4).
    pub fn set_spectator_places(&self, count: u32) {
        self.message(ClientMsg::SetSpectatorPlaces { count });
    }

    /// `setObserverDelay` (host): how far spectators see behind, seconds.
    pub fn set_observer_delay(&self, seconds: u32) {
        self.message(ClientMsg::SetObserverDelay { seconds });
    }

    /// `kick` (host): removes the player in seat `player` from the lobby.
    pub fn kick_player(&self, player: u8) {
        self.message(ClientMsg::Kick {
            player: Some(player),
            observer: None,
        });
    }

    /// `kick` (host): removes spectator `observer` (its public id) from the lobby.
    pub fn kick_observer(&self, observer: &str) {
        self.message(ClientMsg::Kick {
            player: None,
            observer: Some(observer.into()),
        });
    }

    /// `takeSeat`: moves this connection to open seat `player`.
    pub fn take_seat(&self, player: u8) {
        self.message(ClientMsg::TakeSeat { player });
    }

    /// `spectate`: moves this connection to a free spectator place.
    pub fn spectate(&self) {
        self.message(ClientMsg::Spectate {});
    }

    /// `pause`: pauses the match (each player has three pauses).
    pub fn pause(&self) {
        self.message(ClientMsg::Pause {});
    }

    /// `resume`: resumes a paused match (the one who paused at once, others after 30 s).
    pub fn resume(&self) {
        self.message(ClientMsg::Resume {});
    }

    /// `dropPlayer`: drops a seat the match has waited 30 s for.
    pub fn drop_player(&self, player: u8) {
        self.message(ClientMsg::DropPlayer { player });
    }

    /// `startMatch` (host): over the socket, behind this seat's last selection.
    pub fn start_match(&self) {
        self.message(ClientMsg::StartMatch);
    }

    /// `chat`: delivered to everyone while the match is playing (the server cleans the
    /// text and allows one line per half second per seat).
    pub fn chat(&self, text: &str) {
        self.message(ClientMsg::Chat { text: text.into() });
    }

    /// Leaves the room as the old lobby's Back did: closes the socket and tells the server.
    pub fn leave(mut self) {
        self.close();
        if let Transport::Remote(remote) = &self.transport {
            drop(remote.net.leave_room(&remote.room, &remote.token));
        }
    }

    /// Closes the socket for good (no reconnecting). During a match the beats wait for
    /// this seat until the others drop it; [`Session::leave`] leaves at once.
    pub fn close(&mut self) {
        if let Transport::Remote(remote) = &mut self.transport {
            remote.closing = true;
            let _ = remote.link.to_io.send(ToIo::Close);
        }
    }

    // Match.

    /// Reports that this seat's world is built (`ready`): the engine `build` and content
    /// it simulates, and the world's clock (`World::tick()`). Players with different
    /// identities are refused by the server (`CONTENT_MISMATCH`). Confirmations start
    /// here. Spectators send nothing.
    ///
    /// A seat that joined a match under way reports `ready` only once it has caught up
    /// with the commits it missed ([`Session::stepped`] sends it then).
    pub fn ready(&mut self, content: ContentIdentity, build: &str, clock: u32) {
        self.clock = clock;
        if !matches!(self.transport, Transport::Remote(_)) || self.phase != Phase::Loading {
            return;
        }
        self.identity = Some((content, build.into()));
        self.phase = Phase::Starting;
        self.try_ready(clock);
    }

    /// Whether this seat has caught up with the commits it received.
    fn caught_up(&self, clock: u32) -> bool {
        self.received <= clock + CAUGHT_UP_BEATS
    }

    /// Sends `ready` (and starts confirming) once the world is built and caught up.
    fn try_ready(&mut self, clock: u32) {
        if self.ready_sent || !self.caught_up(clock) {
            return;
        }
        self.catch_up_from = None;
        let Transport::Remote(remote) = &self.transport else {
            return;
        };
        let Some((content, build)) = self.identity.clone() else {
            return;
        };
        self.ready_sent = true;
        if self.player().is_some() {
            let _ = remote
                .link
                .to_io
                .send(ToIo::Send(ClientMsg::Ready { content, build }));
            let _ = remote.link.to_io.send(ToIo::Clock {
                tick: clock,
                hash: None,
            });
        }
    }

    /// The committed commands for the beat after `clock` (`World::tick()`), or `None`
    /// while it has not arrived (or the match stopped). Once the match has ended, only
    /// the beats committed before the end follow.
    pub fn next_turn(&mut self, clock: u32) -> Option<Turn> {
        self.clock = clock;
        let next = clock + 1;
        if let Transport::Local(local) = &mut self.transport {
            while let Ok(action) = local.commands.try_recv() {
                if let Some(outbox) = self
                    .you
                    .as_ref()
                    .and_then(|y| y.player)
                    .and_then(|p| local.outboxes.get_mut(&p))
                {
                    outbox.send(action);
                }
            }
            for (player, outbox) in &mut local.outboxes {
                if let Some(ClientMsg::Turn { through, bundles }) = outbox.confirm(next, next) {
                    for commit in local.mailbox.confirm(*player, through, &bundles) {
                        self.received = commit.tick;
                        self.commits.insert(commit.tick, commit);
                    }
                }
            }
        } else {
            self.pump();
        }
        // After `ended` the beats already committed still play out: a slower peer reaches
        // the deciding beat (and every peer the same final state) before it stops.
        let ended = self.phase == Phase::Ended;
        if !ended && !matches!(self.phase, Phase::Starting | Phase::Playing) {
            return None;
        }
        let Some(commit) = self.commits.remove(&next) else {
            if next <= self.empty_through {
                // A beat `history` left out: nobody did anything.
                self.stalled_since = None;
                return Some(Turn {
                    beat: next,
                    commands: Vec::new(),
                });
            }
            if !ended {
                self.stalled_since.get_or_insert_with(Instant::now);
            }
            return None;
        };
        self.stalled_since = None;
        Some(self.turn(commit))
    }

    fn turn(&mut self, commit: Commit) -> Turn {
        let beat = commit.tick;
        let mut commands = Vec::new();
        for slot in commit.slots {
            for (seq, action) in slot.actions.into_iter().enumerate() {
                let seq = seq as u32;
                match action {
                    WireAction::Valid(action) => match action.problem() {
                        None => commands.push(Queued {
                            tick: beat - 1,
                            player: slot.player,
                            seq,
                            action,
                        }),
                        Some(problem) => self.events.push(Event::Skipped {
                            beat,
                            player: slot.player,
                            seq,
                            reason: problem.into(),
                        }),
                    },
                    WireAction::Unreadable { error, .. } => self.events.push(Event::Skipped {
                        beat,
                        player: slot.player,
                        seq,
                        reason: error,
                    }),
                }
            }
        }
        Turn { beat, commands }
    }

    /// Reports a simulated beat: `clock` is `World::tick()` after the step. Every
    /// `checksumEvery` beats a player sends `fingerprint()` as the beat's desync hash (old
    /// `hash`); the confirmations then move on.
    pub fn stepped(&mut self, clock: u32, fingerprint: impl FnOnce() -> u32) {
        self.clock = clock;
        if !matches!(self.transport, Transport::Remote(_)) {
            return;
        }
        if !self.ready_sent {
            self.try_ready(clock);
            return;
        }
        let Transport::Remote(remote) = &self.transport else {
            return;
        };
        if self.player().is_none() || !matches!(self.phase, Phase::Starting | Phase::Playing) {
            return;
        }
        let every = self.config.as_ref().map_or(0, |c| c.checksum_every);
        let hash = (every > 0 && clock.is_multiple_of(every)).then(fingerprint);
        let _ = remote.link.to_io.send(ToIo::Clock { tick: clock, hash });
    }

    /// Simulates the next beat if it is committed: enqueues its commands, steps the world
    /// and reports the beat. Returns the turn applied, or `None` while waiting.
    pub fn advance(&mut self, world: &mut World) -> Option<Turn> {
        let turn = self.next_turn(world.tick())?;
        for command in &turn.commands {
            if let Err(rejected) = world.enqueue(command.clone()) {
                self.events.push(Event::Skipped {
                    beat: turn.beat,
                    player: command.player,
                    seq: command.seq,
                    reason: rejected.to_string(),
                });
            }
        }
        world.step();
        self.stepped(world.tick(), || world.fingerprint());
        Some(turn)
    }

    /// Reports the match's outcome (`ended`): the server ends the room for everyone. The
    /// old client never sent it, which left finished rooms listed as playing.
    pub fn report_outcome(&self, outcome: WireOutcome, tick: u32, checksum: u32) {
        self.message(ClientMsg::Ended {
            outcome,
            tick,
            checksum,
        });
    }

    /// Committed beats not simulated yet: a seat catching up with a match under way is
    /// far behind; one playing along stays within its input delay.
    pub fn behind(&self) -> u32 {
        self.received.saturating_sub(self.clock)
    }

    pub fn status(&self) -> Status {
        let me = self.player();
        Status {
            phase: self.phase,
            delay: self.config.as_ref().map_or(1, |c| c.delay),
            round_trip_ms: self.room.as_ref().and_then(|room| {
                room.slots
                    .iter()
                    .find(|s| Some(s.player) == me)
                    .and_then(|s| s.round_trip_ms)
            }),
            received: self.received,
            clock: self.clock,
            stalled: self
                .stalled_since
                .filter(|_| self.phase == Phase::Playing)
                .map(|since| since.elapsed()),
            last_hash_ok: self.last_hash_ok,
            desync: self.desync,
            catching_up: self.catch_up_from.map(|from| {
                let total = self.received.saturating_sub(from).max(1);
                (self.clock.saturating_sub(from) as f32 / total as f32).clamp(0.0, 1.0)
            }),
            paused: self.paused.clone(),
            waiting: self.waiting.clone(),
        }
    }

    fn message(&self, message: ClientMsg) {
        if let Transport::Remote(remote) = &self.transport {
            let _ = remote.link.to_io.send(ToIo::Send(message));
        }
    }

    fn pump(&mut self) {
        self.retry();
        let Transport::Remote(remote) = &self.transport else {
            return;
        };
        let incoming: Vec<FromIo> = remote.link.from_io.try_iter().collect();
        for message in incoming {
            match message {
                FromIo::Open => {
                    if self.phase == Phase::Connecting {
                        self.phase = Phase::Lobby;
                    }
                    self.events.push(Event::Connected);
                }
                FromIo::Message(message) => self.receive(*message),
                FromIo::Malformed { text, error } => {
                    self.events.push(Event::Malformed { text, error })
                }
                FromIo::Closed(reason) => self.closed(reason),
            }
        }
    }

    /// The socket closed: during a match it is opened again (unless this session closed
    /// it), otherwise the session is over.
    fn closed(&mut self, reason: String) {
        let during = matches!(
            self.phase,
            Phase::Loading | Phase::Starting | Phase::Playing | Phase::Reconnecting
        );
        if let Transport::Remote(remote) = &mut self.transport
            && during
            && !remote.closing
            && remote.attempts < RECONNECT_ATTEMPTS
        {
            remote.attempts += 1;
            remote.retry_at = Some(Instant::now() + RECONNECT_EVERY);
            if self.phase != Phase::Reconnecting {
                self.phase = Phase::Reconnecting;
                self.events.push(Event::Reconnecting { reason });
            }
            return;
        }
        if !matches!(
            self.phase,
            Phase::Ended | Phase::Desynced | Phase::Stopped | Phase::Closed
        ) {
            self.phase = Phase::Closed;
        }
        self.events.push(Event::Closed { reason });
    }

    /// Opens the socket again when a reconnection attempt is due, asking for the commits
    /// after the last one received.
    fn retry(&mut self) {
        let since = self.received;
        let Transport::Remote(remote) = &mut self.transport else {
            return;
        };
        if self.phase != Phase::Reconnecting || remote.retry_at.is_none_or(|at| Instant::now() < at)
        {
            return;
        }
        remote.retry_at = None;
        remote.link = remote.net.link(&remote.room, &remote.token, since);
    }

    fn stop(&mut self, reason: String) {
        if !matches!(self.phase, Phase::Ended | Phase::Desynced | Phase::Closed) {
            self.phase = Phase::Stopped;
        }
        self.events.push(Event::Stopped { reason });
    }

    fn receive(&mut self, message: ServerMsg) {
        match message {
            ServerMsg::Welcome { you, room } => {
                self.you = Some(you);
                self.room = Some(room.clone());
                self.events.push(Event::Room(room));
            }
            ServerMsg::Room { room } => {
                self.room = Some(room.clone());
                self.events.push(Event::Room(room));
            }
            ServerMsg::Start { config, you, save } => {
                if save.is_some() {
                    return self.stop(
                        "the match continues from a saved game, which this client cannot load"
                            .into(),
                    );
                }
                self.you = Some(you.clone());
                self.ready_sent = false;
                self.paused = None;
                self.waiting = (Vec::new(), 0);
                if self.phase == Phase::Reconnecting && self.config.as_ref() == Some(&config) {
                    // The same match: the world goes on from where it was.
                    if let Transport::Remote(remote) = &mut self.transport {
                        remote.attempts = 0;
                    }
                    self.phase = Phase::Starting;
                    self.events.push(Event::Reconnected);
                    let clock = self.clock;
                    self.try_ready(clock);
                    return;
                }
                self.setup = Some(config.setup());
                self.config = Some(config.clone());
                self.commits.clear();
                self.received = 0;
                self.empty_through = 0;
                self.identity = None;
                self.phase = Phase::Loading;
                self.events.push(Event::Started { config, you });
            }
            ServerMsg::Go { tick } => {
                if matches!(self.phase, Phase::Loading | Phase::Starting) {
                    self.phase = Phase::Playing;
                }
                self.events.push(Event::Go { tick });
            }
            ServerMsg::History { through, commits } => {
                if self.config.is_none() {
                    return;
                }
                for commit in commits {
                    if commit.tick > self.received && commit.tick <= through {
                        self.commits.insert(commit.tick, commit);
                    }
                }
                if through > self.received {
                    self.received = through;
                    self.empty_through = self.empty_through.max(through);
                }
                if !self.caught_up(self.clock) {
                    self.catch_up_from.get_or_insert(self.clock);
                }
            }
            ServerMsg::Waiting { players, seconds } => {
                self.waiting = (players.clone(), seconds);
                self.events.push(Event::Waiting { players, seconds });
            }
            ServerMsg::Paused {
                player,
                name,
                pauses_left,
            } => {
                self.paused = Some(name.clone());
                self.events.push(Event::Paused {
                    player,
                    name,
                    pauses_left,
                });
            }
            ServerMsg::Resuming { seconds } => self.events.push(Event::Resuming { seconds }),
            ServerMsg::Resumed {} => {
                self.paused = None;
                self.events.push(Event::Resumed);
            }
            ServerMsg::Kicked {} => {
                if let Transport::Remote(remote) = &mut self.transport {
                    remote.closing = true;
                }
                self.events.push(Event::Kicked);
            }
            // The others dropped this seat; the socket closes and the session rejoins.
            ServerMsg::Dropped {} => {}
            ServerMsg::Commit { tick, slots } => {
                if self.config.is_none() || tick <= self.received {
                    return;
                }
                if tick != self.received + 1 {
                    return self.stop(format!(
                        "beats {} to {} were committed before this seat connected; \
                         the server does not replay them",
                        self.received + 1,
                        tick - 1
                    ));
                }
                self.received = tick;
                self.commits.insert(tick, Commit { tick, slots });
            }
            ServerMsg::HashOk { tick } => {
                self.last_hash_ok = Some(tick);
                self.events.push(Event::HashOk { tick });
            }
            ServerMsg::Desync { tick, hashes } => {
                self.desync = Some(tick);
                self.phase = Phase::Desynced;
                self.commits.clear();
                self.events.push(Event::Desync { tick, hashes });
            }
            ServerMsg::Ended { outcome, replay_id } => {
                self.phase = Phase::Ended;
                self.events.push(Event::Ended { outcome, replay_id });
            }
            ServerMsg::Chat { message } => self.events.push(Event::Chat(message)),
            ServerMsg::Error { code, message } => {
                let mismatch = code == "CONTENT_MISMATCH";
                self.events.push(Event::Error { code, message });
                if mismatch {
                    self.stop("players run different engine builds or content".into());
                }
            }
            ServerMsg::Restart { .. } => {
                self.stop("the host restarted the match, which this client does not support".into())
            }
            ServerMsg::Load { .. } => {
                self.stop("the host loaded a saved game, which this client cannot load".into())
            }
            // Answered on the network thread; never forwarded.
            ServerMsg::LatencyProbe { .. } => {}
        }
    }
}

impl Drop for Session {
    /// A session given up during a match leaves it (protocol 2: the seat is dropped at
    /// once and cannot rejoin); a game that crashes leaves its seat waiting instead.
    fn drop(&mut self) {
        let during = matches!(
            self.phase,
            Phase::Loading | Phase::Starting | Phase::Playing | Phase::Reconnecting
        );
        if let Transport::Remote(remote) = &self.transport
            && during
            && !remote.closing
        {
            drop(remote.net.leave_room(&remote.room, &remote.token));
        }
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_sim::{Slot, SlotKind};

    fn setup(players: u8) -> MatchSetup {
        MatchSetup {
            seed: 1,
            slots: (0..players)
                .map(|player| Slot {
                    difficulty: Default::default(),
                    player,
                    kind: if player == 0 {
                        SlotKind::Human
                    } else {
                        SlotKind::Ai
                    },
                    name: Some(format!("P{player}")),
                    team: None,
                    hero: None,
                    race: None,
                })
                .collect(),
        }
    }

    fn stop(actor: u32) -> Action {
        Action::Stop {
            actors: vec![actor],
        }
    }

    #[test]
    fn local_commands_apply_on_the_next_beat() {
        let mut session = Session::local(setup(2), Some(0));
        assert_eq!(session.player(), Some(0));
        let first = session.next_turn(0).expect("beat 1 is committed at once");
        assert_eq!(first.beat, 1);
        assert!(first.commands.is_empty());
        let commands = session.commands();
        std::thread::spawn(move || assert!(commands.send(stop(7))))
            .join()
            .unwrap();
        assert!(session.send(stop(8)));
        let turn = session.next_turn(1).unwrap();
        assert_eq!(turn.beat, 2);
        let scheduled: Vec<_> = turn
            .commands
            .iter()
            .map(|q| (q.tick, q.player, q.seq))
            .collect();
        assert_eq!(scheduled, [(1, 0, 0), (1, 0, 1)]);
        assert_eq!(turn.commands[1].action, stop(8));
        assert!(session.next_turn(2).unwrap().commands.is_empty());
        assert_eq!(session.status().delay, 1);
    }

    #[test]
    fn a_resumed_local_match_continues_from_its_clock() {
        // Far beyond how far a confirmation may reach past the last commit.
        let clock = crate::mailbox::MAX_CONFIRM_AHEAD * 3;
        let mut session = Session::local_from(setup(2), Some(0), clock);
        let turn = session
            .next_turn(clock)
            .expect("the next beat is committed at once");
        assert_eq!(turn.beat, clock + 1);
        assert!(session.send(stop(3)));
        let turn = session.next_turn(clock + 1).unwrap();
        assert_eq!(turn.beat, clock + 2);
        assert_eq!(turn.commands.len(), 1);
        assert_eq!(turn.commands[0].tick, clock + 1);
        assert_eq!(session.commits.len(), 0, "no stale commits pile up");
    }

    #[test]
    fn committed_beats_play_out_after_the_match_ended() {
        // A seat that is behind when the server ends the match: beats 1–3 are committed,
        // it has simulated none of them when `ended` arrives.
        let net = Net::new(crate::ServerAddress::parse("http://127.0.0.1:9").unwrap()).unwrap();
        let (to_io, _sent) = tokio::sync::mpsc::unbounded_channel();
        let (from, from_io) = mpsc::channel();
        let remote = Remote {
            net,
            link: Link { to_io, from_io },
            room: "room".into(),
            token: "token".into(),
            closing: false,
            attempts: 0,
            retry_at: None,
        };
        let you = ClientIdentity {
            role: Role::Player,
            player: Some(1),
            name: "Slow".into(),
        };
        let mut session = Session::new(
            Transport::Remote(remote),
            Some(you.clone()),
            None,
            None,
            Phase::Connecting,
        );
        let config = MatchConfig {
            v: 1,
            room_id: "room".into(),
            map_id: "map".into(),
            map_revision: "1".into(),
            seed: 1,
            delay: 1,
            checksum_every: 0,
            tick_ms: TICK_MS,
            slots: setup(2).slots,
            colors: None,
            company: None,
            observer_delay: 0,
            queue: None,
        };
        let send = |message: ServerMsg| from.send(FromIo::Message(Box::new(message))).unwrap();
        send(ServerMsg::Start {
            config,
            you,
            save: None,
        });
        assert!(session.next_turn(0).is_none());
        session.ready(
            ContentIdentity::digest("test", [b"x".as_slice()]),
            "test",
            0,
        );
        send(ServerMsg::Go { tick: 0 });
        for tick in 1..=3 {
            send(ServerMsg::Commit {
                tick,
                slots: Vec::new(),
            });
        }
        send(ServerMsg::Ended {
            outcome: WireOutcome {
                winner: Some(0),
                defeated: vec![1],
            },
            replay_id: String::new(),
        });
        let beats: Vec<u32> = (0..5)
            .map_while(|clock| session.next_turn(clock).map(|t| t.beat))
            .collect();
        assert_eq!(session.status().phase, Phase::Ended);
        assert_eq!(beats, [1, 2, 3]);
        assert!(session.next_turn(3).is_none());
        assert!(session.status().stalled.is_none());
        assert!(
            session
                .poll()
                .iter()
                .any(|e| matches!(e, Event::Ended { .. })),
            "the end is still reported"
        );
    }

    #[test]
    fn a_seat_joining_a_match_under_way_catches_up_before_it_is_ready() {
        let net = Net::new(crate::ServerAddress::parse("http://127.0.0.1:9").unwrap()).unwrap();
        let (to_io, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let (from, from_io) = mpsc::channel();
        let mut session = Session::rejoining(net, Link { to_io, from_io }, "room", "token");
        let you = ClientIdentity {
            role: Role::Player,
            player: Some(1),
            name: "Back".into(),
        };
        let config = MatchConfig {
            v: 2,
            room_id: "room".into(),
            map_id: "map".into(),
            map_revision: "1".into(),
            seed: 1,
            delay: 2,
            checksum_every: 0,
            tick_ms: TICK_MS,
            slots: setup(2).slots,
            colors: None,
            company: None,
            observer_delay: 0,
            queue: None,
        };
        let send = |message: ServerMsg| from.send(FromIo::Message(Box::new(message))).unwrap();
        send(ServerMsg::Start {
            config,
            you,
            save: None,
        });
        let commit: Commit = serde_json::from_value(serde_json::json!({"tick": 20, "slots": [
            {"player": 0, "actions": [{"type": "stop", "actors": [4]}]},
        ]}))
        .unwrap();
        send(ServerMsg::History {
            through: 50,
            commits: vec![commit],
        });
        session.poll();
        session.ready(
            ContentIdentity::digest("test", [b"x".as_slice()]),
            "test",
            0,
        );
        let is_ready = |sent: &mut tokio::sync::mpsc::UnboundedReceiver<ToIo>| {
            std::iter::from_fn(|| sent.try_recv().ok())
                .any(|m| matches!(m, ToIo::Send(ClientMsg::Ready { .. })))
        };
        assert!(!is_ready(&mut sent), "still 50 beats behind");
        assert_eq!(session.status().catching_up, Some(0.0));
        let mut commands = Vec::new();
        for clock in 0..45 {
            let turn = session
                .next_turn(clock)
                .expect("history beats are all there");
            assert_eq!(turn.beat, clock + 1);
            commands.extend(turn.commands.iter().map(|c| c.tick));
            session.stepped(clock + 1, || 0);
            if clock + 1 < 42 {
                assert!(!is_ready(&mut sent), "not caught up at {}", clock + 1);
            }
        }
        assert_eq!(commands, [19], "the one commit with actions, at its beat");
        assert!(
            is_ready(&mut sent) || session.ready_sent,
            "ready once caught up"
        );
        assert_eq!(session.status().catching_up, None);
    }

    #[test]
    fn local_observers_send_nothing() {
        let mut session = Session::local(setup(2), None);
        assert!(!session.send(stop(1)));
        assert!(!session.commands().send(stop(1)));
        assert!(session.next_turn(0).is_some());
    }

    #[test]
    fn malformed_commits_are_skipped_identically() {
        let mut session = Session::local(setup(1), Some(0));
        let commit: Commit = serde_json::from_value(serde_json::json!({"tick": 1, "slots": [
            {"player": 0, "actions": [
                {"type": "teleport"},
                {"type": "stop", "actors": []},
                {"type": "stop", "actors": [4]},
            ]},
        ]}))
        .unwrap();
        let turn = session.turn(commit);
        assert_eq!(turn.commands.len(), 1);
        assert_eq!(turn.commands[0].seq, 2, "sequence numbers keep their place");
        let skipped: Vec<u32> = session
            .poll()
            .iter()
            .filter_map(|e| match e {
                Event::Skipped { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect();
        assert_eq!(skipped, [0, 1]);
    }
}
