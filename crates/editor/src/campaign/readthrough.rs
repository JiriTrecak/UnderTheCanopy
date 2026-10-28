//! The read-through (D110): every step of every campaign in one row, in play order, to
//! read the whole story through and see that it holds together. Chapters band the steps;
//! each step is a card (its kind, title, kinds of mission and pitch or purpose); under them
//! a lane per character of the outlines' casts, the tension curve, and the early access
//! line drawn across. A click opens the step's outline.

use super::outline::{Character, characters, flow, kind_color, paint_portrait, read};
use super::view::{Ctx, Mode, Request, State, dashed_rect, draft_badge};
use super::{StepKind, is_draft, mission_numbers, text};
use crate::theme::*;
use canopy_content::outline::{Outline, Presence};
use egui::{Align2, Color32, FontId, Frame, Margin, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::Value;

const LABEL: f32 = 160.0;
const COLUMN: f32 = 124.0;
const CARD: f32 = 150.0;
const LANE: f32 = 38.0;
const CURVE: f32 = 90.0;

/// One column: a step, where it is, its outline.
struct Column {
    root: String,
    index: usize,
    step: Value,
    title: String,
    number: Option<usize>,
    outline: Outline,
}

pub fn show(ui: &mut Ui, ctx: &mut Ctx, state: &mut State) {
    let here = ctx.campaign_root().to_string();
    let all = characters(ctx);
    // Every step but chapters (which band the others), with its outline.
    let mut columns = Vec::new();
    let mut chapters: Vec<(usize, String)> = Vec::new();
    let mut campaigns: Vec<(usize, String, bool)> = Vec::new();
    let mut numbers: std::collections::HashMap<String, Vec<Option<usize>>> = Default::default();
    for (place, step) in flow(ctx) {
        if campaigns.last().is_none_or(|(_, r, _)| *r != place.root) {
            let value = ctx
                .store
                .get(&super::document(&place.root))
                .ok()
                .flatten()
                .cloned()
                .unwrap_or_default();
            numbers.insert(place.root.clone(), mission_numbers(&value));
            campaigns.push((columns.len(), place.root.clone(), is_draft(&value)));
        }
        let id = text(&step, "id").to_string();
        let title = match StepKind::of(&step) {
            Some(StepKind::EarlyAccess) => "Early access ends".to_string(),
            _ => say(ctx, &place.root, text(&step, "title")),
        };
        if StepKind::of(&step) == Some(StepKind::Chapter) {
            chapters.push((columns.len(), title));
            continue;
        }
        let outline = read(ctx, &place.root, &id);
        let number = numbers[&place.root].get(place.index).copied().flatten();
        columns.push(Column {
            root: place.root,
            index: place.index,
            step,
            title,
            number,
            outline,
        });
    }
    // Every character a cast names, by first appearance.
    let mut lanes: Vec<String> = Vec::new();
    for c in &columns {
        for member in &c.outline.cast {
            if !lanes.contains(&member.who) {
                lanes.push(member.who.clone());
            }
        }
    }
    egui::CentralPanel::default()
        .frame(Frame::new().fill(MAIN).inner_margin(Margin::same(16)))
        .show(ui, |ui| {
            ui.label(
                RichText::new(
                    "Every step of every campaign in the order they play. Click a step to open its outline.",
                )
                .size(11.0)
                .color(FAINT),
            );
            ui.add_space(8.0);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = LABEL + COLUMN * columns.len() as f32 + 20.0;
                    let height = 28.0 + 24.0 + CARD + 12.0 + LANE * lanes.len() as f32 + CURVE + 40.0;
                    let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                    board(ui, ctx, state, &here, area, &columns, &chapters, &campaigns, &lanes, &all);
                    ui.add_space(12.0);
                    for warning in warnings(&columns) {
                        ui.label(RichText::new(warning).size(12.0).color(super::view::DRAFT));
                    }
                });
        });
}

fn say(ctx: &mut Ctx, root: &str, value: &str) -> String {
    ctx.store
        .get(&super::text_file(root))
        .ok()
        .flatten()
        .and_then(|t| t.get(value).and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| value.to_string())
}

