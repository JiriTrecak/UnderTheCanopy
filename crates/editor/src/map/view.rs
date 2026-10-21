//! The map view: the tool panel, the viewport (the game renderer drawing the editor's
//! scene of the map) with its camera, picking the ground under the pointer, the brush
//! ring, and turning strokes and clicks into undo steps.

use super::tools::{self, Picked, Placement, Stroke};
use super::{Camera, Session};
use crate::document::{Command, Context};
use crate::library::Kind;
use crate::tabs::Edit;
use canopy_content::Content;
use egui::{Pos2, Rect, pos2, vec2};
use glam::{Mat4, Vec3, Vec4};
use serde_json::{Value, json};

/// Vertical field of view of the map camera (radians).
pub const FOV: f32 = 0.8;

/// What the map view asks of the shell this frame.
#[derive(Default)]
pub struct Out {
    /// The viewport's size in physical pixels: the stage draws the scene at this size.
    pub size: Option<[u32; 2]>,
    /// The split view's right pane, the generated world, in physical pixels.
    pub size_generated: Option<[u32; 2]>,
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

/// The nearest start, entity or object to world (`x`, `z`) within 4 cells.
pub(crate) fn nearest(session: &Session, x: f32, z: f32) -> Option<(Picked, String)> {
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

/// The commands that delete everything in `items` at once (a camp member takes its whole
/// camp with it), removing from the end of each list so the indices hold.
pub(crate) fn delete_all(
    session: &Session,
    docs: &Docs,
    items: &[(Picked, String)],
) -> Vec<Command> {
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let (starts, entities, camps, objects) = (
        list(&docs.map["playerStarts"]),
        list(&docs.map["entities"]),
        list(&docs.map["camps"]),
        list(&docs.scenery["objects"]),
    );
    let has = |kind: Picked, id: &str| items.iter().any(|(k, i)| *k == kind && i == id);
    let gone_starts: Vec<usize> = (0..starts.len())
        .filter(|i| {
            starts[*i]["player"]
                .as_u64()
                .is_some_and(|p| has(Picked::Start, &p.to_string()))
        })
        .collect();
    let mut gone_entities: std::collections::BTreeSet<String> = items
        .iter()
        .filter(|(k, _)| *k == Picked::Entity)
        .map(|(_, id)| id.clone())
        .collect();
    let mut gone_camps = Vec::new();
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
    let entity_indices: Vec<usize> = (0..entities.len())
        .filter(|i| {
            entities[*i]["id"]
                .as_str()
                .is_some_and(|id| gone_entities.contains(id))
        })
        .collect();
    let gone_objects: Vec<usize> = (0..objects.len())
        .filter(|i| {
            objects[*i]["id"]
                .as_str()
                .is_some_and(|id| has(Picked::Object, id))
        })
        .collect();
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
    let mut commands = remove(&map_doc, "playerStarts", gone_starts);
    commands.extend(remove(&map_doc, "entities", entity_indices));
    commands.extend(remove(&map_doc, "camps", gone_camps));
    commands.extend(remove(
        &session.path("scenery.json"),
        "objects",
        gone_objects,
    ));
    commands
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

/// What a click of `tool` at world (`x`, `z`) places: an amber site facing the nearest
/// start, a creep camp, or a start with its amber and a level pad. Shared by the view and
/// MCP.
pub fn place(
    session: &mut Session,
    content: &Content,
    docs: &Docs,
    tool: Placement,
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
        Placement::Amber => {
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
        Placement::Camp => {
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
        Placement::Start => {
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
            let mut pad = Stroke::new();
            let root = session.root.clone();
            if let Some(map) = session.map.as_mut() {
                tools::level_pad(map, &mut pad, x as f32, z as f32, 16.0);
                commands.extend(pad.patches(map, &root));
            }
            Some(step(session, &format!("Place start {player}"), commands))
        }
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
