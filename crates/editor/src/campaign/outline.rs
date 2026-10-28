//! A step's outline (D110): the campaign editor's Outline mode, one step at a time with
//! previous and next across every campaign. Every step has what it is for, its cast, its
//! threads, its tension and notes; a mission adds its kinds, pitch, pillars, objectives,
//! story beats and the tabletop sketch ([`super::sketch`]); a cinematic its shots; a story
//! shows its real pages. The outline is `outlines/<step id>.json` in the campaign package,
//! plain words (never text keys), left out of releases.

use super::art::paint_cover;
use super::view::{Ctx, Mode, Request, State, dashed_rect, draft_badge, make_map, step_art};
use super::{StepKind, is_draft, mission_numbers, outline_document, steps, text};
use crate::document::Command;
use crate::tabs::Edit;
use crate::theme::*;
use crate::widgets::*;
use canopy_content::outline::{
    BeatMode, MISSION_KINDS, ObjectiveKind, Outline, Phase, Presence, ThreadMark,
};
use egui::{Align2, Color32, FontId, Frame, Margin, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

/// A step of the whole game's flow: its campaign's folder and its index there.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub root: String,
    pub index: usize,
}

/// Every step of every campaign, in play order.
pub fn flow(ctx: &mut Ctx) -> Vec<(Place, Value)> {
    let mut out = Vec::new();
    for root in ctx.campaigns.clone() {
        let Ok(Some(value)) = ctx.store.get(&super::document(&root)) else {
            continue;
        };
        for (index, step) in steps(value).iter().enumerate() {
            out.push((
                Place {
                    root: root.clone(),
                    index,
                },
                step.clone(),
            ));
        }
    }
    out
}

/// The outline of the step with `id` in the campaign at `root` (empty when it has none).
pub fn read(ctx: &mut Ctx, root: &str, id: &str) -> Outline {
    ctx.store
        .get(&outline_document(root, id))
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Sets field `field` of the step's outline to `value` (removed when empty), creating the
/// outline when it has none; typing folds into one undo step per field.
fn put(ctx: &mut Ctx, id: &str, field: &str, value: Value, label: &str, merge: Option<String>) {
    let root = ctx.campaign_root().to_string();
    let document = outline_document(&root, id);
    let empty = match &value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    };
    let current = ctx.store.get(&document).ok().flatten().cloned();
    let pointer = format!("/{field}");
    let commands = match current {
        Some(held) if empty => {
            if held.get(field).is_none() {
                return;
            }
            vec![Command::Remove {
                document: document.clone(),
                pointer,
            }]
        }
        Some(_) => vec![Command::Set {
            document: document.clone(),
            pointer,
            value,
            at: None,
        }],
        None if empty => return,
        None => vec![Command::Create {
            document: document.clone(),
            value: json!({ field: value }),
        }],
    };
    ctx.edits.push(Edit {
        label: label.to_string(),
        context: ctx.context(None),
        commands,
        merge: merge.map(|m| format!("{document}#{m}")),
    });
}

fn to_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A unit or hero the cast and beats can name: id, name and portrait.
#[derive(Debug, Clone)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub hero: bool,
}

/// The units and heroes of the content, heroes first.
pub fn characters(ctx: &Ctx) -> Vec<Character> {
    let Some(content) = ctx.content else {
        return Vec::new();
    };
    let mut out: Vec<Character> = content
        .definitions
        .iter()
        .filter(|d| d.kind == canopy_content::game::Kind::Unit)
        .map(|d| Character {
            id: d.id.to_string(),
            name: d.name.clone(),
            icon: d.icon.to_string(),
            hero: d.hero == Some(true),
        })
        .collect();
    out.sort_by(|a, b| b.hero.cmp(&a.hero).then(a.name.cmp(&b.name)));
    out
}

fn character<'a>(all: &'a [Character], id: &str) -> Option<&'a Character> {
    all.iter().find(|c| c.id == id)
}

/// A round portrait of `who` (its initial without one).
pub fn portrait(ui: &mut Ui, ctx: &mut Ctx, all: &[Character], who: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_portrait(ui, ctx, all, who, rect);
}

pub fn paint_portrait(ui: &Ui, ctx: &mut Ctx, all: &[Character], who: &str, rect: Rect) {
    let found = character(all, who);
    let texture = found.and_then(|c| ctx.thumb(ui, &c.icon));
    let painter = ui.painter();
    match texture {
        Some(t) => {
            painter.add(
                egui::epaint::RectShape::filled(rect, rect.width() / 2.0, Color32::WHITE)
                    .with_texture(t, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))),
            );
        }
        None => {
            painter.circle_filled(rect.center(), rect.width() / 2.0, SELECTED);
            let initial = found
                .map(|c| c.name.as_str())
                .unwrap_or(who)
                .rsplit('.')
                .next()
                .unwrap_or("?")
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                initial,
                FontId::proportional(rect.width() * 0.45),
                MUTED,
            );
        }
    }
    painter.circle_stroke(rect.center(), rect.width() / 2.0, Stroke::new(1.0, STRONG));
}

