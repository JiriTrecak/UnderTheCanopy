//! The lab's tab: the viewport (the grid, the units, the tools and the debug layers drawn
//! over them) and the panel beside it (the scenario, what Place puts down, groups, orders,
//! the run and its measures).

use super::{FLOOR, Layers, Session, Tool};
use crate::document::{Command, Context, Store};
use crate::library::Kind;
use crate::tabs::Edit;
use crate::theme::*;
use crate::widgets::eyebrow;
use canopy_content::{
    Content, Id,
    game::Kind as DefKind,
    lab::{Lab, LabOrder, LabUnit, OrderKind, SIZES},
    placement::Owner,
};
use canopy_sim::movement::probe::{Outcome, Verdict};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use glam::{Vec3, Vec4};
use serde_json::Value;
use std::sync::Arc;

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Steering vectors are a tick's travel: drawn this many times longer.
const VECTOR_SCALE: f32 = 12.0;
/// The panel's width.
const PANEL: f32 = 300.0;

/// A drag in the viewport.
pub struct Drag {
    kind: DragKind,
}

enum DragKind {
    /// Cells painted so far, closed or opened.
    Paint {
        close: bool,
        cells: std::collections::BTreeSet<(u32, u32)>,
    },
    /// A box of cells from its first corner.
    Cells { close: bool, from: (u32, u32) },
    /// A selection box from its first corner, on the screen.
    Select { from: Pos2, add: bool },
    /// Moving the selection from where the drag began, on the ground.
    Move { from: [f32; 2] },
}

/// Where a hand order went, shown for a moment.
pub struct Mark {
    at: [f32; 2],
    color: Color32,
    until: f64,
}

/// A hand order for the selection.
enum Hand {
    Move { at: [f32; 2], attack: bool },
    Attack(u32),
    Hold,
    Stop,
}

/// Gives the selected units `order` in the running world, as their players would; the run
/// plays on so it shows. Hand orders are not part of the scenario: restarting forgets them.
fn hand(session: &mut Session, order: Hand, append: bool, now: f64) {
    use canopy_sim::{Action, action::CellPoint, action::MAX_ACTORS};
    let Some(run) = &mut session.run else {
        return;
    };
    let mut by_owner: std::collections::BTreeMap<u8, Vec<u32>> = Default::default();
    for &i in &session.selected {
        let Some(id) = run.entities.get(i).copied().flatten() else {
            continue;
        };
        let Some(e) = run.world.state().entities.get(id) else {
            continue;
        };
        if e.unit.is_none() || e.hp.is_some_and(|hp| hp == 0) {
            continue;
        }
        if let Owner::Player(p) = e.owner {
            by_owner.entry(p).or_default().push(id.0);
        }
    }
    let append = append.then_some(true);
    let mut given = false;
    for (player, actors) in by_owner {
        for chunk in actors.chunks(MAX_ACTORS) {
            let actors = chunk.to_vec();
            let action = match &order {
                Hand::Move { at, attack } => Action::Move {
                    actors,
                    destination: CellPoint {
                        x: at[0].max(0.0) as u32,
                        y: at[1].max(0.0) as u32,
                        surface: None,
                    },
                    attack_move: attack.then_some(true),
                    append,
                },
                Hand::Attack(target) => {
                    // A neutral creature is fought only by force.
                    let neutral = run
                        .world
                        .state()
                        .entities
                        .get(canopy_sim::entity::EntityId(*target))
                        .is_some_and(|t| t.owner == Owner::Nobody);
                    Action::Attack {
                        actors,
                        target: *target,
                        force: neutral.then_some(true),
                        append,
                    }
                }
                Hand::Hold => Action::Hold { actors, append },
                Hand::Stop => Action::Stop { actors },
            };
            given |= run.command(player, action);
        }
    }
    if !given {
        return;
    }
    let mark = match &order {
        Hand::Move { at, attack } => Some((*at, if *attack { ERROR } else { LIVE })),
        Hand::Attack(target) => run
            .world
            .state()
            .entities
            .get(canopy_sim::entity::EntityId(*target))
            .map(|t| {
                (
                    [t.position.x as f32 / 1000.0, t.position.y as f32 / 1000.0],
                    ERROR,
                )
            }),
        _ => None,
    };
    if let Some((at, color)) = mark {
        session.marks.push(Mark {
            at,
            color,
            until: now + 0.9,
        });
    }
    if !run.finished() {
        session.playing = true;
    }
}

/// Selects group `name` and walks it to `at` by hand (captures).
pub fn hand_move_group(session: &mut Session, name: &str, at: [f32; 2]) {
    let Some((_, lab)) = &session.lab else {
        return;
    };
    session.selected = (0..lab.units.len())
        .filter(|&i| lab.units[i].group.as_deref() == Some(name))
        .collect();
    session.inspected = session
        .selected
        .first()
        .and_then(|&i| session.run.as_ref()?.entities.get(i).copied().flatten())
        .map(|id| id.0);
    hand(session, Hand::Move { at, attack: false }, false, 0.0);
    session.playing = false;
}

/// The selected units' owners, for telling friend from foe under a right-click.
fn selected_owners(session: &Session) -> Vec<Owner> {
    let Some(run) = &session.run else {
        return Vec::new();
    };
    let mut out: Vec<Owner> = session
        .selected
        .iter()
        .filter_map(|&i| run.entities.get(i).copied().flatten())
        .filter_map(|id| run.world.state().entities.get(id))
        .map(|e| e.owner)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// What the tab asks of the shell this frame.
#[derive(Default)]
pub struct Out {
    /// The viewport's size in physical pixels.
    pub size: Option<[u32; 2]>,
}

/// A colour per owner: the scene's player colours, neutral in grey.
fn owner_color(session: &Session, owner: Owner) -> Color32 {
    match owner {
        Owner::Nobody => MUTED,
        Owner::Player(p) => session
            .scene
            .as_ref()
            .and_then(|(_, s)| s.colors.get(p as usize))
            .map_or(SOFT, |c| {
                let g = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                Color32::from_rgb(g(c[0]), g(c[1]), g(c[2]))
            }),
    }
}

fn owner_label(owner: Owner) -> String {
    match owner {
        Owner::Nobody => "Neutral".into(),
        Owner::Player(p) => format!("Player {}", p + 1),
    }
}

struct Edits<'a> {
    edits: &'a mut Vec<Edit>,
    document: &'a str,
}

impl Edits<'_> {
    fn set(&mut self, label: &str, pointer: &str, value: Value, merge: Option<&str>) {
        self.edits.push(Edit {
            label: label.to_string(),
            context: Context {
                editor: Kind::Lab,
                document: self.document.to_string(),
                pointer: Some(pointer.to_string()),
            },
            commands: vec![Command::Set {
                document: self.document.to_string(),
                pointer: pointer.to_string(),
                value,
                at: None,
            }],
            merge: merge.map(|m| format!("{}{m}", self.document)),
        });
    }

    fn units(&mut self, label: &str, units: &[LabUnit]) {
        self.set(
            label,
            "/units",
            serde_json::to_value(units).expect("units serialise"),
            None,
        );
    }

    fn orders(&mut self, label: &str, orders: &[LabOrder], merge: Option<&str>) {
        self.set(
            label,
            "/orders",
            serde_json::to_value(orders).expect("orders serialise"),
            merge,
        );
    }
}

