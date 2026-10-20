//! The map view: the tool panel, the viewport (the game renderer drawing the editor's
//! scene of the map) with its camera, picking the ground under the pointer, the brush
//! ring, and turning strokes and clicks into undo steps.

use super::tools::{self, Ground, Picked, Stroke, Tier, Tool};
use super::{Camera, Session};
use crate::document::{Command, Context};
use crate::library::Kind;
use crate::tabs::Edit;
use crate::theme::*;
use crate::widgets::eyebrow;
use canopy_content::Content;
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke as Line, Ui, Vec2, pos2, vec2};
use glam::{Mat4, Vec3, Vec4};
use serde_json::{Value, json};

/// Vertical field of view of the map camera (radians).
pub const FOV: f32 = 0.8;

/// What the map view asks of the shell this frame.
#[derive(Default)]
pub struct Out {
    /// The viewport's size in physical pixels: the stage draws the scene at this size.
    pub size: Option<[u32; 2]>,
    pub edits: Vec<Edit>,
    /// The map picker's preview was asked for.
    pub preview: bool,
}

/// The camera's view and projection for a viewport of `aspect`.
pub fn matrices(camera: &Camera, ground: f32, aspect: f32) -> (Vec3, Vec3, Mat4) {
    let (eye, target) = camera.eye_target(ground);
    let distance = camera.distance.max(1.0);
    let view = Mat4::look_at_rh(eye, target, Vec3::Y);
    // A forward projection for picking (the renderer's own is reverse-Z; same frustum).
    let projection = Mat4::perspective_rh(FOV, aspect.max(0.1), distance * 0.02, distance * 6.0);
    (eye, target, projection * view)
}

