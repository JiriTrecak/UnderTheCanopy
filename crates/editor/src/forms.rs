//! Forms generated from a document's JSON Schema (docs/editor.md): every element gets one.
//! Objects list their fields in schema order with "add field" for missing optional ones,
//! lists and maps add and remove entries, enums and tagged variants are menus, ids get a
//! searchable picker. Without a schema the form follows the value's own shape. Every
//! change becomes a [`Command`]; the shell runs them as undo steps.

use crate::document::Command;
use crate::theme::*;
use egui::{ComboBox, DragValue, Id, Margin, RichText, Sense, TextEdit, Ui, vec2};
use serde_json::{Map, Value};

/// A change a form asks for.
pub struct Change {
    pub label: String,
    /// The field it changed.
    pub pointer: String,
    pub commands: Vec<Command>,
    /// Continuous edits (dragging a number) fold into one undo step.
    pub merge: bool,
}

pub struct Form<'a> {
    /// The root schema (`$defs` are resolved against it), if the format has one.
    pub root: Option<&'a Value>,
    pub document: &'a str,
    /// Every element id, for pickers.
    pub ids: &'a [String],
    /// A field to mark (the place an undo or redo changed).
    pub highlight: Option<&'a str>,
    pub changes: Vec<Change>,
}

const LABEL: f32 = 150.0;