/// The lab's tab for `session`.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    store: &mut Store,
    edits: &mut Vec<Edit>,
    content: Option<&Arc<Content>>,
    pack: Option<&Arc<canopy_assets::Pack>>,
    look: Option<&canopy_scene::Look>,
    texture: Option<egui::TextureId>,
) -> Out {
    let mut out = Out::default();
    let Some(lab) = session.read(store).cloned() else {
        let text = session
            .error
            .clone()
            .unwrap_or_else(|| "Opening the scenario…".into());
        ui.centered_and_justified(|ui| ui.label(RichText::new(text).color(FAINT)));
        return out;
    };
    if let (Some(content), Some(pack), Some(look)) = (content, pack, look) {
        session.prepare(content, pack, look);
    }
    let document = session.document.clone();
    let mut edit = Edits {
        edits,
        document: &document,
    };
    let full = ui.available_rect_before_wrap();
    let panel_rect = Rect::from_min_max(pos2(full.right() - PANEL, full.top()), full.max);
    let view_rect = Rect::from_min_max(full.min, pos2(panel_rect.left() - 1.0, full.bottom()));
    ui.painter().vline(
        panel_rect.left() - 0.5,
        full.y_range(),
        Stroke::new(1.0, BORDER),
    );
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(panel_rect.shrink(12.0)));
    egui::ScrollArea::vertical()
        .id_salt(("lab-panel", &document))
        .auto_shrink([false, false])
        .show(&mut panel, |ui| {
            panel_ui(ui, session, &lab, &mut edit, content);
        });
    let mut viewport = ui.new_child(egui::UiBuilder::new().max_rect(view_rect));
    viewport_ui(
        &mut viewport,
        session,
        &lab,
        &mut edit,
        content,
        texture,
        &mut out,
    );
    ui.allocate_rect(full, Sense::hover());
    out
}

// ----- the viewport ----------------------------------------------------------------------

fn viewport_ui(
    ui: &mut Ui,
    session: &mut Session,
    lab: &Lab,
    edit: &mut Edits,
    content: Option<&Arc<Content>>,
    texture: Option<egui::TextureId>,
    out: &mut Out,
) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    keys(ui, session, lab, edit, &response);
    session.advance(ui.input(|i| i.stable_dt));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, RAISED);
    if session.shown[1].is_none() || session.camera.is_none() {
        let text = session
            .error
            .clone()
            .unwrap_or_else(|| "Setting up the lab…".into());
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(12.0),
            if session.error.is_some() {
                ERROR
            } else {
                FAINT
            },
        );
        return;
    }
    if let Some(texture) = texture {
        painter.image(
            texture,
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let scale = ui.ctx().pixels_per_point();
    out.size = Some([
        (rect.width() * scale) as u32,
        (rect.height() * scale) as u32,
    ]);
    camera_input(ui, session, &response, rect);
    let camera = session.camera.expect("camera");
    let aspect = rect.width() / rect.height().max(1.0);
    let matrix = camera.matrix(aspect);
    let project = |p: Vec3| crate::map::view::project(matrix, rect, p);
    let ground = |p: Pos2| -> Option<[f32; 2]> {
        let ndc = vec2(
            (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
            1.0 - (p.y - rect.top()) / rect.height() * 2.0,
        );
        let inverse = matrix.inverse();
        let far = inverse * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let near = inverse * Vec4::new(ndc.x, ndc.y, -1.0, 1.0);
        let (far, near) = (far.truncate() / far.w, near.truncate() / near.w);
        let d = (far - near).normalize_or_zero();
        (d.y < -1e-4).then(|| {
            let t = (FLOOR - near.y) / d.y;
            let g = near + d * t;
            [g.x, g.z]
        })
    };
    let hover = response.hover_pos().and_then(ground);
    tools(
        ui, session, lab, edit, content, &response, rect, &ground, &project,
    );
    overlays(&painter, session, lab, hover, &project, rect);
    tool_strip(ui, session, rect);
    status(&painter, session, lab, rect);
}

fn keys(ui: &Ui, session: &mut Session, lab: &Lab, edit: &mut Edits, response: &egui::Response) {
    if !response.hovered() || ui.ctx().memory(|m| m.focused().is_some()) {
        return;
    }
    let (space, period, restart, delete, escape, modifiers) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Space),
            i.key_pressed(egui::Key::Period),
            i.key_pressed(egui::Key::R),
            i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
            i.key_pressed(egui::Key::Escape),
            i.modifiers,
        )
    });
    if modifiers.command {
        return;
    }
    for tool in Tool::ALL {
        if ui.input(|i| i.key_pressed(tool.key())) {
            session.tool = tool;
            session.armed = false;
        }
    }
    let now = ui.input(|i| i.time);
    if !session.selected.is_empty() {
        if ui.input(|i| i.key_pressed(egui::Key::A)) {
            session.armed = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::H)) {
            hand(session, Hand::Hold, modifiers.shift, now);
        }
    }
    if space && session.run.is_some() {
        session.playing = !session.playing;
    }
    if period {
        session.playing = false;
        session.step();
    }
    if restart {
        session.restart();
    }
    if escape {
        if session.armed {
            session.armed = false;
        } else {
            session.selected.clear();
        }
    }
    if delete && !session.selected.is_empty() {
        let units: Vec<LabUnit> = lab
            .units
            .iter()
            .enumerate()
            .filter(|(i, _)| !session.selected.contains(i))
            .map(|(_, u)| u.clone())
            .collect();
        session.selected.clear();
        edit.units("Remove units", &units);
    }
}

fn camera_input(ui: &Ui, session: &mut Session, response: &egui::Response, rect: Rect) {
    let Some(camera) = session.camera.as_mut() else {
        return;
    };
    // Right drag turns, middle drag (or Alt with the left button) pans, scroll zooms.
    if response.dragged_by(egui::PointerButton::Secondary) {
        let d = response.drag_delta();
        camera.yaw -= d.x * 0.008;
        camera.pitch = (camera.pitch + d.y * 0.005).clamp(0.25, 1.55);
    }
    let alt = ui.input(|i| i.modifiers.alt);
    if response.dragged_by(egui::PointerButton::Middle)
        || (alt && response.dragged_by(egui::PointerButton::Primary))
    {
        let d = response.drag_delta();
        let k = camera.distance / rect.height().max(1.0) * 1.2;
        let (s, c) = camera.yaw.sin_cos();
        camera.focus.x -= (d.x * c + d.y * s) * k;
        camera.focus.z -= (-d.x * s + d.y * c) * k;
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            camera.distance = (camera.distance * (-scroll * 0.002).exp()).clamp(6.0, 900.0);
        }
    }
}

/// Cells within the brush around `at`.
fn brush_cells(at: [f32; 2], brush: u32, size: u32) -> Vec<(u32, u32)> {
    let w = brush.max(1) as i64;
    let (cx, cz) = (at[0].floor() as i64, at[1].floor() as i64);
    let lo = -(w - 1) / 2;
    let mut out = Vec::new();
    for dz in lo..lo + w {
        for dx in lo..lo + w {
            let (x, z) = (cx + dx, cz + dz);
            if (0..size as i64).contains(&x) && (0..size as i64).contains(&z) {
                out.push((x as u32, z as u32));
            }
        }
    }
    out
}

fn cell_of(at: [f32; 2], size: u32) -> (u32, u32) {
    let c = |v: f32| (v.floor().max(0.0) as u32).min(size - 1);
    (c(at[0]), c(at[1]))
}

