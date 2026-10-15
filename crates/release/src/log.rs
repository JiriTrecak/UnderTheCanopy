//! The release log (`releases/log.json`): what changed for players, collected under
//! Unreleased as work lands and frozen into a version when a release is prepared. It is
//! the only changelog: the game's "What's new", the feed's notes and the published
//! changelogs all come from it. Field order matches the old log so rewrites stay small.

use crate::version::Version;
use serde::{Deserialize, Serialize};

/// The kinds of change, in the order notes show them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Improved,
    Fixed,
    KnownIssues,
}

impl ChangeKind {
    pub const ALL: [ChangeKind; 4] = [Self::Added, Self::Improved, Self::Fixed, Self::KnownIssues];

    /// The heading of the kind in notes.
    pub fn heading(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Improved => "Improved",
            Self::Fixed => "Fixed",
            Self::KnownIssues => "Known issues",
        }
    }

    /// Its name on the command line and in the log (`added`, `knownIssues`, …).
    pub fn key(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Improved => "improved",
            Self::Fixed => "fixed",
            Self::KnownIssues => "knownIssues",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// A screenshot in a release's gallery: an optimised WebP in `releases/media/` named by
/// its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseImage {
    pub file: String,
    pub alt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub width: u32,
    pub height: u32,
}

/// Changes of one kind each, and an optional gallery.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ReleaseImage>>,
    pub added: Vec<String>,
    pub improved: Vec<String>,
    pub fixed: Vec<String>,
    pub known_issues: Vec<String>,
}

impl ChangeSet {
    pub fn notes(&self, kind: ChangeKind) -> &[String] {
        match kind {
            ChangeKind::Added => &self.added,
            ChangeKind::Improved => &self.improved,
            ChangeKind::Fixed => &self.fixed,
            ChangeKind::KnownIssues => &self.known_issues,
        }
    }

    pub fn notes_mut(&mut self, kind: ChangeKind) -> &mut Vec<String> {
        match kind {
            ChangeKind::Added => &mut self.added,
            ChangeKind::Improved => &mut self.improved,
            ChangeKind::Fixed => &mut self.fixed,
            ChangeKind::KnownIssues => &mut self.known_issues,
        }
    }

    pub fn is_empty(&self) -> bool {
        ChangeKind::ALL.iter().all(|&k| self.notes(k).is_empty())
    }
}

/// A released version: its changes, when it was prepared and its title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseEntry {
    #[serde(flatten)]
    pub changes: ChangeSet,
    pub version: Version,
    /// ISO 8601 UTC (`2026-10-13T15:44:03.334Z`).
    pub published_at: String,
    pub title: String,
}

