//! The node tree: compiled layout documents, building, paths and handles, and the
//! setters the game uses to push its models in.

use crate::config::{Kind, RawNode};
use crate::geom::{Color, Rect, Vec2};
use crate::input::Hotkey;
use crate::style::{ImageKey, PointerEvents, RawStyle, Style, TextStyle};
use crate::text::TextBlock;
use crate::{NodeId, Tooltip, Ui, UiError};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use taffy::TaffyTree;
use taffy::style::Display;

/// Widget parts styled by `<class>::<part>` theme entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Track,
    Fill,
    Thumb,
    Box,
    Mark,
    Arrow,
    Popup,
    Option,
    Caret,
    Selection,
    Placeholder,
    Scrollbar,
    ScrollThumb,
}

impl Part {
    fn name(self) -> &'static str {
        match self {
            Part::Track => "track",
            Part::Fill => "fill",
            Part::Thumb => "thumb",
            Part::Box => "box",
            Part::Mark => "mark",
            Part::Arrow => "arrow",
            Part::Popup => "popup",
            Part::Option => "option",
            Part::Caret => "caret",
            Part::Selection => "selection",
            Part::Placeholder => "placeholder",
            Part::Scrollbar => "scrollbar",
            Part::ScrollThumb => "scroll-thumb",
        }
    }

    fn for_kind(kind: Kind, scroll: bool) -> &'static [Part] {
        if scroll {
            return &[Part::Scrollbar, Part::ScrollThumb];
        }
        match kind {
            Kind::Slider => &[Part::Track, Part::Fill, Part::Thumb],
            Kind::Checkbox => &[Part::Box, Part::Mark],
            Kind::Dropdown => &[Part::Arrow, Part::Popup, Part::Option],
            Kind::TextField => &[Part::Caret, Part::Selection, Part::Placeholder],
            _ => &[],
        }
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Box => "box",
        Kind::Button => "button",
        Kind::Image => "image",
        Kind::Label => "label",
        Kind::Bar => "bar",
        Kind::Sweep => "sweep",
        Kind::TextField => "text_field",
        Kind::Slider => "slider",
        Kind::Checkbox => "checkbox",
        Kind::Dropdown => "dropdown",
    }
}

/// A node of a layout document, compiled.
#[derive(Debug)]
pub(crate) struct NodeSpec {
    pub id: Option<String>,
    pub kind: Kind,
    pub style: Rc<Style>,
    pub parts: Vec<(Part, Rc<Style>)>,
    pub text: Option<String>,
    pub image: Option<ImageKey>,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub options: Vec<String>,
    pub selected: usize,
    pub placeholder: Option<String>,
    pub max_length: usize,
    pub action: Option<Arc<str>>,
    pub hotkey: Option<Hotkey>,
    pub tooltip: Option<Arc<Tooltip>>,
    pub tooltip_anchor: Option<String>,
    pub toggle: bool,
    pub drag: bool,
    pub checked: bool,
    pub visible: bool,
    pub enabled: bool,
    pub repeat: Option<String>,
    pub count: usize,
    pub children: Vec<Rc<NodeSpec>>,
}

impl NodeSpec {
    pub fn part(&self, part: Part) -> Option<&Style> {
        self.parts
            .iter()
            .find(|(p, _)| *p == part)
            .map(|(_, s)| s.as_ref())
    }

    pub fn interactive(&self) -> bool {
        matches!(
            self.kind,
            Kind::Button | Kind::TextField | Kind::Slider | Kind::Checkbox | Kind::Dropdown
        ) || self.action.is_some()
            || self.drag
    }

    pub fn focusable(&self) -> bool {
        matches!(
            self.kind,
            Kind::Button | Kind::TextField | Kind::Slider | Kind::Checkbox | Kind::Dropdown
        )
    }

    /// Reacts to the pointer itself (its own states and tooltip), rather than through
    /// an ancestor. A styled part with states inside a button follows the button.
    fn owner(&self, inside_owner: bool) -> bool {
        self.interactive()
            || self.tooltip.is_some()
            || self.hotkey.is_some()
            || self.tooltip_anchor.is_some()
            || (self.style.has_states() && !inside_owner)
    }

    fn has_text(&self) -> bool {
        self.text.is_some()
            || matches!(
                self.kind,
                Kind::Label | Kind::TextField | Kind::Checkbox | Kind::Dropdown
            )
    }
}