/// The scenario's unit nearest the pointer at `p`, within its body.
fn unit_at(session: &Session, p: Pos2, project: &dyn Fn(Vec3) -> Option<Pos2>) -> Option<usize> {
    let run = session.run.as_ref()?;
    let b = session.shown[1].as_ref()?;
    let mut best: Option<(f32, usize)> = None;
    for body in b.bodies.iter().filter(|x| x.hp != Some(0)) {
        let Some(index) = run
            .entities
            .iter()
            .position(|e| e.is_some_and(|e| e.0 == body.id))
        else {
            continue;
        };
        let Some(centre) = project(body.position) else {
            continue;
        };
        let Some(rim) = project(body.position + Vec3::X * body.radius.max(0.6)) else {
            continue;
        };
        let reach = (rim - centre).length().max(6.0);
        let d = (p - centre).length();
        if d <= reach && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, index));
        }
    }
    best.map(|(_, i)| i)
}

#[allow(clippy::too_many_arguments)]
fn tools(
    ui: &Ui,
    session: &mut Session,
    lab: &Lab,
    edit: &mut Edits,
    content: Option<&Arc<Content>>,
    response: &egui::Response,
    rect: Rect,
    ground: &dyn Fn(Pos2) -> Option<[f32; 2]>,
    project: &dyn Fn(Vec3) -> Option<Pos2>,
) {
    let modifiers = ui.input(|i| i.modifiers);
    if modifiers.alt {
        return;
    }
    let pointer = response.interact_pointer_pos();
    let primary = ui.input(|i| i.pointer.primary_down());
    let released = ui.input(|i| i.pointer.primary_released());
    // A drag begins.
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(p) = pointer
    {
        let at = ground(p);
        session.drag = match session.tool {
            Tool::Close | Tool::Open => at.map(|at| {
                let close = (session.tool == Tool::Close) != modifiers.shift;
                Drag {
                    kind: if session.boxed {
                        DragKind::Cells {
                            close,
                            from: cell_of(at, lab.size),
                        }
                    } else {
                        DragKind::Paint {
                            close,
                            cells: Default::default(),
                        }
                    },
                }
            }),
            Tool::Select => {
                let on = unit_at(session, p, project);
                match (on, at) {
                    (Some(i), Some(at)) if session.selected.contains(&i) => Some(Drag {
                        kind: DragKind::Move { from: at },
                    }),
                    _ => Some(Drag {
                        kind: DragKind::Select {
                            from: p,
                            add: modifiers.shift,
                        },
                    }),
                }
            }
            _ => None,
        };
    }
    // Painting goes on.
    if primary
        && let Some(Drag {
            kind: DragKind::Paint { cells, .. },
        }) = &mut session.drag
        && let Some(at) = response.hover_pos().and_then(ground)
    {
        cells.extend(brush_cells(at, session.brush, lab.size));
    }
    // A drag ends.
    if released && let Some(drag) = session.drag.take() {
        let at = response.hover_pos().and_then(ground);
        match drag.kind {
            DragKind::Paint { close, cells } => paint(lab, edit, cells, close),
            DragKind::Cells { close, from } => {
                if let Some(at) = at {
                    let to = cell_of(at, lab.size);
                    let cells = (from.1.min(to.1)..=from.1.max(to.1))
                        .flat_map(|z| (from.0.min(to.0)..=from.0.max(to.0)).map(move |x| (x, z)))
                        .collect();
                    paint(lab, edit, cells, close);
                }
            }
            DragKind::Select { from, add } => {
                if let Some(to) = response.hover_pos() {
                    let area = Rect::from_two_pos(from, to);
                    if !add {
                        session.selected.clear();
                    }
                    for (i, u) in lab.units.iter().enumerate() {
                        let p = Vec3::new(u.x as f32, FLOOR, u.y as f32);
                        let position = session
                            .run
                            .as_ref()
                            .and_then(|r| r.entities.get(i).copied().flatten())
                            .and_then(|id| session.run.as_ref()?.world.state().entities.get(id))
                            .map_or(p, |e| {
                                Vec3::new(
                                    e.position.x as f32 / 1000.0,
                                    FLOOR,
                                    e.position.y as f32 / 1000.0,
                                )
                            });
                        if project(position).is_some_and(|s| area.contains(s))
                            && !session.selected.contains(&i)
                        {
                            session.selected.push(i);
                        }
                    }
                }
            }
            DragKind::Move { from } => {
                if let Some(at) = at {
                    let (dx, dz) = ((at[0] - from[0]).round(), (at[1] - from[1]).round());
                    if dx != 0.0 || dz != 0.0 {
                        let mut units = lab.units.clone();
                        for &i in &session.selected {
                            if let Some(u) = units.get_mut(i) {
                                // Buildings keep to the 4-cell lattice.
                                let building = content
                                    .and_then(|c| c.definition(u.definition.as_str()))
                                    .is_some_and(|d| d.kind == DefKind::Building);
                                let (sx, sz) = if building {
                                    ((dx / 4.0).round() * 4.0, (dz / 4.0).round() * 4.0)
                                } else {
                                    (dx, dz)
                                };
                                let max = f64::from(lab.size - 1);
                                u.x = (u.x + f64::from(sx)).clamp(0.0, max);
                                u.y = (u.y + f64::from(sz)).clamp(0.0, max);
                            }
                        }
                        edit.units("Move units", &units);
                    }
                }
            }
        }
    }
    let now = ui.input(|i| i.time);
    // Hand orders: a right-click moves the selection, or attacks the foe under it; with A
    // armed a left click attack-moves.
    if response.clicked_by(egui::PointerButton::Secondary)
        && !session.selected.is_empty()
        && let Some(p) = pointer
    {
        let owners = selected_owners(session);
        let foe = unit_at(session, p, project)
            .and_then(|i| session.run.as_ref()?.entities.get(i).copied().flatten())
            .filter(|id| {
                session
                    .run
                    .as_ref()
                    .and_then(|r| r.world.state().entities.get(*id))
                    .is_some_and(|e| !owners.contains(&e.owner))
            });
        match (foe, ground(p)) {
            (Some(target), _) => hand(session, Hand::Attack(target.0), modifiers.shift, now),
            (None, Some(at)) => hand(
                session,
                Hand::Move { at, attack: false },
                modifiers.shift,
                now,
            ),
            _ => {}
        }
        return;
    }
    if session.armed
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(at) = pointer.and_then(ground)
    {
        session.armed = false;
        hand(
            session,
            Hand::Move { at, attack: true },
            modifiers.shift,
            now,
        );
        return;
    }
    // Clicks.
    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(p) = pointer
    {
        let at = ground(p);
        match session.tool {
            Tool::Select => {
                let on = unit_at(session, p, project);
                if !modifiers.shift {
                    session.selected.clear();
                }
                if let Some(i) = on {
                    if let Some(k) = session.selected.iter().position(|&s| s == i) {
                        session.selected.remove(k);
                    } else {
                        session.selected.push(i);
                    }
                }
                session.inspected = on
                    .and_then(|i| session.run.as_ref()?.entities.get(i).copied().flatten())
                    .map(|id| id.0);
            }
            Tool::Close | Tool::Open => {
                if let Some(at) = at {
                    let close = (session.tool == Tool::Close) != modifiers.shift;
                    paint(
                        lab,
                        edit,
                        brush_cells(at, session.brush, lab.size)
                            .into_iter()
                            .collect(),
                        close,
                    );
                }
            }
            Tool::Place => {
                if let (Some(at), Some(content)) = (at, content) {
                    place(session, lab, edit, content, at);
                }
            }
            Tool::Order => {
                if let Some(at) = at {
                    give_order(session, lab, edit, Some(at));
                }
            }
        }
    }
    let cursor = match session.tool {
        _ if session.armed => egui::CursorIcon::Crosshair,
        Tool::Select => egui::CursorIcon::Default,
        _ => egui::CursorIcon::Crosshair,
    };
    if response.hovered() && rect.is_positive() {
        ui.ctx().set_cursor_icon(cursor);
    }
}

