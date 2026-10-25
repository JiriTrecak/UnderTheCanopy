//! The editor's look: Tailwind's zinc palette as the old spell studio used it
//! (`tooling/spell-editor/src/theme.css` in the old repository), applied to egui's style
//! and to egui_dock's tabs. Colours are sRGB hex values; the editor draws into a non-sRGB
//! view of the surface, so they reach the screen unchanged.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The page behind everything and the darkest wells (inputs, segmented controls).
pub const BG: Color32 = hex(0x09090b);
/// The open document's area.
pub const MAIN: Color32 = hex(0x101012);
/// The assistant and the timeline.
pub const SUNK: Color32 = hex(0x111113);
/// Side panels (library, inspector).
pub const PANEL: Color32 = hex(0x131315);
/// The header, cards and resting buttons.
pub const RAISED: Color32 = hex(0x18181b);
/// The user's messages in the assistant.
pub const BUBBLE: Color32 = hex(0x202023);
/// Selected rows, hovered buttons.
pub const SELECTED: Color32 = hex(0x27272a);
pub const BORDER: Color32 = hex(0x303034);
/// The composer's border and pressed controls.
pub const STRONG: Color32 = hex(0x3f3f46);
pub const CONTROL: Color32 = hex(0x52525b);
pub const TEXT: Color32 = hex(0xfafafa);
pub const BODY: Color32 = hex(0xe4e4e7);
pub const SOFT: Color32 = hex(0xd4d4d8);
pub const MUTED: Color32 = hex(0xa1a1aa);
pub const FAINT: Color32 = hex(0x71717a);
/// Unsaved edits.
pub const DIRTY: Color32 = hex(0xfbbf24);
/// Errors and refusals.
pub const ERROR: Color32 = hex(0xf87171);
/// A running service (MCP).
pub const LIVE: Color32 = hex(0x22c55e);

/// Header and assistant heading height.
pub const HEADER: f32 = 56.0;
pub const RAIL: f32 = 64.0;
pub const LIBRARY: f32 = 224.0;
pub const INSPECTOR: f32 = 320.0;
pub const ASSISTANT: f32 = 370.0;

/// Button and input corners.
pub const RADIUS: u8 = 6;

fn widget(
    fill: Color32,
    weak: Color32,
    border: Color32,
    text: Color32,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: weak,
        bg_stroke: Stroke::new(1.0, border),
        corner_radius: CornerRadius::same(RADIUS),
        fg_stroke: Stroke::new(1.0, text),
        expansion: 0.0,
    }
}

/// Sets the zinc style and the fonts: `fonts` go first in the proportional family (the
/// game passes its own; egui's stay as fallbacks).
pub fn apply(ctx: &egui::Context, fonts: &[Vec<u8>]) {
    let mut definitions = egui::FontDefinitions::default();
    for (i, data) in fonts.iter().enumerate() {
        let name = format!("editor-{i}");
        definitions.font_data.insert(
            name.clone(),
            std::sync::Arc::new(egui::FontData::from_owned(data.clone())),
        );
        definitions
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(i, name);
    }
    // Claude's text and the tool rows use arrows and marks the interface font and egui's
    // proportional fallbacks lack (→, ✓, ✕); egui's monospace font has them.
    if definitions.font_data.contains_key("Hack") {
        definitions
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .push("Hack".into());
    }
    ctx.set_fonts(definitions);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(style);
}