/// Values the game has set on a node, kept across hot reloads.
#[derive(Debug, Clone, Default)]
pub(crate) struct Overrides {
    pub visible: Option<bool>,
    pub enabled: Option<bool>,
    pub checked: Option<bool>,
    pub value: Option<f32>,
    pub image: Option<Option<ImageKey>>,
    pub background_image: Option<ImageKey>,
    pub color: Option<Option<Color>>,
    pub tooltip: Option<Option<Arc<Tooltip>>>,
    pub hotkey: Option<Option<Hotkey>>,
    pub text: Option<String>,
    pub options: Option<Vec<String>>,
    pub selected: Option<usize>,
    pub count: Option<usize>,
    pub scale: Option<Option<f32>>,
    pub opacity: Option<f32>,
    pub shift: Option<Vec2>,
}

/// Editing state of a text field.
#[derive(Debug, Clone, Default)]
pub(crate) struct FieldState {
    pub cursor: usize,
    pub anchor: usize,
    pub preedit: String,
    pub scroll: f32,
}

pub(crate) struct Node {
    pub spec: Rc<NodeSpec>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub taffy: taffy::NodeId,
    pub path: Option<Arc<str>>,
    pub handle: Option<u32>,
    pub repeat_index: Option<usize>,
    /// Nearest ancestor-or-self that reacts to the pointer (states, tooltips).
    pub owner: Option<usize>,
    pub set: Overrides,

    pub visible: bool,
    pub enabled: bool,
    pub checked: bool,
    pub value: f32,
    pub image: Option<ImageKey>,
    pub color: Option<Color>,
    pub tooltip: Option<Arc<Tooltip>>,
    pub hotkey: Option<Hotkey>,
    pub options: Vec<String>,
    pub selected: usize,
    pub count: usize,
    pub scale_override: Option<f32>,
    /// Opacity set by the game, multiplied into the subtree's (fades).
    pub opacity: f32,
    /// Translation set by the game, logical pixels (animations).
    pub shift: Vec2,

    pub text: Option<TextBlock>,
    /// A text field's placeholder.
    pub placeholder: Option<TextBlock>,
    /// A dropdown's options while it is open.
    pub option_text: Vec<TextBlock>,
    pub text_style: TextStyle,
    pub pointer: PointerEvents,
    pub layer: i32,
    pub field: FieldState,
    pub open: bool,

    // Geometry from the last layout, physical pixels.
    pub rect: Rect,
    pub content: Rect,
    pub clip: Option<Rect>,
    pub scale: f32,
    /// Scroll offset and the scrollable extent beyond the box (logical pixels).
    pub scroll: Vec2,
    pub scroll_max: Vec2,
    /// Extra translation (tooltips), logical pixels.
    pub offset: Vec2,
    /// Ancestor display scale at layout time (for text shaping).
    pub text_scale: f32,
}

impl Node {
    pub fn kind(&self) -> Kind {
        self.spec.kind
    }
}

pub(crate) struct Tree {
    pub taffy: TaffyTree<usize>,
    pub nodes: Vec<Option<Node>>,
    free: Vec<usize>,
    pub root: Option<usize>,
    handles: Vec<Option<usize>>,
    paths: HashMap<Arc<str>, u32>,
    pub layout_dirty: bool,
    pub viewport_changed: bool,
    /// Text blocks whose text changed without needing layout.
    pub tooltip_root: Option<usize>,
}

impl Tree {
    pub fn new() -> Self {
        let mut taffy = TaffyTree::new();
        taffy.disable_rounding();
        Self {
            taffy,
            nodes: Vec::new(),
            free: Vec::new(),
            root: None,
            handles: Vec::new(),
            paths: HashMap::new(),
            layout_dirty: true,
            viewport_changed: true,
            tooltip_root: None,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len() - self.free.len()
    }

    pub fn node(&self, i: usize) -> &Node {
        self.nodes[i].as_ref().expect("live node")
    }

    pub fn node_mut(&mut self, i: usize) -> &mut Node {
        self.nodes[i].as_mut().expect("live node")
    }

    pub fn node_of(&self, id: NodeId) -> Option<usize> {
        self.handles.get(id.0 as usize).copied().flatten()
    }

    pub fn handle(&mut self, path: &Arc<str>) -> u32 {
        if let Some(h) = self.paths.get(path) {
            return *h;
        }
        let h = self.handles.len() as u32;
        self.handles.push(None);
        self.paths.insert(path.clone(), h);
        h
    }

    pub fn find(&self, path: &str) -> Option<NodeId> {
        let h = *self.paths.get(path)?;
        self.handles[h as usize].map(|_| NodeId(h))
    }

    fn insert(&mut self, node: Node) -> usize {
        let path = node.path.clone();
        let index = match self.free.pop() {
            Some(i) => {
                self.nodes[i] = Some(node);
                i
            }
            None => {
                self.nodes.push(Some(node));
                self.nodes.len() - 1
            }
        };
        if let Some(path) = path {
            let h = self.handle(&path);
            self.handles[h as usize] = Some(index);
            self.node_mut(index).handle = Some(h);
        }
        index
    }

    /// Removes a subtree.
    pub fn remove(&mut self, index: usize) {
        let node = self.nodes[index].take().expect("live node");
        for c in node.children {
            self.remove(c);
        }
        if let Some(h) = node.handle {
            self.handles[h as usize] = None;
        }
        let _ = self.taffy.remove(node.taffy);
        self.free.push(index);
    }

    /// Indices of a subtree, depth first.
    pub fn subtree(&self, index: usize, out: &mut Vec<usize>) {
        out.push(index);
        for &c in &self.node(index).children {
            self.subtree(c, out);
        }
    }

    pub fn mark_dirty(&mut self, index: usize) {
        let t = self.node(index).taffy;
        let _ = self.taffy.mark_dirty(t);
        self.layout_dirty = true;
    }
}

// --- Compiling documents --------------------------------------------------------------

struct Compiler<'a> {
    config: &'a crate::UiConfig,
    images: &'a mut crate::style::ImageNames,
    cache: HashMap<String, Rc<Style>>,
    document: String,
}

