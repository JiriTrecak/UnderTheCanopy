//! The replay file (`.utcr`, D60): everything needed to rebuild a match and feed it the
//! same commands, as the deterministic simulation (D4) then plays it out identically.
//!
//! ```text
//! bytes 0–3   "UTCR"
//! bytes 4–7   format version, u32 little endian (VERSION)
//! bytes 8–11  header length N, u32 little endian
//! 12…12+N     header: gzip over JSON of [`Header`] (what the list shows)
//! rest        body: gzip over JSON of [`Body`] (what playing it needs)
//! ```
//!
//! The header comes first and on its own, so the replay list reads only a few hundred
//! bytes of each file. JSON for the same reason as saves (D38): the simulation's types skip
//! absent optional fields, which binary serde formats cannot read back. Commands are the
//! protocol's actions, which compress well: a 20-minute match is tens of kilobytes.
//!
//! A newer format version is refused with its version named; the header's `game_version`
//! and content digest tell whether this game can play it (content identity, D38).

use crate::saves::MatchIdentity;
use anyhow::{Context, Result, bail};
use canopy_sim::{MatchSetup, Queued};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};

pub const MAGIC: &[u8; 4] = b"UTCR";
/// The layout's version: header and body fields. Bump when an older game could not read
/// a newer file.
pub const VERSION: u32 = 1;
pub const EXTENSION: &str = "utcr";
/// Ticks between recorded fingerprints (10 s of match time).
pub const FINGERPRINT_EVERY: u32 = 400;
/// The most bytes a header may take (a damaged length is refused before allocating).
const MAX_HEADER: u32 = 1 << 20;

/// What kind of match was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Skirmish,
    Campaign,
    Multiplayer,
}

impl Mode {
    pub fn name(self) -> String {
        match self {
            Self::Skirmish => tr!("match.kind-skirmish"),
            Self::Campaign => tr!("match.kind-campaign"),
            Self::Multiplayer => tr!("match.kind-multiplayer"),
        }
    }
}

/// One player as the list shows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    /// Zero-based slot.
    pub slot: u8,
    pub name: String,
    pub computer: bool,
    /// Race and starting hero ids (`None`: the content's defaults).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub race: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<u8>,
    /// Palette colour index (`config/players.json`).
    pub color: usize,
}

/// How the match ended, in slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ending {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<u8>,
    #[serde(default)]
    pub defeated: Vec<u8>,
}

/// What the replay list shows without reading the body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// The game version that recorded it (`0.3.7`).
    pub game_version: String,
    /// The simulation ABI (`canopy_net::wire::ABI`).
    pub abi: String,
    /// When the recording ended, milliseconds since the Unix epoch.
    pub recorded_at: u64,
    pub mode: Mode,
    pub map_name: String,
    /// Map id, map package digest and content digest: what a game must have to play it.
    pub identity: MatchIdentity,
    /// The map package is in the body (a map the game does not ship).
    #[serde(default)]
    pub map_included: bool,
    /// The world's clock when the first command could apply (a match continued from a
    /// save starts later than 0).
    #[serde(default)]
    pub start_tick: u32,
    /// The world's clock at the end of the recording.
    pub end_tick: u32,
    pub players: Vec<Player>,
    /// Whose game recorded it; `None` when watching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorder: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ending: Option<Ending>,
    /// How it ended for the recorder: Victory, Defeat, Draw, Surrendered, Left, In progress.
    pub result: String,
}

impl Header {
    /// Match time recorded, in ticks.
    pub fn duration(&self) -> u32 {
        self.end_tick.saturating_sub(self.start_tick)
    }
}

/// A map package carried inside the replay: its files by `/`-separated path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapPackage {
    /// File contents, base64.
    pub files: BTreeMap<String, String>,
}

/// What playing the replay needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    pub setup: MatchSetup,
    /// A match continued from a save starts from that save's world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<canopy_sim::Save>,
    /// Palette colour of each player by player number, when not their own (the lobby's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<usize>>,
    /// Every command the session handed the world, in order: each is enqueued at the
    /// world clock it names, before that tick is stepped.
    pub commands: Vec<Queued>,
    /// `World::fingerprint()` after stepping to each tick listed (every
    /// [`FINGERPRINT_EVERY`]), to tell when playback leaves the recorded match.
    pub fingerprints: Vec<(u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<MapPackage>,
}

