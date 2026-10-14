//! The UI's data: the theme (`config/ui/theme.json`), layout documents
//! (`config/ui/layouts/*.json`) and bundled fonts (`config/ui/fonts/`).
//!
//! Loading reads and parses JSON only; styles are compiled by [`crate::Ui`] so image
//! names stay interned across hot reloads.

use crate::style::{RawStyle, StyleError};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{file}: {message}")]
    Parse { file: String, message: String },
    #[error("{file}: style {style:?}: {source}")]
    Style {
        file: String,
        style: String,
        source: StyleError,
    },
}

/// How tooltips appear.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TooltipConfig {
    /// The layout template drawn as the tooltip box.
    pub template: String,
    /// Hover time before the first tooltip shows. Moving to another tooltip while one is
    /// visible (or within `warm_ms` of hiding one) shows it at once.
    #[serde(default)]
    pub delay_ms: u32,
    #[serde(default = "default_warm")]
    pub warm_ms: u32,
    /// Width when the target names no anchor.
    #[serde(default = "default_tooltip_width")]
    pub width: f32,
    /// Distance from the target.
    #[serde(default = "default_tooltip_gap")]
    pub gap: f32,
    /// Distance from the screen edges.
    #[serde(default = "default_tooltip_margin")]
    pub margin: f32,
}

fn default_warm() -> u32 {
    400
}
fn default_tooltip_width() -> f32 {
    320.0
}
fn default_tooltip_gap() -> f32 {
    10.0
}
fn default_tooltip_margin() -> f32 {
    8.0
}

/// Font role → family stack; the first family present wins.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontConfig {
    /// Font files relative to the config folder, bundled with the game.
    #[serde(default)]
    pub files: Vec<String>,
    /// Role (`ui`, `serif`, `dock`, …) → family names. `system-ui`, `serif`, `sans-serif`
    /// and `monospace` name the platform's generic fonts when system fonts are loaded.
    #[serde(default)]
    pub families: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct RawTheme {
    #[serde(default)]
    palette: BTreeMap<String, String>,
    #[serde(default)]
    fonts: FontConfig,
    /// Image name → asset path (an entry of the asset pack).
    #[serde(default)]
    images: BTreeMap<String, String>,
    tooltip: TooltipConfig,
    /// Text colour of `[key]` markup.
    #[serde(default = "default_hotkey")]
    hotkey_color: String,
    /// Pixels scrolled per wheel line.
    #[serde(default = "default_scroll_line")]
    scroll_line: f32,
    #[serde(default)]
    styles: BTreeMap<String, Value>,
}

fn default_hotkey() -> String {
    "#ffd26a".into()
}
fn default_scroll_line() -> f32 {
    40.0
}

/// Kinds of nodes in a layout document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A box: background, borders, images, children. Scrolls with `overflow: scroll`.
    #[default]
    Box,
    /// A clickable box (command cells, menu buttons, slots); hotkey, toggle, tooltip.
    Button,
    /// An image (`image` property, or the style's `background-image`).
    Image,
    /// Text with markup.
    Label,
    /// A value bar (health, mana, progress).
    Bar,
    /// A clockwise sweep from twelve o'clock (cooldowns, experience ring).
    Sweep,
    TextField,
    Slider,
    Checkbox,
    Dropdown,
}

/// Static tooltip content in a layout file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTooltip {
    pub title: String,
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub note: Option<String>,
}

/// A node as written in a layout file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawNode {
    /// Addressable name; paths join the ids of addressable ancestors with `/`.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub kind: Kind,
    /// Theme styles, space separated, applied in order.
    #[serde(default)]
    pub class: String,
    /// Inline declarations, applied after the classes.
    #[serde(default)]
    pub style: Option<Value>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub value: Option<f32>,
    #[serde(default)]
    pub min: Option<f32>,
    #[serde(default)]
    pub max: Option<f32>,
    #[serde(default)]
    pub step: Option<f32>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub selected: usize,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub max_length: Option<usize>,
    /// Name reported with this node's actions.
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub hotkey: Option<String>,
    #[serde(default)]
    pub tooltip: Option<RawTooltip>,
    /// Id of an ancestor whose box places and sizes this node's tooltip.
    #[serde(default)]
    pub tooltip_anchor: Option<String>,
    /// Reports presses, moves while held and releases with positions
    /// ([`crate::ActionKind::DragStart`]…), e.g. the minimap.
    #[serde(default)]
    pub drag: bool,
    /// A button that stays pressed (`:checked`) when clicked.
    #[serde(default)]
    pub toggle: bool,
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Children are instances of this template; the game sets how many.
    #[serde(default)]
    pub repeat: Option<String>,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub children: Vec<RawNode>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDocument {
    #[serde(default)]
    pub templates: BTreeMap<String, RawNode>,
    /// A screen. Documents with only templates have none.
    #[serde(default)]
    pub root: Option<RawNode>,
    /// Shown when the UI starts.
    #[serde(default)]
    pub visible: bool,
}

