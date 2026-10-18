//! The release log commands: noting a change, checking the log, and preparing a release
//! (freezing Unreleased into a version and setting the app version).

use crate::{
    media::{self, Checked},
    repo::{self, Repo},
};
use anyhow::{Context, Result, bail};
use canopy_release::{ChangeKind, ReleaseEntry, ReleaseLog, Version};
use std::fs;

/// Adds a note to Unreleased; an exact duplicate is ignored. Returns whether it was added.
pub fn add_note(log: &mut ReleaseLog, kind: ChangeKind, text: &str) -> Result<bool> {
    let text = text.trim();
    if text.is_empty() {
        bail!("the note is empty");
    }
    let notes = log.unreleased.notes_mut(kind);
    if notes.iter().any(|n| n == text) {
        return Ok(false);
    }
    notes.push(text.to_owned());
    log.validate()?;
    Ok(true)
}

/// Moves Unreleased into release `version` (newest first) and clears it. The version must
/// follow both the app version and every recorded release under the bump policy.
pub fn prepare(
    log: &mut ReleaseLog,
    app_version: Version,
    version: Version,
    title: &str,
    milestone: bool,
    published_at: String,
) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        bail!("supply --title \"Short release title\"");
    }
    if log.unreleased.is_empty() {
        bail!("no unreleased changes to release");
    }
    let newest = log.releases.iter().map(|r| r.version).max();
    if newest.is_some_and(|n| version <= n) {
        bail!("the release version must be newer than every recorded release");
    }
    // The app version may lag the log (the Rust game started at 0.1.0 after 0.2.3 was
    // released), so the bump follows whichever is newer.
    let current = newest.map_or(app_version, |n| n.max(app_version));
    current.check_bump(version, milestone)?;
    let changes = std::mem::take(&mut log.unreleased);
    log.releases.insert(
        0,
        ReleaseEntry {
            changes,
            version,
            published_at,
            title: title.to_owned(),
        },
    );
    log.validate()?;
    Ok(())
}

/// Verifies every gallery image the log names. Returns the LFS pointers met (only when
/// `allow_pointers`).
pub fn verify_media(repo: &Repo, log: &ReleaseLog, allow_pointers: bool) -> Result<Vec<String>> {
    let mut pointers = Vec::new();
    let sets = std::iter::once(&log.unreleased).chain(log.releases.iter().map(|r| &r.changes));
    for set in sets {
        for image in set.images.iter().flatten() {
            if let Checked::LfsPointer(_) =
                media::verify_image(&repo.media_dir(), image, allow_pointers)?
            {
                pointers.push(image.file.clone());
            }
        }
    }
    Ok(pointers)
}

pub fn cmd_note(repo: &Repo, kind: &str, text: &str) -> Result<()> {
    let Some(kind) = ChangeKind::parse(kind) else {
        bail!("use note --kind added|improved|fixed|knownIssues --text \"Player-facing change\"");
    };
    let _lock = repo.lock_log()?;
    let mut log = repo.read_log()?;
    if add_note(&mut log, kind, text)? {
        repo::write_atomic(&repo.log_path(), log.to_json().as_bytes())?;
        println!("Release log updated (note).");
    } else {
        println!("The note is already in Unreleased.");
    }
    Ok(())
}

pub fn cmd_check(repo: &Repo, allow_pointers: bool) -> Result<()> {
    let _lock = repo.lock_log()?;
    let log = repo.read_log()?;
    let pointers = verify_media(repo, &log, allow_pointers)?;
    for p in &pointers {
        println!("warning: releases/media/{p} is a Git LFS pointer; its image was not checked");
    }
    println!("Release log valid.");
    Ok(())
}

/// `text` (a `package.json`) with its first `"version"` value set to `version`, the rest of
/// the document untouched.
fn set_package_version(text: &str, version: &str) -> Result<String> {
    let key = "\"version\": \"";
    let start = text.find(key).context("package.json has no version")? + key.len();
    let end = start + text[start..].find('"').context("unterminated version")?;
    Ok(format!("{}{version}{}", &text[..start], &text[end..]))
}

