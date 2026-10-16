//! The editor's small widgets in the zinc look: rail items, icon buttons, chips, library
//! rows and labels.

use crate::icons::{self, Icon};
use crate::theme::*;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

/// A library row: a folder (`folder` = whether it is open) or an element, marked when it
/// has unsaved changes.
pub fn row(
    ui: &mut Ui,
    depth: usize,
    label: &str,
    count: Option<usize>,
    folder: Option<bool>,
    selected: bool,
    dirty: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        SELECTED
    } else if response.hovered() {
        RAISED
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 5, fill);
    let mut x = rect.left() + 8.0 + depth as f32 * 14.0;
    let y = rect.center().y;
    match folder {
        Some(open) => {
            let icon = if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            icons::paint(painter, icons::square(pos2(x + 6.0, y), 12.0), icon, MUTED);
        }
        None => {
            let color = if selected { SOFT } else { FAINT };
            icons::paint(
                painter,
                icons::square(pos2(x + 6.0, y), 13.0),
                Icon::File,
                color,
            );
        }
    }
    x += 18.0;
    let color = if selected { TEXT } else { SOFT };
    let right = match count {
        Some(n) => {
            let r = painter.text(
                pos2(rect.right() - 8.0, y),
                Align2::RIGHT_CENTER,
                n.to_string(),
                FontId::proportional(11.0),
                FAINT,
            );
            r.left() - 6.0
        }
        None if dirty => {
            painter.circle_filled(pos2(rect.right() - 11.0, y), 3.0, DIRTY);
            rect.right() - 20.0
        }
        None => rect.right() - 6.0,
    };
    let galley = painter.layout_no_wrap(label.to_string(), FontId::proportional(12.0), color);
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
    painter
        .with_clip_rect(clip.intersect(painter.clip_rect()))
        .galley(pos2(x, y - galley.size().y / 2.0), galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A labelled read-only value in an input-like well.
pub fn field(ui: &mut Ui, label: &str, value: &str, mono: bool) {
    ui.label(RichText::new(label).color(MUTED).size(11.0));
    Frame::new()
        .fill(BG)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let text = RichText::new(value).color(TEXT);
            ui.add(egui::Label::new(if mono { text.monospace() } else { text }).truncate());
        });
}

/// A centred title and explanation for an empty area.
pub fn empty(ui: &mut Ui, title: &str, body: &str) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space((ui.available_height() / 2.0 - 40.0).max(24.0));
        ui.set_max_width(ui.available_width().min(320.0));
        ui.label(RichText::new(title).color(SOFT).size(13.0));
        ui.add_space(2.0);
        ui.label(RichText::new(body).color(FAINT));
    });
}

/// An uppercase spaced label (the spell studio's section titles).
pub fn eyebrow(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .size(9.0)
        .color(FAINT)
        .extra_letter_spacing(1.1)
}

pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(RAISED))
        .fill(TEXT)
        .stroke(Stroke::NONE)
}

/// A service's state: a coloured dot and a word, as a button.
pub fn status(ui: &mut Ui, text: &str, dot: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(11.5), MUTED);
    let size = vec2(32.0 + galley.size().x, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        RADIUS,
        if response.hovered() { SELECTED } else { RAISED },
    );
    painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    painter.circle_filled(pos2(rect.left() + 13.0, rect.center().y), 3.0, dot);
    let top = rect.center().y - galley.size().y / 2.0;
    painter.galley(pos2(rect.left() + 22.0, top), galley, MUTED);
    response
}

/// The package chip: a dot, the package and a badge.
pub fn chip(ui: &mut Ui, name: &str, badge: &str) -> Response {
    let painter = ui.painter();
    let name = painter.layout_no_wrap(name.to_string(), FontId::proportional(11.5), BODY);
    let badge = painter.layout_no_wrap(badge.to_string(), FontId::proportional(10.0), SOFT);
    let badge_width = badge.size().x + 14.0;
    let size = vec2(22.0 + name.size().x + 8.0 + badge_width + 4.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    painter.rect_filled(rect, RADIUS, RAISED);
    painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    painter.circle_filled(pos2(rect.left() + 13.0, y), 3.0, TEXT);
    let x = rect.left() + 22.0;
    let name_width = name.size().x;
    painter.galley(pos2(x, y - name.size().y / 2.0), name, BODY);
    let pill = Rect::from_min_size(
        pos2(x + name_width + 8.0, y - 10.0),
        vec2(badge_width, 20.0),
    );
    painter.rect_filled(pill, 5, SELECTED);
    painter.galley(
        pos2(pill.left() + 7.0, y - badge.size().y / 2.0),
        badge,
        SOFT,
    );
    response
}

/// A 28-point square button with an icon.
pub fn icon_button(ui: &mut Ui, icon: Icon, on: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), sense);
    let painter = ui.painter();
    let hovered = enabled && response.hovered();
    if on || hovered {
        painter.rect_filled(rect, RADIUS, SELECTED);
    }
    if on {
        painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, CONTROL), StrokeKind::Inside);
    }
    let color = match (enabled, on || hovered) {
        (false, _) => CONTROL,
        (true, true) => TEXT,
        (true, false) => MUTED,
    };
    icons::paint(painter, icons::square(rect.center(), 15.0), icon, color);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// An editor's button on the rail: icon over a short label.
pub fn rail_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(52.0, 48.0), Sense::click());
    let painter = ui.painter();
    let hovered = response.hovered();
    let fill = if selected {
        SELECTED
    } else if hovered {
        RAISED
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, CornerRadius::same(8), fill);
    let color = if selected {
        TEXT
    } else if hovered {
        SOFT
    } else {
        FAINT
    };
    icons::paint(
        painter,
        icons::square(pos2(rect.center().x, rect.top() + 18.0), 19.0),
        icon,
        color,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.5),
        color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small label on a dark pill, after an optional coloured dot.
pub fn badge(ui: &mut Ui, text: &str, dot: Option<Color32>) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(10.5), SOFT);
    let inset = if dot.is_some() { 19.0 } else { 8.0 };
    let size = vec2(inset + galley.size().x + 8.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 5, SELECTED);
    if let Some(color) = dot {
        painter.circle_filled(pos2(rect.left() + 10.0, rect.center().y), 3.0, color);
    }
    let top = rect.center().y - galley.size().y / 2.0;
    painter.galley(pos2(rect.left() + inset, top), galley, SOFT);
    response
}

/// A tab title marked as unsaved.
pub fn dirty_title(name: &str) -> egui::WidgetText {
    let mut job = egui::text::LayoutJob::default();
    let font = FontId::proportional(12.0);
    job.append("• ", 0.0, egui::TextFormat::simple(font.clone(), DIRTY));
    job.append(name, 0.0, egui::TextFormat::simple(font, TEXT));
    job.into()
}