impl Compiler<'_> {
    fn error(&self, message: impl Into<String>) -> UiError {
        UiError::Layout {
            document: self.document.clone(),
            message: message.into(),
        }
    }

    fn style(
        &mut self,
        names: &[&str],
        inline: Option<&serde_json::Value>,
        base: Option<&RawStyle>,
    ) -> Result<Rc<Style>, UiError> {
        let key = format!(
            "{}|{}|{}",
            names.join(" "),
            inline.map(|v| v.to_string()).unwrap_or_default(),
            base.is_some()
        );
        if let Some(s) = self.cache.get(&key) {
            return Ok(s.clone());
        }
        let mut raw = base.cloned().unwrap_or_default();
        for name in names {
            match self.config.styles.get(*name) {
                Some(s) => raw.merge(s),
                None if name.contains("::") || name.starts_with('@') => {}
                None => return Err(self.error(format!("unknown style {name:?}"))),
            }
        }
        if let Some(inline) = inline {
            let inline = RawStyle::from_json(inline).map_err(|e| self.error(e.to_string()))?;
            raw.merge(&inline);
        }
        let style = Style::compile(&raw, &self.config.palette, self.images)
            .map_err(|e| self.error(format!("style {:?}: {e}", names.join(" "))))?;
        let style = Rc::new(style);
        self.cache.insert(key, style.clone());
        Ok(style)
    }

    fn node(&mut self, raw: &RawNode, base: Option<&RawStyle>) -> Result<Rc<NodeSpec>, UiError> {
        let classes: Vec<&str> = raw.class.split_whitespace().collect();
        let mut names = vec![];
        let kind_default = format!("@{}", kind_name(raw.kind));
        names.push(kind_default.as_str());
        names.extend(classes.iter().copied());
        let style = self.style(&names, raw.style.as_ref(), base)?;
        let mut parts = Vec::new();
        for &part in Part::for_kind(raw.kind, style.scroll) {
            let owner = if style.scroll {
                "scroll"
            } else {
                kind_name(raw.kind)
            };
            let mut part_names = vec![format!("{owner}::{}", part.name())];
            for c in &classes {
                part_names.push(format!("{c}::{}", part.name()));
            }
            let refs: Vec<&str> = part_names.iter().map(String::as_str).collect();
            parts.push((part, self.style(&refs, None, None)?));
        }
        let hotkey = match &raw.hotkey {
            Some(h) => {
                Some(Hotkey::parse(h).ok_or_else(|| self.error(format!("bad hotkey {h:?}")))?)
            }
            None => None,
        };
        let children = raw
            .children
            .iter()
            .map(|c| self.node(c, None))
            .collect::<Result<_, _>>()?;
        Ok(Rc::new(NodeSpec {
            id: raw.id.clone(),
            kind: raw.kind,
            style,
            parts,
            text: raw.text.clone(),
            image: raw.image.as_deref().map(|i| self.images.intern(i)),
            value: raw.value.unwrap_or(0.0),
            min: raw.min.unwrap_or(0.0),
            max: raw.max.unwrap_or(1.0),
            step: raw.step.unwrap_or(0.0),
            options: raw.options.clone(),
            selected: raw.selected,
            placeholder: raw.placeholder.clone(),
            max_length: raw.max_length.unwrap_or(256),
            action: raw.action.as_deref().map(Arc::from),
            hotkey,
            tooltip: raw.tooltip.as_ref().map(|t| {
                Arc::new(Tooltip {
                    title: t.title.clone(),
                    hotkey: t.hotkey.clone(),
                    costs: Vec::new(),
                    body: t.body.clone(),
                    note: t.note.clone(),
                })
            }),
            tooltip_anchor: raw.tooltip_anchor.clone(),
            toggle: raw.toggle,
            drag: raw.drag,
            checked: raw.checked,
            visible: raw.visible.unwrap_or(true),
            enabled: raw.enabled.unwrap_or(true),
            repeat: raw.repeat.clone(),
            count: raw.count,
            children,
        }))
    }
}

