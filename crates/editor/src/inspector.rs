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
            ui.label(RichText::new("Properties").size(13.0).color(TEXT).strong());
            let value = inspect.store.get(&doc.document).ok().flatten().cloned();
            let field_of = |name: &str| {
                value
                    .as_ref()
                    .and_then(|v| v.get(name))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
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
                    if response.lost_focus() && doc.name_field != name {
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
                            merge: None,
                        });
                    }
                }
                None => field(ui, "Name", &doc.name, false),
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