fn paint(lab: &Lab, edit: &mut Edits, cells: std::collections::BTreeSet<(u32, u32)>, close: bool) {
    if cells.is_empty() {
        return;
    }
    let mut grid = lab.closed_cells();
    let n = lab.size as usize;
    for (x, z) in cells {
        grid[z as usize * n + x as usize] = close;
    }
    let mut changed = lab.clone();
    changed.set_closed_cells(&grid);
    if changed.closed == lab.closed {
        return;
    }
    edit.set(
        if close { "Close cells" } else { "Open cells" },
        "/closed",
        serde_json::to_value(&changed.closed).expect("cells serialise"),
        None,
    );
}

/// Places `session.count` of what Place puts down around `at`, packed in a square.
fn place(session: &mut Session, lab: &Lab, edit: &mut Edits, content: &Content, at: [f32; 2]) {
    let Some(id) = session.placing.clone() else {
        return;
    };
    let Some(d) = content.definition(&id) else {
        return;
    };
    let owner = if session.owner == 0 {
        Owner::Nobody
    } else {
        Owner::Player(session.owner - 1)
    };
    let group = (!session.group.trim().is_empty()).then(|| session.group.trim().to_string());
    let max = f64::from(lab.size - 1);
    let mut units = lab.units.clone();
    let first = units.len();
    if d.kind == DefKind::Building {
        let (w, h) = d.footprint.as_ref().map_or((4, 4), |f| (f.width, f.depth));
        let snap = |v: f64, size: u32| {
            let offset = f64::from(size) / 2.0 - 0.5;
            ((v - offset) / 4.0).round() * 4.0 + offset
        };
        units.push(LabUnit {
            definition: d.id.clone(),
            owner,
            x: snap(f64::from(at[0]), w).clamp(0.0, max),
            y: snap(f64::from(at[1]), h).clamp(0.0, max),
            facing: 0.0,
            group,
        });
    } else {
        let spacing = session
            .run
            .as_ref()
            .and_then(|r| {
                let def = r.world.catalog().find(&id)?;
                Some(r.world.spatial().profiles.get(def).body.spacing)
            })
            .unwrap_or(3.0)
            .max(1.0)
            .ceil();
        let n = session.count.max(1);
        let side = (f64::from(n)).sqrt().ceil() as u32;
        let half = f64::from(side - 1) / 2.0;
        for k in 0..n {
            let (cx, cz) = (f64::from(k % side) - half, f64::from(k / side) - half);
            units.push(LabUnit {
                definition: d.id.clone(),
                owner,
                x: (f64::from(at[0]) + cx * spacing).round().clamp(0.0, max),
                y: (f64::from(at[1]) + cz * spacing).round().clamp(0.0, max),
                facing: 0.0,
                group: group.clone(),
            });
        }
    }
    session.selected = (first..units.len()).collect();
    edit.units(&format!("Place {}", d.name), &units);
}

/// Adds the Order tool's order for its group at the run's tick (at `at`, for orders to a
/// point).
fn give_order(session: &mut Session, lab: &Lab, edit: &mut Edits, at: Option<[f32; 2]>) {
    let groups = lab.groups();
    let group = if groups.contains(&session.order_group.as_str()) {
        session.order_group.clone()
    } else if let Some(first) = groups.first() {
        first.to_string()
    } else {
        return;
    };
    let tick = session
        .run
        .as_ref()
        .map_or(0, |r| r.tick())
        .min(lab.duration.saturating_sub(1));
    let kind = session.order;
    let target = (kind == OrderKind::Attack).then(|| {
        if groups.contains(&session.order_target.as_str()) {
            session.order_target.clone()
        } else {
            groups
                .iter()
                .find(|g| **g != group)
                .map_or(group.clone(), |g| g.to_string())
        }
    });
    let mut orders = lab.orders.clone();
    orders.push(LabOrder {
        tick,
        group,
        order: kind,
        x: kind
            .has_point()
            .then(|| at.map(|a| f64::from(a[0].round())))
            .flatten(),
        y: kind
            .has_point()
            .then(|| at.map(|a| f64::from(a[1].round())))
            .flatten(),
        target,
    });
    orders.sort_by_key(|o| o.tick);
    edit.orders(&format!("Order: {}", kind.label()), &orders, None);
}

// ----- what is drawn over the viewport -------------------------------------------------

fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    if (b - a).length() < 2.0 {
        return;
    }
    painter.arrow(a, b - a, Stroke::new(1.5, color));
}

