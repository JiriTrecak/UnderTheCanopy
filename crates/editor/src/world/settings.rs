//! The map editor's one panel, on the right: with nothing selected, the map's settings
//! (its name, description and picture, its size, its biome, weather and time of day);
//! with something selected, what that is.

use super::{Picked, catalog, palette::Palette};
use crate::document::{Command, Sample};
use crate::map::Session;
use crate::map::view::{Docs, step};
use crate::tabs::Edit;
use crate::theme::*;
use crate::widgets::eyebrow;
use canopy_content::map::blocks::{BLOCK, Block, BlockKind, Blocks};
use canopy_content::map::{MAP_HALO, WeatherKind};
use egui::{RichText, Sense, Ui, vec2};
use serde_json::{Value, json};
use std::path::Path;

/// A biome the picker offers: id, name, what it is about, its picture (a pack entry).
#[derive(Debug, Clone)]
pub struct BiomeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub picture: Option<String>,
}

/// Map sizes to start from: (name, playable width and height, border), in cells.
const SIZES: [(&str, u32, u32); 4] = [
    ("Small", 160, 32),
    ("Medium", 224, 32),
    ("Large", 288, 32),
    ("Huge", 384, 48),
];

/// Times of day to start from: (name, hour).
const TIMES: [(&str, f64); 4] = [("Dawn", 6.5), ("Day", 12.0), ("Dusk", 18.0), ("Night", 0.0)];

/// Weather a map can have: (kind, name).
const WEATHER: [(WeatherKind, &str); 4] = [
    (WeatherKind::Clear, "None"),
    (WeatherKind::Rain, "Rain"),
    (WeatherKind::Snow, "Snow"),
    (WeatherKind::GodRays, "God rays"),
];

/// The panel's state between frames: the size being set, the picture shown.
#[derive(Default)]
pub struct State {
    /// Playable width, height and border being set (before Apply).
    pub size: Option<[u32; 3]>,
    /// The map picture as shown, and when its file was last written.
    pub picture: Option<(std::time::SystemTime, egui::TextureHandle)>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("settings::State")
            .field("size", &self.size)
            .finish()
    }
}

impl Clone for State {
    fn clone(&self) -> Self {
        Self {
            size: self.size,
            picture: self.picture.clone(),
        }
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(eyebrow(title));
    ui.add_space(4.0);
}

fn field(ui: &mut Ui, name: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(name).size(11.0).color(MUTED));
}

/// A setting of map.json at `pointer`, as one undo step (typing merges into one).
fn set(session: &Session, label: &str, pointer: &str, value: Value) -> Edit {
    let mut edit = step(
        session,
        label,
        vec![Command::Set {
            document: session.path("map.json"),
            pointer: pointer.into(),
            value,
            at: None,
        }],
    );
    edit.merge = Some(format!("{}{pointer}", session.root));
    edit
}

/// The panel. Returns the edits it made.
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    content: Option<&canopy_content::Content>,
    docs: Option<&Docs>,
    package: Option<&Path>,
) -> Vec<Edit> {
    let mut edits = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if session.blocks.selection.is_empty() {
                map_settings(ui, session, docs, package, &mut edits);
            } else if let Some(content) = content {
                selection(ui, session, content);
            }
        });
    edits
}

