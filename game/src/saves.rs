//! The save library (D16): saved local matches in `saves/` of the user folder.
//!
//! Each save is two files named by its id:
//!
//! - `<id>.json`: a small, readable header ([`SaveMeta`]) the library lists without
//!   touching the state: name, time, map, mode, the match's identity and its tick.
//! - `<id>.save.gz`: the body, gzip over JSON of [`SaveBody`]: the simulation's
//!   `World::save()` (setup, state, pending commands; versioned by the simulation) and
//!   the game's own additions.
//!
//! JSON rather than a binary format because the simulation's state skips absent optional
//! fields (`skip_serializing_none`), which non-self-describing formats cannot read back;
//! gzip brings a mid-game save from a few megabytes to a few hundred kilobytes.
//!
//! A save loads only on the content and map it was made with: the header carries the
//! digest of the content files and of the map file ([`MatchIdentity`]), checked before
//! the body is read. Saves are unlimited (as in the old game); one `autosave` slot is
//! overwritten when the window closes during a local match.
//!
//! **Export and import** (old `Export`, `Import save…`): a save leaves the library as one
//! `.utcsave` file, gzip over JSON of its header and body together ([`Exported`]), so a
//! player shares a single file. Importing checks the file like loading does (format,
//! the map and content identity, the mode when importing from a match) and adds it to
//! the library under the file's name with a fresh id.

use crate::settings::write_atomic;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The header's `format` value.
pub const FORMAT: &str = "canopy-save";
/// Version of the header and body layout (the simulation versions its own state).
pub const VERSION: u32 = 1;
/// Folder in the user folder.
pub const FOLDER: &str = "saves";
/// The id of the slot written when the window closes during a match.
pub const AUTOSAVE: &str = "autosave";
/// Longest save name (old limit).
pub const MAX_NAME: usize = 100;
/// Extension of an exported save (old `.utcsave`).
pub const EXPORT_EXTENSION: &str = "utcsave";
/// The exported file's `format` value.
pub const EXPORT_FORMAT: &str = "canopy-save-export";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Skirmish,
    Campaign,
}

/// What a save must match to be loaded: the content and the map file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchIdentity {
    pub map_id: String,
    /// SHA-256 of the map package's digest, hex.
    pub map_digest: String,
    /// `ContentIdentity::sha256` of the content files.
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveMeta {
    pub format: String,
    pub version: u32,
    pub id: String,
    pub name: String,
    /// Milliseconds since the Unix epoch.
    pub saved_at: u64,
    pub mode: Mode,
    pub map_name: String,
    pub identity: MatchIdentity,
    /// The local player's slot; `None` when watching.
    pub player: Option<u8>,
    pub tick: u32,
}

/// The body: the simulation's save and what the game adds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBody {
    pub world: canopy_sim::Save,
    /// Camera focus and zoom when saved (`x`, `z`, zoom, yaw).
    #[serde(default)]
    pub camera: Option<[f32; 4]>,
    /// The player's control groups (entity ids by slot, key 1 first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<Vec<u32>>>,
}

/// An exported save: header and body in one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub format: String,
    pub version: u32,
    pub meta: SaveMeta,
    pub body: SaveBody,
}

/// A save about to be written.
pub struct NewSave<'a> {
    pub name: &'a str,
    pub mode: Mode,
    pub map_name: &'a str,
    pub identity: &'a MatchIdentity,
    pub player: Option<u8>,
    pub body: &'a SaveBody,
}

#[derive(Debug, Clone)]
pub struct SaveLibrary {
    dir: PathBuf,
}

/// Why a save cannot be loaded here.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Mismatch {
    #[error("this save was made on another map ({0})")]
    Map(String),
    #[error("this save was made on another version of the map")]
    MapRevision,
    #[error("this save was made with other game content")]
    Content,
    #[error("only {0} saves can be loaded here")]
    Mode(&'static str),
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Skirmish => "skirmish",
            Self::Campaign => "campaign",
        }
    }
}

