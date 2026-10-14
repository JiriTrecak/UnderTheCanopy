//! Finding the game's maps, as the old `src/shared/map/library.ts` does.

use super::{UTCMAP_EXTENSION, UtcMap};
use crate::error::{Error, Errors};
use std::path::{Path, PathBuf};

/// Folder holding the maps, relative to the data folder.
pub const MAPS_FOLDER: &str = "assets/maps";
/// Subfolders searched (recursively) for `.utcmap` files.
pub const MAP_GROUPS: [&str; 3] = ["campaign", "skirmish", "showcase"];

/// A map file the game offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapFile {
    /// Lowercased file stem, unique across all groups.
    pub id: String,
    /// One of [`MAP_GROUPS`].
    pub group: &'static str,
    /// Relative to the data folder, `/`-separated.
    pub file: String,
    pub path: PathBuf,
}

impl MapFile {
    pub fn load(&self) -> Result<UtcMap, Errors> {
        let bytes = std::fs::read(&self.path)
            .map_err(|error| Errors(vec![Error::new(&self.file, "", error.to_string())]))?;
        UtcMap::parse(&bytes, &self.file)
    }
}

/// Every map below `data`, sorted by id. Fails on unreadable folders and on two files that
/// would share an id. Menus order maps by name, which needs the documents.
pub fn library(data: &Path) -> Result<Vec<MapFile>, Errors> {
    let mut maps = Vec::new();
    let mut errors = Vec::new();
    for group in MAP_GROUPS {
        let relative = format!("{MAPS_FOLDER}/{group}");
        let folder = data.join(&relative);
        if folder.is_dir() {
            collect(&folder, &relative, group, &mut maps, &mut errors);
        }
    }
    maps.sort_by(|a: &MapFile, b| a.id.cmp(&b.id).then(a.file.cmp(&b.file)));
    for pair in maps.windows(2) {
        if pair[0].id == pair[1].id {
            errors.push(Error::new(
                &pair[1].file,
                "",
                format!("map id {} is also used by {}", pair[1].id, pair[0].file),
            ));
        }
    }
    if errors.is_empty() {
        Ok(maps)
    } else {
        Err(Errors(errors))
    }
}

fn collect(
    folder: &Path,
    relative: &str,
    group: &'static str,
    maps: &mut Vec<MapFile>,
    errors: &mut Vec<Error>,
) {
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(Error::new(relative, "", error.to_string()));
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Globs skip hidden files and folders.
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let file = format!("{relative}/{name}");
        if path.is_dir() {
            collect(&path, &file, group, maps, errors);
        } else if path.extension().is_some_and(|e| e == UTCMAP_EXTENSION) {
            maps.push(MapFile {
                id: name.replacen(".utcmap", "", 1).to_lowercase(),
                group,
                file,
                path,
            });
        }
    }
}