// --- Building ------------------------------------------------------------------------

pub(crate) struct BuildCtx<'a> {
    pub templates: &'a HashMap<String, Rc<NodeSpec>>,
    pub snapshot: &'a HashMap<Arc<str>, Overrides>,
}

impl Ui {
    /// Compiles the config and builds every screen, keeping game-set values when
    /// `keep` (hot reload).
    pub(crate) fn rebuild(&mut self, keep: bool) -> Result<(), UiError> {
        let _span = tracing::info_span!("ui build").entered();
        let mut compiler = Compiler {
            config: &self.config,
            images: &mut self.images,
            cache: HashMap::new(),
            document: String::new(),
        };
        let mut templates = HashMap::new();
        let mut screens = Vec::new();
        let screen_base = RawStyle::from_json(&serde_json::json!({
            "position": "absolute", "left": 0, "top": 0, "width": "100%", "height": "100%"
        }))
        .expect("screen base style");
        for (name, doc) in &self.config.documents {
            compiler.document = format!("layouts/{name}.json");
            for (t, raw) in &doc.templates {
                if templates.contains_key(t) {
                    return Err(compiler.error(format!("template {t:?} defined twice")));
                }
                templates.insert(t.clone(), compiler.node(raw, None)?);
            }
            if let Some(root) = &doc.root {
                screens.push((
                    name.clone(),
                    doc.visible,
                    compiler.node(root, Some(&screen_base))?,
                ));
            }
        }
        compiler.document = "theme.json".into();
        let root_spec = compiler.node(
            &RawNode {
                class: if self.config.styles.contains_key("root") {
                    "root".into()
                } else {
                    String::new()
                },
                ..RawNode::default()
            },
            None,
        )?;
        let tooltip_spec = templates
            .get(&self.config.tooltip.template)
            .cloned()
            .ok_or_else(|| UiError::Layout {
                document: "theme.json".into(),
                message: format!(
                    "tooltip template {:?} not found",
                    self.config.tooltip.template
                ),
            })?;
        for spec in templates.values().chain(screens.iter().map(|(_, _, s)| s)) {
            check_repeats(spec, &templates)?;
        }

        // Snapshot what the game set, then rebuild.
        let mut snapshot = HashMap::new();
        if keep {
            for node in self.tree.nodes.iter().flatten() {
                if let Some(path) = &node.path {
                    snapshot.insert(path.clone(), node.set.clone());
                }
            }
        }
        let focus_path = self
            .input
            .focus
            .and_then(|f| self.tree.nodes.get(f).and_then(|n| n.as_ref()))
            .and_then(|n| n.path.clone());
        if let Some(root) = self.tree.root.take() {
            self.tree.remove(root);
        }
        self.input.reset();
        self.templates = templates;
        let ctx = BuildCtx {
            templates: &self.templates,
            snapshot: &snapshot,
        };
        let root = self
            .tree
            .build(&root_spec, None, None, None, &ctx, &mut self.text);
        self.tree.root = Some(root);
        self.tree.node_mut(root).pointer = PointerEvents::Auto;
        for (name, visible, spec) in &screens {
            let path: Arc<str> = Arc::from(name.as_str());
            let index = self.tree.build(
                spec,
                Some(root),
                Some(path.clone()),
                None,
                &ctx,
                &mut self.text,
            );
            if !snapshot.contains_key(&path) || snapshot[&path].visible.is_none() {
                self.tree.set_visible_raw(index, *visible);
            }
        }
        let tip = self.tree.build(
            &tooltip_spec,
            Some(root),
            Some(Arc::from("@tooltip")),
            None,
            &ctx,
            &mut self.text,
        );
        let mut sub = Vec::new();
        self.tree.subtree(tip, &mut sub);
        for s in sub {
            let node = self.tree.node_mut(s);
            if node.spec.style.z.is_none() {
                node.layer = i32::MAX;
            }
        }
        self.tree.set_visible_raw(tip, false);
        self.tree.tooltip_root = Some(tip);
        self.input.focus = focus_path
            .and_then(|p| self.tree.find(&p))
            .and_then(|id| self.tree.node_of(id));
        self.tree.layout_dirty = true;
        self.tree.viewport_changed = true;
        Ok(())
    }
}

