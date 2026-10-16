//! Each editor's Overview (docs/editor.md, M2): what a form cannot show. A unit's stats,
//! abilities and who trains it; what a building trains and researches; an ability's rank
//! table, cast timeline and who binds it; an effect's layers; a race's roster. Changes
//! come back as [`Change`]s and links as ids to open, like the forms'.

use crate::document::Command;
use crate::forms::Change;
use crate::references::Use;
use crate::theme::*;
use crate::widgets::eyebrow;
use egui::{ComboBox, DragValue, Grid, Margin, RichText, TextEdit, Ui};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Ticks a second (the simulation's 25 ms step).
const TICKS: f64 = 40.0;

/// What an element is called and where it lives, by id.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub document: String,
}

pub struct Overview<'a> {
    pub document: &'a str,
    /// Every element by id.
    pub index: &'a BTreeMap<String, Entry>,
    /// Uses of this element's id elsewhere.
    pub uses: &'a [Use],
    pub changes: Vec<Change>,
    /// Ids to open (links).
    pub opens: Vec<String>,
    /// A new element to create, with its id started (the shell opens the dialog).
    pub create: Option<String>,
    /// A unit to try this ability with in a test match (the encounter).
    pub try_with: Option<String>,
}

/// Whether `document` has an Overview.
pub fn has(document: &str) -> bool {
    matches!(
        document.split('/').next(),
        Some("units" | "buildings" | "items" | "abilities" | "effects" | "races")
    )
}

fn number(value: &Value, pointer: &str) -> Option<f64> {
    value.pointer(pointer).and_then(Value::as_f64)
}

fn text<'v>(value: &'v Value, pointer: &str) -> Option<&'v str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn seconds(ticks: f64) -> String {
    let s = ticks / TICKS;
    if (s - s.round()).abs() < 0.005 {
        format!("{s:.0} s")
    } else {
        format!("{s:.2} s")
    }
}

/// `items: [{item, amount}]` as "425 amber · 100 wood".
fn cost(items: Option<&Value>) -> String {
    let Some(Value::Array(items)) = items else {
        return "free".into();
    };
    let parts: Vec<String> = items
        .iter()
        .filter_map(|i| {
            let amount = i.get("amount")?.as_f64()?;
            let item = i.get("item")?.as_str()?;
            Some(format!("{amount:.0} {}", item.trim_start_matches("item.")))
        })
        .collect();
    if parts.is_empty() {
        "free".into()
    } else {
        parts.join(" · ")
    }
}

impl<'a> Overview<'a> {
    pub fn new(document: &'a str, index: &'a BTreeMap<String, Entry>, uses: &'a [Use]) -> Self {
        Self {
            document,
            index,
            uses,
            changes: Vec::new(),
            opens: Vec::new(),
            create: None,
            try_with: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, value: &Value) {
        ui.spacing_mut().item_spacing.y = 6.0;
        match self.document.split('/').next() {
            Some("units") => self.unit(ui, value),
            Some("buildings") => self.building(ui, value),
            Some("items") => self.item(ui, value),
            Some("abilities") => self.ability(ui, value),
            Some("effects") => self.effect(ui, value),
            Some("races") => self.race(ui, value),
            _ => {}
        }
    }

    fn step(&mut self, label: &str, pointer: &str, commands: Vec<Command>, merge: bool) {
        self.changes.push(Change {
            label: label.to_string(),
            pointer: pointer.to_string(),
            commands,
            merge,
        });
    }

    fn set(&mut self, label: &str, pointer: &str, value: Value, merge: bool) {
        let command = Command::Set {
            document: self.document.to_string(),
            pointer: pointer.to_string(),
            value,
            at: None,
        };
        self.step(label, pointer, vec![command], merge);
    }

    /// A clickable element name that opens it.
    fn link(&mut self, ui: &mut Ui, id: &str) {
        let (label, known) = match self.index.get(id) {
            Some(entry) => (entry.name.clone(), true),
            None => (id.to_string(), false),
        };
        let text = RichText::new(label).color(if known { SOFT } else { ERROR });
        let response = ui
            .add(egui::Label::new(text).sense(egui::Sense::click()))
            .on_hover_text(if known {
                format!("Open {id}")
            } else {
                format!("{id} is not an element")
            });
        if known {
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                self.opens.push(id.to_string());
            }
        }
    }

    fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
        egui::Frame::new()
            .fill(RAISED)
            .stroke(egui::Stroke::new(1.0, BORDER))
            .corner_radius(8)
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(eyebrow(title));
                ui.add_space(2.0);
                body(ui);
            });
        ui.add_space(6.0);
    }

    fn stats(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
        Grid::new(id)
            .num_columns(2)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                for (label, value) in rows {
                    ui.label(RichText::new(*label).color(MUTED).size(11.5));
                    ui.label(RichText::new(value).color(TEXT));
                    ui.end_row();
                }
            });
    }

    /// Where this element is named, as links (`filter` picks the uses by pointer).
    fn users(&mut self, ui: &mut Ui, filter: impl Fn(&Use) -> bool, none: &str) {
        let mut documents: Vec<&str> = self
            .uses
            .iter()
            .filter(|u| u.document != self.document && filter(u))
            .map(|u| u.document.as_str())
            .collect();
        documents.dedup();
        if documents.is_empty() {
            ui.label(RichText::new(none).color(FAINT).size(11.5));
        }
        // Each document's element, by its id.
        let ids: Vec<String> = documents
            .iter()
            .filter_map(|d| {
                self.index
                    .iter()
                    .find(|(_, e)| e.document == *d)
                    .map(|(id, _)| id.clone())
            })
            .collect();
        ui.horizontal_wrapped(|ui| {
            for id in ids {
                self.link(ui, &id);
            }
        });
    }

    fn unit(&mut self, ui: &mut Ui, value: &Value) {
        let combat = value.pointer("/behaviors/combat");
        let hero = value.get("hero").and_then(Value::as_bool).unwrap_or(false);
        let mut rows = vec![
            (
                "Health",
                number(value, "/body/maxHp").map_or("–".into(), |v| format!("{v:.0}")),
            ),
            (
                "Armour",
                format!(
                    "{} {}",
                    number(value, "/body/armor").map_or("–".into(), |v| format!("{v:.0}")),
                    text(value, "/body/armorType").unwrap_or_default()
                ),
            ),
        ];
        if let Some(combat) = combat {
            let damage = number(combat, "/damage").unwrap_or(0.0);
            let cooldown = number(combat, "/cooldownTicks").unwrap_or(0.0);
            let dps = if cooldown > 0.0 {
                damage * TICKS / cooldown
            } else {
                0.0
            };
            rows.push((
                "Damage",
                format!(
                    "{damage:.0} {} every {} ({dps:.1} a second)",
                    text(combat, "/damageType").unwrap_or_default(),
                    seconds(cooldown)
                ),
            ));
            rows.push((
                "Range",
                number(combat, "/range").map_or("–".into(), |v| format!("{v}")),
            ));
        }
        rows.push((
            "Speed",
            number(value, "/behaviors/movement/speed").map_or("–".into(), |v| format!("{v}")),
        ));
        rows.push((
            "Sight",
            number(value, "/vision").map_or("–".into(), |v| format!("{v}")),
        ));
        rows.push((
            "Supply",
            number(value, "/supplyCost").map_or("–".into(), |v| format!("{v:.0}")),
        ));
        rows.push((
            "Cost",
            format!(
                "{} · {}",
                cost(value.pointer("/creation/items")),
                seconds(number(value, "/creation/workTicks").unwrap_or(0.0))
            ),
        ));
        Self::card(ui, if hero { "Hero" } else { "Unit" }, |ui| {
            Self::stats(ui, "unit-stats", &rows)
        });
        if hero && let Some(Value::Array(levels)) = value.pointer("/behaviors/progression/levels") {
            Self::card(ui, "Levels", |ui| {
                Grid::new("levels")
                    .striped(false)
                    .spacing([18.0, 4.0])
                    .show(ui, |ui| {
                        for head in ["Level", "Experience", "Health", "Damage", "Armour", "Mana"] {
                            ui.label(RichText::new(head).color(FAINT).size(11.0));
                        }
                        ui.end_row();
                        for (i, level) in levels.iter().enumerate() {
                            ui.label(RichText::new(format!("{}", i + 1)).color(MUTED));
                            for key in ["experience", "maxHp", "damage", "armor", "maxMana"] {
                                ui.label(level.get(key).map_or("–".into(), |v| v.to_string()));
                            }
                            ui.end_row();
                        }
                    });
            });
        }
        self.bindings(ui, value, hero);
        let me = self;
        Self::card(ui, "Trained by", |ui| {
            me.users(
                ui,
                |u| u.pointer.contains("/production/outputs"),
                "Nothing trains it.",
            )
        });
    }

    /// A unit's abilities: hotkeys, links, removing one, binding another.
    fn bindings(&mut self, ui: &mut Ui, value: &Value, hero: bool) {
        let bindings = value
            .pointer("/behaviors/abilities/bindings")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut remove = None;
        let mut bind = None;
        let mut create = false;
        let abilities: Vec<String> = self
            .index
            .keys()
            .filter(|id| {
                id.starts_with("ability.") && !bindings.iter().any(|b| b["ability"] == **id)
            })
            .cloned()
            .collect();
        let me = &mut *self;
        Self::card(ui, "Abilities", |ui| {
            if bindings.is_empty() {
                ui.label(RichText::new("No abilities.").color(FAINT).size(11.5));
            }
            for (i, binding) in bindings.iter().enumerate() {
                ui.horizontal(|ui| {
                    let key = binding
                        .pointer("/command/hotkey")
                        .and_then(Value::as_str)
                        .unwrap_or("·");
                    ui.label(
                        RichText::new(key)
                            .monospace()
                            .color(TEXT)
                            .background_color(SELECTED),
                    );
                    if let Some(id) = binding.get("ability").and_then(Value::as_str) {
                        me.link(ui, id);
                    }
                    let hidden = binding
                        .get("controls")
                        .and_then(Value::as_array)
                        .is_some_and(|c| !c.iter().any(|v| v == "player"));
                    if hidden {
                        ui.label(RichText::new("AI only").color(FAINT).size(11.0));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Unbind").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            ui.horizontal(|ui| {
                ComboBox::from_id_salt(("bind", me.document))
                    .selected_text(RichText::new("+ Bind an ability").color(FAINT).size(11.5))
                    .width(220.0)
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        for id in &abilities {
                            let name = me.index.get(id).map_or(id.as_str(), |e| e.name.as_str());
                            if ui.selectable_label(false, name).on_hover_text(id).clicked() {
                                bind = Some(id.clone());
                            }
                        }
                    });
                if ui.button("New ability…").clicked() {
                    create = true;
                }
            });
        });
        if let Some(i) = remove {
            let pointer = format!("/behaviors/abilities/bindings/{i}");
            let command = Command::Remove {
                document: self.document.to_string(),
                pointer: pointer.clone(),
            };
            self.step("Unbind an ability", &pointer, vec![command], false);
        }
        if let Some(id) = bind {
            let binding = new_binding(&bindings, &id, hero);
            let command = if value.pointer("/behaviors/abilities").is_some() {
                Command::Insert {
                    document: self.document.to_string(),
                    pointer: "/behaviors/abilities/bindings".into(),
                    index: bindings.len(),
                    value: binding,
                }
            } else {
                Command::Set {
                    document: self.document.to_string(),
                    pointer: "/behaviors/abilities".into(),
                    value: json!({"maxMana": 200, "manaRegenPerSecond": 1, "bindings": [binding]}),
                    at: None,
                }
            };
            self.step(
                &format!("Bind {id}"),
                "/behaviors/abilities/bindings",
                vec![command],
                false,
            );
        }
        if create {
            let tail = value
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.rsplit('.').next())
                .unwrap_or("new");
            self.create = Some(format!("ability.{tail}."));
        }
    }

    fn building(&mut self, ui: &mut Ui, value: &Value) {
        let rows = vec![
            (
                "Health",
                number(value, "/body/maxHp").map_or("–".into(), |v| format!("{v:.0}")),
            ),
            (
                "Cost",
                format!(
                    "{} · {}",
                    cost(value.pointer("/creation/items")),
                    seconds(number(value, "/creation/workTicks").unwrap_or(0.0))
                ),
            ),
            (
                "Footprint",
                value.get("footprint").map_or("–".into(), |f| {
                    format!(
                        "{} × {}",
                        f.get("width").map_or("?".into(), |v| v.to_string()),
                        f.get("depth")
                            .or_else(|| f.get("height"))
                            .map_or("?".into(), |v| v.to_string())
                    )
                }),
            ),
            (
                "Sight",
                number(value, "/vision").map_or("–".into(), |v| format!("{v}")),
            ),
        ];
        Self::card(ui, "Building", |ui| {
            Self::stats(ui, "building-stats", &rows)
        });
        for (title, key, prefixes) in [
            ("Trains", "production", &["unit.", "item."][..]),
            ("Researches", "research", &["research."][..]),
        ] {
            self.outputs(ui, value, title, key, prefixes);
        }
        let me = self;
        Self::card(ui, "Built by", |ui| {
            me.users(
                ui,
                |u| u.pointer.contains("construction") || u.pointer.contains("roster"),
                "No race lists it.",
            )
        });
    }

    /// A building's `behaviors.<key>.outputs`: links, removing, adding from a picker.
    fn outputs(&mut self, ui: &mut Ui, value: &Value, title: &str, key: &str, prefixes: &[&str]) {
        let pointer = format!("/behaviors/{key}/outputs");
        let Some(outputs) = value.pointer(&pointer).and_then(Value::as_array).cloned() else {
            return;
        };
        let choices: Vec<String> = self
            .index
            .keys()
            .filter(|id| {
                prefixes.iter().any(|p| id.starts_with(p)) && !outputs.iter().any(|o| o == *id)
            })
            .cloned()
            .collect();
        let mut remove = None;
        let mut add = None;
        let me = &mut *self;
        Self::card(ui, title, |ui| {
            for (i, output) in outputs.iter().enumerate() {
                ui.horizontal(|ui| {
                    if let Some(id) = output.as_str() {
                        me.link(ui, id);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            ComboBox::from_id_salt(("outputs", me.document, key))
                .selected_text(RichText::new("+ Add").color(FAINT).size(11.5))
                .width(220.0)
                .height(320.0)
                .show_ui(ui, |ui| {
                    for id in &choices {
                        let name = me.index.get(id).map_or(id.as_str(), |e| e.name.as_str());
                        if ui.selectable_label(false, name).on_hover_text(id).clicked() {
                            add = Some(id.clone());
                        }
                    }
                });
        });
        if let Some(i) = remove {
            let at = format!("{pointer}/{i}");
            let command = Command::Remove {
                document: self.document.to_string(),
                pointer: at.clone(),
            };
            self.step(&format!("Remove from {title}"), &at, vec![command], false);
        }
        if let Some(id) = add {
            let command = Command::Insert {
                document: self.document.to_string(),
                pointer: pointer.clone(),
                index: outputs.len(),
                value: Value::String(id.clone()),
            };
            self.step(&format!("Add {id}"), &pointer, vec![command], false);
        }
    }

    fn item(&mut self, ui: &mut Ui, value: &Value) {
        let mut rows = vec![
            (
                "Tier",
                number(value, "/itemTier").map_or("–".into(), |v| format!("{v:.0}")),
            ),
            (
                "Stack",
                number(value, "/stackLimit").map_or("–".into(), |v| format!("{v:.0}")),
            ),
        ];
        if let Some(Value::Object(effect)) = value.get("itemEffect") {
            for (key, v) in effect {
                if key != "type" && v.as_f64().is_some_and(|n| n != 0.0) {
                    rows.push((key.as_str(), v.to_string()));
                }
            }
        }
        let kind = text(value, "/itemEffect/type")
            .unwrap_or("item")
            .to_string();
        Self::card(ui, &kind, |ui| Self::stats(ui, "item-stats", &rows));
        let me = self;
        Self::card(ui, "Found in", |ui| {
            me.users(ui, |_| true, "Nothing names it.")
        });
    }

    fn ability(&mut self, ui: &mut Ui, value: &Value) {
        self.ranks(ui, value);
        // The cast drawn to scale: prepare, channel, recover; then the cooldown.
        let prepare = number(value, "/cast/prepareTicks").unwrap_or(0.0);
        let recover = number(value, "/cast/recoverTicks").unwrap_or(0.0);
        let channel = number(value, "/cast/channel/ticks")
            .or_else(|| number(value, "/cast/channel/durationTicks"))
            .unwrap_or(0.0);
        let first_rank = value.pointer("/ranks/0").cloned().unwrap_or(Value::Null);
        let amount = |pointer: &str| match value.pointer(pointer) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::Object(o)) => o
                .get("rankParameter")
                .and_then(Value::as_str)
                .and_then(|p| first_rank.get(p))
                .and_then(Value::as_f64),
            _ => None,
        };
        let cooldown = amount("/cast/cooldown/ticks").unwrap_or(0.0);
        let mana = amount("/cast/cost/amount");
        Self::card(ui, "Cast", |ui| {
            let total = (prepare + channel + recover).max(1.0);
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
            let painter = ui.painter();
            let mut x = rect.left();
            for (ticks, label, color) in [
                (prepare, "Prepare", STRONG),
                (channel, "Channel", CONTROL),
                (recover, "Recover", SELECTED),
            ] {
                if ticks <= 0.0 {
                    continue;
                }
                let w = rect.width() * (ticks / total) as f32;
                let r = egui::Rect::from_min_size(
                    egui::pos2(x, rect.top()),
                    egui::vec2(w, rect.height()),
                );
                painter.rect_filled(r, 4, color);
                painter.text(
                    r.left_center() + egui::vec2(6.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    format!("{label} {}", seconds(ticks)),
                    egui::FontId::proportional(10.5),
                    TEXT,
                );
                x += w;
            }
            painter.line_segment(
                [
                    egui::pos2(
                        rect.left() + rect.width() * (prepare / total) as f32,
                        rect.top() - 3.0,
                    ),
                    egui::pos2(
                        rect.left() + rect.width() * (prepare / total) as f32,
                        rect.bottom() + 3.0,
                    ),
                ],
                egui::Stroke::new(2.0, TEXT),
            );
            let mut rows = vec![("Release", format!("after {}", seconds(prepare)))];
            rows.push(("Cooldown", seconds(cooldown)));
            if let Some(mana) = mana {
                rows.push(("Mana", format!("{mana:.0} (rank 1)")));
            }
            Self::stats(ui, "cast-stats", &rows);
        });
        let targeting = value.get("targeting").cloned().unwrap_or(Value::Null);
        let words = {
            let kind = text(&targeting, "/kind").unwrap_or("?");
            let relations: Vec<&str> = targeting
                .get("relations")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let range = number(&targeting, "/range").unwrap_or(0.0);
            let radius = number(&targeting, "/radius");
            let mut s = format!(
                "{} · {}",
                match kind {
                    "self" => "On the caster".to_string(),
                    "point" => "On a point".to_string(),
                    "unit" | "entity" => "On a unit".to_string(),
                    other => other.to_string(),
                },
                if relations.is_empty() {
                    "anyone".to_string()
                } else {
                    relations.join(", ")
                }
            );
            if range > 0.0 {
                s.push_str(&format!(" · range {range}"));
            }
            if let Some(radius) = radius {
                s.push_str(&format!(" · radius {radius}"));
            }
            s
        };
        Self::card(ui, "Targeting", |ui| {
            ui.label(RichText::new(words).color(TEXT));
        });
        // The units that bind it, each with Try (a test match where it casts this).
        let mut binders: Vec<String> = self
            .uses
            .iter()
            .filter(|u| u.document != self.document && u.pointer.ends_with("/ability"))
            .filter_map(|u| {
                self.index
                    .iter()
                    .find(|(_, e)| e.document == u.document)
                    .map(|(id, _)| id.clone())
            })
            .collect();
        binders.dedup();
        let mut try_with = None;
        let me = &mut *self;
        Self::card(ui, "Bound by", |ui| {
            if binders.is_empty() {
                ui.label(RichText::new("No unit binds it.").color(FAINT).size(11.5));
            }
            for id in &binders {
                ui.horizontal(|ui| {
                    me.link(ui, id);
                    if ui
                        .small_button("Try it")
                        .on_hover_text("A test match: this unit near your start with allies and creeps around it")
                        .clicked()
                    {
                        try_with = Some(id.clone());
                    }
                });
            }
        });
        self.try_with = try_with;
    }

    /// The rank table: parameters by rank, each cell a number.
    fn ranks(&mut self, ui: &mut Ui, value: &Value) {
        let Some(ranks) = value.get("ranks").and_then(Value::as_array).cloned() else {
            return;
        };
        let mut names: Vec<String> = Vec::new();
        for rank in &ranks {
            if let Some(object) = rank.as_object() {
                for key in object.keys() {
                    if !names.contains(key) {
                        names.push(key.clone());
                    }
                }
            }
        }
        let mut sets = Vec::new();
        let mut add_rank = false;
        let mut drop_rank = None;
        let mut add_parameter = None;
        let document = self.document.to_string();
        Self::card(ui, "Ranks", |ui| {
            Grid::new(("ranks", &document))
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("");
                    for (i, _) in ranks.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Rank {}", i + 1))
                                    .color(FAINT)
                                    .size(11.0),
                            );
                            if ranks.len() > 1
                                && ui
                                    .small_button("×")
                                    .on_hover_text("Remove this rank")
                                    .clicked()
                            {
                                drop_rank = Some(i);
                            }
                        });
                    }
                    ui.end_row();
                    for name in &names {
                        ui.label(RichText::new(name).color(MUTED).size(11.5));
                        for (i, rank) in ranks.iter().enumerate() {
                            match rank.get(name).and_then(Value::as_i64) {
                                Some(mut n) => {
                                    if ui.add(DragValue::new(&mut n).speed(0.2)).changed() {
                                        sets.push((format!("/ranks/{i}/{name}"), n));
                                    }
                                }
                                None => {
                                    if ui
                                        .small_button("+")
                                        .on_hover_text("Give this rank the parameter")
                                        .clicked()
                                    {
                                        sets.push((format!("/ranks/{i}/{name}"), 0));
                                    }
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            ui.horizontal(|ui| {
                if ui
                    .button("+ Rank")
                    .on_hover_text("A copy of the last rank")
                    .clicked()
                {
                    add_rank = true;
                }
                let id = egui::Id::new(("new-parameter", &document));
                let mut name = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
                ui.add(
                    TextEdit::singleline(&mut name)
                        .hint_text("New parameter")
                        .desired_width(130.0),
                );
                let valid = !name.trim().is_empty() && !names.contains(&name.trim().to_string());
                if ui
                    .add_enabled(valid, egui::Button::new("+ Parameter"))
                    .clicked()
                {
                    add_parameter = Some(name.trim().to_string());
                    name.clear();
                }
                ui.data_mut(|d| d.insert_temp(id, name));
            });
        });
        for (pointer, n) in sets {
            self.set(
                &format!("Set {}", pointer.rsplit('/').next().unwrap_or("")),
                &pointer,
                json!(n),
                true,
            );
        }
        if add_rank && let Some(last) = ranks.last() {
            let command = Command::Insert {
                document: self.document.to_string(),
                pointer: "/ranks".into(),
                index: ranks.len(),
                value: last.clone(),
            };
            self.step("Add a rank", "/ranks", vec![command], false);
        }
        if let Some(i) = drop_rank {
            let pointer = format!("/ranks/{i}");
            let command = Command::Remove {
                document: self.document.to_string(),
                pointer: pointer.clone(),
            };
            self.step(
                &format!("Remove rank {}", i + 1),
                &pointer,
                vec![command],
                false,
            );
        }
        if let Some(name) = add_parameter {
            let commands = (0..ranks.len())
                .map(|i| Command::Set {
                    document: self.document.to_string(),
                    pointer: format!("/ranks/{i}/{name}"),
                    value: json!(0),
                    at: None,
                })
                .collect();
            self.step(
                &format!("Add the rank parameter {name}"),
                "/ranks",
                commands,
                false,
            );
        }
    }

    fn effect(&mut self, ui: &mut Ui, value: &Value) {
        let duration = number(value, "/durationTicks").unwrap_or(0.0);
        let looping = value.get("loop").and_then(Value::as_bool).unwrap_or(false);
        let layers = value
            .get("layers")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Self::card(ui, "Effect", |ui| {
            Self::stats(
                ui,
                "effect-stats",
                &[
                    (
                        "Lasts",
                        if looping {
                            format!("{} (loops)", seconds(duration))
                        } else {
                            seconds(duration)
                        },
                    ),
                    ("Layers", layers.len().to_string()),
                ],
            );
        });
        Self::card(ui, "Layers", |ui| {
            let total = duration.max(1.0);
            for layer in &layers {
                let start = number(layer, "/startTick").unwrap_or(0.0);
                let length = number(layer, "/durationTicks").unwrap_or(duration);
                let enabled = layer
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(text(layer, "/id").unwrap_or("layer"))
                            .color(if enabled { SOFT } else { FAINT })
                            .size(11.5),
                    );
                    ui.label(
                        RichText::new(text(layer, "/shape").unwrap_or(""))
                            .color(FAINT)
                            .size(11.0),
                    );
                });
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 8.0),
                    egui::Sense::hover(),
                );
                let a = rect.left() + rect.width() * (start / total).min(1.0) as f32;
                let b = rect.left() + rect.width() * ((start + length) / total).min(1.0) as f32;
                ui.painter().rect_filled(rect, 3, SELECTED);
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(a, rect.top()),
                        egui::pos2(b.max(a + 2.0), rect.bottom()),
                    ),
                    3,
                    if enabled { MUTED } else { STRONG },
                );
            }
        });
        let me = self;
        Self::card(ui, "Used by", |ui| {
            me.users(ui, |_| true, "Nothing plays it.")
        });
    }

    fn race(&mut self, ui: &mut Ui, value: &Value) {
        if let Some(Value::Object(roster)) = value.get("roster") {
            for (group, ids) in roster {
                let ids: Vec<String> = ids
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let title = crate::forms::words(group);
                let me = &mut *self;
                Self::card(ui, &title, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for id in &ids {
                            me.link(ui, id);
                        }
                    });
                });
            }
        }
        if let Some(setup) = value.get("startingSetup") {
            let me = &mut *self;
            Self::card(ui, "Starting setup", |ui| {
                ui.horizontal_wrapped(|ui| {
                    if let Some(fort) = setup.get("fort").and_then(Value::as_str) {
                        me.link(ui, fort);
                    }
                    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
                    for unit in setup
                        .get("units")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let Some(id) = unit.get("definition").and_then(Value::as_str) {
                            *counts.entry(id.to_string()).or_default() += 1;
                        }
                    }
                    for (id, n) in counts {
                        ui.label(RichText::new(format!("{n} ×")).color(FAINT));
                        me.link(ui, &id);
                    }
                });
                if let Some(Value::Object(inventory)) = setup.get("inventory") {
                    let items: Vec<String> = inventory
                        .iter()
                        .map(|(k, v)| format!("{v} {}", k.trim_start_matches("item.")))
                        .collect();
                    ui.label(RichText::new(items.join(" · ")).color(MUTED).size(11.5));
                }
            });
        }
    }
}

