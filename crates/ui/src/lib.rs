//! `canopy-ui`: the game's HUD and menu toolkit.
//!
//! The UI is a **retained tree built from data** with **immediate-style updates**:
//!
//! - Screens and widget templates are layout documents (`config/ui/layouts/*.json`)
//!   styled by a theme (`config/ui/theme.json`) written in CSS terms, so the old game's
//!   stylesheets port directly. Both reload while the game runs ([`Ui::reload`]).
//! - Each frame the game pushes its presentation models into named nodes
//!   ([`Ui::set_text`], [`Ui::set_value`], [`Ui::set_count`] for lists, …); setters
//!   compare and only invalidate what changed. Layout (taffy) and text shaping
//!   (cosmic-text) re-run only for changed nodes.
//! - [`Ui::frame`] returns a [`DrawList`]: instanced quads in as few batches as image
//!   textures and scissor rectangles allow, plus glyph-atlas uploads. A renderer draws it
//!   in one pass; [`gpu`] (feature `wgpu`) is the reference pass.
//! - [`Ui::input`] takes pointer, keyboard, text and IME events and reports whether the
//!   UI consumed them (so clicks don't fall through to the world). Interactions come
//!   back as [`Action`]s; the UI holds no gameplay logic.
//!
//! See `docs/ui.md`.

pub mod config;
pub mod draw;
pub mod geom;
#[cfg(feature = "wgpu")]
pub mod gpu;
mod input;
mod layout;
mod node;
mod paint;
pub mod style;
pub mod text;
#[cfg(feature = "winit")]
pub mod winit;

pub use config::{ConfigError, Kind, UiConfig};
pub use draw::{AtlasChanges, AtlasUpdate, Batch, DrawList, Quad, QuadKind, TextureId};
pub use geom::{Color, Rect, Vec2};
pub use input::{Cursor, InputEvent, Key, Modifiers, PointerButton};
pub use style::ImageKey;
pub use text::FontSources;

use node::{NodeSpec, Tree};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("{document}: {message}")]
    Layout { document: String, message: String },
}

/// A handle to an addressable node, stable across hot reloads and list resizes.
/// Setters on a handle whose node is gone do nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u32);

/// A texture region the UI can draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageDesc {
    pub texture: TextureId,
    /// Source size in pixels: nine-slice cuts and intrinsic sizes use it.
    pub size: [f32; 2],
    /// Normalised texture rectangle `u0, v0, u1, v1`.
    pub uv: [f32; 4],
    /// Texels are premultiplied by alpha (the asset pack stores straight alpha).
    pub premultiplied: bool,
    /// The renderer binds this texture through an sRGB view (e.g. the pack's
    /// `Bc7RgbaUnormSrgb`), so samples arrive linear; the pass re-encodes them.
    pub srgb_view: bool,
    /// Every texel samples with an alpha of exactly 1 (`canopy_assets::Texture::exactly_opaque`):
    /// what it covers is not drawn.
    pub opaque: bool,
}

impl ImageDesc {
    pub fn new(texture: TextureId, width: f32, height: f32) -> Self {
        Self {
            texture,
            size: [width, height],
            uv: [0.0, 0.0, 1.0, 1.0],
            premultiplied: true,
            srgb_view: false,
            opaque: false,
        }
    }
}

/// One cost row of a tooltip.
#[derive(Debug, Clone, PartialEq)]
pub struct Cost {
    pub image: Option<ImageKey>,
    pub amount: String,
}

/// Tooltip content. `body` and `title` accept markup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tooltip {
    pub title: String,
    pub hotkey: Option<String>,
    pub costs: Vec<Cost>,
    pub body: String,
    pub note: Option<String>,
}

