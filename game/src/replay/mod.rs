//! Replays (D60, [ui §Replays](../../docs/ui.md#replays)): every match is recorded as it
//! plays ([`record`]), kept as a `.utcr` file ([`format`]) in the replays folder
//! ([`library`]) and watched by rebuilding the world and feeding it the recorded commands
//! ([`playback`]), with the observers' counts ([`tally`]). `canopy replay <file>
//! --verify` plays one headless and checks it ([`verify`]).

pub mod format;
pub mod library;
pub mod playback;
pub mod record;
pub mod tally;
#[cfg(test)]
mod tests;
pub mod verify;

use crate::saves::MatchIdentity;
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use format::{Body, Ending, Header, MapPackage, Mode, Player, Replay};
use std::path::{Path, PathBuf};

/// What the game knows about a match besides its recording.
#[derive(Debug, Clone)]
pub struct MatchInfo {
    pub mode: Mode,
    pub map_name: String,
    pub identity: MatchIdentity,
    /// The map package's folder, when the map is not one the game ships (it goes into
    /// the replay).
    pub map_package: Option<PathBuf>,
    pub players: Vec<Player>,
    pub recorder: Option<u8>,
    pub colors: Option<Vec<usize>>,
}

/// How the match ended for whoever recorded it, in a word.
pub fn result_word(
    outcome: Option<&canopy_sim::state::Outcome>,
    recorder: Option<u8>,
    setup: &canopy_sim::MatchSetup,
    surrendered: bool,
) -> &'static str {
    use canopy_content::placement::Owner;
    if surrendered {
        return "Surrendered";
    }
    let Some(o) = outcome else {
        return "Unfinished";
    };
    let Some(winner) = o.winner else {
        return "Draw";
    };
    let Some(me) = recorder else {
        return "Finished";
    };
    let teams = canopy_sim::teams::Teams::new(setup);
    let me = Owner::Player(me);
    if me == winner || teams.relation(me, winner) == canopy_sim::teams::Relation::Ally {
        "Victory"
    } else {
        "Defeat"
    }
}

/// The replay of `recording`.
pub fn build(
    recording: &record::Recording,
    info: &MatchInfo,
    result: &str,
    recorded_at: u64,
) -> Result<Replay> {
    use canopy_content::placement::Owner;
    let slot = |o: &Owner| match o {
        Owner::Player(p) => Some(*p),
        Owner::Nobody => None,
    };
    let map = info.map_package.as_deref().map(pack_map).transpose()?;
    Ok(Replay {
        header: Header {
            game_version: env!("CARGO_PKG_VERSION").into(),
            abi: canopy_net::wire::ABI.into(),
            recorded_at,
            mode: info.mode,
            map_name: info.map_name.clone(),
            identity: info.identity.clone(),
            map_included: map.is_some(),
            start_tick: recording.start_tick,
            end_tick: recording.tick,
            players: info.players.clone(),
            recorder: info.recorder,
            ending: recording.outcome.as_ref().map(|o| Ending {
                winner: o.winner.as_ref().and_then(slot),
                defeated: o.defeated.iter().filter_map(slot).collect(),
            }),
            result: result.into(),
        },
        body: Body {
            setup: recording.setup.clone(),
            start: recording.start.clone(),
            colors: info.colors.clone(),
            commands: recording.commands.clone(),
            fingerprints: recording.fingerprints.clone(),
            map,
        },
    })
}

/// The default name of a replay: `<Map> <date> <time> <result>`.
pub fn default_name(header: &Header) -> String {
    format!(
        "{} {} {}",
        header.map_name,
        crate::saves::format_time(header.recorded_at).replace(':', "-"),
        header.result
    )
}

/// `mm:ss` (or `h:mm:ss`) of `ticks`.
pub fn clock(ticks: u32) -> String {
    let s = ticks / 40;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The identity files of a map package (`Package::digest`'s), to carry in a replay.
pub fn pack_map(dir: &Path) -> Result<MapPackage> {
    fn walk(root: &Path, dir: &Path, out: &mut MapPackage) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(root, &path, out)?;
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| canopy_content::package::IDENTITY_EXTENSIONS.contains(&e))
            {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let bytes = std::fs::read(&path)?;
                out.files.insert(
                    relative,
                    base64::engine::general_purpose::STANDARD.encode(bytes),
                );
            }
        }
        Ok(())
    }
    let mut out = MapPackage {
        files: Default::default(),
    };
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// Writes a replay's map package under `cache` (once per map digest) and returns its
/// folder, checked against the digest the replay names.
pub fn unpack_map(header: &Header, package: &MapPackage, cache: &Path) -> Result<PathBuf> {
    let digest = &header.identity.map_digest;
    if digest.is_empty() || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("the replay names no map digest");
    }
    let id: String = header
        .identity
        .map_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let dir = cache
        .join("replay-maps")
        .join(&digest[..digest.len().min(16)])
        .join(format!("{id}.{}", canopy_content::package::MAP_EXTENSION));
    for (relative, data) in &package.files {
        if relative
            .split('/')
            .any(|p| p.is_empty() || p == ".." || p == ".")
        {
            bail!("the replay's map has a bad file name");
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("the replay's map is damaged")?;
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    let package =
        canopy_content::package::Package::open(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let here = crate::saves::digest(&package.digest()?);
    if &here != digest {
        bail!("the replay's map does not match its digest");
    }
    Ok(dir)
}