#[allow(clippy::too_many_arguments)]
fn board(
    ui: &mut Ui,
    ctx: &mut Ctx,
    state: &mut State,
    here: &str,
    area: Rect,
    columns: &[Column],
    chapters: &[(usize, String)],
    campaigns: &[(usize, String, bool)],
    lanes: &[String],
    all: &[Character],
) {
    let x_of = |c: usize| area.left() + LABEL + COLUMN * c as f32;
    let mut y = area.top();
    let painter = ui.painter().clone();
    // The campaigns.
    for (n, (from, root, draft)) in campaigns.iter().enumerate() {
        let to = campaigns.get(n + 1).map_or(columns.len(), |c| c.0);
        let r = Rect::from_min_max(pos2(x_of(*from) + 2.0, y), pos2(x_of(to) - 2.0, y + 22.0));
        painter.rect_filled(r, 5, SELECTED);
        let name = ctx
            .store
            .get(&format!("{root}/package.json"))
            .ok()
            .flatten()
            .and_then(|m| m.get("name").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| super::id_of(root).to_string());
        painter.text(
            r.left_center() + vec2(8.0, 0.0),
            Align2::LEFT_CENTER,
            if *draft {
                format!("{name} · draft")
            } else {
                name
            },
            FontId::proportional(12.0),
            TEXT,
        );
    }
    painter.text(
        pos2(area.left(), y + 11.0),
        Align2::LEFT_CENTER,
        "CAMPAIGNS",
        FontId::proportional(9.5),
        FAINT,
    );
    y += 28.0;
    // The chapters.
    for (n, (from, title)) in chapters.iter().enumerate() {
        let to = chapters.get(n + 1).map_or(columns.len(), |c| c.0).min(
            campaigns
                .iter()
                .map(|c| c.0)
                .find(|c| *c > *from)
                .unwrap_or(columns.len()),
        );
        if to <= *from {
            continue;
        }
        let r = Rect::from_min_max(pos2(x_of(*from) + 2.0, y), pos2(x_of(to) - 2.0, y + 18.0));
        painter.rect_filled(r, 4, if n % 2 == 0 { BUBBLE } else { RAISED });
        painter.with_clip_rect(r).text(
            r.left_center() + vec2(6.0, 0.0),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(11.0),
            SOFT,
        );
    }
    painter.text(
        pos2(area.left(), y + 9.0),
        Align2::LEFT_CENTER,
        "CHAPTERS",
        FontId::proportional(9.5),
        FAINT,
    );
    y += 24.0;
    // The steps.
    painter.text(
        pos2(area.left(), y + 10.0),
        Align2::LEFT_TOP,
        "STEPS",
        FontId::proportional(9.5),
        FAINT,
    );
    let mut line_x = None;
    for (c, column) in columns.iter().enumerate() {
        let kind = StepKind::of(&column.step);
        let r = Rect::from_min_size(pos2(x_of(c) + 3.0, y), vec2(COLUMN - 6.0, CARD));
        let response = ui.interact(r, egui::Id::new(("read-through", c)), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &column.title)
        });
        if kind == Some(StepKind::EarlyAccess) {
            line_x = Some(r.center().x);
            painter.text(
                r.center_top() + vec2(-8.0, 30.0),
                Align2::RIGHT_TOP,
                "EARLY ACCESS\nENDS",
                FontId::proportional(11.0),
                TEXT,
            );
        } else {
            let color = kind_color(kind);
            let current = column.root == here && state.selected == Some(column.index);
            if is_draft(&column.step) {
                dashed_rect(
                    &painter,
                    r,
                    Stroke::new(1.0, if current { TEXT } else { CONTROL }),
                );
            } else {
                painter.rect(
                    r,
                    7,
                    RAISED,
                    Stroke::new(1.0, if current { TEXT } else { BORDER }),
                    egui::StrokeKind::Inside,
                );
            }
            painter.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 3.0)), 2, color);
            let kind_text = match (kind, column.number) {
                (Some(StepKind::Mission), Some(n)) => format!("MISSION {n}"),
                (Some(k), _) => k.label().to_uppercase(),
                _ => "STEP".into(),
            };
            let inner = r.shrink2(vec2(8.0, 8.0));
            let clip = painter.with_clip_rect(inner);
            clip.text(
                inner.left_top() + vec2(0.0, 2.0),
                Align2::LEFT_TOP,
                kind_text,
                FontId::proportional(9.5),
                color,
            );
            if is_draft(&column.step) {
                draft_badge(&clip, inner.left_top() + vec2(0.0, 16.0));
            }
            let body = format!(
                "{}\n{}",
                column.title,
                column
                    .outline
                    .pitch
                    .clone()
                    .or(column.outline.purpose.clone())
                    .unwrap_or_default()
            );
            let galley =
                ui.fonts_mut(|f| f.layout(body, FontId::proportional(11.5), BODY, inner.width()));
            clip.galley(inner.left_top() + vec2(0.0, 34.0), galley, BODY);
            if !column.outline.kinds.is_empty() {
                clip.text(
                    inner.left_bottom(),
                    Align2::LEFT_BOTTOM,
                    column.outline.kinds.join(" · ").replace('-', " "),
                    FontId::proportional(10.0),
                    kind_color(Some(StepKind::Mission)),
                );
            }
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if response.clicked() {
            if column.root == here {
                state.selected = Some(column.index);
                state.mode = Mode::Outline;
            } else {
                ctx.shared.requests.push(Request::Show {
                    document: super::document(&column.root),
                    step: Some(column.index),
                    mode: Mode::Outline,
                });
            }
        }
    }
    y += CARD + 12.0;
    // The cast's lanes.
    for who in lanes {
        let lane = Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), LANE));
        painter.line_segment(
            [lane.left_top(), lane.right_top()],
            Stroke::new(1.0, BUBBLE),
        );
        let face =
            Rect::from_center_size(pos2(area.left() + 14.0, lane.center().y), vec2(24.0, 24.0));
        paint_portrait(ui, ctx, all, who, face);
        let name = all
            .iter()
            .find(|c| c.id == *who)
            .map_or(who.as_str(), |c| c.name.as_str());
        painter
            .with_clip_rect(Rect::from_min_size(lane.min, vec2(LABEL - 6.0, LANE)))
            .text(
                pos2(area.left() + 32.0, lane.center().y),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(12.0),
                SOFT,
            );
        for (c, column) in columns.iter().enumerate() {
            let at = pos2(x_of(c) + COLUMN / 2.0, lane.center().y);
            match column
                .outline
                .cast
                .iter()
                .find(|m| m.who == *who)
                .map(|m| m.presence)
            {
                Some(Presence::On) => {
                    painter.circle_filled(at, 6.0, BODY);
                }
                Some(Presence::Mentioned) => {
                    painter.circle_stroke(at, 6.0, Stroke::new(1.5, MUTED));
                }
                Some(Presence::Joins) => mark(&painter, at, "JOINS", TEXT, BG),
                Some(Presence::Leaves) => mark(&painter, at, "LEAVES", ERROR, BG),
                Some(Presence::Open) => {
                    painter.text(
                        at,
                        Align2::CENTER_CENTER,
                        "?",
                        FontId::proportional(14.0),
                        super::view::DRAFT,
                    );
                }
                None => {
                    painter.circle_filled(at, 2.0, SELECTED);
                }
            }
        }
        y += LANE;
    }
    // The tension curve.
    painter.line_segment(
        [pos2(area.left(), y), pos2(area.right(), y)],
        Stroke::new(1.0, BUBBLE),
    );
    painter.text(
        pos2(area.left(), y + 16.0),
        Align2::LEFT_TOP,
        "Tension",
        FontId::proportional(12.0),
        SOFT,
    );
    painter.text(
        pos2(area.left(), y + 32.0),
        Align2::LEFT_TOP,
        "from the outlines",
        FontId::proportional(10.5),
        FAINT,
    );
    let points: Vec<egui::Pos2> = columns
        .iter()
        .enumerate()
        .filter_map(|(c, column)| {
            column.outline.tension.map(|t| {
                pos2(
                    x_of(c) + COLUMN / 2.0,
                    y + CURVE - 10.0 - f32::from(t) * 14.0,
                )
            })
        })
        .collect();
    if points.len() > 1 {
        painter.add(egui::Shape::line(points.clone(), Stroke::new(2.0, BODY)));
    }
    for p in points {
        painter.circle(p, 4.0, BG, Stroke::new(2.0, BODY));
    }
    y += CURVE;
    if let Some(x) = line_x {
        painter.line_segment(
            [pos2(x, area.top() + 28.0), pos2(x, y)],
            Stroke::new(3.0, TEXT),
        );
    }
    let _ = Color32::WHITE;
}

fn mark(painter: &egui::Painter, at: egui::Pos2, text: &str, fill: Color32, ink: Color32) {
    let galley = painter.layout_no_wrap(text.into(), FontId::proportional(9.0), ink);
    let r = Rect::from_center_size(at, galley.size() + vec2(10.0, 4.0));
    painter.rect_filled(r, 3, fill);
    painter.galley(r.min + vec2(5.0, 2.0), galley, ink);
}

/// What the read-through notices: the same kind of mission twice in a row.
fn warnings(columns: &[Column]) -> Vec<String> {
    let missions: Vec<&Column> = columns
        .iter()
        .filter(|c| StepKind::of(&c.step) == Some(StepKind::Mission))
        .collect();
    let mut out = Vec::new();
    for pair in missions.windows(2) {
        let shared: Vec<&String> = pair[0]
            .outline
            .kinds
            .iter()
            .filter(|k| *k != "base" && pair[1].outline.kinds.contains(k))
            .collect();
        if let Some(kind) = shared.first() {
            out.push(format!(
                "{} and {} are both {} missions, back to back",
                pair[0].title,
                pair[1].title,
                kind.replace('-', " ")
            ));
        }
    }
    out
}