/// A menu choosing a character (or none).
fn pick_character(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    all: &[Character],
    current: &str,
    none: &str,
) -> Option<String> {
    let shown = character(all, current).map_or_else(
        || {
            if current.is_empty() {
                none.to_string()
            } else {
                current.to_string()
            }
        },
        |c| c.name.clone(),
    );
    let mut chosen = None;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(shown)
        .width(150.0)
        .height(360.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_empty(), none).clicked() {
                chosen = Some(String::new());
            }
            let mut heroes = true;
            for c in all {
                if heroes && !c.hero {
                    heroes = false;
                    ui.separator();
                }
                if ui.selectable_label(c.id == current, &c.name).clicked() {
                    chosen = Some(c.id.clone());
                }
            }
        });
    chosen.filter(|c| c != current)
}

/// The kind's colour on chips and the read-through.
pub fn kind_color(kind: Option<StepKind>) -> Color32 {
    match kind {
        Some(StepKind::Mission) => Color32::from_rgb(0x93, 0xc5, 0xfd),
        Some(StepKind::Story) => Color32::from_rgb(0xf0, 0xab, 0xfc),
        Some(StepKind::Cinematic) => Color32::from_rgb(0xfc, 0xd3, 0x4d),
        Some(StepKind::EarlyAccess) => TEXT,
        _ => MUTED,
    }
}

/// The step strip in the mode bar: previous, every step of this campaign, next. Previous
/// and next cross into the campaigns before and after.
pub fn stepper(ui: &mut Ui, ctx: &mut Ctx, campaign: &Value, state: &mut State) {
    let root = ctx.campaign_root().to_string();
    let here = state.selected.unwrap_or(0);
    let all = flow(ctx);
    let at = all
        .iter()
        .position(|(p, _)| p.root == root && p.index == here);
    let go = |ctx: &mut Ctx, state: &mut State, target: &(Place, Value)| {
        if target.0.root == root {
            state.selected = Some(target.0.index);
        } else {
            ctx.shared.requests.push(Request::Show {
                document: super::document(&target.0.root),
                step: Some(target.0.index),
                mode: Mode::Outline,
            });
        }
    };
    let title = |ctx: &mut Ctx, step: &Value| -> String {
        match StepKind::of(step) {
            Some(StepKind::EarlyAccess) => "Early access line".into(),
            _ => ctx.say(text(step, "title")),
        }
    };
    let previous = at.and_then(|a| a.checked_sub(1)).and_then(|a| all.get(a));
    let label = previous.map_or("Start".to_string(), |(p, s)| {
        let t = title(ctx, s);
        if p.root == root {
            format!("‹  {t}")
        } else {
            format!("‹  {} · {t}", super::id_of(&p.root))
        }
    });
    if ui
        .add_enabled(previous.is_some(), egui::Button::new(label))
        .on_hover_text("The step before")
        .clicked()
        && let Some(p) = previous
    {
        go(ctx, state, p);
    }
    ui.add_space(8.0);
    let numbers = mission_numbers(campaign);
    for (i, step) in steps(campaign).iter().enumerate() {
        let kind = StepKind::of(step);
        if kind == Some(StepKind::EarlyAccess) {
            let (r, response) = ui.allocate_exact_size(vec2(10.0, 24.0), Sense::click());
            ui.painter()
                .rect_filled(Rect::from_center_size(r.center(), vec2(3.0, 24.0)), 1, TEXT);
            if response.on_hover_text("The early access line").clicked() {
                state.selected = Some(i);
            }
            continue;
        }
        let label = match (kind, numbers[i]) {
            (Some(StepKind::Mission), Some(n)) => n.to_string(),
            (Some(StepKind::Chapter), _) => "▪".into(),
            (Some(StepKind::Story), _) => "S".into(),
            (Some(StepKind::Cinematic), _) => "C".into(),
            _ => "?".into(),
        };
        let size = vec2(24.0, 24.0);
        let (r, response) = ui.allocate_exact_size(size, Sense::click());
        let color = kind_color(kind);
        let current = i == here;
        let painter = ui.painter();
        if current {
            painter.rect_filled(r, 5, TEXT);
        } else if is_draft(step) {
            dashed_rect(painter, r.shrink(0.5), Stroke::new(1.0, color));
        } else {
            painter.rect(
                r,
                5,
                RAISED,
                Stroke::new(1.0, color),
                egui::StrokeKind::Inside,
            );
        }
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
            if current { BG } else { color },
        );
        let name = title(ctx, step);
        let response = response.on_hover_text(name.clone());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, current, &name)
        });
        if response.clicked() {
            state.selected = Some(i);
        }
    }
    ui.add_space(8.0);
    let next = at.and_then(|a| all.get(a + 1));
    let label = next.map_or("End".to_string(), |(p, s)| {
        let t = title(ctx, s);
        if p.root == root {
            format!("{t}  ›")
        } else {
            format!("{} · {t}  ›", super::id_of(&p.root))
        }
    });
    if ui
        .add_enabled(next.is_some(), egui::Button::new(label))
        .on_hover_text("The step after")
        .clicked()
        && let Some(n) = next
    {
        go(ctx, state, n);
    }
}