pub fn cmd_prepare(repo: &Repo, version: &str, title: &str, milestone: bool) -> Result<()> {
    let version: Version = version.parse()?;
    let _lock = repo.lock_log()?;
    let mut log = repo.read_log()?;
    let manifest_path = repo.manifest_path();
    let manifest = fs::read_to_string(&manifest_path).context("reading Cargo.toml")?;
    let (_, app_version) = repo::workspace_version(&manifest)?;
    prepare(
        &mut log,
        app_version,
        version,
        title,
        milestone,
        repo::iso_now(),
    )?;
    verify_media(repo, &log, false)?;
    let next_manifest = repo::set_workspace_version(&manifest, version)?;
    // Both files or neither: the log first, then the manifest, restoring the log if the
    // manifest cannot be written.
    let old_log = fs::read(repo.log_path())?;
    repo::write_atomic(&repo.log_path(), log.to_json().as_bytes())?;
    if let Err(e) = repo::write_atomic(&manifest_path, next_manifest.as_bytes()) {
        repo::write_atomic(&repo.log_path(), &old_log)?;
        return Err(e);
    }
    // The base package and the match server carry the game's version.
    for file in ["base/package.json", "server/package.json"] {
        let path = repo.root.join(file);
        if let Ok(text) = fs::read_to_string(&path) {
            repo::write_atomic(
                &path,
                set_package_version(&text, &version.to_string())?.as_bytes(),
            )?;
        }
    }
    // Cargo.lock records the workspace members' versions.
    let status = std::process::Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .args(["update", "--workspace", "--offline"])
        .current_dir(&repo.root)
        .status();
    if !status.is_ok_and(|s| s.success()) {
        eprintln!(
            "warning: `cargo update --workspace` failed; Cargo.lock updates on the next build"
        );
    }
    println!("Release log updated (prepare): {version}, app version {app_version} → {version}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(unreleased: &[&str]) -> ReleaseLog {
        let mut log = ReleaseLog::parse(
            r#"{"schemaVersion":1,"unreleased":{"added":[],"improved":[],"fixed":[],"knownIssues":[]},
            "releases":[{"added":["Old."],"improved":[],"fixed":[],"knownIssues":[],"version":"0.2.3",
            "publishedAt":"2026-10-13T15:44:03.334Z","title":"Old"}]}"#,
        )
        .unwrap();
        for n in unreleased {
            add_note(&mut log, ChangeKind::Fixed, n).unwrap();
        }
        log
    }

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn notes_are_trimmed_and_duplicates_ignored() {
        let mut log = log(&[]);
        assert!(add_note(&mut log, ChangeKind::Added, "  A thing. ").unwrap());
        assert!(!add_note(&mut log, ChangeKind::Added, "A thing.").unwrap());
        assert!(add_note(&mut log, ChangeKind::Fixed, "A thing.").unwrap());
        assert!(add_note(&mut log, ChangeKind::Added, "   ").is_err());
        assert_eq!(log.unreleased.added, ["A thing."]);
    }

    #[test]
    fn preparing_freezes_unreleased_under_the_bump_policy() {
        let at = || "2026-10-15T10:00:00.000Z".to_owned();
        // Nothing to release.
        assert!(prepare(&mut log(&[]), v("0.1.0"), v("0.2.4"), "T", false, at()).is_err());
        // The app version lags the log: the newest release decides.
        let mut l = log(&["Fixed it."]);
        assert!(prepare(&mut l, v("0.1.0"), v("0.3.0"), "T", false, at()).is_err());
        assert!(prepare(&mut l, v("0.1.0"), v("0.2.3"), "T", true, at()).is_err());
        assert!(prepare(&mut l, v("0.1.0"), v("0.3.1"), "T", true, at()).is_err());
        assert!(prepare(&mut l, v("0.1.0"), v("0.3.0"), " ", true, at()).is_err());
        prepare(&mut l, v("0.1.0"), v("0.3.0"), "New engine", true, at()).unwrap();
        assert!(l.unreleased.is_empty());
        let entry = &l.releases[0];
        assert_eq!(
            (entry.version, entry.title.as_str()),
            (v("0.3.0"), "New engine")
        );
        assert_eq!(entry.changes.fixed, ["Fixed it."]);
        // A routine patch after it, from an app version that caught up.
        add_note(&mut l, ChangeKind::Improved, "Faster.").unwrap();
        prepare(&mut l, v("0.3.0"), v("0.3.1"), "Patch", false, at()).unwrap();
        assert_eq!(l.releases.len(), 3);
    }
}
