//! One seat in one match: what the game drives its simulation with.
//!
//! A networked session starts in the room's lobby (choices, start), turns into a match
//! when the server sends `start`, and from then on hands out the server's commits one beat
//! at a time. A local session (single-player, the old `MemoryChannel`) runs the same
//! mailbox in-process with an input delay of one beat, so local and networked play feed the
//! simulation identically.
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
    PlayerHash, Role, RoomView, ServerMsg, TICK_MS, WireAction, WireOutcome,
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
    /// The socket closed. A closed seat cannot rejoin a running match: the server does
    /// not replay the commits it missed.
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
}

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
        };
        Self::new(
            Transport::Remote(remote),
            Some(joined.you.clone()),
            Some(joined.room.clone()),
            None,
            Phase::Connecting,
        )
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

    /// `selectHero`: this seat's starting hero (while the room waits).
    pub fn select_hero(&self, hero: &str) {
        self.message(ClientMsg::SelectHero { hero: hero.into() });
    }

    /// `selectRace`: this seat's race; the hero resets to the race's default.
    pub fn select_race(&self, race: &str) {
        self.message(ClientMsg::SelectRace { race: race.into() });
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

    /// Closes the socket (the old match screen's Leave). The server drops this seat and
    /// the others play on.
    pub fn close(&mut self) {
        if let Transport::Remote(remote) = &self.transport {
            let _ = remote.link.to_io.send(ToIo::Close);
        }
    }

    // Match.

    /// Reports that this seat's world is built (`ready`): the engine `build` and content
    /// it simulates, and the world's clock (`World::tick()`). Players with different
    /// identities are refused by the server (`CONTENT_MISMATCH`). Confirmations start
    /// here. Spectators send nothing.
    pub fn ready(&mut self, content: ContentIdentity, build: &str, clock: u32) {
        self.clock = clock;
        let Transport::Remote(remote) = &self.transport else {
            return;
        };
        if self.phase != Phase::Loading {
            return;
        }
        if self.player().is_some() {
            let _ = remote.link.to_io.send(ToIo::Send(ClientMsg::Ready {
                content,
                build: build.into(),
            }));
            let _ = remote.link.to_io.send(ToIo::Clock {
                tick: clock,
                hash: None,
            });
        }
        self.phase = Phase::Starting;
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
        }
    }

    fn message(&self, message: ClientMsg) {
        if let Transport::Remote(remote) = &self.transport {
            let _ = remote.link.to_io.send(ToIo::Send(message));
        }
    }

    fn pump(&mut self) {
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
                FromIo::Closed(reason) => {
                    if !matches!(
                        self.phase,
                        Phase::Ended | Phase::Desynced | Phase::Stopped | Phase::Closed
                    ) {
                        self.phase = Phase::Closed;
                    }
                    self.events.push(Event::Closed { reason });
                }
            }
        }
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
                self.setup = Some(config.setup());
                self.config = Some(config.clone());
                self.commits.clear();
                self.received = 0;
                self.phase = Phase::Loading;
                self.events.push(Event::Started { config, you });
            }
            ServerMsg::Go { tick } => {
                if matches!(self.phase, Phase::Loading | Phase::Starting) {
                    self.phase = Phase::Playing;
                }
                self.events.push(Event::Go { tick });
            }
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
    fn drop(&mut self) {
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