/// The Outline mode: the step's one-pager on the left, its body in the middle (a mission's
/// tabletop, a story's pages, a cinematic's shots) and, for a mission, its beats on the
/// right.
pub fn show(ui: &mut Ui, ctx: &mut Ctx, campaign: &Value, state: &mut State) {
    let list = steps(campaign);
    if list.is_empty() {
        empty(
            ui,
            "No steps yet",
            "Add a step in the Steps view to outline it.",
        );
        return;
    }
    let i = state.selected.unwrap_or(0).min(list.len() - 1);
    state.selected = Some(i);
    let step = list[i].clone();
    let kind = StepKind::of(&step);
    let id = text(&step, "id").to_string();
    let root = ctx.campaign_root().to_string();
    let outline = read(ctx, &root, &id);
    let cast = characters(ctx);
    egui::Panel::left(egui::Id::new(("outline-pager", ctx.document)))
        .exact_size(380.0)
        .resizable(false)
        .show_separator_line(true)
        .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(16)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    one_pager(ui, ctx, campaign, i, &step, &outline, &cast)
                });
        });
    if kind == Some(StepKind::Mission) {
        egui::Panel::right(egui::Id::new(("outline-beats", ctx.document)))
            .exact_size(360.0)
            .resizable(false)
            .show_separator_line(true)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(14)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| beats(ui, ctx, &id, &outline, &cast));
            });
    }
    egui::CentralPanel::default()
        .frame(Frame::new().fill(MAIN).inner_margin(Margin::same(16)))
        .show(ui, |ui| {
            if kind != Some(StepKind::EarlyAccess) {
                let own = match kind {
                    Some(StepKind::Mission) => "Tabletop",
                    Some(StepKind::Story) => "Pages",
                    Some(StepKind::Cinematic) => "Shots",
                    _ => "Chapter",
                };
                ui.horizontal(|ui| {
                    if ui.selectable_label(!state.pictures, own).clicked() {
                        state.pictures = false;
                    }
                    let n = outline.images.len();
                    let name = match n {
                        0 => "Pictures".to_string(),
                        n => format!("Pictures ({n})"),
                    };
                    if ui
                        .selectable_label(state.pictures, name)
                        .on_hover_text("Concept pictures to think with; the assistant paints them")
                        .clicked()
                    {
                        state.pictures = true;
                    }
                });
                ui.add_space(8.0);
                if state.pictures {
                    pictures(ui, ctx, &id, &outline, state);
                    return;
                }
            }
            centre(ui, ctx, campaign, i, &step, &id, &outline, &cast, state);
        });
}

/// The middle of the outline: the step's own view (the tabletop, the pages, the shots).
#[allow(clippy::too_many_arguments)]
fn centre(
    ui: &mut Ui,
    ctx: &mut Ctx,
    campaign: &Value,
    i: usize,
    step: &Value,
    id: &str,
    outline: &Outline,
    cast: &[Character],
    state: &mut State,
) {
    let kind = StepKind::of(step);
    match kind {
        Some(StepKind::Mission) => {
            let sketch = outline.sketch.clone().unwrap_or_default();
            let changed = super::sketch::show(ui, ctx, &sketch, &mut state.sketch, cast);
            if let Some((label, sketch)) = changed {
                put(ctx, id, "sketch", to_value(&sketch), &label, None);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "A sketch, not the map: it stays beside the mission and never ships.",
                    )
                    .size(11.0)
                    .color(FAINT),
                );
                if text(step, "map").is_empty()
                    && ui
                        .add_enabled(ctx.content.is_some(), egui::Button::new("Make map"))
                        .on_hover_text("A new map of flat blocks in the campaign's missions/")
                        .clicked()
                {
                    make_map(ctx, step, i);
                }
            });
        }
        Some(StepKind::Story) => pages(ui, ctx, campaign, i, step),
        Some(StepKind::Cinematic) => shots(ui, ctx, id, outline, cast),
        Some(StepKind::EarlyAccess) => {
            ui.add_space(60.0);
            empty(
                ui,
                "You have reached the end of early access",
                "The story continues later. Releases ship the steps before this line; the steps after it stay here, in the drafts and outlines.",
            );
        }
        _ => {
            ui.add_space(60.0);
            empty(
                ui,
                "A chapter",
                "A title card. Its outline is what the chapter is about: write it on the left.",
            );
        }
    }
}