fn map_settings(
    ui: &mut Ui,
    session: &mut Session,
    docs: Option<&Docs>,
    package: Option<&Path>,
    edits: &mut Vec<Edit>,
) {
    let (Some(map), Some(docs)) = (session.map.clone(), docs) else {
        ui.label(RichText::new("Loading the map…").color(FAINT));
        return;
    };
    ui.label(RichText::new("Map").size(15.0).color(TEXT).strong());

    // --- Meta ----------------------------------------------------------------------
    section(ui, "Meta");
    field(ui, "Name");
    let mut name = docs.package["name"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if ui
        .add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY))
        .changed()
    {
        edits.push(package_set(session, "Name", "/name", json!(name)));
    }
    field(ui, "Description");
    let mut description = docs.package["description"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if ui
        .add(
            egui::TextEdit::multiline(&mut description)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        edits.push(package_set(
            session,
            "Description",
            "/description",
            json!(description),
        ));
    }
    field(ui, "Picture");
    picture(ui, session, package);
    ui.horizontal(|ui| {
        if ui
            .button("Draw again")
            .on_hover_text("Draws the map's picture from its ground, starts, amber and camps")
            .clicked()
        {
            session.blocks.write_picture = true;
        }
        if ui
            .button("Upload…")
            .on_hover_text("A picture of your own (PNG, JPEG or WebP), cut square")
            .clicked()
            && let Some(package) = package
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
        {
            match upload(&file, &package.join(&session.root)) {
                Ok(()) => session.blocks.settings.picture = None,
                Err(error) => session.error = Some(error),
            }
        }
    });

    // --- Size ----------------------------------------------------------------------
    section(ui, "Size");
    let area = map.playable_area();
    let border = area.x.min(area.z);
    let now = [area.width, area.height, border];
    let mut size = session.blocks.settings.size.unwrap_or(now);
    ui.horizontal_wrapped(|ui| {
        for (name, cells, border) in SIZES {
            let on = size == [cells, cells, border];
            if ui
                .selectable_label(on, name)
                .on_hover_text(format!("{cells} × {cells} playable, {border} border"))
                .clicked()
            {
                size = [cells, cells, border];
            }
        }
    });
    let number =
        |ui: &mut Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(11.5).color(SOFT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::DragValue::new(value)
                            .range(range)
                            .speed(4.0)
                            .suffix(" cells"),
                    );
                });
            });
            *value -= *value % BLOCK;
        };
    number(ui, "Playable width", &mut size[0], 64..=960);
    number(ui, "Playable height", &mut size[1], 64..=960);
    number(ui, "Border", &mut size[2], 0..=128);
    let total = total_size(size);
    ui.label(
        RichText::new(match total {
            Some(total) => format!("{total} × {total} cells in all (now {})", map.size),
            None => "Too large: a map is at most 1024 cells across".into(),
        })
        .size(11.0)
        .color(MUTED),
    );
    session.blocks.settings.size = (size != now).then_some(size);
    if size != now {
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked()
                && let Some(edit) = resize(session, docs, size)
            {
                edits.push(edit);
                session.blocks.settings.size = None;
            }
            if ui.button("Cancel").clicked() {
                session.blocks.settings.size = None;
            }
        });
    }

    // --- Visuals -------------------------------------------------------------------
    section(ui, "Visuals");
    field(ui, "Biome");
    if let Some(biome) = biome_picker(ui, session, map.biome()) {
        edits.push(set(session, "Biome", "/biome", json!(biome)));
    }
    let environment = map.landscape.as_ref().map(|l| l.environment.clone());
    let weather = environment
        .as_ref()
        .and_then(|e| e.weather.as_ref())
        .map_or(WeatherKind::Clear, |w| w.kind);
    let hour = environment.as_ref().map_or(12.0, |e| e.hour);
    let dynamic = environment.as_ref().is_none_or(|e| e.playing);
    let mut next = (weather, hour, dynamic);
    field(ui, "Weather");
    ui.horizontal_wrapped(|ui| {
        for (kind, name) in WEATHER {
            if ui.selectable_label(weather == kind, name).clicked() {
                next.0 = kind;
            }
        }
    });
    field(ui, "Time of day");
    ui.horizontal_wrapped(|ui| {
        for (name, at) in TIMES {
            if ui
                .selectable_label((hour - at).abs() < 0.01, name)
                .clicked()
            {
                next.1 = at;
            }
        }
    });
    let mut h = next.1 as f32;
    if ui
        .add(
            egui::Slider::new(&mut h, 0.0..=23.75)
                .step_by(0.25)
                .custom_formatter(|v, _| {
                    format!("{:02}:{:02}", v as u32, ((v.fract()) * 60.0).round() as u32)
                }),
        )
        .changed()
    {
        next.1 = f64::from(h);
    }
    ui.checkbox(&mut next.2, "Time passes")
        .on_hover_text("The day goes on from this hour during the match; off, it stays");
    if next != (weather, hour, dynamic) {
        let mut environment = json!({"hour": next.1, "playing": next.2});
        if next.0 != WeatherKind::Clear {
            environment["weather"] = json!({"kind": next.0});
        }
        edits.push(if docs.map.get("landscape").is_some_and(Value::is_object) {
            set(
                session,
                "Weather and time",
                "/landscape/environment",
                environment,
            )
        } else {
            set(
                session,
                "Weather and time",
                "/landscape",
                json!({"environment": environment}),
            )
        });
    }
}

