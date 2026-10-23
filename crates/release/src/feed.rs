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
    /// The first-install download (the macOS disk image), when the release has one. The
    /// feed never names it: updaters only read `stable.json` and the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<Installer>,
    pub changelog: ReleaseEntry,
}

/// A release's first-install file (`under-the-canopy-<version>-<platform>.dmg`): the same
/// app as the update package, for people installing the game for the first time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installer {
    pub filename: String,
    /// Its public URL, beside the package.
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
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
        check_file("package", &self.filename, &self.sha256, self.bytes)?;
        if self.signature.len() < 80 || self.signature.len() > 4096 {
            return Err("signature length out of range".into());
        }
        if self.version != self.changelog.version {
            return Err("artifact and changelog versions differ".into());
        }
        if let Some(installer) = &self.installer {
            check_file(
                "installer",
                &installer.filename,
                &installer.sha256,
                installer.bytes,
            )?;
            if installer.filename == self.filename {
                return Err("the installer and the package share a file name".into());
            }
            let folder = installer
                .url
                .strip_suffix(&installer.filename)
                .and_then(|u| u.strip_suffix('/'));
            if !installer.url.starts_with("https://") || folder.is_none() {
                return Err(format!(
                    "the installer URL {:?} must be https and end with its file name",
                    installer.url
                ));
            }
        }
        Ok(())
    }
}

/// A released file's name (plain ASCII, no paths), hash and size.
fn check_file(what: &str, filename: &str, sha256: &str, bytes: u64) -> Result<(), String> {
    if filename.is_empty()
        || filename.starts_with('.')
        || !filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(format!("bad {what} file name {filename:?}"));
    }
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!(
            "the {what}'s sha256 must be 64 lowercase hex digits"
        ));
    }
    if bytes == 0 || bytes > 2 * 1024 * 1024 * 1024 {
        return Err(format!("{what} size out of range"));
    }
    Ok(())
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
        // Installed updaters parse exactly this shape: no installer, no new keys.
        assert_eq!(
            serde_json::to_value(&feed).unwrap(),
            serde_json::json!({
                "version": "0.2.3",
                "notes": "Title\n\nImproved\n- x",
                "pub_date": "2026-10-13T15:44:03.334Z",
                "platforms": {"darwin-aarch64": {"url": "https://example.org/p.app.tar.gz", "signature": "c2ln"}}
            })
        );
    }

    fn artifact_json(installer: Option<serde_json::Value>) -> serde_json::Value {
        let mut value = serde_json::json!({
            "schemaVersion": 1,
            "version": "0.4.2",
            "platform": "darwin-aarch64",
            "filename": "under-the-canopy-0.4.2-darwin-aarch64.app.tar.gz",
            "sha256": "a".repeat(64),
            "bytes": 1000,
            "signature": "s".repeat(100),
            "changelog": {"added": ["A thing."], "improved": [], "fixed": [], "knownIssues": [], "version": "0.4.2", "publishedAt": "2026-10-22T10:00:00.000Z", "title": "T"}
        });
        if let Some(installer) = installer {
            value["installer"] = installer;
        }
        value
    }

    #[test]
    fn installers_round_trip_and_artifacts_without_one_still_read() {
        // Artifacts published before installers existed read and write back unchanged.
        let old = artifact_json(None);
        let artifact: Artifact = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(artifact.installer, None);
        artifact.validate().unwrap();
        assert_eq!(serde_json::to_value(&artifact).unwrap(), old);

        let url = "https://cdn.test/releases/0.4.2/under-the-canopy-0.4.2-darwin-aarch64.dmg";
        let with = artifact_json(Some(serde_json::json!({
            "filename": "under-the-canopy-0.4.2-darwin-aarch64.dmg",
            "url": url,
            "sha256": "b".repeat(64),
            "bytes": 2000
        })));
        let artifact: Artifact = serde_json::from_value(with.clone()).unwrap();
        artifact.validate().unwrap();
        let installer = artifact.installer.as_ref().unwrap();
        assert_eq!((installer.url.as_str(), installer.bytes), (url, 2000));
        assert_eq!(serde_json::to_value(&artifact).unwrap(), with);

        // Broken installers are refused.
        let bad = |change: &dyn Fn(&mut Installer)| {
            let mut a = artifact.clone();
            change(a.installer.as_mut().unwrap());
            a.validate().is_err()
        };
        assert!(bad(&|i| i.url = "http://cdn.test/x.dmg".into()));
        assert!(bad(&|i| i.url = "https://cdn.test/other.dmg".into()));
        assert!(bad(&|i| i.sha256 = "B".repeat(64)));
        assert!(bad(&|i| i.bytes = 0));
        assert!(bad(&|i| i.filename = "../x.dmg".into()));
        assert!(bad(&|i| i.filename = artifact.filename.clone()));
    }
}
