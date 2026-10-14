//! The frozen start of a match: seed and player slots. Identical on every peer.

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlotKind {
    Human,
    Ai,
}

/// One player of a match (protocol `Slot`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    /// Zero-based; owner `player.{player + 1}`.
    pub player: u8,
    pub kind: SlotKind,
    pub name: Option<String>,
    pub team: Option<u8>,
    /// Chosen starting hero; the setup's default when absent.
    pub hero: Option<String>,
    /// The rules' default race when absent.
    pub race: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSetup {
    pub seed: u32,
    pub slots: Vec<Slot>,
}
