//! The frozen wire format of the match server (docs/decisions.md D10).
//!
//! Every type mirrors the old game's `src/shared/net/wire.ts` and `src/shared/match/match.ts`.
//! Client messages serialize to exactly the JSON the TypeScript client sends, key order
//! included (`JSON.stringify` writes keys in insertion order, which serde reproduces with
//! the field order below). Server messages are read leniently: unknown fields are ignored,
//! because the server spreads content records into some of them (`heroes.offset`).
//!
//! Quirks of the frozen protocol, kept as they are:
//! - The effective input delay is `delay + 1` beats: confirmations run `delay` beats ahead
//!   of the clock, and a command's bundle sits one beat past them (raising `through`).
//! - The server ignores a whole `turn` whose `through` does not rise, bundles included,
//!   and one with any bundle past `through + delay + 1` (after first discarding bundles
//!   past `through + delay + 2`).
//! - Committed actions are forwarded as the sender wrote them, so a peer may receive one
//!   it cannot read ([`WireAction::Unreadable`]).
//! - Chat is delivered only while a match plays; the server publishes measured round trips
//!   in room views only while the room waits; `tick` is absent from a room view until a
//!   match starts; probes go to players only and are echoed only for nonces of at most
//!   128 UTF-16 units.
//! - A seat that connects to a running match (spectator or reconnect) receives `start`
//!   but no earlier commits, so it cannot rebuild the match.

use canopy_sim::{Action, Slot};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// Milliseconds per lockstep beat (`TICK_MS`); the simulation's 40 Hz tick (D3).
pub const TICK_MS: u32 = 25;
/// The fallback input delay in beats while round trips are unmeasured (`COMMAND_DELAY`).
pub const COMMAND_DELAY: u32 = 8;
/// Beats between desync hashes (`CHECKSUM_EVERY`); equals `canopy_sim::fingerprint::EVERY`.
pub const CHECKSUM_EVERY: u32 = 8;
/// Most bundles one `turn` may carry, and most actions per bundle (server limits).
pub const MAX_BUNDLES: usize = 64;
pub const MAX_BUNDLE_ACTIONS: usize = 64;

/// How a match ended (`WireOutcome`): zero-based player slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireOutcome {
    pub winner: Option<u8>,
    pub defeated: Vec<u8>,
}

/// One player's actions for one beat, sent in a `turn` (`Bundle`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub tick: u32,
    pub actions: Vec<Action>,
}

/// An action as committed by the server. Every peer receives the same bytes, so an action
/// this client cannot read (another client's extension the server let through) is kept as
/// it came and skipped identically by every Rust peer.
#[derive(Debug, Clone, PartialEq)]
pub enum WireAction {
    Valid(Action),
    Unreadable { json: Value, error: String },
}

impl Serialize for WireAction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Valid(action) => action.serialize(serializer),
            Self::Unreadable { json, .. } => json.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for WireAction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let json = Value::deserialize(deserializer)?;
        Ok(match Action::deserialize(&json) {
            Ok(action) => Self::Valid(action),
            Err(error) => Self::Unreadable {
                json,
                error: error.to_string(),
            },
        })
    }
}

/// One slot's actions in a commit (`CommitSlot`). Their index is the command's sequence
/// number, as the old client enqueued them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitSlot {
    pub player: u8,
    pub actions: Vec<WireAction>,
}

/// Everything the server decided for one beat (`Commit`): every slot of the match in
/// player order, dropped players included with no actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub tick: u32,
    pub slots: Vec<CommitSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Player,
    Spectator,
}

/// Who this connection is (`ClientIdentity`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientIdentity {
    pub role: Role,
    /// The zero-based slot of a player; absent for spectators.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<u8>,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoomState {
    Waiting,
    Playing,
    Ended,
    Desynced,
}

/// What holds a lobby seat (docs/server.md, "Lobby seats").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeatKind {
    /// A human may join.
    Open,
    /// Locked by the host: nobody plays it.
    Closed,
    /// The computer plays it.
    Ai,
    /// A human has joined.
    Human,
}

/// A seat in a room view. `name` is `null` for an empty seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seat {
    pub player: u8,
    pub name: Option<String>,
    /// Absent from servers before lobby seats: then a named seat is a human's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SeatKind>,
    /// 1 to 8; every seat its own team by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<u8>,
    /// Palette colour (index into `config/players.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub race: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero: Option<String>,
    /// Server-measured round trip of this seat's connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_trip_ms: Option<u32>,
}

