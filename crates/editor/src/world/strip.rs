//! The block editor's tool strip: a floating, bordered bar of tool icons at the foot of
//! the view, and above it a second strip with the chosen tool's settings (its brush, the
//! kinds and faces it lays down, what Place puts down, what Select holds). Kinds, faces and
//! placements are pickers: a button with the current choice's picture, opening a grid of
//! every choice's picture above it.

use super::catalog::Entry;
use super::palette::{Choice, Palette};
use super::{BlockTool, Placing, View, mesh};
use crate::icons::{self, Icon};
use crate::map::Session;
use crate::map::tools::Picked;
use crate::theme::*;
use canopy_content::map::blocks::{Block, BlockKind};
use egui::{
    Align2, Color32, FontId, Id, Order, Popup, PopupCloseBehavior, Rect, RectAlign, RichText,
    Sense, Stroke, Ui, pos2, vec2,
};

/// The tools in their groups: shaping the blocks, their surface, things on the map.
const GROUPS: [&[BlockTool]; 3] = [
    &[BlockTool::Select, BlockTool::Place],
    &[
        BlockTool::Raise,
        BlockTool::Level,
        BlockTool::Paint,
        BlockTool::Ramp,
        BlockTool::Cut,
        BlockTool::Round,
    ],
    &[BlockTool::Cells, BlockTool::Face],
];

fn icon(tool: BlockTool) -> Icon {
    match tool {
        BlockTool::Raise => Icon::Raise,
        BlockTool::Level => Icon::Level,
        BlockTool::Paint => Icon::Paint,
        BlockTool::Ramp => Icon::Ramp,
        BlockTool::Cut => Icon::Cut,
        BlockTool::Cells => Icon::Cells,
        BlockTool::Face => Icon::Face,
        BlockTool::Place => Icon::Place,
        BlockTool::Round => Icon::Round,
        BlockTool::Select => Icon::Select,
    }
}

/// A floating, bordered strip.
fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .shadow(egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(140),
        })
}

/// One tool's button: its icon, highlighted when chosen; its name, key and use on hover.
fn tool_button(ui: &mut Ui, tool: BlockTool, on: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    if on {
        painter.rect(
            rect,
            7,
            SELECTED,
            Stroke::new(1.0, SOFT),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, 7, RAISED);
    }
    let color = if on {
        TEXT
    } else if hovered {
        SOFT
    } else {
        MUTED
    };
    icons::paint(
        painter,
        icons::square(rect.center(), 20.0),
        icon(tool),
        color,
    );
    response
        .on_hover_text(format!(
            "{}   {:?}\n{}",
            tool.name(),
            tool.key(),
            tool.hint()
        ))
        .clicked()
}

/// A segment of a choice.
fn segment(ui: &mut Ui, on: bool, text: &str) -> bool {
    ui.selectable_label(on, RichText::new(text).size(12.0))
        .clicked()
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 26.0), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, BORDER));
}

/// A block kind's colour in the block view, for its swatch.
fn swatch(kind: BlockKind) -> Color32 {
    let c = mesh::color(Block {
        level: 2,
        kind,
        ..Block::default()
    });
    let s = |v: f32| (v.powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgb(s(c[0]), s(c[1]), s(c[2]))
}

/// One choice a picker offers.
struct Pick {
    choice: Choice,
    name: String,
    /// Its shortcut.
    key: Option<String>,
}

/// The kinds Paint and Cells lay down, with their number keys.
fn kind_picks() -> Vec<Pick> {
    BlockKind::ALL
        .iter()
        .enumerate()
        .filter(|(_, k)| **k != BlockKind::Ramp)
        .map(|(n, k)| Pick {
            choice: Choice::Kind(*k),
            name: k.name().to_string(),
            key: Some(format!("{}", n + 1)),
        })
        .collect()
}

/// `choice`'s picture in `rect`: its texture or portrait, else its colour in the block
/// view (water, or a biome without the texture).
fn picture(ui: &Ui, palette: &mut Palette, rect: Rect, choice: Choice) {
    let radius = (rect.width() * 0.12).clamp(3.0, 6.0) as u8;
    if let Some(texture) = palette.texture(ui.ctx(), choice) {
        egui::Image::from_texture((texture, rect.size()))
            .corner_radius(radius)
            .paint_at(ui, rect);
        return;
    }
    let painter = ui.painter();
    match choice {
        Choice::Kind(kind) => {
            painter.rect_filled(rect, radius, swatch(kind));
            if matches!(kind, BlockKind::Shallow | BlockKind::Deep) {
                // A few ripples.
                let light = Color32::from_white_alpha(46);
                for (i, (x, y)) in [(0.3, 0.32), (0.62, 0.55), (0.36, 0.76)].iter().enumerate() {
                    let c = rect.lerp_inside(vec2(*x, *y));
                    let half = rect.width() * (0.16 - i as f32 * 0.03);
                    painter.line_segment(
                        [c - vec2(half, 0.0), c + vec2(half, 0.0)],
                        Stroke::new((rect.width() / 40.0).max(1.0), light),
                    );
                }
            }
        }
        Choice::Face(_) => {
            painter.rect_filled(rect, radius, RAISED);
        }
    }
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_black_alpha(60)),
        egui::StrokeKind::Inside,
    );
}

