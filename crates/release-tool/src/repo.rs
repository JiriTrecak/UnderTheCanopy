//! The repository the tool works in: the release log and its lock, the app version in the
//! workspace manifest, atomic writes and timestamps.

use anyhow::{Context, Result, anyhow, bail};
use canopy_release::{ReleaseLog, Version};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    /// The repository at `root`, which must hold the release log.
    pub fn open(root: PathBuf) -> Result<Self> {
        let repo = Repo { root };
        if !repo.log_path().is_file() {
            bail!(
                "no release log at {}; run the tool from the repository root",
                repo.log_path().display()
            );
        }
        Ok(repo)
    }

    pub fn log_path(&self) -> PathBuf {
        self.root.join("releases/log.json")
    }

    pub fn media_dir(&self) -> PathBuf {
        self.root.join("releases/media")
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("Cargo.toml")
    }

    pub fn read_log(&self) -> Result<ReleaseLog> {
        let text = fs::read_to_string(self.log_path()).context("reading releases/log.json")?;
        Ok(ReleaseLog::parse(&text)?)
    }

    /// The app version: the workspace version in the root `Cargo.toml`.
    pub fn app_version(&self) -> Result<Version> {
        let text = fs::read_to_string(self.manifest_path()).context("reading Cargo.toml")?;
        let (_, version) = workspace_version(&text)?;
        Ok(version)
    }

    /// Takes the release log's lock (`releases/log.json.lock`, created exclusively), so
    /// agents writing notes at the same time never lose each other's.
    pub fn lock_log(&self) -> Result<LogLock> {
        let path = self.root.join("releases/log.json.lock");
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                // Who holds it, for whoever finds a stale lock.
                let _ = writeln!(file, "pid {} since {}", std::process::id(), iso_now());
                Ok(LogLock { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => bail!(
                "another agent is updating the release log ({} exists); retry after it finishes. \
                 Remove a stale lock only after checking its owner has stopped.",
                path.display()
            ),
            Err(e) => Err(e).context("creating the release log lock"),
        }
    }
}

/// Removes the log lock when dropped.
pub struct LogLock {
    path: PathBuf,
}

impl Drop for LogLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Writes `bytes` to `path` through a temporary file beside it and a rename, so readers
/// see the old or the new file, never half of one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("{} has no file name", path.display()))?
        .to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.with_context(|| format!("writing {}", path.display()))
}

/// The `[workspace.package]` version in a manifest: the byte range of the quoted value
/// and the version.
pub fn workspace_version(manifest: &str) -> Result<(std::ops::Range<usize>, Version)> {
    let mut section = "";
    let mut offset = 0;
    for line in manifest.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed;
        } else if section == "[workspace.package]"
            && let Some(rest) = trimmed.strip_prefix("version")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            let value = value.trim();
            let inner = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .ok_or_else(|| anyhow!("the workspace version is not a plain string"))?;
            let start = offset + line.find(value).expect("value is in the line") + 1;
            let version = inner
                .parse()
                .with_context(|| format!("the workspace version {inner:?}"))?;
            return Ok((start..start + inner.len(), version));
        }
        offset += line.len();
    }
    bail!("Cargo.toml has no [workspace.package] version")
}

/// The manifest with its workspace version set to `version`; everything else unchanged.
pub fn set_workspace_version(manifest: &str, version: Version) -> Result<String> {
    let (range, _) = workspace_version(manifest)?;
    let mut out = manifest.to_owned();
    out.replace_range(range, &version.to_string());
    Ok(out)
}

/// Now as ISO 8601 UTC with milliseconds, like JavaScript's `toISOString`.
pub fn iso_now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970");
    iso_millis(since.as_millis() as i64)
}

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs()
}

/// Milliseconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn iso_millis(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let in_day = ms.rem_euclid(86_400_000);
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_match_javascript() {
        assert_eq!(iso_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_millis(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso_millis(1_791_906_403_334), "2026-10-13T15:46:43.334Z");
        assert_eq!(iso_millis(-1), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn only_the_workspace_version_changes() {
        let manifest = "[package]\nversion = \"9.9.9\"\n\n[workspace.package]\nedition = \"2024\"\nversion = \"0.1.0\"\nlicense = \"x\"\n\n[dependencies]\nversion = \"1\"\n";
        let (_, version) = workspace_version(manifest).unwrap();
        assert_eq!(version.to_string(), "0.1.0");
        let next = set_workspace_version(manifest, "0.3.0".parse().unwrap()).unwrap();
        assert_eq!(next, manifest.replace("0.1.0", "0.3.0"));
        assert!(workspace_version("[package]\nversion = \"1.0.0\"\n").is_err());
    }
}
