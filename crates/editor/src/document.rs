//! The documents being edited and the commands that change them (docs/editor.md, the
//! editing model). Every change is a [`Command`]; a [`Step`] groups commands into one undo
//! step with the context to bring back on undo. Edits live in memory until [`Store::save`]
//! writes every changed document in the one file format ([`format`]).

use canopy_content::map::grid::{Element, Grid};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One change to one document. Pointers are JSON pointers (`/ranks/0/damage`); `""` is
/// the whole document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Command {
    /// Sets a field or an array entry (adding the field if missing). In an object, `at`
    /// places a new field at that position instead of last (undo uses it to restore the
    /// file's key order).
    Set {
        document: String,
        pointer: String,
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<usize>,
    },
    /// Inserts into an array at `index` (its length appends).
    Insert {
        document: String,
        pointer: String,
        index: usize,
        value: Value,
    },
    /// Removes a field or an array entry.
    Remove { document: String, pointer: String },
    /// A new document.
    Create { document: String, value: Value },
    /// Deletes a document or a grid (its file goes on save).
    Delete { document: String },
    /// Sets a whole terrain grid (a `.grid` file), creating it when missing: `width ×
    /// height` samples, little-endian, rows along x (base64 in JSON).
    Grid {
        document: String,
        element: Sample,
        width: u32,
        height: u32,
        #[serde(with = "base64_bytes")]
        #[schemars(with = "String")]
        bytes: Vec<u8>,
    },
    /// Replaces a rectangle of a grid: `height` rows of `width` samples from sample
    /// (`x`, `z`) (base64 in JSON). A brush stroke is one patch per grid it touched.
    Patch {
        document: String,
        x: u32,
        z: u32,
        width: u32,
        height: u32,
        #[serde(with = "base64_bytes")]
        #[schemars(with = "String")]
        bytes: Vec<u8>,
    },
}

/// What one sample of a grid is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Sample {
    F32,
    U8,
    Rgba8,
}

impl Sample {
    pub fn element(self) -> Element {
        match self {
            Sample::F32 => Element::F32,
            Sample::U8 => Element::U8,
            Sample::Rgba8 => Element::Rgba8,
        }
    }

    pub fn of(element: Element) -> Self {
        match element {
            Element::F32 => Sample::F32,
            Element::U8 => Sample::U8,
            Element::Rgba8 => Sample::Rgba8,
        }
    }
}

mod base64_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}

impl Command {
    pub fn document(&self) -> &str {
        match self {
            Command::Set { document, .. }
            | Command::Insert { document, .. }
            | Command::Remove { document, .. }
            | Command::Create { document, .. }
            | Command::Delete { document }
            | Command::Grid { document, .. }
            | Command::Patch { document, .. } => document,
        }
    }
}

/// Whether `path` names a terrain grid rather than a JSON document.
pub fn is_grid(path: &str) -> bool {
    path.ends_with(".grid")
}

/// A grid as the editor holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct GridData {
    pub element: Element,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// Whether `path` is an asset's `asset.json`.
fn is_asset(path: &str) -> bool {
    path.starts_with("assets/") && path.ends_with("/asset.json")
}

/// The file names an asset lists (`<role>.<format>`, `<role>_<n>.<format>`).
pub fn asset_files(asset: &Value) -> Vec<String> {
    asset
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let role = r.get("role")?.as_str()?;
            let format = r.get("format")?.as_str()?;
            let index = r.get("index").and_then(Value::as_u64).unwrap_or(1);
            Some(if index <= 1 {
                format!("{role}.{format}")
            } else {
                format!("{role}_{index}.{format}")
            })
        })
        .filter(|name| !name.contains(['/', '\\']) && !name.starts_with('.'))
        .collect()
}

