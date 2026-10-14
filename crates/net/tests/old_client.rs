//! A Rust client and the old TypeScript client (its own lobby and socket code, run by
//! `tests/ts/lobby_peer.mts`) in one room of the old match server. They share the lobby;
//! their simulations differ, so the server refuses to start them together
//! (`CONTENT_MISMATCH`, docs/decisions.md D10, D29).
//!
//! Skipped without the old repository, its `node_modules` and node (CI).

mod fixture;

use canopy_net::wire::{CreateRoom, Role};
use canopy_net::{ContentIdentity, Event, Phase};
use fixture::{OldServer, wait_for};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn shares_a_lobby_with_the_typescript_client() {
    let Some(server) = OldServer::start() else {
        return;
    };
    let net = server.net();
    let host = net
        .create_room(&CreateRoom {
            name: "Ann's room".into(),
            map_id: "lockstep-fixture".into(),
            map_revision: "1".into(),
            slot_count: 2,
            guest_name: "Ann".into(),
        })
        .wait()
        .unwrap();
    let mut session = net.enter(&host);
    let mut seen = Vec::new();

    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ts/lobby_peer.mts");
    let mut peer = Command::new("node")
        .args(["--import", "tsx", script])
        .arg(&server.repository)
        .arg(server.address.http(""))
        .arg(&host.room.id)
        .current_dir(&server.repository)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("run the TypeScript peer");
    let stdout = peer.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str(&line) {
                let _ = tx.send(value);
            }
        }
    });
    let mut ts = Peer {
        lines,
        seen: Vec::new(),
    };
    ts.wait("joining", &|l| {
        l["event"] == "joined" && l["you"]["player"] == 1 && l["you"]["role"] == "player"
    });
    // Each client answers the server's probes; the server measures both seats and both
    // clients see the same room.
    let room = wait_for(
        &mut session,
        &mut seen,
        "the TypeScript seat",
        |e| match e {
            Event::Room(room)
                if room.slots[1].name.as_deref() == Some("TS")
                    && room.slots.iter().all(|s| s.round_trip_ms.is_some()) =>
            {
                Some(room.clone())
            }
            _ => None,
        },
    );
    assert_eq!(room.slots[0].name.as_deref(), Some("Ann"));
    ts.wait("the Rust seat measured", &|l| {
        l["msg"]["type"] == "room"
            && l["msg"]["room"]["slots"][0]["name"] == "Ann"
            && l["msg"]["room"]["slots"][0]["roundTripMs"].is_number()
    });

    // The Rust host starts; both get the same frozen configuration.
    session.start_match();
    let config = wait_for(&mut session, &mut seen, "start", |e| match e {
        Event::Started { config, you } => {
            assert_eq!((you.role, you.player), (Role::Player, Some(0)));
            Some(config.clone())
        }
        _ => None,
    });
    ts.wait("start", &|l| l["msg"]["type"] == "start");
    let ts_config = &ts.seen.last().unwrap()["msg"]["config"];
    assert_eq!(*ts_config, serde_json::to_value(&config).unwrap());

    // Different engines: the server refuses the pair before any beat is committed.
    session.ready(
        ContentIdentity {
            abi: "canopy-test-1".into(),
            sha256: "0".repeat(64),
        },
        "canopy-net-test",
        0,
    );
    let code = wait_for(&mut session, &mut seen, "the refusal", |e| match e {
        Event::Error { code, .. } => Some(code.clone()),
        _ => None,
    });
    assert_eq!(code, "CONTENT_MISMATCH");
    session.poll();
    assert_eq!(session.phase(), Phase::Stopped);
    assert!(session.next_turn(0).is_none());
    ts.wait("the refusal", &|l| {
        l["msg"]["type"] == "error" && l["msg"]["code"] == "CONTENT_MISMATCH"
    });
    let _ = peer.kill();
    let _ = peer.wait();
}

/// What the TypeScript peer reported.
struct Peer {
    lines: mpsc::Receiver<Value>,
    seen: Vec<Value>,
}

impl Peer {
    fn wait(&mut self, what: &str, found: &dyn Fn(&Value) -> bool) {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| {
                    panic!(
                        "the TypeScript peer did not report {what}: {:#?}",
                        self.seen
                    )
                });
            let hit = found(&line);
            self.seen.push(line);
            if hit {
                return;
            }
        }
    }
}
