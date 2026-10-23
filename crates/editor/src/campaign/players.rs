//! A mission map's Players (docs/campaign.md "Campaign AI"), in the map's panel: each
//! computer player of `mission.players` with its race, side, vision and behaviour, its
//! waves as rows of unit portraits with their timing, source and target, and its
//! skirmish limits. Every change is one undo step on map.json (dragging a number folds
//! into one).

use super::MissionLink;
use crate::map::Session;
use crate::map::view::{Docs, step};
use crate::tabs::Edit;
use crate::theme::*;
use crate::widgets::eyebrow;
use canopy_content::Content;
use egui::{Color32, FontId, Frame, Margin, RichText, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};

const BEHAVIOURS: [(&str, &str, &str); 4] = [
    (
        "passive",
        "Passive",
        "Holds its ground: nothing is trained or rebuilt",
    ),
    (
        "base",
        "Base",
        "Keeps its defenders at full strength and rebuilds, never attacks",
    ),
    ("waves", "Waves", "A base that also sends attack waves"),
    (
        "skirmish",
        "Skirmish",
        "The skirmish AI, held back by the limits below",
    ),
];

const STYLES: [&str; 4] = ["standard", "rush", "expand", "tech"];

/// A unit a wave can send: id, name, pack picture.
#[derive(Debug, Clone)]
struct Unit {
    id: String,
    name: String,
    icon: Option<String>,
}

fn units_of(content: Option<&Content>, race: &str) -> Vec<Unit> {
    let Some(content) = content else {
        return Vec::new();
    };
    let source = super::tech::Source::of(content, race);
    let mut units: Vec<&super::tech::Item> = source
        .items
        .iter()
        .filter(|i| !i.building && !i.hero && i.builds.is_empty())
        .collect();
    units.sort_by(|a, b| (a.order, &a.name).cmp(&(b.order, &b.name)));
    units
        .into_iter()
        .map(|i| Unit {
            id: i.id.clone(),
            name: i.name.clone(),
            icon: (!i.icon.is_empty()).then(|| format!("{}/image.png", i.icon)),
        })
        .collect()
}

fn srgb(c: [f32; 3]) -> Color32 {
    let s = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    Color32::from_rgb(s(c[0]), s(c[1]), s(c[2]))
}

/// The colour a mission player plays in: its own `color`, else its slot's.
fn team_color(palette: &[Color32], player: &Value, number: u64) -> Color32 {
    use canopy_content::map::PlayerColor;
    match player["color"].as_str().and_then(PlayerColor::parse) {
        Some(PlayerColor::Rgb([r, g, b])) => Color32::from_rgb(r, g, b),
        Some(PlayerColor::Palette(i)) => palette.get(i).copied().unwrap_or(CONTROL),
        None => palette
            .get(number.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(CONTROL),
    }
}

/// The player's colour: its slot's (no `color`), one of the player colours by name, or
/// any colour (`#rrggbb`) from the picker.
fn color_row(
    ui: &mut Ui,
    palette: &[Color32],
    player: &mut Value,
    number: u64,
    salt: impl std::hash::Hash + std::fmt::Debug,
    drag: &mut bool,
) {
    use canopy_content::map::{COLOR_NAMES, PlayerColor};
    let current = player["color"].as_str().and_then(PlayerColor::parse);
    let mut chosen: Option<Option<String>> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(RichText::new("Colour").size(11.0).color(MUTED));
        if ui
            .selectable_label(current.is_none(), "Slot")
            .on_hover_text(format!("Player {number}'s own colour"))
            .clicked()
        {
            chosen = Some(None);
        }
        for (i, name) in COLOR_NAMES.iter().enumerate() {
            let fill = palette.get(i).copied().unwrap_or(CONTROL);
            let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
            ui.painter().circle_filled(rect.center(), 6.5, fill);
            if current == Some(PlayerColor::Palette(i)) {
                ui.painter()
                    .circle_stroke(rect.center(), 7.5, Stroke::new(1.5, TEXT));
            }
            if response.on_hover_text(*name).clicked() {
                chosen = Some(Some(name.to_string()));
            }
        }
        let shown = team_color(palette, player, number);
        let mut rgb = [shown.r(), shown.g(), shown.b()];
        let custom = matches!(current, Some(PlayerColor::Rgb(_)));
        ui.push_id(salt, |ui| {
            let response = egui::widgets::color_picker::color_edit_button_srgb(ui, &mut rgb)
                .on_hover_text("Any colour: a tint no player has (a feral brood's)");
            if custom {
                ui.painter().rect_stroke(
                    response.rect.expand(1.0),
                    3,
                    Stroke::new(1.5, TEXT),
                    egui::StrokeKind::Outside,
                );
            }
            if response.changed() {
                chosen = Some(Some(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])));
                // Dragging in the picker folds into one undo step.
                *drag = true;
            }
        });
    });
    match chosen {
        Some(Some(color)) => player["color"] = json!(color),
        Some(None) => {
            if let Some(o) = player.as_object_mut() {
                o.shift_remove("color");
            }
        }
        None => {}
    }
}

