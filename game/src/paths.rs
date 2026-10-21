//! Where the game finds its data and keeps the player's files.
//!
//! Nothing else in the game builds file paths from the working directory or the
//! executable location; everything goes through [`Paths`] (the updater finds the bundle
//! it replaces from the executable, `update/install.rs`).

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// A file every data folder contains; used to recognise one.
const DATA_MARKER: &str = "base/package.json";
/// A file only a repository checkout has beside its data: the packaged game ships the
/// data alone.
const REPOSITORY_MARKER: &str = "Cargo.toml";

#[derive(Debug, Clone)]
pub struct Paths {
    /// Read-only game data: the base package, the asset registry, the cooked asset pack.
    pub data: PathBuf,
    /// Writable per-user folder for settings and saves.
    pub user: PathBuf,
    /// Per-user cache (downloaded update packages); may be emptied by the system.
    pub cache: PathBuf,
    /// The player's replays (D60): `Under the Canopy/Replays` in their documents, where
    /// they find and share them; the user folder's `replays` without a documents folder.
    pub replays: PathBuf,
}

/// The game's folder in the player's documents.
pub const DOCUMENTS_FOLDER: &str = "Under the Canopy";
/// Replays, in the game's documents folder.
pub const REPLAYS: &str = "Replays";

/// The player's own map packages, in the user folder.
pub const USER_MAPS: &str = "maps";
/// Map packages received from other players, in the user folder.
pub const DOWNLOADED_MAPS: &str = "downloads";
/// Feedback reports waiting to be sent, in the user folder.
pub const FEEDBACK_OUTBOX: &str = "feedback";
/// This installation's anonymous id (feedback reports), in the user folder.
pub const INSTALL_ID: &str = "install-id";
/// The player's progress through each campaign (`<campaign id>.json`), in the user folder.
pub const CAMPAIGN_PROGRESS: &str = "campaigns";

impl Paths {
    pub fn resolve(data_override: Option<PathBuf>) -> Result<Self> {
        let exe = std::env::current_exe().context("cannot locate the game executable")?;
        let data = data_dir(data_override, &exe)?;
        // The user folder keeps its name across the bundle identifier change to the old
        // app's (D43): the old app's files were its web view's, which this game never read.
        let dirs = directories::ProjectDirs::from("com", "jt", "canopy")
            .context("cannot locate the user data folder")?;
        let user = dirs.data_dir().to_path_buf();
        std::fs::create_dir_all(user.join(USER_MAPS))
            .with_context(|| format!("cannot create {}", user.display()))?;
        let replays = directories::UserDirs::new()
            .and_then(|u| {
                u.document_dir()
                    .map(|d| d.join(DOCUMENTS_FOLDER).join(REPLAYS))
            })
            .unwrap_or_else(|| user.join("replays"));
        Ok(Self {
            data,
            user,
            cache: dirs.cache_dir().to_path_buf(),
            replays,
        })
    }
}

impl Paths {
    /// The player's progress through campaign `id`.
    pub fn campaign_progress(&self, id: &str) -> PathBuf {
        self.user.join(CAMPAIGN_PROGRESS).join(format!("{id}.json"))
    }

    /// The repository checkout the data folder is, if it is one. The editor edits its
    /// base package and nothing else, and exists only then (docs/editor.md).
    pub fn repository(&self) -> Option<&Path> {
        is_repository(&self.data).then_some(self.data.as_path())
    }
}

fn is_repository(data: &Path) -> bool {
    data.join(REPOSITORY_MARKER).is_file()
}

fn data_dir(data_override: Option<PathBuf>, exe: &Path) -> Result<PathBuf> {
    if let Some(dir) = data_override {
        return checked(dir);
    }
    let bundled = bundled_data_dir(exe);
    if bundled.join(DATA_MARKER).is_file() {
        return checked(bundled);
    }
    // Development builds started with plain `cargo run` read the repository.
    #[cfg(debug_assertions)]
    {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        if repository.join(DATA_MARKER).is_file() {
            return checked(repository);
        }
    }
    bail!(
        "game data not found at {} (run from a package, or pass --data <folder>)",
        bundled.display()
    )
}

/// The packaged layout: `Contents/Resources/data` in a macOS bundle, `data/` beside the
/// executable elsewhere.
fn bundled_data_dir(exe: &Path) -> PathBuf {
    let exe_dir = exe.parent().unwrap_or(Path::new("."));
    if cfg!(target_os = "macos") {
        exe_dir.join("../Resources/data")
    } else {
        exe_dir.join("data")
    }
}

fn checked(dir: PathBuf) -> Result<PathBuf> {
    if !dir.join(DATA_MARKER).is_file() {
        bail!(
            "{} is not a game data folder (no {DATA_MARKER})",
            dir.display()
        );
    }
    Ok(dir.canonicalize().unwrap_or(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("canopy-paths-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("base")).unwrap();
        std::fs::write(dir.join(DATA_MARKER), "{}").unwrap();
        dir
    }

    #[test]
    fn override_must_be_a_data_folder() {
        let dir = data_folder("override");
        assert_eq!(
            data_dir(Some(dir.clone()), Path::new("/x/canopy")).unwrap(),
            dir.canonicalize().unwrap()
        );
        assert!(data_dir(Some(dir.join("content")), Path::new("/x/canopy")).is_err());
    }

    #[test]
    fn only_a_checkout_is_a_repository() {
        let dir = data_folder("repository");
        assert!(!is_repository(&dir));
        std::fs::write(dir.join(REPOSITORY_MARKER), "[workspace]").unwrap();
        assert!(is_repository(&dir));
    }

    #[test]
    fn finds_data_in_the_packaged_layout() {
        let root = data_folder("bundle");
        let (exe, data) = if cfg!(target_os = "macos") {
            (
                root.join("Contents/MacOS/canopy"),
                root.join("Contents/Resources/data"),
            )
        } else {
            (root.join("canopy.exe"), root.join("data"))
        };
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(data.join("base")).unwrap();
        std::fs::write(data.join(DATA_MARKER), "{}").unwrap();
        let found = data_dir(None, &exe).unwrap();
        assert_eq!(found.canonicalize().unwrap(), data.canonicalize().unwrap());
    }
}