/// Removes the files a deleted asset listed beside its `asset.json` (`manifest`), and the
/// folder when that leaves it empty.
fn remove_asset_files(manifest: &Path, asset: &Value) -> std::io::Result<()> {
    let Some(folder) = manifest.parent() else {
        return Ok(());
    };
    for name in asset_files(asset) {
        match std::fs::remove_file(folder.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    if std::fs::read_dir(folder)?.next().is_none() {
        std::fs::remove_dir(folder)?;
    }
    Ok(())
}

/// A grid document; its unsaved state is worked out once per change.
#[derive(Debug, Clone)]
struct GridDoc {
    value: Option<GridData>,
    saved: Option<GridData>,
    revision: u64,
    /// (revision compared, whether it differed from the file), `None` until compared.
    compared: std::cell::Cell<Option<(u64, bool)>>,
}

impl GridDoc {
    fn dirty(&self) -> bool {
        if let Some((at, dirty)) = self.compared.get()
            && at == self.revision
        {
            return dirty;
        }
        let dirty = self.value != self.saved;
        self.compared.set(Some((self.revision, dirty)));
        dirty
    }
}

/// Where a step happened: undo and redo bring it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    /// The editor (a `Kind`'s index on the rail).
    pub editor: crate::Kind,
    /// The document to show.
    pub document: String,
    /// What it changed, when one place.
    pub pointer: Option<String>,
}

/// One undo step: what was done and how to take it back.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub context: Context,
    done: Vec<Command>,
    inverse: Vec<Command>,
    /// Continuous edits with the same key fold into this step ([`Store::run_merging`]).
    merge: Option<String>,
    at: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    NoDocument(String),
    Exists(String),
    NoTarget { document: String, pointer: String },
    NotAnArray { document: String, pointer: String },
    Read { document: String, message: String },
    Write { document: String, message: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoDocument(d) => write!(f, "{d}: no such document"),
            Error::Exists(d) => write!(f, "{d}: already exists"),
            Error::NoTarget { document, pointer } => {
                write!(f, "{document}: nothing at {pointer:?}")
            }
            Error::NotAnArray { document, pointer } => {
                write!(f, "{document}: {pointer:?} is not a list")
            }
            Error::Read { document, message } => write!(f, "{document}: {message}"),
            Error::Write { document, message } => write!(f, "{document}: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// A document as the editor holds it. `None` values are absent: not yet created
/// (`saved`) or deleted (`value`).
#[derive(Debug, Clone)]
struct Document {
    value: Option<Value>,
    saved: Option<Value>,
    /// Counts changes, so views can cache what they derive from the value.
    revision: u64,
    /// (revision compared, whether it differed from the file), `None` until compared:
    /// the shell asks every frame which documents are unsaved, and comparing whole
    /// documents is costly.
    compared: std::cell::Cell<Option<(u64, bool)>>,
}

impl Document {
    fn dirty(&self) -> bool {
        if let Some((at, dirty)) = self.compared.get()
            && at == self.revision
        {
            return dirty;
        }
        let dirty = self.value != self.saved;
        self.compared.set(Some((self.revision, dirty)));
        dirty
    }

    /// The saved copy changed (a save, a reload, a resolved conflict): compare again.
    fn saved_changed(&self) {
        self.compared.set(None);
    }
}

/// What a change on disk did to a document ([`Store::outside_change`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Outside {
    /// Nothing to do: not loaded, or the file holds what the editor last read or saved.
    Unchanged,
    /// The document had no unsaved edits and now shows the file.
    Reloaded,
    /// The document has unsaved edits and the file changed too: someone must choose.
    Conflict,
    /// The file is not valid JSON (yet: an agent or git may be writing it).
    Unreadable(String),
}

/// The documents of one package, loaded on first use, and the undo history.
pub struct Store {
    root: PathBuf,
    documents: BTreeMap<String, Document>,
    grids: BTreeMap<String, GridDoc>,
    /// Documents changed on disk while they had unsaved edits, with the file's version.
    conflicts: BTreeMap<String, Option<Value>>,
    done: Vec<Step>,
    undone: Vec<Step>,
    revision: u64,
    /// Counts creations and deletions: the documents that exist changed.
    structure: u64,
}

/// The one file format: 2-space JSON in the document's key order, with a final newline.
pub fn format(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("JSON values serialize");
    text.push('\n');
    text
}

impl Store {
    /// The documents below `root` (a package's folder).
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            documents: BTreeMap::new(),
            grids: BTreeMap::new(),
            conflicts: BTreeMap::new(),
            done: Vec::new(),
            undone: Vec::new(),
            revision: 0,
            structure: 0,
        }
    }

    fn load(&mut self, document: &str) -> Result<&mut Document, Error> {
        if !self.documents.contains_key(document) {
            let path = self.root.join(document);
            let value = if path.is_file() {
                let text = std::fs::read_to_string(&path).map_err(|e| Error::Read {
                    document: document.into(),
                    message: e.to_string(),
                })?;
                Some(serde_json::from_str(&text).map_err(|e| Error::Read {
                    document: document.into(),
                    message: e.to_string(),
                })?)
            } else {
                None
            };
            self.documents.insert(
                document.to_string(),
                Document {
                    saved: value.clone(),
                    value,
                    revision: 0,
                    compared: std::cell::Cell::new(None),
                },
            );
        }
        Ok(self.documents.get_mut(document).expect("just loaded"))
    }

    /// A document's current value (`None` when it does not exist).
    pub fn get(&mut self, document: &str) -> Result<Option<&Value>, Error> {
        if is_grid(document) {
            return Err(Error::Read {
                document: document.into(),
                message: "a grid, not a document".into(),
            });
        }
        Ok(self.load(document)?.value.as_ref())
    }

    fn read_grid(&self, path: &str) -> Result<Option<GridData>, Error> {
        let file = self.root.join(path);
        if !file.is_file() {
            return Ok(None);
        }
        let failed = |message: String| Error::Read {
            document: path.into(),
            message,
        };
        let bytes = std::fs::read(&file).map_err(|e| failed(e.to_string()))?;
        let (grid, element, width, height) = Grid::read_file(&bytes).map_err(failed)?;
        Ok(Some(GridData {
            element,
            width,
            height,
            bytes: grid.bytes().to_vec(),
        }))
    }

    fn load_grid(&mut self, path: &str) -> Result<&mut GridDoc, Error> {
        if !self.grids.contains_key(path) {
            let value = self.read_grid(path)?;
            self.grids.insert(
                path.to_string(),
                GridDoc {
                    saved: value.clone(),
                    value,
                    revision: 0,
                    compared: std::cell::Cell::new(None),
                },
            );
        }
        Ok(self.grids.get_mut(path).expect("just loaded"))
    }

    /// Counts creations and deletions of documents (libraries list what exists).
    pub fn structure(&self) -> u64 {
        self.structure
    }

    /// Counts every change to any document: a check is current while it matches.
    pub fn generation(&self) -> u64 {
        self.revision
    }

    /// The unsaved JSON documents only (what the content check reads; grids are maps').
    pub fn document_overlay(&self) -> canopy_content::package::Overlay {
        canopy_content::package::Overlay(
            self.documents
                .iter()
                .filter(|(_, d)| d.dirty())
                .map(|(path, d)| {
                    (
                        path.clone(),
                        d.value.as_ref().map(|v| format(v).into_bytes()),
                    )
                })
                .collect(),
        )
    }

    /// The unsaved documents and grids as the loader reads them: their file bytes, `None`
    /// when deleted (grids uncompressed: they never reach a package this way).
    pub fn overlay(&self) -> canopy_content::package::Overlay {
        let documents = self
            .documents
            .iter()
            .filter(|(_, d)| d.dirty())
            .map(|(path, d)| {
                (
                    path.clone(),
                    d.value.as_ref().map(|v| format(v).into_bytes()),
                )
            });
        let grids = self
            .grids
            .iter()
            .filter(|(_, g)| g.dirty())
            .map(|(path, g)| {
                let file = g
                    .value
                    .as_ref()
                    .map(|v| Grid(v.bytes.clone()).to_raw_file(v.element, v.width, v.height));
                (path.clone(), file)
            });
        canopy_content::package::Overlay(documents.chain(grids).collect())
    }

    /// Changes so far of `document` (0 until it changes).
    pub fn revision(&self, document: &str) -> u64 {
        if is_grid(document) {
            return self.grids.get(document).map_or(0, |g| g.revision);
        }
        self.documents.get(document).map_or(0, |d| d.revision)
    }

    /// Whether `document` differs from its file.
    pub fn is_dirty(&self, document: &str) -> bool {
        if is_grid(document) {
            return self.grids.get(document).is_some_and(GridDoc::dirty);
        }
        self.documents.get(document).is_some_and(Document::dirty)
    }

    /// Every document and grid that differs from its file, in path order.
    pub fn dirty(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .documents
            .iter()
            .filter(|(_, d)| d.dirty())
            .map(|(p, _)| p.as_str())
            .chain(
                self.grids
                    .iter()
                    .filter(|(_, g)| g.dirty())
                    .map(|(p, _)| p.as_str()),
            )
            .collect();
        out.sort();
        out
    }

    /// Runs `commands` as one undo step; all or nothing.
    pub fn run(
        &mut self,
        label: &str,
        context: Context,
        commands: Vec<Command>,
    ) -> Result<(), Error> {
        self.run_merging(label, context, commands, None)
    }

    /// Like [`Store::run`]; with `merge`, folds into the last step when it carries the same
    /// key and came moments ago (dragging a number is one step, not hundreds).
    pub fn run_merging(
        &mut self,
        label: &str,
        context: Context,
        commands: Vec<Command>,
        merge: Option<&str>,
    ) -> Result<(), Error> {
        let mut inverse = Vec::with_capacity(commands.len());
        for command in &commands {
            match self.apply(command) {
                Ok(back) => inverse.push(back),
                Err(e) => {
                    for back in inverse.iter().rev() {
                        self.apply(back).expect("an inverse applies");
                    }
                    return Err(e);
                }
            }
        }
        if commands.is_empty() {
            return Ok(());
        }
        let now = std::time::Instant::now();
        if let (Some(key), Some(last)) = (merge, self.done.last_mut())
            && self.undone.is_empty()
            && last.merge.as_deref() == Some(key)
            && now.duration_since(last.at) < std::time::Duration::from_millis(1500)
        {
            last.done.extend(commands);
            last.inverse.extend(inverse);
            last.at = now;
            return Ok(());
        }
        self.done.push(Step {
            label: label.to_string(),
            context,
            done: commands,
            inverse,
            merge: merge.map(str::to_string),
            at: now,
        });
        self.undone.clear();
        Ok(())
    }

    /// Takes back the last step; returns its context.
    pub fn undo(&mut self) -> Option<Context> {
        let step = self.done.pop()?;
        for back in step.inverse.iter().rev() {
            self.apply(back).expect("an inverse applies");
        }
        let context = step.context.clone();
        self.undone.push(step);
        Some(context)
    }

    /// Does the last undone step again; returns its context.
    pub fn redo(&mut self) -> Option<Context> {
        let mut step = self.undone.pop()?;
        step.inverse = step
            .done
            .iter()
            .map(|c| self.apply(c).expect("a redone command applies"))
            .collect();
        let context = step.context.clone();
        self.done.push(step);
        Some(context)
    }

    /// The label of the step undo would take back.
    pub fn undo_label(&self) -> Option<&str> {
        self.done.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.undone.last().map(|s| s.label.as_str())
    }

    /// The file of `document` changed on disk (an agent, git, another editor).
    pub fn outside_change(&mut self, document: &str) -> Outside {
        if is_grid(document) {
            return self.outside_grid(document);
        }
        let Some(current) = self.documents.get(document) else {
            return Outside::Unchanged;
        };
        let file = self.root.join(document);
        let disk = match std::fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => Some(value),
                Err(error) => return Outside::Unreadable(format!("{document}: {error}")),
            },
            Err(_) if !file.exists() => None,
            Err(error) => return Outside::Unreadable(format!("{document}: {error}")),
        };
        if disk == current.saved {
            return Outside::Unchanged;
        }
        if current.value == current.saved {
            self.take(document, disk);
            Outside::Reloaded
        } else {
            self.conflicts.insert(document.to_string(), disk);
            Outside::Conflict
        }
    }

    /// A grid's file changed: it follows the file when it has no unsaved edits; with some,
    /// the edits stay (grids are not offered as conflicts) and the next save wins.
    fn outside_grid(&mut self, path: &str) -> Outside {
        let Some(current) = self.grids.get(path) else {
            return Outside::Unchanged;
        };
        let disk = match self.read_grid(path) {
            Ok(disk) => disk,
            Err(error) => return Outside::Unreadable(error.to_string()),
        };
        if disk == current.saved {
            return Outside::Unchanged;
        }
        if current.dirty() {
            if let Some(g) = self.grids.get_mut(path) {
                g.saved = disk;
                g.compared.set(None);
            }
            return Outside::Conflict;
        }
        self.revision += 1;
        if let Some(g) = self.grids.get_mut(path) {
            g.value = disk.clone();
            g.saved = disk;
            g.revision = self.revision;
        }
        let touches = |step: &Step| step.done.iter().any(|c| c.document() == path);
        self.done.retain(|s| !touches(s));
        self.undone.retain(|s| !touches(s));
        Outside::Reloaded
    }

    /// Documents changed on disk while they had unsaved edits.
    pub fn conflicts(&self) -> Vec<&str> {
        self.conflicts.keys().map(String::as_str).collect()
    }

    /// Settles a conflict: keep the unsaved edits (the next save overwrites the file), or
    /// take the file's version.
    pub fn resolve(&mut self, document: &str, keep_mine: bool) {
        let Some(disk) = self.conflicts.remove(document) else {
            return;
        };
        if keep_mine {
            if let Some(d) = self.documents.get_mut(document) {
                d.saved = disk;
                d.saved_changed();
            }
        } else {
            self.take(document, disk);
        }
    }

    /// Shows the file's version and forgets the undo steps that touched the document:
    /// they were made against a version that is gone.
    fn take(&mut self, document: &str, disk: Option<Value>) {
        self.revision += 1;
        self.structure += 1;
        if let Some(d) = self.documents.get_mut(document) {
            d.value = disk.clone();
            d.saved = disk;
            d.revision = self.revision;
            d.saved_changed();
        }
        let touches = |step: &Step| step.done.iter().any(|c| c.document() == document);
        self.done.retain(|s| !touches(s));
        self.undone.retain(|s| !touches(s));
    }

    /// Writes every changed document (and removes deleted ones); returns what it wrote.
    pub fn save(&mut self) -> Result<Vec<String>, Error> {
        let mut written = Vec::new();
        for (path, document) in &mut self.documents {
            if !document.dirty() {
                continue;
            }
            let file = self.root.join(path);
            let failed = |e: std::io::Error| Error::Write {
                document: path.clone(),
                message: e.to_string(),
            };
            match &document.value {
                Some(value) => {
                    if let Some(parent) = file.parent() {
                        std::fs::create_dir_all(parent).map_err(failed)?;
                    }
                    std::fs::write(&file, format(value)).map_err(failed)?;
                }
                None => {
                    std::fs::remove_file(&file).map_err(failed)?;
                    // An asset goes with its files, and its folder once nothing else is in it.
                    if is_asset(path)
                        && let Some(saved) = &document.saved
                    {
                        remove_asset_files(&file, saved).map_err(failed)?;
                    }
                }
            }
            document.saved = document.value.clone();
            document.saved_changed();
            written.push(path.clone());
        }
        for (path, grid) in &mut self.grids {
            if !grid.dirty() {
                continue;
            }
            let file = self.root.join(path);
            let failed = |e: std::io::Error| Error::Write {
                document: path.clone(),
                message: e.to_string(),
            };
            match &grid.value {
                Some(v) => {
                    if let Some(parent) = file.parent() {
                        std::fs::create_dir_all(parent).map_err(failed)?;
                    }
                    let bytes = Grid(v.bytes.clone()).to_file(v.element, v.width, v.height);
                    std::fs::write(&file, bytes).map_err(failed)?;
                }
                None => std::fs::remove_file(&file).map_err(failed)?,
            }
            grid.saved = grid.value.clone();
            grid.compared.set(Some((grid.revision, false)));
            written.push(path.clone());
        }
        written.sort();
        Ok(written)
    }

    /// Applies one command; returns the command that takes it back.
    fn apply(&mut self, command: &Command) -> Result<Command, Error> {
        self.revision += 1;
        let revision = self.revision;
        let path = command.document().to_string();
        if matches!(
            command,
            Command::Create { .. } | Command::Delete { .. } | Command::Grid { .. }
        ) {
            self.structure += 1;
        }
        if is_grid(&path) {
            return self.apply_grid(command, &path, revision);
        }
        let document = self.load(&path)?;
        let back = match command {
            Command::Create { value, .. } => {
                if document.value.is_some() {
                    return Err(Error::Exists(path));
                }
                document.value = Some(value.clone());
                Command::Delete { document: path }
            }
            Command::Delete { .. } => {
                let value = document
                    .value
                    .take()
                    .ok_or(Error::NoDocument(path.clone()))?;
                Command::Create {
                    document: path,
                    value,
                }
            }
            Command::Set {
                pointer, value, at, ..
            } => {
                let root = document
                    .value
                    .as_mut()
                    .ok_or(Error::NoDocument(path.clone()))?;
                set(root, &path, pointer, value.clone(), *at)?
            }
            Command::Insert {
                pointer,
                index,
                value,
                ..
            } => {
                let root = document
                    .value
                    .as_mut()
                    .ok_or(Error::NoDocument(path.clone()))?;
                let no_target = || Error::NoTarget {
                    document: path.clone(),
                    pointer: pointer.clone(),
                };
                let list = root
                    .pointer_mut(pointer)
                    .ok_or_else(no_target)?
                    .as_array_mut()
                    .ok_or_else(|| Error::NotAnArray {
                        document: path.clone(),
                        pointer: pointer.clone(),
                    })?;
                if *index > list.len() {
                    return Err(no_target());
                }
                list.insert(*index, value.clone());
                Command::Remove {
                    document: path.clone(),
                    pointer: format!("{pointer}/{index}"),
                }
            }
            Command::Remove { pointer, .. } => {
                let root = document
                    .value
                    .as_mut()
                    .ok_or(Error::NoDocument(path.clone()))?;
                remove(root, &path, pointer)?
            }
            Command::Grid { .. } | Command::Patch { .. } => {
                return Err(Error::Read {
                    document: path,
                    message: "grid commands need a .grid file".into(),
                });
            }
        };
        document.revision = revision;
        Ok(back)
    }
}