/// The world point under screen point `p` of `rect`, on the map's ground.
pub(crate) fn pick(
    session: &Session,
    view_projection: Mat4,
    eye: Vec3,
    rect: Rect,
    p: Pos2,
) -> Option<Vec3> {
    let map = session.map.as_ref()?;
    let ndc = vec2(
        (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
        1.0 - (p.y - rect.top()) / rect.height() * 2.0,
    );
    let inverse = view_projection.inverse();
    let far = inverse * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
    let far = far.truncate() / far.w;
    let direction = (far - eye).normalize();
    let size = map.size as f32;
    let above = |p: Vec3| {
        p.y - tools::ground_at(
            map,
            p.x.clamp(-16.0, size + 16.0),
            p.z.clamp(-16.0, size + 16.0),
        )
    };
    let mut t = 0.0;
    let step = 0.5;
    let mut previous = eye;
    for _ in 0..8000 {
        t += step;
        let point = eye + direction * t;
        if above(point) <= 0.0 {
            // Halve towards the crossing.
            let (mut a, mut b) = (previous, point);
            for _ in 0..12 {
                let m = (a + b) * 0.5;
                if above(m) > 0.0 { a = m } else { b = m }
            }
            return Some(b);
        }
        previous = point;
    }
    None
}

/// World → screen point in `rect`.
pub(crate) fn project(view_projection: Mat4, rect: Rect, p: Vec3) -> Option<Pos2> {
    let clip = view_projection * p.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(pos2(
        rect.left() + (ndc.x + 1.0) / 2.0 * rect.width(),
        rect.top() + (1.0 - ndc.y) / 2.0 * rect.height(),
    ))
}

fn tool_button(ui: &mut Ui, tool: Tool, on: bool) -> bool {
    let text = RichText::new(tool.name())
        .size(11.0)
        .color(if on { TEXT } else { MUTED });
    ui.add_sized(
        vec2(62.0, 26.0),
        egui::Button::new(text)
            .fill(if on { SELECTED } else { RAISED })
            .stroke(Line::new(1.0, if on { CONTROL } else { BORDER })),
    )
    .on_hover_text(tool.hint())
    .clicked()
}

/// The tool panel on the map view's left.
fn panel(ui: &mut Ui, session: &mut Session, content: Option<&Content>) -> bool {
    let mut preview = false;
    let t = &mut session.tools;
    ui.label(eyebrow("Tools"));
    egui::Grid::new(("tools", &session.root))
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            for (i, tool) in Tool::ALL.iter().enumerate() {
                if tool_button(ui, *tool, t.tool == *tool) {
                    t.tool = *tool;
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
    ui.label(RichText::new(t.tool.hint()).color(FAINT).size(11.0));
    ui.add_space(6.0);
    if t.tool.is_brush() {
        ui.label(eyebrow("Brush"));
        ui.add(egui::Slider::new(&mut t.radius, 1.0..=40.0).text("Size"));
        ui.add(egui::Slider::new(&mut t.strength, 0.05..=1.0).text("Strength"));
    }
    let map = session.map.as_ref();
    match t.tool {
        Tool::Cliff => {
            ui.add(egui::Slider::new(&mut t.cliff_level, 1.5..=8.0).text("Level height"));
        }
        Tool::Water => {
            ui.add(egui::Slider::new(&mut t.water_level, -16.0..=12.0).text("Water level"));
            ui.label(
                RichText::new("Ground under the water is wet; units wade 0.6 deep at most.")
                    .color(FAINT)
                    .size(11.0),
            );
        }
        Tool::Paint => {
            ui.label(eyebrow("Ground"));
            let mut grounds = vec![
                (Ground::Soil, "Soil".to_string()),
                (Ground::Grass, "Grass".into()),
                (Ground::Rock, "Rock".into()),
            ];
            if let Some(map) = map {
                for (i, layer) in map.authoring.terrain.paint.iter().enumerate() {
                    let name = layer
                        .material
                        .rsplit('.')
                        .next()
                        .unwrap_or(&layer.material)
                        .to_string();
                    grounds.push((Ground::Paint(i), name));
                }
            }
            for (ground, name) in grounds {
                if ui.selectable_label(t.ground == ground, name).clicked() {
                    t.ground = ground;
                }
            }
        }
        Tool::Trees => {
            t.density = t.density.min(8.0);
            ui.add(egui::Slider::new(&mut t.density, 0.5..=8.0).text("Trees per 100 cells"));
            if let Some(content) = content {
                let trees = tools::tree_kinds(content);
                if t.tree.is_empty() {
                    // The tree the map already uses most, else the first.
                    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
                    if let Some(map) = map {
                        for o in &map.authoring.objects {
                            if trees.contains(&o.asset) {
                                *counts.entry(o.asset.as_str()).or_default() += 1;
                            }
                        }
                    }
                    let most = counts
                        .into_iter()
                        .max_by_key(|(_, n)| *n)
                        .map(|(t, _)| t.to_string());
                    if let Some(tree) = most.or_else(|| trees.first().cloned()) {
                        t.tree = tree;
                    }
                }
                egui::ComboBox::from_label("Tree")
                    .selected_text(t.tree.trim_start_matches("scenery."))
                    .show_ui(ui, |ui| {
                        for tree in trees {
                            let label = tree.trim_start_matches("scenery.").to_string();
                            ui.selectable_value(&mut t.tree, tree, label);
                        }
                    });
            }
        }
        Tool::Props => {
            ui.add(egui::Slider::new(&mut t.density, 1.0..=25.0).text("Density"));
            if let Some(content) = content {
                let props = tools::prop_kinds(content);
                if t.prop.is_empty()
                    && let Some(first) = props.first()
                {
                    t.prop = first.clone();
                }
                egui::ComboBox::from_label("Prop")
                    .selected_text(t.prop.trim_start_matches("scenery."))
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        for prop in props {
                            let label = prop.trim_start_matches("scenery.").to_string();
                            ui.selectable_value(&mut t.prop, prop, label);
                        }
                    });
            }
        }
        Tool::Amber => {
            ui.add(
                egui::Slider::new(&mut t.amber, 500..=6000)
                    .step_by(100.0)
                    .text("Amber a node"),
            );
        }
        Tool::Camp => {
            ui.label(eyebrow("Strength"));
            ui.horizontal(|ui| {
                for tier in Tier::ALL {
                    if ui.selectable_label(t.tier == tier, tier.name()).clicked() {
                        t.tier = tier;
                    }
                }
            });
        }
        _ => {}
    }
    if let Some(map) = session.map.as_ref() {
        ui.add_space(10.0);
        ui.label(eyebrow("Map"));
        let mines = map
            .entities
            .iter()
            .filter(|e| e.id.starts_with("mine.") || e.definition.as_str().contains("mine"))
            .count();
        let rows = [
            ("Size", format!("{0} × {0}", map.size)),
            ("Biome", map.biome().to_string()),
            ("Starts", map.player_starts.len().to_string()),
            ("Camps", map.camps.len().to_string()),
            ("Amber nodes", mines.to_string()),
            ("Trees and props", map.authoring.objects.len().to_string()),
        ];
        egui::Grid::new(("map-facts", &session.root))
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(RichText::new(k).color(MUTED).size(11.0));
                    ui.label(RichText::new(v).color(SOFT).size(11.0));
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        preview = ui
            .button("Write preview")
            .on_hover_text("The map picker's picture (preview.webp), from the map as it is now; cook to see it in the game")
            .clicked();
    }
    if let Some(error) = &session.error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).color(ERROR).size(11.0));
    }
    preview
}

