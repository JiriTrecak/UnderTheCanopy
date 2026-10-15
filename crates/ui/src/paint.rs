//! Painting: the node tree → quads, in tree order within z layers.

use crate::config::Kind;
use crate::draw::{
    FLAG_GRAYSCALE, FLAG_LINEAR_TEXELS, FLAG_PREMULTIPLIED, Painter, Quad, QuadKind,
};
use crate::geom::{Color, Rect};
use crate::input::InputState;
use crate::layout::text_area;
use crate::node::{Node, Part, Tree};
use crate::style::{
    Fill, FillMode, ImageKey, ImageSize, PointerEvents, State, Style, TextShadow, VAlign, Visual,
};
use crate::text::TextBlock;
use crate::{ImageDesc, Ui};

/// What a pointer position hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitPart {
    Node,
    /// A dropdown option.
    Option(usize),
    ScrollThumb,
    ScrollTrack,
}

/// A hit-testable region, recorded in paint order.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hit {
    pub rect: Rect,
    pub clip: Option<Rect>,
    pub node: usize,
    pub part: HitPart,
}

impl Hit {
    pub fn contains(&self, p: crate::geom::Vec2) -> bool {
        self.rect.contains(p) && self.clip.is_none_or(|c| c.contains(p))
    }
}

/// Inherited CSS effects: `opacity` and `filter`.
#[derive(Debug, Clone, Copy)]
struct Fx {
    opacity: f32,
    brightness: f32,
    grayscale: bool,
}

impl Default for Fx {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            brightness: 1.0,
            grayscale: false,
        }
    }
}

impl Fx {
    fn apply(self, v: &Visual) -> Fx {
        Fx {
            opacity: self.opacity * v.opacity.unwrap_or(1.0),
            brightness: self.brightness * v.brightness.unwrap_or(1.0),
            grayscale: self.grayscale || v.grayscale.unwrap_or(false),
        }
    }

    fn color(&self, c: Color) -> [u8; 4] {
        c.with_alpha(self.opacity).to_array()
    }
}

enum Deferred {
    Node(i32, usize, Fx),
    Popup(usize, Fx),
}

impl Deferred {
    fn layer(&self) -> i32 {
        match self {
            Deferred::Node(l, _, _) => *l,
            Deferred::Popup(..) => i32::MAX - 1,
        }
    }
}

/// How dropdown popups lay out their rows (shared with input).
pub(crate) fn popup_rows(node: &Node) -> (Rect, f32) {
    let s = node.scale;
    let row = (node.text_style.line_height_px() + 10.0) * s;
    let pad = 4.0 * s;
    let rect = Rect::new(
        node.rect.x,
        node.rect.bottom() + 2.0 * s,
        node.rect.w,
        row * node.options.len() as f32 + pad * 2.0,
    );
    (rect, row)
}

pub(crate) fn scrollbar(node: &Node) -> Option<(Rect, Rect)> {
    if !node.spec.style.scroll || node.scroll_max.y <= 0.5 {
        return None;
    }
    let s = node.scale;
    let width = node
        .spec
        .part(Part::Scrollbar)
        .and_then(|p| p.width_px)
        .unwrap_or(6.0)
        * s;
    let track = Rect::new(
        node.rect.right() - width - 3.0 * s,
        node.rect.y + 3.0 * s,
        width,
        node.rect.h - 6.0 * s,
    );
    let visible = node.rect.h / s;
    let total = visible + node.scroll_max.y;
    let h = (track.h * visible / total).max(16.0 * s).min(track.h);
    let t = node.scroll.y / node.scroll_max.y;
    let thumb = Rect::new(track.x, track.y + (track.h - h) * t, width, h);
    Some((track, thumb))
}

struct Ctx<'a, 'b> {
    tree: &'a Tree,
    images: &'a [Option<ImageDesc>],
    input: &'a InputState,
    atlas: f32,
    time: f64,
    hits: &'a mut Vec<Hit>,
    p: Painter<'b>,
}