/// The step's pictures (D111): a grid, newest first, and one open large with what it
/// shows, how it was made, and Remove (an undo step; its file goes on save).
fn pictures(ui: &mut Ui, ctx: &mut Ctx, id: &str, outline: &Outline, state: &mut State) {
    let list = &outline.images;
    if list.is_empty() {
        ui.add_space(60.0);
        empty(
            ui,
            "No pictures yet",
            "Ask the assistant (the robot in the header) to paint this step: a place, a character, a moment. It can vary its own pictures and start from the game's art, and they land here.",
        );
        return;
    }
    let folder = ctx.root.map(|r| {
        r.join(ctx.campaign_root())
            .join(canopy_content::outline::IMAGES_DIR)
    });
    let path = |file: &str| folder.as_ref().map(|f| f.join(file));
    if state.picture.is_some_and(|p| p >= list.len()) {
        state.picture = None;
    }
    let Some(open) = state.picture else {
        // The grid, newest first.
        let width = ui.available_width();
        let columns = ((width / 300.0).floor() as usize).clamp(1, 4);
        let gap = 12.0;
        let tile = (width - gap * (columns - 1) as f32) / columns as f32;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let order: Vec<usize> = (0..list.len()).rev().collect();
                for row in order.chunks(columns) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for &n in row {
                            let picture = &list[n];
                            ui.vertical(|ui| {
                                ui.set_width(tile);
                                let (rect, response) = ui.allocate_exact_size(
                                    vec2(tile, tile * 2.0 / 3.0),
                                    Sense::click(),
                                );
                                let texture = path(&picture.file)
                                    .and_then(|p| ctx.shared.thumbs.file(ui.ctx(), &p, 720));
                                paint_cover(ui, rect, texture, "Loading…");
                                if response.hovered() {
                                    ui.painter().rect_stroke(
                                        rect,
                                        4,
                                        Stroke::new(1.5, STRONG),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                let response = response.on_hover_text(
                                    picture.caption.as_deref().unwrap_or(&picture.file),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        format!("Picture {}", picture.file),
                                    )
                                });
                                if response.clicked() {
                                    state.picture = Some(n);
                                }
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(
                                            picture.caption.as_deref().unwrap_or(&picture.file),
                                        )
                                        .size(11.5)
                                        .color(SOFT),
                                    )
                                    .truncate(),
                                );
                            });
                        }
                    });
                    ui.add_space(gap);
                }
            });
        return;
    };
    let picture = list[open].clone();
    ui.horizontal(|ui| {
        if ui.button("All pictures").clicked() {
            state.picture = None;
        }
        // Newest first, as the grid shows them.
        let place = list.len() - open;
        ui.label(
            RichText::new(format!("{place} of {}", list.len()))
                .size(11.0)
                .color(MUTED),
        );
        if ui
            .add_enabled(open + 1 < list.len(), egui::Button::new("Newer"))
            .clicked()
        {
            state.picture = Some(open + 1);
        }
        if ui
            .add_enabled(open > 0, egui::Button::new("Older"))
            .clicked()
        {
            state.picture = Some(open - 1);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Remove")
                .on_hover_text(
                    "Takes it out of the outline (undo brings it back); its file goes on save",
                )
                .clicked()
            {
                let mut next = list.clone();
                next.remove(open);
                put(ctx, id, "images", to_value(&next), "Remove picture", None);
                state.picture = None;
            }
        });
    });
    ui.add_space(8.0);
    // The picture, whole, above what it shows and how it was made.
    let below = 130.0;
    let room = ui.available_size() - vec2(0.0, below);
    let texture = path(&picture.file).and_then(|p| ctx.shared.thumbs.file(ui.ctx(), &p, 2048));
    let size = texture
        .and_then(|t| ui.ctx().tex_manager().read().meta(t).map(|m| m.size))
        .map_or(vec2(room.x, room.x * 2.0 / 3.0), |[w, h]| {
            vec2(w as f32, h as f32)
        });
    let scale = (room.x / size.x).min(room.y.max(120.0) / size.y);
    let (rect, _) = ui.allocate_exact_size(size * scale, Sense::hover());
    paint_cover(ui, rect, texture, "Loading…");
    ui.add_space(8.0);
    let mut caption = picture.caption.clone().unwrap_or_default();
    if ui
        .add(
            egui::TextEdit::singleline(&mut caption)
                .hint_text("What it shows")
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        let mut next = list.clone();
        next[open].caption = (!caption.trim().is_empty()).then_some(caption);
        put(
            ctx,
            id,
            "images",
            to_value(&next),
            "Picture caption",
            Some(format!("images/{open}")),
        );
    }
    egui::CollapsingHeader::new(RichText::new("How it was made").size(11.0).color(MUTED))
        .id_salt(("picture-made", id, open))
        .show(ui, |ui| {
            let mut made = String::new();
            if let Some(model) = &picture.model {
                made.push_str(&format!("{model} · "));
            }
            made.push_str(&picture.file);
            if !picture.references.is_empty() {
                made.push_str(&format!(" · from {}", picture.references.join(", ")));
            }
            ui.label(RichText::new(made).size(10.5).color(FAINT));
            if let Some(prompt) = &picture.prompt {
                ui.label(RichText::new(prompt).size(11.0).color(SOFT));
            }
        });
}

