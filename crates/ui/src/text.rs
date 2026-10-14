//! Text: fonts, rich-text markup, shaping and layout with cosmic-text, and the glyph
//! atlas the draw list samples.
//!
//! Text blocks are shaped at physical size (font size × DPI × transforms) so glyphs are
//! rasterised crisp, and report logical sizes to layout. A block keeps its shaped buffer
//! and its positioned glyph list until its text, style, width or scale changes, so a
//! steady frame costs a copy per glyph.

use crate::draw::{AtlasChanges, AtlasUpdate};
use crate::geom::Color;
use crate::style::{TextAlign, TextStyle, TextTransform};
use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, Ellipsize, EllipsizeHeightLimit, Family, FontFeatures,
    FontSystem, Metrics, Shaping, Style as FontStyle, SwashCache, SwashContent, Weight, Wrap,
    fontdb,
};
use std::collections::{HashMap, HashSet};

/// Where fonts come from.
#[derive(Debug, Clone, Default)]
pub struct FontSources {
    /// Font files shipped with the game (`config/ui/fonts`).
    pub bundled: Vec<Vec<u8>>,
    /// Also load the operating system's fonts, so stacks may name system families
    /// (Georgia, the system UI font) and other scripts fall back to installed fonts.
    pub system: bool,
}

const ATLAS_START: u32 = 1024;
const ATLAS_MAX: u32 = 4096;
/// Empty texels around each glyph, so blurred shadows sample zeros.
const PAD: u32 = 3;

#[derive(Debug, Clone, Copy)]
struct AtlasGlyph {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
}

/// The glyph atlas: R8 coverage, shelf-packed.
pub(crate) struct Atlas {
    allocator: etagere::AtlasAllocator,
    size: u32,
    glyphs: HashMap<CacheKey, Option<AtlasGlyph>>,
    /// Bumped whenever the atlas is cleared; glyph lists from older epochs are stale.
    pub epoch: u64,
    pub changes: AtlasChanges,
}

impl Atlas {
    fn new() -> Self {
        Self {
            allocator: etagere::AtlasAllocator::new(etagere::size2(
                ATLAS_START as i32,
                ATLAS_START as i32,
            )),
            size: ATLAS_START,
            glyphs: HashMap::new(),
            epoch: 0,
            changes: AtlasChanges {
                size: ATLAS_START,
                reset: true,
                updates: Vec::new(),
            },
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// Clears the atlas (growing it if it filled up); visible text re-rasterises.
    pub(crate) fn reset(&mut self, grow: bool) {
        if grow && self.size < ATLAS_MAX {
            self.size *= 2;
        }
        self.allocator =
            etagere::AtlasAllocator::new(etagere::size2(self.size as i32, self.size as i32));
        self.glyphs.clear();
        self.epoch += 1;
        self.changes.size = self.size;
        self.changes.reset = true;
        self.changes.updates.clear();
    }
}

/// Fonts, shaping and the glyph atlas.
pub(crate) struct TextSystem {
    pub fonts: FontSystem,
    swash: SwashCache,
    pub atlas: Atlas,
    /// Role (`ui`, `serif`…) → resolved family name.
    roles: HashMap<String, String>,
    pub hotkey: Color,
    /// Set when the atlas was reset during this frame; text prepared earlier in the
    /// frame must be prepared again.
    pub reset_during_frame: bool,
    /// Family (lower case) → its static weights, or `None` for a variable font.
    weights: HashMap<String, Option<Vec<u16>>>,
}

/// Generic CSS families → platform candidates, in order.
fn generic(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "system-ui" | "ui-sans-serif" => &[
            "SF Pro Text",
            "SF Pro",
            ".SF NS",
            "Segoe UI",
            "Helvetica Neue",
            "Arial",
        ],
        "sans-serif" => &["Helvetica", "Arial", "Helvetica Neue"],
        "serif" => &["Times New Roman", "Times"],
        "monospace" | "ui-monospace" => &["Menlo", "SF Mono", "Consolas", "Courier New"],
        _ => return None,
    })
}