/// A picker: a button with `current`'s picture and name, opening a grid of every choice
/// in `picks` above it. Returns the choice clicked.
fn picker(
    ui: &mut Ui,
    palette: &mut Palette,
    salt: &str,
    title: &str,
    open: bool,
    current: Choice,
    picks: &[Pick],
) -> Option<Choice> {
    let name = picks
        .iter()
        .find(|p| p.choice == current)
        .map_or("—", |p| p.name.as_str());
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_string(), FontId::proportional(12.5), TEXT);
    let size = vec2(5.0 + 24.0 + 8.0 + galley.size().x + 10.0 + 10.0 + 8.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let popup = Popup::default_response_id(&response);
    if open {
        Popup::open_id(ui.ctx(), popup);
    }
    let shown = Popup::is_id_open(ui.ctx(), popup);
    let hovered = response.hovered();
    ui.painter().rect(
        rect,
        7,
        if shown || hovered { RAISED } else { SELECTED },
        Stroke::new(1.0, if shown { SOFT } else { BORDER }),
        egui::StrokeKind::Inside,
    );
    let thumb = Rect::from_min_size(rect.min + vec2(5.0, 4.0), vec2(24.0, 24.0));
    picture(ui, palette, thumb, current);
    let painter = ui.painter();
    painter.galley(
        pos2(thumb.right() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    // A chevron pointing the way the grid opens.
    let c = pos2(rect.right() - 13.0, rect.center().y);
    let chevron = Stroke::new(1.4, if shown || hovered { TEXT } else { MUTED });
    painter.line_segment([c + vec2(-4.0, 2.0), c + vec2(0.0, -2.0)], chevron);
    painter.line_segment([c + vec2(0.0, -2.0), c + vec2(4.0, 2.0)], chevron);
    let response = response.on_hover_text(title);
    let mut chosen = None;
    Popup::from_toggle_button_response(&response)
        .align(RectAlign::TOP_START)
        .align_alternatives(&[RectAlign::TOP, RectAlign::TOP_END, RectAlign::BOTTOM_START])
        .gap(10.0)
        .frame(frame().inner_margin(10))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .id(popup)
        .show(|ui| {
            ui.label(RichText::new(title).size(11.5).color(MUTED));
            ui.add_space(6.0);
            let columns = if picks.len() > 6 {
                4
            } else {
                picks.len().clamp(1, 3)
            };
            egui::Grid::new((salt, "grid"))
                .spacing(vec2(6.0, 6.0))
                .show(ui, |ui| {
                    for (i, pick) in picks.iter().enumerate() {
                        if tile(ui, palette, pick, pick.choice == current) {
                            chosen = Some(pick.choice);
                            ui.close();
                        }
                        if i % columns == columns - 1 {
                            ui.end_row();
                        }
                    }
                });
        });
    chosen
}

/// One choice in a picker's grid: its picture, its name under it, its key in the corner.
fn tile(ui: &mut Ui, palette: &mut Palette, pick: &Pick, on: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(92.0, 112.0), Sense::click());
    let hovered = response.hovered();
    if on || hovered {
        ui.painter().rect(
            rect,
            8,
            if on { SELECTED } else { RAISED },
            Stroke::new(1.0, if on { SOFT } else { BORDER }),
            egui::StrokeKind::Inside,
        );
    }
    let image = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(80.0, 80.0));
    picture(ui, palette, image, pick.choice);
    let painter = ui.painter();
    if on {
        painter.rect_stroke(
            image.expand(1.5),
            7,
            Stroke::new(2.0, TEXT),
            egui::StrokeKind::Outside,
        );
    }
    if let Some(key) = &pick.key {
        let chip = Rect::from_min_size(image.min + vec2(4.0, 4.0), vec2(16.0, 16.0));
        painter.rect_filled(chip, 4, Color32::from_black_alpha(150));
        painter.text(
            chip.center(),
            Align2::CENTER_CENTER,
            key,
            FontId::monospace(10.5),
            TEXT,
        );
    }
    painter.text(
        pos2(rect.center().x, image.bottom() + 12.0),
        Align2::CENTER_CENTER,
        &pick.name,
        FontId::proportional(12.0),
        if on || hovered { TEXT } else { SOFT },
    );
    response.clicked()
}

