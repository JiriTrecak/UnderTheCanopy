//! Walking a package's tree: which files are elements of a collection.

use super::Package;
use crate::{Id, error::Error};
use serde_json::Value;
use std::path::Path;

/// How a collection stores its elements beneath its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// One `.json` file per element, anywhere beneath the folder; the id is inside.
    Files,
    /// One folder per element, holding this manifest file (id inside) and the element's
    /// other files; any folder without it only organises.
    Folders(&'static str),
    /// Fixed documents `<name>.json` directly in the folder; the name is the id.
    Singletons(&'static [&'static str]),
    /// Packages nested in this one (`<name>.<extension>` folders: maps, campaigns).
    Packages(&'static str),
}

/// A typed folder of a package, such as `units` (files) or `assets` (folders).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collection {
    /// Path from the package root, `/`-separated (`landscape/biomes`).
    pub dir: &'static str,
    pub form: Form,
}

/// An element as one package authors it.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub id: String,
    /// Path from the package root, `/`-separated: the file, or the element's folder.
    pub path: String,
    /// The document (for [`Form::Packages`], `Null`).
    pub value: Value,
}

/// Names a walk skips: hidden files, and the marker that keeps empty folders in git.
fn hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn entries(dir: &Path) -> std::io::Result<Vec<(String, bool)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if hidden(&name) {
            continue;
        }
        out.push((name, entry.file_type()?.is_dir()));
    }
    out.sort();
    Ok(out)
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

fn read_document(package: &Package, path: &str) -> Result<Value, Error> {
    let file = package.label(path);
    let text = std::fs::read_to_string(package.root.join(path))
        .map_err(|e| Error::new(&file, "", e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| Error::new(&file, "", e.to_string()))
}

/// The id inside a document, checked.
fn id_of(package: &Package, path: &str, value: &Value) -> Result<String, Error> {
    let file = package.label(path);
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(&file, "id", "missing id"))?;
    Id::new(id).map_err(|e| Error::new(&file, "id", e))?;
    Ok(id.to_string())
}

impl Collection {
    pub const fn files(dir: &'static str) -> Self {
        Self {
            dir,
            form: Form::Files,
        }
    }

    /// Every element of this collection in `package`, in path order. A missing folder is
    /// an empty collection.
    pub fn read(&self, package: &Package, errors: &mut Vec<Error>) -> Vec<Found> {
        let mut found = Vec::new();
        let root = package.root.join(self.dir);
        if !root.is_dir() {
            return found;
        }
        match self.form {
            Form::Singletons(names) => {
                let present = match entries(&root) {
                    Ok(present) => present,
                    Err(e) => {
                        errors.push(Error::new(&package.label(self.dir), "", e.to_string()));
                        return found;
                    }
                };
                for (name, is_dir) in present {
                    let path = join(self.dir, &name);
                    let stem = name.strip_suffix(".json");
                    match stem.filter(|s| !is_dir && names.contains(s)) {
                        Some(stem) => match read_document(package, &path) {
                            Ok(value) => found.push(Found {
                                id: stem.to_string(),
                                path,
                                value,
                            }),
                            Err(e) => errors.push(e),
                        },
                        None => errors.push(Error::new(
                            &package.label(&path),
                            "",
                            format!("unexpected here (expected one of {})", names.join(", ")),
                        )),
                    }
                }
            }
            _ => self.walk(package, self.dir, &mut found, errors),
        }
        found
    }

    fn walk(&self, package: &Package, dir: &str, found: &mut Vec<Found>, errors: &mut Vec<Error>) {
        let listed = match entries(&package.root.join(dir)) {
            Ok(listed) => listed,
            Err(e) => {
                errors.push(Error::new(&package.label(dir), "", e.to_string()));
                return;
            }
        };
        for (name, is_dir) in listed {
            let path = join(dir, &name);
            match (self.form, is_dir) {
                (Form::Files, false) if name.ends_with(".json") => {
                    match read_document(package, &path).and_then(|value| {
                        id_of(package, &path, &value).map(|id| Found { id, path, value })
                    }) {
                        Ok(element) => found.push(element),
                        Err(e) => errors.push(e),
                    }
                }
                (Form::Files, true) => self.walk(package, &path, found, errors),
                (Form::Folders(manifest), true) => {
                    let file = join(&path, manifest);
                    if package.root.join(&file).is_file() {
                        match read_document(package, &file).and_then(|value| {
                            id_of(package, &file, &value).map(|id| Found { id, path, value })
                        }) {
                            Ok(element) => found.push(element),
                            Err(e) => errors.push(e),
                        }
                    } else {
                        self.walk(package, &path, found, errors);
                    }
                }
                (Form::Packages(extension), true) => {
                    if Path::new(&name).extension().is_some_and(|e| e == extension) {
                        let id = name[..name.len() - extension.len() - 1].to_string();
                        found.push(Found {
                            id,
                            path,
                            value: Value::Null,
                        });
                    } else {
                        self.walk(package, &path, found, errors);
                    }
                }
                _ => errors.push(Error::new(
                    &package.label(&path),
                    "",
                    match self.form {
                        Form::Files => "only .json elements and folders belong here",
                        Form::Folders(_) => "only element folders belong here",
                        _ => "only packages and folders belong here",
                    },
                )),
            }
        }
    }
}