impl Ui {
    pub(crate) fn paint(&mut self) {
        let _span = tracing::trace_span!("ui paint").entered();
        let Ui {
            tree,
            draw,
            hits,
            image_table,
            text,
            input,
            time,
            viewport,
            ..
        } = self;
        draw.clear(*viewport);
        hits.clear();
        let changes = &mut text.atlas.changes;
        draw.atlas.size = changes.size;
        draw.atlas.reset = std::mem::take(&mut changes.reset);
        std::mem::swap(&mut draw.atlas.updates, &mut changes.updates);
        let Some(root) = tree.root else {
            return;
        };
        let mut ctx = Ctx {
            tree,
            images: image_table,
            input,
            atlas: text.atlas.size() as f32,
            time: *time,
            hits,
            p: Painter::new(draw),
        };
        let mut deferred = Vec::new();
        ctx.node(root, 0, Fx::default(), &mut deferred);
        while !deferred.is_empty() {
            let layer = deferred.iter().map(Deferred::layer).min().unwrap_or(0);
            let (now, later): (Vec<_>, Vec<_>) =
                deferred.into_iter().partition(|d| d.layer() == layer);
            deferred = later;
            for d in now {
                match d {
                    Deferred::Node(l, n, fx) => ctx.node(n, l, fx, &mut deferred),
                    Deferred::Popup(n, fx) => ctx.popup(n, fx),
                }
            }
        }
    }
}

fn radius(r: Option<crate::style::Len>, rect: &Rect, s: f32) -> f32 {
    match r {
        Some(crate::style::Len::Px(v)) => v * s,
        Some(crate::style::Len::Percent(p)) => p * rect.w.min(rect.h),
        None => 0.0,
    }
    .min(rect.w.min(rect.h) * 0.5)
}

impl Ctx<'_, '_> {
    fn state(&self, index: usize) -> State {
        let tree = self.tree;
        let Some(owner) = tree.node(index).owner else {
            return State(0);
        };
        let o = tree.node(owner);
        let i = self.input;
        let mut s = 0;
        if i.hover_owner == Some(owner) {
            s |= State::HOVER;
        }
        if i.pressed.is_some_and(|p| p.node == owner) && i.hover_owner == Some(owner) {
            s |= State::ACTIVE;
        }
        if i.focus == Some(owner) && (i.focus_visible || o.kind() == Kind::TextField) {
            s |= State::FOCUS;
        }
        if o.checked || (o.kind() == Kind::Dropdown && o.open) {
            s |= State::CHECKED;
        }
        if !o.enabled {
            s |= State::DISABLED;
        }
        State(s)
    }