/// A whole replay.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    pub header: Header,
    pub body: Body,
}

fn gzip(value: &impl Serialize) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(value)?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&json)?;
    Ok(encoder.finish()?)
}

fn gunzip<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(bytes).read_to_end(&mut json)?;
    Ok(serde_json::from_slice(&json)?)
}

/// Checks the start of a file and returns the header's length.
fn preamble(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 12 || &bytes[..4] != MAGIC {
        bail!("not a replay file");
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
    if version > VERSION {
        bail!("made by a newer version of the game (replay format {version})");
    }
    let len = u32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes"));
    if len > MAX_HEADER {
        bail!("the replay's header is damaged");
    }
    Ok(len as usize)
}

impl Replay {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let header = gzip(&self.header)?;
        let body = gzip(&self.body)?;
        let mut out = Vec::with_capacity(12 + header.len() + body.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let len = preamble(bytes)?;
        let header_end = 12 + len;
        let header = bytes
            .get(12..header_end)
            .context("the replay is cut short")?;
        Ok(Self {
            header: gunzip(header).context("the replay's header is damaged")?,
            body: gunzip(&bytes[header_end..]).context("the replay's commands are damaged")?,
        })
    }

    pub fn read(path: &std::path::Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        Self::from_bytes(&bytes)
    }
}

/// Reads only the header of the replay at `path`.
pub fn read_header(path: &std::path::Path) -> Result<Header> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut start = [0u8; 12];
    file.read_exact(&mut start).context("not a replay file")?;
    let len = preamble(&start)?;
    let mut header = vec![0u8; len];
    file.read_exact(&mut header)
        .context("the replay is cut short")?;
    gunzip(&header).context("the replay's header is damaged")
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub(crate) fn sample() -> Replay {
        Replay {
            header: Header {
                game_version: "0.3.7".into(),
                abi: "canopy-sim-1".into(),
                recorded_at: 1_760_000_000_000,
                mode: Mode::Skirmish,
                map_name: "Test Isles".into(),
                identity: MatchIdentity {
                    map_id: "test".into(),
                    map_digest: "ab".into(),
                    content_digest: "cd".into(),
                },
                map_included: false,
                start_tick: 0,
                end_tick: 4000,
                players: vec![Player {
                    slot: 0,
                    name: "You".into(),
                    computer: false,
                    race: None,
                    hero: None,
                    team: None,
                    color: 0,
                }],
                recorder: Some(0),
                ending: Some(Ending {
                    winner: Some(0),
                    defeated: vec![1],
                }),
                result: "Victory".into(),
            },
            body: Body {
                setup: MatchSetup {
                    seed: 7,
                    slots: Vec::new(),
                },
                start: None,
                colors: None,
                commands: vec![Queued {
                    tick: 12,
                    player: 0,
                    seq: 0,
                    action: canopy_sim::Action::Stop { actors: vec![3] },
                }],
                fingerprints: vec![(400, 99)],
                map: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tests_support::sample;

    #[test]
    fn a_replay_reads_back_and_its_header_alone() {
        let replay = sample();
        let bytes = replay.to_bytes().unwrap();
        assert_eq!(&bytes[..4], MAGIC);
        assert_eq!(Replay::from_bytes(&bytes).unwrap(), replay);
        let dir = std::env::temp_dir().join(format!("canopy-replay-format-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.utcr");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_header(&path).unwrap(), replay.header);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newer_and_foreign_files_are_refused() {
        let mut bytes = sample().to_bytes().unwrap();
        bytes[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());
        let error = Replay::from_bytes(&bytes).unwrap_err().to_string();
        assert!(error.contains("newer"), "{error}");
        assert!(Replay::from_bytes(b"PK\x03\x04 not a replay").is_err());
        assert!(Replay::from_bytes(&bytes[..10]).is_err());
    }
}