/// A setting of package.json at `pointer` (its name, its description).
fn package_set(session: &Session, label: &str, pointer: &str, value: Value) -> Edit {
    let mut edit = step(
        session,
        label,
        vec![Command::Set {
            document: session.path("package.json"),
            pointer: pointer.into(),
            value,
            at: None,
        }],
    );
    edit.merge = Some(format!("{}/package{pointer}", session.root));
    edit
}

/// The map picture (`preview.webp`), loaded again when its file changes.
fn picture(ui: &mut Ui, session: &mut Session, package: Option<&Path>) {
    let file = package.map(|p| p.join(&session.root).join("preview.webp"));
    let written = file
        .as_ref()
        .and_then(|f| std::fs::metadata(f).ok())
        .and_then(|m| m.modified().ok());
    let stale = match (&session.blocks.settings.picture, written) {
        (Some((at, _)), Some(written)) => *at != written,
        (None, Some(_)) => true,
        _ => false,
    };
    if stale
        && let (Some(file), Some(written)) = (&file, written)
        && let Ok(image) = image::open(file)
    {
        let image = image.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let texture = ui.ctx().load_texture(
            format!("map-picture/{}", session.root),
            egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
            egui::TextureOptions::LINEAR,
        );
        session.blocks.settings.picture = Some((written, texture));
    }
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, width), Sense::click());
    match &session.blocks.settings.picture {
        Some((_, texture)) => {
            egui::Image::from_texture((texture.id(), rect.size()))
                .corner_radius(6)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 6, RAISED);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No picture yet: click to draw it",
                egui::FontId::proportional(12.0),
                FAINT,
            );
        }
    }
    if response
        .on_hover_text("Click to draw the picture again")
        .clicked()
    {
        session.blocks.write_picture = true;
    }
}

/// Writes `file` (any picture) as the map's `preview.webp` in `dir`: cut to a square
/// from its middle, 512 pixels across.
fn upload(file: &Path, dir: &Path) -> Result<(), String> {
    let image = image::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let side = image.width().min(image.height());
    let square = image.crop_imm(
        (image.width() - side) / 2,
        (image.height() - side) / 2,
        side,
        side,
    );
    let picture = square
        .resize_exact(
            crate::map::preview::SIZE,
            crate::map::preview::SIZE,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgba8();
    let bytes = crate::map::preview::webp(&picture)?;
    std::fs::write(dir.join("preview.webp"), bytes).map_err(|e| e.to_string())
}

/// The biome picker: the current biome's picture and name, opening every biome with its
/// picture and what it is about. Returns the biome chosen.
fn biome_picker(ui: &mut Ui, session: &mut Session, current: &str) -> Option<String> {
    let biomes = session.blocks.biomes.clone();
    let palette = &mut session.blocks.palette;
    let shown = biomes.iter().find(|b| b.id == current);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
    let hovered = response.hovered();
    ui.painter().rect(
        rect,
        7,
        if hovered { RAISED } else { SELECTED },
        egui::Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    let thumb = egui::Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(44.0, 44.0));
    biome_picture(ui, palette, thumb, shown);
    ui.painter().text(
        egui::pos2(thumb.right() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        shown.map_or(current, |b| b.name.as_str()),
        egui::FontId::proportional(13.0),
        TEXT,
    );
    let mut chosen = None;
    egui::Popup::from_toggle_button_response(&response)
        .align(egui::RectAlign::LEFT_START)
        .gap(8.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(320.0);
            for biome in &biomes {
                let (rect, response) = ui.allocate_exact_size(vec2(320.0, 132.0), Sense::click());
                let on = biome.id == current;
                if on || response.hovered() {
                    ui.painter()
                        .rect_filled(rect, 8, if on { SELECTED } else { RAISED });
                }
                let image =
                    egui::Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(116.0, 116.0));
                biome_picture(ui, palette, image, Some(biome));
                let text = egui::Rect::from_min_max(
                    egui::pos2(image.right() + 12.0, rect.top() + 10.0),
                    rect.max - vec2(8.0, 8.0),
                );
                ui.painter().text(
                    text.left_top(),
                    egui::Align2::LEFT_TOP,
                    &biome.name,
                    egui::FontId::proportional(14.0),
                    TEXT,
                );
                let job = egui::text::LayoutJob::simple(
                    biome.description.clone(),
                    egui::FontId::proportional(11.5),
                    SOFT,
                    text.width(),
                );
                let galley = ui.painter().layout_job(job);
                ui.painter()
                    .galley(text.left_top() + vec2(0.0, 22.0), galley, SOFT);
                if response.clicked() {
                    chosen = Some(biome.id.clone());
                    ui.close();
                }
            }
        });
    chosen.filter(|b| b != current)
}