fn check_repeats(
    spec: &NodeSpec,
    templates: &HashMap<String, Rc<NodeSpec>>,
) -> Result<(), UiError> {
    if let Some(t) = &spec.repeat
        && !templates.contains_key(t)
    {
        return Err(UiError::Layout {
            document: spec.id.clone().unwrap_or_default(),
            message: format!("unknown template {t:?}"),
        });
    }
    for c in &spec.children {
        check_repeats(c, templates)?;
    }
    Ok(())
}

impl Tree {
    /// Builds a node and its subtree under `parent`. `path` overrides the node's own
    /// path (screens and list entries).
    pub fn build(
        &mut self,
        spec: &Rc<NodeSpec>,
        parent: Option<usize>,
        path: Option<Arc<str>>,
        repeat_index: Option<usize>,
        ctx: &BuildCtx,
        text: &mut crate::text::TextSystem,
    ) -> usize {
        let parent_node = parent.map(|p| self.node(p));
        let prefix = parent_node.and_then(|p| self.path_prefix(p));
        let path = path.or_else(|| {
            spec.id.as_ref().map(|id| match &prefix {
                Some(p) => Arc::from(format!("{p}/{id}")),
                None => Arc::from(id.as_str()),
            })
        });
        let text_style = parent_node
            .map(|p| p.text_style.clone())
            .unwrap_or_default()
            .inherit(&spec.style.text);
        let pointer = spec
            .style
            .pointer
            .or(parent_node.map(|p| p.pointer))
            .unwrap_or(PointerEvents::Auto);
        let layer = spec.style.z.unwrap_or(parent_node.map_or(0, |p| p.layer));
        let parent_owner = parent_node.and_then(|p| p.owner);
        let set = path
            .as_ref()
            .and_then(|p| ctx.snapshot.get(p))
            .cloned()
            .unwrap_or_default();
        let taffy = self
            .taffy
            .new_leaf_with_context(spec.style.layout.clone(), 0)
            .expect("taffy node");
        let mut node = Node {
            spec: spec.clone(),
            parent,
            children: Vec::new(),
            taffy,
            path,
            handle: None,
            repeat_index,
            owner: parent_owner,
            visible: set.visible.unwrap_or(spec.visible),
            enabled: set.enabled.unwrap_or(spec.enabled),
            checked: set.checked.unwrap_or(spec.checked),
            value: set.value.unwrap_or(spec.value),
            image: set.image.unwrap_or(spec.image),
            color: set.color.unwrap_or(None),
            tooltip: set.tooltip.clone().unwrap_or_else(|| spec.tooltip.clone()),
            hotkey: set.hotkey.unwrap_or(spec.hotkey),
            options: set.options.clone().unwrap_or_else(|| spec.options.clone()),
            selected: set.selected.unwrap_or(spec.selected),
            count: set.count.unwrap_or(spec.count),
            scale_override: set.scale.unwrap_or(None),
            opacity: set.opacity.unwrap_or(1.0),
            shift: set.shift.unwrap_or(Vec2::ZERO),
            text: None,
            placeholder: spec
                .placeholder
                .as_deref()
                .map(|p| TextBlock::new(p, text_style.clone())),
            option_text: Vec::new(),
            text_style,
            pointer,
            layer,
            field: FieldState::default(),
            open: false,
            rect: Rect::ZERO,
            content: Rect::ZERO,
            clip: None,
            scale: 1.0,
            scroll: Vec2::ZERO,
            scroll_max: Vec2::ZERO,
            offset: Vec2::ZERO,
            text_scale: 1.0,
            set,
        };
        if spec.has_text() {
            let initial = node
                .set
                .text
                .clone()
                .or_else(|| spec.text.clone())
                .unwrap_or_default();
            let mut block = TextBlock::new(&initial, node.text_style.clone());
            block.plain = spec.kind == Kind::TextField;
            if spec.kind == Kind::TextField {
                node.field.cursor = initial.len();
                node.field.anchor = initial.len();
            }
            node.text = Some(block);
        }
        if spec.kind == Kind::Dropdown && node.text.is_some() {
            let label = node.options.get(node.selected).cloned().unwrap_or_default();
            if let Some(t) = &mut node.text {
                t.set_markup(&label);
            }
        }
        let _ = text;
        let index = self.insert(node);
        self.taffy
            .set_node_context(taffy, Some(index))
            .expect("taffy context");
        if spec.owner(parent_owner.is_some()) || self.node(index).tooltip.is_some() {
            self.node_mut(index).owner = Some(index);
        }
        if !self.node(index).visible {
            self.apply_display(index);
        }
        if let Some(p) = parent {
            let pt = self.node(p).taffy;
            self.taffy.add_child(pt, taffy).expect("taffy child");
            self.node_mut(p).children.push(index);
        }
        for child in &spec.children {
            self.build(child, Some(index), None, None, ctx, text);
        }
        if let Some(template) = &spec.repeat {
            let count = self.node(index).count;
            let template = ctx.templates[template].clone();
            for i in 0..count {
                let path = self.instance_path(index, i);
                self.build(&template, Some(index), path, Some(i), ctx, text);
            }
        }
        index
    }