impl Store {
    /// Applies a command to a grid; returns the command that takes it back.
    fn apply_grid(
        &mut self,
        command: &Command,
        path: &str,
        revision: u64,
    ) -> Result<Command, Error> {
        let grid = self.load_grid(path)?;
        let back = match command {
            Command::Grid {
                element,
                width,
                height,
                bytes,
                ..
            } => {
                let size = element.element().size();
                if bytes.len() != *width as usize * *height as usize * size {
                    return Err(Error::Read {
                        document: path.into(),
                        message: format!("{} bytes for {width}×{height} samples", bytes.len()),
                    });
                }
                let new = GridData {
                    element: element.element(),
                    width: *width,
                    height: *height,
                    bytes: bytes.clone(),
                };
                match grid.value.replace(new) {
                    Some(old) => Command::Grid {
                        document: path.into(),
                        element: Sample::of(old.element),
                        width: old.width,
                        height: old.height,
                        bytes: old.bytes,
                    },
                    None => Command::Delete {
                        document: path.into(),
                    },
                }
            }
            Command::Delete { .. } => {
                let old = grid.value.take().ok_or(Error::NoDocument(path.into()))?;
                Command::Grid {
                    document: path.into(),
                    element: Sample::of(old.element),
                    width: old.width,
                    height: old.height,
                    bytes: old.bytes,
                }
            }
            Command::Patch {
                x,
                z,
                width,
                height,
                bytes,
                ..
            } => {
                let data = grid.value.as_mut().ok_or(Error::NoDocument(path.into()))?;
                let size = data.element.size();
                let fits = x + width <= data.width
                    && z + height <= data.height
                    && bytes.len() == *width as usize * *height as usize * size;
                if !fits {
                    return Err(Error::NoTarget {
                        document: path.into(),
                        pointer: format!("{x},{z} {width}×{height}"),
                    });
                }
                let row = *width as usize * size;
                let mut old = Vec::with_capacity(bytes.len());
                for r in 0..*height as usize {
                    let at = ((*z as usize + r) * data.width as usize + *x as usize) * size;
                    old.extend_from_slice(&data.bytes[at..at + row]);
                    data.bytes[at..at + row].copy_from_slice(&bytes[r * row..(r + 1) * row]);
                }
                Command::Patch {
                    document: path.into(),
                    x: *x,
                    z: *z,
                    width: *width,
                    height: *height,
                    bytes: old,
                }
            }
            _ => {
                return Err(Error::Read {
                    document: path.into(),
                    message: "a grid takes grid, patch and delete commands".into(),
                });
            }
        };
        grid.revision = revision;
        Ok(back)
    }
}