/// What the lobby shows of a room (`RoomView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomView {
    pub id: String,
    pub state: RoomState,
    pub name: String,
    pub map_id: String,
    /// The host's name.
    pub host: String,
    pub slots: Vec<Seat>,
    pub spectators: u32,
    /// The last committed beat, once playing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick: Option<u32>,
    /// The input buffer the match uses (or would use if started now).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_delay_ms: Option<u32>,
}

/// The frozen start of a networked match (`MatchConfig`), identical on every peer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchConfig {
    /// Always 1.
    pub v: u32,
    pub room_id: String,
    pub map_id: String,
    pub map_revision: String,
    pub seed: u32,
    /// Input delay in beats.
    pub delay: u32,
    pub checksum_every: u32,
    pub tick_ms: u32,
    pub slots: Vec<Slot>,
    /// Palette colour (index into `config/players.json`) of each slot, in slot order; from
    /// servers with lobby seats. Outside `Slot`, which 0.3.1 and earlier read strictly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<u8>>,
    /// Campaign carry-over of the old game; never set by the match server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<Value>,
}

impl MatchConfig {
    /// The simulation's start: seed and slots, exactly as the server froze them.
    pub fn setup(&self) -> canopy_sim::MatchSetup {
        canopy_sim::MatchSetup {
            seed: self.seed,
            slots: self.slots.clone(),
        }
    }
}

/// Body of `POST /api/rooms` (`CreateRoom`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoom {
    pub name: String,
    pub map_id: String,
    pub map_revision: String,
    /// 2 to 8 seats (the server clamps).
    pub slot_count: u32,
    pub guest_name: String,
}

/// Body of `POST /api/rooms/:id/join` (`JoinRoom`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRoom {
    pub guest_name: String,
    pub role: Role,
}

/// Reply to creating or joining a room (`JoinResult`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Joined {
    pub token: String,
    pub room: RoomView,
    pub you: ClientIdentity,
}

/// `GET /api/health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub ok: bool,
    pub version: String,
}

/// What a client reports when it is loaded: the engine build and content it simulates
/// with. The server refuses to start players whose identities differ (`CONTENT_MISMATCH`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentIdentity {
    pub abi: String,
    /// 64 lowercase hex digits.
    pub sha256: String,
}

/// The engine build a Rust client reports in `ready` unless the game passes its own. It
/// never equals an old client's build, so mixed rooms are refused before they could
/// desync (D10).
pub const BUILD: &str = concat!("canopy-", env!("CARGO_PKG_VERSION"));
/// The `abi` a Rust client reports: the simulation's command and content interface.
pub const ABI: &str = "canopy-sim-1";

impl ContentIdentity {
    /// The identity of the content a match is simulated with: SHA-256 over each part
    /// (length-prefixed, in the given order), e.g. the bytes of every content file read.
    pub fn digest<'a>(abi: &str, parts: impl IntoIterator<Item = &'a [u8]>) -> Self {
        use sha2::Digest;
        let mut hash = sha2::Sha256::new();
        for part in parts {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        Self {
            abi: abi.into(),
            sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
}

/// A chat line (`ChatMessage`). `player` is `null` for spectators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub name: String,
    pub player: Option<u8>,
    pub text: String,
}

/// One player's hash in a `desync` report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerHash {
    pub player: u8,
    pub checksum: i64,
}

/// Client → server (`ClientMsg`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMsg {
    LatencyReply {
        id: String,
    },
    Chat {
        text: String,
    },
    /// The sender's own seat's hero, or the host's for computer seat `player`.
    SelectHero {
        hero: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        player: Option<u8>,
    },
    SelectRace {
        race: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        player: Option<u8>,
    },
    SelectTeam {
        team: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        player: Option<u8>,
    },
    SelectColor {
        color: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        player: Option<u8>,
    },
    /// Host: open, close or hand a seat to the computer (never `Human`).
    SetSeat {
        player: u8,
        seat: SeatKind,
    },
    StartMatch,
    /// Declared by the protocol; the server ignores it and the old client never sends it.
    Hello {
        token: String,
    },
    Ready {
        content: ContentIdentity,
        build: String,
    },
    Turn {
        through: u32,
        bundles: Vec<Bundle>,
    },
    Hash {
        tick: u32,
        checksum: u32,
    },
    Ended {
        outcome: WireOutcome,
        tick: u32,
        checksum: u32,
    },
    LoadSave {
        save: Value,
    },
    Restart,
}