    /// The path children extend: this node's, or the nearest ancestor's.
    fn path_prefix<'a>(&'a self, mut index: &'a Node) -> Option<Arc<str>> {
        loop {
            if let Some(p) = &index.path {
                return Some(p.clone());
            }
            index = self.node(index.parent?);
        }
    }

    fn instance_path(&self, list: usize, i: usize) -> Option<Arc<str>> {
        self.path_prefix(self.node(list))
            .map(|p| Arc::from(format!("{p}/{i}")))
    }

    /// Sets visibility, updating the layout style.
    pub fn set_visible_raw(&mut self, index: usize, visible: bool) {
        if self.node(index).visible != visible {
            self.node_mut(index).visible = visible;
            self.apply_display(index);
        }
    }

    fn apply_display(&mut self, index: usize) {
        let node = self.node(index);
        let mut style = node.spec.style.layout.clone();
        if !node.visible {
            style.display = Display::None;
        }
        let t = node.taffy;
        self.taffy.set_style(t, style).expect("taffy style");
        self.layout_dirty = true;
    }

    /// Sets the text the game (or typing) gave a node.
    pub fn set_text(&mut self, n: usize, text: &str) {
        let node = self.node_mut(n);
        if node.set.text.as_deref() != Some(text) {
            node.set.text = Some(text.to_owned());
        }
        let is_field = node.kind() == Kind::TextField;
        let changed = node.text.as_mut().is_some_and(|t| t.set_markup(text));
        if changed {
            if is_field {
                let f = &mut self.node_mut(n).field;
                f.cursor = f.cursor.min(text.len());
                f.anchor = f.anchor.min(text.len());
            }
            if self.sized_by_content(n) {
                self.mark_dirty(n);
            }
        }
    }

    /// Whether a node's box size depends on its text (so text changes need layout).
    pub fn sized_by_content(&self, index: usize) -> bool {
        let l = &self.node(index).spec.style.layout;
        let auto = |d: taffy::style::Dimension| d.is_auto();
        auto(l.size.width) || auto(l.size.height)
    }
}

// --- The game's setters --------------------------------------------------------------

impl Ui {
    /// The handle of a node by path: `screen/id/id`, with list entries as numbers
    /// (`hud/commands/3/icon`).
    pub fn find(&self, path: &str) -> Option<NodeId> {
        self.tree.find(path)
    }

    /// The `index`th entry of a list node.
    pub fn child(&self, list: NodeId, index: usize) -> Option<NodeId> {
        let n = self.tree.node_of(list)?;
        let path = self.tree.node(n).path.as_ref()?;
        self.tree.find(&format!("{path}/{index}"))
    }

    /// A node by path relative to another node.
    pub fn find_in(&self, base: NodeId, relative: &str) -> Option<NodeId> {
        let n = self.tree.node_of(base)?;
        let path = self.tree.node(n).path.as_ref()?;
        self.tree.find(&format!("{path}/{relative}"))
    }

    /// A node's box from the last frame, in physical pixels (`None` if hidden or gone).
    pub fn rect(&self, id: NodeId) -> Option<crate::Rect> {
        let n = self.tree.node_of(id)?;
        let node = self.tree.node(n);
        node.visible.then_some(node.rect)
    }

    /// A text dump of the tree with kinds, paths and boxes (for debugging layouts).
    pub fn dump(&self) -> String {
        let mut out = String::new();
        if let Some(root) = self.tree.root {
            self.dump_node(root, 0, &mut out);
        }
        out
    }

