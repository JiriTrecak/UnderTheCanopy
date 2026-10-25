//! The ranked and unranked 1v1 queues (D107, docs/server.md "Queues and matchmaking"):
//! what the server offers, searching with a ticket, and the room a match puts both
//! players in. The room is then entered like any other ([`crate::Net::enter`]); it starts
//! by itself once both players are connected.

use crate::wire::{Joined, PROTOCOL};
use serde::{Deserialize, Serialize};

/// Which queue: ranked games move the players' ratings, unranked ones record nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueueMode {
    Ranked,
    Unranked,
}

/// A race a queue takes (the ladder's races) or a map of its pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub id: String,
    pub name: String,
}

/// How many players search each queue now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Searching {
    pub ranked: u32,
    pub unranked: u32,
}

/// `GET /api/queue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueInfo {
    pub pool: Vec<Named>,
    pub races: Vec<Named>,
    #[serde(default)]
    pub searching: Searching,
}

/// The race played from a queue: the ladder's race id, or `"random"`.
pub const RANDOM: &str = "random";

/// Body of `POST /api/queue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnterQueue {
    pub protocol: u32,
    pub player_id: String,
    pub name: String,
    pub mode: QueueMode,
    pub race: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub veto: Option<String>,
}

impl EnterQueue {
    pub fn new(
        player_id: &str,
        name: &str,
        mode: QueueMode,
        race: &str,
        veto: Option<&str>,
    ) -> Self {
        Self {
            protocol: PROTOCOL,
            player_id: player_id.into(),
            name: name.into(),
            mode,
            race: race.into(),
            veto: veto.map(Into::into),
        }
    }
}

/// Reply to `POST /api/queue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub ticket: String,
}

/// `GET /api/queue/:ticket`: still searching, or matched into a room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum TicketState {
    /// `range`: the rating difference the search accepts now.
    Searching {
        seconds: u32,
        range: u32,
    },
    Matched(Box<Joined>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn queue_messages_read_and_write_the_server_shapes() {
        let body = EnterQueue::new(
            "Jiri1",
            "Jiri",
            QueueMode::Ranked,
            RANDOM,
            Some("frozen-dream"),
        );
        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!({"protocol": 2, "playerId": "Jiri1", "name": "Jiri", "mode": "ranked", "race": "random", "veto": "frozen-dream"})
        );
        let info: QueueInfo = serde_json::from_value(json!({
            "pool": [{"id": "frozen-dream", "name": "Frozen Dream"}],
            "races": [{"id": "ants", "name": "Ants"}],
            "searching": {"ranked": 3, "unranked": 1},
        }))
        .unwrap();
        assert_eq!(info.searching.ranked, 3);
        let searching: TicketState =
            serde_json::from_value(json!({"state": "searching", "seconds": 12, "range": 450}))
                .unwrap();
        assert_eq!(
            searching,
            TicketState::Searching {
                seconds: 12,
                range: 450
            }
        );
        let matched: TicketState = serde_json::from_value(json!({
            "state": "matched", "token": "t",
            "room": {"id": "7", "state": "waiting", "name": "Ranked 1v1", "mapId": "frozen-dream", "host": "Ann",
                     "slots": [], "spectators": 0, "queue": "ranked"},
            "you": {"role": "player", "player": 1, "name": "Bo"},
        }))
        .unwrap();
        let TicketState::Matched(joined) = matched else {
            panic!("matched");
        };
        assert_eq!(joined.room.queue, Some(QueueMode::Ranked));
        assert_eq!(joined.you.player, Some(1));
    }
}