/// How an area tool takes its blocks: a box, a brush stroke, or a lasso; and the brush's
/// size.
fn shape(ui: &mut Ui, session: &mut Session, cells: bool) {
    let state = &mut session.blocks;
    let options: &[(&str, bool, bool)] = if cells {
        &[("Stroke", true, false), ("Lasso", false, true)]
    } else {
        &[
            ("Box", false, false),
            ("Brush", true, false),
            ("Lasso", false, true),
        ]
    };
    for &(name, brush, lasso) in options {
        let on = state.lasso == lasso && (lasso || cells || state.brush == brush);
        if segment(ui, on, name) {
            state.brush = brush;
            state.lasso = lasso;
        }
    }
    if cells && !state.lasso {
        divider(ui);
        let mut r = state.cell_radius;
        ui.add(egui::Slider::new(&mut r, 1..=16).text("cells"));
        state.cell_radius = r;
    } else if state.brush && !state.lasso {
        divider(ui);
        let mut r = state.radius;
        ui.add(egui::Slider::new(&mut r, 0..=8).text("blocks"));
        state.radius = r;
    }
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(MUTED));
}

/// A catalog entry's picture in `rect`: its icon, or for what has none, a sign of it.
fn entry_picture(ui: &Ui, palette: &mut Palette, rect: Rect, entry: &Entry) {
    let radius = (rect.width() * 0.12).clamp(3.0, 6.0) as u8;
    if let Some(texture) = entry
        .icon
        .as_deref()
        .and_then(|e| palette.icon(ui.ctx(), e))
    {
        egui::Image::from_texture((texture, rect.size()))
            .corner_radius(radius)
            .paint_at(ui, rect);
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, radius, RAISED);
    let big = FontId::proportional(rect.height() * 0.42);
    match &entry.placing {
        Placing::Start(n) => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                n.to_string(),
                big,
                TEXT,
            );
        }
        Placing::Camp(tier) => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tier.name().to_uppercase(),
                FontId::proportional(rect.height() * 0.2),
                SOFT,
            );
        }
        Placing::Scenery(_) => icons::paint(
            painter,
            icons::square(rect.center(), rect.width() * 0.5),
            Icon::World,
            MUTED,
        ),
        _ => icons::paint(
            painter,
            icons::square(rect.center(), rect.width() * 0.5),
            Icon::Place,
            MUTED,
        ),
    }
}

/// One entry in the Place picker's grid: its picture, its name under it.
fn place_tile(ui: &mut Ui, palette: &mut Palette, entry: &Entry, on: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(84.0, 108.0), Sense::click());
    let hovered = response.hovered();
    if on || hovered {
        ui.painter().rect(
            rect,
            8,
            if on { SELECTED } else { RAISED },
            Stroke::new(1.0, if on { SOFT } else { BORDER }),
            egui::StrokeKind::Inside,
        );
    }
    let image = Rect::from_min_size(rect.min + vec2(8.0, 6.0), vec2(68.0, 68.0));
    entry_picture(ui, palette, image, entry);
    if on {
        ui.painter().rect_stroke(
            image.expand(1.5),
            7,
            Stroke::new(2.0, TEXT),
            egui::StrokeKind::Outside,
        );
    }
    let mut job = egui::text::LayoutJob::simple(
        entry.name.clone(),
        FontId::proportional(11.5),
        if on || hovered { TEXT } else { SOFT },
        rect.width() - 6.0,
    );
    job.wrap.max_rows = 2;
    job.halign = egui::Align::Center;
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos2(rect.center().x, image.bottom() + 5.0), galley, TEXT);
    response.on_hover_text(&entry.name).clicked()
}

