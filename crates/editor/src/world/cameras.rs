//! A mission's saved cameras in the block editor (docs/editor.md "Cameras"): placed from
//! Place → Special → Camera, drawn as a camera body with its view's frustum reaching to
//! the point it looks at, picked by that point or the body, edited in the panel, and
//! looked through (the view shows what the camera sees).

use super::Picked;
use crate::document::Command;
use crate::map::Session;
use crate::map::view::{Docs, project, step};
use crate::tabs::Edit;
use crate::theme::*;
use canopy_content::map::{
    CAMERA_DISTANCE, CAMERA_FOV, CAMERA_HEIGHT, CAMERA_PITCH, CAMERA_ROLL, MapCamera, UtcMap,
};
use egui::{Color32, Pos2, Rect, RichText, Stroke, Ui};
use glam::{Mat4, Quat, Vec3};
use serde_json::{Value, json};

/// The frame the frustum is drawn for (the game's 16:9).
const ASPECT: f32 = 16.0 / 9.0;
/// How near the pointer must come to a camera's body to pick it, points.
const PICK_POINTS: f32 = 16.0;
/// The editor view's own lens (`crate::map::view::FOV`), radians.
const EDITOR_FOV: f32 = crate::map::view::FOV;

/// The map's cameras.
pub fn of(map: &UtcMap) -> &[MapCamera] {
    map.mission
        .as_ref()
        .and_then(|m| m.cameras.as_deref())
        .unwrap_or_default()
}

pub fn find<'a>(map: &'a UtcMap, id: &str) -> Option<(usize, &'a MapCamera)> {
    of(map).iter().enumerate().find(|(_, c)| c.id == id)
}

/// The point looked at and the eye, in the world.
pub fn eye_target(map: &UtcMap, c: &MapCamera) -> (Vec3, Vec3) {
    let (x, z) = (c.x as f32, c.y as f32);
    let ground = crate::map::tools::ground_at(map, x, z).max(0.0);
    let target = Vec3::new(x, ground + c.height.unwrap_or(0.0) as f32, z);
    let (yaw, pitch) = ((c.yaw as f32).to_radians(), (c.pitch as f32).to_radians());
    let offset = Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    (target + offset * c.distance as f32, target)
}

/// The view's axes: forward, right and up (rolled).
fn axes(c: &MapCamera, eye: Vec3, target: Vec3) -> (Vec3, Vec3, Vec3) {
    let forward = (target - eye).normalize_or_zero();
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward);
    let roll = Quat::from_axis_angle(forward, -(c.roll.unwrap_or(0.0) as f32).to_radians());
    (forward, roll * right, roll * up)
}

/// Draws every camera: its body, its frustum to the target's distance and its id; the
/// selected ones gold.
pub fn draw(
    painter: &egui::Painter,
    view_projection: Mat4,
    rect: Rect,
    map: &UtcMap,
    selected: &[(Picked, String)],
) {
    for c in of(map) {
        let on = selected
            .iter()
            .any(|(k, id)| *k == Picked::Camera && *id == c.id);
        let color = if on {
            Color32::from_rgb(255, 214, 120)
        } else {
            Color32::from_rgb(125, 211, 252)
        };
        let faint = color.gamma_multiply(0.55);
        let (eye, target) = eye_target(map, c);
        let (forward, right, up) = axes(c, eye, target);
        let to = |p: Vec3| project(view_projection, rect, p);
        let line = |a: Vec3, b: Vec3, stroke: Stroke| {
            if let (Some(a), Some(b)) = (to(a), to(b)) {
                painter.line_segment([a, b], stroke);
            }
        };
        // The frustum, out to the plane of the point looked at.
        let distance = (target - eye).length();
        let half_h = ((c.fov as f32).to_radians() / 2.0).tan() * distance;
        let half_w = half_h * ASPECT;
        let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(sx, sy)| target + right * (sx * half_w) + up * (sy * half_h));
        for (i, corner) in corners.iter().enumerate() {
            line(eye, *corner, Stroke::new(1.0, faint));
            line(*corner, corners[(i + 1) % 4], Stroke::new(1.5, faint));
        }
        // The frame's top edge, marked so a rolled or upside-down view shows.
        line(
            target + up * half_h * 1.08 - right * half_w * 0.15,
            target + up * half_h * 1.08 + right * half_w * 0.15,
            Stroke::new(2.0, faint),
        );
        // The body: a box behind the lens.
        let size = (distance * 0.04).clamp(0.6, 3.0);
        let back = eye - forward * size * 1.6;
        let box_corners = |at: Vec3, s: f32| {
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(sx, sy)| at + right * (sx * s) + up * (sy * s * 0.7))
        };
        let front = box_corners(eye, size * 0.6);
        let rear = box_corners(back, size);
        for i in 0..4 {
            let j = (i + 1) % 4;
            line(front[i], front[j], Stroke::new(2.0, color));
            line(rear[i], rear[j], Stroke::new(2.0, color));
            line(front[i], rear[i], Stroke::new(2.0, color));
        }
        // The point looked at: a cross on the ground.
        let ground = Vec3::new(
            target.x,
            crate::map::tools::ground_at(map, target.x, target.z),
            target.z,
        );
        line(ground - Vec3::X, ground + Vec3::X, Stroke::new(2.0, color));
        line(ground - Vec3::Z, ground + Vec3::Z, Stroke::new(2.0, color));
        line(ground, target, Stroke::new(1.0, faint));
        if let Some(p) = to(back + up * size) {
            painter.text(
                p + egui::vec2(0.0, -6.0),
                egui::Align2::CENTER_BOTTOM,
                &c.id,
                egui::FontId::proportional(11.0),
                color,
            );
        }
    }
}