impl MatchIdentity {
    /// Whether a save of `self` can resume on `here`.
    pub fn check(&self, here: &MatchIdentity) -> Result<(), Mismatch> {
        if self.map_id != here.map_id {
            Err(Mismatch::Map(self.map_id.clone()))
        } else if self.map_digest != here.map_digest {
            Err(Mismatch::MapRevision)
        } else if self.content_digest != here.content_digest {
            Err(Mismatch::Content)
        } else {
            Ok(())
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A short local date and time for lists (`2026-10-15 14:03`, UTC: no time-zone data).
pub fn format_time(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let (h, m) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{d:02} {h:02}:{m:02}")
}

impl SaveLibrary {
    pub fn new(user: &Path) -> Self {
        Self {
            dir: user.join(FOLDER),
        }
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn body_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.save.gz"))
    }

    /// Every readable save, newest first. Unreadable headers are skipped (and logged).
    pub fn list(&self) -> Vec<SaveMeta> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<SaveMeta> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .filter_map(|path| {
                let read = std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|b| serde_json::from_slice::<SaveMeta>(&b).map_err(|e| e.to_string()));
                match read {
                    Ok(meta) if meta.format == FORMAT && meta.version == VERSION => Some(meta),
                    Ok(meta) => {
                        tracing::warn!(save = %path.display(), version = meta.version, "unsupported save");
                        None
                    }
                    Err(error) => {
                        tracing::warn!(save = %path.display(), %error, "unreadable save");
                        None
                    }
                }
            })
            .filter(|meta| self.body_path(&meta.id).is_file())
            .collect();
        out.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.id.cmp(&b.id)));
        out
    }

    /// Writes a new save (or the autosave slot when `id` is given) and returns its header.
    pub fn write(&self, save: &NewSave<'_>, id: Option<&str>) -> Result<SaveMeta> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("cannot create {}", self.dir.display()))?;
        let saved_at = now_ms();
        let id = match id {
            Some(id) => id.to_string(),
            None => (0u32..)
                .map(|n| format!("{saved_at:x}-{n}"))
                .find(|id| !self.meta_path(id).exists())
                .expect("a free id"),
        };
        let name: String = save.name.trim().chars().take(MAX_NAME).collect();
        let meta = SaveMeta {
            format: FORMAT.into(),
            version: VERSION,
            id: id.clone(),
            name: if name.is_empty() {
                save.map_name.to_string()
            } else {
                name
            },
            saved_at,
            mode: save.mode,
            map_name: save.map_name.into(),
            identity: save.identity.clone(),
            player: save.player,
            tick: save.body.world.state.tick,
        };
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        serde_json::to_writer(&mut gz, save.body)?;
        let bytes = gz.finish()?;
        // Body first: a header is only listed once its body exists.
        write_atomic(&self.body_path(&id), &bytes)?;
        write_atomic(&self.meta_path(&id), &serde_json::to_vec_pretty(&meta)?)?;
        Ok(meta)
    }

    pub fn meta(&self, id: &str) -> Result<SaveMeta> {
        let path = self.meta_path(id);
        let bytes =
            std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Reads a save, refusing it unless it matches `here` (`None`: no check).
    pub fn read(&self, id: &str, here: Option<&MatchIdentity>) -> Result<(SaveMeta, SaveBody)> {
        let meta = self.meta(id)?;
        if meta.format != FORMAT || meta.version != VERSION {
            bail!("unsupported save format {} {}", meta.format, meta.version);
        }
        if let Some(here) = here {
            meta.identity.check(here)?;
        }
        let path = self.body_path(id);
        let file = std::fs::File::open(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(file)
            .read_to_end(&mut json)
            .with_context(|| format!("{} is damaged", path.display()))?;
        let body: SaveBody = serde_json::from_slice(&json)
            .with_context(|| format!("{} is not a save", path.display()))?;
        Ok((meta, body))
    }

    /// The file name an export of `meta` is offered under (old: `<mode>-<name>.utcsave`
    /// with everything but letters, digits and dashes replaced by dashes).
    pub fn export_name(meta: &SaveMeta) -> String {
        let name: String = meta
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        format!("{}-{name}.{EXPORT_EXTENSION}", meta.mode.name())
    }

    /// Writes save `id` to `path` as one exported file.
    pub fn export(&self, id: &str, path: &Path) -> Result<SaveMeta> {
        let (meta, body) = self.read(id, None)?;
        write_atomic(path, &exported_bytes(&meta, &body)?)
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(meta)
    }

    /// Adds the exported save at `path` to the library, named after the file. `here`
    /// gives this game's identity of the save's map (an error when the map is not in
    /// this game); `mode` restricts the save's mode (importing from a match's menu).
    pub fn import(
        &self,
        path: &Path,
        mode: Option<Mode>,
        here: impl FnOnce(&MatchIdentity) -> Result<MatchIdentity>,
    ) -> Result<SaveMeta> {
        let file =
            std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(file)
            .read_to_end(&mut json)
            .context("the file is not an exported save")?;
        let exported: Exported =
            serde_json::from_slice(&json).context("the file is not an exported save")?;
        if exported.format != EXPORT_FORMAT
            || exported.version != VERSION
            || exported.meta.format != FORMAT
            || exported.meta.version != VERSION
        {
            bail!(
                "unsupported save format {} {}",
                exported.format,
                exported.version
            );
        }
        let meta = &exported.meta;
        if let Some(mode) = mode
            && meta.mode != mode
        {
            return Err(Mismatch::Mode(mode.name()).into());
        }
        meta.identity.check(&here(&meta.identity)?)?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.write(
            &NewSave {
                name: &name,
                mode: meta.mode,
                map_name: &meta.map_name,
                identity: &meta.identity,
                player: meta.player,
                body: &exported.body,
            },
            None,
        )
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        for path in [self.meta_path(id), self.body_path(id)] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(e).with_context(|| format!("cannot delete {}", path.display()));
                }
            }
        }
        Ok(())
    }
}