/// Splits a pointer into its parent and last token (unescaped).
fn split(pointer: &str) -> Option<(&str, String)> {
    let (parent, last) = pointer.rsplit_once('/')?;
    Some((parent, last.replace("~1", "/").replace("~0", "~")))
}

fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn set(
    root: &mut Value,
    document: &str,
    pointer: &str,
    value: Value,
    at: Option<usize>,
) -> Result<Command, Error> {
    let no_target = || Error::NoTarget {
        document: document.into(),
        pointer: pointer.into(),
    };
    let Some((parent, key)) = split(pointer) else {
        if !pointer.is_empty() {
            return Err(no_target());
        }
        let old = std::mem::replace(root, value);
        return Ok(Command::Set {
            document: document.into(),
            pointer: String::new(),
            value: old,
            at: None,
        });
    };
    match root.pointer_mut(parent).ok_or_else(no_target)? {
        Value::Object(map) => match map.get_mut(&key) {
            Some(slot) => {
                let old = std::mem::replace(slot, value);
                Ok(Command::Set {
                    document: document.into(),
                    pointer: pointer.into(),
                    value: old,
                    at: None,
                })
            }
            None => {
                insert_at(map, key, value, at);
                Ok(Command::Remove {
                    document: document.into(),
                    pointer: pointer.into(),
                })
            }
        },
        Value::Array(list) => {
            let slot = key
                .parse::<usize>()
                .ok()
                .and_then(|i| list.get_mut(i))
                .ok_or_else(no_target)?;
            let old = std::mem::replace(slot, value);
            Ok(Command::Set {
                document: document.into(),
                pointer: pointer.into(),
                value: old,
                at: None,
            })
        }
        _ => Err(no_target()),
    }
}