/// Server → client (`ServerMsg`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMsg {
    LatencyProbe {
        id: String,
    },
    Chat {
        message: ChatMessage,
    },
    Welcome {
        you: ClientIdentity,
        room: RoomView,
    },
    Room {
        room: RoomView,
    },
    Start {
        config: MatchConfig,
        you: ClientIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        save: Option<Value>,
    },
    Restart {
        config: MatchConfig,
        you: ClientIdentity,
    },
    Go {
        tick: u32,
    },
    Commit {
        tick: u32,
        slots: Vec<CommitSlot>,
    },
    HashOk {
        tick: u32,
    },
    Desync {
        tick: u32,
        hashes: Vec<PlayerHash>,
    },
    Ended {
        outcome: WireOutcome,
        #[serde(rename = "replayId")]
        replay_id: String,
    },
    Load {
        save: Value,
        you: ClientIdentity,
    },
    Error {
        code: String,
        message: String,
    },
}

impl ClientMsg {
    /// The text frame the old client would send.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("client messages always serialize")
    }
}

impl ServerMsg {
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

/// `encodeURIComponent`: everything but `A–Z a–z 0–9 - _ . ! ~ * ' ( )` is percent-encoded
/// as UTF-8.
pub fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The server's chat cleanup (`chatText`): control characters become spaces, then the text
/// is trimmed and cut to 300 UTF-16 units; empty text is not sent. Applied by the server;
/// exposed so the UI can preview what others will see.
pub fn chat_text(text: &str) -> Option<String> {
    let replaced: String = text
        .chars()
        .map(|c| {
            if (c as u32) < 0x20 || c as u32 == 0x7f {
                ' '
            } else {
                c
            }
        })
        .collect();
    let trimmed = js_trim(&replaced);
    let mut units = 0;
    let mut out = String::new();
    for c in trimmed.chars() {
        units += c.len_utf16();
        if units > 300 {
            break;
        }
        out.push(c);
    }
    (!out.is_empty()).then_some(out)
}

/// JavaScript's `trim` (whitespace and line terminators, which Rust's `trim` also covers
/// apart from U+FEFF).
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_sim::SlotKind;
    use serde_json::json;

