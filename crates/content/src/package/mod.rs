//! Content packages (docs/packages.md, D53): the base game, campaigns, maps and mods share
//! one structure. A package is a folder with `package.json` and one file per element in
//! free folders beneath a type folder (`units/ants/heroes/marshal.json`); folders carry
//! no meaning. Packages are layered in load order: the same id in a later package merges
//! onto the earlier one (see [`layer`]).
//!
//! This module knows nothing about what the elements mean: it reads packages, walks their
//! trees and layers documents. [`crate::Content`] gives the collections their types.

pub mod layer;
mod tree;

pub use layer::{Contribution, Layered, Layers};
pub use tree::{Collection, Form, Found};

use crate::error::{Error, parse};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The manifest every package has at its root.
pub const MANIFEST: &str = "package.json";

/// Package format this build reads and writes.
pub const FORMAT: u32 = 1;

/// Extension of a map package folder (or its zip).
pub const MAP_EXTENSION: &str = "utcm";
/// Extension of a campaign package folder (or its zip).
pub const CAMPAIGN_EXTENSION: &str = "utcc";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageKind {
    /// The shipped game; the bottom layer of every load.
    Base,
    Campaign,
    Map,
    Mod,
}

/// `package.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    /// Package format version ([`FORMAT`]).
    pub format: u32,
    /// Unique among packages; lowercase with dashes.
    pub id: crate::Id,
    pub kind: PackageKind,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Package id → version requirement (`">=0.3.5"`); every dependency loads earlier.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<crate::Id, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
}

/// A package on disk: its manifest and where it lives.
#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: Manifest,
    pub root: PathBuf,
}

impl Package {
    /// Reads the manifest of the package folder `root`.
    pub fn open(root: &Path) -> Result<Self, Error> {
        let label = root.display().to_string();
        let text = std::fs::read_to_string(root.join(MANIFEST))
            .map_err(|e| Error::new(&label, MANIFEST, e.to_string()))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| Error::new(&label, MANIFEST, e.to_string()))?;
        let manifest: Manifest = parse(value, &format!("{label}/{MANIFEST}"), "")?;
        if manifest.format > FORMAT {
            return Err(Error::new(
                &label,
                MANIFEST,
                format!(
                    "package format {} is newer than this game reads ({FORMAT})",
                    manifest.format
                ),
            ));
        }
        Ok(Self {
            manifest,
            root: root.to_path_buf(),
        })
    }

    pub fn id(&self) -> &str {
        self.manifest.id.as_str()
    }

    /// `file` of this package as errors name it: `<package id>/<path>`.
    pub fn label(&self, path: &str) -> String {
        format!("{}/{path}", self.id())
    }
}

/// Checks a load order: ids are unique, the first package is the base and no other is, and
/// every dependency is present earlier in the order. Version requirements are recorded,
/// not yet enforced.
pub fn check_order(packages: &[Package]) -> Vec<Error> {
    let mut errors = Vec::new();
    for (i, package) in packages.iter().enumerate() {
        let file = package.label(MANIFEST);
        let base = package.manifest.kind == PackageKind::Base;
        if i == 0 && !base {
            errors.push(Error::new(
                &file,
                "kind",
                "the first package must be the base",
            ));
        }
        if i > 0 && base {
            errors.push(Error::new(
                &file,
                "kind",
                "only the first package may be a base",
            ));
        }
        if packages[..i].iter().any(|p| p.id() == package.id()) {
            errors.push(Error::new(&file, "id", "loaded twice"));
        }
        for dependency in package.manifest.dependencies.keys() {
            if !packages[..i].iter().any(|p| p.id() == dependency.as_str()) {
                errors.push(Error::new(
                    &file,
                    format!("dependencies.{dependency}"),
                    "not loaded before this package",
                ));
            }
        }
    }
    errors
}

/// Extensions of the files that make up a package's identity: everything that can change a
/// match (documents, scripts, map grids). Models, textures and sounds never change the
/// simulation and are left out, so hashing stays cheap.
pub const IDENTITY_EXTENSIONS: [&str; 3] = ["json", "lua", "grid"];

impl Package {
    /// SHA-256 over the package's identity files (see [`IDENTITY_EXTENSIONS`]), sorted
    /// by path, each as its path, its length and its bytes. Hidden files are skipped.
    pub fn digest(&self) -> std::io::Result<[u8; 32]> {
        use sha2::{Digest, Sha256};
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    walk(root, &path, out)?;
                } else if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| IDENTITY_EXTENSIONS.contains(&e))
                {
                    let relative = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    out.push((relative, path));
                }
            }
            Ok(())
        }
        let mut files = Vec::new();
        walk(&self.root, &self.root, &mut files)?;
        files.sort();
        let mut hash = Sha256::new();
        for (relative, path) in files {
            let bytes = std::fs::read(&path)?;
            hash.update((relative.len() as u64).to_le_bytes());
            hash.update(relative.as_bytes());
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(&bytes);
        }
        Ok(hash.finalize().into())
    }
}