/// The left column: what the step is, for every kind; a mission's brief.
fn one_pager(
    ui: &mut Ui,
    ctx: &mut Ctx,
    campaign: &Value,
    i: usize,
    step: &Value,
    outline: &Outline,
    all: &[Character],
) {
    let kind = StepKind::of(step);
    let id = text(step, "id").to_string();
    let at = format!("/steps/{i}");
    let number = mission_numbers(campaign)[i];
    let kicker = match (kind, number) {
        (Some(StepKind::Mission), Some(n)) => format!("STEP {} · MISSION {n}", i + 1),
        (Some(k), _) => format!("STEP {} · {}", i + 1, k.label().to_uppercase()),
        _ => format!("STEP {}", i + 1),
    };
    ui.horizontal(|ui| {
        ui.label(eyebrow(&kicker));
        if is_draft(step) {
            let (r, _) = ui.allocate_exact_size(vec2(44.0, 16.0), Sense::hover());
            draft_badge(ui.painter(), r.min);
        }
    });
    if kind == Some(StepKind::EarlyAccess) {
        ui.label(
            RichText::new("The early access line")
                .size(20.0)
                .color(TEXT),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new("One in the whole game: releases end here.")
                .size(12.0)
                .color(MUTED),
        );
        return;
    }
    let shown = ctx.say(text(step, "title"));
    let mut title = shown.clone();
    if ui
        .add(
            egui::TextEdit::singleline(&mut title)
                .font(FontId::proportional(20.0))
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        ctx.edit_text(
            "Step title",
            (step, step, &at),
            "title",
            "title",
            &title,
            false,
        );
    }
    let words = |v: &Option<String>| v.clone().unwrap_or_default();
    let text_field =
        |ui: &mut Ui, ctx: &mut Ctx, name: &str, field: &str, value: String, rows: usize| {
            label(ui, name);
            let mut edited = value.clone();
            let response = if rows > 1 {
                ui.add(
                    egui::TextEdit::multiline(&mut edited)
                        .desired_rows(rows)
                        .desired_width(f32::INFINITY),
                )
            } else {
                ui.add(egui::TextEdit::singleline(&mut edited).desired_width(f32::INFINITY))
            };
            if response.changed() {
                put(ctx, &id, field, json!(edited), name, Some(field.into()));
            }
        };
    if kind == Some(StepKind::Mission) {
        label(ui, "Kind of mission");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for k in MISSION_KINDS {
                let on = outline.kinds.iter().any(|x| x == k);
                if ui.selectable_label(on, k.replace('-', " ")).clicked() {
                    let mut kinds = outline.kinds.clone();
                    if on {
                        kinds.retain(|x| x != k);
                    } else {
                        kinds.push(k.to_string());
                    }
                    put(ctx, &id, "kinds", json!(kinds), "Mission kinds", None);
                }
            }
        });
        text_field(ui, ctx, "The pitch", "pitch", words(&outline.pitch), 2);
    }
    text_field(
        ui,
        ctx,
        "What this step is for",
        "purpose",
        words(&outline.purpose),
        3,
    );
    if kind == Some(StepKind::Mission) {
        label(ui, "Pillars");
        let pillars = outline.pillars.clone().unwrap_or_default();
        for (name, field, value) in [
            ("Play: what it teaches or tests", "play", &pillars.play),
            ("Character: who it grows", "character", &pillars.character),
            ("Story: what it tells", "story", &pillars.story),
        ] {
            let mut edited = value.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut edited)
                        .hint_text(name)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                let mut next = pillars.clone();
                let slot = match field {
                    "play" => &mut next.play,
                    "character" => &mut next.character,
                    _ => &mut next.story,
                };
                *slot = (!edited.trim().is_empty()).then_some(edited);
                put(
                    ctx,
                    &id,
                    "pillars",
                    to_value(&next),
                    "Pillars",
                    Some(format!("pillars/{field}")),
                );
            }
        }
        objectives(ui, ctx, &id, outline);
        ui.horizontal(|ui| {
            ui.label(RichText::new("About").size(11.0).color(MUTED));
            let mut minutes = outline.minutes.unwrap_or(0);
            if ui
                .add(
                    egui::DragValue::new(&mut minutes)
                        .range(0..=120)
                        .suffix(" minutes"),
                )
                .changed()
            {
                put(
                    ctx,
                    &id,
                    "minutes",
                    if minutes == 0 {
                        Value::Null
                    } else {
                        json!(minutes)
                    },
                    "Length",
                    Some("minutes".into()),
                );
            }
        });
    }
    if kind == Some(StepKind::Cinematic) {
        text_field(ui, ctx, "Where", "location", words(&outline.location), 1);
        label(ui, "Made as");
        ui.horizontal(|ui| {
            for (value, name) in [
                ("scene", "In-engine scene"),
                ("film", "Rendered film"),
                ("card", "Story card for now"),
            ] {
                let on = outline.made_as.as_deref() == Some(value);
                if ui.selectable_label(on, name).clicked() {
                    put(
                        ctx,
                        &id,
                        "madeAs",
                        if on { Value::Null } else { json!(value) },
                        "Made as",
                        None,
                    );
                }
            }
        });
    }
    // How tense, for the read-through's curve.
    label(ui, "Tension");
    ui.horizontal(|ui| {
        for t in 1..=5u8 {
            let on = outline.tension.is_some_and(|x| x >= t);
            let (r, response) = ui.allocate_exact_size(vec2(36.0, 10.0), Sense::click());
            ui.painter().rect_filled(
                r,
                3,
                if on {
                    Color32::from_gray(90 + t * 32)
                } else {
                    SELECTED
                },
            );
            let response = response.on_hover_text(match t {
                1 => "Calm",
                2 => "Gentle",
                3 => "Tense",
                4 => "Hard",
                _ => "The peak",
            });
            if response.clicked() {
                let value = if outline.tension == Some(t) {
                    Value::Null
                } else {
                    json!(t)
                };
                put(ctx, &id, "tension", value, "Tension", None);
            }
        }
    });
    cast(ui, ctx, &id, outline, all);
    threads(ui, ctx, &id, outline);
    text_field(ui, ctx, "Notes", "notes", words(&outline.notes), 3);
}

fn label(ui: &mut Ui, name: &str) {
    ui.add_space(12.0);
    ui.label(eyebrow(name));
    ui.add_space(3.0);
}

fn objectives(ui: &mut Ui, ctx: &mut Ctx, id: &str, outline: &Outline) {
    label(ui, "Objectives and secrets");
    let list = &outline.objectives;
    let mut next = list.clone();
    let mut changed = None;
    for (n, objective) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut kind = objective.kind;
            egui::ComboBox::from_id_salt(("objective-kind", id, n))
                .selected_text(match kind {
                    ObjectiveKind::Main => "Main",
                    ObjectiveKind::Optional => "Optional",
                    ObjectiveKind::Secret => "Secret",
                    ObjectiveKind::EasterEgg => "Easter egg",
                })
                .width(86.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut kind, ObjectiveKind::Main, "Main");
                    ui.selectable_value(&mut kind, ObjectiveKind::Optional, "Optional");
                    ui.selectable_value(&mut kind, ObjectiveKind::Secret, "Secret");
                    ui.selectable_value(&mut kind, ObjectiveKind::EasterEgg, "Easter egg");
                });
            if kind != objective.kind {
                next[n].kind = kind;
                changed = Some(None);
            }
            let mut words = objective.text.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut words)
                        .desired_width(ui.available_width() - 30.0),
                )
                .changed()
            {
                next[n].text = words;
                changed = Some(Some(format!("objectives/{n}")));
            }
            if icon_button(ui, crate::icons::Icon::Close, false, true)
                .on_hover_text("Remove")
                .clicked()
            {
                next.remove(n);
                changed = Some(None);
            }
        });
    }
    if ui.small_button("+ Objective").clicked() {
        next.push(canopy_content::outline::Objective {
            kind: if list.iter().any(|o| o.kind == ObjectiveKind::Main) {
                ObjectiveKind::Optional
            } else {
                ObjectiveKind::Main
            },
            text: String::new(),
        });
        changed = Some(None);
    }
    if let Some(merge) = changed {
        put(ctx, id, "objectives", to_value(&next), "Objectives", merge);
    }
}