/// The camera whose body is under screen point `at`.
pub fn pick(map: &UtcMap, view_projection: Mat4, rect: Rect, at: Pos2) -> Option<String> {
    of(map)
        .iter()
        .filter_map(|c| {
            let (eye, _) = eye_target(map, c);
            let p = project(view_projection, rect, eye)?;
            let d = p.distance(at);
            (d <= PICK_POINTS).then(|| (d, c.id.clone()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// A new camera looking at (`x`, `z`) from the south, a little above the ground, as a
/// close cinematic shot; maps without a mission refuse it.
pub fn place(session: &Session, docs: &Docs, x: f64, z: f64) -> Result<Edit, String> {
    let mission = &docs.map["mission"];
    if !mission.is_object() {
        return Err("Cameras belong to a mission: this map has none".into());
    }
    let cameras = mission["cameras"].as_array().cloned().unwrap_or_default();
    let taken = |id: &str| cameras.iter().any(|c| c["id"] == id);
    let id = (1..)
        .map(|n| format!("camera.{n}"))
        .find(|id| !taken(id))
        .unwrap_or_default();
    let camera = json!({
        "id": id, "x": x, "y": z, "height": 1.5, "distance": 24.0,
        "pitch": 22.0, "yaw": 0.0, "fov": 40.0,
    });
    let document = session.path("map.json");
    let command = if mission.get("cameras").is_some() {
        Command::Insert {
            document,
            pointer: "/mission/cameras".into(),
            index: cameras.len(),
            value: camera,
        }
    } else {
        Command::Set {
            document,
            pointer: "/mission/cameras".into(),
            value: json!([camera]),
            at: None,
        }
    };
    Ok(step(session, "Place a camera", vec![command]))
}

/// The commands that move camera `id` (its target and so all of it) by `delta`.
pub fn shift(session: &Session, docs: &Docs, id: &str, delta: (f64, f64)) -> Vec<Command> {
    let Some((i, c)) = docs.map["mission"]["cameras"]
        .as_array()
        .and_then(|list| list.iter().enumerate().find(|(_, c)| c["id"] == id))
    else {
        return Vec::new();
    };
    let f = |v: &Value| v.as_f64().unwrap_or(0.0);
    let set = |field: &str, value: f64| Command::Set {
        document: session.path("map.json"),
        pointer: format!("/mission/cameras/{i}/{field}"),
        value: value.into(),
        at: None,
    };
    vec![
        set("x", f(&c["x"]) + delta.0),
        set("y", f(&c["y"]) + delta.1),
    ]
}

/// The removal of the cameras among `items`, from the end so the indices hold.
pub fn remove(session: &Session, docs: &Docs, items: &[(Picked, String)]) -> Vec<Command> {
    let list = docs.map["mission"]["cameras"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut gone: Vec<usize> = (0..list.len())
        .filter(|&i| {
            items
                .iter()
                .any(|(k, id)| *k == Picked::Camera && list[i]["id"] == id.as_str())
        })
        .collect();
    gone.sort_unstable_by(|a, b| b.cmp(a));
    gone.into_iter()
        .map(|i| Command::Remove {
            document: session.path("map.json"),
            pointer: format!("/mission/cameras/{i}"),
        })
        .collect()
}

/// The editor's view through camera `c`: the same eye and target, the lens matched by
/// the distance (the editor's view keeps its own field of view).
pub fn look_through(map: &UtcMap, c: &MapCamera) -> crate::map::Camera {
    let (eye, target) = eye_target(map, c);
    let lens = ((c.fov as f32).to_radians() / 2.0).tan() / (EDITOR_FOV / 2.0).tan();
    let distance = (eye - target).length() * lens;
    crate::map::Camera {
        focus: [target.x, target.z],
        distance: distance.max(1.0),
        yaw: (c.yaw as f32).to_radians(),
        pitch: (c.pitch as f32).to_radians(),
    }
}

/// The selected camera's panel: its fields, and Look through.
pub fn panel(ui: &mut Ui, session: &mut Session, id: &str, edits: &mut Vec<Edit>) {
    let Some(map) = session.map.clone() else {
        return;
    };
    let Some((i, c)) = find(&map, id) else {
        return;
    };
    ui.label(
        RichText::new(format!("Camera {}", c.id))
            .size(15.0)
            .color(TEXT)
            .strong(),
    );
    ui.label(
        RichText::new("Scenes cut or blend to it: mission.camera_to(\"id\", seconds)")
            .size(11.0)
            .color(MUTED),
    );
    ui.add_space(8.0);
    let looking = session
        .blocks
        .looking
        .as_ref()
        .is_some_and(|(l, _)| l == id);
    if ui
        .selectable_label(looking, "Look through")
        .on_hover_text("The view shows what the camera sees (the lens matched by distance)")
        .clicked()
    {
        session.blocks.looking = if looking {
            if let Some((_, before)) = session.blocks.looking.take() {
                session.camera = before;
            }
            None
        } else {
            let before = session
                .blocks
                .looking
                .take()
                .map_or(session.camera, |(_, before)| before);
            Some((id.to_string(), before))
        };
    }
    ui.add_space(8.0);
    let mut field = |ui: &mut Ui,
                     label: &str,
                     name: &str,
                     value: f64,
                     range: std::ops::RangeInclusive<f64>,
                     speed: f64,
                     suffix: &str| {
        let mut v = value;
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).size(11.5).color(SOFT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::DragValue::new(&mut v)
                        .range(range)
                        .speed(speed)
                        .max_decimals(2)
                        .suffix(suffix),
                );
            });
        });
        if v != value {
            edits.push(super::settings::set_value(
                session,
                "Camera",
                &format!("/mission/cameras/{i}/{name}"),
                json!(v),
            ));
        }
    };
    let size = f64::from(map.size);
    field(ui, "Target x", "x", c.x, 0.0..=size, 0.1, " cells");
    field(ui, "Target y", "y", c.y, 0.0..=size, 0.1, " cells");
    field(
        ui,
        "Target lift",
        "height",
        c.height.unwrap_or(0.0),
        CAMERA_HEIGHT,
        0.05,
        "",
    );
    field(
        ui,
        "Distance",
        "distance",
        c.distance,
        CAMERA_DISTANCE,
        0.2,
        "",
    );
    field(
        ui,
        "Angle of attack",
        "pitch",
        c.pitch,
        CAMERA_PITCH,
        0.2,
        "°",
    );
    field(ui, "Rotation", "yaw", c.yaw, -360.0..=360.0, 0.5, "°");
    field(ui, "Field of view", "fov", c.fov, CAMERA_FOV, 0.2, "°");
    field(
        ui,
        "Roll",
        "roll",
        c.roll.unwrap_or(0.0),
        CAMERA_ROLL,
        0.2,
        "°",
    );
}
