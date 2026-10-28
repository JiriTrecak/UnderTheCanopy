//! The tabletop (D110): a mission's map sketched before it is built, as Blizzard's
//! designers drew theirs. A square of the map's size in cells; brushes draw lines (pen,
//! main road, side path, attack) and areas (high ground, forest, water); stamps mark the
//! start, amber, camps, heroes, objectives, secrets, story beats, shops, springs and words.
//! Drawn in the editor's own colours: it is a working tool, not a picture.

use super::outline::{Character, paint_portrait};
use super::view::Ctx;
use crate::icons::{self, Icon};
use crate::theme::*;
use crate::widgets::*;
use canopy_content::outline::{Brush, Mark, Shape, Sketch, Stamp};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

/// What a drag or click on the board does.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Tool {
    /// Move a stamp; click one to edit it.
    #[default]
    Select,
    Brush(Brush),
    Stamp(Mark),
    Eraser,
}

/// The tabletop between frames.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub tool: Tool,
    /// The stroke being drawn, in cells.
    stroke: Vec<[f32; 2]>,
    /// The stamp being moved and where it is now.
    moving: Option<(usize, [f32; 2])>,
    /// The stamp last clicked.
    selected: Option<usize>,
    /// What new stamps carry: a camp's size, a hero, words.
    count: u32,
    who: String,
    words: String,
}

/// The sizes a sketch can take: the map sizes.
const SIZES: [u32; 6] = [128, 256, 384, 512, 768, 1024];

pub fn brush_label(brush: Brush) -> &'static str {
    match brush {
        Brush::Pen => "Pen",
        Brush::High => "High ground",
        Brush::Forest => "Forest",
        Brush::Water => "Water",
        Brush::Road => "Main road",
        Brush::Path => "Side path",
        Brush::Attack => "Attack",
    }
}

pub fn mark_label(mark: Mark) -> &'static str {
    match mark {
        Mark::Start => "Start",
        Mark::Amber => "Amber",
        Mark::Camp => "Camp",
        Mark::Hero => "Hero",
        Mark::Objective => "Objective",
        Mark::Optional => "Optional",
        Mark::Secret => "Secret",
        Mark::Beat => "Beat",
        Mark::Shop => "Shop",
        Mark::Spring => "Spring",
        Mark::Text => "Words",
    }
}

const ROAD: Color32 = TEXT;
const PATH: Color32 = BODY;
const ATTACK: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
const WATER: Color32 = Color32::from_rgb(0x60, 0xa5, 0xfa);
const FOREST: Color32 = Color32::from_rgb(0xa3, 0xd9, 0xa5);
const AMBER: Color32 = Color32::from_rgb(0xd9, 0xa5, 0x4a);
const OBJECTIVE: Color32 = Color32::from_rgb(0x1d, 0x4e, 0xd8);
const OPTIONAL: Color32 = Color32::from_rgb(0x93, 0xc5, 0xfd);
const CAMP: Color32 = Color32::from_rgb(0xb9, 0x1c, 0x1c);
const BEAT: Color32 = Color32::from_rgb(0xc0, 0x26, 0xd3);

