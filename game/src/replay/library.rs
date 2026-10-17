//! The replays folder (D60): `Under the Canopy/Replays` in the player's documents
//! (`Paths::replays`). A replay's name is its file name, so what the list shows is what
//! the player finds in Finder and shares; renaming renames the file. `Last match.utcr` is
//! overwritten by every match that ends or is left, so the latest one is never lost.

use super::format::{self, EXTENSION, Header, Replay};
use crate::settings::write_atomic;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The replay kept of the latest match.
pub const LAST_MATCH: &str = "Last match";
/// Longest replay name, in characters.
pub const MAX_NAME: usize = 80;

/// A replay in the folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The file name without its extension.
    pub name: String,
    pub path: PathBuf,
    /// The file's size in bytes.
    pub bytes: u64,
    pub header: Result<Header, String>,
}

impl Entry {
    /// When it was recorded (milliseconds since the epoch), for sorting.
    fn recorded(&self) -> u64 {
        self.header.as_ref().map_or(0, |h| h.recorded_at)
    }
}

#[derive(Debug, Clone)]
pub struct Library {
    dir: PathBuf,
}

/// A name the file system takes: no path separators or characters some systems refuse,
/// trimmed, at most [`MAX_NAME`] characters.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    cleaned
        .trim()
        .trim_start_matches('.')
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_string()
}

impl Library {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.{EXTENSION}"))
    }

    /// Every replay, newest first (unreadable ones too, with the reason).
    pub fn list(&self) -> Vec<Entry> {
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<Entry> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == EXTENSION))
            .filter_map(|path| {
                let name = path.file_stem()?.to_string_lossy().into_owned();
                let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
                let header = format::read_header(&path).map_err(|e| format!("{e:#}"));
                Some(Entry {
                    name,
                    path,
                    bytes,
                    header,
                })
            })
            .collect();
        out.sort_by(|a, b| {
            b.recorded()
                .cmp(&a.recorded())
                .then_with(|| a.name.cmp(&b.name))
        });
        out
    }

    /// Writes `replay` as `name` (a number added while the name is taken), or over the
    /// last match's replay when `name` is [`LAST_MATCH`]. Returns the name written.
    pub fn write(&self, replay: &Replay, name: &str) -> Result<String> {
        let bytes = replay.to_bytes()?;
        let base = clean_name(name);
        let base = if base.is_empty() {
            "Replay".to_string()
        } else {
            base
        };
        let name = if base == LAST_MATCH {
            base
        } else {
            self.free_name(&base)
        };
        write_atomic(&self.path(&name), &bytes)?;
        Ok(name)
    }

    fn free_name(&self, base: &str) -> String {
        (1u32..)
            .map(|n| {
                if n == 1 {
                    base.to_string()
                } else {
                    format!("{base} ({n})")
                }
            })
            .find(|n| !self.path(n).exists())
            .expect("a free name")
    }

    /// Renames a replay; returns the new name.
    pub fn rename(&self, from: &str, to: &str) -> Result<String> {
        let to = clean_name(to);
        if to.is_empty() {
            bail!("a replay needs a name");
        }
        if to == from {
            return Ok(to);
        }
        if self.path(&to).exists() {
            bail!("a replay called “{to}” already exists");
        }
        std::fs::rename(self.path(from), self.path(&to))
            .with_context(|| format!("cannot rename “{from}”"))?;
        Ok(to)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        std::fs::remove_file(self.path(name)).with_context(|| format!("cannot delete “{name}”"))
    }

    #[cfg(test)]
    pub fn read(&self, name: &str) -> Result<Replay> {
        Replay::read(&self.path(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_files_and_the_list_is_newest_first() {
        let dir = crate::settings::temp_folder("replays");
        let library = Library::new(&dir);
        assert!(library.list().is_empty(), "no folder yet");
        let mut replay = format::tests_support::sample();
        replay.header.recorded_at = 1000;
        assert_eq!(library.write(&replay, "Echo: 1/2").unwrap(), "Echo- 1-2");
        assert_eq!(
            library.write(&replay, "Echo: 1/2").unwrap(),
            "Echo- 1-2 (2)"
        );
        replay.header.recorded_at = 2000;
        library.write(&replay, LAST_MATCH).unwrap();
        library.write(&replay, LAST_MATCH).unwrap();
        std::fs::write(dir.join("broken.utcr"), b"nope").unwrap();
        let names: Vec<String> = library.list().into_iter().map(|e| e.name).collect();
        assert_eq!(names, [LAST_MATCH, "Echo- 1-2", "Echo- 1-2 (2)", "broken"]);
        assert!(library.list()[3].header.is_err());

        assert_eq!(library.rename("Echo- 1-2", "  Final  ").unwrap(), "Final");
        assert!(library.rename("Final", "Echo- 1-2 (2)").is_err(), "taken");
        assert!(library.rename("Final", "   ").is_err());
        assert_eq!(library.read("Final").unwrap(), replay_at(1000));
        library.delete("Final").unwrap();
        assert_eq!(library.list().len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn replay_at(recorded: u64) -> Replay {
        let mut r = format::tests_support::sample();
        r.header.recorded_at = recorded;
        r
    }
}