/// Everything the UI is built from.
#[derive(Debug, Clone)]
pub struct UiConfig {
    pub palette: BTreeMap<String, String>,
    pub fonts: FontConfig,
    /// Bytes of `fonts.files`, in order.
    pub font_data: Vec<Vec<u8>>,
    pub images: BTreeMap<String, String>,
    pub tooltip: TooltipConfig,
    pub hotkey_color: String,
    pub scroll_line: f32,
    pub styles: BTreeMap<String, RawStyle>,
    /// Screen and template documents by name (file stem).
    pub documents: BTreeMap<String, RawDocument>,
    watched: Vec<(PathBuf, Option<SystemTime>)>,
}

impl UiConfig {
    /// Loads `theme.json`, `layouts/*.json` and the theme's font files from `dir`
    /// (the repository's `config/ui`).
    pub fn load(dir: &Path) -> Result<Self, ConfigError> {
        let theme_path = dir.join("theme.json");
        let theme = read_string(&theme_path)?;
        let mut documents = Vec::new();
        let layouts = dir.join("layouts");
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&layouts)
            .map_err(|source| ConfigError::Io {
                path: layouts.clone(),
                source,
            })?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        entries.sort();
        for path in &entries {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            documents.push((name, read_string(path)?));
        }
        let mut config = Self::parse(&theme, &documents, |file| {
            let path = dir.join(file);
            std::fs::read(&path).map_err(|source| ConfigError::Io { path, source })
        })?;
        config.watched = std::iter::once(theme_path)
            .chain(entries)
            .map(|p| {
                let t = modified(&p);
                (p, t)
            })
            .collect();
        Ok(config)
    }

    /// Builds a config from JSON text; `read_font` supplies the theme's font files.
    pub fn parse(
        theme: &str,
        documents: &[(String, String)],
        mut read_font: impl FnMut(&str) -> Result<Vec<u8>, ConfigError>,
    ) -> Result<Self, ConfigError> {
        let raw: RawTheme = from_json("theme.json", theme)?;
        let mut styles = BTreeMap::new();
        for (name, value) in &raw.styles {
            let style = RawStyle::from_json(value).map_err(|source| ConfigError::Style {
                file: "theme.json".into(),
                style: name.clone(),
                source,
            })?;
            styles.insert(name.clone(), style);
        }
        let mut docs = BTreeMap::new();
        for (name, text) in documents {
            let doc: RawDocument = from_json(&format!("layouts/{name}.json"), text)?;
            docs.insert(name.clone(), doc);
        }
        let font_data = raw
            .fonts
            .files
            .iter()
            .map(|f| read_font(f))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            palette: raw.palette,
            fonts: raw.fonts,
            font_data,
            images: raw.images,
            tooltip: raw.tooltip,
            hotkey_color: raw.hotkey_color,
            scroll_line: raw.scroll_line,
            styles,
            documents: docs,
            watched: Vec::new(),
        })
    }

    /// Whether a loaded file changed on disk since loading (for hot reload).
    pub fn changed_on_disk(&self) -> bool {
        self.watched.iter().any(|(p, t)| modified(p) != *t)
            || self.watched.first().is_some_and(|(theme, _)| {
                // A new layout file appeared.
                let dir = theme.parent().map(|d| d.join("layouts"));
                let count = dir
                    .and_then(|d| std::fs::read_dir(d).ok())
                    .map(|r| {
                        r.filter_map(Result::ok)
                            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                            .count()
                    })
                    .unwrap_or(0);
                count + 1 != self.watched.len()
            })
    }

    /// The folder this config was loaded from.
    pub fn dir(&self) -> Option<&Path> {
        self.watched.first().and_then(|(p, _)| p.parent())
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn read_string(path: &Path) -> Result<String, ConfigError> {
    std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_owned(),
        source,
    })
}

fn from_json<T: serde::de::DeserializeOwned>(file: &str, text: &str) -> Result<T, ConfigError> {
    let de = &mut serde_json::Deserializer::from_str(text);
    serde_path_to_error::deserialize(de).map_err(|e| ConfigError::Parse {
        file: file.to_owned(),
        message: e.to_string(),
    })
}
