//! The inspector: the open element's name, id and file, where it is used, and renaming or
//! deleting it.

use crate::document::{Command, Context, Store};
use crate::library::Kind;
use crate::references::Use;
use crate::tabs::{Doc, Edit};
use crate::theme::*;
use crate::widgets::*;
use egui::{Frame, Id, Margin, RichText, Sense, TextEdit, Ui};
use egui_dock::DockState;
use serde_json::Value;

/// What the inspector asks the shell for.
pub enum Request {
    Rename(String),
    Delete(String),
    Reveal(String),
}

pub struct Inspect<'a> {
    pub kind: Kind,
    pub store: &'a mut Store,
    pub edits: &'a mut Vec<Edit>,
    /// Uses of the open element's id, in every document.
    pub uses: Option<&'a [Use]>,
    /// Whether the open element's id may be renamed and the element deleted.
    pub can_rename: bool,
    pub can_delete: bool,
    /// The open element's model in the viewport.
    pub preview: Option<Preview<'a>>,
}

/// The viewport at the inspector's top: the model, its picture and camera, and where this
/// frame's request goes.
pub struct Preview<'a> {
    pub model: String,
    pub scale: f32,
    pub texture: Option<egui::TextureId>,
    pub error: Option<&'a str>,
    pub orbit: &'a mut crate::stage::Orbit,
    pub shot: &'a mut Option<crate::stage::Shot>,
    /// The model's animation clips (name, seconds) and the one playing.
    pub clips: Vec<(String, f32)>,
    pub clip: &'a mut Option<usize>,
}

fn viewport(ui: &mut Ui, preview: Preview) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, width * 0.8), Sense::drag());
    let painter = ui.painter();
    painter.rect_filled(rect, 7, RAISED);
    match (preview.error, preview.texture) {
        (Some(error), _) => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(11.0),
                FAINT,
            );
        }
        (None, Some(texture)) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture, rect, uv, egui::Color32::WHITE);
        }
        (None, None) => {}
    }
    painter.rect_stroke(
        rect,
        7,
        egui::Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.left_bottom() + egui::vec2(8.0, -8.0),
        egui::Align2::LEFT_BOTTOM,
        "Drag to turn · scroll to zoom",
        egui::FontId::proportional(10.0),
        FAINT,
    );
    let orbit = preview.orbit;
    if response.dragged() {
        let delta = response.drag_delta();
        orbit.yaw -= delta.x * 0.01;
        orbit.pitch = (orbit.pitch + delta.y * 0.01).clamp(-0.2, 1.45);
    }
    if response.double_clicked() {
        *orbit = crate::stage::Orbit::default();
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            orbit.distance = (orbit.distance * (-scroll * 0.003).exp()).clamp(0.8, 8.0);
        }
    }
    if !preview.clips.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            if ui
                .selectable_label(preview.clip.is_none(), "rest")
                .clicked()
            {
                *preview.clip = None;
            }
            for (i, (name, seconds)) in preview.clips.iter().enumerate() {
                let label = ui
                    .selectable_label(*preview.clip == Some(i), name.as_str())
                    .on_hover_text(format!("{seconds:.2} s"));
                if label.clicked() {
                    *preview.clip = Some(i);
                }
            }
        });
    }
    let clip = preview
        .clip
        .and_then(|i| preview.clips.get(i).map(|(_, s)| (i as u16, *s)));
    let scale = ui.ctx().pixels_per_point();
    *preview.shot = Some(crate::stage::Shot {
        model: preview.model,
        scale: preview.scale,
        size: [
            (rect.width() * scale) as u32,
            (rect.height() * scale) as u32,
        ],
        orbit: *orbit,
        clip,
    });
}