fn style(style: &mut egui::Style) {
    use FontFamily::{Monospace, Proportional};
    style.text_styles = [
        (TextStyle::Small, FontId::new(10.0, Proportional)),
        (TextStyle::Body, FontId::new(12.0, Proportional)),
        (TextStyle::Button, FontId::new(11.5, Proportional)),
        (TextStyle::Heading, FontId::new(13.0, Proportional)),
        (TextStyle::Monospace, FontId::new(11.0, Monospace)),
    ]
    .into();
    style.animation_time = 0.1;

    let s = &mut style.spacing;
    s.item_spacing = Vec2::new(8.0, 6.0);
    s.button_padding = Vec2::new(10.0, 5.0);
    s.interact_size = Vec2::new(28.0, 28.0);
    s.window_margin = Margin::same(12);
    s.menu_margin = Margin::same(6);
    s.indent = 14.0;
    s.icon_width = 14.0;
    s.icon_width_inner = 8.0;
    s.combo_height = 240.0;
    s.text_edit_width = 200.0;
    s.scroll = egui::style::ScrollStyle::thin();

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = None;
    v.weak_text_color = Some(MUTED);
    v.hyperlink_color = SOFT;
    v.faint_bg_color = RAISED;
    v.extreme_bg_color = BG;
    v.text_edit_bg_color = Some(BG);
    v.code_bg_color = RAISED;
    v.warn_fg_color = DIRTY;
    v.error_fg_color = ERROR;
    v.panel_fill = MAIN;
    v.window_fill = RAISED;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(7);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    v.selection.bg_fill = STRONG;
    v.selection.stroke = Stroke::new(1.0, TEXT);
    v.text_cursor.stroke = Stroke::new(1.5, TEXT);
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    v.disabled_alpha = 0.45;
    v.widgets.noninteractive = widget(MAIN, RAISED, BORDER, BODY);
    v.widgets.inactive = widget(SELECTED, RAISED, BORDER, BODY);
    v.widgets.hovered = widget(STRONG, SELECTED, CONTROL, TEXT);
    v.widgets.active = widget(CONTROL, STRONG, MUTED, TEXT);
    v.widgets.open = widget(SELECTED, SELECTED, BORDER, TEXT);
}

/// egui_dock's tabs in the same palette: a dark strip, the open tab merged with the
/// document below it.
pub fn dock(egui_style: &egui::Style) -> egui_dock::Style {
    let mut d = egui_dock::Style::from_egui(egui_style);
    d.dock_area_padding = None;
    d.main_surface_border_stroke = Stroke::NONE;
    d.main_surface_border_rounding = CornerRadius::ZERO;
    d.tab_bar.bg_fill = BG;
    d.tab_bar.height = 33.0;
    d.tab_bar.hline_color = BORDER;
    d.tab_bar.corner_radius = CornerRadius::ZERO;
    d.tab_bar.inner_margin = Margin::ZERO;
    let tab = |fill, text| egui_dock::TabInteractionStyle {
        outline_color: BORDER,
        corner_radius: CornerRadius::ZERO,
        bg_fill: fill,
        text_color: text,
    };
    d.tab.active = tab(MAIN, TEXT);
    d.tab.focused = tab(MAIN, TEXT);
    d.tab.active_with_kb_focus = tab(MAIN, TEXT);
    d.tab.focused_with_kb_focus = tab(MAIN, TEXT);
    d.tab.inactive = tab(BG, MUTED);
    d.tab.inactive_with_kb_focus = tab(BG, MUTED);
    d.tab.hovered = tab(RAISED, TEXT);
    d.tab.tab_body.bg_fill = MAIN;
    d.tab.tab_body.stroke = Stroke::NONE;
    d.tab.tab_body.corner_radius = CornerRadius::ZERO;
    d.tab.tab_body.inner_margin = Margin::same(12);
    d.tab.hline_below_active_tab_name = false;
    d.tab.spacing = 0.0;
    d.tab.minimum_width = Some(96.0);
    d.buttons.close_tab_color = FAINT;
    d.buttons.close_tab_active_color = TEXT;
    d.buttons.close_tab_bg_fill = SELECTED;
    d.separator.color_idle = BORDER;
    d.separator.color_hovered = CONTROL;
    d.separator.color_dragged = MUTED;
    d.separator.width = 1.0;
    d.overlay.selection_color = TEXT.gamma_multiply(0.12);
    d.overlay.button_color = SELECTED;
    d.overlay.button_border_stroke = Stroke::new(1.0, CONTROL);
    d
}