fn biome_picture(ui: &Ui, palette: &mut Palette, rect: egui::Rect, biome: Option<&BiomeInfo>) {
    match biome
        .and_then(|b| b.picture.as_deref())
        .and_then(|p| palette.icon(ui.ctx(), p))
    {
        Some(texture) => {
            egui::Image::from_texture((texture, rect.size()))
                .corner_radius(6)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(rect, 6, RAISED);
        }
    }
}

/// Cells across in all for playable `width` × `height` with `border` round it: the next
/// multiple of 32, or none past the largest map.
fn total_size([width, height, border]: [u32; 3]) -> Option<u32> {
    let total = (width.max(height) + 2 * border).div_ceil(32) * 32;
    canopy_content::map::valid_size(total.max(128)).then_some(total.max(128))
}

/// The commands that give the map a playable area of `width` × `height` with `border`
/// round it: the map grows or shrinks round its playable area, keeping its blocks and
/// everything on them where they stand on the old playable area (new ground inside, forest
/// in the border), as one undo step.
fn resize(session: &Session, docs: &Docs, size: [u32; 3]) -> Option<Edit> {
    let map = session.map.as_ref()?;
    let old = Blocks::of(&map.authoring.terrain)?.ok()?;
    let total = total_size(size)?;
    let [width, height, _] = size;
    let snap = |v: u32| v - v % BLOCK;
    let (x, z) = (snap((total - width) / 2), snap((total - height) / 2));
    let area = map.playable_area();
    // How far everything moves (whole blocks).
    let (dx, dz) = (
        i64::from(x) - i64::from(area.x),
        i64::from(z) - i64::from(area.z),
    );
    let (cx, cz) = (
        (dx / i64::from(BLOCK)) as i32,
        (dz / i64::from(BLOCK)) as i32,
    );
    let mut blocks = Blocks::flat(total);
    let side = blocks.side as i32;
    let b = BLOCK as i32;
    let halo = MAP_HALO as i32;
    for column_z in 0..side {
        for column_x in 0..side {
            let (ox, oz) = (column_x - cx, column_z - cz);
            let block = match old.get(ox, oz) {
                Some(block) => block,
                None => {
                    // New ground: open inside the playable area, forest in the border.
                    let (wx, wz) = (column_x * b - halo, column_z * b - halo);
                    let inside = wx >= x as i32
                        && wz >= z as i32
                        && wx + b <= (x + width) as i32
                        && wz + b <= (z + height) as i32;
                    let mut block = Block::ground(1);
                    if !inside {
                        block.kind = BlockKind::Wall;
                    }
                    block
                }
            };
            blocks.set(column_x, column_z, block);
        }
    }
    if let Some(paint) = &old.paint {
        let (old_cells, cells) = (old.cells() as i64, blocks.cells() as i64);
        let mut moved = vec![0u8; (cells * cells) as usize];
        for z in 0..cells {
            for x in 0..cells {
                let (ox, oz) = (x - dx, z - dz);
                if (0..old_cells).contains(&ox) && (0..old_cells).contains(&oz) {
                    moved[(z * cells + x) as usize] = paint[(oz * old_cells + ox) as usize];
                }
            }
        }
        blocks.paint = Some(moved);
    }
    let terrain = blocks.terrain(total);
    let (span, vertices) = (terrain.span(), terrain.vertices());
    let map_doc = session.path("map.json");
    let set = |document: &str, pointer: &str, value: Value| Command::Set {
        document: document.to_string(),
        pointer: pointer.into(),
        value,
        at: None,
    };
    let (fx, fz) = (dx as f64, dz as f64);
    let inside =
        |px: f64, pz: f64| px >= 0.0 && pz >= 0.0 && px < f64::from(total) && pz < f64::from(total);
    let shifted = |list: &Value, f: &dyn Fn(&Value) -> Option<Value>| -> Value {
        Value::Array(
            list.as_array()
                .into_iter()
                .flatten()
                .filter_map(f)
                .collect(),
        )
    };
    let num = |v: &Value| v.as_f64().unwrap_or(0.0);
    let entities = shifted(&docs.map["entities"], &|e| {
        let (px, pz) = (num(&e["position"]["x"]) + fx, num(&e["position"]["y"]) + fz);
        inside(px, pz).then(|| {
            let mut e = e.clone();
            e["position"] = json!({"x": px, "y": pz});
            e
        })
    });
    let kept: Vec<String> = entities
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["id"].as_str().map(str::to_string))
        .collect();
    let starts = shifted(&docs.map["playerStarts"], &|s| {
        let (px, pz) = (num(&s["x"]) + fx, num(&s["z"]) + fz);
        inside(px, pz).then(|| {
            let mut s = s.clone();
            s["x"] = json!(px);
            s["z"] = json!(pz);
            s
        })
    });
    let camps = shifted(&docs.map["camps"], &|c| {
        let members: Vec<Value> = c["members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m.as_str().is_some_and(|m| kept.iter().any(|k| k == m)))
            .cloned()
            .collect();
        (!members.is_empty()).then(|| {
            let mut c = c.clone();
            c["members"] = Value::Array(members);
            c["home"] = json!({"x": num(&c["home"]["x"]) + fx, "y": num(&c["home"]["y"]) + fz});
            c
        })
    });
    let objects = shifted(&docs.scenery["objects"], &|o| {
        let (px, pz) = (num(&o["x"]) + fx, num(&o["z"]) + fz);
        inside(px, pz).then(|| {
            let mut o = o.clone();
            o["x"] = json!(px);
            o["z"] = json!(pz);
            o
        })
    });
    let grid = |file: &str, element: Sample, side: u32, bytes: &[u8]| Command::Grid {
        document: session.path(&format!("terrain/{file}.grid")),
        element,
        width: side,
        height: side,
        bytes: bytes.to_vec(),
    };
    let mut commands = vec![
        set(&map_doc, "/size", json!(total)),
        set(
            &map_doc,
            "/playable",
            json!({"x": x, "z": z, "width": width, "height": height}),
        ),
        set(&map_doc, "/entities", entities),
        set(&map_doc, "/playerStarts", starts),
        set(&map_doc, "/camps", camps),
        set(&session.path("scenery.json"), "/objects", objects),
        grid("heights", Sample::F32, vertices, &terrain.heights.0),
        grid(
            "water-heights",
            Sample::F32,
            vertices,
            &terrain.water_heights.0,
        ),
        grid("water-flow", Sample::Rgba8, span, &terrain.water_flow.0),
    ];
    if let Some(blocks) = &terrain.blocks {
        commands.push(grid("blocks", Sample::Rgba8, span / BLOCK, &blocks.0));
    }
    if let Some(blocked) = &terrain.blocked {
        commands.push(grid("blocked", Sample::U8, span, &blocked.0));
    }
    if let Some(paint) = &terrain.block_paint {
        commands.push(grid("block-paint", Sample::U8, span, &paint.0));
    }
    let mut edit = step(session, "Map size", commands);
    edit.merge = None;
    Some(edit)
}