/// A new binding of `ability`: the next free hotkey and column, everyone may use it,
/// heroes learn it at levels 1, 3 and 5.
fn new_binding(bindings: &[Value], ability: &str, hero: bool) -> Value {
    let taken = |key: &str| {
        bindings
            .iter()
            .any(|b| b.pointer("/command/hotkey") == Some(&json!(key)))
    };
    let hotkey = ["Q", "W", "E", "R", "D", "F"]
        .into_iter()
        .find(|k| !taken(k))
        .unwrap_or("T");
    let column = (1..=5)
        .find(|c| {
            !bindings
                .iter()
                .any(|b| b.pointer("/command/column") == Some(&json!(c)))
        })
        .unwrap_or(5);
    let mut binding = json!({
        "id": ability.rsplit('.').next().unwrap_or(ability),
        "ability": ability,
        "initialRank": if hero { 0 } else { 1 },
        "controls": ["player", "ai", "scenario"],
        "command": {"hotkey": hotkey, "column": column},
        "ai": {"preference": "enemy", "intervalTicks": 20}
    });
    if hero {
        binding["learning"] = json!({"requiredLevels": [1, 3, 5]});
    }
    binding
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_bindings_take_the_next_free_hotkey_and_column() {
        let bindings = vec![
            json!({"ability": "ability.a", "command": {"hotkey": "Q", "column": 1}}),
            json!({"ability": "ability.b", "command": {"hotkey": "E", "column": 3}}),
        ];
        let hero = new_binding(&bindings, "ability.x.blast", true);
        assert_eq!(hero["command"], json!({"hotkey": "W", "column": 2}));
        assert_eq!(hero["learning"]["requiredLevels"], json!([1, 3, 5]));
        assert_eq!(hero["initialRank"], 0);
        let unit = new_binding(&[], "ability.x.blast", false);
        assert_eq!(unit["id"], "blast");
        assert_eq!(unit["initialRank"], 1);
        assert!(unit.get("learning").is_none());
        assert_eq!(
            cost(Some(
                &json!([{"item": "item.amber", "amount": 425}, {"item": "item.wood", "amount": 100}])
            )),
            "425 amber · 100 wood"
        );
        assert_eq!(seconds(80.0), "2 s");
        assert_eq!(seconds(73.0), "1.82 s");
    }
}
