//! Styles: CSS-like declarations from the theme and layout files, compiled once.
//!
//! A style is a map of CSS property names to values (`"padding": "12px 14px"`,
//! `"border": "1px solid #746c47"`), with optional state blocks (`":hover"`,
//! `":active"`, `":disabled"`, `":focus"`, `":checked"`). Layout properties become a
//! [`taffy::Style`]; the rest is [`Visual`] (box painting), [`TextDecl`] (inherited text
//! properties) and a few behaviour flags. Only visual and text-colour properties may
//! change with state, so hovering never re-runs layout.
//!
//! Values may name palette entries from the theme as `$name`.

use crate::geom::Color;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use taffy::prelude::*;
use taffy::style::{
    AlignContent, AlignItems, BoxSizing, Dimension, Display, FlexDirection, FlexWrap, GridAutoFlow,
    GridPlacement, GridTemplateComponent, LengthPercentage, LengthPercentageAuto, Overflow,
    Position,
};

/// A style problem in a theme or layout file.
#[derive(Debug, thiserror::Error)]
#[error("{property}: {message}")]
pub struct StyleError {
    pub property: String,
    pub message: String,
}

fn err(property: &str, message: impl Into<String>) -> StyleError {
    StyleError {
        property: property.to_owned(),
        message: message.into(),
    }
}

// --- Raw declarations --------------------------------------------------------------

/// Uncompiled declarations, merged class by class before compiling.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawStyle {
    pub props: BTreeMap<String, String>,
    pub states: BTreeMap<String, BTreeMap<String, String>>,
}

impl RawStyle {
    pub fn from_json(value: &Value) -> Result<Self, StyleError> {
        let mut raw = RawStyle::default();
        let Value::Object(map) = value else {
            return Err(err("style", "expected an object of CSS properties"));
        };
        for (key, value) in map {
            if let Some(state) = key.strip_prefix(':') {
                let Value::Object(inner) = value else {
                    return Err(err(key, "a state block must be an object"));
                };
                let block = raw.states.entry(state.to_owned()).or_default();
                for (k, v) in inner {
                    block.insert(k.clone(), value_string(k, v)?);
                }
            } else {
                raw.props.insert(key.clone(), value_string(key, value)?);
            }
        }
        Ok(raw)
    }

    /// Later declarations win, as in CSS.
    pub fn merge(&mut self, other: &RawStyle) {
        for (k, v) in &other.props {
            self.props.insert(k.clone(), v.clone());
        }
        for (state, block) in &other.states {
            let mine = self.states.entry(state.clone()).or_default();
            for (k, v) in block {
                mine.insert(k.clone(), v.clone());
            }
        }
    }
}

fn value_string(key: &str, value: &Value) -> Result<String, StyleError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(err(key, "expected a string or number")),
    }
}

