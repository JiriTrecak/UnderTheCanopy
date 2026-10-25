//! The frozen start of a match: seed and player slots. Identical on every peer.

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SlotKind {
    Human,
    Ai,
}

/// How hard a computer player plays, or a campaign mission is.
///
/// Every level plays the same AI for now (Jiri, 2026-10-25: push one AI first, then decide
/// what each level changes): the level travels through setups, saves, replays and campaign
/// progress, and [`crate::ai::AiRules::at`] and the missions' scripts are where the levels
/// will differ.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum Difficulty {
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Self::Easy, Self::Normal, Self::Hard];

    pub fn name(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }

    /// The text library's key for the level's name.
    pub fn text_key(self) -> String {
        format!("difficulty.{}.name", self.name())
    }

    pub fn is_normal(&self) -> bool {
        *self == Self::Normal
    }
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
    /// A computer player's level; humans ignore it.
    #[serde(default, skip_serializing_if = "Difficulty::is_normal")]
    pub difficulty: Difficulty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSetup {
    pub seed: u32,
    pub slots: Vec<Slot>,
}
