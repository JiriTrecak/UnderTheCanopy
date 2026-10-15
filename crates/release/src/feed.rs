//! What the release host serves: the stable feed (Tauri's static update JSON, which the
//! old desktop app reads), the changelog index, and the staged artifact the release tool
//! publishes from.

use crate::{log::ReleaseEntry, version::Version};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `stable.json`: the newest release, its plain notes and one package per platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableFeed {
    pub version: Version,
    pub notes: String,
    pub pub_date: String,
    pub platforms: BTreeMap<String, PlatformPackage>,
}

/// A platform's package: its URL and its signature (base64 of the minisign signature
/// file, as Tauri writes it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformPackage {
    pub url: String,
    pub signature: String,
}

/// `changelog.json`: every published release, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogIndex {
    pub schema_version: u32,
    pub releases: Vec<ChangelogLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogLink {
    pub version: Version,
    pub title: String,
    pub published_at: String,
    /// Relative to the base URL (`releases/0.3.0/changelog.json`).
    pub url: String,
}

/// `build/releases/<version>/<platform>/artifact.json`: a signed, verified package
/// staged for publishing, with the notes it was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub schema_version: u32,
    pub version: Version,
    pub platform: String,
    pub filename: String,
    pub sha256: String,
    pub bytes: u64,
    pub signature: String,
    pub changelog: ReleaseEntry,
}

/// Platforms the feed may name.
pub const PLATFORMS: [&str; 6] = [
    "darwin-aarch64",
    "darwin-x86_64",
    "windows-x86_64",
    "windows-aarch64",
    "linux-x86_64",
    "linux-aarch64",
];

impl Artifact {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!("unknown artifact schema {}", self.schema_version));
        }
        if !PLATFORMS.contains(&self.platform.as_str()) {
            return Err(format!("unknown platform {}", self.platform));
        }
        if self.filename.is_empty()
            || !self
                .filename
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(format!("bad package file name {:?}", self.filename));
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err("sha256 must be 64 lowercase hex digits".into());
        }
        if self.bytes == 0 || self.bytes > 2 * 1024 * 1024 * 1024 {
            return Err("package size out of range".into());
        }
        if self.signature.len() < 80 || self.signature.len() > 4096 {
            return Err("signature length out of range".into());
        }
        if self.version != self.changelog.version {
            return Err("artifact and changelog versions differ".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_feed_shape_reads() {
        let feed: StableFeed = serde_json::from_str(
            r#"{"version":"0.2.3","notes":"Title\n\nImproved\n- x","pub_date":"2026-10-13T15:44:03.334Z",
            "platforms":{"darwin-aarch64":{"url":"https://example.org/p.app.tar.gz","signature":"c2ln"}}}"#,
        )
        .unwrap();
        assert_eq!(feed.version, "0.2.3".parse().unwrap());
        assert!(feed.platforms.contains_key("darwin-aarch64"));
    }
}