/// The tabletop: its tools above, the board below. Returns a changed sketch and the undo
/// step's label.
pub fn show(
    ui: &mut Ui,
    ctx: &mut Ctx,
    sketch: &Sketch,
    state: &mut State,
    cast: &[Character],
) -> Option<(String, Sketch)> {
    let mut changed: Option<(String, Sketch)> = None;
    if state.count == 0 {
        state.count = 2;
    }
    // The tools.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let tool = |ui: &mut Ui, state: &mut State, icon: Icon, t: Tool, hint: &str| {
            if icon_button(ui, icon, state.tool == t, true)
                .on_hover_text(hint)
                .clicked()
            {
                state.tool = t;
                state.stroke.clear();
            }
        };
        tool(
            ui,
            state,
            Icon::Select,
            Tool::Select,
            "Select: move a stamp, click one to edit it",
        );
        tool(
            ui,
            state,
            Icon::Eraser,
            Tool::Eraser,
            "Eraser: click a stamp or a stroke",
        );
        ui.separator();
        for brush in Brush::ALL {
            if ui
                .selectable_label(state.tool == Tool::Brush(brush), brush_label(brush))
                .on_hover_text(if brush.area() {
                    "Drag around an area"
                } else {
                    "Drag a line"
                })
                .clicked()
            {
                state.tool = Tool::Brush(brush);
                state.stroke.clear();
            }
        }
        ui.separator();
        for mark in Mark::ALL {
            if ui
                .selectable_label(state.tool == Tool::Stamp(mark), mark_label(mark))
                .clicked()
            {
                state.tool = Tool::Stamp(mark);
            }
        }
    });
    // What the tool needs, and the board's size.
    ui.horizontal(|ui| {
        match state.tool {
            Tool::Brush(b) => {
                ui.label(
                    RichText::new(format!("{}: drag on the board", brush_label(b)))
                        .size(11.0)
                        .color(MUTED),
                );
            }
            Tool::Stamp(Mark::Camp) => {
                ui.label(RichText::new("Camp of").size(11.0).color(MUTED));
                ui.add(
                    egui::DragValue::new(&mut state.count)
                        .range(1..=12)
                        .suffix(" creeps"),
                );
            }
            Tool::Stamp(Mark::Hero) => {
                ui.label(RichText::new("Hero").size(11.0).color(MUTED));
                egui::ComboBox::from_id_salt("sketch-hero")
                    .selected_text(
                        cast.iter()
                            .find(|c| c.id == state.who)
                            .map_or("Choose…".to_string(), |c| c.name.clone()),
                    )
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        for c in cast.iter().filter(|c| c.hero) {
                            ui.selectable_value(&mut state.who, c.id.clone(), &c.name);
                        }
                    });
            }
            Tool::Stamp(Mark::Text) => {
                ui.label(RichText::new("Words").size(11.0).color(MUTED));
                ui.add(egui::TextEdit::singleline(&mut state.words).desired_width(200.0));
            }
            Tool::Stamp(m) => {
                ui.label(
                    RichText::new(format!("{}: click on the board", mark_label(m)))
                        .size(11.0)
                        .color(MUTED),
                );
            }
            Tool::Select => {
                ui.label(
                    RichText::new("Drag a stamp to move it")
                        .size(11.0)
                        .color(MUTED),
                );
            }
            Tool::Eraser => {
                ui.label(
                    RichText::new("Click what to remove")
                        .size(11.0)
                        .color(MUTED),
                );
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut size = sketch.size;
            egui::ComboBox::from_id_salt("sketch-size")
                .selected_text(format!("{size} cells"))
                .show_ui(ui, |ui| {
                    for s in SIZES {
                        ui.selectable_value(&mut size, s, format!("{s} cells"));
                    }
                });
            if size != sketch.size {
                // Everything keeps its place on the square.
                let k = size as f32 / sketch.size.max(1) as f32;
                let mut next = sketch.clone();
                next.size = size;
                for shape in &mut next.shapes {
                    shape
                        .points
                        .iter_mut()
                        .for_each(|p| *p = [p[0] * k, p[1] * k]);
                }
                for stamp in &mut next.stamps {
                    stamp.x *= k;
                    stamp.z *= k;
                }
                changed = Some(("Sketch size".into(), next));
            }
        });
    });
    // The selected stamp's words.
    if let Some(n) = state.selected.filter(|n| *n < sketch.stamps.len()) {
        let stamp = &sketch.stamps[n];
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{}:", mark_label(stamp.mark)))
                    .size(11.0)
                    .color(MUTED),
            );
            let mut label = stamp.label.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut label)
                        .hint_text("label")
                        .desired_width(160.0),
                )
                .changed()
            {
                let mut next = sketch.clone();
                next.stamps[n].label = (!label.is_empty()).then_some(label);
                changed = Some(("Stamp label".into(), next));
            }
            if stamp.mark == Mark::Camp {
                let mut count = stamp.count.unwrap_or(2);
                if ui
                    .add(egui::DragValue::new(&mut count).range(1..=12))
                    .changed()
                {
                    let mut next = sketch.clone();
                    next.stamps[n].count = Some(count);
                    changed = Some(("Camp size".into(), next));
                }
            }
            if ui.button("Remove").clicked() {
                let mut next = sketch.clone();
                next.stamps.remove(n);
                state.selected = None;
                changed = Some(("Remove stamp".into(), next));
            }
        });
    }
    ui.add_space(6.0);
    // The board.
    let side = ui
        .available_width()
        .min(ui.available_height() - 26.0)
        .max(200.0);
    let (board, response) = ui.allocate_exact_size(vec2(side, side), Sense::click_and_drag());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "The tabletop"));
    let size = sketch.size.max(1) as f32;
    let scale = side / size;
    let to_screen = |p: [f32; 2]| pos2(board.left() + p[0] * scale, board.top() + p[1] * scale);
    let to_cells = |p: Pos2| -> [f32; 2] {
        [
            ((p.x - board.left()) / scale).clamp(0.0, size),
            ((p.y - board.top()) / scale).clamp(0.0, size),
        ]
    };
    let painter = ui.painter_at(board);
    painter.rect_filled(board, 8, RAISED);
    let cells = 8;
    for k in 1..cells {
        let t = k as f32 / cells as f32;
        let x = board.left() + side * t;
        let y = board.top() + side * t;
        painter.line_segment(
            [pos2(x, board.top()), pos2(x, board.bottom())],
            Stroke::new(1.0, BUBBLE),
        );
        painter.line_segment(
            [pos2(board.left(), y), pos2(board.right(), y)],
            Stroke::new(1.0, BUBBLE),
        );
    }
    painter.rect_stroke(
        board,
        8,
        Stroke::new(1.0, CONTROL),
        egui::StrokeKind::Inside,
    );
    for shape in &sketch.shapes {
        let points: Vec<Pos2> = shape.points.iter().map(|p| to_screen(*p)).collect();
        draw_shape(&painter, shape.brush, &points);
    }
    if let Tool::Brush(b) = state.tool
        && state.stroke.len() > 1
    {
        let points: Vec<Pos2> = state.stroke.iter().map(|p| to_screen(*p)).collect();
        draw_shape(&painter, b, &points);
    }
    for (n, stamp) in sketch.stamps.iter().enumerate() {
        let at = match state.moving {
            Some((m, p)) if m == n => p,
            _ => [stamp.x, stamp.z],
        };
        draw_stamp(
            ui,
            ctx,
            &painter,
            cast,
            stamp,
            to_screen(at),
            state.selected == Some(n),
        );
    }
    // What the pointer does.
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let nearest_stamp = |p: Pos2| {
        sketch
            .stamps
            .iter()
            .enumerate()
            .map(|(n, s)| (n, to_screen([s.x, s.z]).distance(p)))
            .filter(|(_, d)| *d < 16.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    };
    match state.tool {
        Tool::Brush(brush) => {
            if response.drag_started()
                && let Some(p) = pointer
            {
                state.stroke = vec![to_cells(p)];
            }
            if response.dragged()
                && let Some(p) = pointer
            {
                let c = to_cells(p);
                let far = state.stroke.last().is_none_or(|l| {
                    ((l[0] - c[0]).powi(2) + (l[1] - c[1]).powi(2)).sqrt() >= size / 160.0
                });
                if far {
                    state.stroke.push(c);
                }
            }
            if response.drag_stopped() {
                let points = std::mem::take(&mut state.stroke);
                if points.len() > 1 {
                    let mut next = sketch.clone();
                    next.shapes.push(Shape {
                        brush,
                        points: points
                            .iter()
                            .map(|p| [p[0].round(), p[1].round()])
                            .collect(),
                    });
                    changed = Some((format!("Draw {}", brush_label(brush).to_lowercase()), next));
                }
            }
        }
        Tool::Stamp(mark) => {
            if response.clicked()
                && let Some(p) = pointer
            {
                let c = to_cells(p);
                let numbered = |m: Mark| sketch.stamps.iter().filter(|s| s.mark == m).count();
                let label = match mark {
                    Mark::Objective | Mark::Beat => Some((numbered(mark) + 1).to_string()),
                    Mark::Optional => {
                        Some(((b'A' + numbered(mark) as u8 % 26) as char).to_string())
                    }
                    Mark::Text => (!state.words.trim().is_empty()).then(|| state.words.clone()),
                    _ => None,
                };
                // Words need some words first.
                if mark != Mark::Text || label.is_some() {
                    let mut next = sketch.clone();
                    next.stamps.push(Stamp {
                        mark,
                        x: c[0].round(),
                        z: c[1].round(),
                        label,
                        count: (mark == Mark::Camp).then_some(state.count),
                        who: (mark == Mark::Hero && !state.who.is_empty())
                            .then(|| state.who.clone()),
                    });
                    changed = Some((format!("Place {}", mark_label(mark).to_lowercase()), next));
                }
            }
        }
        Tool::Select => {
            if response.drag_started()
                && let Some(p) = pointer
                && let Some(n) = nearest_stamp(p)
            {
                state.moving = Some((n, [sketch.stamps[n].x, sketch.stamps[n].z]));
                state.selected = Some(n);
            }
            if response.dragged()
                && let (Some(p), Some((n, _))) = (pointer, state.moving)
            {
                state.moving = Some((n, to_cells(p)));
            }
            if response.drag_stopped()
                && let Some((n, at)) = state.moving.take()
                && n < sketch.stamps.len()
            {
                let mut next = sketch.clone();
                next.stamps[n].x = at[0].round();
                next.stamps[n].z = at[1].round();
                changed = Some(("Move stamp".into(), next));
            }
            if response.clicked()
                && let Some(p) = pointer
            {
                state.selected = nearest_stamp(p);
            }
        }
        Tool::Eraser => {
            if response.clicked()
                && let Some(p) = pointer
            {
                let mut next = sketch.clone();
                if let Some(n) = nearest_stamp(p) {
                    next.stamps.remove(n);
                    state.selected = None;
                    changed = Some(("Erase stamp".into(), next));
                } else if let Some(n) = nearest_shape(sketch, p, &to_screen) {
                    next.shapes.remove(n);
                    changed = Some(("Erase stroke".into(), next));
                }
            }
        }
    }
    if let Some(n) = state.selected
        && n < sketch.stamps.len()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        && !ui.ctx().egui_wants_keyboard_input()
    {
        let mut next = sketch.clone();
        next.stamps.remove(n);
        state.selected = None;
        changed = Some(("Remove stamp".into(), next));
    }
    changed
}

