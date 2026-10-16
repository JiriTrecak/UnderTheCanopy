//! Rust clients against the match server (`server/`, docs/server.md), run locally:
//! lobby, start, lockstep with simulations driven from the server's commits, desync
//! hashes, chat, spectators, disconnects and the end of a match.
//!
//! Needs node and the server's `node_modules` (`npm ci` in `server/`); skipped otherwise.

mod fixture;

use canopy_net::wire::{CreateRoom, JoinRoom, Joined, Role, SeatKind, WireOutcome};
use canopy_net::{ContentIdentity, Event, Net, Phase, Session};
use canopy_sim::{Queued, World};
use fixture::{MatchServer, march, positions, wait_for};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const BUILD: &str = "canopy-net-test";

fn identity(tag: &str) -> ContentIdentity {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(tag.as_bytes());
    ContentIdentity {
        abi: "canopy-test-1".into(),
        sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
    }
}

/// One client: its session, its world once started, and what it saw.
struct Peer {
    name: &'static str,
    session: Session,
    world: Option<World>,
    events: Vec<Event>,
    turns: BTreeMap<u32, Vec<Queued>>,
    fingerprints: BTreeMap<u32, u32>,
    content: ContentIdentity,
}

impl Peer {
    fn new(name: &'static str, net: &Net, joined: &Joined) -> Self {
        let mut peer = Self {
            name,
            session: net.enter(joined),
            world: None,
            events: Vec::new(),
            turns: BTreeMap::new(),
            fingerprints: BTreeMap::new(),
            content: identity("fixture"),
        };
        wait_for(&mut peer.session, &mut peer.events, "welcome", |e| {
            matches!(e, Event::Room(_)).then_some(())
        });
        peer
    }

    /// Takes events; builds the world on `start` and reports ready; simulates every beat
    /// already committed (at most `limit`).
    fn pump(&mut self, limit: u32) {
        for event in self.session.poll() {
            if let Event::Started { config, .. } = &event {
                let world = fixture::world(&config.setup());
                self.session
                    .ready(self.content.clone(), BUILD, world.tick());
                self.world = Some(world);
            }
            self.events.push(event);
        }
        let Some(world) = self.world.as_mut() else {
            return;
        };
        while world.tick() < limit {
            let Some(turn) = self.session.advance(world) else {
                break;
            };
            self.turns.insert(turn.beat, turn.commands);
            self.fingerprints.insert(world.tick(), world.fingerprint());
        }
    }

    fn tick(&self) -> u32 {
        self.world.as_ref().map_or(0, World::tick)
    }

    fn world(&self) -> &World {
        self.world.as_ref().expect("started")
    }

    fn hash_oks(&self) -> Vec<u32> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::HashOk { tick } => Some(*tick),
                _ => None,
            })
            .collect()
    }

    fn assert_no_trouble(&self) {
        for event in &self.events {
            assert!(
                !matches!(
                    event,
                    Event::Desync { .. }
                        | Event::Error { .. }
                        | Event::Stopped { .. }
                        | Event::Skipped { .. }
                        | Event::Malformed { .. }
                ),
                "{}: {event:?}",
                self.name
            );
        }
    }
}

/// Pumps every peer until each has simulated `beats` beats (or `done` says stop).
fn run(peers: &mut [&mut Peer], beats: u32, mut each: impl FnMut(&mut [&mut Peer])) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while peers.iter().any(|p| p.tick() < beats) {
        for peer in peers.iter_mut() {
            peer.pump(beats);
        }
        each(peers);
        assert!(
            Instant::now() < deadline,
            "stalled at {:?}",
            peers
                .iter()
                .map(|p| (p.name, p.tick(), p.session.status()))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_micros(200));
    }
}

fn create(net: &Net, name: &str) -> Joined {
    net.create_room(&CreateRoom {
        name: format!("{name}'s room"),
        map_id: "lockstep-fixture".into(),
        map_revision: "1".into(),
        slot_count: 2,
        guest_name: name.into(),
    })
    .wait()
    .expect("create a room")
}

fn join(net: &Net, room: &str, name: &str, role: Role) -> Joined {
    net.join_room(
        room,
        &JoinRoom {
            guest_name: name.into(),
            role,
        },
    )
    .wait()
    .expect("join the room")
}