fn overlays(
    painter: &egui::Painter,
    session: &Session,
    lab: &Lab,
    hover: Option<[f32; 2]>,
    project: &dyn Fn(Vec3) -> Option<Pos2>,
    rect: Rect,
) {
    let layers: Layers = session.layers;
    let at = |x: f32, z: f32| project(Vec3::new(x, FLOOR + 0.06, z));
    let line = |points: &[Vec3], color: Color32, width: f32| {
        let points: Vec<Pos2> = points.iter().filter_map(|&p| project(p)).collect();
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points, Stroke::new(width, color)));
        }
    };
    let size = lab.size as f32;
    // The grid's edge, and the navigation sectors.
    line(
        &[
            Vec3::new(0.0, FLOOR, 0.0),
            Vec3::new(size, FLOOR, 0.0),
            Vec3::new(size, FLOOR, size),
            Vec3::new(0.0, FLOOR, size),
            Vec3::new(0.0, FLOOR, 0.0),
        ],
        CONTROL,
        1.0,
    );
    if layers.sectors {
        let mut v = 16.0;
        while v < size {
            line(
                &[
                    Vec3::new(v, FLOOR + 0.04, 0.0),
                    Vec3::new(v, FLOOR + 0.04, size),
                ],
                hex(0x6366f1).gamma_multiply(0.6),
                1.0,
            );
            line(
                &[
                    Vec3::new(0.0, FLOOR + 0.04, v),
                    Vec3::new(size, FLOOR + 0.04, v),
                ],
                hex(0x6366f1).gamma_multiply(0.6),
                1.0,
            );
            v += 16.0;
        }
    }
    // What the tool would touch.
    if let Some(h) = hover {
        let cell_outline = |x: u32, z: u32, color: Color32| {
            let (x, z) = (x as f32, z as f32);
            line(
                &[
                    Vec3::new(x, FLOOR + 0.05, z),
                    Vec3::new(x + 1.0, FLOOR + 0.05, z),
                    Vec3::new(x + 1.0, FLOOR + 0.05, z + 1.0),
                    Vec3::new(x, FLOOR + 0.05, z + 1.0),
                    Vec3::new(x, FLOOR + 0.05, z),
                ],
                color,
                1.0,
            );
        };
        match (&session.tool, &session.drag) {
            (
                _,
                Some(Drag {
                    kind: DragKind::Cells { close, from },
                }),
            ) => {
                let to = cell_of(h, lab.size);
                let (x0, x1) = (from.0.min(to.0) as f32, from.0.max(to.0) as f32 + 1.0);
                let (z0, z1) = (from.1.min(to.1) as f32, from.1.max(to.1) as f32 + 1.0);
                line(
                    &[
                        Vec3::new(x0, FLOOR + 0.05, z0),
                        Vec3::new(x1, FLOOR + 0.05, z0),
                        Vec3::new(x1, FLOOR + 0.05, z1),
                        Vec3::new(x0, FLOOR + 0.05, z1),
                        Vec3::new(x0, FLOOR + 0.05, z0),
                    ],
                    if *close { DIRTY } else { SOFT },
                    1.5,
                );
            }
            (
                _,
                Some(Drag {
                    kind: DragKind::Paint { close, cells },
                }),
            ) => {
                let color = if *close { DIRTY } else { SOFT };
                for &(x, z) in cells.iter().take(4000) {
                    cell_outline(x, z, color);
                }
            }
            (Tool::Close | Tool::Open, None) => {
                for (x, z) in brush_cells(h, session.brush, lab.size) {
                    cell_outline(x, z, MUTED);
                }
            }
            _ => {}
        }
    }
    if let Some(Drag {
        kind: DragKind::Select { from, .. },
    }) = &session.drag
        && let Some(to) = painter.ctx().pointer_hover_pos()
    {
        let area = Rect::from_two_pos(*from, to).intersect(rect);
        painter.rect_stroke(area, 0, Stroke::new(1.0, SOFT), egui::StrokeKind::Inside);
    }
    // Orders' points.
    if layers.orders {
        for o in &lab.orders {
            if let (Some(x), Some(y)) = (o.x, o.y)
                && let Some(p) = at(x as f32 + 0.5, y as f32 + 0.5)
            {
                let color = if o.order == OrderKind::AttackMove {
                    ERROR
                } else {
                    SOFT
                };
                painter.circle_stroke(p, 6.0, Stroke::new(1.5, color));
                painter.text(
                    p + vec2(9.0, -9.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!(
                        "{} · {} · {:.1}s",
                        o.group,
                        o.order.label(),
                        o.tick as f32 / 40.0
                    ),
                    egui::FontId::proportional(11.0),
                    color,
                );
            }
        }
    }
    // Where hand orders went.
    let now = painter.ctx().input(|i| i.time);
    for m in session.marks.iter().filter(|m| m.until > now) {
        let fade = ((m.until - now) / 0.9) as f32;
        if let Some(p) = at(m.at[0], m.at[1]) {
            painter.circle_stroke(
                p,
                5.0 + 10.0 * (1.0 - fade),
                Stroke::new(2.0, m.color.gamma_multiply(fade)),
            );
        }
    }
    let Some(run) = &session.run else {
        return;
    };
    let world = &run.world;
    let spatial = world.spatial();
    let probe = world.probe();
    let state = world.state();
    let cells = |m: i32| m as f32 / 1000.0;
    for e in state.entities.iter() {
        let Some(u) = e.unit.as_ref() else {
            continue;
        };
        if e.hp.is_some_and(|hp| hp == 0) {
            continue;
        }
        let here = Vec3::new(cells(e.position.x), FLOOR + 0.08, cells(e.position.y));
        let inspected = session.inspected == Some(e.id.0);
        if layers.bodies {
            let d = world.catalog().get(e.definition);
            let size = d.dimensions.as_ref().and_then(|x| x.size);
            let color = match size {
                Some(canopy_content::game::BodySize::Small) => hex(0x38bdf8),
                Some(canopy_content::game::BodySize::Medium) => hex(0x34d399),
                Some(canopy_content::game::BodySize::Large) => hex(0xfbbf24),
                Some(canopy_content::game::BodySize::Huge) => hex(0xf472b6),
                None => MUTED,
            };
            let r = spatial.body_of(e).radius as f32;
            let points: Vec<Pos2> = (0..=32)
                .filter_map(|k| {
                    let a = k as f32 / 32.0 * std::f32::consts::TAU;
                    project(here + Vec3::new(a.cos() * r, 0.0, a.sin() * r))
                })
                .collect();
            painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
            if let Some(p) = project(here + Vec3::new(0.0, 0.0, r + 1.2)) {
                painter.text(
                    p,
                    egui::Align2::CENTER_TOP,
                    &d.name,
                    egui::FontId::proportional(10.0),
                    color,
                );
            }
        }
        if layers.routes && !u.route.is_empty() {
            let mut points = vec![here];
            for &n in &u.route {
                let p = spatial.point(n);
                points.push(Vec3::new(cells(p.x), FLOOR + 0.08, cells(p.y)));
            }
            let color = if inspected {
                TEXT
            } else {
                hex(0x38bdf8).gamma_multiply(0.55)
            };
            line(&points, color, if inspected { 2.0 } else { 1.0 });
            if let Some(last) = points.last().and_then(|&p| project(p)) {
                painter.circle_filled(last, 2.5, color);
            }
        }
        if layers.detours
            && let Some(d) = &u.detour
        {
            let mut points = vec![here];
            points.extend(
                d.points
                    .iter()
                    .map(|p| Vec3::new(cells(p.x), FLOOR + 0.1, cells(p.y))),
            );
            line(&points, hex(0xc084fc), 1.5);
        }
        let Some(trace) = probe.and_then(|p| p.units.get(&e.id)) else {
            continue;
        };
        let origin = project(here);
        let tip = |v: (i64, i64)| {
            project(here + Vec3::new(v.0 as f32, 0.0, v.1 as f32) / 1000.0 * VECTOR_SCALE)
        };
        if layers.tried
            && let Some(o) = origin
        {
            for (v, verdict) in &trace.tried {
                let color = match verdict {
                    Verdict::Terrain => hex(0xef4444),
                    Verdict::Bodies => hex(0xf59e0b),
                    Verdict::Clear => TEXT,
                };
                if let Some(t) = tip(*v) {
                    painter.line_segment([o, t], Stroke::new(1.0, color.gamma_multiply(0.8)));
                }
            }
        }
        if layers.vectors
            && let Some(o) = origin
        {
            for (v, color) in [
                (trace.desired, SOFT.gamma_multiply(0.7)),
                (trace.separation, hex(0x60a5fa)),
                (trace.avoidance, hex(0xfbbf24)),
                (trace.velocity, hex(0x2dd4bf)),
            ] {
                if let Some(t) = tip(v) {
                    arrow(painter, o, t, color);
                }
            }
        }
        if layers.states
            && let Some(o) = origin
        {
            let badge = match trace.outcome {
                Outcome::Blocked => Some(hex(0xef4444)),
                Outcome::Recovering => Some(hex(0xf97316)),
                Outcome::Detour => Some(hex(0xc084fc)),
                Outcome::Replanned | Outcome::Rerouted => Some(hex(0xfacc15)),
                Outcome::Turned => Some(hex(0x94a3b8)),
                Outcome::Settled | Outcome::Moved => None,
            };
            if let Some(color) = badge {
                painter.circle_filled(o + vec2(0.0, -10.0), 3.5, color);
                if trace.stalled >= 12 {
                    painter.text(
                        o + vec2(6.0, -10.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{:.1}s", trace.stalled as f32 / 40.0),
                        egui::FontId::proportional(10.0),
                        color,
                    );
                }
            }
        }
    }
}

fn tool_strip(ui: &mut Ui, session: &mut Session, view: Rect) {
    let id = egui::Id::new(("lab-tools", &session.document));
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(view.center().x, view.bottom() - 14.0))
        .constrain_to(view)
        .show(ui.ctx(), |ui| {
            crate::world::strip_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tool in Tool::ALL {
                        let on = session.tool == tool;
                        let label = format!("{} {:?}", tool.name(), tool.key());
                        if ui
                            .selectable_label(on, RichText::new(label).size(12.0))
                            .on_hover_text(tool.hint())
                            .clicked()
                        {
                            session.tool = tool;
                        }
                    }
                    if matches!(session.tool, Tool::Close | Tool::Open) {
                        ui.separator();
                        for w in [1, 2, 4, 8] {
                            if ui
                                .selectable_label(
                                    !session.boxed && session.brush == w,
                                    format!("{w}"),
                                )
                                .on_hover_text(format!("A brush {w} cells wide"))
                                .clicked()
                            {
                                session.brush = w;
                                session.boxed = false;
                            }
                        }
                        if ui
                            .selectable_label(session.boxed, "Box")
                            .on_hover_text("Drag a box of cells")
                            .clicked()
                        {
                            session.boxed = true;
                        }
                    }
                });
            });
        });
}