/// The nearest start, entity or object to world (`x`, `z`) within 4 cells.
fn nearest(session: &Session, x: f32, z: f32) -> Option<(Picked, String)> {
    let map = session.map.as_ref()?;
    let d = |px: f64, pz: f64| (px as f32 - x).powi(2) + (pz as f32 - z).powi(2);
    let mut best: Option<(f32, Picked, String)> = None;
    let mut consider = |dist: f32, kind: Picked, id: String, limit: f32| {
        if dist <= limit * limit && best.as_ref().is_none_or(|b| dist < b.0) {
            best = Some((dist, kind, id));
        }
    };
    for s in &map.player_starts {
        consider(d(s.x, s.z), Picked::Start, s.player.to_string(), 10.0);
    }
    for e in &map.entities {
        consider(
            d(e.position.x, e.position.y),
            Picked::Entity,
            e.id.clone(),
            3.0,
        );
    }
    for o in &map.authoring.objects {
        consider(d(o.x, o.z), Picked::Object, o.id.clone(), 1.5);
    }
    best.map(|b| (b.1, b.2))
}

/// The commands that delete the selection (and, for a camp member, its camp entry).
fn delete(session: &Session, docs: &Docs, picked: Picked, id: &str) -> Vec<Command> {
    let map_doc = session.path("map.json");
    let mut out = Vec::new();
    match picked {
        Picked::Start => {
            if let Some(i) = docs.map["playerStarts"].as_array().and_then(|s| {
                s.iter()
                    .position(|s| s["player"].as_u64() == id.parse().ok())
            }) {
                out.push(Command::Remove {
                    document: map_doc,
                    pointer: format!("/playerStarts/{i}"),
                });
            }
        }
        Picked::Entity => {
            // A camp member takes its whole camp with it.
            let camp = docs.map["camps"].as_array().and_then(|camps| {
                camps.iter().position(|c| {
                    c["members"]
                        .as_array()
                        .is_some_and(|m| m.iter().any(|v| v == id))
                })
            });
            let mut ids: Vec<String> = vec![id.to_string()];
            if let Some(c) = camp {
                ids = docs.map["camps"][c]["members"]
                    .as_array()
                    .map(|m| {
                        m.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            let mut indices: Vec<usize> = docs.map["entities"]
                .as_array()
                .map(|e| {
                    e.iter()
                        .enumerate()
                        .filter(|(_, e)| ids.iter().any(|i| e["id"] == *i))
                        .map(|(i, _)| i)
                        .collect()
                })
                .unwrap_or_default();
            indices.sort_unstable_by(|a, b| b.cmp(a));
            for i in indices {
                out.push(Command::Remove {
                    document: map_doc.clone(),
                    pointer: format!("/entities/{i}"),
                });
            }
            if let Some(c) = camp {
                out.push(Command::Remove {
                    document: map_doc,
                    pointer: format!("/camps/{c}"),
                });
            }
        }
        Picked::Object => {
            if let Some(i) = docs.scenery["objects"]
                .as_array()
                .and_then(|o| o.iter().position(|o| o["id"] == id))
            {
                out.push(Command::Remove {
                    document: session.path("scenery.json"),
                    pointer: format!("/objects/{i}"),
                });
            }
        }
    }
    out
}

/// Removes what lies within `radius` of world (`x`, `z`) as one step: starts, entities
/// (a camp member takes its whole camp) and scenery objects, as `what` names them. What
/// MCP's `map_remove` does.
pub fn remove_area(
    session: &Session,
    docs: &Docs,
    x: f64,
    z: f64,
    radius: f64,
    what: &[&str],
) -> Option<Edit> {
    let inside = |v: &Value, px: &str, pz: &str| {
        let (Some(vx), Some(vz)) = (
            v.pointer(px).and_then(Value::as_f64),
            v.pointer(pz).and_then(Value::as_f64),
        ) else {
            return false;
        };
        (vx - x).powi(2) + (vz - z).powi(2) <= radius * radius
    };
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let (starts, entities, camps, objects) = (
        list(&docs.map["playerStarts"]),
        list(&docs.map["entities"]),
        list(&docs.map["camps"]),
        list(&docs.scenery["objects"]),
    );
    let mut gone_starts: Vec<usize> = Vec::new();
    let mut gone_entities: std::collections::BTreeSet<String> = Default::default();
    let mut gone_camps: Vec<usize> = Vec::new();
    let mut gone_objects: Vec<usize> = Vec::new();
    if what.contains(&"starts") {
        gone_starts = (0..starts.len())
            .filter(|i| inside(&starts[*i], "/x", "/z"))
            .collect();
    }
    if what.contains(&"entities") {
        for e in &entities {
            if inside(e, "/position/x", "/position/y")
                && let Some(id) = e["id"].as_str()
            {
                gone_entities.insert(id.to_string());
            }
        }
        for (i, camp) in camps.iter().enumerate() {
            let members: Vec<String> = list(&camp["members"])
                .iter()
                .filter_map(|m| m.as_str().map(str::to_string))
                .collect();
            if members.iter().any(|m| gone_entities.contains(m)) {
                gone_entities.extend(members);
                gone_camps.push(i);
            }
        }
    }
    if what.contains(&"trees") || what.contains(&"objects") {
        gone_objects = (0..objects.len())
            .filter(|i| inside(&objects[*i], "/x", "/z"))
            .collect();
    }
    let map_doc = session.path("map.json");
    let remove = |document: &str, array: &str, mut indices: Vec<usize>| {
        indices.sort_unstable_by(|a, b| b.cmp(a));
        indices
            .into_iter()
            .map(|i| Command::Remove {
                document: document.to_string(),
                pointer: format!("/{array}/{i}"),
            })
            .collect::<Vec<_>>()
    };
    let entity_indices: Vec<usize> = (0..entities.len())
        .filter(|i| {
            entities[*i]["id"]
                .as_str()
                .is_some_and(|id| gone_entities.contains(id))
        })
        .collect();
    let mut commands = remove(&map_doc, "playerStarts", gone_starts);
    commands.extend(remove(&map_doc, "entities", entity_indices));
    commands.extend(remove(&map_doc, "camps", gone_camps));
    commands.extend(remove(
        &session.path("scenery.json"),
        "objects",
        gone_objects,
    ));
    (!commands.is_empty()).then(|| step(session, "Remove", commands))
}

/// The map package's JSON documents as the store holds them.
pub struct Docs {
    pub map: Value,
    pub scenery: Value,
}

pub(crate) fn step(session: &Session, label: &str, commands: Vec<Command>) -> Edit {
    Edit {
        label: label.to_string(),
        context: Context {
            editor: Kind::Maps,
            document: session.path("map.json"),
            pointer: None,
        },
        commands,
        merge: None,
    }
}

/// The map view of `session`; `texture` is the scene's last picture.
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    content: Option<&Content>,
    docs: Option<&Docs>,
    texture: Option<egui::TextureId>,
) -> Out {
    let mut out = Out::default();
    let mut preview = false;
    egui::Panel::left(egui::Id::new(("map-tools", &session.root)))
        .exact_size(214.0)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| preview = panel(ui, session, content));
        });
    out.preview = preview;
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, RAISED);
    let Some(map) = session.map.as_ref() else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Loading the map…",
            egui::FontId::proportional(12.0),
            FAINT,
        );
        return out;
    };
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
    let ground = tools::ground_at(map, session.camera.focus[0], session.camera.focus[1]);
    let (eye, _, view_projection) = matrices(
        &session.camera,
        ground,
        rect.width() / rect.height().max(1.0),
    );

    // Camera: right or middle drag pans, Alt-drag turns, scroll zooms, arrows move.
    let input = ui.input(|i| i.clone());
    let shift = input.modifiers.shift;
    navigate(&input, &response, rect, &mut session.camera, map.size);
    let pointer = response.hover_pos().or(response.interact_pointer_pos());
    let hit = pointer.and_then(|p| pick(session, view_projection, eye, rect, p));
    let tool = session.tools.tool;

    // Starts and the selection drawn over the picture.
    for start in &map.player_starts {
        if let Some(p) = project(
            view_projection,
            rect,
            Vec3::new(
                start.x as f32,
                tools::ground_at(map, start.x as f32, start.z as f32) + 12.0,
                start.z as f32,
            ),
        ) {
            painter.circle_filled(p, 11.0, Color32::from_black_alpha(160));
            painter.text(
                p,
                egui::Align2::CENTER_CENTER,
                start.player.to_string(),
                egui::FontId::proportional(12.0),
                TEXT,
            );
        }
    }
    if let Some((kind, id)) = &session.tools.selected {
        let at = match kind {
            Picked::Start => map
                .player_starts
                .iter()
                .find(|s| s.player.to_string() == *id)
                .map(|s| (s.x, s.z)),
            Picked::Entity => map
                .entities
                .iter()
                .find(|e| e.id == *id)
                .map(|e| (e.position.x, e.position.y)),
            Picked::Object => map
                .authoring
                .objects
                .iter()
                .find(|o| o.id == *id)
                .map(|o| (o.x, o.z)),
        };
        if let Some((x, z)) = at {
            ring(
                &painter,
                map,
                view_projection,
                rect,
                x as f32,
                z as f32,
                3.0,
                TEXT,
            );
        }
    }

    // The playable area, when the map has one: a dashed line on the ground.
    if let Some(area) = map.playable {
        let (x0, z0) = (area.x as f32, area.z as f32);
        let (x1, z1) = ((area.x + area.width) as f32, (area.z + area.height) as f32);
        let corners = [(x0, z0), (x1, z0), (x1, z1), (x0, z1), (x0, z0)];
        for pair in corners.windows(2) {
            let ((ax, az), (bx, bz)) = (pair[0], pair[1]);
            let steps = (((bx - ax).abs() + (bz - az).abs()) / 4.0).max(1.0) as usize;
            for k in (0..steps).step_by(2) {
                let at = |t: f32| {
                    let (x, z) = (ax + (bx - ax) * t, az + (bz - az) * t);
                    project(
                        view_projection,
                        rect,
                        Vec3::new(x, tools::ground_at(map, x, z) + 0.3, z),
                    )
                };
                if let (Some(p), Some(q)) = (
                    at(k as f32 / steps as f32),
                    at((k + 1) as f32 / steps as f32),
                ) {
                    painter.line_segment(
                        [p, q],
                        Line::new(2.0, Color32::from_rgba_unmultiplied(250, 250, 250, 170)),
                    );
                }
            }
        }
    }

    // The ramp being dragged, from where it began.
    if let Some(stroke) = &session.tools.stroke
        && let (Some(a), Some(b)) = (stroke.from, stroke.to)
    {
        let point = |p: [f32; 2]| {
            project(
                view_projection,
                rect,
                Vec3::new(p[0], tools::ground_at(map, p[0], p[1]) + 0.2, p[1]),
            )
        };
        if let (Some(pa), Some(pb)) = (point(a), point(b)) {
            painter.line_segment([pa, pb], Line::new(3.0, TEXT));
            painter.circle_filled(pa, 4.0, TEXT);
        }
    }

    if let Some(hit) = hit {
        if tool.is_brush() {
            let color = if shift { ERROR } else { TEXT };
            ring(
                &painter,
                map,
                view_projection,
                rect,
                hit.x,
                hit.z,
                session.tools.radius,
                color,
            );
        }
        painter.text(
            rect.left_bottom() + vec2(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{:.0}, {:.0} · height {:.1}", hit.x, hit.z, hit.y),
            egui::FontId::monospace(11.0),
            SOFT,
        );
    }
    painter.text(
        rect.right_bottom() + vec2(-10.0, -10.0),
        egui::Align2::RIGHT_BOTTOM,
        "Right-drag pans · Alt+right-drag turns · scroll zooms",
        egui::FontId::proportional(11.0),
        FAINT,
    );

    // Brush strokes.
    let cliff = content.and_then(|c| session.map.as_ref().and_then(|m| tools::cliff_layer(m, c)));
    if tool.is_brush() && response.dragged_by(egui::PointerButton::Primary) {
        if let Some(hit) = hit {
            if session.tools.stroke.is_none() {
                let ground = hit.y;
                let target = match tool {
                    Tool::Cliff => {
                        let level = session.tools.cliff_level;
                        let base = (ground / level).round() * level;
                        if shift { base - level } else { base + level }
                    }
                    _ => ground,
                };
                session.tools.stroke = Some(Stroke::new(tool, target, shift));
            }
            let dt = input.stable_dt.clamp(0.005, 0.05);
            let mut stroke = session.tools.stroke.take().expect("a stroke");
            if tool == Tool::Ramp {
                // A ramp is laid when the drag ends, from where it began to where it ends.
                stroke.from.get_or_insert([hit.x, hit.z]);
                stroke.to = Some([hit.x, hit.z]);
            } else if let Some(map) = session.map.as_mut() {
                if matches!(tool, Tool::Trees | Tool::Props) {
                    tools::trees(map, &session.tools, &mut stroke, hit.x, hit.z);
                } else {
                    tools::brush(map, &session.tools, &mut stroke, hit.x, hit.z, dt, cliff);
                }
            }
            session.tools.stroke = Some(stroke);
            session.stale = true;
        }
    } else if let Some(mut stroke) = session.tools.stroke.take()
        && {
            if let (Some(a), Some(b), Some(map)) = (stroke.from, stroke.to, session.map.as_mut()) {
                let (ha, hb) = (
                    tools::ground_at(map, a[0], a[1]),
                    tools::ground_at(map, b[0], b[1]),
                );
                tools::ramp(
                    map,
                    &mut stroke,
                    a,
                    ha,
                    b,
                    hb,
                    session.tools.radius * 2.0,
                    cliff,
                );
            }
            true
        }
        && let Some(edit) = finish(session, docs, stroke)
    {
        out.edits.push(edit);
    }

    // Clicks: place, select.
    if response.clicked_by(egui::PointerButton::Primary)
        && let (Some(hit), Some(content), Some(docs)) = (hit, content, docs)
    {
        if tool == Tool::Select {
            session.tools.selected = nearest(session, hit.x, hit.z);
        } else {
            match place(session, content, docs, tool, hit.x as f64, hit.z as f64) {
                Ok(Some(edit)) => out.edits.push(edit),
                Ok(None) => {}
                Err(error) => session.error = Some(error),
            }
        }
    }
    if session.tools.tool == Tool::Select
        && let (Some((kind, id)), Some(docs)) = (session.tools.selected.clone(), docs)
        && response.hovered()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        let commands = delete(session, docs, kind, &id);
        if !commands.is_empty() {
            out.edits.push(step(session, "Delete", commands));
            session.tools.selected = None;
        }
    }
    out
}