fn cast(ui: &mut Ui, ctx: &mut Ctx, id: &str, outline: &Outline, all: &[Character]) {
    label(ui, "On stage");
    let mut next = outline.cast.clone();
    let mut changed = false;
    for (n, member) in outline.cast.iter().enumerate() {
        ui.horizontal(|ui| {
            portrait(ui, ctx, all, &member.who, 26.0);
            if let Some(who) = pick_character(ui, ("cast", id, n), all, &member.who, "Someone") {
                next[n].who = who;
                changed = true;
            }
            let mut presence = member.presence;
            egui::ComboBox::from_id_salt(("presence", id, n))
                .selected_text(presence_label(presence))
                .width(82.0)
                .show_ui(ui, |ui| {
                    for p in [
                        Presence::On,
                        Presence::Mentioned,
                        Presence::Joins,
                        Presence::Leaves,
                        Presence::Open,
                    ] {
                        ui.selectable_value(&mut presence, p, presence_label(p));
                    }
                });
            if presence != member.presence {
                next[n].presence = presence;
                changed = true;
            }
            if icon_button(ui, crate::icons::Icon::Close, false, true)
                .on_hover_text("Remove")
                .clicked()
            {
                next.remove(n);
                changed = true;
            }
        });
    }
    if let Some(who) = pick_character(ui, ("cast-add", id), all, "", "+ Someone on stage")
        && !who.is_empty()
    {
        {
            next.push(canopy_content::outline::Cast {
                who,
                presence: Presence::On,
                note: None,
            });
            changed = true;
        }
    }
    if changed {
        put(ctx, id, "cast", to_value(&next), "Cast", None);
    }
}

pub fn presence_label(p: Presence) -> &'static str {
    match p {
        Presence::On => "on stage",
        Presence::Mentioned => "mentioned",
        Presence::Joins => "joins",
        Presence::Leaves => "leaves",
        Presence::Open => "not decided",
    }
}

fn threads(ui: &mut Ui, ctx: &mut Ctx, id: &str, outline: &Outline) {
    label(ui, "Threads");
    let mut next = outline.threads.clone();
    let mut changed = None;
    for (n, thread) in outline.threads.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut mark = thread.mark;
            egui::ComboBox::from_id_salt(("thread", id, n))
                .selected_text(match mark {
                    ThreadMark::Opens => "Opens",
                    ThreadMark::Carries => "Carries",
                    ThreadMark::Closes => "Closes",
                })
                .width(72.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mark, ThreadMark::Opens, "Opens");
                    ui.selectable_value(&mut mark, ThreadMark::Carries, "Carries");
                    ui.selectable_value(&mut mark, ThreadMark::Closes, "Closes");
                });
            if mark != thread.mark {
                next[n].mark = mark;
                changed = Some(None);
            }
            let mut words = thread.text.clone();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut words)
                        .desired_width(ui.available_width() - 30.0),
                )
                .changed()
            {
                next[n].text = words;
                changed = Some(Some(format!("threads/{n}")));
            }
            if icon_button(ui, crate::icons::Icon::Close, false, true)
                .on_hover_text("Remove")
                .clicked()
            {
                next.remove(n);
                changed = Some(None);
            }
        });
    }
    if ui.small_button("+ Thread").clicked() {
        next.push(canopy_content::outline::Thread {
            mark: ThreadMark::Opens,
            text: String::new(),
        });
        changed = Some(None);
    }
    if let Some(merge) = changed {
        put(ctx, id, "threads", to_value(&next), "Threads", merge);
    }
}