/// A choice among a few, as a row of segments.
fn segments(ui: &mut Ui, current: &str, choices: &[(&str, &str, &str)]) -> Option<String> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (value, label, hint) in choices {
            if ui
                .selectable_label(current == *value, *label)
                .on_hover_text(*hint)
                .clicked()
                && current != *value
            {
                chosen = Some(value.to_string());
            }
        }
    });
    chosen
}

/// An optional number of seconds: a box to have it and its value.
fn seconds(ui: &mut Ui, label: &str, hint: &str, value: &mut Value, default: f64, drag: &mut bool) {
    let mut on = value.is_number();
    let mut v = value.as_f64().unwrap_or(default);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if ui.checkbox(&mut on, label).on_hover_text(hint).changed() {
            *value = if on { json!(v) } else { Value::Null };
        }
        if on {
            let response = ui.add(
                egui::DragValue::new(&mut v)
                    .range(0.0..=36_000.0)
                    .speed(1.0)
                    .suffix(" s"),
            );
            if response.changed() {
                *value = json!(v.round());
                *drag = true;
            }
        }
    });
}

/// A region picker: `none` is the first choice's label; returns the region chosen, or
/// `Some(None)` for the first choice.
fn region_choice(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    current: &Value,
    plain: &[(&str, &str)],
    regions: &[String],
) -> Option<Value> {
    let shown = match current {
        Value::String(s) => plain
            .iter()
            .find(|(v, _)| v == s)
            .map_or(s.clone(), |(_, l)| l.to_string()),
        Value::Object(o) => o
            .get("region")
            .and_then(Value::as_str)
            .map_or("Region".into(), |r| format!("Region {r}")),
        _ => plain.first().map_or(String::new(), |(_, l)| l.to_string()),
    };
    let mut chosen = None;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(shown)
        .width(170.0)
        .show_ui(ui, |ui| {
            for (value, label) in plain {
                if ui
                    .selectable_label(current.as_str() == Some(*value), *label)
                    .clicked()
                {
                    chosen = Some(json!(value));
                }
            }
            if regions.is_empty() {
                ui.label(
                    RichText::new("The mission has no regions yet")
                        .size(11.0)
                        .color(FAINT),
                );
            }
            for region in regions {
                let on = current.get("region").and_then(Value::as_str) == Some(region);
                if ui
                    .selectable_label(on, format!("Region {region}"))
                    .clicked()
                {
                    chosen = Some(json!({"region": region}));
                }
            }
        });
    chosen
}

/// A unit's portrait with its count in the corner.
fn portrait(
    ui: &mut Ui,
    palette: &mut crate::world::palette::Palette,
    unit: Option<&Unit>,
    id: &str,
    count: u64,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
    let painter = ui.painter();
    let texture = unit
        .and_then(|u| u.icon.as_deref())
        .and_then(|e| palette.icon(ui.ctx(), e));
    match texture {
        Some(t) => {
            egui::Image::from_texture((t, rect.size()))
                .corner_radius(6)
                .paint_at(ui, rect);
        }
        None => {
            painter.rect_filled(rect, 6, SELECTED);
            crate::icons::paint(
                painter,
                crate::icons::square(rect.center(), 20.0),
                crate::icons::Icon::Units,
                FAINT,
            );
        }
    }
    if response.hovered() {
        painter.rect_stroke(rect, 6, Stroke::new(1.5, SOFT), egui::StrokeKind::Inside);
    }
    let badge = egui::Rect::from_min_size(rect.right_bottom() - vec2(18.0, 15.0), vec2(18.0, 15.0));
    painter.rect_filled(badge, 4, Color32::from_black_alpha(200));
    painter.text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        count.to_string(),
        FontId::proportional(10.5),
        TEXT,
    );
    let name = unit.map_or(id.to_string(), |u| u.name.clone());
    response.on_hover_text(format!(
        "{name}\n{id}\nClick: one more · right-click: one fewer"
    ))
}