/// A grid of entries; the one clicked.
fn place_grid(
    ui: &mut Ui,
    palette: &mut Palette,
    salt: impl std::hash::Hash + std::fmt::Debug,
    entries: &[&Entry],
    current: &Placing,
) -> Option<Placing> {
    let mut chosen = None;
    egui::Grid::new(salt)
        .spacing(vec2(4.0, 4.0))
        .show(ui, |ui| {
            for (i, entry) in entries.iter().enumerate() {
                if place_tile(ui, palette, entry, &entry.placing == current) {
                    chosen = Some(entry.placing.clone());
                }
                if i % 7 == 6 {
                    ui.end_row();
                }
            }
        });
    chosen
}

/// What Place puts down: a button with the current entry, opening the catalog above it:
/// a search over everything, the categories, their groups (races, kinds) and a grid of
/// what is in the group.
fn place_picker(ui: &mut Ui, session: &mut Session, open: bool) {
    let state = &mut session.blocks;
    let current = state.placing.clone();
    let entry = state.catalog.find(&current).cloned();
    let name = entry
        .as_ref()
        .map_or("Choose what to place", |e| e.name.as_str());
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_string(), FontId::proportional(12.5), TEXT);
    let size = vec2(5.0 + 24.0 + 8.0 + galley.size().x + 10.0 + 10.0 + 8.0, 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let popup = Popup::default_response_id(&response);
    if open {
        Popup::open_id(ui.ctx(), popup);
    }
    let shown = Popup::is_id_open(ui.ctx(), popup);
    let hovered = response.hovered();
    ui.painter().rect(
        rect,
        7,
        if shown || hovered { RAISED } else { SELECTED },
        Stroke::new(1.0, if shown { SOFT } else { BORDER }),
        egui::StrokeKind::Inside,
    );
    let thumb = Rect::from_min_size(rect.min + vec2(5.0, 4.0), vec2(24.0, 24.0));
    if let Some(entry) = &entry {
        entry_picture(ui, &mut state.palette, thumb, entry);
    }
    let painter = ui.painter();
    painter.galley(
        pos2(thumb.right() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
    let c = pos2(rect.right() - 13.0, rect.center().y);
    let chevron = Stroke::new(1.4, if shown || hovered { TEXT } else { MUTED });
    painter.line_segment([c + vec2(-4.0, 2.0), c + vec2(0.0, -2.0)], chevron);
    painter.line_segment([c + vec2(0.0, -2.0), c + vec2(4.0, 2.0)], chevron);
    let response = response.on_hover_text("What Place puts down");
    let mut chosen = None;
    Popup::from_toggle_button_response(&response)
        .align(RectAlign::TOP_START)
        .align_alternatives(&[RectAlign::TOP, RectAlign::TOP_END])
        .gap(10.0)
        .frame(frame().inner_margin(10))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .id(popup)
        .show(|ui| {
            let (catalog, palette) = (&state.catalog, &mut state.palette);
            ui.set_width(7.0 * 88.0);
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.add(
                egui::TextEdit::singleline(&mut state.place_search)
                    .hint_text("Search everything…")
                    .desired_width(f32::INFINITY),
            );
            let search = state.place_search.trim().to_lowercase();
            if search.is_empty() {
                state.place_category = state
                    .place_category
                    .min(catalog.categories.len().saturating_sub(1));
                ui.horizontal(|ui| {
                    for (i, category) in catalog.categories.iter().enumerate() {
                        if ui
                            .selectable_label(
                                state.place_category == i,
                                RichText::new(&category.name).size(13.0),
                            )
                            .clicked()
                        {
                            state.place_category = i;
                            state.place_group = 0;
                        }
                    }
                });
                let Some(category) = catalog.categories.get(state.place_category) else {
                    ui.label(RichText::new("Nothing to place yet").color(MUTED));
                    return;
                };
                state.place_group = state
                    .place_group
                    .min(category.groups.len().saturating_sub(1));
                if category.groups.len() > 1 {
                    ui.horizontal_wrapped(|ui| {
                        for (i, group) in category.groups.iter().enumerate() {
                            if segment(ui, state.place_group == i, &group.name) {
                                state.place_group = i;
                            }
                        }
                    });
                }
                ui.separator();
                if let Some(group) = category.groups.get(state.place_group) {
                    let entries: Vec<&Entry> = group.entries.iter().collect();
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            chosen = place_grid(
                                ui,
                                palette,
                                ("place-grid", state.place_category, state.place_group),
                                &entries,
                                &current,
                            );
                        });
                }
            } else {
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let mut any = false;
                        for category in &catalog.categories {
                            for group in &category.groups {
                                let entries: Vec<&Entry> = group
                                    .entries
                                    .iter()
                                    .filter(|e| e.name.to_lowercase().contains(&search))
                                    .collect();
                                if entries.is_empty() {
                                    continue;
                                }
                                any = true;
                                ui.label(
                                    RichText::new(format!("{}  ›  {}", category.name, group.name))
                                        .size(11.5)
                                        .color(MUTED),
                                );
                                if let Some(p) = place_grid(
                                    ui,
                                    palette,
                                    ("place-search", &category.name, &group.name),
                                    &entries,
                                    &current,
                                ) {
                                    chosen = Some(p);
                                }
                            }
                        }
                        if !any {
                            ui.label(RichText::new("Nothing matches").color(MUTED));
                        }
                    });
            }
            if chosen.is_some() {
                ui.close();
            }
        });
    if let Some(placing) = chosen {
        let neutral = state.catalog.find(&placing).is_none_or(|e| e.neutral);
        if !neutral && state.owner == 0 {
            state.owner = 1;
        }
        state.placing = placing;
    }
}