/// SHA-256 of bytes, lowercase hex.
/// A save as an exported `.utcsave` file's bytes (gzip over JSON of header and body).
pub fn exported_bytes(meta: &SaveMeta, body: &SaveBody) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    struct ExportedRef<'a> {
        format: &'a str,
        version: u32,
        meta: &'a SaveMeta,
        body: &'a SaveBody,
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    serde_json::to_writer(
        &mut gz,
        &ExportedRef {
            format: EXPORT_FORMAT,
            version: VERSION,
            meta,
            body,
        },
    )?;
    Ok(gz.finish()?)
}

pub fn digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Gzip helper for tests and tools.
#[allow(dead_code)]
pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(bytes).expect("in memory");
    gz.finish().expect("in memory")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::ai_fixture;

    fn identity() -> MatchIdentity {
        MatchIdentity {
            map_id: "fixture".into(),
            map_digest: digest(b"map"),
            content_digest: digest(b"content"),
        }
    }

    #[test]
    fn a_saved_match_resumes_with_identical_fingerprints() {
        let user = crate::settings::temp_folder("saves");
        let library = SaveLibrary::new(&user);
        assert!(library.list().is_empty());
        let map = ai_fixture::skirmish(vec![], vec![]);
        let mut world = ai_fixture::world(&map, true);
        ai_fixture::run(&mut world, 300);
        let body = SaveBody {
            world: world.save(),
            camera: Some([10.0, 20.0, 1.0, 0.0]),
            groups: Some(vec![vec![3, 5], vec![]]),
        };
        let meta = library
            .write(
                &NewSave {
                    name: "  Before the push ",
                    mode: Mode::Skirmish,
                    map_name: "Fixture",
                    identity: &identity(),
                    player: Some(0),
                    body: &body,
                },
                None,
            )
            .unwrap();
        assert_eq!(meta.name, "Before the push");
        assert_eq!(meta.tick, world.tick());
        let listed = library.list();
        assert_eq!(listed, vec![meta.clone()]);

        let (read_meta, read_body) = library.read(&meta.id, Some(&identity())).unwrap();
        assert_eq!(read_meta, meta);
        assert_eq!(read_body, body);
        let mut resumed =
            canopy_sim::World::restore(ai_fixture::content(), &map, read_body.world).unwrap();
        assert_eq!(resumed.fingerprint(), world.fingerprint());
        for _ in 0..400 {
            world.step();
            resumed.step();
            assert_eq!(
                resumed.fingerprint(),
                world.fingerprint(),
                "tick {}",
                world.tick()
            );
        }
        assert_eq!(resumed.save(), world.save());
    }

    #[test]
    fn the_library_refuses_other_maps_and_content_and_lists_newest_first() {
        let user = crate::settings::temp_folder("saves-identity");
        let library = SaveLibrary::new(&user);
        let map = ai_fixture::skirmish(vec![], vec![]);
        let world = ai_fixture::world(&map, false);
        let body = SaveBody {
            world: world.save(),
            camera: None,
            groups: None,
        };
        let save = |name: &'static str, id: Option<&str>| {
            library
                .write(
                    &NewSave {
                        name,
                        mode: Mode::Skirmish,
                        map_name: "Fixture",
                        identity: &identity(),
                        player: None,
                        body: &body,
                    },
                    id,
                )
                .unwrap()
        };
        let first = save("", None);
        assert_eq!(first.name, "Fixture", "an empty name takes the map's");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = save("second", None);
        let auto = save("Autosave", Some(AUTOSAVE));
        let again = save("Autosave", Some(AUTOSAVE));
        assert_eq!(auto.id, again.id);
        let ids: Vec<String> = library.list().into_iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], AUTOSAVE);
        assert_eq!(ids[1], second.id);
        assert_eq!(ids[2], first.id);

        let mut other = identity();
        other.content_digest = digest(b"other content");
        let error = library.read(&first.id, Some(&other)).unwrap_err();
        assert_eq!(error.downcast_ref::<Mismatch>(), Some(&Mismatch::Content));
        other = identity();
        other.map_digest = digest(b"edited map");
        assert!(library.read(&first.id, Some(&other)).is_err());
        library.delete(&first.id).unwrap();
        assert_eq!(library.list().len(), 2);
        // A damaged body is an error, not a panic.
        std::fs::write(library.body_path(&second.id), b"garbage").unwrap();
        assert!(library.read(&second.id, None).is_err());
    }

    #[test]
    fn an_exported_save_imports_where_its_map_and_content_match() {
        let user = crate::settings::temp_folder("saves-export");
        let library = SaveLibrary::new(&user);
        let map = ai_fixture::skirmish(vec![], vec![]);
        let mut world = ai_fixture::world(&map, true);
        ai_fixture::run(&mut world, 50);
        let body = SaveBody {
            world: world.save(),
            camera: Some([1.0, 2.0, 1.0, 0.0]),
            groups: None,
        };
        let meta = library
            .write(
                &NewSave {
                    name: "Ridge: hold!",
                    mode: Mode::Campaign,
                    map_name: "Fixture",
                    identity: &identity(),
                    player: Some(1),
                    body: &body,
                },
                None,
            )
            .unwrap();
        assert_eq!(
            SaveLibrary::export_name(&meta),
            "campaign-Ridge--hold-.utcsave"
        );
        let file = user.join("shared").join("Ridge hold.utcsave");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        library.export(&meta.id, &file).unwrap();

        // Another player's library takes it under the file's name, checked like a load.
        let other = SaveLibrary::new(&user.join("other"));
        let here = |_: &MatchIdentity| Ok(identity());
        let imported = other.import(&file, None, here).unwrap();
        assert_eq!(imported.name, "Ridge hold");
        assert_eq!(imported.mode, Mode::Campaign);
        assert_eq!(imported.identity, identity());
        assert_eq!(imported.player, Some(1));
        let (_, read) = other.read(&imported.id, Some(&identity())).unwrap();
        assert_eq!(read, body);

        // Refused: another mode from a match, another revision of the map, a missing
        // map, a file that is not a save.
        let error = other.import(&file, Some(Mode::Skirmish), here).unwrap_err();
        assert_eq!(
            error.downcast_ref::<Mismatch>(),
            Some(&Mismatch::Mode("skirmish"))
        );
        let edited = |_: &MatchIdentity| {
            let mut i = identity();
            i.map_digest = digest(b"edited");
            Ok(i)
        };
        let error = other.import(&file, None, edited).unwrap_err();
        assert_eq!(
            error.downcast_ref::<Mismatch>(),
            Some(&Mismatch::MapRevision)
        );
        assert!(
            other
                .import(&file, None, |_| anyhow::bail!("no such map"))
                .is_err()
        );
        let junk = user.join("junk.utcsave");
        std::fs::write(&junk, b"not a save").unwrap();
        assert!(other.import(&junk, None, here).is_err());
        assert_eq!(other.list().len(), 1);
    }

    #[test]
    fn times_format_as_dates() {
        assert_eq!(format_time(0), "1970-01-01 00:00");
        assert_eq!(format_time(1_792_000_000_000), "2026-10-14 17:46");
    }
}