/// The stroke nearest to `p` on screen, within a few pixels.
fn nearest_shape(sketch: &Sketch, p: Pos2, to_screen: &dyn Fn([f32; 2]) -> Pos2) -> Option<usize> {
    let mut best = None;
    for (n, shape) in sketch.shapes.iter().enumerate() {
        let points: Vec<Pos2> = shape.points.iter().map(|q| to_screen(*q)).collect();
        let mut d = points
            .windows(2)
            .map(|w| segment_distance(p, w[0], w[1]))
            .fold(f32::INFINITY, f32::min);
        if shape.brush.area() && inside(p, &points) {
            d = 0.0;
        }
        if d < 10.0 && best.is_none_or(|(_, b)| d < b) {
            best = Some((n, d));
        }
    }
    best.map(|(n, _)| n)
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

fn inside(p: Pos2, polygon: &[Pos2]) -> bool {
    let mut odd = false;
    let n = polygon.len();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            odd = !odd;
        }
    }
    odd
}

/// Triangles filling a simple polygon (ear clipping); none when it cannot be filled.
fn triangles(polygon: &[Pos2]) -> Vec<[Pos2; 3]> {
    let mut index: Vec<usize> = (0..polygon.len()).collect();
    let area: f32 = (0..polygon.len())
        .map(|i| {
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    if area < 0.0 {
        index.reverse();
    }
    let mut out = Vec::new();
    let mut guard = 0;
    while index.len() > 3 && guard < polygon.len() * polygon.len() {
        guard += 1;
        let n = index.len();
        let mut clipped = false;
        for i in 0..n {
            let (a, b, c) = (
                polygon[index[(i + n - 1) % n]],
                polygon[index[i]],
                polygon[index[(i + 1) % n]],
            );
            let cross = (b - a).x * (c - b).y - (b - a).y * (c - b).x;
            if cross <= 0.0 {
                continue;
            }
            let ear = index.iter().all(|&k| {
                let p = polygon[k];
                p == a || p == b || p == c || !inside(p, &[a, b, c])
            });
            if ear {
                out.push([a, b, c]);
                index.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    if index.len() == 3 {
        out.push([polygon[index[0]], polygon[index[1]], polygon[index[2]]]);
    }
    out
}

fn fill(painter: &egui::Painter, points: &[Pos2], color: Color32) {
    let mut mesh = egui::Mesh::default();
    for [a, b, c] in triangles(points) {
        let at = mesh.vertices.len() as u32;
        for p in [a, b, c] {
            mesh.colored_vertex(p, color);
        }
        mesh.add_triangle(at, at + 1, at + 2);
    }
    painter.add(mesh);
}

fn arrowhead(painter: &egui::Painter, points: &[Pos2], color: Color32, size: f32) {
    let (Some(&tip), Some(&from)) = (points.last(), points.iter().rev().nth(2).or(points.first()))
    else {
        return;
    };
    let dir = (tip - from).normalized();
    if !dir.x.is_finite() {
        return;
    }
    let side = vec2(-dir.y, dir.x);
    let base = tip - dir * size;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base + side * size * 0.6, base - side * size * 0.6],
        color,
        Stroke::NONE,
    ));
}

/// One stroke as its brush draws it.
pub fn draw_shape(painter: &egui::Painter, brush: Brush, points: &[Pos2]) {
    match brush {
        Brush::Pen => {
            painter.add(egui::Shape::line(points.to_vec(), Stroke::new(2.0, SOFT)));
        }
        Brush::High => {
            fill(
                painter,
                points,
                Color32::from_rgba_unmultiplied(161, 161, 170, 22),
            );
            painter.add(egui::Shape::closed_line(
                points.to_vec(),
                Stroke::new(2.5, SOFT),
            ));
        }
        Brush::Forest => {
            fill(
                painter,
                points,
                Color32::from_rgba_unmultiplied(163, 217, 165, 26),
            );
            painter.add(egui::Shape::closed_line(
                points.to_vec(),
                Stroke::new(1.5, FOREST),
            ));
            // Trees scattered over it.
            let bounds = Rect::from_points(points);
            let step = 14.0;
            let mut y = bounds.top() + step / 2.0;
            let mut row = 0;
            while y < bounds.bottom() {
                let mut x =
                    bounds.left() + step / 2.0 + if row % 2 == 1 { step / 2.0 } else { 0.0 };
                while x < bounds.right() {
                    if inside(pos2(x, y), points) {
                        painter.circle_stroke(pos2(x, y), 4.5, Stroke::new(1.1, FOREST));
                    }
                    x += step;
                }
                y += step * 0.86;
                row += 1;
            }
        }
        Brush::Water => {
            fill(
                painter,
                points,
                Color32::from_rgba_unmultiplied(59, 130, 246, 50),
            );
            painter.add(egui::Shape::closed_line(
                points.to_vec(),
                Stroke::new(1.8, WATER),
            ));
        }
        Brush::Road => {
            painter.add(egui::Shape::line(points.to_vec(), Stroke::new(4.0, ROAD)));
            arrowhead(painter, points, ROAD, 12.0);
        }
        Brush::Path => {
            painter.extend(egui::Shape::dashed_line(
                points,
                Stroke::new(2.2, PATH),
                8.0,
                6.0,
            ));
        }
        Brush::Attack => {
            painter.extend(egui::Shape::dashed_line(
                points,
                Stroke::new(3.0, ATTACK),
                9.0,
                6.0,
            ));
            arrowhead(painter, points, ATTACK, 12.0);
        }
    }
}

/// One stamp at `at` on screen.
fn draw_stamp(
    ui: &Ui,
    ctx: &mut Ctx,
    painter: &egui::Painter,
    cast: &[Character],
    stamp: &Stamp,
    at: Pos2,
    selected: bool,
) {
    let label = stamp.label.clone().unwrap_or_default();
    let small = FontId::proportional(12.0);
    match stamp.mark {
        Mark::Start => {
            painter.circle_filled(at, 13.0, TEXT);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at + vec2(0.0, -7.0),
                    at + vec2(7.0, -1.0),
                    at + vec2(7.0, 6.0),
                    at + vec2(-7.0, 6.0),
                    at + vec2(-7.0, -1.0),
                ],
                BG,
                Stroke::NONE,
            ));
        }
        Mark::Amber => {
            let points: Vec<Pos2> = (0..6)
                .map(|k| {
                    let a = std::f32::consts::PI / 3.0 * k as f32 - std::f32::consts::FRAC_PI_2;
                    at + vec2(a.cos(), a.sin()) * 10.0
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(points, AMBER, Stroke::NONE));
        }
        Mark::Camp => {
            painter.circle_filled(at, 11.0, CAMP);
            painter.text(
                at,
                Align2::CENTER_CENTER,
                stamp.count.unwrap_or(2).to_string(),
                small.clone(),
                TEXT,
            );
        }
        Mark::Hero => {
            let r = Rect::from_center_size(at, vec2(28.0, 28.0));
            paint_portrait(ui, ctx, cast, stamp.who.as_deref().unwrap_or_default(), r);
            painter.circle_stroke(at, 14.0, Stroke::new(2.0, TEXT));
        }
        Mark::Objective => {
            let r = Rect::from_center_size(at, vec2(26.0, 26.0));
            painter.rect_filled(r, 4, OBJECTIVE);
            painter.text(
                at,
                Align2::CENTER_CENTER,
                label.clone(),
                FontId::proportional(13.0),
                TEXT,
            );
        }
        Mark::Optional => {
            let r = Rect::from_center_size(at, vec2(24.0, 24.0));
            painter.rect(
                r,
                4,
                RAISED,
                Stroke::new(2.0, OPTIONAL),
                egui::StrokeKind::Inside,
            );
            painter.text(
                at,
                Align2::CENTER_CENTER,
                label.clone(),
                FontId::proportional(13.0),
                OPTIONAL,
            );
        }
        Mark::Secret => {
            painter.circle_filled(at, 12.0, STRONG);
            painter.text(
                at,
                Align2::CENTER_CENTER,
                "?",
                FontId::proportional(13.0),
                TEXT,
            );
        }
        Mark::Beat => {
            painter.circle_filled(at, 10.0, BEAT);
            painter.text(
                at,
                Align2::CENTER_CENTER,
                label.clone(),
                small.clone(),
                TEXT,
            );
        }
        Mark::Shop => {
            let r = Rect::from_center_size(at, vec2(38.0, 20.0));
            painter.rect(
                r,
                4,
                RAISED,
                Stroke::new(1.0, AMBER),
                egui::StrokeKind::Inside,
            );
            painter.text(
                at,
                Align2::CENTER_CENTER,
                "shop",
                FontId::proportional(11.0),
                AMBER,
            );
        }
        Mark::Spring => {
            painter.circle_filled(at, 11.0, Color32::from_rgb(0x1e, 0x3a, 0x5f));
            icons::paint(painter, icons::square(at, 14.0), Icon::Wave, WATER);
        }
        Mark::Text => {
            painter.text(
                at,
                Align2::CENTER_CENTER,
                label.clone(),
                FontId::proportional(15.0),
                SOFT,
            );
        }
    }
    if stamp.mark != Mark::Text
        && stamp.mark != Mark::Objective
        && stamp.mark != Mark::Optional
        && stamp.mark != Mark::Beat
        && !label.is_empty()
    {
        painter.text(
            at + vec2(16.0, -2.0),
            Align2::LEFT_CENTER,
            label,
            small,
            SOFT,
        );
    }
    if selected {
        painter.circle_stroke(at, 18.0, Stroke::new(1.5, crate::theme::DIRTY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_concave_area_fills_with_triangles_covering_it() {
        // An L: six corners, four triangles, the notch left empty.
        let l = [
            pos2(0.0, 0.0),
            pos2(2.0, 0.0),
            pos2(2.0, 1.0),
            pos2(1.0, 1.0),
            pos2(1.0, 2.0),
            pos2(0.0, 2.0),
        ];
        let tris = triangles(&l);
        assert_eq!(tris.len(), 4);
        let area: f32 = tris
            .iter()
            .map(|&[a, b, c]| ((b - a).x * (c - a).y - (b - a).y * (c - a).x).abs() / 2.0)
            .sum();
        assert!((area - 3.0).abs() < 1e-4, "{area}");
        assert!(inside(pos2(0.5, 1.5), &l) && !inside(pos2(1.5, 1.5), &l));
    }
}