/// How finely Place and moves snap what may stand anywhere: whole blocks or single
/// cells (X). Buildings, starts and nodes always keep the building lattice.
fn precision(ui: &mut Ui, session: &mut Session) {
    let fine = &mut session.blocks.fine;
    if segment(ui, !*fine, "4 cells") {
        *fine = false;
    }
    if segment(ui, *fine, "1 cell") {
        *fine = true;
    }
}

/// The chosen tool's settings.
fn settings(
    ui: &mut Ui,
    session: &mut Session,
    docs: Option<&crate::map::view::Docs>,
) -> Option<crate::tabs::Edit> {
    let tool = session.blocks.tool;
    // A capture asks for the picker open.
    let open = std::mem::take(&mut session.blocks.open_picker);
    ui.label(RichText::new(tool.name()).size(12.5).color(TEXT).strong());
    divider(ui);
    let mut edit = None;
    match tool {
        BlockTool::Raise => {
            shape(ui, session, false);
            divider(ui);
            note(ui, "Shift lowers");
        }
        BlockTool::Level => {
            shape(ui, session, false);
            divider(ui);
            note(ui, "takes the level of the block the drag begins on");
        }
        BlockTool::Paint => {
            shape(ui, session, false);
            divider(ui);
            let state = &mut session.blocks;
            if let Some(Choice::Kind(kind)) = picker(
                ui,
                &mut state.palette,
                "paint-kind",
                "What Paint makes the blocks",
                open,
                Choice::Kind(state.kind),
                &kind_picks(),
            ) {
                state.kind = kind;
            }
        }
        BlockTool::Round => {
            shape(ui, session, false);
            divider(ui);
            note(ui, "Shift uncuts");
        }
        BlockTool::Face => {
            shape(ui, session, false);
            divider(ui);
            let state = &mut session.blocks;
            let picks: Vec<Pick> = std::iter::once("The biome's cliff".to_string())
                .chain(state.faces.iter().cloned())
                .enumerate()
                .map(|(n, name)| Pick {
                    choice: Choice::Face(n as u8),
                    name,
                    key: None,
                })
                .collect();
            if let Some(Choice::Face(style)) = picker(
                ui,
                &mut state.palette,
                "face",
                "The face the cliffs below the blocks take",
                open,
                Choice::Face(state.style),
                &picks,
            ) {
                state.style = style;
            }
        }
        BlockTool::Ramp => {
            let mut r = session.blocks.radius;
            ui.add(egui::Slider::new(&mut r, 0..=8).text("blocks each side"));
            session.blocks.radius = r;
            divider(ui);
            note(ui, "drag from a cliff's top down to its foot");
        }
        BlockTool::Cut => note(ui, "click near a block's corner; again removes the cut"),
        BlockTool::Cells => {
            shape(ui, session, true);
            divider(ui);
            let state = &mut session.blocks;
            if let Some(Choice::Kind(kind)) = picker(
                ui,
                &mut state.palette,
                "cell-kind",
                "What Cells makes the cells",
                open,
                Choice::Kind(state.cell_kind),
                &kind_picks(),
            ) {
                state.cell_kind = kind;
            }
        }
        BlockTool::Place => {
            place_picker(ui, session, open);
            divider(ui);
            precision(ui, session);
            let state = &session.blocks;
            let entry = state.catalog.find(&state.placing).cloned();
            match &state.placing {
                Placing::Definition(_) if entry.as_ref().is_some_and(|e| !e.neutral) => {
                    divider(ui);
                    note(ui, "owner");
                    let mut owner = session.blocks.owner;
                    for n in 0..=8u8 {
                        let name = if n == 0 {
                            "none".to_string()
                        } else {
                            n.to_string()
                        };
                        if segment(ui, owner == n, &name) {
                            owner = n;
                        }
                    }
                    session.blocks.owner = owner;
                }
                Placing::Amber => {
                    divider(ui);
                    ui.add(
                        egui::Slider::new(&mut session.tools.amber, 500..=6000)
                            .step_by(100.0)
                            .text("amber"),
                    );
                }
                Placing::Start(_) => {
                    divider(ui);
                    note(ui, "moves the start there when the map has it");
                }
                _ => {}
            }
        }
        BlockTool::Select => {
            let selection = session.blocks.selection.clone();
            match selection.as_slice() {
                [] => note(
                    ui,
                    "click a start, unit, building or prop; drag a rectangle round several; Shift adds",
                ),
                items => {
                    let what = match items {
                        [(Picked::Start, id)] => format!("start of player {id}"),
                        [(Picked::Entity, id)] => id.clone(),
                        [(Picked::Object, id)] => format!("prop {id}"),
                        many => format!("{} selected", many.len()),
                    };
                    ui.label(RichText::new(what).size(12.0).color(TEXT));
                    divider(ui);
                    precision(ui, session);
                    divider(ui);
                    note(ui, "drag to move; Shift adds or removes");
                    if ui.button("Delete").clicked()
                        && let Some(docs) = docs
                    {
                        let commands = crate::map::view::delete_all(session, docs, items);
                        if !commands.is_empty() {
                            edit = Some(crate::map::view::step(session, "Delete", commands));
                            session.blocks.selection.clear();
                        }
                    }
                }
            }
        }
    }
    edit
}