/// A mission's story beats, grouped before, during and after the mission.
fn beats(ui: &mut Ui, ctx: &mut Ctx, id: &str, outline: &Outline, all: &[Character]) {
    ui.horizontal(|ui| {
        ui.label(eyebrow("Story beats"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+ Beat").clicked() {
                let mut next = outline.beats.clone();
                next.push(canopy_content::outline::Beat {
                    phase: Phase::During,
                    speaker: None,
                    trigger: None,
                    mode: BeatMode::Line,
                    text: String::new(),
                });
                put(ctx, id, "beats", to_value(&next), "Add beat", None);
            }
        });
    });
    let mut next = outline.beats.clone();
    let mut changed = None;
    for (phase, heading) in [
        (Phase::Before, "Before · the opening scene"),
        (Phase::During, "During"),
        (Phase::After, "After · the closing scene"),
    ] {
        ui.add_space(10.0);
        ui.label(RichText::new(heading).size(11.0).color(MUTED));
        ui.add_space(4.0);
        for (n, beat) in outline
            .beats
            .iter()
            .enumerate()
            .filter(|(_, b)| b.phase == phase)
        {
            Frame::new()
                .fill(RAISED)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(8)
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        portrait(
                            ui,
                            ctx,
                            all,
                            beat.speaker.as_deref().unwrap_or_default(),
                            34.0,
                        );
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let speaker = beat.speaker.clone().unwrap_or_default();
                                if let Some(who) = pick_character(
                                    ui,
                                    ("beat-speaker", id, n),
                                    all,
                                    &speaker,
                                    "Narration",
                                ) {
                                    next[n].speaker = (!who.is_empty()).then_some(who);
                                    changed = Some(None);
                                }
                                if icon_button(ui, crate::icons::Icon::Close, false, true)
                                    .on_hover_text("Remove this beat")
                                    .clicked()
                                {
                                    next.remove(n);
                                    changed = Some(None);
                                }
                            });
                            ui.horizontal(|ui| {
                                let mut trigger = beat.trigger.clone().unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut trigger)
                                            .hint_text("When: Start, Wave 3…")
                                            .desired_width(110.0),
                                    )
                                    .changed()
                                    && n < next.len()
                                {
                                    next[n].trigger =
                                        (!trigger.trim().is_empty()).then_some(trigger);
                                    changed = Some(Some(format!("beats/{n}/trigger")));
                                }
                                let scene = beat.mode == BeatMode::Scene;
                                if ui
                                    .selectable_label(scene, "Scene")
                                    .on_hover_text("Letterboxed, the battle held")
                                    .clicked()
                                    && n < next.len()
                                {
                                    next[n].mode = BeatMode::Scene;
                                    changed = Some(None);
                                }
                                if ui
                                    .selectable_label(!scene, "Line")
                                    .on_hover_text("Spoken in play, with a portrait")
                                    .clicked()
                                    && n < next.len()
                                {
                                    next[n].mode = BeatMode::Line;
                                    changed = Some(None);
                                }
                                let mut moved = beat.phase;
                                egui::ComboBox::from_id_salt(("beat-phase", id, n))
                                    .selected_text("·")
                                    .width(24.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut moved, Phase::Before, "Before");
                                        ui.selectable_value(&mut moved, Phase::During, "During");
                                        ui.selectable_value(&mut moved, Phase::After, "After");
                                    });
                                if moved != beat.phase && n < next.len() {
                                    next[n].phase = moved;
                                    changed = Some(None);
                                }
                            });
                        });
                    });
                    let mut words = beat.text.clone();
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut words)
                                .desired_rows(2)
                                .hint_text("What is said or shown")
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                        && n < next.len()
                    {
                        next[n].text = words;
                        changed = Some(Some(format!("beats/{n}/text")));
                    }
                });
            ui.add_space(6.0);
        }
    }
    if let Some(merge) = changed {
        put(ctx, id, "beats", to_value(&next), "Story beats", merge);
    }
}

/// A story's pages, read as the manuscript, with what comes before and after.
fn pages(ui: &mut Ui, ctx: &mut Ctx, campaign: &Value, i: usize, step: &Value) {
    let at = format!("/steps/{i}");
    let pages = step
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width().min(720.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(width);
                around(ui, ctx, campaign, i, true);
                for (p, page) in pages.iter().enumerate() {
                    ui.add_space(14.0);
                    ui.label(eyebrow(&format!("Page {}", p + 1)));
                    let art = text(page, "art").to_string();
                    if !art.is_empty() {
                        let (r, _) =
                            ui.allocate_exact_size(vec2(width, width * 0.28), Sense::hover());
                        let t = ctx.thumb(ui, &art);
                        paint_cover(ui, r, t, "");
                    }
                    let pointer = format!("{at}/pages/{p}");
                    let shown = ctx.say(text(page, "text"));
                    let mut words = shown.clone();
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut words)
                                .font(FontId::proportional(16.0))
                                .desired_rows(5)
                                .desired_width(width),
                        )
                        .changed()
                    {
                        ctx.edit_text(
                            "Page text",
                            (step, page, &pointer),
                            "text",
                            &format!("page-{}", p + 1),
                            &words,
                            false,
                        );
                    }
                }
                ui.add_space(10.0);
                if ui.button("+ Page").clicked() {
                    let key = ctx.key(step, &format!("page-{}", pages.len() + 1));
                    let mut commands = ctx.put_text(vec![(key.clone(), String::new())]);
                    commands.push(if step.get("pages").is_some_and(Value::is_array) {
                        Command::Insert {
                            document: ctx.document.to_string(),
                            pointer: format!("{at}/pages"),
                            index: pages.len(),
                            value: json!({ "text": key }),
                        }
                    } else {
                        Command::Set {
                            document: ctx.document.to_string(),
                            pointer: format!("{at}/pages"),
                            value: json!([{ "text": key }]),
                            at: None,
                        }
                    });
                    ctx.step("Add page", commands);
                }
                ui.add_space(14.0);
                around(ui, ctx, campaign, i, false);
            });
        });
}