/// The Players section of a mission map's panel; pushes its edits.
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    docs: &Docs,
    content: Option<&Content>,
    edits: &mut Vec<Edit>,
) {
    let Some(link) = session.mission.clone() else {
        return;
    };
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.label(eyebrow("Players"));
    });
    ui.label(
        RichText::new("Computer players of this mission; player 1 is the campaign's player.")
            .size(11.0)
            .color(FAINT),
    );
    ui.add_space(4.0);
    let before = docs
        .map
        .pointer("/mission/players")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut players = before.clone();
    let regions: Vec<String> = docs
        .map
        .pointer("/mission/regions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    let races: Vec<(String, String)> = content
        .map(|c| {
            c.rules
                .races
                .iter()
                .map(|(id, r)| (id.clone(), r.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let palette: Vec<Color32> = session
        .scene
        .as_ref()
        .map(|s| s.palette.iter().map(|c| srgb(*c)).collect())
        .unwrap_or_default();
    let mut drag = false;
    let mut remove = None;
    let list = players.as_array_mut().expect("a list");
    for (i, player) in list.iter_mut().enumerate() {
        let number = player["player"].as_u64().unwrap_or(0);
        Frame::new()
            .fill(RAISED)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(8)
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    let color = team_color(&palette, player, number);
                    ui.painter().circle_filled(dot.center(), 5.5, color);
                    ui.label(
                        RichText::new(format!("Player {number}"))
                            .color(TEXT)
                            .strong(),
                    );
                    let race = player["race"].as_str().unwrap_or(link.race()).to_string();
                    let shown = races
                        .iter()
                        .find(|(id, _)| *id == race)
                        .map_or(race.clone(), |(_, n)| n.clone());
                    egui::ComboBox::from_id_salt(("player-race", &session.root, i))
                        .selected_text(shown)
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            for (id, name) in &races {
                                if ui.selectable_label(*id == race, name).clicked() {
                                    player["race"] = json!(id);
                                }
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::icon_button(ui, crate::icons::Icon::Close, false, true)
                            .on_hover_text("Remove this player from the mission")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Team").size(11.0).color(MUTED));
                    let team = player["team"].as_str().unwrap_or("enemy").to_string();
                    if let Some(t) = segments(
                        ui,
                        &team,
                        &[
                            ("ally", "Ally", "On player 1's team"),
                            ("enemy", "Enemy", "On the other team, with every enemy"),
                        ],
                    ) {
                        player["team"] = json!(t);
                        if t == "enemy"
                            && let Some(o) = player.as_object_mut()
                        {
                            o.shift_remove("vision");
                        }
                    }
                    if team == "ally" {
                        let mut vision = player["vision"].as_bool().unwrap_or(true);
                        if ui
                            .checkbox(&mut vision, "Shares vision")
                            .on_hover_text("Player 1 sees what this ally sees")
                            .changed()
                        {
                            player["vision"] = json!(vision);
                        }
                    }
                });
                color_row(ui, &palette, player, number, (&session.root, i), &mut drag);
                if !player["ai"].is_object() {
                    player["ai"] = json!({"behaviour": "passive"});
                }
                let ai = &mut player["ai"];
                let behaviour = ai["behaviour"].as_str().unwrap_or("passive").to_string();
                ui.label(RichText::new("Behaviour").size(11.0).color(MUTED));
                if let Some(b) = segments(ui, &behaviour, &BEHAVIOURS) {
                    ai["behaviour"] = json!(b);
                }
                ui.horizontal(|ui| {
                    let mut dormant = ai["dormant"].as_bool().unwrap_or(false);
                    if ui
                        .checkbox(&mut dormant, "Dormant")
                        .on_hover_text("Starts with nothing until the script wakes it (ai_wake)")
                        .changed()
                    {
                        if dormant {
                            ai["dormant"] = json!(true);
                        } else if let Some(o) = ai.as_object_mut() {
                            o.shift_remove("dormant");
                        }
                    }
                    if matches!(behaviour.as_str(), "base" | "waves") {
                        let mut rebuild = ai["rebuild"].as_bool().unwrap_or(true);
                        if ui
                            .checkbox(&mut rebuild, "Rebuilds")
                            .on_hover_text("Destroyed buildings rise again where they stood")
                            .changed()
                        {
                            ai["rebuild"] = json!(rebuild);
                        }
                    }
                });
                let race = player["race"].as_str().unwrap_or(link.race()).to_string();
                let ai = &mut player["ai"];
                if behaviour == "waves" || ai["waves"].as_array().is_some_and(|w| !w.is_empty()) {
                    waves(ui, session, ai, &race, content, &regions, i, &mut drag);
                }
                if behaviour == "skirmish" {
                    skirmish(ui, ai, &mut drag);
                }
            });
        ui.add_space(6.0);
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    let taken: Vec<u64> = list.iter().filter_map(|p| p["player"].as_u64()).collect();
    let free = (2..=8).find(|n| !taken.contains(n));
    if ui
        .add_enabled(free.is_some(), egui::Button::new("+ Add player"))
        .on_hover_text("A computer player: an enemy base of the campaign's race")
        .on_disabled_hover_text("Players 2 to 8 are all in the mission")
        .clicked()
        && let Some(n) = free
    {
        list.push(
            json!({"player": n, "race": link.race(), "team": "enemy", "ai": {"behaviour": "base"}}),
        );
    }
    if players != before {
        edits.push(write(session, &link, docs, players, drag));
    }
}

/// The waves of one player: a row per wave line.
#[allow(clippy::too_many_arguments)]
fn waves(
    ui: &mut Ui,
    session: &mut Session,
    ai: &mut Value,
    race: &str,
    content: Option<&Content>,
    regions: &[String],
    player: usize,
    drag: &mut bool,
) {
    ui.add_space(6.0);
    ui.label(eyebrow("Waves"));
    if !ai["waves"].is_array() {
        ai["waves"] = json!([]);
    }
    let units = units_of(content, race);
    let lines = ai["waves"].as_array_mut().expect("a list");
    let mut remove = None;
    for (j, line) in lines.iter_mut().enumerate() {
        Frame::new()
            .fill(SUNK)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(6)
            .inner_margin(Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Line {}", j + 1)).color(SOFT).size(12.0));
                    let timing = match (line["after"].as_f64(), line["every"].as_f64()) {
                        (Some(a), Some(e)) => format!("from {a:.0} s, every {e:.0} s"),
                        (Some(a), None) => format!("once at {a:.0} s"),
                        (None, _) => "when the script starts it".into(),
                    };
                    ui.label(RichText::new(timing).size(11.0).color(FAINT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::icon_button(ui, crate::icons::Icon::Close, false, true)
                            .on_hover_text("Remove this wave line")
                            .clicked()
                        {
                            remove = Some(j);
                        }
                    });
                });
                ui.vertical(|ui| {
                    seconds(ui, "After", "The first wave this long after the player is active; without it, the script starts the line (ai_waves)", &mut line["after"], 120.0, drag);
                    seconds(ui, "Every", "Seconds between waves; without it, one wave only", &mut line["every"], 90.0, drag);
                    seconds(ui, "Until", "The line sends nothing from this time on", &mut line["until"], 600.0, drag);
                });
                for field in ["after", "every", "until"] {
                    if line[field].is_null()
                        && let Some(o) = line.as_object_mut()
                    {
                        o.shift_remove(field);
                    }
                }
                // The units: portraits with their counts.
                if !line["units"].is_array() {
                    line["units"] = json!([]);
                }
                let list = line["units"].as_array_mut().expect("a list");
                let mut gone = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for (k, entry) in list.iter_mut().enumerate() {
                        let id = entry["unit"].as_str().unwrap_or_default().to_string();
                        let count = entry["count"].as_u64().unwrap_or(1);
                        let unit = units.iter().find(|u| u.id == id);
                        let response = portrait(ui, &mut session.blocks.palette, unit, &id, count);
                        if response.clicked() {
                            entry["count"] = json!(count + 1);
                        }
                        if response.secondary_clicked() {
                            if count <= 1 {
                                gone = Some(k);
                            } else {
                                entry["count"] = json!(count - 1);
                            }
                        }
                    }
                    let add = ui.add_sized(vec2(40.0, 40.0), egui::Button::new("+"));
                    egui::Popup::menu(&add)
                        .id(egui::Id::new(("wave-add", &session.root, player, j)))
                        .show(|ui| {
                            ui.set_max_width(260.0);
                            ui.label(RichText::new("Add a unit").size(11.0).color(MUTED));
                            if units.is_empty() {
                                ui.label(RichText::new("The race has no units yet").size(11.0).color(FAINT));
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                for unit in &units {
                                    if portrait(ui, &mut session.blocks.palette, Some(unit), &unit.id, 1)
                                        .clicked()
                                    {
                                        match list.iter_mut().find(|e| e["unit"] == unit.id.as_str()) {
                                            Some(e) => e["count"] = json!(e["count"].as_u64().unwrap_or(0) + 1),
                                            None => list.push(json!({"unit": unit.id, "count": 1})),
                                        }
                                        ui.close();
                                    }
                                }
                            });
                        });
                });
                if let Some(k) = gone {
                    list.remove(k);
                }
                // More every few waves.
                let mut grows = line["grow"].is_object();
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut grows, "Grows")
                        .on_hover_text("More units every few waves")
                        .changed()
                    {
                        line["grow"] = if grows {
                            let unit = line["units"][0]["unit"]
                                .as_str()
                                .map(str::to_string)
                                .or_else(|| units.first().map(|u| u.id.clone()))
                                .unwrap_or_default();
                            json!({"every": 3, "unit": unit, "count": 1})
                        } else {
                            Value::Null
                        };
                    }
                    if let Some(grow) = line["grow"].as_object_mut() {
                        let mut every = grow["every"].as_u64().unwrap_or(3);
                        let mut count = grow["count"].as_u64().unwrap_or(1);
                        ui.label(RichText::new("+").size(11.0).color(MUTED));
                        if ui.add(egui::DragValue::new(&mut count).range(1..=20)).changed() {
                            grow.insert("count".into(), json!(count));
                            *drag = true;
                        }
                        ui.label(RichText::new("every").size(11.0).color(MUTED));
                        if ui
                            .add(egui::DragValue::new(&mut every).range(1..=50).suffix(" waves"))
                            .changed()
                        {
                            grow.insert("every".into(), json!(every));
                            *drag = true;
                        }
                    }
                });
                if let Some(grow) = line["grow"].as_object_mut() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("of").size(11.0).color(MUTED));
                        let current = grow.get("unit").and_then(Value::as_str).unwrap_or_default().to_string();
                        let shown = units.iter().find(|u| u.id == current).map_or(current.clone(), |u| u.name.clone());
                        egui::ComboBox::from_id_salt(("grow-unit", &session.root, player, j))
                            .selected_text(shown)
                            .width(170.0)
                            .show_ui(ui, |ui| {
                                for unit in &units {
                                    if ui.selectable_label(unit.id == current, &unit.name).clicked() {
                                        grow.insert("unit".into(), json!(unit.id));
                                    }
                                }
                            });
                    });
                }
                if line["grow"].is_null()
                    && let Some(o) = line.as_object_mut()
                {
                    o.shift_remove("grow");
                }
                ui.horizontal_wrapped(|ui| {
                    let mut hero = line["hero"].as_bool().unwrap_or(false);
                    if ui
                        .checkbox(&mut hero, "Hero joins")
                        .on_hover_text("The player's living heroes march with each wave")
                        .changed()
                    {
                        if hero {
                            line["hero"] = json!(true);
                        } else if let Some(o) = line.as_object_mut() {
                            o.shift_remove("hero");
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("From").size(11.0).color(MUTED));
                    let from = line.get("from").cloned().unwrap_or(json!("trained"));
                    if let Some(v) = region_choice(
                        ui,
                        ("wave-from", &session.root, player, j),
                        &from,
                        &[("trained", "Trained")],
                        regions,
                    ) {
                        line["from"] = v;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("To").size(11.0).color(MUTED));
                    let target = line.get("target").cloned().unwrap_or(json!("base"));
                    if let Some(v) = region_choice(
                        ui,
                        ("wave-target", &session.root, player, j),
                        &target,
                        &[("base", "Main base"), ("nearest", "Nearest units")],
                        regions,
                    ) {
                        line["target"] = v;
                    }
                });
            });
        ui.add_space(4.0);
    }
    if let Some(j) = remove {
        lines.remove(j);
    }
    if ui
        .button("+ Wave line")
        .on_hover_text("A line of attack waves: the first at a time, then every so often")
        .clicked()
    {
        let unit = units.first().map(|u| u.id.clone());
        lines.push(json!({
            "after": 120,
            "every": 90,
            "units": unit.map(|u| vec![json!({"unit": u, "count": 2})]).unwrap_or_default(),
            "from": "trained",
            "target": "base",
        }));
    }
}