/// The strips over `view`, the map's viewport: the tool bar at its foot, the chosen
/// tool's settings above it. Returns an edit when a setting made one (Select's Delete).
pub(super) fn show(
    ctx: &egui::Context,
    session: &mut Session,
    view: Rect,
    docs: Option<&crate::map::view::Docs>,
) -> Option<crate::tabs::Edit> {
    let bar = egui::Area::new(Id::new(("block-tools", &session.root)))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(view.center_bottom() - vec2(0.0, 16.0))
        .constrain_to(view)
        .show(ctx, |ui| {
            frame().show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                ui.horizontal(|ui| {
                    for (g, group) in GROUPS.iter().enumerate() {
                        if g > 0 {
                            divider(ui);
                        }
                        for &tool in *group {
                            if tool_button(ui, tool, session.blocks.tool == tool) {
                                session.blocks.tool = tool;
                            }
                        }
                    }
                });
            });
        });
    let top = bar.response.rect.top();
    egui::Area::new(Id::new(("block-tool-settings", &session.root)))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(view.center().x, top - 8.0))
        .constrain_to(view)
        .show(ctx, |ui| {
            frame()
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(26.0);
                        settings(ui, session, docs)
                    })
                    .inner
                })
                .inner
        })
        .inner
}

/// A key's chip beside the view switch's buttons.
fn key_chip(ui: &mut Ui, key: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 30.0), Sense::hover());
    let chip = Rect::from_center_size(rect.center(), vec2(17.0, 17.0));
    ui.painter()
        .rect_stroke(chip, 4, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
    ui.painter().text(
        chip.center(),
        Align2::CENTER_CENTER,
        key,
        FontId::monospace(10.5),
        MUTED,
    );
}