impl TextSystem {
    pub fn new(
        sources: &FontSources,
        families: &std::collections::BTreeMap<String, Vec<String>>,
        hotkey: Color,
    ) -> Self {
        let _span = tracing::info_span!("ui fonts").entered();
        let mut db = fontdb::Database::new();
        for data in &sources.bundled {
            db.load_font_data(data.clone());
        }
        if sources.system {
            db.load_system_fonts();
        }
        let present: HashSet<String> = db
            .faces()
            .flat_map(|f| f.families.iter().map(|(n, _)| n.to_lowercase()))
            .collect();
        let first_family = db
            .faces()
            .next()
            .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
            .unwrap_or_default();
        let mut roles = HashMap::new();
        for (role, stack) in families {
            let found = stack.iter().find_map(|name| {
                let candidates: Vec<&str> = match generic(name) {
                    Some(list) => list.to_vec(),
                    None => vec![name.as_str()],
                };
                candidates
                    .into_iter()
                    .find(|c| present.contains(&c.to_lowercase()))
                    .map(str::to_owned)
            });
            let family = found.unwrap_or_else(|| first_family.clone());
            tracing::debug!(role, family, "ui font role");
            roles.insert(role.clone(), family);
        }
        let fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
        Self {
            fonts,
            swash: SwashCache::new(),
            atlas: Atlas::new(),
            roles,
            hotkey,
            reset_during_frame: false,
            weights: HashMap::new(),
        }
    }

    /// The weight to request so that a family's faces match, as CSS font matching
    /// would choose (cosmic-text only matches exact weights of static fonts).
    pub fn snap_weight(&mut self, family: &str, wanted: u16) -> u16 {
        let key = family.to_lowercase();
        if !self.weights.contains_key(&key) {
            let db = self.fonts.db();
            let faces: Vec<_> = db
                .faces()
                .filter(|f| f.families.iter().any(|(n, _)| n.to_lowercase() == key))
                .map(|f| (f.id, f.weight.0))
                .collect();
            let variable = faces.iter().any(|(id, _)| {
                db.with_face_data(*id, |data, index| has_table(data, index, *b"fvar"))
                    .unwrap_or(false)
            });
            let mut list: Vec<u16> = faces.iter().map(|(_, w)| *w).collect();
            list.sort_unstable();
            list.dedup();
            self.weights
                .insert(key.clone(), (!variable && !list.is_empty()).then_some(list));
        }
        let Some(Some(list)) = self.weights.get(&key) else {
            return wanted;
        };
        if list.contains(&wanted) {
            return wanted;
        }
        // CSS: above 500 prefer heavier, otherwise lighter; then the other direction.
        let heavier = list.iter().copied().filter(|w| *w > wanted).min();
        let lighter = list.iter().copied().filter(|w| *w < wanted).max();
        if wanted > 500 {
            heavier.or(lighter).unwrap_or(wanted)
        } else {
            lighter.or(heavier).unwrap_or(wanted)
        }
    }

    /// The family a role (or a literal family name) resolves to.
    pub fn family<'a>(&'a self, role: &'a str) -> &'a str {
        self.roles.get(role).map_or(role, String::as_str)
    }

    /// The atlas entry for a glyph, rasterising it on first use.
    fn glyph(&mut self, key: CacheKey) -> Option<AtlasGlyph> {
        if let Some(g) = self.atlas.glyphs.get(&key) {
            return *g;
        }
        let image = self.swash.get_image_uncached(&mut self.fonts, key);
        let entry = image.and_then(|image| {
            let (w, h) = (image.placement.width, image.placement.height);
            if w == 0 || h == 0 {
                return None;
            }
            let coverage: Vec<u8> = match image.content {
                SwashContent::Mask => image.data,
                SwashContent::SubpixelMask => image
                    .data
                    .chunks_exact(4)
                    .map(|p| ((p[0] as u16 + p[1] as u16 + p[2] as u16) / 3) as u8)
                    .collect(),
                // Colour glyphs (emoji) draw as their alpha in the text colour for now.
                SwashContent::Color => image.data.chunks_exact(4).map(|p| p[3]).collect(),
            };
            let (pw, ph) = (w + PAD * 2, h + PAD * 2);
            let alloc = match self
                .atlas
                .allocator
                .allocate(etagere::size2(pw as i32, ph as i32))
            {
                Some(a) => a,
                None => {
                    self.atlas.reset(true);
                    self.reset_during_frame = true;
                    self.atlas
                        .allocator
                        .allocate(etagere::size2(pw as i32, ph as i32))?
                }
            };
            let (x, y) = (
                alloc.rectangle.min.x as u32 + PAD,
                alloc.rectangle.min.y as u32 + PAD,
            );
            self.atlas.changes.updates.push(AtlasUpdate {
                x,
                y,
                width: w,
                height: h,
                data: coverage,
            });
            Some(AtlasGlyph {
                x,
                y,
                w,
                h,
                left: image.placement.left,
                top: image.placement.top,
            })
        });
        self.atlas.glyphs.insert(key, entry);
        entry
    }
}