fn status(painter: &egui::Painter, session: &Session, lab: &Lab, rect: Rect) {
    let Some(run) = &session.run else {
        return;
    };
    let tick = run.tick();
    let hint = if session.armed {
        "Attack-move: click the ground · Escape cancels"
    } else if !session.selected.is_empty() {
        "Right-click: move there, or attack the enemy under it · A, click: attack-move · H hold · Shift queues"
    } else {
        session.tool.hint()
    };
    let text = format!(
        "{}  {:.1} / {:.1} s  ·  {}",
        if session.playing { "▶" } else { "⏸" },
        tick as f32 / 40.0,
        lab.duration as f32 / 40.0,
        hint
    );
    painter.text(
        rect.left_top() + vec2(12.0, 10.0),
        egui::Align2::LEFT_TOP,
        text,
        egui::FontId::proportional(11.5),
        MUTED,
    );
}

// ----- the panel ------------------------------------------------------------------------

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(eyebrow(title));
    ui.add_space(4.0);
}

fn panel_ui(
    ui: &mut Ui,
    session: &mut Session,
    lab: &Lab,
    edit: &mut Edits,
    content: Option<&Arc<Content>>,
) {
    ui.spacing_mut().item_spacing.y = 6.0;
    section(ui, "Scenario");
    let mut name = lab.name.clone();
    if ui
        .add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY))
        .changed()
    {
        edit.set(
            "Rename scenario",
            "/name",
            Value::String(name),
            Some("/name"),
        );
    }
    let mut description = lab.description.clone();
    if ui
        .add(
            egui::TextEdit::multiline(&mut description)
                .hint_text("What it shows")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        edit.set(
            "Describe scenario",
            "/description",
            Value::String(description),
            Some("/description"),
        );
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Grid").color(MUTED));
        egui::ComboBox::from_id_salt(("lab-size", &session.document))
            .selected_text(format!("{0} × {0}", lab.size))
            .show_ui(ui, |ui| {
                for s in SIZES {
                    if ui
                        .selectable_label(lab.size == s, format!("{s} × {s}"))
                        .clicked()
                        && s != lab.size
                    {
                        edit.set("Grid size", "/size", Value::from(s), None);
                    }
                }
            });
        ui.label(RichText::new("Runs").color(MUTED));
        let mut seconds = lab.duration as f32 / 40.0;
        if ui
            .add(
                egui::DragValue::new(&mut seconds)
                    .range(1.0..=1200.0)
                    .suffix(" s"),
            )
            .changed()
        {
            edit.set(
                "Run length",
                "/duration",
                Value::from((seconds * 40.0).round() as u32),
                Some("/duration"),
            );
        }
    });

    section(ui, "Run");
    ui.horizontal(|ui| {
        let ready = session.run.is_some();
        if ui
            .add_enabled(
                ready,
                egui::Button::new(if session.playing { "Pause" } else { "Play" }),
            )
            .on_hover_text("Space")
            .clicked()
        {
            session.playing = !session.playing;
        }
        if ui
            .add_enabled(ready, egui::Button::new("Step"))
            .on_hover_text("One tick ( . )")
            .clicked()
        {
            session.playing = false;
            session.step();
        }
        if ui
            .button("Restart")
            .on_hover_text("Back to the first tick (R)")
            .clicked()
        {
            session.restart();
        }
    });
    ui.horizontal(|ui| {
        for (speed, label) in [
            (0.25, "¼×"),
            (0.5, "½×"),
            (1.0, "1×"),
            (2.0, "2×"),
            (4.0, "4×"),
        ] {
            if ui
                .selectable_label((session.speed - speed).abs() < 0.01, label)
                .clicked()
            {
                session.speed = speed;
            }
        }
    });
    if let Some(run) = &session.run {
        let t = session.timing;
        ui.label(
            RichText::new(format!(
                "Tick {} · step {:.2} ms (mean {:.2}, worst {:.2})",
                run.tick(),
                t.last,
                if t.steps > 0 {
                    t.total / t.steps as f32
                } else {
                    0.0
                },
                t.worst
            ))
            .color(FAINT)
            .size(11.0),
        );
    }
    if let Some(error) = &session.error {
        ui.label(RichText::new(error).color(ERROR).size(11.5));
    }

    measures(ui, session);

    tuning(ui, session);

    section(ui, "Layers");
    let l = &mut session.layers;
    ui.checkbox(&mut l.routes, "Routes and goals");
    ui.checkbox(&mut l.vectors, "Steering vectors")
        .on_hover_text("Grey: straight at the waypoint · blue: separation · amber: avoidance · teal: what it asked for (×12)");
    ui.checkbox(&mut l.tried, "Headings tried")
        .on_hover_text("White taken · amber a foe or planted ally · red terrain");
    ui.checkbox(&mut l.states, "States")
        .on_hover_text("Red blocked · orange negotiating · purple detouring · yellow replanned · grey turning; with seconds held up");
    ui.checkbox(&mut l.detours, "Detours");
    ui.checkbox(&mut l.sectors, "Navigation sectors");
    ui.checkbox(&mut l.orders, "Order points");
    ui.checkbox(&mut l.bodies, "Bodies")
        .on_hover_text("Each unit's collision circle, coloured by its size class");

    inspected(ui, session);

    section(ui, "Place");
    place_panel(ui, session, content);

    section(ui, "Groups");
    groups(ui, session, lab, edit);

    section(ui, "Orders");
    orders(ui, session, lab, edit);
}