/// The Skirmish behaviour's limits.
fn skirmish(ui: &mut Ui, ai: &mut Value, drag: &mut bool) {
    ui.add_space(6.0);
    ui.label(eyebrow("Skirmish limits"));
    if !ai["skirmish"].is_object() {
        ai["skirmish"] = json!({});
    }
    let limits = ai["skirmish"].as_object_mut().expect("an object");
    ui.horizontal(|ui| {
        ui.label(RichText::new("Style").size(11.0).color(MUTED));
        let style = limits
            .get("style")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        egui::ComboBox::from_id_salt("skirmish-style")
            .selected_text(if style.is_empty() {
                "From the seed"
            } else {
                &style
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(style.is_empty(), "From the seed")
                    .clicked()
                {
                    limits.shift_remove("style");
                }
                for s in STYLES {
                    if ui.selectable_label(style == s, s).clicked() {
                        limits.insert("style".into(), json!(s));
                    }
                }
            });
    });
    let mut number = |ui: &mut Ui,
                      key: &str,
                      label: &str,
                      range: std::ops::RangeInclusive<f64>,
                      default: f64,
                      step: f64,
                      hint: &str| {
        let mut on = limits.get(key).is_some_and(Value::is_number);
        let mut v = limits.get(key).and_then(Value::as_f64).unwrap_or(default);
        ui.horizontal(|ui| {
            if ui.checkbox(&mut on, label).on_hover_text(hint).changed() {
                if on {
                    limits.insert(key.into(), json!(v));
                } else {
                    limits.shift_remove(key);
                }
            }
            if on
                && ui
                    .add(egui::DragValue::new(&mut v).range(range).speed(step))
                    .changed()
            {
                let v = if step >= 1.0 {
                    json!(v.round() as u64)
                } else {
                    json!((v * 100.0).round() / 100.0)
                };
                limits.insert(key.into(), v);
                *drag = true;
            }
        });
    };
    number(
        ui,
        "armyCap",
        "Army cap",
        0.0..=200.0,
        20.0,
        1.0,
        "Most fighters it fields",
    );
    number(
        ui,
        "pace",
        "Pace",
        1.0..=10.0,
        2.0,
        0.05,
        "Decision intervals multiplied (2 plays at half speed)",
    );
    number(
        ui,
        "income",
        "Income",
        0.0..=4.0,
        0.5,
        0.01,
        "Gathered goods multiplied (0.5 halves its income)",
    );
}

/// The step that writes `players` into the map's mission (making the section when the
/// map has none).
fn write(session: &Session, link: &MissionLink, docs: &Docs, players: Value, drag: bool) -> Edit {
    let document = session.path("map.json");
    let command = if docs.map.get("mission").is_some_and(Value::is_object) {
        crate::document::Command::Set {
            document,
            pointer: "/mission/players".into(),
            value: players,
            at: None,
        }
    } else {
        let step = link.step.and_then(|s| super::steps(&link.value).get(s));
        let title = step.map_or("", |s| link.title(s));
        let order = link
            .step
            .and_then(|s| {
                super::mission_numbers(&link.value)
                    .get(s)
                    .copied()
                    .flatten()
            })
            .unwrap_or(1);
        let mut section = super::new_mission_section(super::id_of(&link.campaign), title, order);
        section["players"] = players;
        crate::document::Command::Set {
            document,
            pointer: "/mission".into(),
            value: section,
            at: None,
        }
    };
    let mut edit = step(session, "Mission players", vec![command]);
    if drag {
        edit.merge = Some(format!("{}/mission/players", session.root));
    }
    edit
}