    fn dump_node(&self, n: usize, depth: usize, out: &mut String) {
        use std::fmt::Write;
        let node = self.tree.node(n);
        if !node.visible {
            return;
        }
        let r = node.rect;
        let _ = writeln!(
            out,
            "{:indent$}{:?} {} [{:.0} {:.0} {:.0}×{:.0}]{}",
            "",
            node.kind(),
            node.path.as_deref().unwrap_or("-"),
            r.x,
            r.y,
            r.w,
            r.h,
            node.text
                .as_ref()
                .map(|t| format!(" {:?} glyphs={}", t.markup(), t.glyphs.len()))
                .unwrap_or_default(),
            indent = depth * 2
        );
        for &c in &node.children {
            self.dump_node(c, depth + 1, out);
        }
    }

    pub fn path(&self, id: NodeId) -> Option<&str> {
        let n = self.tree.node_of(id)?;
        self.tree.node(n).path.as_deref()
    }

    fn with(&mut self, id: NodeId, f: impl FnOnce(&mut Tree, usize)) {
        if let Some(n) = self.tree.node_of(id) {
            f(&mut self.tree, n);
        }
    }

    /// Sets a node's text (markup for labels, plain for text fields).
    pub fn set_text(&mut self, id: NodeId, text: &str) {
        if let Some(n) = self.tree.node_of(id) {
            self.tree.set_text(n, text);
        }
    }

    /// The current text of a node (a text field's contents).
    pub fn text(&self, id: NodeId) -> Option<&str> {
        let n = self.tree.node_of(id)?;
        self.tree.node(n).text.as_ref().map(|t| t.markup())
    }

    pub fn set_visible(&mut self, id: NodeId, visible: bool) {
        self.with(id, |t, n| {
            t.node_mut(n).set.visible = Some(visible);
            t.set_visible_raw(n, visible);
        });
    }

