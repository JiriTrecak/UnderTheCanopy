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
fn pick(session: &Session, view_projection: Mat4, eye: Vec3, rect: Rect, p: Pos2) -> Option<Vec3> {
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
fn project(view_projection: Mat4, rect: Rect, p: Vec3) -> Option<Pos2> {
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
fn panel(ui: &mut Ui, session: &mut Session, content: Option<&Content>) {
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
            ui.add(egui::Slider::new(&mut t.density, 1.0..=25.0).text("Density"));
            if let Some(content) = content {
                let trees: Vec<String> = canopy_content::assets::scenery(&content.manifest)
                    .filter(|s| {
                        content.landscape.iter().any(|a| {
                            a.scenery
                                .as_ref()
                                .is_some_and(|i| i.as_str() == s.id.as_str())
                                && a.harvesting.is_some()
                        })
                    })
                    .map(|s| s.id.to_string())
                    .collect();
                if t.tree.is_empty()
                    && let Some(first) = trees.first()
                {
                    t.tree = first.clone();
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
    }
    if let Some(error) = &session.error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).color(ERROR).size(11.0));
    }
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
            if let Some(i) = docs.map["playerStarts"]
                .as_array()
                .and_then(|s| s.iter().position(|s| s["player"].to_string() == id))
            {
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

/// The map package's JSON documents as the store holds them.
pub struct Docs {
    pub map: Value,
    pub scenery: Value,
}

fn step(session: &Session, label: &str, commands: Vec<Command>) -> Edit {
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
                .show(ui, |ui| panel(ui, session, content));
        });
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
    if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        let d = response.drag_delta();
        let k = session.camera.distance / rect.height().max(1.0) * 1.6;
        let (s, c) = session.camera.yaw.sin_cos();
        if input.modifiers.alt {
            session.camera.yaw -= d.x * 0.01;
            session.camera.pitch = (session.camera.pitch + d.y * 0.005).clamp(0.35, 1.5);
        } else {
            session.camera.focus[0] -= (d.x * c + d.y * s) * k;
            session.camera.focus[1] -= (-d.x * s + d.y * c) * k;
        }
    }
    if response.hovered() {
        let scroll = input.smooth_scroll_delta.y;
        if scroll != 0.0 {
            session.camera.distance = (session.camera.distance * (-scroll * 0.002).exp())
                .clamp(15.0, map.size as f32 * 1.6);
        }
        let speed = session.camera.distance * input.stable_dt * 0.9;
        let (s, c) = session.camera.yaw.sin_cos();
        for (key, dx, dz) in [
            (egui::Key::ArrowLeft, -1.0, 0.0),
            (egui::Key::ArrowRight, 1.0, 0.0),
            (egui::Key::ArrowUp, 0.0, -1.0),
            (egui::Key::ArrowDown, 0.0, 1.0),
        ] {
            if input.key_down(key) {
                session.camera.focus[0] += (dx * c + dz * s) * speed;
                session.camera.focus[1] += (-dx * s + dz * c) * speed;
            }
        }
    }
    let size = map.size as f32;
    session.camera.focus = [
        session.camera.focus[0].clamp(0.0, size),
        session.camera.focus[1].clamp(0.0, size),
    ];

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
            if let Some(map) = session.map.as_mut() {
                if tool == Tool::Trees {
                    tools::trees(map, &session.tools, &mut stroke, hit.x, hit.z);
                } else {
                    tools::brush(map, &session.tools, &mut stroke, hit.x, hit.z, dt, cliff);
                }
            }
            session.tools.stroke = Some(stroke);
            session.stale = true;
        }
    } else if let Some(mut stroke) = session.tools.stroke.take() {
        // The stroke ended: its patches (and planted or cleared trees) as one step.
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
        if !commands.is_empty() {
            let label = match stroke.tool {
                Tool::Trees if stroke.lower => "Clear trees",
                Tool::Trees => "Plant trees",
                Tool::Paint => "Paint the ground",
                Tool::Water if stroke.lower => "Drain water",
                Tool::Water => "Flood",
                Tool::Cliff => "Cliff",
                Tool::Smooth => "Smooth",
                Tool::Flatten => "Flatten",
                Tool::Lower => "Lower",
                _ => "Raise",
            };
            out.edits.push(step(session, label, commands));
            session.own = true;
        }
    }

    // Clicks: place, select.
    if response.clicked_by(egui::PointerButton::Primary)
        && let (Some(hit), Some(content), Some(docs), Some(map)) =
            (hit, content, docs, session.map.as_ref())
    {
        let map_doc = session.path("map.json");
        let entities = docs.map["entities"].as_array().map_or(0, Vec::len);
        let insert = |pointer: &str, index: usize, value: Value| Command::Insert {
            document: map_doc.clone(),
            pointer: pointer.into(),
            index,
            value,
        };
        match tool {
            Tool::Select => session.tools.selected = nearest(session, hit.x, hit.z),
            Tool::Amber => {
                let toward = map
                    .player_starts
                    .iter()
                    .min_by(|a, b| {
                        let d = |s: &&canopy_content::map::PlayerStart| {
                            (s.x - hit.x as f64).powi(2) + (s.z - hit.z as f64).powi(2)
                        };
                        d(a).total_cmp(&d(b))
                    })
                    .map_or((map.size as f64 / 2.0, map.size as f64 / 2.0), |s| {
                        (s.x, s.z)
                    });
                let nodes = tools::amber_site(
                    map,
                    content,
                    hit.x as f64,
                    hit.z as f64,
                    toward,
                    session.tools.amber,
                );
                let commands = nodes
                    .into_iter()
                    .enumerate()
                    .map(|(k, n)| insert("/entities", entities + k, n))
                    .collect();
                out.edits
                    .push(step(session, "Place an amber site", commands));
            }
            Tool::Camp => {
                let (members, camp) =
                    tools::camp(map, content, hit.x as f64, hit.z as f64, session.tools.tier);
                let camps = docs.map["camps"].as_array().map_or(0, Vec::len);
                let mut commands: Vec<Command> = members
                    .into_iter()
                    .enumerate()
                    .map(|(k, m)| insert("/entities", entities + k, m))
                    .collect();
                commands.push(insert("/camps", camps, camp));
                out.edits.push(step(
                    session,
                    &format!("Place a {} camp", session.tools.tier.name()),
                    commands,
                ));
            }
            Tool::Start => {
                let (x, z) = tools::start_position(hit.x as f64, hit.z as f64);
                let player = map
                    .player_starts
                    .iter()
                    .map(|s| s.player)
                    .max()
                    .unwrap_or(0)
                    + 1;
                if player > 8 {
                    session.error = Some("A map has eight starts at most.".into());
                } else {
                    let setup = content
                        .rules
                        .races
                        .values()
                        .filter_map(|r| r.starting_setup.as_ref())
                        .next()
                        .map_or("setup.ants".to_string(), |s| s.id.to_string());
                    let starts = docs.map["playerStarts"].as_array().map_or(0, Vec::len);
                    let mut commands = vec![insert(
                        "/playerStarts",
                        starts,
                        json!({"mainFort": format!("start.player.{player}/main-fort"), "player": player, "setup": setup, "x": x, "z": z}),
                    )];
                    // Its amber 24 cells towards the middle, its hollow facing the hall.
                    let (mx, mz) = (map.size as f64 / 2.0, map.size as f64 / 2.0);
                    let (dx, dz) = (mx - x, mz - z);
                    let length = (dx * dx + dz * dz).sqrt().max(1.0);
                    let (ax, az) = (x + dx / length * 26.0, z + dz / length * 26.0);
                    let nodes =
                        tools::amber_site(map, content, ax, az, (x, z), session.tools.amber);
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
                    out.edits
                        .push(step(session, &format!("Place start {player}"), commands));
                }
            }
            _ => {}
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