fn start(peers: &mut [&mut Peer]) {
    peers[0].session.start_match();
    let deadline = Instant::now() + Duration::from_secs(20);
    while peers.iter().any(|p| p.world.is_none()) {
        for peer in peers.iter_mut() {
            peer.pump(0);
        }
        assert!(Instant::now() < deadline, "no start");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn two_players_and_two_computers_play_a_team_match() {
    let Some(server) = MatchServer::start() else {
        return;
    };
    let net = server.net();
    let host = net
        .create_room(&CreateRoom {
            name: "Ann's room".into(),
            map_id: "lockstep-fixture".into(),
            map_revision: "1".into(),
            slot_count: 4,
            guest_name: "Ann".into(),
        })
        .wait()
        .unwrap();
    let id = host.room.id.clone();
    let mut a = Peer::new("Ann", &net, &host);
    // Seats 1 and 2 go to the computer before Bo arrives, so Bo takes seat 3.
    a.session.set_seat(1, SeatKind::Ai);
    a.session.set_seat(2, SeatKind::Ai);
    wait_for(
        &mut a.session,
        &mut a.events,
        "computer seats",
        |e| match e {
            Event::Room(room) => (room
                .slots
                .iter()
                .filter(|s| s.kind == Some(SeatKind::Ai))
                .count()
                == 2)
                .then_some(()),
            _ => None,
        },
    );
    let guest = join(&net, &id, "Bo", Role::Player);
    assert_eq!(guest.you.player, Some(3));
    let mut b = Peer::new("Bo", &net, &guest);
    // Ann and Bo against the computers.
    a.session.select_team(None, 1);
    b.session.select_team(None, 1);
    a.session.select_team(Some(1), 2);
    a.session.select_team(Some(2), 2);
    wait_for(&mut a.session, &mut a.events, "teams", |e| match e {
        Event::Room(room) => {
            let teams: Vec<Option<u8>> = room.slots.iter().map(|s| s.team).collect();
            (teams == [Some(1), Some(2), Some(2), Some(1)]).then_some(())
        }
        _ => None,
    });

    start(&mut [&mut a, &mut b]);
    let config = a.session.config().unwrap().clone();
    assert_eq!(b.session.config(), Some(&config));
    let seats: Vec<(u8, canopy_sim::SlotKind, Option<u8>)> = config
        .slots
        .iter()
        .map(|s| (s.player, s.kind, s.team))
        .collect();
    use canopy_sim::SlotKind::{Ai, Human};
    assert_eq!(
        seats,
        [
            (0, Human, Some(1)),
            (1, Ai, Some(2)),
            (2, Ai, Some(2)),
            (3, Human, Some(1))
        ]
    );
    assert_eq!(config.colors.as_deref(), Some(&[0, 1, 2, 3][..]));

    // The computers play inside both simulations, so nothing waits for them and nothing
    // diverges. (This fixture gives an AI nothing to do; the simulation's AI matches test
    // what it does and that it is deterministic.)
    run(&mut [&mut a, &mut b], 400, |_| {});
    for peer in [&a, &b] {
        peer.assert_no_trouble();
    }
    assert_eq!(a.fingerprints, b.fingerprints);
    assert!(!a.hash_oks().is_empty());
    for peer in [&a, &b] {
        let computers: Vec<u8> = peer
            .world()
            .setup()
            .slots
            .iter()
            .filter(|s| s.kind == Ai)
            .map(|s| s.player)
            .collect();
        assert_eq!(computers, [1, 2], "{}", peer.name);
        assert!(
            !positions(peer.world(), 2).is_empty(),
            "computer bases are built"
        );
    }
}

#[test]
fn two_players_and_a_spectator_play_in_lockstep() {
    let Some(server) = MatchServer::start() else {
        return;
    };
    let net = server.net();
    let host = create(&net, "Ann");
    assert_eq!(host.you.player, Some(0));
    let id = host.room.id.clone();
    let guest = join(&net, &id, "Bo", Role::Player);
    assert_eq!(guest.you.player, Some(1));
    let watcher = join(&net, &id, "Cy", Role::Spectator);
    assert_eq!(watcher.you.role, Role::Spectator);
    let listed = net.rooms().wait().unwrap();
    assert!(listed.iter().any(|r| r.id == id), "the room is listed");

    let mut a = Peer::new("Ann", &net, &host);
    let mut b = Peer::new("Bo", &net, &guest);
    let mut c = Peer::new("Cy", &net, &watcher);

    // Lobby: the server measures both players through this client's probe echoes.
    wait_for(
        &mut a.session,
        &mut a.events,
        "measured round trips",
        |e| match e {
            Event::Room(room) => room
                .slots
                .iter()
                .all(|s| s.round_trip_ms.is_some())
                .then_some(()),
            _ => None,
        },
    );
    assert!(a.session.status().round_trip_ms.is_some());

    start(&mut [&mut a, &mut b, &mut c]);
    let config = a.session.config().unwrap().clone();
    assert_eq!(
        b.session.config(),
        Some(&config),
        "one frozen configuration"
    );
    assert_eq!(c.session.config(), Some(&config));
    assert_eq!(config.checksum_every, canopy_sim::fingerprint::EVERY);
    assert!((2..=40).contains(&config.delay));
    assert_eq!(c.session.player(), None);
    let a_start = positions(a.world(), 0);
    let b_start = positions(b.world(), 1);

    const BEATS: u32 = 240;
    let commands_b = b.session.commands();
    let mut sent = [false; 3];
    run(&mut [&mut a, &mut b, &mut c], BEATS, |peers| {
        let (a, rest) = peers.split_first_mut().unwrap();
        let b = &mut rest[0];
        if a.tick() >= 20 && !sent[0] {
            sent[0] = true;
            let order = march(a.world(), 0, 40, 40);
            assert!(a.session.send(order));
            a.session.chat("glhf");
        }
        if b.tick() >= 40 && !sent[1] {
            sent[1] = true;
            // From another thread, as the input thread would.
            let order = march(b.world(), 1, 20, 20);
            let commands = commands_b.clone();
            std::thread::spawn(move || assert!(commands.send(order)))
                .join()
                .unwrap();
        }
        if a.tick() >= 100 && !sent[2] {
            sent[2] = true;
            let stop = canopy_sim::Action::Stop {
                actors: vec![
                    a.world()
                        .state()
                        .entities
                        .iter()
                        .find(|e| e.unit.is_some())
                        .unwrap()
                        .id
                        .0,
                ],
            };
            assert!(a.session.send(stop));
        }
    });

    // Everyone simulated the same commands on the same beats and agrees on every beat.
    assert_eq!(a.turns, b.turns);
    assert_eq!(a.turns, c.turns);
    assert_eq!(a.fingerprints, b.fingerprints);
    assert_eq!(a.fingerprints, c.fingerprints);
    let commanded: Vec<(u32, u8)> = a
        .turns
        .iter()
        .flat_map(|(beat, commands)| commands.iter().map(move |q| (*beat, q.player)))
        .collect();
    assert_eq!(commanded.len(), 3, "{commanded:?}");
    for (beat, commands) in &a.turns {
        assert!(
            commands.iter().all(|q| q.tick + 1 == *beat),
            "beat {beat} runs on the step from tick {}",
            beat - 1
        );
    }
    assert_ne!(positions(a.world(), 0), a_start, "Ann's march took effect");
    assert_ne!(positions(b.world(), 1), b_start, "Bo's march took effect");

    // Settle: let the last hashes come back.
    let target = BEATS / 8 * 8;
    let deadline = Instant::now() + Duration::from_secs(10);
    while [&a, &b].iter().any(|p| !p.hash_oks().contains(&target)) {
        for peer in [&mut a, &mut b, &mut c] {
            peer.pump(BEATS);
        }
        assert!(Instant::now() < deadline, "no hashOk for {target}");
        std::thread::sleep(Duration::from_millis(2));
    }
    let expected: Vec<u32> = (1..=BEATS / 8).map(|n| n * 8).collect();
    assert_eq!(a.hash_oks(), expected, "the server agreed on every hash");
    for peer in [&a, &b, &c] {
        peer.assert_no_trouble();
        assert_eq!(peer.session.phase(), Phase::Playing);
        assert!(
            peer.events.iter().any(|e| matches!(e, Event::Chat(m) if m.text == "glhf" && m.name == "Ann" && m.player == Some(0))),
            "{} got the chat",
            peer.name
        );
    }
    let status = b.session.status();
    assert_eq!((status.clock, status.delay), (BEATS, config.delay));
    assert!(status.received >= BEATS);
    assert_eq!(status.last_hash_ok, Some(target));

    // The end: one report ends the room for everyone, and it leaves the list.
    a.session.report_outcome(
        WireOutcome {
            winner: Some(0),
            defeated: vec![1],
        },
        a.tick(),
        a.world().fingerprint(),
    );
    for peer in [&mut a, &mut b, &mut c] {
        let outcome = wait_for(&mut peer.session, &mut peer.events, "ended", |e| match e {
            Event::Ended { outcome, .. } => Some(outcome.clone()),
            _ => None,
        });
        assert_eq!(outcome.winner, Some(0));
        assert_eq!(peer.session.phase(), Phase::Ended);
    }
    assert!(!net.rooms().wait().unwrap().iter().any(|r| r.id == id));
}

#[test]
fn a_dropped_player_no_longer_holds_the_match() {
    let Some(server) = MatchServer::start() else {
        return;
    };
    let net = server.net();
    let host = create(&net, "Ann");
    let guest = join(&net, &host.room.id, "Bo", Role::Player);
    let mut a = Peer::new("Ann", &net, &host);
    let mut b = Peer::new("Bo", &net, &guest);
    start(&mut [&mut a, &mut b]);
    run(&mut [&mut a, &mut b], 40, |_| {});
    b.session.close();
    wait_for(&mut b.session, &mut b.events, "closed", |e| {
        matches!(e, Event::Closed { .. }).then_some(())
    });
    assert_eq!(b.session.phase(), Phase::Closed);
    assert!(b.session.next_turn(b.tick()).is_none());
    // The server stops waiting for Bo; Ann plays on, still hashed and confirmed.
    run(&mut [&mut a], 160, |_| {});
    let deadline = Instant::now() + Duration::from_secs(10);
    while !a.hash_oks().contains(&160) {
        a.pump(160);
        assert!(Instant::now() < deadline, "no hashOk after the drop");
        std::thread::sleep(Duration::from_millis(2));
    }
    a.assert_no_trouble();
    assert!(
        a.turns.values().flatten().all(|q| q.player == 0),
        "Bo's slot stays in commits, empty"
    );
}

#[test]
fn a_spectator_joining_a_running_match_cannot_catch_up() {
    let Some(server) = MatchServer::start() else {
        return;
    };
    let net = server.net();
    let host = create(&net, "Ann");
    let guest = join(&net, &host.room.id, "Bo", Role::Player);
    let mut a = Peer::new("Ann", &net, &host);
    let mut b = Peer::new("Bo", &net, &guest);
    start(&mut [&mut a, &mut b]);
    run(&mut [&mut a, &mut b], 30, |_| {});
    let late = join(&net, &host.room.id, "Cy", Role::Spectator);
    let mut c = Peer::new("Cy", &net, &late);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !c.events.iter().any(|e| matches!(e, Event::Stopped { .. })) {
        for peer in [&mut a, &mut b, &mut c] {
            peer.pump(400);
        }
        assert!(
            Instant::now() < deadline,
            "the late spectator was not stopped"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(c.session.phase(), Phase::Stopped);
    assert_eq!(c.tick(), 0, "nothing simulated from an incomplete history");
    a.assert_no_trouble();
}

#[test]
fn players_with_different_content_are_refused() {
    let Some(server) = MatchServer::start() else {
        return;
    };
    let net = server.net();
    let host = create(&net, "Ann");
    let guest = join(&net, &host.room.id, "Bo", Role::Player);
    let mut a = Peer::new("Ann", &net, &host);
    let mut b = Peer::new("Bo", &net, &guest);
    b.content = identity("other content");
    start(&mut [&mut a, &mut b]);
    for peer in [&mut a, &mut b] {
        let code = wait_for(
            &mut peer.session,
            &mut peer.events,
            "a refusal",
            |e| match e {
                Event::Error { code, .. } => Some(code.clone()),
                _ => None,
            },
        );
        assert_eq!(code, "CONTENT_MISMATCH");
        peer.pump(10);
        assert_eq!(peer.session.phase(), Phase::Stopped);
        assert_eq!(peer.tick(), 0);
    }
}