/// Escapes one JSON pointer token.
fn token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn words(key: &str) -> String {
    let mut out = String::new();
    for (i, c) in key.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl<'a> Form<'a> {
    pub fn new(root: Option<&'a Value>, document: &'a str, ids: &'a [String]) -> Self {
        Self {
            root,
            document,
            ids,
            highlight: None,
            changes: Vec::new(),
        }
    }

    /// Follows `$ref`s and single-entry `allOf`s.
    fn resolve(&self, schema: &'a Value) -> &'a Value {
        let mut schema = schema;
        for _ in 0..16 {
            if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
                let name = reference.trim_start_matches("#/$defs/");
                match self
                    .root
                    .and_then(|r| r.get("$defs"))
                    .and_then(|d| d.get(name))
                {
                    Some(target) => schema = target,
                    None => return schema,
                }
            } else if let Some([only]) = schema
                .get("allOf")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
            {
                schema = only;
            } else {
                return schema;
            }
        }
        schema
    }

    /// The schema without its `null` alternative (`Option`), resolved.
    fn present(&self, schema: &'a Value) -> &'a Value {
        let schema = self.resolve(schema);
        for key in ["anyOf", "oneOf"] {
            if let Some(list) = schema.get(key).and_then(Value::as_array) {
                let non_null: Vec<&Value> = list
                    .iter()
                    .filter(|s| s.get("type").and_then(Value::as_str) != Some("null"))
                    .collect();
                if non_null.len() == 1 && non_null.len() < list.len() {
                    return self.present(non_null[0]);
                }
            }
        }
        schema
    }

    fn variants(&self, schema: &'a Value) -> Vec<&'a Value> {
        ["oneOf", "anyOf"]
            .iter()
            .filter_map(|k| schema.get(*k).and_then(Value::as_array))
            .flatten()
            .map(|s| self.resolve(s))
            .filter(|s| s.get("type").and_then(Value::as_str) != Some("null"))
            .collect()
    }

    /// A starting value for `schema`: its default, its constant, or the empty form of its
    /// type with every required field.
    pub fn default_of(&self, schema: &'a Value) -> Value {
        let schema = self.present(schema);
        if let Some(value) = schema.get("default").or_else(|| schema.get("const")) {
            return value.clone();
        }
        if let Some(first) = schema
            .get("enum")
            .and_then(Value::as_array)
            .and_then(|e| e.first())
        {
            return first.clone();
        }
        if let Some(first) = self.variants(schema).first() {
            return self.default_of(first);
        }
        match type_of(schema) {
            Some("object") => {
                let mut object = Map::new();
                let required = required(schema);
                if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                    for (name, property) in properties {
                        if required.contains(&name.as_str()) {
                            object.insert(name.clone(), self.default_of(property));
                        }
                    }
                }
                Value::Object(object)
            }
            Some("array") => Value::Array(Vec::new()),
            Some("string") => Value::String(String::new()),
            Some("integer") | Some("number") => {
                schema.get("minimum").cloned().unwrap_or(Value::from(0))
            }
            Some("boolean") => Value::Bool(false),
            _ => Value::Null,
        }
    }

    fn set(&mut self, pointer: &str, label: &str, value: Value, merge: bool) {
        self.changes.push(Change {
            label: label.to_string(),
            pointer: pointer.to_string(),
            commands: vec![Command::Set {
                document: self.document.to_string(),
                pointer: pointer.to_string(),
                value,
                at: None,
            }],
            merge,
        });
    }

    fn command(&mut self, pointer: &str, label: String, command: Command) {
        self.changes.push(Change {
            label,
            pointer: pointer.to_string(),
            commands: vec![command],
            merge: false,
        });
    }

    /// The whole document.
    pub fn show(&mut self, ui: &mut Ui, value: &Value) {
        ui.spacing_mut().item_spacing.y = 4.0;
        let root = self.root;
        let schema = root.unwrap_or(&Value::Null);
        match value {
            Value::Object(object) => self.object(ui, schema, object, "", 0),
            other => self.value(ui, "Document", schema, other, "", 0, None),
        }
    }

    /// A row: the label column and the widget.
    fn row(
        &mut self,
        ui: &mut Ui,
        label: &str,
        pointer: &str,
        description: Option<&str>,
        remove: Option<&str>,
        add: impl FnOnce(&mut Self, &mut Ui),
    ) {
        let marked = self.highlight == Some(pointer);
        let frame = egui::Frame::new()
            .fill(if marked {
                SELECTED
            } else {
                egui::Color32::TRANSPARENT
            })
            .corner_radius(5)
            .inner_margin(Margin::symmetric(4, 1));
        let response = frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, response) = ui.allocate_exact_size(vec2(LABEL, 24.0), Sense::hover());
                ui.painter().text(
                    rect.left_center(),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(11.5),
                    MUTED,
                );
                if let Some(text) = description {
                    response.on_hover_text(text);
                }
                add(self, ui);
                if let Some(name) = remove {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text(format!("Remove {name}"))
                            .clicked()
                        {
                            self.command(
                                pointer,
                                format!("Remove {name}"),
                                Command::Remove {
                                    document: self.document.to_string(),
                                    pointer: pointer.to_string(),
                                },
                            );
                        }
                    });
                }
            });
        });
        if marked {
            response.response.scroll_to_me(Some(egui::Align::Center));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn value(
        &mut self,
        ui: &mut Ui,
        label: &str,
        schema: &'a Value,
        value: &Value,
        pointer: &str,
        depth: usize,
        remove: Option<&str>,
    ) {
        let schema = self.present(schema);
        let description = schema.get("description").and_then(Value::as_str);
        // Containers open a section; everything else is one row.
        let variants = self.variants(schema);
        let tagged = (!variants.is_empty()).then(|| tag_of(&variants)).flatten();
        if let (Some(tag), Value::Object(object)) = (&tagged, value) {
            self.tagged(ui, label, &variants, tag, object, pointer, depth, remove);
            return;
        }
        if !variants.is_empty() && enum_strings(&variants).is_none() {
            // Untagged alternatives: the one matching the value's kind.
            let chosen = variants
                .iter()
                .find(|v| matches_kind(self, v, value))
                .copied()
                .unwrap_or(variants[0]);
            self.value(ui, label, chosen, value, pointer, depth, remove);
            return;
        }
        match value {
            Value::Object(object) => {
                let map_values = schema.get("additionalProperties").filter(|a| a.is_object());
                self.section(
                    ui,
                    label,
                    pointer,
                    depth,
                    remove,
                    |form, ui| match map_values {
                        Some(entry) if schema.get("properties").is_none() => {
                            form.map(ui, entry, object, pointer, depth + 1)
                        }
                        _ => form.object(ui, schema, object, pointer, depth + 1),
                    },
                );
            }
            Value::Array(items) => {
                let item = schema.get("items").unwrap_or(&Value::Null);
                self.section(ui, label, pointer, depth, remove, |form, ui| {
                    form.list(ui, item, items, pointer, depth + 1)
                });
            }
            _ => self.row(ui, label, pointer, description, remove, |form, ui| {
                form.scalar(ui, label, schema, &variants, value, pointer)
            }),
        }
    }

    fn section(
        &mut self,
        ui: &mut Ui,
        label: &str,
        pointer: &str,
        depth: usize,
        remove: Option<&str>,
        body: impl FnOnce(&mut Self, &mut Ui),
    ) {
        let id = Id::new(("form", self.document, pointer));
        let header = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            depth < 2,
        );
        header
            .show_header(ui, |ui| {
                ui.label(RichText::new(label).color(SOFT).size(12.0));
                if let Some(name) = remove {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text(format!("Remove {name}"))
                            .clicked()
                        {
                            self.command(
                                pointer,
                                format!("Remove {name}"),
                                Command::Remove {
                                    document: self.document.to_string(),
                                    pointer: pointer.to_string(),
                                },
                            );
                        }
                    });
                }
            })
            .body(|ui| {
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 6,
                        right: 0,
                        top: 0,
                        bottom: 4,
                    })
                    .show(ui, |ui| body(self, ui));
            });
    }

    fn object(
        &mut self,
        ui: &mut Ui,
        schema: &'a Value,
        object: &Map<String, Value>,
        pointer: &str,
        depth: usize,
    ) {
        let schema = self.present(schema);
        let properties = schema.get("properties").and_then(Value::as_object);
        let required = required(schema);
        // Fields in schema order, then any the schema does not know.
        let mut order: Vec<&str> = properties
            .map(|p| {
                p.keys()
                    .map(String::as_str)
                    .filter(|k| object.contains_key(*k))
                    .collect()
            })
            .unwrap_or_default();
        let unknown: Vec<&str> = object
            .keys()
            .map(String::as_str)
            .filter(|k| !order.contains(k))
            .collect();
        order.extend(unknown);
        for key in order {
            let child = format!("{pointer}/{}", token(key));
            // The element's own id changes only with its uses (the inspector's rename).
            if pointer.is_empty()
                && key == "id"
                && let Some(id) = object[key].as_str()
            {
                self.row(ui, "Id", &child, None, None, |_, ui| {
                    ui.label(RichText::new(id).monospace().color(SOFT));
                    ui.label(
                        RichText::new("Rename it in the inspector")
                            .color(FAINT)
                            .size(11.0),
                    );
                });
                continue;
            }
            let property = properties.and_then(|p| p.get(key)).unwrap_or(&Value::Null);
            let removable = !required.contains(&key);
            let name = words(key);
            self.value(
                ui,
                &name,
                property,
                &object[key],
                &child,
                depth,
                removable.then_some(name.as_str()),
            );
        }
        // Optional fields not present yet.
        let Some(properties) = properties else {
            return;
        };
        let missing: Vec<&String> = properties
            .keys()
            .filter(|k| !object.contains_key(*k))
            .collect();
        if missing.is_empty() {
            return;
        }
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ComboBox::from_id_salt(("add-field", self.document, pointer))
                .selected_text(RichText::new("+ Add field").color(FAINT).size(11.5))
                .width(160.0)
                .show_ui(ui, |ui| {
                    for name in &missing {
                        if ui.selectable_label(false, words(name)).clicked() {
                            chosen = Some((*name).clone());
                        }
                    }
                });
        });
        if let Some(name) = chosen {
            let value = self.default_of(&properties[&name]);
            // Where it belongs in schema order among the fields present.
            let at = properties
                .keys()
                .take_while(|k| **k != name)
                .filter(|k| object.contains_key(*k))
                .count();
            self.command(
                pointer,
                format!("Add {}", words(&name)),
                Command::Set {
                    document: self.document.to_string(),
                    pointer: format!("{pointer}/{}", token(&name)),
                    value,
                    at: Some(at),
                },
            );
        }
    }

    fn map(
        &mut self,
        ui: &mut Ui,
        entry: &'a Value,
        object: &Map<String, Value>,
        pointer: &str,
        depth: usize,
    ) {
        for (key, value) in object {
            let child = format!("{pointer}/{}", token(key));
            self.value(ui, key, entry, value, &child, depth, Some(key));
        }
        let id = Id::new(("new-key", self.document, pointer));
        let mut key = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
        let mut add = false;
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.add(
                TextEdit::singleline(&mut key)
                    .hint_text("New key")
                    .desired_width(160.0),
            );
            add = ui
                .add_enabled(
                    !key.trim().is_empty() && !object.contains_key(key.trim()),
                    egui::Button::new("+ Add"),
                )
                .clicked();
        });
        if add {
            let name = key.trim().to_string();
            let value = self.default_of(entry);
            self.command(
                pointer,
                format!("Add {name}"),
                Command::Set {
                    document: self.document.to_string(),
                    pointer: format!("{pointer}/{}", token(&name)),
                    value,
                    at: None,
                },
            );
            key.clear();
        }
        ui.data_mut(|d| d.insert_temp(id, key));
    }

    fn list(&mut self, ui: &mut Ui, item: &'a Value, items: &[Value], pointer: &str, depth: usize) {
        for (i, value) in items.iter().enumerate() {
            let child = format!("{pointer}/{i}");
            let label = format!("{}", i + 1);
            self.value(
                ui,
                &label,
                item,
                value,
                &child,
                depth,
                Some(&format!("entry {}", i + 1)),
            );
        }
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            if ui.button("+ Add entry").clicked() {
                let value = self.default_of(item);
                self.command(
                    pointer,
                    "Add an entry".into(),
                    Command::Insert {
                        document: self.document.to_string(),
                        pointer: pointer.to_string(),
                        index: items.len(),
                        value,
                    },
                );
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn tagged(
        &mut self,
        ui: &mut Ui,
        label: &str,
        variants: &[&'a Value],
        tag: &str,
        object: &Map<String, Value>,
        pointer: &str,
        depth: usize,
        remove: Option<&str>,
    ) {
        let current = object
            .get(tag)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let names: Vec<String> = variants
            .iter()
            .filter_map(|v| {
                v.pointer(&format!("/properties/{tag}/const"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
            .collect();
        let chosen = variants
            .iter()
            .find(|v| {
                v.pointer(&format!("/properties/{tag}/const"))
                    .and_then(Value::as_str)
                    == Some(&current)
            })
            .copied();
        let title = format!("{label} · {current}");
        self.section(ui, &title, pointer, depth, remove, |form, ui| {
            let tag_pointer = format!("{pointer}/{}", token(tag));
            let mut switch = None;
            form.row(ui, &words(tag), &tag_pointer, None, None, |form, ui| {
                ComboBox::from_id_salt(("tag", form.document, pointer))
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for name in &names {
                            if ui.selectable_label(*name == current, name).clicked()
                                && *name != current
                            {
                                switch = Some(name.clone());
                            }
                        }
                    });
            });
            if let Some(name) = switch
                && let Some(variant) = variants.iter().find(|v| {
                    v.pointer(&format!("/properties/{tag}/const"))
                        .and_then(Value::as_str)
                        == Some(&name)
                })
            {
                // The new variant's required fields, keeping same-named ones.
                let mut value = form.default_of(variant);
                if let (Value::Object(new), Some(properties)) = (
                    &mut value,
                    variant.get("properties").and_then(Value::as_object),
                ) {
                    for (key, old) in object {
                        if key != tag && properties.contains_key(key) {
                            new.insert(key.clone(), old.clone());
                        }
                    }
                }
                form.set(pointer, &format!("Change {label} to {name}"), value, false);
            }
            if let Some(variant) = chosen {
                let mut rest = object.clone();
                rest.shift_remove(tag);
                form.object(ui, variant, &rest, pointer, depth + 1);
            }
        });
    }

    fn scalar(
        &mut self,
        ui: &mut Ui,
        label: &str,
        schema: &'a Value,
        variants: &[&'a Value],
        value: &Value,
        pointer: &str,
    ) {
        let width = ui.available_width().clamp(120.0, 360.0) - 28.0;
        if let Some(constant) = schema.get("const") {
            ui.label(RichText::new(constant.to_string()).color(FAINT).monospace());
            return;
        }
        let choices = schema
            .get("enum")
            .and_then(Value::as_array)
            .map(|e| {
                e.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .or_else(|| enum_strings(variants));
        if let Some(choices) = choices {
            let current = value.as_str().unwrap_or_default().to_string();
            let mut picked = None;
            ComboBox::from_id_salt(("enum", self.document, pointer))
                .selected_text(&current)
                .width(width.min(220.0))
                .show_ui(ui, |ui| {
                    for choice in &choices {
                        if ui.selectable_label(*choice == current, choice).clicked()
                            && *choice != current
                        {
                            picked = Some(choice.clone());
                        }
                    }
                });
            if let Some(choice) = picked {
                self.set(
                    pointer,
                    &format!("Set {label}"),
                    Value::String(choice),
                    false,
                );
            }
            return;
        }
        match value {
            Value::Bool(on) => {
                let mut on = *on;
                if ui.checkbox(&mut on, "").changed() {
                    self.set(pointer, &format!("Set {label}"), Value::Bool(on), false);
                }
            }
            Value::Number(number) => {
                let integer =
                    type_of(schema) == Some("integer") || number.is_i64() || number.is_u64();
                let low = schema
                    .get("minimum")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::NEG_INFINITY);
                let high = schema
                    .get("maximum")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::INFINITY);
                if integer {
                    let mut n = number
                        .as_i64()
                        .unwrap_or_else(|| number.as_f64().unwrap_or(0.0) as i64);
                    let range =
                        (low.max(i64::MIN as f64) as i64)..=(high.min(i64::MAX as f64) as i64);
                    if ui
                        .add(DragValue::new(&mut n).range(range).speed(0.2))
                        .changed()
                    {
                        self.set(pointer, &format!("Set {label}"), Value::from(n), true);
                    }
                } else {
                    let mut n = number.as_f64().unwrap_or(0.0);
                    if ui
                        .add(
                            DragValue::new(&mut n)
                                .range(low..=high)
                                .speed(0.01)
                                .max_decimals(6),
                        )
                        .changed()
                        && let Some(v) = serde_json::Number::from_f64(n)
                    {
                        self.set(pointer, &format!("Set {label}"), Value::Number(v), true);
                    }
                }
            }
            Value::String(text) => {
                let is_id = schema.get("format").and_then(Value::as_str) == Some("canopy-id");
                self.text(
                    ui,
                    label,
                    text,
                    pointer,
                    width - if is_id { 32.0 } else { 0.0 },
                );
                if is_id {
                    self.picker(ui, label, text, pointer);
                }
            }
            Value::Null => {
                ui.label(RichText::new("null").color(FAINT).monospace());
            }
            _ => {}
        }
    }

    /// A text field that commits when it loses focus.
    fn text(&mut self, ui: &mut Ui, label: &str, current: &str, pointer: &str, width: f32) {
        let id = Id::new(("text", self.document, pointer));
        let mut buffer = ui.data_mut(|d| d.get_temp::<String>(id));
        let focused = ui.memory(|m| m.has_focus(id));
        if !focused || buffer.is_none() {
            buffer = Some(current.to_string());
        }
        let mut text = buffer.unwrap_or_default();
        let response = ui.add(
            TextEdit::singleline(&mut text)
                .id(id)
                .desired_width(width.max(80.0))
                .margin(Margin::symmetric(6, 4)),
        );
        if response.lost_focus() && text != current {
            self.set(
                pointer,
                &format!("Set {label}"),
                Value::String(text.clone()),
                false,
            );
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    }

    /// A searchable menu of every element id.
    fn picker(&mut self, ui: &mut Ui, label: &str, current: &str, pointer: &str) {
        let search_id = Id::new(("pick-search", self.document, pointer));
        let mut search = ui.data_mut(|d| d.get_temp::<String>(search_id).unwrap_or_default());
        let mut picked = None;
        // Ids of the same kind as the current one first (`unit.` with `unit.`).
        let kind = current.split('.').next().unwrap_or_default();
        ComboBox::from_id_salt(("pick", self.document, pointer))
            .selected_text("")
            .width(24.0)
            .height(320.0)
            .show_ui(ui, |ui| {
                ui.add(
                    TextEdit::singleline(&mut search)
                        .hint_text("Search ids…")
                        .desired_width(260.0),
                );
                let needle = search.to_lowercase();
                let mut shown = 0;
                let same = self
                    .ids
                    .iter()
                    .filter(|i| i.starts_with(&format!("{kind}.")));
                let other = self
                    .ids
                    .iter()
                    .filter(|i| !i.starts_with(&format!("{kind}.")));
                for id in same.chain(other) {
                    if !needle.is_empty() && !id.to_lowercase().contains(&needle) {
                        continue;
                    }
                    if ui
                        .selectable_label(id == current, RichText::new(id).monospace())
                        .clicked()
                    {
                        picked = Some(id.clone());
                    }
                    shown += 1;
                    if shown >= 200 {
                        ui.label(RichText::new("More… narrow the search").color(FAINT));
                        break;
                    }
                }
            });
        ui.data_mut(|d| d.insert_temp(search_id, search));
        if let Some(id) = picked
            && id != current
        {
            self.set(pointer, &format!("Set {label}"), Value::String(id), false);
        }
    }
}

fn type_of(schema: &Value) -> Option<&str> {
    match schema.get("type")? {
        Value::String(t) => Some(t),
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .find(|t| *t != "null"),
        _ => None,
    }
}

fn required(schema: &Value) -> Vec<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Alternatives that are all string constants: a menu.
fn enum_strings(variants: &[&Value]) -> Option<Vec<String>> {
    if variants.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for v in variants {
        match (
            v.get("const").and_then(Value::as_str),
            v.get("enum").and_then(Value::as_array),
        ) {
            (Some(c), _) => out.push(c.to_string()),
            (None, Some(list)) if type_of(v) == Some("string") => {
                out.extend(list.iter().filter_map(Value::as_str).map(str::to_string))
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The property every object alternative fixes with a constant (serde's `tag`).
fn tag_of(variants: &[&Value]) -> Option<String> {
    let first = variants.first()?.get("properties")?.as_object()?;
    first
        .iter()
        .filter(|(_, p)| p.get("const").is_some())
        .map(|(k, _)| k.clone())
        .find(|k| {
            variants
                .iter()
                .all(|v| v.pointer(&format!("/properties/{k}/const")).is_some())
        })
}

fn matches_kind(form: &Form, schema: &Value, value: &Value) -> bool {
    let schema = form.present(schema);
    match (type_of(schema), value) {
        (Some("object"), Value::Object(_)) => true,
        (Some("array"), Value::Array(_)) => true,
        (Some("string"), Value::String(_)) => true,
        (Some("integer"), Value::Number(n)) => n.is_i64() || n.is_u64(),
        (Some("number"), Value::Number(_)) => true,
        (Some("boolean"), Value::Bool(_)) => true,
        (None, _) => schema.get("const") == Some(value),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_fill_required_fields_and_follow_references() {
        let root = json!({
            "type": "object",
            "properties": {"cost": {"$ref": "#/$defs/Cost"}, "note": {"type": ["string", "null"]}},
            "required": ["cost"],
            "$defs": {"Cost": {
                "type": "object",
                "properties": {"wood": {"type": "integer", "minimum": 1}, "gold": {"type": "integer"}},
                "required": ["wood"]
            }}
        });
        let form = Form::new(Some(&root), "units/x.json", &[]);
        assert_eq!(form.default_of(&root), json!({"cost": {"wood": 1}}));
        let tagged = json!({"oneOf": [
            {"type": "object", "properties": {"op": {"const": "heal"}, "amount": {"type": "integer"}}, "required": ["op", "amount"]},
            {"type": "object", "properties": {"op": {"const": "push"}}, "required": ["op"]}
        ]});
        let variants = form.variants(&tagged);
        assert_eq!(tag_of(&variants).as_deref(), Some("op"));
        assert_eq!(form.default_of(&tagged), json!({"op": "heal", "amount": 0}));
        assert_eq!(words("healthRegenPerSecond"), "Health regen per second");
    }
}
