//! Finding the game's maps: map packages (`<id>.utcm`) in the base package's `maps/`.

use super::UtcMap;
use crate::error::{Error, Errors};
use crate::package::{MAP_EXTENSION, Package};
use std::path::{Path, PathBuf};

/// Folder holding the maps, relative to the data folder.
pub const MAPS_FOLDER: &str = "base/maps";
/// Subfolders searched (recursively) for map packages.
pub const MAP_GROUPS: [&str; 3] = ["campaign", "skirmish", "showcase"];

/// A map the game offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapFile {
    /// The package id, unique across all groups.
    pub id: String,
    /// One of [`MAP_GROUPS`].
    pub group: &'static str,
    /// The package folder relative to the data folder, `/`-separated.
    pub file: String,
    /// The package folder.
    pub path: PathBuf,
}

impl MapFile {
    pub fn load(&self) -> Result<UtcMap, Errors> {
        super::package::load(&self.path)
    }

    /// The package's digest ([`Package::digest`]): the map's identity in saves and matches.
    pub fn digest(&self) -> Result<[u8; 32], Errors> {
        let package = Package::open(&self.path).map_err(|e| Errors(vec![e]))?;
        package
            .digest()
            .map_err(|e| Errors(vec![Error::new(&self.file, "", e.to_string())]))
    }
}

/// Every map below `data`, sorted by id. Fails on unreadable folders and on two packages
/// that share an id. Menus order maps by name, which needs the documents.
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
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let file = format!("{relative}/{name}");
        if !path.is_dir() {
            continue;
        }
        if path.extension().is_some_and(|e| e == MAP_EXTENSION) {
            match Package::open(&path) {
                Ok(package) => maps.push(MapFile {
                    id: package.id().to_string(),
                    group,
                    file,
                    path,
                }),
                Err(error) => errors.push(error),
            }
        } else {
            collect(&path, &file, group, maps, errors);
        }
    }
}
