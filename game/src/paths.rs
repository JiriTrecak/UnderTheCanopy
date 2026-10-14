//! Where the game finds its data and keeps the player's files.
//!
//! Nothing else in the game builds file paths from the working directory or the
//! executable location; everything goes through [`Paths`].

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// A file every data folder contains; used to recognise one.
const DATA_MARKER: &str = "content/game.json";

#[derive(Debug, Clone)]
pub struct Paths {
    /// Read-only game data (`content/`, later the cooked asset pack).
    pub data: PathBuf,
    /// Writable per-user folder for settings and saves.
    pub user: PathBuf,
}

impl Paths {
    pub fn resolve(data_override: Option<PathBuf>) -> Result<Self> {
        let exe = std::env::current_exe().context("cannot locate the game executable")?;
        let data = data_dir(data_override, &exe)?;
        let user = directories::ProjectDirs::from("com", "jt", "canopy")
            .context("cannot locate the user data folder")?
            .data_dir()
            .to_path_buf();
        std::fs::create_dir_all(&user)
            .with_context(|| format!("cannot create {}", user.display()))?;
        Ok(Self { data, user })
    }
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
        std::fs::create_dir_all(dir.join("content")).unwrap();
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
        std::fs::create_dir_all(data.join("content")).unwrap();
        std::fs::write(data.join(DATA_MARKER), "{}").unwrap();
        let found = data_dir(None, &exe).unwrap();
        assert_eq!(found.canonicalize().unwrap(), data.canonicalize().unwrap());
    }
}