    pub fn set_enabled(&mut self, id: NodeId, enabled: bool) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            node.set.enabled = Some(enabled);
            node.enabled = enabled;
        });
    }

    /// Pressed state of toggle buttons (`:checked`) and checkboxes.
    pub fn set_checked(&mut self, id: NodeId, checked: bool) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            node.set.checked = Some(checked);
            node.checked = checked;
        });
    }

    pub fn checked(&self, id: NodeId) -> bool {
        self.tree
            .node_of(id)
            .is_some_and(|n| self.tree.node(n).checked)
    }

    /// Bar fill and sweep fraction (0–1), or a slider's value.
    pub fn set_value(&mut self, id: NodeId, value: f32) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            node.set.value = Some(value);
            node.value = value;
        });
    }

    pub fn value(&self, id: NodeId) -> f32 {
        self.tree
            .node_of(id)
            .map_or(0.0, |n| self.tree.node(n).value)
    }

    /// The image of an image node (icons, portraits, minimap).
    pub fn set_image(&mut self, id: NodeId, image: Option<ImageKey>) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            if node.image != image {
                node.set.image = Some(image);
                node.image = image;
                if t.sized_by_content(n) {
                    t.mark_dirty(n);
                }
            }
        });
    }

    /// Replaces a styled background's texture, preserving its size and inset.
    /// `None` restores the theme background. Survives layout hot reloads.
    pub fn set_background_image(&mut self, id: NodeId, image: Option<ImageKey>) {
        self.with(id, |t, n| t.node_mut(n).set.background_image = image);
    }

    /// Overrides a node's fill colour (bar fills, sweeps) or background.
    pub fn set_color(&mut self, id: NodeId, color: Option<Color>) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            node.set.color = Some(color);
            node.color = color;
        });
    }

    pub fn set_tooltip(&mut self, id: NodeId, tooltip: Option<Tooltip>) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            let same = match (&node.tooltip, &tooltip) {
                (Some(a), Some(b)) => **a == *b,
                (None, None) => true,
                _ => false,
            };
            if same {
                return;
            }
            let tooltip = tooltip.map(Arc::new);
            node.set.tooltip = Some(tooltip.clone());
            node.tooltip = tooltip;
            if node.owner != Some(n) && node.tooltip.is_some() {
                // Now reacts to hovering: it owns its subtree's states.
                let mut sub = Vec::new();
                t.subtree(n, &mut sub);
                for s in sub {
                    if t.node(s).owner == t.node(n).owner || s == n {
                        t.node_mut(s).owner = Some(n);
                    }
                }
            }
        });
    }

    /// Rebinds a node's hotkey (`"Q"`, `"Shift+F1"`); `None` removes it.
    pub fn set_hotkey(&mut self, id: NodeId, hotkey: Option<&str>) {
        let parsed = hotkey.and_then(Hotkey::parse);
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            node.set.hotkey = Some(parsed);
            node.hotkey = parsed;
        });
    }

    pub fn set_options(&mut self, id: NodeId, options: &[String]) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            if node.options != options {
                node.set.options = Some(options.to_vec());
                node.options = options.to_vec();
                let label = node.options.get(node.selected).cloned().unwrap_or_default();
                if let Some(text) = &mut node.text
                    && text.set_markup(&label)
                {
                    t.mark_dirty(n);
                }
            }
        });
    }

    pub fn set_selected(&mut self, id: NodeId, selected: usize) {
        self.with(id, |t, n| select(t, n, selected));
    }

    pub fn selected(&self, id: NodeId) -> usize {
        self.tree
            .node_of(id)
            .map_or(0, |n| self.tree.node(n).selected)
    }

    /// A display scale for a subtree (the HUD scale setting), over its style's `transform`.
    pub fn set_scale(&mut self, id: NodeId, scale: Option<f32>) {
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            if node.scale_override != scale {
                node.set.scale = Some(scale);
                node.scale_override = scale;
                t.layout_dirty = true;
                t.viewport_changed = true;
            }
        });
    }

    /// Opacity multiplied into a subtree (0–1), for fades. Paint only.
    pub fn set_opacity(&mut self, id: NodeId, opacity: f32) {
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            node.set.opacity = Some(opacity);
            node.opacity = opacity;
        });
    }

    /// Moves a subtree by `(dx, dy)` logical pixels from where layout put it (animations).
    /// Re-places boxes without re-running flex layout.
    pub fn set_shift(&mut self, id: NodeId, dx: f32, dy: f32) {
        let shift = Vec2::new(dx, dy);
        self.with(id, |t, n| {
            let node = t.node_mut(n);
            if node.shift != shift {
                node.set.shift = Some(shift);
                node.shift = shift;
                t.layout_dirty = true;
            }
        });
    }

    /// Scrolls a box with `overflow: scroll` back to its start (new content in a reused
    /// box).
    pub fn scroll_to_top(&mut self, id: NodeId) {
        if let Some(n) = self.tree.node_of(id) {
            self.tree.node_mut(n).scroll = Vec2::ZERO;
        }
    }

    /// Scrolls a box with `overflow: scroll` to its end as last laid out (a list whose
    /// newest entry is at the bottom).
    pub fn scroll_to_end(&mut self, id: NodeId) {
        if let Some(n) = self.tree.node_of(id) {
            let node = self.tree.node_mut(n);
            node.scroll.y = node.scroll_max.y;
        }
    }

    /// Resizes a list (a node with `repeat`): entries are built from its template or
    /// removed from the end. Entries keep their handles while they exist.
    pub fn set_count(&mut self, id: NodeId, count: usize) {
        let Some(n) = self.tree.node_of(id) else {
            return;
        };
        let Some(template) = self.tree.node(n).spec.repeat.clone() else {
            return;
        };
        let current = self.tree.node(n).count;
        self.tree.node_mut(n).set.count = Some(count);
        if current == count {
            return;
        }
        self.tree.node_mut(n).count = count;
        if count < current {
            let removed: Vec<usize> = self.tree.node(n).children[..]
                .iter()
                .copied()
                .filter(|&c| self.tree.node(c).repeat_index.is_some_and(|i| i >= count))
                .collect();
            for c in removed {
                let t = self.tree.node(n).taffy;
                let ct = self.tree.node(c).taffy;
                let _ = self.tree.taffy.remove_child(t, ct);
                self.tree.node_mut(n).children.retain(|&x| x != c);
                if self.input.involves(&self.tree, c) {
                    self.input.reset();
                }
                self.tree.remove(c);
            }
        } else {
            let template = self.templates[&template].clone();
            let snapshot = HashMap::new();
            let ctx = BuildCtx {
                templates: &self.templates,
                snapshot: &snapshot,
            };
            for i in current..count {
                let path = self.tree.instance_path(n, i);
                self.tree
                    .build(&template, Some(n), path, Some(i), &ctx, &mut self.text);
            }
        }
        self.tree.mark_dirty(n);
        self.tree.viewport_changed = true;
    }

    pub fn count(&self, id: NodeId) -> usize {
        self.tree.node_of(id).map_or(0, |n| self.tree.node(n).count)
    }
}

pub(crate) fn select(t: &mut Tree, n: usize, selected: usize) {
    let node = t.node_mut(n);
    node.set.selected = Some(selected);
    if node.selected == selected {
        return;
    }
    node.selected = selected;
    let label = node.options.get(selected).cloned().unwrap_or_default();
    if let Some(text) = &mut node.text
        && text.set_markup(&label)
        && t.sized_by_content(n)
    {
        t.mark_dirty(n);
    }
}
