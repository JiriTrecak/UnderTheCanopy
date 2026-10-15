//! Layout: taffy with text measurement, then absolute geometry in physical pixels.

use crate::config::Kind;
use crate::geom::{Rect, Vec2, Xform};
use crate::node::{Node, Part};
use crate::text::TextSystem;
use crate::{ImageDesc, Ui};
use taffy::prelude::*;

impl Ui {
    pub(crate) fn layout(&mut self) {
        let _span = tracing::trace_span!("ui layout").entered();
        let Some(root) = self.tree.root else {
            return;
        };
        let logical = Vec2::new(
            self.viewport[0] as f32 / self.dpi,
            self.viewport[1] as f32 / self.dpi,
        );
        if self.tree.viewport_changed {
            self.tree.viewport_changed = false;
            let t = self.tree.node(root).taffy;
            let mut style = self.tree.node(root).spec.style.layout.clone();
            style.size = Size {
                width: Dimension::length(logical.x),
                height: Dimension::length(logical.y),
            };
            self.tree.taffy.set_style(t, style).expect("root style");
            self.text_scales(root, self.dpi);
            self.tree.layout_dirty = true;
        }
        if self.tree.layout_dirty {
            self.tree.layout_dirty = false;
            self.stats.layout_runs += 1;
            let crate::node::Tree { taffy, nodes, .. } = &mut self.tree;
            let text = &mut self.text;
            let images = &self.image_table;
            let root_t = nodes[root].as_ref().map(|n| n.taffy).expect("root");
            let available = Size {
                width: AvailableSpace::Definite(logical.x),
                height: AvailableSpace::Definite(logical.y),
            };
            let _ =
                taffy.compute_layout_with_measure(root_t, available, |inputs, _id, ctx, style| {
                    let Some(&mut index) = ctx else {
                        return taffy::LayoutOutput::HIDDEN;
                    };
                    let Some(node) = nodes.get_mut(index).and_then(Option::as_mut) else {
                        return taffy::LayoutOutput::HIDDEN;
                    };
                    taffy::compute_leaf_layout(
                        inputs,
                        style,
                        |_, _| 0.0,
                        |known, available| measure(node, text, images, known, available),
                    )
                });
        }
        loop {
            self.text.reset_during_frame = false;
            self.place(root, Vec2::ZERO, Xform::scaling(self.dpi), None, Vec2::ZERO);
            if !self.text.reset_during_frame {
                break;
            }
        }
    }

    /// Display scale of every node's text: DPI × transforms of ancestors and itself.
    fn text_scales(&mut self, index: usize, parent: f32) {
        let node = self.tree.node_mut(index);
        let own = node
            .scale_override
            .or(node.spec.style.scale.map(|s| s.0))
            .unwrap_or(1.0);
        let scale = parent * own;
        if node.text_scale != scale {
            node.text_scale = scale;
            if node.text.is_some() {
                let t = node.taffy;
                let _ = self.tree.taffy.mark_dirty(t);
            }
        }
        let children = self.tree.node(index).children.clone();
        for c in children {
            self.text_scales(c, scale);
        }
    }