    fn node(&mut self, index: usize, layer: i32, fx: Fx, deferred: &mut Vec<Deferred>) {
        let tree = self.tree;
        let n = tree.node(index);
        if !n.visible {
            return;
        }
        if n.layer != layer {
            deferred.push(Deferred::Node(n.layer, index, fx));
            return;
        }
        self.p.set_clip(n.clip);
        let state = self.state(index);
        let style = &n.spec.style;
        let vis = style.visual_for(state);
        let mut fx = fx.apply(&vis);
        fx.opacity *= n.opacity;
        if fx.opacity <= 0.002 {
            return;
        }
        let rect = n.rect;
        let s = n.scale;
        if n.pointer == PointerEvents::Auto && Some(index) != tree.root {
            self.hits.push(Hit {
                rect,
                clip: n.clip,
                node: index,
                part: HitPart::Node,
            });
        }
        let override_bg = match n.kind() {
            Kind::Bar | Kind::Sweep => None,
            _ => n.color,
        };
        let r = radius(vis.radius, &rect, s);
        if !self.p.culled(&rect.expand(64.0 * s)) {
            self.decorate(&vis, rect, s, r, fx, override_bg);
        }
        let text_color = style.text_color_for(state).unwrap_or(n.text_style.color);
        match n.kind() {
            Kind::Image => {
                let fit = vis
                    .background_image
                    .as_ref()
                    .map_or(ImageSize::Stretch, |b| b.size);
                if let Some(key) = n.image {
                    let tint = vis.image_tint.unwrap_or(Color::WHITE);
                    let ir = vis.image_radius.map_or(r, |l| radius(Some(l), &rect, s));
                    self.image(key, n.content, fit, tint, ir, fx, 0);
                }
            }
            Kind::Bar => self.bar(n, &vis, fx),
            Kind::Sweep => {
                let fill = n.color.or(match &vis.fill {
                    Some(Fill::Solid(c)) => Some(*c),
                    _ => None,
                });
                let mut q = Quad::new(QuadKind::Sweep, n.content, Color::TRANSPARENT);
                q.color = fx.color(fill.unwrap_or(Color::rgba(0, 0, 0, 200)));
                q.color2 = fx.color(vis.sweep_rest.unwrap_or(Color::TRANSPARENT));
                q.param = n.value.clamp(0.0, 1.0);
                q.border = vis.sweep_ring.unwrap_or(0.0) * s;
                q.radius = radius(vis.radius, &n.content, s);
                q.shade = fx.brightness;
                if n.value > 0.0 {
                    self.p.push(q, None);
                }
            }
            Kind::Slider => self.slider(index, fx),
            Kind::Checkbox => self.checkbox(index, fx),
            Kind::TextField => self.field(index, fx, text_color),
            Kind::Dropdown => self.dropdown(index, fx, text_color, deferred),
            _ => {}
        }
        if n.kind() != Kind::TextField
            && let Some(block) = &n.text
            && let Some(area) = text_area(n)
        {
            self.text(
                block,
                area,
                n.text_style.valign,
                text_color,
                n.text_style.shadow,
                s,
                fx,
            );
        }
        for &c in &n.children {
            self.node(c, layer, fx, deferred);
        }
        self.p.set_clip(n.clip);
        if let Some((track, thumb)) = scrollbar(n) {
            self.part_box(
                n,
                Part::Scrollbar,
                track,
                State(0),
                fx,
                Some(Color::rgba(0, 0, 0, 60)),
            );
            let hover = self.input.hover_part == Some((index, HitPart::ScrollThumb));
            self.part_box(
                n,
                Part::ScrollThumb,
                thumb,
                State(if hover { State::HOVER } else { 0 }),
                fx,
                Some(Color::rgba(232, 224, 208, 70)),
            );
            self.hits.push(Hit {
                rect: track,
                clip: n.clip,
                node: index,
                part: HitPart::ScrollTrack,
            });
            self.hits.push(Hit {
                rect: thumb,
                clip: n.clip,
                node: index,
                part: HitPart::ScrollThumb,
            });
        }
        if let Some(o) = vis.outline
            && o.width > 0.0
            && !o.color.is_transparent()
        {
            let grow = (o.offset + o.width) * s;
            let outer = rect.expand(grow);
            let mut q = Quad::new(QuadKind::Shape, outer, Color::TRANSPARENT);
            q.color2 = fx.color(o.color);
            q.border = o.width * s;
            q.radius = (r + grow).max(0.0);
            q.shade = fx.brightness;
            self.p.push(q, None);
        }
    }