/// Whether a font (or a face of a collection) has a table, read from its directory.
fn has_table(data: &[u8], index: u32, tag: [u8; 4]) -> bool {
    let u32_at = |o: usize| -> Option<u32> {
        Some(u32::from_be_bytes(data.get(o..o + 4)?.try_into().ok()?))
    };
    let mut offset = 0usize;
    if data.get(0..4) == Some(b"ttcf") {
        match u32_at(12 + 4 * index as usize) {
            Some(o) => offset = o as usize,
            None => return false,
        }
    }
    let Some(count) = data
        .get(offset + 4..offset + 6)
        .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
    else {
        return false;
    };
    (0..count).any(|i| data.get(offset + 12 + i * 16..offset + 16 + i * 16) == Some(&tag[..]))
}

// --- Markup --------------------------------------------------------------------------

/// Style of a run of marked-up text.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpanStyle {
    pub bold: bool,
    pub italic: bool,
    pub color: Option<Color>,
}

/// Parses markup: `[b]…[/b]`, `[i]…[/i]`, `[color=#hex]…[/color]`, `[key]…[/key]`
/// (the theme's hotkey colour). `[[` is a literal bracket; unknown tags stay text.
pub fn parse_markup(text: &str, hotkey: Color) -> Vec<(String, SpanStyle)> {
    let mut out: Vec<(String, SpanStyle)> = Vec::new();
    let mut stack: Vec<SpanStyle> = vec![SpanStyle::default()];
    let mut current = String::new();
    let mut rest = text;
    let flush = |out: &mut Vec<(String, SpanStyle)>, current: &mut String, style: SpanStyle| {
        if !current.is_empty() {
            out.push((std::mem::take(current), style));
        }
    };
    while let Some(open) = rest.find('[') {
        current.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if let Some(stripped) = after.strip_prefix('[') {
            current.push('[');
            rest = stripped;
            continue;
        }
        let Some(close) = after.find(']') else {
            current.push('[');
            rest = after;
            continue;
        };
        let tag = &after[..close];
        let top = *stack.last().unwrap_or(&SpanStyle::default());
        let new = match tag {
            "b" => Some(SpanStyle { bold: true, ..top }),
            "i" => Some(SpanStyle {
                italic: true,
                ..top
            }),
            "key" => Some(SpanStyle {
                color: Some(hotkey),
                ..top
            }),
            t if t.starts_with("color=") => crate::style::color(&t[6..]).ok().map(|c| SpanStyle {
                color: Some(c),
                ..top
            }),
            _ => None,
        };
        let closing = matches!(tag, "/b" | "/i" | "/key" | "/color");
        if let Some(style) = new {
            flush(&mut out, &mut current, top);
            stack.push(style);
        } else if closing && stack.len() > 1 {
            flush(&mut out, &mut current, top);
            stack.pop();
        } else {
            current.push('[');
            current.push_str(tag);
            current.push(']');
        }
        rest = &after[close + 1..];
    }
    current.push_str(rest);
    let top = *stack.last().unwrap_or(&SpanStyle::default());
    flush(&mut out, &mut current, top);
    out
}

/// Plain text without markup (for text fields and accessibility).
pub fn strip_markup(text: &str) -> String {
    parse_markup(text, Color::WHITE)
        .into_iter()
        .map(|(s, _)| s)
        .collect()
}

// --- Text blocks ---------------------------------------------------------------------

/// A positioned glyph quad relative to the block's top left, in physical pixels.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placed {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Atlas texel rectangle `x, y, w, h`.
    pub atlas: [u32; 4],
    pub color: Option<Color>,
}

/// One piece of shaped text.
pub(crate) struct TextBlock {
    markup: String,
    pub style: TextStyle,
    /// Markup is plain text (text fields): brackets are literal.
    pub plain: bool,
    buffer: Buffer,
    scale: f32,
    width: Option<f32>,
    needs_text: bool,
    layout_gen: u64,
    glyph_key: (u64, u64),
    pub glyphs: Vec<Placed>,
    /// Physical size of the laid-out text.
    pub size: (f32, f32),
}