/// A panel's switch in the view bar: its icon, highlighted while the panel shows.
fn panel_button(ui: &mut Ui, icon: Icon, on: bool, hint: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, 7, SELECTED);
    } else if hovered {
        painter.rect_filled(rect, 7, RAISED);
    }
    let color = if on {
        TEXT
    } else if hovered {
        SOFT
    } else {
        FAINT
    };
    icons::paint(painter, icons::square(rect.center(), 17.0), icon, color);
    response.on_hover_text(hint).clicked()
}

/// The view bar, floating at the top of the view: the map's panel, and what it shows: the
/// blocks or the world the biome makes of them (G), both side by side (V), where ground
/// units walk (H).
pub(super) fn view_bar(ctx: &egui::Context, session: &mut Session, view: Rect) {
    egui::Area::new(Id::new(("block-view", &session.root)))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(view.center_top() + vec2(0.0, 12.0))
        .constrain_to(view)
        .show(ctx, |ui| {
            frame()
                .inner_margin(egui::Margin::symmetric(5, 5))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 0.0);
                    ui.horizontal(|ui| {
                        let mut panels = super::Panels::get(ui.ctx());
                        let before = panels;
                        if panel_button(
                            ui,
                            Icon::PanelRight,
                            panels.properties,
                            "The map's panel: its settings, or what is selected",
                        ) {
                            panels.properties = !panels.properties;
                        }
                        if panels != before {
                            panels.set(ui.ctx());
                        }
                        divider(ui);
                        // Symmetry: every edit and placement made in the mirror too (Y).
                        let symmetry = session.blocks.symmetry;
                        let hint = format!(
                            "Symmetry: {} (Y cycles off, point, left-right, top-bottom)",
                            symmetry.name()
                        );
                        if panel_button(
                            ui,
                            Icon::Symmetry,
                            symmetry != super::Symmetry::None,
                            &hint,
                        ) {
                            let all = super::Symmetry::ALL;
                            let i = all.iter().position(|s| *s == symmetry).unwrap_or(0);
                            session.blocks.symmetry = all[(i + 1) % all.len()];
                        }
                        if symmetry != super::Symmetry::None {
                            ui.label(RichText::new(symmetry.name()).size(11.5).color(SOFT));
                        }
                        divider(ui);
                        let views = [
                            (View::Blocks, Icon::Blocks, "Blocks", "The blocks themselves   G"),
                            (
                                View::Generated,
                                Icon::World,
                                "Generated",
                                "The world the biome makes of the blocks   G",
                            ),
                            (
                                View::Split,
                                Icon::Split,
                                "Split",
                                "The blocks and the generated world side by side, one camera; the world follows two seconds after the last edit   V",
                            ),
                            (
                                View::Pathing,
                                Icon::Pathing,
                                "Pathing",
                                "Green where ground units can walk, grey where they cannot   H",
                            ),
                        ];
                        for (shown, icon, name, hint) in views {
                            match shown {
                                View::Split => {
                                    key_chip(ui, "G");
                                    divider(ui);
                                }
                                View::Pathing => {
                                    key_chip(ui, "V");
                                    divider(ui);
                                }
                                _ => {}
                            }
                            let on = session.blocks.view == shown;
                            if view_button(ui, icon, name, hint, on) && !on {
                                let next = match shown {
                                    View::Pathing | View::Split => session.blocks.toggle(shown),
                                    other => other,
                                };
                                session.blocks.set_view(next);
                                session.stale = true;
                            }
                        }
                        key_chip(ui, "H");
                    });
                });
        });
}

/// One side of the view switch: its icon and name, highlighted when shown.
fn view_button(ui: &mut Ui, icon: Icon, name: &str, hint: &str, on: bool) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_string(), FontId::proportional(12.5), TEXT);
    let size = vec2(10.0 + 18.0 + 7.0 + galley.size().x + 12.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    if on {
        painter.rect(
            rect,
            7,
            SELECTED,
            Stroke::new(1.0, SOFT),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, 7, RAISED);
    }
    let color = if on {
        TEXT
    } else if hovered {
        SOFT
    } else {
        MUTED
    };
    icons::paint(
        painter,
        icons::square(pos2(rect.left() + 10.0 + 9.0, rect.center().y), 18.0),
        icon,
        color,
    );
    painter.text(
        pos2(rect.left() + 10.0 + 18.0 + 7.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.5),
        color,
    );
    response.on_hover_text(hint).clicked()
}