/// A ring of `radius` around world (`x`, `z`), following the ground.
#[allow(clippy::too_many_arguments)]
fn ring(
    painter: &egui::Painter,
    map: &canopy_content::map::UtcMap,
    view_projection: Mat4,
    rect: Rect,
    x: f32,
    z: f32,
    radius: f32,
    color: Color32,
) {
    let points: Vec<Pos2> = (0..=40)
        .filter_map(|i| {
            let a = i as f32 / 40.0 * std::f32::consts::TAU;
            let (px, pz) = (x + a.cos() * radius, z + a.sin() * radius);
            project(
                view_projection,
                rect,
                Vec3::new(px, tools::ground_at(map, px, pz) + 0.15, pz),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Line::new(1.5, color)));
    let _ = Vec2::ZERO;
}

/// The step a finished stroke makes: its grid patches and planted or cleared trees.
fn finish(session: &mut Session, docs: Option<&Docs>, mut stroke: Stroke) -> Option<Edit> {
    let root = session.root.clone();
    let mut commands = match session.map.as_mut() {
        Some(map) => stroke.patches(map, &root),
        None => Vec::new(),
    };
    if let Some(docs) = docs {
        let objects = docs.scenery["objects"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        commands.extend(tools::tree_commands(&root, &stroke, &objects));
    }
    stroke.before.clear();
    if commands.is_empty() {
        return None;
    }
    let label = match stroke.tool {
        Tool::Trees if stroke.lower => "Clear trees",
        Tool::Trees => "Plant trees",
        Tool::Props if stroke.lower => "Clear props",
        Tool::Props => "Scatter props",
        Tool::Paint => "Paint the ground",
        Tool::Water if stroke.lower => "Drain water",
        Tool::Water => "Flood",
        Tool::Cliff => "Cliff",
        Tool::Ramp => "Ramp",
        Tool::Smooth => "Smooth",
        Tool::Flatten => "Flatten",
        Tool::Lower => "Lower",
        _ => "Raise",
    };
    session.own = true;
    session.stale = true;
    Some(step(session, label, commands))
}

/// A whole stroke of `settings.tool` along `points` (world x, z), gone over `passes`
/// times, as one step: what an agent's stroke does (MCP). The brush steps along the line
/// a third of its radius at a time. `level` is the height Flatten and Cliff level to and
/// the water surface Water floods to; without it Flatten keeps the first point's height
/// and Cliff lifts it one cliff level.
#[allow(clippy::too_many_arguments)]
pub fn stroke(
    session: &mut Session,
    content: &Content,
    docs: &Docs,
    mut settings: tools::Tools,
    points: &[[f32; 2]],
    level: Option<f32>,
    lower: bool,
    passes: u32,
) -> Result<Option<Edit>, String> {
    let tool = settings.tool;
    if !tool.is_brush() {
        return Err(format!("{} is not a brush", tool.name()));
    }
    let map = session.map.as_mut().ok_or("The map does not load")?;
    let Some(first) = points.first() else {
        return Err("A stroke needs a point".into());
    };
    let cliff = tools::cliff_layer(map, content);
    let ground = tools::ground_at(map, first[0], first[1]);
    let target = match (tool, level) {
        (Tool::Flatten | Tool::Cliff, Some(level)) => level,
        (Tool::Cliff, None) => {
            let step = settings.cliff_level;
            let base = (ground / step).round() * step;
            if lower { base - step } else { base + step }
        }
        _ => ground,
    };
    if tool == Tool::Water
        && let Some(level) = level
    {
        settings.water_level = level;
    }
    if tool == Tool::Props && settings.prop.is_empty() {
        settings.prop = tools::prop_kinds(content)
            .into_iter()
            .next()
            .ok_or("The package has no props")?;
    }
    if tool == Tool::Trees && settings.tree.is_empty() {
        settings.tree = tools::tree_kinds(content)
            .into_iter()
            .next()
            .ok_or("The package has no harvestable trees")?;
    }
    // The line's points a third of a radius apart.
    let spacing = (settings.radius / 3.0).max(0.5);
    let mut along = vec![*first];
    for pair in points.windows(2) {
        let ([ax, az], [bx, bz]) = (pair[0], pair[1]);
        let length = ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt();
        let steps = (length / spacing).ceil().max(1.0) as usize;
        along.extend((1..=steps).map(|i| {
            let t = i as f32 / steps as f32;
            [ax + (bx - ax) * t, az + (bz - az) * t]
        }));
    }
    let mut stroke = Stroke::new(tool, target, lower);
    if tool == Tool::Ramp {
        // From the first point to the last, between the heights there (or `level` at the
        // bottom), as wide as the brush.
        let last = *points.last().expect("a point");
        if points.len() < 2 {
            return Err("A ramp runs from its first point to its last".into());
        }
        let ha = tools::ground_at(map, first[0], first[1]);
        let hb = level.unwrap_or_else(|| tools::ground_at(map, last[0], last[1]));
        tools::ramp(
            map,
            &mut stroke,
            *first,
            ha,
            last,
            hb,
            settings.radius * 2.0,
            cliff,
        );
        return Ok(finish(session, Some(docs), stroke));
    }
    for _ in 0..passes.max(1) {
        for [x, z] in &along {
            if matches!(tool, Tool::Trees | Tool::Props) {
                tools::trees(map, &settings, &mut stroke, *x, *z);
            } else {
                tools::brush(map, &settings, &mut stroke, *x, *z, 0.1, cliff);
            }
        }
    }
    Ok(finish(session, Some(docs), stroke))
}

/// What a click of `tool` at world (`x`, `z`) places: an amber site facing the nearest
/// start, a creep camp, or a start with its amber and a level pad. Shared by the view and
/// MCP.
pub fn place(
    session: &mut Session,
    content: &Content,
    docs: &Docs,
    tool: Tool,
    x: f64,
    z: f64,
) -> Result<Option<Edit>, String> {
    let Some(map) = session.map.as_ref() else {
        return Err("the map is not loaded".into());
    };
    let map_doc = session.path("map.json");
    let entities = docs.map["entities"].as_array().map_or(0, Vec::len);
    let insert = |pointer: &str, index: usize, value: Value| Command::Insert {
        document: map_doc.clone(),
        pointer: pointer.into(),
        index,
        value,
    };
    Ok(match tool {
        Tool::Amber => {
            let toward = map
                .player_starts
                .iter()
                .min_by(|a, b| {
                    let d = |s: &&canopy_content::map::PlayerStart| {
                        (s.x - x).powi(2) + (s.z - z).powi(2)
                    };
                    d(a).total_cmp(&d(b))
                })
                .map_or((map.size as f64 / 2.0, map.size as f64 / 2.0), |s| {
                    (s.x, s.z)
                });
            let nodes = tools::amber_site(map, content, x, z, toward, session.tools.amber);
            if nodes.is_empty() {
                return Err("the content has no amber mine".into());
            }
            let commands = nodes
                .into_iter()
                .enumerate()
                .map(|(k, n)| insert("/entities", entities + k, n))
                .collect();
            Some(step(session, "Place an amber site", commands))
        }
        Tool::Camp => {
            let (members, camp) = tools::camp(map, content, x, z, session.tools.tier);
            if members.is_empty() {
                return Err("the content has no creeps for camps".into());
            }
            let camps = docs.map["camps"].as_array().map_or(0, Vec::len);
            let mut commands: Vec<Command> = members
                .into_iter()
                .enumerate()
                .map(|(k, m)| insert("/entities", entities + k, m))
                .collect();
            commands.push(insert("/camps", camps, camp));
            Some(step(
                session,
                &format!("Place a {} camp", session.tools.tier.name()),
                commands,
            ))
        }
        Tool::Start => {
            let (x, z) = tools::start_position(x, z);
            // The lowest free slot, so starts stay consecutive after one is removed.
            let Some(player) = (1..=8u8).find(|n| map.player_starts.iter().all(|s| s.player != *n))
            else {
                return Err("A map has eight starts at most.".into());
            };
            let setup = content
                .rules
                .races
                .values()
                .filter_map(|r| r.starting_setup.as_ref())
                .next()
                .unwrap_or(&content.rules.starting_setup);
            // The hall turned so its door faces the middle of the map.
            let rotation = tools::facing_middle(content, setup.fort.as_str(), x, z, map.size);
            let mut start = json!({"mainFort": format!("start.player.{player}/main-fort"), "player": player, "setup": setup.id.as_str(), "x": x, "z": z});
            if rotation != 0 {
                start["rotation"] = json!(rotation);
            }
            let starts = docs.map["playerStarts"].as_array().map_or(0, Vec::len);
            let mut commands = vec![insert("/playerStarts", starts, start)];
            // Its amber 26 cells towards the middle, its hollow facing the hall.
            let (mx, mz) = (map.size as f64 / 2.0, map.size as f64 / 2.0);
            let (dx, dz) = (mx - x, mz - z);
            let length = (dx * dx + dz * dz).sqrt().max(1.0);
            let (ax, az) = (x + dx / length * 26.0, z + dz / length * 26.0);
            let nodes = tools::amber_site(map, content, ax, az, (x, z), session.tools.amber);
            commands.extend(
                nodes
                    .into_iter()
                    .enumerate()
                    .map(|(k, n)| insert("/entities", entities + k, n)),
            );
            // A level pad for the hall and its workers.
            let mut pad = Stroke::new(Tool::Flatten, 0.0, false);
            let root = session.root.clone();
            if let Some(map) = session.map.as_mut() {
                tools::level_pad(map, &mut pad, x as f32, z as f32, 16.0);
                commands.extend(pad.patches(map, &root));
            }
            Some(step(session, &format!("Place start {player}"), commands))
        }
        _ => None,
    })
}

/// The viewport camera's controls: right or middle drag pans, Alt-drag turns, scroll
/// zooms, arrows move; the focus stays over the map.
pub(crate) fn navigate(
    input: &egui::InputState,
    response: &egui::Response,
    rect: Rect,
    camera: &mut Camera,
    map_size: u32,
) {
    if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        let d = response.drag_delta();
        let k = camera.distance / rect.height().max(1.0) * 1.6;
        let (s, c) = camera.yaw.sin_cos();
        if input.modifiers.alt {
            camera.yaw -= d.x * 0.01;
            camera.pitch = (camera.pitch + d.y * 0.005).clamp(0.35, 1.5);
        } else {
            camera.focus[0] -= (d.x * c + d.y * s) * k;
            camera.focus[1] -= (-d.x * s + d.y * c) * k;
        }
    }
    if response.hovered() {
        let scroll = input.smooth_scroll_delta.y;
        if scroll != 0.0 {
            camera.distance =
                (camera.distance * (-scroll * 0.002).exp()).clamp(15.0, map_size as f32 * 1.6);
        }
        let speed = camera.distance * input.stable_dt * 0.9;
        let (s, c) = camera.yaw.sin_cos();
        for (key, dx, dz) in [
            (egui::Key::ArrowLeft, -1.0, 0.0),
            (egui::Key::ArrowRight, 1.0, 0.0),
            (egui::Key::ArrowUp, 0.0, -1.0),
            (egui::Key::ArrowDown, 0.0, 1.0),
        ] {
            if input.key_down(key) {
                camera.focus[0] += (dx * c + dz * s) * speed;
                camera.focus[1] += (-dx * s + dz * c) * speed;
            }
        }
    }
    let size = map_size as f32;
    camera.focus = [
        camera.focus[0].clamp(0.0, size),
        camera.focus[1].clamp(0.0, size),
    ];
}