    /// Computes absolute geometry of a subtree and prepares its text.
    fn place(
        &mut self,
        index: usize,
        origin: Vec2,
        xf: Xform,
        clip: Option<Rect>,
        parent_scroll: Vec2,
    ) {
        let (visible, taffy_id) = {
            let n = self.tree.node(index);
            (n.visible, n.taffy)
        };
        if !visible {
            return;
        }
        if Some(index) == self.tree.tooltip_root {
            self.position_tooltip(index);
        }
        let layout = *self.tree.taffy.layout(taffy_id).expect("layout");
        let node = self.tree.node(index);
        let local = Rect::new(
            origin.x + layout.location.x + node.offset.x + node.shift.x - parent_scroll.x,
            origin.y + layout.location.y + node.offset.y + node.shift.y - parent_scroll.y,
            layout.size.width,
            layout.size.height,
        );
        let scale = node
            .scale_override
            .map(|s| (s, node.spec.style.scale.map_or([0.5, 0.5], |s| s.1)))
            .or(node.spec.style.scale);
        let xf = match scale {
            Some((s, o)) => xf.then_scale_about(
                s,
                Vec2::new(local.x + o[0] * local.w, local.y + o[1] * local.h),
            ),
            None => xf,
        };
        let content_local = Rect::new(
            local.x + layout.padding.left + layout.border.left,
            local.y + layout.padding.top + layout.border.top,
            (local.w
                - layout.padding.left
                - layout.padding.right
                - layout.border.left
                - layout.border.right)
                .max(0.0),
            (local.h
                - layout.padding.top
                - layout.padding.bottom
                - layout.border.top
                - layout.border.bottom)
                .max(0.0),
        );
        let rect = xf.rect(local);
        let content = xf.rect(content_local);
        let style_clip = node.spec.style.clip;
        let scrolls = node.spec.style.scroll;
        let children = node.children.clone();
        let mut scroll = node.scroll;
        if scrolls {
            // The scrollable extent: the farthest child edge plus padding.
            let mut extent = Vec2::ZERO;
            for &c in &children {
                let cn = self.tree.node(c);
                if !cn.visible {
                    continue;
                }
                let l = self.tree.taffy.layout(cn.taffy).expect("layout");
                extent.x = extent.x.max(l.location.x + l.size.width);
                extent.y = extent.y.max(l.location.y + l.size.height);
            }
            let max = Vec2::new(
                (extent.x + layout.padding.right + layout.border.right - layout.size.width)
                    .max(0.0),
                (extent.y + layout.padding.bottom + layout.border.bottom - layout.size.height)
                    .max(0.0),
            );
            scroll = Vec2::new(scroll.x.clamp(0.0, max.x), scroll.y.clamp(0.0, max.y));
            let n = self.tree.node_mut(index);
            n.scroll_max = max;
            n.scroll = scroll;
        }
        {
            let n = self.tree.node_mut(index);
            n.rect = rect;
            n.content = content;
            n.clip = clip;
            n.scale = xf.scale;
        }
        self.prepare_text(index);
        let child_clip = if style_clip {
            let border = xf.rect(Rect::new(
                local.x + layout.border.left,
                local.y + layout.border.top,
                local.w - layout.border.left - layout.border.right,
                local.h - layout.border.top - layout.border.bottom,
            ));
            Some(clip.map_or(border, |c| c.intersect(&border)))
        } else {
            clip
        };
        let child_scroll = if scrolls { scroll } else { Vec2::ZERO };
        for c in children {
            self.place(c, Vec2::new(local.x, local.y), xf, child_clip, child_scroll);
        }
    }

    fn prepare_text(&mut self, index: usize) {
        let text = &mut self.text;
        let node = self.tree.nodes[index].as_mut().expect("live node");
        let width = text_area(node).map(|r| r.w);
        let scale = node.text_scale;
        let field = node.kind() == Kind::TextField;
        if let Some(block) = &mut node.text {
            if !block.is_empty() {
                block.prepare(text, scale, if field { None } else { width });
            } else if let Some(placeholder) = &mut node.placeholder {
                placeholder.prepare(text, scale, None);
            }
            if field {
                // Keep the caret inside the field.
                let at = node.field.cursor + node.field.preedit.len();
                let x = if block.is_empty() {
                    0.0
                } else {
                    block.caret_x(at)
                };
                let w = node.content.w;
                let f = &mut node.field;
                if x - f.scroll > w - 2.0 {
                    f.scroll = x - w + 2.0;
                }
                if x < f.scroll {
                    f.scroll = x;
                }
                f.scroll = f.scroll.min((block.size.0 - w + 2.0).max(0.0)).max(0.0);
            }
        }
        if node.open && node.kind() == Kind::Dropdown {
            if node.option_text.len() != node.options.len() {
                node.option_text = node
                    .options
                    .iter()
                    .map(|o| crate::text::TextBlock::new(o, node.text_style.clone()))
                    .collect();
            }
            let (popup, _) = crate::paint::popup_rows(node);
            let w = popup.w - 24.0 * node.scale;
            for (block, option) in node.option_text.iter_mut().zip(&node.options) {
                block.set_markup(option);
                block.prepare(text, scale, Some(w));
            }
        }
    }
}