/// The step before (`before`) or after this one in its campaign, as a card.
fn around(ui: &mut Ui, ctx: &mut Ctx, campaign: &Value, i: usize, before: bool) {
    let list = steps(campaign);
    let found = if before {
        (0..i)
            .rev()
            .find(|&j| StepKind::of(&list[j]) != Some(StepKind::Chapter))
    } else {
        (i + 1..list.len()).find(|&j| StepKind::of(&list[j]) != Some(StepKind::Chapter))
    };
    let Some(j) = found else {
        return;
    };
    let step = &list[j];
    let id = text(step, "id").to_string();
    let root = ctx.campaign_root().to_string();
    let outline = read(ctx, &root, &id);
    let title = ctx.say(text(step, "title"));
    let line = outline
        .pitch
        .clone()
        .or(outline.purpose.clone())
        .unwrap_or_default();
    Frame::new()
        .fill(RAISED)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(8)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let art = step_art(step).to_string();
                let (r, _) = ui.allocate_exact_size(vec2(84.0, 48.0), Sense::hover());
                let t = ctx.thumb(ui, &art);
                paint_cover(ui, r, t, "");
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            if before { "COMING FROM" } else { "GOING TO" },
                            StepKind::of(step)
                                .map_or("STEP", |k| k.label())
                                .to_uppercase()
                        ))
                        .size(10.0)
                        .color(kind_color(StepKind::of(step))),
                    );
                    ui.label(RichText::new(title).color(TEXT));
                    if !line.is_empty() {
                        ui.label(RichText::new(line).size(12.0).color(MUTED));
                    }
                });
            });
        });
}

/// A cinematic's shots as a storyboard.
fn shots(ui: &mut Ui, ctx: &mut Ctx, id: &str, outline: &Outline, all: &[Character]) {
    let total: f32 = outline.shots.iter().filter_map(|s| s.seconds).sum();
    ui.horizontal(|ui| {
        ui.label(eyebrow("Shots"));
        ui.label(
            RichText::new(format!(
                "{} shots · about {total:.0} seconds",
                outline.shots.len()
            ))
            .size(11.0)
            .color(MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("+ Shot").clicked() {
                let mut next = outline.shots.clone();
                next.push(canopy_content::outline::Shot {
                    framing: Some("wide".into()),
                    action: String::new(),
                    speaker: None,
                    line: None,
                    seconds: Some(5.0),
                });
                put(ctx, id, "shots", to_value(&next), "Add shot", None);
            }
        });
    });
    ui.add_space(8.0);
    let mut next = outline.shots.clone();
    let mut changed = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let columns = ((ui.available_width() / 300.0).floor() as usize).max(1);
            egui::Grid::new(("shots", id))
                .num_columns(columns)
                .spacing(vec2(12.0, 12.0))
                .show(ui, |ui| {
                    for (n, shot) in outline.shots.iter().enumerate() {
                        Frame::new()
                            .fill(RAISED)
                            .stroke(Stroke::new(1.0, BORDER))
                            .corner_radius(8)
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| ui.vertical(|ui| {
                                ui.set_width(280.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{}", n + 1)).strong().color(FAINT));
                                    let framing = shot.framing.clone().unwrap_or_default();
                                    let mut chosen = framing.clone();
                                    egui::ComboBox::from_id_salt(("framing", id, n))
                                        .selected_text(if framing.is_empty() { "framing".into() } else { framing.to_uppercase() })
                                        .width(90.0)
                                        .show_ui(ui, |ui| {
                                            for f in ["wide", "medium", "close", "two-shot", "over", "aerial"] {
                                                ui.selectable_value(&mut chosen, f.to_string(), f);
                                            }
                                        });
                                    if chosen != framing {
                                        next[n].framing = Some(chosen);
                                        changed = Some(None);
                                    }
                                    let mut seconds = shot.seconds.unwrap_or(0.0);
                                    if ui.add(egui::DragValue::new(&mut seconds).range(0.0..=120.0).speed(0.5).suffix(" s")).changed() {
                                        next[n].seconds = Some(seconds);
                                        changed = Some(Some(format!("shots/{n}/seconds")));
                                    }
                                    if icon_button(ui, crate::icons::Icon::Close, false, true).on_hover_text("Remove this shot").clicked() {
                                        next.remove(n);
                                        changed = Some(None);
                                    }
                                });
                                let mut action = shot.action.clone();
                                if ui
                                    .add(egui::TextEdit::multiline(&mut action).desired_rows(2).hint_text("What we see").desired_width(f32::INFINITY))
                                    .changed()
                                    && n < next.len()
                                {
                                    next[n].action = action;
                                    changed = Some(Some(format!("shots/{n}/action")));
                                }
                                ui.horizontal(|ui| {
                                    let speaker = shot.speaker.clone().unwrap_or_default();
                                    if let Some(who) = pick_character(ui, ("shot-speaker", id, n), all, &speaker, "No one speaks")
                                        && n < next.len()
                                    {
                                        next[n].speaker = (!who.is_empty()).then_some(who);
                                        changed = Some(None);
                                    }
                                });
                                let mut line = shot.line.clone().unwrap_or_default();
                                if ui
                                    .add(egui::TextEdit::singleline(&mut line).hint_text("The line").desired_width(f32::INFINITY))
                                    .changed()
                                    && n < next.len()
                                {
                                    next[n].line = (!line.trim().is_empty()).then_some(line);
                                    changed = Some(Some(format!("shots/{n}/line")));
                                }
                            }));
                        if (n + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
            if outline.shots.is_empty() {
                ui.add_space(40.0);
                empty(ui, "No shots yet", "A cinematic told shot by shot: its framing, what we see, who speaks and for how long.");
            }
        });
    if let Some(merge) = changed {
        put(ctx, id, "shots", to_value(&next), "Shots", merge);
    }
}