/// What happened to a node.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    /// Pressed and released over the node. `count` is 2 for a double click.
    Click {
        button: PointerButton,
        modifiers: Modifiers,
        count: u8,
    },
    /// The node's hotkey was pressed.
    Hotkey {
        modifiers: Modifiers,
    },
    /// A toggle button or checkbox changed.
    Toggled(bool),
    /// A slider moved.
    Value(f32),
    /// A dropdown option was chosen.
    Selected(usize),
    /// A text field's text changed.
    TextChanged(String),
    /// Enter in a text field.
    Submitted(String),
    /// Copy or cut in a text field: put this on the clipboard.
    Copy(String),
    /// Paste requested in a text field: answer with [`Ui::paste`].
    PasteRequest,
    /// Pointer pressed, moved while held, or released on a `drag` node; `at` is the
    /// position within the node, 0–1 on each axis.
    DragStart {
        button: PointerButton,
        at: Vec2,
    },
    Drag {
        at: Vec2,
    },
    DragEnd {
        at: Vec2,
    },
}

/// An interaction for the game to act on.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    /// The node's handle, when it has an id.
    pub node: Option<NodeId>,
    /// The node's path (`hud/commands/3`), when it has one.
    pub path: Option<Arc<str>>,
    /// The `action` name from the layout file.
    pub name: Option<Arc<str>>,
    /// Index of the nearest list entry (repeat instance) containing the node.
    pub index: Option<usize>,
    pub kind: ActionKind,
}

/// Frame statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub nodes: usize,
    pub quads: usize,
    pub batches: usize,
    pub layout_runs: u64,
    pub shaped_blocks: u64,
}

/// Resolves a text library key to its string ([`UiConfig::localize`]).
type Lookup = dyn Fn(&str) -> Option<String>;

/// The UI: a tree of screens built from the config, its input state and its output.
pub struct Ui {
    config: UiConfig,
    images: style::ImageNames,
    image_table: Vec<Option<ImageDesc>>,
    templates: HashMap<String, Rc<NodeSpec>>,
    tree: Tree,
    text: text::TextSystem,
    input: input::InputState,
    viewport: [u32; 2],
    dpi: f32,
    time: f64,
    actions: Vec<Action>,
    draw: DrawList,
    hits: Vec<paint::Hit>,
    stats: Stats,
    /// Tooltips show (a player setting).
    tooltips: bool,
    /// Resolves layout strings written as text library keys on hot reload
    /// ([`UiConfig::localize`]).
    localizer: Option<Box<Lookup>>,
    /// Builds the paths [`Ui::child`] and [`Ui::find_in`] look up, without allocating.
    path: std::cell::RefCell<String>,
    /// The paint's deferred layers, and the layer being painted.
    deferred: (Vec<paint::Deferred>, Vec<paint::Deferred>),
}

impl Ui {
    /// Builds the UI from its config. Every screen document is built; those marked
    /// `visible` are shown.
    pub fn new(config: UiConfig, fonts: FontSources) -> Result<Self, UiError> {
        let mut sources = fonts;
        sources.bundled.extend(config.font_data.iter().cloned());
        let hotkey = style::color(&config.hotkey_color).unwrap_or(Color::hex(0xffd26a));
        let text = text::TextSystem::new(&sources, &config.fonts.families, hotkey);
        let mut ui = Self {
            config,
            images: style::ImageNames::default(),
            image_table: Vec::new(),
            templates: HashMap::new(),
            tree: Tree::new(),
            text,
            input: input::InputState::default(),
            viewport: [1, 1],
            dpi: 1.0,
            time: 0.0,
            actions: Vec::new(),
            draw: DrawList::default(),
            hits: Vec::new(),
            stats: Stats::default(),
            tooltips: true,
            localizer: None,
            path: std::cell::RefCell::new(String::new()),
            deferred: (Vec::new(), Vec::new()),
        };
        for name in ui.config.images.keys() {
            ui.images.intern(name);
        }
        ui.rebuild(false)?;
        Ok(ui)
    }

    /// Replaces the config (hot reload). Dynamic values set by the game (texts, values,
    /// list counts, visibility) are kept for nodes whose paths still exist. On error the
    /// old UI stays.
    pub fn reload(&mut self, config: UiConfig) -> Result<(), UiError> {
        let old = std::mem::replace(&mut self.config, config);
        for name in self.config.images.keys() {
            self.images.intern(name);
        }
        if let Err(e) = self.rebuild(true) {
            self.config = old;
            return Err(e);
        }
        Ok(())
    }