impl TextBlock {
    pub fn new(markup: &str, style: TextStyle) -> Self {
        Self {
            markup: markup.to_owned(),
            style,
            plain: false,
            buffer: Buffer::new_empty(Metrics::new(13.0, 16.0)),
            scale: 0.0,
            width: None,
            needs_text: true,
            layout_gen: 1,
            glyph_key: (0, u64::MAX),
            glyphs: Vec::new(),
            size: (0.0, 0.0),
        }
    }

    pub fn markup(&self) -> &str {
        &self.markup
    }

    /// Returns whether the text changed.
    pub fn set_markup(&mut self, markup: &str) -> bool {
        if self.markup == markup {
            return false;
        }
        self.markup.clear();
        self.markup.push_str(markup);
        self.needs_text = true;
        true
    }

    pub fn is_empty(&self) -> bool {
        self.markup.is_empty()
    }

    fn ensure(&mut self, text: &mut TextSystem, scale: f32) {
        if !self.needs_text && self.scale == scale {
            return;
        }
        let _span = tracing::trace_span!("ui shape").entered();
        let st = &self.style;
        let size = st.size * scale;
        self.buffer
            .set_metrics(Metrics::new(size, st.line_height_px() * scale));
        self.buffer.set_wrap(if st.nowrap {
            Wrap::None
        } else {
            Wrap::WordOrGlyph
        });
        let ellipsize = if st.ellipsis {
            Ellipsize::End(EllipsizeHeightLimit::Lines(st.max_lines.max(1)))
        } else if st.max_lines > 0 {
            Ellipsize::End(EllipsizeHeightLimit::Lines(st.max_lines))
        } else {
            Ellipsize::None
        };
        self.buffer.set_ellipsize(ellipsize);
        let family = text.family(&st.family).to_owned();
        let weight = text.snap_weight(&family, st.weight);
        let bold = text.snap_weight(&family, st.weight.max(700));
        let mut features = FontFeatures::new();
        if st.tabular {
            features.enable(cosmic_text::FeatureTag::new(b"tnum"));
        }
        let base = Attrs::new()
            .family(Family::Name(&family))
            .weight(Weight(weight))
            .style(if st.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            })
            .letter_spacing(st.letter_spacing)
            .font_features(features);
        let spans = if self.plain {
            vec![(self.markup.clone(), SpanStyle::default())]
        } else {
            parse_markup(&self.markup, text.hotkey)
        };
        let small = Metrics::new(size * 0.8, st.line_height_px() * scale);
        let mut runs: Vec<(String, Attrs)> = Vec::with_capacity(spans.len());
        for (s, span) in &spans {
            let mut attrs = base.clone();
            if span.bold {
                attrs = attrs.weight(Weight(bold));
            }
            if span.italic {
                attrs = attrs.style(FontStyle::Italic);
            }
            if let Some(c) = span.color {
                attrs = attrs.color(cosmic_text::Color::rgba(c.r, c.g, c.b, c.a));
            }
            match st.transform {
                TextTransform::None => runs.push((s.clone(), attrs)),
                TextTransform::Uppercase => runs.push((s.to_uppercase(), attrs)),
                TextTransform::SmallCaps => {
                    // Lower-case runs become capitals at 80 %.
                    let mut chunk = String::new();
                    let mut lower = None;
                    for ch in s.chars() {
                        let is_lower = ch.is_lowercase();
                        if lower.is_some_and(|l| l != is_lower) {
                            let a = if lower == Some(true) {
                                attrs.clone().metrics(small)
                            } else {
                                attrs.clone()
                            };
                            runs.push((std::mem::take(&mut chunk), a));
                        }
                        lower = Some(is_lower);
                        chunk.extend(ch.to_uppercase());
                    }
                    if !chunk.is_empty() {
                        let a = if lower == Some(true) {
                            attrs.clone().metrics(small)
                        } else {
                            attrs.clone()
                        };
                        runs.push((chunk, a));
                    }
                }
            }
        }
        let align = match st.align {
            TextAlign::Left => None,
            TextAlign::Center => Some(Align::Center),
            TextAlign::Right => Some(Align::Right),
        };
        self.buffer.set_rich_text(
            runs.iter().map(|(s, a)| (s.as_str(), a.clone())),
            &base,
            Shaping::Advanced,
            align,
        );
        self.scale = scale;
        self.needs_text = false;
        self.width = None;
        self.layout_gen += 1;
        self.relayout(text, None, true);
    }

    fn relayout(&mut self, text: &mut TextSystem, width: Option<f32>, force: bool) {
        if !force && self.width == width {
            return;
        }
        self.width = width;
        self.buffer.set_size(width, None);
        self.buffer.shape_until_scroll(&mut text.fonts, false);
        let mut w: f32 = 0.0;
        let mut h: f32 = 0.0;
        for run in self.buffer.layout_runs() {
            w = w.max(run.line_w);
            h = h.max(run.line_top + run.line_height);
        }
        self.size = (w, h);
        self.layout_gen += 1;
    }

    /// Logical size with text wrapped at `max_width` logical pixels.
    pub fn measure(
        &mut self,
        text: &mut TextSystem,
        scale: f32,
        max_width: Option<f32>,
    ) -> (f32, f32) {
        self.ensure(text, scale);
        let width = max_width.map(|w| (w * scale).max(1.0).ceil());
        self.relayout(text, width, false);
        (self.size.0 / scale, self.size.1 / scale)
    }

    /// Lays the text out at its final width (physical pixels) and refreshes the glyph list.
    pub fn prepare(&mut self, text: &mut TextSystem, scale: f32, width: Option<f32>) {
        self.ensure(text, scale);
        let width = width.map(|w| w.max(1.0).ceil());
        self.relayout(text, width, false);
        let key = (self.layout_gen, text.atlas.epoch);
        if self.glyph_key == key {
            return;
        }
        self.glyphs.clear();
        let mut placed = Vec::new();
        for run in self.buffer.layout_runs() {
            for g in run.glyphs {
                let p = g.physical((0.0, 0.0), 1.0);
                placed.push((p.cache_key, p.x, run.line_y as i32 + p.y, g.color_opt));
            }
        }
        for (cache_key, x, y, color) in placed {
            if let Some(a) = text.glyph(cache_key) {
                self.glyphs.push(Placed {
                    x: (x + a.left) as f32,
                    y: (y - a.top) as f32,
                    w: a.w as f32,
                    h: a.h as f32,
                    atlas: [a.x, a.y, a.w, a.h],
                    color: color.map(|c| {
                        let [r, g, b, a] = c.as_rgba();
                        Color::rgba(r, g, b, a)
                    }),
                });
            }
        }
        self.glyph_key = (self.layout_gen, text.atlas.epoch);
    }

    /// X offset (physical px) of the caret before byte `index` on the first line.
    pub fn caret_x(&self, index: usize) -> f32 {
        let mut x = 0.0;
        if let Some(run) = self.buffer.layout_runs().next() {
            for g in run.glyphs {
                if g.start >= index {
                    return g.x;
                }
                x = g.x + g.w;
            }
        }
        x
    }

    /// Byte index nearest to `x` physical px on the first line.
    pub fn hit_index(&self, x: f32) -> usize {
        if let Some(run) = self.buffer.layout_runs().next() {
            for g in run.glyphs {
                if x < g.x + g.w * 0.5 {
                    return g.start;
                }
            }
            return run.glyphs.last().map_or(0, |g| g.end);
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_spans() {
        let gold = Color::hex(0xffd26a);
        let spans = parse_markup("Attack ([key]A[/key]) [b]now[/b] [[x] [nope]", gold);
        let texts: Vec<&str> = spans.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(texts, ["Attack (", "A", ") ", "now", " [x] [nope]"]);
        assert_eq!(spans[1].1.color, Some(gold));
        assert!(spans[3].1.bold);
        assert!(!spans[4].1.bold);
    }

    #[test]
    fn nested_colour() {
        let spans = parse_markup("[color=#ff0000]red [b]bold[/b][/color] plain", Color::WHITE);
        assert_eq!(spans[0].1.color, Some(Color::hex(0xff0000)));
        assert!(spans[1].1.bold && spans[1].1.color == Some(Color::hex(0xff0000)));
        assert_eq!(spans[2].1, SpanStyle::default());
        assert_eq!(strip_markup("a [key]Q[/key] b"), "a Q b");
    }
}