/// The movement constants of this session's runs (D100, D101): the game always plays its
/// own; changed ones show in amber, and a change starts the run again.
fn tuning(ui: &mut Ui, session: &mut Session) {
    use canopy_sim::movement::tuning::{Approach, Tuning};
    section(ui, "Tuning");
    let game = Tuning::default();
    let t = &mut session.tuning;
    let mark = |changed: bool, text: &str| {
        RichText::new(text)
            .size(11.5)
            .color(if changed { DIRTY } else { MUTED })
    };
    ui.horizontal(|ui| {
        ui.label(mark(t.approach != game.approach, "Melee approach"));
        egui::ComboBox::from_id_salt(("lab-approach", &session.document))
            .selected_text(t.approach.label())
            .show_ui(ui, |ui| {
                for a in Approach::ALL {
                    ui.selectable_value(&mut t.approach, a, a.label());
                }
            });
    });
    let label = mark(
        t.lead_fleeing != game.lead_fleeing,
        "Lead a fleeing target (ring)",
    );
    ui.checkbox(&mut t.lead_fleeing, label)
    .on_hover_text("Fast melee units aim where a target running away is going and close in front of it. Never on in matches.");
    let label = mark(t.aside != game.aside, "Shoved idle allies step aside");
    ui.checkbox(&mut t.aside, label)
        .on_hover_text("Off: they are pushed straight away from the mover.");
    egui::Grid::new(("lab-tuning", &session.document))
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            let int = |ui: &mut Ui,
                           label: &str,
                           hint: &str,
                           value: &mut i64,
                           default: i64,
                           range: std::ops::RangeInclusive<i64>,
                           suffix: &str| {
                ui.label(mark(*value != default, label)).on_hover_text(hint);
                ui.add(egui::DragValue::new(value).range(range).suffix(suffix));
                ui.end_row();
            };
            int(
                ui,
                "Spacing gap",
                "Room kept between bodies beyond touching (milli-cells)",
                &mut t.gap,
                game.gap,
                0..=1000,
                " m",
            );
            int(
                ui,
                "Arrive within",
                "A unit stops beside allies standing at its goal within this (plus three radii), milli-cells",
                &mut t.arrive,
                game.arrive,
                0..=20_000,
                " m",
            );
            int(
                ui,
                "Look ahead",
                "Ticks of travel ahead a unit eases round bodies",
                &mut t.look_ticks,
                game.look_ticks,
                0..=60,
                " ticks",
            );
            int(
                ui,
                "Most pushed",
                "The most a body is pushed in a tick (milli-cells)",
                &mut t.push_most,
                game.push_most,
                0..=2000,
                " m",
            );
            int(
                ui,
                "Pushed apart to",
                "Room kept beyond touching once pushed apart (milli-cells)",
                &mut t.push_room,
                game.push_room,
                0..=500,
                " m",
            );
            int(
                ui,
                "Squeeze",
                "How much of their overlap two allies on the move resolve a tick (hundredths): lower squeezes more",
                &mut t.squeeze,
                game.squeeze,
                0..=100,
                " %",
            );
            int(
                ui,
                "Ring spacing",
                "Ring spots this many hundredths of a body apart",
                &mut t.ring_spacing,
                game.ring_spacing,
                50..=300,
                " %",
            );
        });
    egui::Grid::new(("lab-tuning-ticks", &session.document))
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for (label, hint, value, default) in [
                (
                    "Push rounds",
                    "How far a shove travels through a crowd in one tick",
                    &mut t.push_rounds,
                    game.push_rounds,
                ),
                (
                    "Settle after",
                    "Ticks blocked beside its goal before a unit settles there",
                    &mut t.settle_ticks,
                    game.settle_ticks,
                ),
                (
                    "Negotiate after",
                    "Ticks held up by a solid body before a yield or detour",
                    &mut t.negotiate_ticks,
                    game.negotiate_ticks,
                ),
                (
                    "Replan after",
                    "Ticks held up before the route is planned again",
                    &mut t.reroute_ticks,
                    game.reroute_ticks,
                ),
            ] {
                ui.label(mark(*value != default, label)).on_hover_text(hint);
                let suffix = if label == "Push rounds" { "" } else { " ticks" };
                ui.add(egui::DragValue::new(value).range(0..=400).suffix(suffix));
                ui.end_row();
            }
        });
    if *t != game && ui.small_button("Back to the game's values").clicked() {
        *t = game;
    }
}

fn measures(ui: &mut Ui, session: &Session) {
    let Some(run) = &session.run else {
        return;
    };
    let m = run.measures();
    section(ui, "Measures");
    let seconds = |t: Option<u32>| t.map_or("—".into(), |t| format!("{:.1}s", t as f32 / 40.0));
    egui::Grid::new(("lab-measures", &session.document))
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            for g in &m.groups {
                ui.label(RichText::new(&g.name).strong());
                ui.label(RichText::new(format!("{} of {} alive", g.alive, g.units)).color(MUTED));
                ui.end_row();
                let rows = [
                    (
                        "Arrived",
                        format!(
                            "{} · first {} · last {}",
                            g.arrived,
                            seconds(g.first_arrival),
                            seconds(g.last_arrival)
                        ),
                    ),
                    (
                        "Held up",
                        format!(
                            "{:.1} unit-s · longest {:.1}s",
                            g.stuck_ticks as f32 / 40.0,
                            g.longest_stuck as f32 / 40.0
                        ),
                    ),
                    ("Course flips", g.flips.to_string()),
                    ("Damage taken", g.damage_taken.to_string()),
                    (
                        "Fighting",
                        format!(
                            "first swing {} · most in reach at once {}",
                            seconds(g.first_swing),
                            g.most_in_reach
                        ),
                    ),
                ];
                for (label, value) in rows {
                    ui.label(RichText::new(label).color(FAINT).size(11.0));
                    ui.label(RichText::new(value).size(11.0));
                    ui.end_row();
                }
            }
            ui.label(RichText::new("Overlap").color(FAINT).size(11.0));
            ui.label(
                RichText::new(format!(
                    "worst {:.2} cells · {} pairs now",
                    m.worst_overlap, m.overlapping
                ))
                .size(11.0),
            );
            ui.end_row();
        });
}

fn inspected(ui: &mut Ui, session: &Session) {
    let Some(id) = session.inspected else {
        return;
    };
    let Some(trace) = session
        .run
        .as_ref()
        .and_then(|r| r.world.probe())
        .and_then(|p| p.units.get(&canopy_sim::entity::EntityId(id)))
    else {
        return;
    };
    section(ui, "Selected unit");
    let v = |(x, y): (i64, i64)| format!("{:.2}, {:.2}", x as f32 / 1000.0, y as f32 / 1000.0);
    for (label, value) in [
        ("State", trace.outcome.label().to_string()),
        ("Held up", format!("{:.2}s", trace.stalled as f32 / 40.0)),
        ("Straight", v(trace.desired)),
        ("Separation", v(trace.separation)),
        ("Avoidance", v(trace.avoidance)),
        ("Asked for", v(trace.velocity)),
        (
            "Headings",
            trace
                .tried
                .iter()
                .map(|(_, verdict)| match verdict {
                    Verdict::Terrain => "terrain",
                    Verdict::Bodies => "bodies",
                    Verdict::Clear => "clear",
                })
                .collect::<Vec<_>>()
                .join(" → "),
        ),
    ] {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(label).color(FAINT).size(11.0));
            ui.label(RichText::new(value).size(11.0));
        });
    }
}

fn place_panel(ui: &mut Ui, session: &mut Session, content: Option<&Arc<Content>>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("For").color(MUTED));
        let owner = if session.owner == 0 {
            Owner::Nobody
        } else {
            Owner::Player(session.owner - 1)
        };
        egui::ComboBox::from_id_salt(("lab-owner", &session.document))
            .selected_text(RichText::new(owner_label(owner)).color(owner_color(session, owner)))
            .show_ui(ui, |ui| {
                for n in 0..=8u8 {
                    let o = if n == 0 {
                        Owner::Nobody
                    } else {
                        Owner::Player(n - 1)
                    };
                    let text = RichText::new(owner_label(o)).color(owner_color(session, o));
                    if ui.selectable_label(session.owner == n, text).clicked() {
                        session.owner = n;
                    }
                }
            });
        ui.label(RichText::new("Count").color(MUTED));
        ui.add(egui::DragValue::new(&mut session.count).range(1..=400));
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Group").color(MUTED));
        ui.add(egui::TextEdit::singleline(&mut session.group).desired_width(120.0));
    });
    ui.add(
        egui::TextEdit::singleline(&mut session.search)
            .hint_text("Search units and buildings")
            .desired_width(f32::INFINITY),
    );
    let Some(catalog) = session.catalog.clone() else {
        return;
    };
    let search = session.search.to_lowercase();
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .id_salt(("lab-place", &session.document))
        .max_height(220.0)
        .show(ui, |ui| {
            for category in catalog
                .categories
                .iter()
                .filter(|c| c.name == "Units" || c.name == "Buildings")
            {
                for group in &category.groups {
                    let entries: Vec<_> = group
                        .entries
                        .iter()
                        .filter(|e| search.is_empty() || e.name.to_lowercase().contains(&search))
                        .collect();
                    if entries.is_empty() {
                        continue;
                    }
                    ui.label(
                        RichText::new(format!("{} · {}", category.name, group.name))
                            .color(FAINT)
                            .size(10.5),
                    );
                    for entry in entries {
                        let crate::world::Placing::Definition(id) = &entry.placing else {
                            continue;
                        };
                        let on = session.placing.as_deref() == Some(id.as_str());
                        let texture = entry.icon.as_deref().and_then(|icon| {
                            session
                                .palette
                                .as_mut()
                                .and_then(|p| p.icon(ui.ctx(), icon))
                        });
                        let response = ui
                            .horizontal(|ui| {
                                if let Some(t) = texture {
                                    ui.add(
                                        egui::Image::new((t, vec2(20.0, 20.0))).corner_radius(3.0),
                                    );
                                }
                                ui.selectable_label(on, &entry.name)
                            })
                            .inner;
                        if response.clicked() {
                            chosen = Some(id.clone());
                        }
                    }
                }
            }
        });
    if let Some(id) = chosen {
        session.placing = Some(id);
        session.tool = Tool::Place;
    }
    if let (Some(id), Some(content)) = (&session.placing, content)
        && let Some(d) = content.definition(id)
    {
        ui.label(
            RichText::new(format!("Placing {} (P, then click)", d.name))
                .color(MUTED)
                .size(11.0),
        );
    }
}