    /// The exact text the TypeScript client sends for each client message (written from
    /// the object literals in the old `ws.ts`, `lockstep.ts`, `runtime.ts`, `session.ts`
    /// and `GameApp.ts`, serialized by `JSON.stringify`).
    #[test]
    fn client_messages_match_the_typescript_text() {
        let move_action: Action = serde_json::from_value(
            json!({"type": "move", "actors": [3], "destination": {"x": 10, "y": 20}}),
        )
        .unwrap();
        let cases = [
            (
                ClientMsg::LatencyReply { id: "n-1".into() },
                r#"{"type":"latencyReply","id":"n-1"}"#,
            ),
            (
                ClientMsg::Chat { text: "gg".into() },
                r#"{"type":"chat","text":"gg"}"#,
            ),
            (
                ClientMsg::SelectHero {
                    hero: "unit.h".into(),
                    player: None,
                },
                r#"{"type":"selectHero","hero":"unit.h"}"#,
            ),
            (
                ClientMsg::SelectRace {
                    race: "race.r".into(),
                    player: None,
                },
                r#"{"type":"selectRace","race":"race.r"}"#,
            ),
            (
                ClientMsg::SelectRace {
                    race: "race.r".into(),
                    player: Some(2),
                },
                r#"{"type":"selectRace","race":"race.r","player":2}"#,
            ),
            (
                ClientMsg::SelectTeam {
                    team: 2,
                    player: None,
                },
                r#"{"type":"selectTeam","team":2}"#,
            ),
            (
                ClientMsg::SelectColor {
                    color: 5,
                    player: Some(3),
                },
                r#"{"type":"selectColor","color":5,"player":3}"#,
            ),
            (
                ClientMsg::SetSeat {
                    player: 1,
                    seat: SeatKind::Ai,
                },
                r#"{"type":"setSeat","player":1,"seat":"ai"}"#,
            ),
            (ClientMsg::StartMatch, r#"{"type":"startMatch"}"#),
            (
                ClientMsg::Hello { token: "t".into() },
                r#"{"type":"hello","token":"t"}"#,
            ),
            (
                ClientMsg::Ready {
                    content: ContentIdentity {
                        abi: "abi-1".into(),
                        sha256: "ab".repeat(32),
                    },
                    build: "build-1".into(),
                },
                &format!(
                    r#"{{"type":"ready","content":{{"abi":"abi-1","sha256":"{}"}},"build":"build-1"}}"#,
                    "ab".repeat(32)
                ),
            ),
            (
                ClientMsg::Turn {
                    through: 12,
                    bundles: vec![],
                },
                r#"{"type":"turn","through":12,"bundles":[]}"#,
            ),
            (
                ClientMsg::Turn {
                    through: 13,
                    bundles: vec![Bundle {
                        tick: 13,
                        actions: vec![move_action],
                    }],
                },
                r#"{"type":"turn","through":13,"bundles":[{"tick":13,"actions":[{"type":"move","actors":[3],"destination":{"x":10,"y":20}}]}]}"#,
            ),
            (
                ClientMsg::Hash {
                    tick: 16,
                    checksum: 4_000_000_000,
                },
                r#"{"type":"hash","tick":16,"checksum":4000000000}"#,
            ),
            (
                ClientMsg::Ended {
                    outcome: WireOutcome {
                        winner: Some(1),
                        defeated: vec![0],
                    },
                    tick: 900,
                    checksum: 7,
                },
                r#"{"type":"ended","outcome":{"winner":1,"defeated":[0]},"tick":900,"checksum":7}"#,
            ),
            (
                ClientMsg::Ended {
                    outcome: WireOutcome {
                        winner: None,
                        defeated: vec![],
                    },
                    tick: 1,
                    checksum: 0,
                },
                r#"{"type":"ended","outcome":{"winner":null,"defeated":[]},"tick":1,"checksum":0}"#,
            ),
            (
                ClientMsg::LoadSave {
                    save: json!({"v": 1}),
                },
                r#"{"type":"loadSave","save":{"v":1}}"#,
            ),
            (ClientMsg::Restart, r#"{"type":"restart"}"#),
        ];
        for (message, text) in cases {
            assert_eq!(message.to_json(), text);
            let back: ClientMsg = serde_json::from_str(text).unwrap();
            assert_eq!(back, message);
        }
    }

    fn room_json() -> Value {
        // As `HostedMatch.view()` writes it.
        json!({
            "id": "1", "state": "waiting", "name": "Ann's room", "mapId": "map.a", "host": "Ann",
            "slots": [
                {"player": 0, "name": "Ann", "race": "race.a", "hero": "unit.h1", "roundTripMs": 31},
                {"player": 1, "name": null},
            ],
            "spectators": 0,
            "inputDelayMs": 200,
        })
    }

    fn config_json() -> Value {
        json!({
            "v": 1, "roomId": "1", "mapId": "map.a", "mapRevision": "r1", "seed": 3_000_000_000u32,
            "delay": 8, "checksumEvery": 8, "tickMs": 25,
            "slots": [
                {"player": 0, "kind": "human", "name": "Ann", "race": "race.a", "hero": "unit.h1"},
                {"player": 1, "kind": "human", "name": "Bo", "race": "race.a"},
            ],
        })
    }

    /// Every server message as `host.ts`, `room.ts` and `server/index.ts` write it.
    #[test]
    fn server_messages_read_the_typescript_shapes() {
        let player = json!({"role": "player", "player": 1, "name": "Bo"});
        let spectator = json!({"role": "spectator", "name": "Cy"});
        let cases = [
            json!({"type": "latencyProbe", "id": "8f0c"}),
            json!({"type": "chat", "message": {"name": "Ann", "player": 0, "text": "hi"}}),
            json!({"type": "chat", "message": {"name": "Cy", "player": null, "text": "hi"}}),
            json!({"type": "welcome", "you": player, "room": room_json()}),
            json!({"type": "room", "room": room_json()}),
            json!({"type": "start", "config": config_json(), "you": player}),
            json!({"type": "start", "config": config_json(), "you": spectator, "save": {"v": 9}}),
            json!({"type": "restart", "config": config_json(), "you": player}),
            json!({"type": "go", "tick": 1}),
            json!({"type": "commit", "tick": 5, "slots": [
                {"player": 0, "actions": [{"type": "stop", "actors": [4]}, {"type": "ping"}]},
                {"player": 1, "actions": []},
            ]}),
            json!({"type": "hashOk", "tick": 8}),
            json!({"type": "desync", "tick": 16, "hashes": [{"player": 0, "checksum": 1}, {"player": 1, "checksum": -2}]}),
            json!({"type": "ended", "outcome": {"winner": null, "defeated": []}, "replayId": "r"}),
            json!({"type": "load", "save": {"v": 9}, "you": player}),
            json!({"type": "error", "code": "CONTENT_MISMATCH", "message": "Players have different engine or published ability content."}),
            json!({"type": "error", "code": "bad_json", "message": "bad json"}),
        ];
        for case in cases {
            let message: ServerMsg =
                serde_json::from_value(case.clone()).unwrap_or_else(|e| panic!("{case}: {e}"));
            let mut expected = case.clone();
            // Unknown record fields are dropped; everything else survives a round trip.
            for path in ["/room/races/race.a/heroes", "/room/heroes"] {
                if let Some(heroes) = expected.pointer_mut(path) {
                    heroes.as_object_mut().unwrap().remove("offset");
                }
            }
            assert_eq!(serde_json::to_value(&message).unwrap(), expected);
        }
    }

    #[test]
    fn config_becomes_the_simulation_setup() {
        let config: MatchConfig = serde_json::from_value(config_json()).unwrap();
        let setup = config.setup();
        assert_eq!(setup.seed, 3_000_000_000);
        assert_eq!(setup.slots[0].kind, SlotKind::Human);
        assert_eq!(setup.slots[0].hero.as_deref(), Some("unit.h1"));
        assert_eq!(setup.slots[1].hero, None);
    }

    #[test]
    fn unreadable_committed_actions_are_kept_verbatim() {
        let commit: Commit = serde_json::from_value(json!({"tick": 2, "slots": [
            {"player": 0, "actions": [{"type": "fly", "to": 1}, {"type": "noop"}]},
        ]}))
        .unwrap();
        let actions = &commit.slots[0].actions;
        assert!(
            matches!(&actions[0], WireAction::Unreadable { json, .. } if json["type"] == "fly")
        );
        assert_eq!(actions[1], WireAction::Valid(Action::Noop));
    }

    #[test]
    fn lobby_bodies_match_the_typescript_text() {
        let create = CreateRoom {
            name: "Ann's room".into(),
            map_id: "map.a".into(),
            map_revision: "r1".into(),
            slot_count: 2,
            guest_name: "Ann".into(),
        };
        assert_eq!(
            serde_json::to_string(&create).unwrap(),
            r#"{"name":"Ann's room","mapId":"map.a","mapRevision":"r1","slotCount":2,"guestName":"Ann"}"#
        );
        let join = JoinRoom {
            guest_name: "Bo".into(),
            role: Role::Spectator,
        };
        assert_eq!(
            serde_json::to_string(&join).unwrap(),
            r#"{"guestName":"Bo","role":"spectator"}"#
        );
        let joined: Joined = serde_json::from_value(
            json!({"token": "t", "room": room_json(), "you": {"role": "player", "player": 0, "name": "Ann"}}),
        )
        .unwrap();
        assert_eq!(joined.you.player, Some(0));
        assert_eq!(joined.room.slots[0].round_trip_ms, Some(31));
    }

    #[test]
    fn content_identity_is_a_sha256_the_server_accepts() {
        let a = ContentIdentity::digest(ABI, [b"game".as_slice(), b"maps".as_slice()]);
        assert_eq!(a.sha256.len(), 64);
        assert!(
            a.sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        let b = ContentIdentity::digest(ABI, [b"gam".as_slice(), b"emaps".as_slice()]);
        assert_ne!(a, b, "parts are delimited");
    }

    #[test]
    fn uri_components_encode_like_javascript() {
        assert_eq!(encode_uri_component("a b/c?d=é"), "a%20b%2Fc%3Fd%3D%C3%A9");
        assert_eq!(encode_uri_component("A-z_0.!~*'()"), "A-z_0.!~*'()");
    }

    #[test]
    fn chat_text_matches_the_server_cleanup() {
        assert_eq!(chat_text("  hi\tthere\n "), Some("hi there".into()));
        assert_eq!(chat_text("\u{7}"), None);
        assert_eq!(chat_text(&"x".repeat(400)).unwrap().len(), 300);
        assert_eq!(chat_text(&"😀".repeat(200)).unwrap().chars().count(), 150);
    }
}