fn insert_at(map: &mut Map<String, Value>, key: String, value: Value, at: Option<usize>) {
    match at {
        Some(index) if index <= map.len() => {
            map.shift_insert(index, key, value);
        }
        _ => {
            map.insert(key, value);
        }
    }
}

fn remove(root: &mut Value, document: &str, pointer: &str) -> Result<Command, Error> {
    let no_target = || Error::NoTarget {
        document: document.into(),
        pointer: pointer.into(),
    };
    let (parent, key) = split(pointer).ok_or_else(no_target)?;
    match root.pointer_mut(parent).ok_or_else(no_target)? {
        Value::Object(map) => {
            let at = map.keys().position(|k| *k == key).ok_or_else(no_target)?;
            let old = map.shift_remove(&key).expect("present");
            Ok(Command::Set {
                document: document.into(),
                pointer: format!("{parent}/{}", escape(&key)),
                value: old,
                at: Some(at),
            })
        }
        Value::Array(list) => {
            let index = key
                .parse::<usize>()
                .ok()
                .filter(|i| *i < list.len())
                .ok_or_else(no_target)?;
            let old = list.remove(index);
            Ok(Command::Insert {
                document: document.into(),
                pointer: parent.into(),
                index,
                value: old,
            })
        }
        _ => Err(no_target()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;
    use serde_json::json;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("canopy-store-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(path.join("units")).unwrap();
            std::fs::write(
                path.join("units/knight.json"),
                format(&json!({"id": "unit.knight", "name": "Knight", "cost": {"wood": 5}, "tags": ["a", "b"]})),
            )
            .unwrap();
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn here(document: &str) -> Context {
        Context {
            editor: Kind::Units,
            document: document.into(),
            pointer: None,
        }
    }

    const KNIGHT: &str = "units/knight.json";

    #[test]
    fn a_deleted_asset_takes_its_files_and_empty_folder_on_save() {
        let folder = Folder::new("asset-delete");
        let dir = folder.0.join("assets/props/rock");
        std::fs::create_dir_all(&dir).unwrap();
        let asset = json!({"id": "asset.rock", "kind": "prop", "resources": [
            {"role": "geometry", "index": 1, "format": "glb"},
            {"role": "albedo", "index": 2, "format": "png"}
        ]});
        std::fs::write(dir.join("asset.json"), format(&asset)).unwrap();
        std::fs::write(dir.join("geometry.glb"), b"g").unwrap();
        std::fs::write(dir.join("albedo_2.png"), b"a").unwrap();
        let mut store = Store::new(&folder.0);
        let document = "assets/props/rock/asset.json";
        store.get(document).unwrap();
        store
            .run(
                "Delete",
                here(document),
                vec![Command::Delete {
                    document: document.into(),
                }],
            )
            .unwrap();
        assert!(
            dir.join("geometry.glb").exists(),
            "nothing goes before the save"
        );
        store.save().unwrap();
        assert!(!dir.exists());
        // A folder holding something else stays, with that file.
        let other = folder.0.join("assets/props/log");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(
            other.join("asset.json"),
            format(&json!({"id": "asset.log", "resources": []})),
        )
        .unwrap();
        std::fs::write(other.join("notes.txt"), b"keep").unwrap();
        let document = "assets/props/log/asset.json";
        store.get(document).unwrap();
        store
            .run(
                "Delete",
                here(document),
                vec![Command::Delete {
                    document: document.into(),
                }],
            )
            .unwrap();
        store.save().unwrap();
        assert!(other.join("notes.txt").exists() && !other.join("asset.json").exists());
    }

    fn grid_of<'a>(store: &'a mut Store, path: &str) -> &'a GridData {
        store.load_grid(path).unwrap().value.as_ref().unwrap()
    }

    /// The sample at (`x`, `z`) of an `f32` grid.
    fn f32_at(grid: &GridData, x: u32, z: u32) -> f32 {
        let i = (z as usize * grid.width as usize + x as usize) * 4;
        f32::from_le_bytes(grid.bytes[i..i + 4].try_into().unwrap())
    }

    fn set(pointer: &str, value: Value) -> Command {
        Command::Set {
            document: KNIGHT.into(),
            pointer: pointer.into(),
            value,
            at: None,
        }
    }

    fn text(store: &mut Store) -> String {
        format(store.get(KNIGHT).unwrap().unwrap())
    }

    #[test]
    fn undo_restores_values_and_key_order_and_redo_repeats() {
        let folder = Folder::new("undo");
        let mut store = Store::new(&folder.0);
        let original = text(&mut store);
        store
            .run(
                "Edit knight",
                here(KNIGHT),
                vec![
                    set("/name", json!("Paladin")),
                    set("/speed", json!(3)),
                    Command::Remove {
                        document: KNIGHT.into(),
                        pointer: "/cost".into(),
                    },
                    Command::Insert {
                        document: KNIGHT.into(),
                        pointer: "/tags".into(),
                        index: 1,
                        value: json!("x"),
                    },
                ],
            )
            .unwrap();
        let edited = text(&mut store);
        assert_eq!(
            store.get(KNIGHT).unwrap().unwrap(),
            &json!({"id": "unit.knight", "name": "Paladin", "tags": ["a", "x", "b"], "speed": 3})
        );
        assert!(store.is_dirty(KNIGHT));
        assert_eq!(store.undo().unwrap().document, KNIGHT);
        // Byte for byte: `cost` is back in its place.
        assert_eq!(text(&mut store), original);
        assert!(!store.is_dirty(KNIGHT));
        assert_eq!(store.redo_label(), Some("Edit knight"));
        store.redo().unwrap();
        assert_eq!(text(&mut store), edited);
        assert!(store.redo().is_none());
    }

    #[test]
    fn continuous_edits_fold_into_one_step() {
        let folder = Folder::new("merge");
        let mut store = Store::new(&folder.0);
        let original = text(&mut store);
        for wood in 6..10 {
            store
                .run_merging(
                    "Set wood",
                    here(KNIGHT),
                    vec![set("/cost/wood", json!(wood))],
                    Some("units/knight.json/cost/wood"),
                )
                .unwrap();
        }
        assert_eq!(store.get(KNIGHT).unwrap().unwrap()["cost"]["wood"], 9);
        store.undo().unwrap();
        assert_eq!(text(&mut store), original);
        assert!(store.undo().is_none());
        store.redo().unwrap();
        assert_eq!(store.get(KNIGHT).unwrap().unwrap()["cost"]["wood"], 9);
    }

    #[test]
    fn a_failing_step_changes_nothing() {
        let folder = Folder::new("atomic");
        let mut store = Store::new(&folder.0);
        let original = text(&mut store);
        let result = store.run(
            "Broken",
            here(KNIGHT),
            vec![
                set("/name", json!("Paladin")),
                set("/missing/deep", json!(1)),
            ],
        );
        assert!(matches!(result, Err(Error::NoTarget { .. })));
        assert_eq!(text(&mut store), original);
        assert!(store.undo().is_none());
    }

    #[test]
    fn save_writes_changed_documents_in_the_file_format() {
        let folder = Folder::new("save");
        let mut store = Store::new(&folder.0);
        let new = "units/squire.json";
        store
            .run(
                "Add squire",
                here(new),
                vec![
                    Command::Create {
                        document: new.into(),
                        value: json!({"id": "unit.squire", "name": "Squire"}),
                    },
                    set("/name", json!("Sir Knight")),
                ],
            )
            .unwrap();
        assert_eq!(store.dirty(), [KNIGHT, new]);
        assert_eq!(store.save().unwrap(), [KNIGHT, new]);
        assert!(store.dirty().is_empty());
        let written = std::fs::read_to_string(folder.0.join(new)).unwrap();
        assert_eq!(
            written,
            "{\n  \"id\": \"unit.squire\",\n  \"name\": \"Squire\"\n}\n"
        );
        let knight = std::fs::read_to_string(folder.0.join(KNIGHT)).unwrap();
        assert!(knight.starts_with("{\n  \"id\": \"unit.knight\",\n  \"name\": \"Sir Knight\","));
        // Undoing past a save makes the documents differ from their files again; a
        // deleted document's file goes on the next save.
        store.undo().unwrap();
        assert_eq!(store.dirty(), [KNIGHT, new]);
        store.save().unwrap();
        assert!(!folder.0.join(new).exists());
    }

    #[test]
    fn files_changed_on_disk_reload_or_conflict() {
        let folder = Folder::new("outside");
        let mut store = Store::new(&folder.0);
        let other = |name: &str| {
            format(
                &json!({"id": "unit.knight", "name": name, "cost": {"wood": 5}, "tags": ["a", "b"]}),
            )
        };
        store.get(KNIGHT).unwrap();
        // Unchanged files and the editor's own saves are not changes.
        assert_eq!(store.outside_change(KNIGHT), Outside::Unchanged);
        store
            .run("Rename", here(KNIGHT), vec![set("/name", json!("Paladin"))])
            .unwrap();
        store.save().unwrap();
        assert_eq!(store.outside_change(KNIGHT), Outside::Unchanged);
        // A clean document follows the file, and its old steps are forgotten.
        std::fs::write(folder.0.join(KNIGHT), other("Agent")).unwrap();
        assert_eq!(store.outside_change(KNIGHT), Outside::Reloaded);
        assert_eq!(store.get(KNIGHT).unwrap().unwrap()["name"], "Agent");
        assert!(store.undo().is_none());
        // Unsaved edits and a changed file: a conflict either side can win.
        store
            .run("Rename", here(KNIGHT), vec![set("/name", json!("Mine"))])
            .unwrap();
        std::fs::write(folder.0.join(KNIGHT), other("Theirs")).unwrap();
        assert_eq!(store.outside_change(KNIGHT), Outside::Conflict);
        assert_eq!(store.conflicts(), [KNIGHT]);
        store.resolve(KNIGHT, true);
        assert_eq!(store.get(KNIGHT).unwrap().unwrap()["name"], "Mine");
        assert!(store.is_dirty(KNIGHT));
        std::fs::write(folder.0.join(KNIGHT), other("Again")).unwrap();
        assert_eq!(store.outside_change(KNIGHT), Outside::Conflict);
        store.resolve(KNIGHT, false);
        assert_eq!(store.get(KNIGHT).unwrap().unwrap()["name"], "Again");
        assert!(!store.is_dirty(KNIGHT));
        std::fs::write(folder.0.join(KNIGHT), "{ half").unwrap();
        assert!(matches!(
            store.outside_change(KNIGHT),
            Outside::Unreadable(_)
        ));
    }

    #[test]
    fn grids_patch_undo_and_save_as_grid_files() {
        let folder = Folder::new("grids");
        let path = "maps/test.utcm/terrain/heights.grid";
        let heights: Vec<u8> = (0..16).flat_map(|i| (i as f32).to_le_bytes()).collect();
        let file = folder.0.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, Grid(heights.clone()).to_file(Element::F32, 4, 4)).unwrap();
        let mut store = Store::new(&folder.0);
        assert_eq!(f32_at(grid_of(&mut store, path), 1, 2), 9.0);
        // A 2×2 patch at (1, 1) raising four samples to 50.
        let patch: Vec<u8> = (0..4).flat_map(|_| 50f32.to_le_bytes()).collect();
        store
            .run(
                "Raise",
                here(path),
                vec![Command::Patch {
                    document: path.into(),
                    x: 1,
                    z: 1,
                    width: 2,
                    height: 2,
                    bytes: patch,
                }],
            )
            .unwrap();
        let grid = grid_of(&mut store, path).clone();
        assert_eq!(
            (
                f32_at(&grid, 1, 1),
                f32_at(&grid, 2, 2),
                f32_at(&grid, 3, 3)
            ),
            (50.0, 50.0, 15.0)
        );
        assert_eq!(store.dirty(), [path]);
        assert!(store.overlay().0.contains_key(path));
        store.undo().unwrap();
        assert_eq!(grid_of(&mut store, path).bytes, heights);
        assert!(store.dirty().is_empty());
        store.redo().unwrap();
        assert_eq!(store.save().unwrap(), [path]);
        let saved = Grid::read_file(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved.0.bytes(), grid.bytes.as_slice());
        // A patch out of the grid fails without changing anything.
        let wrong = Command::Patch {
            document: path.into(),
            x: 3,
            z: 3,
            width: 2,
            height: 2,
            bytes: vec![0; 16],
        };
        assert!(store.run("Wrong", here(path), vec![wrong]).is_err());
        assert!(store.dirty().is_empty());
    }

    /// Every document of the repository's base package is already in the file format, so
    /// a save changes only what was edited. Assets' own data files (anything in an asset
    /// folder but its `asset.json`) are resources, not documents, and are left alone.
    #[test]
    fn every_base_document_is_in_the_file_format() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../base");
        let mut files = Vec::new();
        let mut folders = vec![base.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e == "json") {
                    let relative = path.strip_prefix(&base).unwrap();
                    let resource = relative.starts_with("assets")
                        && relative.file_name().is_some_and(|n| n != "asset.json");
                    if !resource {
                        files.push(path);
                    }
                }
            }
        }
        assert!(!files.is_empty());
        let off: Vec<_> = files
            .iter()
            .filter(|path| {
                let text = std::fs::read_to_string(path).unwrap();
                let value: Value = serde_json::from_str(&text).unwrap();
                format(&value) != text
            })
            .map(|p| p.strip_prefix(&base).unwrap().display().to_string())
            .collect();
        assert!(off.is_empty(), "not in the file format: {off:?}");
    }
}