fn groups(ui: &mut Ui, session: &mut Session, lab: &Lab, edit: &mut Edits) {
    let names = lab.groups();
    if names.is_empty() {
        ui.label(
            RichText::new("Units placed with a group name form groups that orders move.")
                .color(FAINT)
                .size(11.0),
        );
    }
    for name in &names {
        let members: Vec<usize> = (0..lab.units.len())
            .filter(|&i| lab.units[i].group.as_deref() == Some(*name))
            .collect();
        let owner = members
            .first()
            .map_or(Owner::Nobody, |&i| lab.units[i].owner);
        ui.horizontal(|ui| {
            ui.label(RichText::new("●").color(owner_color(session, owner)));
            ui.label(RichText::new(*name).strong());
            ui.label(RichText::new(format!("{} units", members.len())).color(MUTED));
            if ui.small_button("Select").clicked() {
                session.selected = members.clone();
                session.tool = Tool::Select;
            }
            if ui.small_button("Orders").clicked() {
                session.order_group = name.to_string();
                session.tool = Tool::Order;
            }
        });
    }
    if !session.selected.is_empty() {
        let now = ui.input(|i| i.time);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Hand orders").color(MUTED));
            if ui
                .small_button("Attack-move")
                .on_hover_text("A, then click the ground")
                .clicked()
            {
                session.armed = true;
            }
            if ui.small_button("Hold").on_hover_text("H").clicked() {
                hand(session, Hand::Hold, false, now);
            }
            if ui.small_button("Stop").clicked() {
                hand(session, Hand::Stop, false, now);
            }
        });
        ui.label(
            RichText::new(
                "Right-click the ground to move them, an enemy to attack it; Shift queues. \
                 Hand orders play now and are not saved: Restart forgets them.",
            )
            .color(FAINT)
            .size(11.0),
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} selected", session.selected.len())).color(MUTED));
            if ui
                .small_button(format!("Into “{}”", session.group.trim()))
                .on_hover_text("Puts the selected units into the Place group")
                .clicked()
            {
                let group = session.group.trim();
                let mut units = lab.units.clone();
                for &i in &session.selected {
                    if let Some(u) = units.get_mut(i) {
                        u.group = (!group.is_empty()).then(|| group.to_string());
                    }
                }
                edit.units("Group units", &units);
            }
            if ui.small_button("Delete").clicked() {
                let units: Vec<LabUnit> = lab
                    .units
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !session.selected.contains(i))
                    .map(|(_, u)| u.clone())
                    .collect();
                session.selected.clear();
                edit.units("Remove units", &units);
            }
        });
    }
}

fn orders(ui: &mut Ui, session: &mut Session, lab: &Lab, edit: &mut Edits) {
    let names: Vec<String> = lab.groups().into_iter().map(str::to_string).collect();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(("lab-order-kind", &session.document))
            .selected_text(session.order.label())
            .show_ui(ui, |ui| {
                for kind in OrderKind::ALL {
                    ui.selectable_value(&mut session.order, kind, kind.label());
                }
            });
        egui::ComboBox::from_id_salt(("lab-order-group", &session.document))
            .selected_text(if session.order_group.is_empty() {
                "group"
            } else {
                session.order_group.as_str()
            })
            .show_ui(ui, |ui| {
                for n in &names {
                    ui.selectable_value(&mut session.order_group, n.clone(), n);
                }
            });
        if session.order == OrderKind::Attack {
            egui::ComboBox::from_id_salt(("lab-order-target", &session.document))
                .selected_text(if session.order_target.is_empty() {
                    "target"
                } else {
                    session.order_target.as_str()
                })
                .show_ui(ui, |ui| {
                    for n in &names {
                        ui.selectable_value(&mut session.order_target, n.clone(), n);
                    }
                });
        }
    });
    if session.order.has_point() {
        ui.label(
            RichText::new("G, then click the ground: the order is given at the run's tick.")
                .color(FAINT)
                .size(11.0),
        );
    } else if ui.button("Add at this tick").clicked() {
        give_order(session, lab, edit, None);
    }
    let mut changed = lab.orders.clone();
    let mut remove = None;
    let mut dirty = false;
    for (i, o) in changed.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut seconds = o.tick as f32 / 40.0;
            if ui
                .add(
                    egui::DragValue::new(&mut seconds)
                        .range(0.0..=(lab.duration as f32 / 40.0))
                        .speed(0.05)
                        .suffix(" s"),
                )
                .changed()
            {
                o.tick = (seconds * 40.0).round() as u32;
                dirty = true;
            }
            ui.label(RichText::new(&o.group).strong());
            ui.label(RichText::new(o.order.label()).color(MUTED));
            let detail = match (o.x, o.y, &o.target) {
                (Some(x), Some(y), _) => format!("({x}, {y})"),
                (_, _, Some(t)) => format!("→ {t}"),
                _ => String::new(),
            };
            ui.label(RichText::new(detail).color(FAINT).size(11.0));
            if ui.small_button("×").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        changed.remove(i);
        edit.orders("Remove order", &changed, None);
    } else if dirty {
        changed.sort_by_key(|o| o.tick);
        edit.orders("Order tick", &changed, Some("/orders-tick"));
    }
}

/// A fresh scenario's first content: two groups facing each other on an open field, for
/// `content`'s first two trainable units (or nothing if it has none).
pub fn starter(name: &str, content: &Content) -> Lab {
    let mut lab = Lab::new(name, 128);
    let unit = content
        .definitions
        .iter()
        .find(|d| d.kind == DefKind::Unit && d.behaviors.combat.is_some() && !d.is_hero());
    if let Some(d) = unit {
        for (group, owner, x) in [
            ("red", Owner::Player(0), 40.0),
            ("blue", Owner::Player(1), 88.0),
        ] {
            for k in 0..9 {
                lab.units.push(LabUnit {
                    definition: Id::new(d.id.as_str()).expect("id"),
                    owner,
                    x: x + f64::from(k % 3) * 4.0,
                    y: 56.0 + f64::from(k / 3) * 4.0,
                    facing: 0.0,
                    group: Some(group.into()),
                });
            }
        }
    }
    lab
}