/// What the selection is: one thing in full, several counted.
fn selection(ui: &mut Ui, session: &mut Session, content: &canopy_content::Content) {
    let Some(map) = session.map.clone() else {
        return;
    };
    let items = session.blocks.selection.clone();
    if items.len() > 1 {
        ui.label(
            RichText::new(format!("{} selected", items.len()))
                .size(15.0)
                .color(TEXT)
                .strong(),
        );
        let count = |kind: Picked| items.iter().filter(|(k, _)| *k == kind).count();
        for (kind, name) in [
            (Picked::Start, "starts"),
            (Picked::Entity, "units, buildings and nodes"),
            (Picked::Object, "scenery"),
        ] {
            let n = count(kind);
            if n > 0 {
                ui.label(RichText::new(format!("{n} {name}")).color(SOFT));
            }
        }
        return;
    }
    let Some((kind, id)) = items.first() else {
        return;
    };
    let row = |ui: &mut Ui, name: &str, value: String| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).size(11.5).color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(value).size(12.0).color(TEXT));
            });
        });
    };
    let cell = |x: f64, z: f64| format!("{x} , {z}");
    match kind {
        Picked::Start => {
            let Some(start) = map
                .player_starts
                .iter()
                .find(|s| s.player.to_string() == *id)
            else {
                return;
            };
            header(
                ui,
                session,
                content,
                None,
                &format!("Start of player {}", start.player),
            );
            section(ui, "Start");
            row(ui, "Player", start.player.to_string());
            let race = content
                .rules
                .races
                .values()
                .find(|r| {
                    r.starting_setup
                        .as_ref()
                        .is_some_and(|s| s.id.as_str() == start.setup)
                })
                .map_or(start.setup.clone(), |r| r.name.clone());
            row(ui, "Race", race);
            row(ui, "Cell", cell(start.x, start.z));
            row(ui, "Facing", format!("{}°", start.rotation.unwrap_or(0)));
        }
        Picked::Entity => {
            let Some(entity) = map.entities.iter().find(|e| e.id == *id) else {
                return;
            };
            let definition = content.definition(entity.definition.as_str());
            let name = definition.map_or(entity.definition.to_string(), |d| d.name.clone());
            let icon = definition.map(|d| format!("{}/image.png", d.icon.as_str()));
            header(ui, session, content, icon, &name);
            section(ui, "Placement");
            row(ui, "Id", entity.id.clone());
            let kind = definition.map_or("", |d| d.kind.name());
            let kind = if definition.is_some_and(catalog::is_mine) {
                "amber node"
            } else {
                kind
            };
            row(ui, "Kind", kind.to_string());
            row(
                ui,
                "Owner",
                match entity.owner {
                    canopy_content::placement::Owner::Player(p) => format!("Player {}", p + 1),
                    canopy_content::placement::Owner::Nobody => "No one".into(),
                },
            );
            row(ui, "Cell", cell(entity.position.x, entity.position.y));
            row(ui, "Facing", format!("{}°", entity.rotation));
            if let Some(f) = definition.and_then(|d| d.footprint.as_ref()) {
                row(ui, "Footprint", format!("{} × {} cells", f.width, f.depth));
            }
            if let Some(state) = &entity.initial_state {
                if let Some(amount) = state.amount {
                    row(ui, "Amount", amount.to_string());
                }
                if let Some(health) = state.health {
                    row(ui, "Health", health.to_string());
                }
                if let Some(experience) = state.experience {
                    row(ui, "Experience", experience.to_string());
                }
                if let Some(construction) = &state.construction {
                    row(
                        ui,
                        "Construction",
                        format!("{construction:?}").to_lowercase(),
                    );
                }
                for (item, count) in state.inventory.iter().flatten() {
                    let name = content
                        .definition(item.as_str())
                        .map_or(item.to_string(), |d| d.name.clone());
                    row(ui, &name, count.to_string());
                }
            }
            if let Some(level) = definition.and_then(|d| d.level) {
                row(ui, "Level", level.to_string());
            }
            if let Some(camp) = map.camps.iter().find(|c| c.members.iter().any(|m| m == id)) {
                section(ui, "Camp");
                row(ui, "Camp", camp.id.clone());
                row(ui, "Creeps", camp.members.len().to_string());
                let levels: u32 = camp
                    .members
                    .iter()
                    .filter_map(|m| map.entities.iter().find(|e| &e.id == m))
                    .filter_map(|e| content.definition(e.definition.as_str())?.level)
                    .sum();
                row(ui, "Levels in all", levels.to_string());
                row(ui, "Home", cell(camp.home.x, camp.home.y));
                row(ui, "Aggro range", format!("{} cells", camp.aggro_range));
                row(ui, "Leash", format!("{} cells", camp.leash));
                if let Some(pool) = &camp.loot_pool {
                    row(ui, "Loot", pool.to_string());
                }
            }
        }
        Picked::Object => {
            let Some(object) = map.authoring.objects.iter().find(|o| o.id == *id) else {
                return;
            };
            let name = session
                .blocks
                .catalog
                .find(&super::Placing::Scenery(object.asset.clone()))
                .map_or(object.asset.clone(), |e| e.name.clone());
            header(ui, session, content, None, &name);
            section(ui, "Scenery");
            row(ui, "Id", object.id.clone());
            row(ui, "Piece", object.asset.clone());
            row(ui, "Cell", cell(object.x, object.z));
            row(ui, "Turn", format!("{:.0}°", object.yaw.to_degrees()));
            row(ui, "Scale", format!("{:.2}", object.scale));
        }
    }
}

/// The selection's title with its icon.
fn header(
    ui: &mut Ui,
    session: &mut Session,
    _content: &canopy_content::Content,
    icon: Option<String>,
    title: &str,
) {
    ui.horizontal(|ui| {
        if let Some(texture) = icon
            .as_deref()
            .and_then(|i| session.blocks.palette.icon(ui.ctx(), i))
        {
            ui.add(egui::Image::from_texture((texture, vec2(40.0, 40.0))).corner_radius(6));
        }
        ui.label(RichText::new(title).size(15.0).color(TEXT).strong());
    });
}