    /// Reloads from disk when a theme or layout file changed (debug builds call this
    /// a few times a second). Returns whether it reloaded.
    pub fn reload_if_changed(&mut self) -> Result<bool, UiError> {
        if !self.config.changed_on_disk() {
            return Ok(false);
        }
        let Some(dir) = self.config.dir().map(std::path::Path::to_path_buf) else {
            return Ok(false);
        };
        let mut config = UiConfig::load(&dir)?;
        if let Some(lookup) = &self.localizer {
            config.localize(lookup.as_ref());
        }
        self.reload(config)?;
        tracing::info!("ui reloaded");
        Ok(true)
    }

    /// Keeps resolving text library keys in layouts when they reload from disk.
    pub fn set_localizer(&mut self, lookup: impl Fn(&str) -> Option<String> + 'static) {
        self.localizer = Some(Box::new(lookup));
    }

    pub fn config(&self) -> &UiConfig {
        &self.config
    }

    /// Sets the target size in physical pixels and the display scale (logical → physical).
    pub fn set_viewport(&mut self, width: u32, height: u32, scale: f32) {
        let viewport = [width.max(1), height.max(1)];
        if viewport != self.viewport || scale != self.dpi {
            self.viewport = viewport;
            self.dpi = scale.max(0.1);
            self.tree.viewport_changed = true;
        }
    }

    /// Shows or hides a screen (a layout document with a root).
    pub fn show(&mut self, screen: &str, visible: bool) {
        if let Some(id) = self.find(screen) {
            self.set_visible(id, visible);
        }
    }

    pub fn is_shown(&self, screen: &str) -> bool {
        self.find(screen)
            .and_then(|id| self.tree.node_of(id))
            .is_some_and(|n| self.tree.node(n).visible)
    }

    // --- Images ----------------------------------------------------------------------

    /// The key for an image name (interning it).
    pub fn image_key(&mut self, name: &str) -> ImageKey {
        self.images.intern(name)
    }

    /// Supplies (or replaces) the texture behind an image name.
    pub fn register_image(&mut self, name: &str, desc: ImageDesc) -> ImageKey {
        let key = self.images.intern(name);
        let i = key.0 as usize;
        if self.image_table.len() <= i {
            self.image_table.resize(i + 1, None);
        }
        self.image_table[i] = Some(desc);
        key
    }

    /// The theme's images: `(name, asset path)` for the game to load and register.
    pub fn theme_images(&self) -> impl Iterator<Item = (&str, &str)> {
        self.config
            .images
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    // --- Output ----------------------------------------------------------------------

    /// Lays out (where needed), updates tooltips and paints. `time` is in seconds and
    /// drives tooltip delays, double clicks and caret blinking.
    pub fn frame(&mut self, time: f64) -> &DrawList {
        let _span = tracing::trace_span!("ui frame").entered();
        self.time = time;
        self.update_tooltip();
        self.layout();
        self.paint();
        &self.draw
    }

    /// Starts the glyph atlas afresh: the next frame re-rasterises visible text and
    /// sets [`AtlasChanges::reset`]. Call it when the renderer is created after the UI
    /// or loses its textures, since atlas updates are incremental: every frame's draw
    /// list must reach the renderer.
    pub fn reset_glyph_atlas(&mut self) {
        self.text.atlas.reset(false);
    }

    /// Turns tooltips on or off (on by default); off hides the one showing.
    pub fn set_tooltips(&mut self, on: bool) {
        self.tooltips = on;
    }

    /// The last frame's draw list.
    pub fn draw_list(&self) -> &DrawList {
        &self.draw
    }

    /// Actions since the last call.
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }

    pub fn stats(&self) -> Stats {
        Stats {
            nodes: self.tree.len(),
            quads: self.draw.quads.len(),
            batches: self.draw.batches.len(),
            ..self.stats
        }
    }
}