/// Replaces `$name` palette references.
pub fn substitute(value: &str, palette: &BTreeMap<String, String>) -> Result<String, String> {
    if !value.contains('$') {
        return Ok(value.to_owned());
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while let Some(&(j, c)) = chars.peek() {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                end = j + c.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let name = &value[start..end];
        match palette.get(name) {
            Some(v) => out.push_str(v),
            None => return Err(format!("unknown palette entry ${name}")),
        }
    }
    Ok(out)
}

// --- Image names -------------------------------------------------------------------

/// An interned image name. Styles and nodes refer to images by key; the game registers
/// the texture behind a name whenever it likes (before or after the style is compiled).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageKey(pub u32);

/// Interns image names. Keys survive hot reloads.
#[derive(Debug, Default)]
pub struct ImageNames {
    names: Vec<String>,
    map: HashMap<String, ImageKey>,
}

impl ImageNames {
    pub fn intern(&mut self, name: &str) -> ImageKey {
        if let Some(k) = self.map.get(name) {
            return *k;
        }
        let key = ImageKey(self.names.len() as u32);
        self.names.push(name.to_owned());
        self.map.insert(name.to_owned(), key);
        key
    }

    pub fn get(&self, name: &str) -> Option<ImageKey> {
        self.map.get(name).copied()
    }

    pub fn name(&self, key: ImageKey) -> &str {
        &self.names[key.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

// --- Compiled style ----------------------------------------------------------------

/// A length that may be relative to the box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Len {
    Px(f32),
    Percent(f32),
}

impl Len {
    pub fn resolve(self, basis: f32) -> f32 {
        match self {
            Len::Px(v) => v,
            Len::Percent(p) => p * basis,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    Solid(Color),
    /// CSS angle: 180° is top to bottom.
    Linear {
        angle: f32,
        from: Color,
        to: Color,
    },
    Radial {
        from: Color,
        to: Color,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub inset: bool,
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSize {
    Stretch,
    Contain,
    Cover,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageFill {
    /// `None` until a `background-image` is given (a size alone may come first).
    pub image: Option<ImageKey>,
    pub size: ImageSize,
    /// Insets `[top, right, bottom, left]` in px (negative grows), like `inset` on a pseudo element.
    pub inset: [Len; 4],
}

/// CSS `border-image`: nine-slice `image` cut at `slice` source pixels, drawn with
/// `width` destination pixels, grown by `outset`.
#[derive(Debug, Clone, PartialEq)]
pub struct BorderImage {
    pub image: ImageKey,
    pub slice: [f32; 4],
    pub width: [f32; 4],
    pub outset: [f32; 4],
    pub fill: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub width: f32,
    pub color: Color,
    pub offset: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    Normal,
    /// The fill image's luminance tinted by the fill colour (`background-blend-mode: luminosity`).
    Luminosity,
}

/// How a box paints. Every field is optional so state blocks can override single fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Visual {
    pub background: Option<Fill>,
    pub background_image: Option<ImageFill>,
    pub border_image: Option<BorderImage>,
    pub border_color: Option<Color>,
    pub border_width: Option<f32>,
    pub radius: Option<Len>,
    pub shadows: Option<Vec<Shadow>>,
    pub outline: Option<Outline>,
    pub opacity: Option<f32>,
    pub brightness: Option<f32>,
    pub grayscale: Option<bool>,
    pub image_tint: Option<Color>,
    pub image_radius: Option<Len>,
    /// Mirror image pixels horizontally without changing the node or its children.
    pub image_flip_x: Option<bool>,
    /// Bar and sweep fill.
    pub fill: Option<Fill>,
    pub fill_image: Option<ImageKey>,
    pub fill_inset: Option<[f32; 4]>,
    pub fill_radius: Option<f32>,
    pub fill_mode: Option<FillMode>,
    /// Sweep: colour of the part not yet swept.
    pub sweep_rest: Option<Color>,
    /// Sweep: ring thickness (0 = a filled pie).
    pub sweep_ring: Option<f32>,
}

impl Visual {
    fn overlay(&mut self, o: &Visual) {
        macro_rules! take {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        take!(
            background,
            background_image,
            border_image,
            border_color,
            border_width,
            radius,
            shadows,
            outline,
            opacity,
            brightness,
            grayscale,
            image_tint,
            image_radius,
            image_flip_x,
            fill,
            fill_image,
            fill_inset,
            fill_radius,
            fill_mode,
            sweep_rest,
            sweep_ring
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    /// Synthesised small capitals (Georgia has no `smcp` feature either; browsers fake it too).
    SmallCaps,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextShadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    Mult(f32),
    Px(f32),
}

/// Inherited text properties; `None` inherits from the parent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextDecl {
    pub family: Option<String>,
    pub size: Option<f32>,
    pub weight: Option<u16>,
    pub italic: Option<bool>,
    pub line_height: Option<LineHeight>,
    pub color: Option<Color>,
    /// In em.
    pub letter_spacing: Option<f32>,
    pub shadow: Option<Option<TextShadow>>,
    pub align: Option<TextAlign>,
    pub valign: Option<VAlign>,
    pub transform: Option<TextTransform>,
    pub nowrap: Option<bool>,
    pub ellipsis: Option<bool>,
    pub max_lines: Option<usize>,
    pub tabular: Option<bool>,
}

/// Fully resolved text style of a node.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    pub line_height: LineHeight,
    pub color: Color,
    pub letter_spacing: f32,
    pub shadow: Option<TextShadow>,
    pub align: TextAlign,
    pub valign: VAlign,
    pub transform: TextTransform,
    pub nowrap: bool,
    pub ellipsis: bool,
    pub max_lines: usize,
    pub tabular: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "ui".into(),
            size: 13.0,
            weight: 400,
            italic: false,
            line_height: LineHeight::Mult(1.25),
            color: Color::WHITE,
            letter_spacing: 0.0,
            shadow: None,
            align: TextAlign::Left,
            valign: VAlign::Top,
            transform: TextTransform::None,
            nowrap: false,
            ellipsis: false,
            max_lines: 0,
            tabular: false,
        }
    }
}

impl TextStyle {
    /// Applies a node's declarations over the inherited style.
    pub fn inherit(&self, decl: &TextDecl) -> TextStyle {
        let mut s = self.clone();
        macro_rules! take {
            ($($f:ident),*) => { $( if let Some(v) = &decl.$f { s.$f = v.clone(); } )* };
        }
        take!(
            family,
            size,
            weight,
            italic,
            line_height,
            color,
            letter_spacing,
            shadow,
            align,
            valign,
            transform,
            nowrap,
            ellipsis,
            max_lines,
            tabular
        );
        s
    }

    pub fn line_height_px(&self) -> f32 {
        match self.line_height {
            LineHeight::Mult(m) => m * self.size,
            LineHeight::Px(px) => px,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEvents {
    Auto,
    None,
}

/// Interaction states a style can react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State(pub u8);

impl State {
    pub const HOVER: u8 = 1;
    pub const ACTIVE: u8 = 2;
    pub const FOCUS: u8 = 4;
    pub const CHECKED: u8 = 8;
    pub const DISABLED: u8 = 16;

    pub fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    fn parse(name: &str) -> Option<u8> {
        Some(match name {
            "hover" => Self::HOVER,
            "active" => Self::ACTIVE,
            "focus" | "focus-visible" => Self::FOCUS,
            "checked" | "pressed" => Self::CHECKED,
            "disabled" => Self::DISABLED,
            _ => return None,
        })
    }
}

/// A compiled style.
#[derive(Debug, Clone, Default)]
pub struct Style {
    pub layout: taffy::Style,
    pub visual: Visual,
    pub text: TextDecl,
    /// State overrides in increasing precedence: checked, hover, active, focus, disabled.
    pub states: Vec<(u8, Visual, TextDecl)>,
    pub pointer: Option<PointerEvents>,
    pub z: Option<i32>,
    /// `transform: scale(s)` about `transform-origin` (fractions of the box).
    pub scale: Option<(f32, [f32; 2])>,
    /// Clips children (`overflow: hidden | scroll`).
    pub clip: bool,
    pub scroll: bool,
    /// Explicit pixel `width` / `height`, for widget parts drawn without layout.
    pub width_px: Option<f32>,
    pub height_px: Option<f32>,
    /// Gap between a widget's own parts (checkbox box and label, dropdown arrow).
    pub gap_px: f32,
}

impl Style {
    pub fn compile(
        raw: &RawStyle,
        palette: &BTreeMap<String, String>,
        images: &mut ImageNames,
    ) -> Result<Style, StyleError> {
        let mut style = Style::default();
        // Every box is a containing block for its absolute children (as in React
        // Native); CSS's `static` default would place them against a farther ancestor.
        style.layout.position = Position::Relative;
        // Padding etc. may be given after the shorthand; BTreeMap order is alphabetical,
        // so apply shorthands first.
        let mut props: Vec<(&String, &String)> = raw.props.iter().collect();
        props.sort_by_key(|(k, _)| !is_shorthand(k));
        for (key, value) in props {
            let value = substitute(value, palette).map_err(|m| err(key, m))?;
            apply(&mut style, key, value.trim(), images).map_err(|m| err(key, m))?;
        }
        let order = [
            State::CHECKED,
            State::HOVER,
            State::ACTIVE,
            State::FOCUS,
            State::DISABLED,
        ];
        let mut states = Vec::new();
        for (name, block) in &raw.states {
            let bit = State::parse(name).ok_or_else(|| err(name, "unknown state"))?;
            let mut s = Style::default();
            for (key, value) in block {
                let value = substitute(value, palette).map_err(|m| err(key, m))?;
                if !state_property(key) {
                    return Err(err(key, format!("cannot change with :{name}")));
                }
                apply(&mut s, key, value.trim(), images).map_err(|m| err(key, m))?;
            }
            states.push((bit, s.visual, s.text));
        }
        states.sort_by_key(|(bit, _, _)| order.iter().position(|b| b == bit));
        style.states = states;
        Ok(style)
    }

    /// The visual for a state: base overlaid by every matching state block.
    pub fn visual_for(&self, state: State) -> std::borrow::Cow<'_, Visual> {
        let mut matching = self
            .states
            .iter()
            .filter(|(b, _, _)| state.has(*b))
            .peekable();
        if matching.peek().is_none() {
            return std::borrow::Cow::Borrowed(&self.visual);
        }
        let mut v = self.visual.clone();
        for (_, o, _) in matching {
            v.overlay(o);
        }
        std::borrow::Cow::Owned(v)
    }

    /// Text colour override for a state, if a matching state block sets one.
    pub fn text_color_for(&self, state: State) -> Option<Color> {
        self.states
            .iter()
            .filter(|(b, _, _)| state.has(*b))
            .filter_map(|(_, _, t)| t.color)
            .next_back()
    }

    pub fn has_states(&self) -> bool {
        !self.states.is_empty()
    }
}

fn is_shorthand(key: &str) -> bool {
    matches!(
        key,
        "padding" | "margin" | "inset" | "gap" | "flex" | "border" | "font" | "overflow"
    )
}

fn state_property(key: &str) -> bool {
    matches!(
        key,
        "background"
            | "background-color"
            | "background-image"
            | "border-color"
            | "border-image"
            | "border-radius"
            | "box-shadow"
            | "outline"
            | "outline-offset"
            | "opacity"
            | "filter"
            | "color"
            | "image-tint"
            | "image-flip-x"
            | "fill"
            | "fill-image"
            | "sweep-rest"
    )
}

// --- Property application ------------------------------------------------------------

fn apply(style: &mut Style, key: &str, v: &str, images: &mut ImageNames) -> Result<(), String> {
    let l = &mut style.layout;
    match key {
        // Layout.
        "display" => {
            l.display = match v {
                "none" => Display::None,
                "flex" => Display::Flex,
                "grid" => Display::Grid,
                "block" => Display::Block,
                _ => return Err(format!("unsupported display {v}")),
            }
        }
        "position" => l.position = parse::<Position>(v)?,
        "box-sizing" => l.box_sizing = parse::<BoxSizing>(v)?,
        "left" => l.inset.left = lpa(v)?,
        "right" => l.inset.right = lpa(v)?,
        "top" => l.inset.top = lpa(v)?,
        "bottom" => l.inset.bottom = lpa(v)?,
        "inset" => l.inset = edges(v, lpa)?,
        "width" => {
            l.size.width = dim(v)?;
            style.width_px = px_opt(v);
        }
        "height" => {
            l.size.height = dim(v)?;
            style.height_px = px_opt(v);
        }
        "min-width" => l.min_size.width = lpa(v)?,
        "min-height" => l.min_size.height = lpa(v)?,
        "max-width" => l.max_size.width = lpa(v)?,
        "max-height" => l.max_size.height = lpa(v)?,
        "aspect-ratio" => l.aspect_ratio = Some(aspect(v)?),
        "margin" => l.margin = edges(v, lpa)?,
        "margin-left" => l.margin.left = lpa(v)?,
        "margin-right" => l.margin.right = lpa(v)?,
        "margin-top" => l.margin.top = lpa(v)?,
        "margin-bottom" => l.margin.bottom = lpa(v)?,
        "padding" => l.padding = edges(v, lp)?,
        "padding-left" => l.padding.left = lp(v)?,
        "padding-right" => l.padding.right = lp(v)?,
        "padding-top" => l.padding.top = lp(v)?,
        "padding-bottom" => l.padding.bottom = lp(v)?,
        "flex-direction" => l.flex_direction = parse::<FlexDirection>(v)?,
        "flex-wrap" => l.flex_wrap = parse::<FlexWrap>(v)?,
        "flex-grow" => l.flex_grow = num(v)?,
        "flex-shrink" => l.flex_shrink = num(v)?,
        "flex-basis" => l.flex_basis = dim(v)?,
        "flex" => {
            let parts: Vec<&str> = v.split_whitespace().collect();
            match parts.as_slice() {
                ["none"] => {
                    l.flex_grow = 0.0;
                    l.flex_shrink = 0.0;
                    l.flex_basis = Dimension::auto();
                }
                [g] => {
                    l.flex_grow = num(g)?;
                    l.flex_shrink = 1.0;
                    l.flex_basis = Dimension::length(0.0);
                }
                [g, s] => {
                    l.flex_grow = num(g)?;
                    l.flex_shrink = num(s)?;
                    l.flex_basis = Dimension::length(0.0);
                }
                [g, s, b] => {
                    l.flex_grow = num(g)?;
                    l.flex_shrink = num(s)?;
                    l.flex_basis = dim(b)?;
                }
                _ => return Err("expected `grow [shrink [basis]]`".into()),
            }
        }
        "justify-content" => l.justify_content = parse::<AlignContent>(v)?,
        "align-content" => l.align_content = parse::<AlignContent>(v)?,
        "align-items" => l.align_items = parse::<AlignItems>(v)?,
        "align-self" => l.align_self = Some(parse::<AlignItems>(v)?),
        "justify-items" => l.justify_items = parse::<AlignItems>(v)?,
        "justify-self" => l.justify_self = Some(parse::<AlignItems>(v)?),
        "gap" => {
            let parts: Vec<&str> = v.split_whitespace().collect();
            let (r, c) = match parts.as_slice() {
                [a] => (lp(a)?, lp(a)?),
                [a, b] => (lp(a)?, lp(b)?),
                _ => return Err("expected one or two lengths".into()),
            };
            l.gap = Size {
                width: c,
                height: r,
            };
            style.gap_px = px_opt(parts[0]).unwrap_or(0.0);
        }
        "row-gap" => l.gap.height = lp(v)?,
        "column-gap" => l.gap.width = lp(v)?,
        "grid-template-columns" => l.grid_template_columns = tracks(v)?,
        "grid-template-rows" => l.grid_template_rows = tracks(v)?,
        "grid-auto-flow" => l.grid_auto_flow = parse::<GridAutoFlow>(v)?,
        "grid-column" => l.grid_column = grid_line(v)?,
        "grid-row" => l.grid_row = grid_line(v)?,
        "overflow" => {
            let o = match v {
                "visible" => Overflow::Visible,
                "hidden" | "clip" => Overflow::Hidden,
                "scroll" | "auto" => Overflow::Scroll,
                _ => return Err(format!("unsupported overflow {v}")),
            };
            l.overflow = taffy::geometry::Point { x: o, y: o };
            l.scrollbar_width = 0.0;
            style.clip = o != Overflow::Visible;
            style.scroll = o == Overflow::Scroll;
        }
        "border-width" => {
            let w = edges(v, lp)?;
            l.border = w;
            style.visual.border_width = px_opt(v.split_whitespace().next().unwrap_or("0"));
        }
        "border" => {
            if v == "none" || v == "0" {
                l.border = Rect::length(0.0);
                style.visual.border_width = Some(0.0);
                style.visual.border_color = None;
            } else {
                let (w, c) = border(v)?;
                l.border = Rect::length(w);
                style.visual.border_width = Some(w);
                style.visual.border_color = Some(c);
            }
        }

        // Visual.
        "background" | "background-color" => {
            style.visual.background = if v == "none" || v == "transparent" {
                Some(Fill::Solid(Color::TRANSPARENT))
            } else {
                Some(fill(v)?)
            }
        }
        "background-image" => {
            let size = style
                .visual
                .background_image
                .as_ref()
                .map_or(ImageSize::Stretch, |b| b.size);
            let inset = style
                .visual
                .background_image
                .as_ref()
                .map_or([Len::Px(0.0); 4], |b| b.inset);
            style.visual.background_image = (v != "none").then(|| ImageFill {
                image: Some(images.intern(v)),
                size,
                inset,
            });
        }
        "background-size" | "object-fit" => {
            let size = match v {
                "100% 100%" | "fill" | "stretch" => ImageSize::Stretch,
                "contain" => ImageSize::Contain,
                "cover" => ImageSize::Cover,
                _ => return Err(format!("unsupported size {v}")),
            };
            match &mut style.visual.background_image {
                Some(b) => b.size = size,
                None => {
                    style.visual.background_image = Some(ImageFill {
                        image: None,
                        size,
                        inset: [Len::Px(0.0); 4],
                    })
                }
            }
        }
        "background-inset" => {
            let inset = edges4(v, len)?;
            match &mut style.visual.background_image {
                Some(b) => b.inset = inset,
                None => {
                    style.visual.background_image = Some(ImageFill {
                        image: None,
                        size: ImageSize::Stretch,
                        inset,
                    })
                }
            }
        }
        "border-color" => style.visual.border_color = Some(color(v)?),
        "border-radius" => style.visual.radius = Some(len(v)?),
        "border-image" => style.visual.border_image = Some(border_image(v, images)?),
        "box-shadow" => {
            style.visual.shadows = Some(if v == "none" {
                Vec::new()
            } else {
                split_top(v, ',')
                    .into_iter()
                    .map(shadow)
                    .collect::<Result<_, _>>()?
            })
        }
        "outline" => {
            if v == "none" || v == "0" {
                style.visual.outline = Some(Outline {
                    width: 0.0,
                    color: Color::TRANSPARENT,
                    offset: 0.0,
                });
            } else {
                let (w, c) = border(v)?;
                let offset = style.visual.outline.map_or(0.0, |o| o.offset);
                style.visual.outline = Some(Outline {
                    width: w,
                    color: c,
                    offset,
                });
            }
        }
        "outline-offset" => {
            let o = style.visual.outline.get_or_insert(Outline {
                width: 0.0,
                color: Color::TRANSPARENT,
                offset: 0.0,
            });
            o.offset = px(v)?;
        }
        "opacity" => style.visual.opacity = Some(num(v)?),
        "filter" => {
            style.visual.brightness = Some(1.0);
            style.visual.grayscale = Some(false);
            if v != "none" {
                for f in split_ws_top(v) {
                    let (name, arg) = function(f).ok_or_else(|| format!("bad filter {f}"))?;
                    match name {
                        "brightness" => style.visual.brightness = Some(num(arg)?),
                        "grayscale" => style.visual.grayscale = Some(num(arg)? > 0.5),
                        // Drop shadows on artwork are baked into the published images.
                        "drop-shadow" => {}
                        _ => return Err(format!("unsupported filter {name}")),
                    }
                }
            }
        }
        "image-tint" => style.visual.image_tint = Some(color(v)?),
        "image-radius" => style.visual.image_radius = Some(len(v)?),
        "image-flip-x" => style.visual.image_flip_x = Some(parse(v)?),
        "fill" => style.visual.fill = Some(fill(v)?),
        "fill-image" => style.visual.fill_image = Some(images.intern(v)),
        "fill-inset" => style.visual.fill_inset = Some(edges4(v, px)?),
        "fill-radius" => style.visual.fill_radius = Some(px(v)?),
        "fill-mode" => {
            style.visual.fill_mode = Some(match v {
                "normal" => FillMode::Normal,
                "luminosity" => FillMode::Luminosity,
                _ => return Err(format!("unsupported fill-mode {v}")),
            })
        }
        "sweep-rest" => style.visual.sweep_rest = Some(color(v)?),
        "sweep-ring" => style.visual.sweep_ring = Some(px(v)?),

        // Text (inherited).
        "font" => font(&mut style.text, v)?,
        "font-family" => style.text.family = Some(v.to_owned()),
        "font-size" => style.text.size = Some(px(v)?),
        "font-weight" => style.text.weight = Some(weight(v)?),
        "font-style" => style.text.italic = Some(v == "italic"),
        "line-height" => style.text.line_height = Some(line_height(v)?),
        "color" => style.text.color = Some(color(v)?),
        "letter-spacing" => {
            style.text.letter_spacing = Some(if let Some(em) = v.strip_suffix("em") {
                num(em)?
            } else if v == "normal" {
                0.0
            } else {
                // Pixel spacing is stored relative to the size when the size is known.
                let size = style.text.size.unwrap_or(13.0);
                px(v)? / size
            })
        }
        "text-shadow" => {
            style.text.shadow = Some(if v == "none" {
                None
            } else {
                let first = split_top(v, ',')[0];
                let s = shadow(first)?;
                Some(TextShadow {
                    x: s.x,
                    y: s.y,
                    blur: s.blur,
                    color: s.color,
                })
            })
        }
        "text-align" => {
            style.text.align = Some(match v {
                "left" | "start" => TextAlign::Left,
                "center" => TextAlign::Center,
                "right" | "end" => TextAlign::Right,
                _ => return Err(format!("unsupported text-align {v}")),
            })
        }
        "vertical-align" => {
            style.text.valign = Some(match v {
                "top" => VAlign::Top,
                "middle" => VAlign::Middle,
                "bottom" => VAlign::Bottom,
                _ => return Err(format!("unsupported vertical-align {v}")),
            })
        }
        "text-transform" => {
            style.text.transform = Some(match v {
                "none" => TextTransform::None,
                "uppercase" => TextTransform::Uppercase,
                _ => return Err(format!("unsupported text-transform {v}")),
            })
        }
        "font-variant" => {
            style.text.transform = Some(match v {
                "normal" => TextTransform::None,
                "small-caps" => TextTransform::SmallCaps,
                _ => return Err(format!("unsupported font-variant {v}")),
            })
        }
        "font-variant-numeric" => style.text.tabular = Some(v == "tabular-nums"),
        "white-space" => style.text.nowrap = Some(v == "nowrap"),
        "text-overflow" => style.text.ellipsis = Some(v == "ellipsis"),
        "line-clamp" => style.text.max_lines = Some(num(v)? as usize),

        // Behaviour.
        "pointer-events" => {
            style.pointer = Some(match v {
                "auto" => PointerEvents::Auto,
                "none" => PointerEvents::None,
                _ => return Err(format!("unsupported pointer-events {v}")),
            })
        }
        "z-index" => style.z = Some(num(v)? as i32),
        "transform" => {
            let (name, arg) = function(v).ok_or("expected scale(…)")?;
            if name != "scale" {
                return Err(format!("unsupported transform {name}"));
            }
            let origin = style.scale.map_or([0.5, 0.5], |s| s.1);
            style.scale = Some((num(arg)?, origin));
        }
        "transform-origin" => {
            let mut origin = [0.5, 0.5];
            for word in v.split_whitespace() {
                match word {
                    "left" => origin[0] = 0.0,
                    "right" => origin[0] = 1.0,
                    "top" => origin[1] = 0.0,
                    "bottom" => origin[1] = 1.0,
                    "center" => {}
                    _ => return Err(format!("unsupported origin {word}")),
                }
            }
            let s = style.scale.map_or(1.0, |s| s.0);
            style.scale = Some((s, origin));
        }
        _ => return Err("unknown property".into()),
    }
    Ok(())
}

// --- Value parsers -------------------------------------------------------------------

fn parse<T: FromStr>(v: &str) -> Result<T, String> {
    T::from_str(v).map_err(|_| format!("invalid value {v:?}"))
}

/// Bare numbers are pixels.
fn css_len(v: &str) -> std::borrow::Cow<'_, str> {
    if v.parse::<f32>().is_ok() {
        format!("{v}px").into()
    } else {
        v.into()
    }
}

fn dim(v: &str) -> Result<Dimension, String> {
    parse(&css_len(v))
}

fn lp(v: &str) -> Result<LengthPercentage, String> {
    parse(&css_len(v))
}

fn lpa(v: &str) -> Result<LengthPercentageAuto, String> {
    parse(&css_len(v))
}

pub(crate) fn num(v: &str) -> Result<f32, String> {
    if let Some(p) = v.strip_suffix('%') {
        return p
            .trim()
            .parse::<f32>()
            .map(|n| n / 100.0)
            .map_err(|_| format!("invalid number {v:?}"));
    }
    v.trim()
        .parse::<f32>()
        .map_err(|_| format!("invalid number {v:?}"))
}

pub(crate) fn px(v: &str) -> Result<f32, String> {
    num(v.trim().strip_suffix("px").unwrap_or(v))
}

fn px_opt(v: &str) -> Option<f32> {
    let v = v.trim();
    if v.ends_with('%') || v == "auto" {
        return None;
    }
    px(v).ok()
}

pub(crate) fn len(v: &str) -> Result<Len, String> {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        return Ok(Len::Percent(num(p)? / 100.0));
    }
    Ok(Len::Px(px(v)?))
}

fn aspect(v: &str) -> Result<f32, String> {
    match v.split_once('/') {
        Some((a, b)) => Ok(num(a)? / num(b)?),
        None => num(v),
    }
}

fn edges<T: Copy>(v: &str, f: impl Fn(&str) -> Result<T, String>) -> Result<Rect<T>, String> {
    let [top, right, bottom, left] = edges4(v, f)?;
    Ok(Rect {
        left,
        right,
        top,
        bottom,
    })
}

/// CSS edge shorthand → `[top, right, bottom, left]`.
fn edges4<T: Copy>(v: &str, f: impl Fn(&str) -> Result<T, String>) -> Result<[T; 4], String> {
    let parts: Vec<T> = v.split_whitespace().map(&f).collect::<Result<_, _>>()?;
    Ok(match parts.as_slice() {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return Err("expected one to four values".into()),
    })
}

fn tracks(v: &str) -> Result<Vec<GridTemplateComponent<String>>, String> {
    // CSS allows a unitless zero inside `minmax()`; the parser wants `0px`.
    let v = v
        .replace("(0,", "(0px,")
        .replace("( 0,", "(0px,")
        .replace(", 0)", ", 0px)");
    split_ws_top(&v).into_iter().map(parse).collect()
}

fn grid_line(v: &str) -> Result<taffy::geometry::Line<GridPlacement<String>>, String> {
    let (start, end) = match v.split_once('/') {
        Some((a, b)) => (parse(a.trim())?, parse(b.trim())?),
        None => (parse(v)?, GridPlacement::Auto),
    };
    Ok(taffy::geometry::Line { start, end })
}

/// Splits at `sep` outside parentheses.
pub(crate) fn split_top(v: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in v.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(v[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(v[start..].trim());
    out
}

/// Splits at whitespace outside parentheses.
pub(crate) fn split_ws_top(v: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = None;
    for (i, c) in v.char_indices() {
        match c {
            '(' => {
                depth += 1;
                start.get_or_insert(i);
            }
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if let Some(s) = start.take() {
                    out.push(&v[s..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(s) = start {
        out.push(&v[s..]);
    }
    out
}

/// `name(args)` → `(name, args)`.
fn function(v: &str) -> Option<(&str, &str)> {
    let open = v.find('(')?;
    let inner = v[open + 1..].strip_suffix(')')?;
    Some((v[..open].trim(), inner.trim()))
}

pub fn color(v: &str) -> Result<Color, String> {
    let v = v.trim();
    if let Some(hex) = v.strip_prefix('#') {
        let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).map(|d| d * 17);
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16);
        let bad = |_| format!("invalid colour {v}");
        return match hex.len() {
            3 => Ok(Color::rgba(
                digit(0).map_err(bad)?,
                digit(1).map_err(bad)?,
                digit(2).map_err(bad)?,
                255,
            )),
            4 => Ok(Color::rgba(
                digit(0).map_err(bad)?,
                digit(1).map_err(bad)?,
                digit(2).map_err(bad)?,
                digit(3).map_err(bad)?,
            )),
            6 => Ok(Color::rgba(
                pair(0).map_err(bad)?,
                pair(2).map_err(bad)?,
                pair(4).map_err(bad)?,
                255,
            )),
            8 => Ok(Color::rgba(
                pair(0).map_err(bad)?,
                pair(2).map_err(bad)?,
                pair(4).map_err(bad)?,
                pair(6).map_err(bad)?,
            )),
            _ => Err(format!("invalid colour {v}")),
        };
    }
    match v {
        "transparent" => return Ok(Color::TRANSPARENT),
        "black" => return Ok(Color::BLACK),
        "white" => return Ok(Color::WHITE),
        _ => {}
    }
    if let Some((name, args)) = function(v)
        && (name == "rgb" || name == "rgba")
    {
        let parts: Vec<&str> = args
            .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        let channel =
            |s: &str| -> Result<u8, String> { Ok(num(s)?.round().clamp(0.0, 255.0) as u8) };
        let alpha = |s: &str| -> Result<u8, String> {
            Ok((num(s)? * 255.0).round().clamp(0.0, 255.0) as u8)
        };
        return match parts.as_slice() {
            [r, g, b] => Ok(Color::rgba(channel(r)?, channel(g)?, channel(b)?, 255)),
            [r, g, b, a] => Ok(Color::rgba(
                channel(r)?,
                channel(g)?,
                channel(b)?,
                alpha(a)?,
            )),
            _ => Err(format!("invalid colour {v}")),
        };
    }
    Err(format!("invalid colour {v}"))
}

/// Colour stops of a gradient: first and last colour (intermediate stops are dropped).
fn stops(args: &[&str]) -> Result<(Color, Color), String> {
    let colors: Vec<Color> = args
        .iter()
        .map(|a| color(split_ws_top(a)[0]))
        .collect::<Result<_, _>>()?;
    match (colors.first(), colors.last()) {
        (Some(a), Some(b)) if colors.len() >= 2 => Ok((*a, *b)),
        _ => Err("a gradient needs two colours".into()),
    }
}

fn fill(v: &str) -> Result<Fill, String> {
    if let Some((name, args)) = function(v) {
        let args = split_top(args, ',');
        match name {
            "linear-gradient" => {
                let (angle, rest) = match args[0] {
                    "to bottom" => (180.0, &args[1..]),
                    "to top" => (0.0, &args[1..]),
                    "to right" => (90.0, &args[1..]),
                    "to left" => (270.0, &args[1..]),
                    a if a.ends_with("deg") => (num(a.trim_end_matches("deg"))?, &args[1..]),
                    _ => (180.0, &args[..]),
                };
                let (from, to) = stops(rest)?;
                return Ok(Fill::Linear { angle, from, to });
            }
            "radial-gradient" => {
                let rest = if color(split_ws_top(args[0])[0]).is_err() {
                    &args[1..]
                } else {
                    &args[..]
                };
                let (from, to) = stops(rest)?;
                return Ok(Fill::Radial { from, to });
            }
            _ => {}
        }
    }
    Ok(Fill::Solid(color(v)?))
}

fn border(v: &str) -> Result<(f32, Color), String> {
    let mut width = 1.0;
    let mut c = Color::TRANSPARENT;
    for part in split_ws_top(v) {
        if matches!(part, "solid" | "dashed" | "dotted") {
            continue;
        }
        if let Ok(w) = px(part) {
            width = w;
        } else {
            c = color(part)?;
        }
    }
    Ok((width, c))
}

fn shadow(v: &str) -> Result<Shadow, String> {
    let mut inset = false;
    let mut lengths = Vec::new();
    let mut c = Color::rgba(0, 0, 0, 255);
    for part in split_ws_top(v) {
        if part == "inset" {
            inset = true;
        } else if let Ok(l) = px(part) {
            lengths.push(l);
        } else {
            c = color(part)?;
        }
    }
    let get = |i: usize| lengths.get(i).copied().unwrap_or(0.0);
    if lengths.len() < 2 {
        return Err(format!("shadow needs offsets: {v}"));
    }
    Ok(Shadow {
        inset,
        x: get(0),
        y: get(1),
        blur: get(2),
        spread: get(3),
        color: c,
    })
}

fn border_image(v: &str, images: &mut ImageNames) -> Result<BorderImage, String> {
    let groups: Vec<&str> = v.split('/').map(str::trim).collect();
    let mut first = split_ws_top(groups[0]).into_iter();
    let image = images.intern(first.next().ok_or("missing image")?);
    let mut fill = false;
    let mut slice = Vec::new();
    for p in first {
        match p {
            "fill" => fill = true,
            "stretch" | "round" | "repeat" => {}
            p => slice.push(p),
        }
    }
    let slice = edges4(&slice.join(" "), num)?;
    let mut lengths = |i: usize| -> Result<Option<[f32; 4]>, String> {
        let Some(g) = groups.get(i) else {
            return Ok(None);
        };
        let words: Vec<&str> = g
            .split_whitespace()
            .filter(|w| {
                let keyword = matches!(*w, "fill" | "stretch" | "round" | "repeat");
                fill |= *w == "fill";
                !keyword
            })
            .collect();
        Ok(Some(edges4(&words.join(" "), px)?))
    };
    let width = lengths(1)?.unwrap_or(slice);
    let outset = lengths(2)?.unwrap_or([0.0; 4]);
    Ok(BorderImage {
        image,
        slice,
        width,
        outset,
        fill,
    })
}

fn weight(v: &str) -> Result<u16, String> {
    Ok(match v {
        "normal" => 400,
        "bold" => 700,
        _ => num(v)? as u16,
    })
}

fn line_height(v: &str) -> Result<LineHeight, String> {
    if v.ends_with("px") {
        Ok(LineHeight::Px(px(v)?))
    } else if v == "normal" {
        Ok(LineHeight::Mult(1.25))
    } else {
        Ok(LineHeight::Mult(num(v)?))
    }
}

/// `[italic] [weight] size[/line-height] family`.
fn font(t: &mut TextDecl, v: &str) -> Result<(), String> {
    let words: Vec<&str> = v.split_whitespace().collect();
    let size_at = words
        .iter()
        .position(|w| w.split('/').next().is_some_and(|s| s.ends_with("px")))
        .ok_or("font needs a size in px")?;
    for w in &words[..size_at] {
        match *w {
            "italic" => t.italic = Some(true),
            "normal" => {}
            w => t.weight = Some(weight(w)?),
        }
    }
    let mut size = words[size_at].split('/');
    t.size = Some(px(size.next().unwrap_or_default())?);
    if let Some(lh) = size.next() {
        t.line_height = Some(line_height(lh)?);
    }
    let family = words[size_at + 1..].join(" ");
    if !family.is_empty() {
        t.family = Some(family.trim_matches(',').to_owned());
    }
    if t.weight.is_none() {
        t.weight = Some(400);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn compile(v: Value) -> Style {
        Style::compile(
            &RawStyle::from_json(&v).unwrap(),
            &BTreeMap::new(),
            &mut ImageNames::default(),
        )
        .unwrap()
    }

    #[test]
    fn colours() {
        assert_eq!(color("#050805eb").unwrap(), Color::rgba(5, 8, 5, 0xeb));
        assert_eq!(color("#0008").unwrap(), Color::rgba(0, 0, 0, 0x88));
        assert_eq!(color("#fff").unwrap(), Color::WHITE);
        assert_eq!(
            color("rgb(0 0 0 / 0.5)").unwrap(),
            Color::rgba(0, 0, 0, 128)
        );
        assert!(color("#12").is_err());
    }

    #[test]
    fn shorthands_and_states() {
        let s = compile(json!({
            "padding": "12px 14px",
            "border": "1px solid #746c47",
            "box-shadow": "0 5px 16px #0008, inset 0 0 12px #e4bd5960",
            "font": "600 17px/1.5 serif",
            ":hover": { "filter": "brightness(1.22)", "color": "#fff" },
            ":disabled": { "opacity": ".42" }
        }));
        assert_eq!(s.layout.padding.left, LengthPercentage::length(14.0));
        assert_eq!(s.layout.padding.top, LengthPercentage::length(12.0));
        assert_eq!(s.visual.border_color, Some(Color::hex(0x746c47)));
        let shadows = s.visual.shadows.as_ref().unwrap();
        assert_eq!(shadows.len(), 2);
        assert!(shadows[1].inset);
        assert_eq!(s.text.size, Some(17.0));
        assert_eq!(s.text.weight, Some(600));
        assert_eq!(s.text.family.as_deref(), Some("serif"));
        let hover = s.visual_for(State(State::HOVER));
        assert_eq!(hover.brightness, Some(1.22));
        assert_eq!(s.text_color_for(State(State::HOVER)), Some(Color::WHITE));
        let both = s.visual_for(State(State::HOVER | State::DISABLED));
        assert_eq!(both.opacity, Some(0.42));
        assert_eq!(both.brightness, Some(1.22));
    }

    #[test]
    fn layout_states_are_refused() {
        let raw = RawStyle::from_json(&json!({":hover": {"width": "10px"}})).unwrap();
        assert!(Style::compile(&raw, &BTreeMap::new(), &mut ImageNames::default()).is_err());
    }

    #[test]
    fn grid_and_border_image() {
        let s = compile(json!({
            "display": "grid",
            "grid-template-columns": "repeat(4, minmax(0, 1fr))",
            "border-image": "rim 70 65 / 7px 9px / 2px stretch"
        }));
        assert_eq!(s.layout.grid_template_columns.len(), 1);
        let b = s.visual.border_image.unwrap();
        assert_eq!(b.slice, [70.0, 65.0, 70.0, 65.0]);
        assert_eq!(b.width, [7.0, 9.0, 7.0, 9.0]);
        assert_eq!(b.outset, [2.0; 4]);
    }

    #[test]
    fn palette_references() {
        let mut palette = BTreeMap::new();
        palette.insert("gold".to_owned(), "#d8c58c".to_owned());
        let raw = RawStyle::from_json(&json!({"color": "$gold"})).unwrap();
        let s = Style::compile(&raw, &palette, &mut ImageNames::default()).unwrap();
        assert_eq!(s.text.color, Some(Color::hex(0xd8c58c)));
        let raw = RawStyle::from_json(&json!({"color": "$missing"})).unwrap();
        assert!(Style::compile(&raw, &palette, &mut ImageNames::default()).is_err());
    }
}