impl ReleaseEntry {
    /// Plain-text notes: the title, then each kind's heading and bullets (old
    /// `releaseNotes`; the feed carries these).
    pub fn plain_notes(&self) -> String {
        let mut lines = vec![self.title.clone()];
        for kind in ChangeKind::ALL {
            let notes = self.changes.notes(kind);
            if notes.is_empty() {
                continue;
            }
            lines.push(String::new());
            lines.push(kind.heading().to_owned());
            lines.extend(notes.iter().map(|n| format!("- {n}")));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseLog {
    pub schema_version: u32,
    pub unreleased: ChangeSet,
    /// Newest first.
    pub releases: Vec<ReleaseEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LogError {
    #[error("the release log does not parse: {0}")]
    Parse(String),
    #[error("release log: {0}")]
    Invalid(String),
}

impl ReleaseLog {
    /// Parses and validates a log (old `releaseLog` schema).
    pub fn parse(text: &str) -> Result<Self, LogError> {
        let log: Self = serde_json::from_str(text).map_err(|e| LogError::Parse(e.to_string()))?;
        log.validate()?;
        Ok(log)
    }

    /// The log as written to disk: two-space JSON and a final newline.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("the log serialises");
        s.push('\n');
        s
    }

    pub fn validate(&self) -> Result<(), LogError> {
        let bad = |m: String| Err(LogError::Invalid(m));
        if self.schema_version != 1 {
            return bad(format!("unknown schema version {}", self.schema_version));
        }
        let check = |set: &ChangeSet, at: &str| -> Result<(), LogError> {
            for kind in ChangeKind::ALL {
                let notes = set.notes(kind);
                if notes.len() > 200 {
                    return Err(LogError::Invalid(format!(
                        "{at}: more than 200 {} notes",
                        kind.key()
                    )));
                }
                for n in notes {
                    if n.trim().is_empty() || n.trim() != n || n.chars().count() > 1000 {
                        return Err(LogError::Invalid(format!(
                            "{at}: {} notes must be trimmed, 1–1000 characters: {n:?}",
                            kind.key()
                        )));
                    }
                }
            }
            if let Some(images) = &set.images {
                if images.len() > 8 {
                    return Err(LogError::Invalid(format!("{at}: more than eight images")));
                }
                let mut seen = std::collections::BTreeSet::new();
                for i in images {
                    let hex = i.file.strip_suffix(".webp").unwrap_or("");
                    if hex.len() != 64
                        || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                    {
                        return Err(LogError::Invalid(format!(
                            "{at}: image file {:?} is not <sha256>.webp",
                            i.file
                        )));
                    }
                    if !seen.insert(&i.file) {
                        return Err(LogError::Invalid(format!(
                            "{at}: duplicate gallery image {}",
                            i.file
                        )));
                    }
                    if i.alt.trim().is_empty()
                        || i.alt.chars().count() > 240
                        || i.width == 0
                        || i.height == 0
                        || i.width > 1600
                        || i.height > 1600
                    {
                        return Err(LogError::Invalid(format!(
                            "{at}: image {} needs alt text and at most 1600 px a side",
                            i.file
                        )));
                    }
                }
            }
            Ok(())
        };
        check(&self.unreleased, "Unreleased")?;
        let mut versions = std::collections::BTreeSet::new();
        for r in &self.releases {
            let at = r.version.to_string();
            check(&r.changes, &at)?;
            if !versions.insert(r.version) {
                return bad(format!("duplicate release version {at}"));
            }
            if r.title.trim().is_empty() || r.title.chars().count() > 120 {
                return bad(format!("{at}: the title must be 1–120 characters"));
            }
            if !is_timestamp(&r.published_at) {
                return bad(format!("{at}: publishedAt must be an ISO 8601 UTC time"));
            }
        }
        Ok(())
    }

    /// The release of `version`.
    pub fn release(&self, version: Version) -> Option<&ReleaseEntry> {
        self.releases.iter().find(|r| r.version == version)
    }

    /// Releases up to and including `version`, newest first (the installed history).
    pub fn history(&self, version: Version) -> Vec<&ReleaseEntry> {
        let mut out: Vec<_> = self
            .releases
            .iter()
            .filter(|r| r.version <= version)
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.version));
        out
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.fff]Z`.
fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let fixed = b.len() >= 20
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    let rest = &s[19.min(s.len())..];
    fixed
        && (rest == "Z"
            || rest
                .strip_prefix('.')
                .and_then(|r| r.strip_suffix('Z'))
                .is_some_and(|f| !f.is_empty() && f.bytes().all(|d| d.is_ascii_digit())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = r#"{
  "schemaVersion": 1,
  "unreleased": {
    "added": [],
    "improved": [
      "Faster."
    ],
    "fixed": [],
    "knownIssues": []
  },
  "releases": [
    {
      "added": [
        "A thing."
      ],
      "improved": [],
      "fixed": [],
      "knownIssues": [],
      "version": "0.1.0",
      "publishedAt": "2026-10-01T10:00:00.000Z",
      "title": "First"
    }
  ]
}
"#;

    #[test]
    fn the_log_round_trips_byte_for_byte_and_renders_notes() {
        let log = ReleaseLog::parse(LOG).unwrap();
        assert_eq!(log.to_json(), LOG);
        let first = log.release("0.1.0".parse().unwrap()).unwrap();
        assert_eq!(first.plain_notes(), "First\n\nAdded\n- A thing.");
        assert_eq!(log.history("0.2.0".parse().unwrap()).len(), 1);
        assert!(log.history("0.0.9".parse().unwrap()).is_empty());
    }

    #[test]
    fn invalid_logs_are_refused() {
        let duplicate = LOG.replace(r#""title": "First"
    }"#, r#""title": "First"
    },
    {"added": [], "improved": [], "fixed": [], "knownIssues": [], "version": "0.1.0", "publishedAt": "2026-10-01T10:00:00Z", "title": "Again"}"#);
        assert!(ReleaseLog::parse(&duplicate).is_err());
        assert!(ReleaseLog::parse(&LOG.replace("\"Faster.\"", "\" Faster.\"")).is_err());
        assert!(ReleaseLog::parse(&LOG.replace("2026-10-01T10:00:00.000Z", "yesterday")).is_err());
        assert!(
            ReleaseLog::parse(&LOG.replace(
                "\"fixed\": [],\n    \"knownIssues\"",
                "\"fixed\": [],\n    \"extra\": [],\n    \"knownIssues\""
            ))
            .is_err()
        );
    }
}