/// The rectangle a node's main text occupies (physical pixels).
pub(crate) fn text_area(node: &Node) -> Option<Rect> {
    let c = node.content;
    let s = node.scale;
    Some(match node.kind() {
        Kind::Checkbox => {
            let size = node
                .spec
                .part(Part::Box)
                .and_then(|p| p.width_px)
                .unwrap_or(16.0)
                * s;
            let gap = node.spec.style.gap_px.max(8.0) * s;
            Rect::new(c.x + size + gap, c.y, (c.w - size - gap).max(0.0), c.h)
        }
        Kind::Dropdown => {
            let arrow = node
                .spec
                .part(Part::Arrow)
                .and_then(|p| p.width_px)
                .unwrap_or(8.0)
                * s;
            let gap = node.spec.style.gap_px.max(6.0) * s;
            Rect::new(c.x, c.y, (c.w - arrow - gap).max(0.0), c.h)
        }
        _ => c,
    })
}

fn measure(
    node: &mut Node,
    text: &mut TextSystem,
    images: &[Option<ImageDesc>],
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
) -> Size<f32> {
    if let (Some(width), Some(height)) = (known.width, known.height) {
        return Size { width, height };
    }
    let scale = node.text_scale;
    let width_limit = known.width.or(match available.width {
        AvailableSpace::Definite(w) => Some(w),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    });
    let mut text_size = |extra: f32, limit: Option<f32>| -> (f32, f32) {
        match &mut node.text {
            Some(t) if !t.is_empty() => t.measure(text, scale, limit.map(|l| (l - extra).max(0.0))),
            Some(t) => (0.0, t.style.line_height_px()),
            None => (0.0, 0.0),
        }
    };
    let (w, h) = match node.spec.kind {
        Kind::Image => {
            let size = node
                .image
                .and_then(|k| images.get(k.0 as usize).copied().flatten())
                .map_or([0.0, 0.0], |d| d.size);
            match (known.width, known.height) {
                (Some(w), None) if size[0] > 0.0 => (w, w * size[1] / size[0]),
                (None, Some(h)) if size[1] > 0.0 => (h * size[0] / size[1], h),
                _ => (size[0], size[1]),
            }
        }
        Kind::Checkbox => {
            let b = node
                .spec
                .part(Part::Box)
                .and_then(|p| p.width_px)
                .unwrap_or(16.0);
            let gap = node.spec.style.gap_px.max(8.0);
            let (tw, th) = text_size(b + gap, width_limit);
            (b + gap + tw, th.max(b))
        }
        Kind::Slider => {
            let h = node
                .spec
                .part(Part::Thumb)
                .and_then(|p| p.height_px)
                .unwrap_or(16.0);
            (120.0, h)
        }
        Kind::TextField => {
            let lh = node.text_style.line_height_px();
            (160.0, lh)
        }
        Kind::Dropdown => {
            let arrow = node
                .spec
                .part(Part::Arrow)
                .and_then(|p| p.width_px)
                .unwrap_or(8.0);
            let gap = node.spec.style.gap_px.max(6.0);
            // As wide as the widest option, like a native select.
            let mut widest: f32 = 0.0;
            let mut height: f32 = node.text_style.line_height_px();
            if let Some(t) = &mut node.text {
                let current = t.markup().to_owned();
                for option in &node.options {
                    t.set_markup(option);
                    let (w, h) = t.measure(text, scale, None);
                    widest = widest.max(w);
                    height = height.max(h);
                }
                t.set_markup(&current);
            }
            (widest + gap + arrow, height)
        }
        _ => text_size(0.0, width_limit),
    };
    Size {
        width: known.width.unwrap_or(w),
        height: known.height.unwrap_or(h),
    }
}