pub fn show(ui: &mut Ui, dock: &mut DockState<Doc>, inspect: Inspect) -> Option<Request> {
    let mut request = None;
    egui::Panel::right(Id::new(("inspector", inspect.kind)))
        .default_size(INSPECTOR)
        .size_range(260.0..=480.0)
        .resizable(true)
        .show_separator_line(true)
        .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(14)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let Some((_, doc)) = dock.main_surface_mut().find_active_focused() else {
                ui.label(eyebrow("Inspector"));
                ui.label(RichText::new("Open an element from the library.").color(FAINT));
                return;
            };
            if let Some(preview) = inspect.preview {
                viewport(ui, preview);
            }
            ui.label(RichText::new("Properties").size(13.0).color(TEXT).strong());
            let value = inspect.store.get(&doc.document).ok().flatten().cloned();
            let field_of = |name: &str| {
                value
                    .as_ref()
                    .and_then(|v| v.get(name))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let named = value
                .as_ref()
                .and_then(|v| crate::text::key(&doc.document, v))
                .zip(crate::text::file(&doc.document));
            if let Some((key, file)) = named {
                text_fields(
                    ui,
                    (inspect.kind, &mut *inspect.store, &mut *inspect.edits),
                    doc,
                    &key,
                    &file,
                );
            } else {
                match field_of("name") {
                    Some(name) => {
                        ui.label(RichText::new("Name").color(MUTED).size(11.0));
                        let id = Id::new(("name-field", &doc.document));
                        if !ui.memory(|m| m.has_focus(id)) {
                            doc.name_field = name.clone();
                        }
                        let response = ui.add(
                            TextEdit::singleline(&mut doc.name_field)
                                .id(id)
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8, 6)),
                        );
                        // Committed as it is typed (one undo step), so a click elsewhere or a
                        // save never loses it.
                        if response.changed() && doc.name_field != name {
                            inspect.edits.push(Edit {
                                label: format!("Rename {name}"),
                                context: Context {
                                    editor: inspect.kind,
                                    document: doc.document.clone(),
                                    pointer: Some("/name".into()),
                                },
                                commands: vec![Command::Set {
                                    document: doc.document.clone(),
                                    pointer: "/name".into(),
                                    value: Value::String(doc.name_field.clone()),
                                    at: None,
                                }],
                                merge: Some(format!("{}/name", doc.document)),
                            });
                        }
                    }
                    None => field(ui, "Name", &doc.name, false),
                }
            }
            let id = field_of("id").unwrap_or_else(|| doc.id.clone());
            field(ui, "Id", &id, true);
            field(ui, "File", &doc.document, true);
            ui.horizontal(|ui| {
                if inspect.can_rename && ui.button("Rename id…").clicked() {
                    request = Some(Request::Rename(doc.document.clone()));
                }
                if inspect.can_delete && ui.button("Delete…").clicked() {
                    request = Some(Request::Delete(doc.document.clone()));
                }
            });
            if let Some(uses) = inspect.uses {
                ui.add_space(6.0);
                ui.label(eyebrow("Used by"));
                let mut documents: Vec<&str> = uses
                    .iter()
                    .map(|u| u.document.as_str())
                    .filter(|d| *d != doc.document)
                    .collect();
                documents.dedup();
                if documents.is_empty() {
                    ui.label(RichText::new("Nothing uses it.").color(FAINT).size(11.0));
                }
                for document in documents.iter().take(16) {
                    let label = egui::Label::new(
                        RichText::new(*document).monospace().color(SOFT).size(11.0),
                    )
                    .truncate()
                    .sense(Sense::click());
                    if ui
                        .add(label)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        request = Some(Request::Reveal(document.to_string()));
                    }
                }
                if documents.len() > 16 {
                    ui.label(
                        RichText::new(format!("and {} more", documents.len() - 16))
                            .color(FAINT)
                            .size(11.0),
                    );
                }
            }
        });
    request
}

/// A named element's name and description, edited in its package's text library (D96):
/// each keystroke is one undoable edit of the text file, created when it is missing.
fn text_fields(
    ui: &mut Ui,
    (kind, store, edits): (Kind, &mut Store, &mut Vec<Edit>),
    doc: &mut Doc,
    key: &str,
    file: &str,
) {
    let text = store.get(file).ok().flatten().cloned();
    for (field, multiline) in [("name", false), ("description", true)] {
        let entry = format!("{key}.{field}");
        let current = text
            .as_ref()
            .and_then(|t| t.get(&entry))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        ui.label(
            RichText::new(if multiline { "Description" } else { "Name" })
                .color(MUTED)
                .size(11.0),
        );
        let id = Id::new((field, "text-field", &doc.document));
        let buffer = if multiline {
            &mut doc.description_field
        } else {
            &mut doc.name_field
        };
        if !ui.memory(|m| m.has_focus(id)) {
            *buffer = current.clone();
        }
        let edit = if multiline {
            TextEdit::multiline(buffer).desired_rows(3)
        } else {
            TextEdit::singleline(buffer)
        };
        let response = ui.add(
            edit.id(id)
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(8, 6)),
        );
        if response.changed() && *buffer != current {
            let value = Value::String(buffer.clone());
            let command = if text.is_some() {
                Command::Set {
                    document: file.to_string(),
                    pointer: crate::text::pointer(&entry),
                    value,
                    at: None,
                }
            } else {
                Command::Create {
                    document: file.to_string(),
                    value: serde_json::json!({ entry.clone(): value }),
                }
            };
            edits.push(Edit {
                label: format!("Edit {entry}"),
                context: Context {
                    editor: kind,
                    document: file.to_string(),
                    pointer: Some(crate::text::pointer(&entry)),
                },
                commands: vec![command],
                merge: Some(format!("{file}#{entry}")),
            });
        }
        ui.add_space(4.0);
    }
}
