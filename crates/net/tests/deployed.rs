//! Read-only checks against the deployed match server over HTTPS and WSS with the
//! platform's certificate verifier. Needs the internet, so ignored by default:
//! `cargo test -p canopy-net --test deployed -- --ignored`.

use canopy_net::{Net, ServerAddress};

#[test]
#[ignore = "reaches the deployed server over the internet"]
fn deployed_server_answers_over_verified_tls() {
    let net = Net::new(ServerAddress::default()).expect("network thread");
    let health = net.health().wait().expect("health over HTTPS");
    assert!(health.ok);
    let rooms = net.rooms().wait().expect("room list over HTTPS");
    for room in rooms {
        assert!(!room.id.is_empty());
    }
}

#[test]
#[ignore = "reaches the deployed server over the internet"]
fn deployed_server_refuses_an_unknown_room() {
    let net = Net::new(ServerAddress::default()).expect("network thread");
    let error = net.room("no-such-room").wait().unwrap_err();
    assert_eq!(error.to_string(), "Room not found");
}

/// The match socket's TLS handshake succeeds; the server then refuses the upgrade for a
/// room that does not exist (no side effects on the deployed server).
#[test]
#[ignore = "reaches the deployed server over the internet"]
fn deployed_match_socket_completes_tls() {
    use canopy_net::{Event, Phase, wire};
    let net = Net::new(ServerAddress::default()).expect("network thread");
    let joined: wire::Joined = serde_json::from_value(serde_json::json!({
        "token": "not-a-token",
        "room": {"id": "no-such-room", "state": "waiting", "name": "", "mapId": "", "host": "",
                 "slots": [], "spectators": 0},
        "you": {"role": "player", "player": 0, "name": "probe"},
    }))
    .unwrap();
    let mut session = net.enter(&joined);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let reason = loop {
        assert!(std::time::Instant::now() < deadline, "no answer");
        if let Some(reason) = session.poll().into_iter().find_map(|e| match e {
            Event::Closed { reason } => Some(reason),
            _ => None,
        }) {
            break reason;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(session.phase(), Phase::Closed);
    assert!(!reason.contains("TLS"), "TLS failed: {reason}");
}