    /// Box painting: outer shadows, background, background image, inset shadows,
    /// border, border image.
    fn decorate(
        &mut self,
        v: &Visual,
        rect: Rect,
        s: f32,
        r: f32,
        fx: Fx,
        bg_override: Option<Color>,
    ) {
        if let Some(shadows) = &v.shadows {
            for sh in shadows.iter().filter(|sh| !sh.inset) {
                let spread = sh.spread * s;
                let blur = sh.blur * s;
                let shape = rect
                    .translate(crate::geom::Vec2::new(sh.x * s, sh.y * s))
                    .expand(spread);
                let mut q = Quad::new(
                    QuadKind::Shadow,
                    shape.expand(blur + 1.0),
                    Color::TRANSPARENT,
                );
                q.color = fx.color(sh.color);
                q.uv = [sh.x * s, sh.y * s, 0.0, 0.0];
                q.border = spread;
                q.soft = blur;
                q.radius = r;
                self.p.push(q, None);
            }
        }
        let background = match bg_override {
            Some(c) => Some(Fill::Solid(c)),
            None => v.background.clone(),
        };
        let border = v
            .border_width
            .filter(|w| *w > 0.0)
            .zip(v.border_color)
            .filter(|(_, c)| !c.is_transparent());
        match &background {
            Some(Fill::Solid(c)) if !c.is_transparent() || border.is_some() => {
                let mut q = Quad::new(QuadKind::Shape, rect, Color::TRANSPARENT);
                q.color = fx.color(*c);
                if let Some((w, bc)) = border {
                    q.border = w * s;
                    q.color2 = fx.color(bc);
                }
                q.radius = r;
                q.shade = fx.brightness;
                self.p.push(q, None);
            }
            Some(Fill::Linear { angle, from, to }) => {
                let mut q = Quad::new(QuadKind::Linear, rect, *from);
                q.color = fx.color(*from);
                q.color2 = fx.color(*to);
                q.param = *angle;
                q.radius = r;
                q.shade = fx.brightness;
                self.p.push(q, None);
            }
            Some(Fill::Radial { from, to }) => {
                let mut q = Quad::new(QuadKind::Radial, rect, *from);
                q.color = fx.color(*from);
                q.color2 = fx.color(*to);
                q.radius = r;
                q.shade = fx.brightness;
                self.p.push(q, None);
            }
            _ => {}
        }
        if let Some(img) = &v.background_image
            && let Some(key) = img.image
        {
            let inset = [
                img.inset[0].resolve(rect.h / s) * s,
                img.inset[1].resolve(rect.w / s) * s,
                img.inset[2].resolve(rect.h / s) * s,
                img.inset[3].resolve(rect.w / s) * s,
            ];
            let area = rect.outset([-inset[0], -inset[1], -inset[2], -inset[3]]);
            let tint = v.image_tint.unwrap_or(Color::WHITE);
            let ir = v.image_radius.map_or(0.0, |l| radius(Some(l), &area, s));
            self.image(key, area, img.size, tint, ir, fx, 0);
        }
        if let Some(shadows) = &v.shadows {
            for sh in shadows.iter().filter(|sh| sh.inset) {
                let mut q = Quad::new(QuadKind::InsetShadow, rect, Color::TRANSPARENT);
                q.color = fx.color(sh.color);
                q.uv = [sh.x * s, sh.y * s, 0.0, 0.0];
                q.border = sh.spread * s;
                q.soft = sh.blur * s;
                q.radius = r;
                self.p.push(q, None);
            }
        }
        if let Some((w, c)) = border
            && !matches!(background, Some(Fill::Solid(_)))
        {
            let mut q = Quad::new(QuadKind::Shape, rect, Color::TRANSPARENT);
            q.color2 = fx.color(c);
            q.border = w * s;
            q.radius = r;
            q.shade = fx.brightness;
            self.p.push(q, None);
        }
        if let Some(b) = &v.border_image {
            self.nine_slice(b, rect, s, fx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn image(
        &mut self,
        key: ImageKey,
        area: Rect,
        fit: ImageSize,
        tint: Color,
        r: f32,
        fx: Fx,
        extra: u32,
    ) {
        let Some(desc) = self.images.get(key.0 as usize).copied().flatten() else {
            return;
        };
        if self.p.culled(&area) {
            return;
        }
        let [iw, ih] = desc.size;
        let mut uv = desc.uv;
        let mut dest = area;
        if iw > 0.0 && ih > 0.0 && fit != ImageSize::Stretch {
            let sx = area.w / iw;
            let sy = area.h / ih;
            match fit {
                ImageSize::Contain => {
                    let k = sx.min(sy);
                    let (w, h) = (iw * k, ih * k);
                    dest = Rect::new(
                        area.x + (area.w - w) * 0.5,
                        area.y + (area.h - h) * 0.5,
                        w,
                        h,
                    );
                }
                ImageSize::Cover => {
                    let k = sx.max(sy);
                    // Crop the texture rectangle to the visible part.
                    let vis_w = area.w / (iw * k);
                    let vis_h = area.h / (ih * k);
                    let (du, dv) = (uv[2] - uv[0], uv[3] - uv[1]);
                    let cu = uv[0] + du * 0.5;
                    let cv = uv[1] + dv * 0.5;
                    uv = [
                        cu - du * vis_w * 0.5,
                        cv - dv * vis_h * 0.5,
                        cu + du * vis_w * 0.5,
                        cv + dv * vis_h * 0.5,
                    ];
                }
                ImageSize::Stretch => {}
            }
        }
        let mut q = Quad::new(QuadKind::Image, dest, tint);
        q.color = fx.color(tint);
        q.uv = uv;
        q.radius = r;
        q.shade = fx.brightness;
        q.flags |= extra;
        if desc.premultiplied {
            q.flags |= FLAG_PREMULTIPLIED;
        }
        if desc.srgb_view {
            q.flags |= FLAG_LINEAR_TEXELS;
        }
        if fx.grayscale {
            q.flags |= FLAG_GRAYSCALE;
        }
        self.p.push(q, Some(desc.texture));
    }

    fn nine_slice(&mut self, b: &crate::style::BorderImage, rect: Rect, s: f32, fx: Fx) {
        let Some(desc) = self.images.get(b.image.0 as usize).copied().flatten() else {
            return;
        };
        let outer = rect.outset(b.outset.map(|o| o * s));
        if self.p.culled(&outer) {
            return;
        }
        let [iw, ih] = desc.size;
        let [u0, v0, u1, v1] = desc.uv;
        let su = |px: f32| u0 + (u1 - u0) * (px / iw);
        let sv = |px: f32| v0 + (v1 - v0) * (px / ih);
        // Source cuts and destination widths: top, right, bottom, left.
        let [st, sr, sb, sl] = b.slice;
        let [wt, wr, wb, wl] = b.width.map(|w| w * s);
        let xs = [outer.x, outer.x + wl, outer.right() - wr, outer.right()];
        let ys = [outer.y, outer.y + wt, outer.bottom() - wb, outer.bottom()];
        let us = [su(0.0), su(sl), su(iw - sr), su(iw)];
        let vs = [sv(0.0), sv(st), sv(ih - sb), sv(ih)];
        for row in 0..3 {
            for col in 0..3 {
                if row == 1 && col == 1 && !b.fill {
                    continue;
                }
                let dest = Rect::new(
                    xs[col],
                    ys[row],
                    xs[col + 1] - xs[col],
                    ys[row + 1] - ys[row],
                );
                if dest.is_empty() {
                    continue;
                }
                let mut q = Quad::new(QuadKind::Image, dest, Color::WHITE);
                q.color = fx.color(Color::WHITE);
                q.uv = [us[col], vs[row], us[col + 1], vs[row + 1]];
                q.shade = fx.brightness;
                if desc.premultiplied {
                    q.flags |= FLAG_PREMULTIPLIED;
                }
                if desc.srgb_view {
                    q.flags |= FLAG_LINEAR_TEXELS;
                }
                if fx.grayscale {
                    q.flags |= FLAG_GRAYSCALE;
                }
                self.p.push(q, Some(desc.texture));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        block: &TextBlock,
        area: Rect,
        valign: VAlign,
        color: Color,
        shadow: Option<TextShadow>,
        s: f32,
        fx: Fx,
    ) {
        if block.glyphs.is_empty() || block.is_empty() {
            return;
        }
        let (_, th) = block.size;
        let dy = match valign {
            VAlign::Top => 0.0,
            VAlign::Middle => (area.h - th) * 0.5,
            VAlign::Bottom => area.h - th,
        };
        let ox = area.x.round();
        let oy = (area.y + dy).round();
        let bounds = Rect::new(ox, oy, block.size.0.max(area.w), th);
        if self.p.culled(&bounds.expand(8.0 * s)) {
            return;
        }
        self.glyphs(block, ox, oy, color, shadow, s, fx);
    }

    #[allow(clippy::too_many_arguments)]
    fn glyphs(
        &mut self,
        block: &TextBlock,
        ox: f32,
        oy: f32,
        color: Color,
        shadow: Option<TextShadow>,
        s: f32,
        fx: Fx,
    ) {
        let inv = 1.0 / self.atlas;
        if let Some(sh) = shadow
            && !sh.color.is_transparent()
        {
            let blur = (sh.blur * s).min(4.0);
            let (dx, dy) = ((sh.x * s).round(), (sh.y * s).round());
            for g in &block.glyphs {
                let rect = Rect::new(
                    ox + g.x + dx - blur,
                    oy + g.y + dy - blur,
                    g.w + blur * 2.0,
                    g.h + blur * 2.0,
                );
                let mut q = Quad::new(QuadKind::Glyph, rect, sh.color);
                q.color = fx.color(sh.color);
                let [ax, ay, aw, ah] = g.atlas.map(|v| v as f32);
                q.uv = [
                    (ax - blur) * inv,
                    (ay - blur) * inv,
                    (ax + aw + blur) * inv,
                    (ay + ah + blur) * inv,
                ];
                q.soft = blur;
                self.p.push(q, None);
            }
        }
        for g in &block.glyphs {
            let c = g.color.unwrap_or(color);
            let rect = Rect::new(ox + g.x, oy + g.y, g.w, g.h);
            let mut q = Quad::new(QuadKind::Glyph, rect, c);
            q.color = fx.color(c);
            let [ax, ay, aw, ah] = g.atlas.map(|v| v as f32);
            q.uv = [ax * inv, ay * inv, (ax + aw) * inv, (ay + ah) * inv];
            q.shade = fx.brightness;
            self.p.push(q, None);
        }
    }

    fn part_style<'a>(&self, n: &'a Node, part: Part) -> Option<&'a Style> {
        n.spec.part(part)
    }

    /// Paints a widget part's box; `fallback` fills it when the theme gives no background.
    fn part_box(
        &mut self,
        n: &Node,
        part: Part,
        rect: Rect,
        state: State,
        fx: Fx,
        fallback: Option<Color>,
    ) {
        let s = n.scale;
        let (vis, has) = match self.part_style(n, part) {
            Some(st) => (st.visual_for(state).into_owned(), true),
            None => (Visual::default(), false),
        };
        let mut vis = vis;
        if vis.background.is_none() && (vis.border_image.is_none() || !has) {
            vis.background = fallback.map(Fill::Solid);
        }
        let fx = fx.apply(&vis);
        let r = match vis.radius {
            Some(_) => radius(vis.radius, &rect, s),
            None => (rect.w.min(rect.h) * 0.5).min(3.0 * s),
        };
        self.decorate(&vis, rect, s, r, fx, None);
    }

    fn bar(&mut self, n: &Node, v: &Visual, fx: Fx) {
        let s = n.scale;
        let inset = v.fill_inset.unwrap_or([0.0; 4]).map(|e| e * s);
        let area = n.rect.outset([-inset[0], -inset[1], -inset[2], -inset[3]]);
        let t = n.value.clamp(0.0, 1.0);
        if t <= 0.0 {
            return;
        }
        let shown = Rect::new(area.x, area.y, area.w * t, area.h);
        let fill_color = n.color.or(match &v.fill {
            Some(Fill::Solid(c)) => Some(*c),
            _ => None,
        });
        let r = v.fill_radius.unwrap_or(0.0) * s;
        if let Some(key) = v.fill_image
            && let Some(desc) = self.images.get(key.0 as usize).copied().flatten()
        {
            let luminosity = v.fill_mode == Some(FillMode::Luminosity);
            let tint = if luminosity {
                fill_color.unwrap_or(Color::WHITE)
            } else {
                Color::WHITE
            };
            let [u0, v0, u1, v1] = desc.uv;
            let mut q = Quad::new(QuadKind::Image, shown, tint);
            q.color = fx.color(tint);
            q.uv = [u0, v0, u0 + (u1 - u0) * t, v1];
            q.radius = r;
            // Luminosity blending keeps the texture's light and dark around the colour.
            q.shade = fx.brightness * if luminosity { 1.8 } else { 1.0 };
            if desc.premultiplied {
                q.flags |= FLAG_PREMULTIPLIED;
            }
            if desc.srgb_view {
                q.flags |= FLAG_LINEAR_TEXELS;
            }
            if luminosity || fx.grayscale {
                q.flags |= FLAG_GRAYSCALE;
            }
            self.p.push(q, Some(desc.texture));
            return;
        }
        let fill = match fill_color {
            Some(c) => Fill::Solid(c),
            None => v.fill.clone().unwrap_or(Fill::Solid(Color::hex(0x65ef62))),
        };
        let fake = Visual {
            background: Some(fill),
            ..Visual::default()
        };
        self.decorate(&fake, shown, s, r, fx, None);
    }

    fn slider(&mut self, index: usize, fx: Fx) {
        let n = self.tree.node(index);
        let s = n.scale;
        let c = n.content;
        let state = self.state(index);
        let span = (n.spec.max - n.spec.min).max(f32::EPSILON);
        let t = ((n.value - n.spec.min) / span).clamp(0.0, 1.0);
        let track_h = self
            .part_style(n, Part::Track)
            .and_then(|p| p.height_px)
            .unwrap_or(4.0)
            * s;
        let tw = self
            .part_style(n, Part::Thumb)
            .and_then(|p| p.width_px)
            .unwrap_or(12.0)
            * s;
        let th = self
            .part_style(n, Part::Thumb)
            .and_then(|p| p.height_px)
            .unwrap_or(16.0)
            * s;
        let cy = c.y + c.h * 0.5;
        let track = Rect::new(c.x, cy - track_h * 0.5, c.w, track_h);
        self.part_box(
            n,
            Part::Track,
            track,
            state,
            fx,
            Some(Color::rgba(255, 255, 255, 30)),
        );
        let filled = Rect::new(track.x, track.y, track.w * t, track.h);
        if filled.w > 0.0 {
            self.part_box(n, Part::Fill, filled, state, fx, Some(Color::hex(0xa9bf88)));
        }
        let thumb = Rect::new(c.x + c.w * t - tw * 0.5, cy - th * 0.5, tw, th);
        self.part_box(n, Part::Thumb, thumb, state, fx, Some(Color::hex(0xe8e0d0)));
    }

    fn checkbox(&mut self, index: usize, fx: Fx) {
        let n = self.tree.node(index);
        let s = n.scale;
        let c = n.content;
        let state = self.state(index);
        let size = self
            .part_style(n, Part::Box)
            .and_then(|p| p.width_px)
            .unwrap_or(16.0)
            * s;
        let b = Rect::new(c.x, c.y + (c.h - size) * 0.5, size, size);
        self.part_box(
            n,
            Part::Box,
            b,
            state,
            fx,
            Some(Color::rgba(255, 255, 255, 15)),
        );
        if n.checked {
            let inset = self
                .part_style(n, Part::Mark)
                .and_then(|p| p.width_px)
                .map_or(4.0 * s, |w| (size - w * s) * 0.5);
            self.part_box(
                n,
                Part::Mark,
                b.expand(-inset),
                state,
                fx,
                Some(Color::hex(0xd8c58c)),
            );
        }
    }

    fn field(&mut self, index: usize, fx: Fx, color: Color) {
        let n = self.tree.node(index);
        let s = n.scale;
        let c = n.content;
        let state = self.state(index);
        let Some(block) = &n.text else {
            return;
        };
        let clip = match self.p.clip() {
            Some(old) => old.intersect(&c.expand(1.0)),
            None => c.expand(1.0),
        };
        let restore = self.p.clip();
        self.p.set_clip(Some(clip));
        let lh = n.text_style.line_height_px() * s;
        let oy = (c.y + (c.h - lh) * 0.5).round();
        let ox = (c.x - n.field.scroll).round();
        let focused = self.input.focus == Some(index);
        if block.is_empty() && n.field.preedit.is_empty() {
            if let Some(ph) = &n.placeholder {
                let pc = self
                    .part_style(n, Part::Placeholder)
                    .and_then(|p| p.text.color)
                    .unwrap_or(color.with_alpha(0.45));
                self.glyphs(ph, ox, oy, pc, None, s, fx);
            }
        } else {
            let (a, b) = (
                n.field.anchor.min(n.field.cursor),
                n.field.anchor.max(n.field.cursor),
            );
            if focused && a != b {
                let x0 = block.caret_x(a);
                let x1 = block.caret_x(b);
                let sel = Rect::new(ox + x0, oy, x1 - x0, lh);
                self.part_box(
                    n,
                    Part::Selection,
                    sel,
                    state,
                    fx,
                    Some(Color::rgba(216, 197, 140, 90)),
                );
            }
            self.glyphs(block, ox, oy, color, n.text_style.shadow, s, fx);
            if !n.field.preedit.is_empty() {
                let x0 = block.caret_x(n.field.cursor);
                let x1 = block.caret_x(n.field.cursor + n.field.preedit.len());
                let mut q = Quad::new(
                    QuadKind::Shape,
                    Rect::new(ox + x0, oy + lh - s, x1 - x0, s),
                    color,
                );
                q.color = fx.color(color);
                self.p.push(q, None);
            }
        }
        if focused && self.input.caret_visible(self.time) {
            let at = n.field.cursor + n.field.preedit.len();
            let x = if block.is_empty() {
                0.0
            } else {
                block.caret_x(at)
            };
            let w = self
                .part_style(n, Part::Caret)
                .and_then(|p| p.width_px)
                .unwrap_or(1.0)
                * s;
            let caret = Rect::new((ox + x).round(), oy, w.max(1.0), lh);
            self.part_box(n, Part::Caret, caret, state, fx, Some(color));
        }
        self.p.set_clip(restore);
    }

    fn dropdown(&mut self, index: usize, fx: Fx, color: Color, deferred: &mut Vec<Deferred>) {
        let n = self.tree.node(index);
        let s = n.scale;
        let c = n.content;
        let w = self
            .part_style(n, Part::Arrow)
            .and_then(|p| p.width_px)
            .unwrap_or(8.0)
            * s;
        let arrow_color = self
            .part_style(n, Part::Arrow)
            .and_then(|p| p.text.color)
            .unwrap_or(color);
        // A small downward triangle drawn as rows of whole pixels.
        let rows = (w * 0.5).round().max(2.0) as i32;
        let x0 = c.right() - w;
        let y0 = (c.y + (c.h - rows as f32) * 0.5).round();
        for i in 0..rows {
            let rw = w - 2.0 * i as f32;
            if rw <= 0.0 {
                break;
            }
            let mut q = Quad::new(
                QuadKind::Shape,
                Rect::new(x0 + i as f32, y0 + i as f32, rw, 1.0),
                arrow_color,
            );
            q.color = fx.color(arrow_color);
            self.p.push(q, None);
        }
        if n.open {
            deferred.push(Deferred::Popup(index, fx));
        }
    }

    fn popup(&mut self, index: usize, fx: Fx) {
        let n = self.tree.node(index);
        let s = n.scale;
        self.p.set_clip(None);
        let (rect, row) = popup_rows(n);
        self.part_box(
            n,
            Part::Popup,
            rect,
            State(0),
            fx,
            Some(Color::hex(0x101a1c)),
        );
        self.hits.push(Hit {
            rect,
            clip: None,
            node: index,
            part: HitPart::Node,
        });
        let pad = 4.0 * s;
        for (i, block) in n.option_text.iter().enumerate() {
            let r = Rect::new(
                rect.x + pad,
                rect.y + pad + row * i as f32,
                rect.w - pad * 2.0,
                row,
            );
            let hover = self.input.hover_part == Some((index, HitPart::Option(i)));
            let mut st = 0;
            if hover {
                st |= State::HOVER;
            }
            if i == n.selected {
                st |= State::CHECKED;
            }
            let state = State(st);
            let option = self.part_style(n, Part::Option);
            let text_color = option
                .and_then(|o| o.text_color_for(state).or(o.text.color))
                .unwrap_or(n.text_style.color);
            self.part_box(n, Part::Option, r, state, fx, None);
            let area = Rect::new(r.x + 8.0 * s, r.y, r.w - 16.0 * s, r.h);
            self.text(block, area, VAlign::Middle, text_color, None, s, fx);
            self.hits.push(Hit {
                rect: r,
                clip: None,
                node: index,
                part: HitPart::Option(i),
            });
        }
    }
}
