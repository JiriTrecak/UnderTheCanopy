//! Document tabs (egui_dock): each open document as a form built from its schema, or as
//! its JSON text. Changes come back as [`Edit`]s for the shell to run as undo steps.

use crate::document::{Command, Context, Store};
use crate::forms::Form;
use crate::library::{Element, Kind};
use crate::theme::{self, *};
use crate::widgets::*;
use egui::{Frame, Id, RichText, ScrollArea, TextEdit, Ui};
use egui_dock::{DockArea, DockState, TabViewer};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// A map's view: the viewport and the map tools.
    Map,
    Overview,
    Form,
    Json,
}

/// One open document.
pub struct Doc {
    /// The element's name when it was opened (its `name` field wins while it has one).
    pub name: String,
    pub id: String,
    /// Path from the package root.
    pub document: String,
    view: View,
    /// The document's schema, once looked up (`None` inside: the format has none).
    schema: Option<Option<Value>>,
    /// The JSON view: the text of the revision shown, and the user's edit of it.
    shown: Option<u64>,
    text: String,
    edit: String,
    error: Option<String>,
    /// The inspector's name field while it is not being typed in.
    pub name_field: String,
}

impl Doc {
    pub fn new(element: &Element) -> Self {
        Self {
            name: element.name.clone(),
            id: element.id.clone(),
            document: element.document.clone(),
            view: if crate::map::root_of(&element.document).is_some() {
                View::Map
            } else if crate::overview::has(&element.document) {
                View::Overview
            } else {
                View::Form
            },
            schema: None,
            shown: None,
            text: String::new(),
            edit: String::new(),
            error: None,
            name_field: String::new(),
        }
    }

    pub fn show_form(&mut self) {
        self.view = View::Form;
    }
}

/// An edit a panel asks for this frame; it runs once the frame's panels are drawn.
pub struct Edit {
    pub label: String,
    pub context: Context,
    pub commands: Vec<Command>,
    /// Continuous edits with the same key fold into one undo step.
    pub merge: Option<String>,
}

pub struct Viewer<'a> {
    pub kind: Kind,
    pub store: &'a mut Store,
    pub edits: &'a mut Vec<Edit>,
    /// Every element id, for pickers.
    pub ids: &'a [String],
    /// The field an undo or redo changed: (document, pointer).
    pub highlight: Option<(&'a str, &'a str)>,
    /// Every element by id (overviews name and link them).
    pub index: &'a std::collections::BTreeMap<String, crate::overview::Entry>,
    /// Uses of the focused document's element: (document, uses).
    pub uses: Option<(&'a str, &'a [crate::references::Use])>,
    /// Links clicked this frame, and a new element to start.
    pub opens: &'a mut Vec<String>,
    pub create: &'a mut Option<String>,
    pub try_with: &'a mut Option<String>,
    /// Open maps, the game data of the last passing check, the scene's last picture, and
    /// the map view's size this frame (the stage draws the scene at it).
    pub maps: &'a mut std::collections::HashMap<String, crate::map::Session>,
    pub content: Option<std::sync::Arc<canopy_content::Content>>,
    pub scene_texture: Option<egui::TextureId>,
    pub map_size: &'a mut Option<(String, [u32; 2])>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Doc;

    fn id(&mut self, tab: &mut Doc) -> Id {
        Id::new(&tab.document)
    }

    fn title(&mut self, tab: &mut Doc) -> egui::WidgetText {
        let name = match self.store.get(&tab.document) {
            Ok(Some(value)) => value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&tab.name)
                .to_string(),
            _ => tab.name.clone(),
        };
        if self.store.is_dirty(&tab.document) {
            dirty_title(&name)
        } else {
            name.into()
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Doc) {
        let revision = self.store.revision(&tab.document);
        let value = match self.store.get(&tab.document) {
            Ok(Some(value)) => value.clone(),
            Ok(None) => {
                empty(
                    ui,
                    "This document is deleted",
                    "Undo brings it back; saving removes its file.",
                );
                return;
            }
            Err(error) => {
                empty(ui, "This document could not be read", &error.to_string());
                return;
            }
        };
        if tab.shown != Some(revision) {
            let text = crate::document::format(&value);
            // Typing is kept until applied or reverted; otherwise follow the document.
            if tab.edit == tab.text {
                tab.edit = text.clone();
            }
            tab.text = text;
            tab.shown = Some(revision);
        }
        let changed = tab.edit != tab.text;
        ui.horizontal(|ui| {
            let views: &[(View, &str)] = if crate::map::root_of(&tab.document).is_some() {
                &[
                    (View::Map, "Map"),
                    (View::Form, "Form"),
                    (View::Json, "JSON"),
                ]
            } else if crate::overview::has(&tab.document) {
                &[
                    (View::Overview, "Overview"),
                    (View::Form, "Form"),
                    (View::Json, "JSON"),
                ]
            } else {
                &[(View::Form, "Form"), (View::Json, "JSON")]
            };
            for (view, label) in views {
                if ui.selectable_label(tab.view == *view, *label).clicked() {
                    tab.view = *view;
                }
            }
            if tab.view == View::Json && (changed || tab.error.is_some()) {
                ui.add_space(12.0);
                if ui.add(primary("Apply")).clicked() {
                    match serde_json::from_str::<Value>(&tab.edit) {
                        Ok(value) => {
                            tab.error = None;
                            self.edits.push(Edit {
                                label: format!("Edit {} as JSON", tab.name),
                                context: Context {
                                    editor: self.kind,
                                    document: tab.document.clone(),
                                    pointer: None,
                                },
                                commands: vec![Command::Set {
                                    document: tab.document.clone(),
                                    pointer: String::new(),
                                    value,
                                    at: None,
                                }],
                                merge: None,
                            });
                        }
                        Err(error) => tab.error = Some(format!("Not valid JSON: {error}")),
                    }
                }
                if ui.button("Revert").clicked() {
                    tab.edit = tab.text.clone();
                    tab.error = None;
                }
                if let Some(error) = &tab.error {
                    ui.label(RichText::new(error).color(ERROR).size(11.0));
                }
            } else if tab.view == View::Json && changed {
                ui.label(RichText::new("Edited").color(DIRTY).size(11.0));
            }
        });
        ui.add_space(6.0);
        match tab.view {
            View::Map => {
                let Some(root) = crate::map::root_of(&tab.document).map(str::to_string) else {
                    return;
                };
                let docs = match (
                    self.store
                        .get(&format!("{root}/map.json"))
                        .ok()
                        .flatten()
                        .cloned(),
                    self.store
                        .get(&format!("{root}/scenery.json"))
                        .ok()
                        .flatten()
                        .cloned(),
                ) {
                    (Some(map), Some(scenery)) => Some(crate::map::view::Docs { map, scenery }),
                    _ => None,
                };
                let session = self
                    .maps
                    .entry(root.clone())
                    .or_insert_with(|| crate::map::Session::new(&root));
                let out = crate::map::view::show(
                    ui,
                    session,
                    self.content.as_deref(),
                    docs.as_ref(),
                    self.scene_texture,
                );
                self.edits.extend(out.edits);
                if let Some(size) = out.size {
                    *self.map_size = Some((root, size));
                }
            }
            View::Overview => {
                let uses = self
                    .uses
                    .filter(|(document, _)| *document == tab.document)
                    .map_or(&[][..], |(_, uses)| uses);
                let mut overview = crate::overview::Overview::new(&tab.document, self.index, uses);
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| overview.show(ui, &value));
                self.opens.extend(overview.opens);
                if overview.create.is_some() {
                    *self.create = overview.create;
                }
                if overview.try_with.is_some() {
                    *self.try_with = overview.try_with;
                }
                for change in overview.changes {
                    let merge = change
                        .merge
                        .then(|| format!("{}{}", tab.document, change.pointer));
                    self.edits.push(Edit {
                        label: change.label,
                        context: Context {
                            editor: self.kind,
                            document: tab.document.clone(),
                            pointer: Some(change.pointer),
                        },
                        commands: change.commands,
                        merge,
                    });
                }
            }
            View::Form => {
                let schema = tab
                    .schema
                    .get_or_insert_with(|| canopy_content::schema::of_document(&tab.document));
                let mut form = Form::new(schema.as_ref(), &tab.document, self.ids);
                form.highlight = self
                    .highlight
                    .filter(|(document, _)| *document == tab.document)
                    .map(|(_, pointer)| pointer);
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if schema.is_none() {
                            ui.label(
                                RichText::new("This kind of document has no schema yet: the form follows its values.")
                                    .color(FAINT)
                                    .size(11.0),
                            );
                        }
                        form.show(ui, &value);
                    });
                self.opens.extend(std::mem::take(&mut form.opens));
                for change in form.changes {
                    let merge = change
                        .merge
                        .then(|| format!("{}{}", tab.document, change.pointer));
                    self.edits.push(Edit {
                        label: change.label,
                        context: Context {
                            editor: self.kind,
                            document: tab.document.clone(),
                            pointer: Some(change.pointer),
                        },
                        commands: change.commands,
                        merge,
                    });
                }
            }
            View::Json => {
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            TextEdit::multiline(&mut tab.edit)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .frame(Frame::NONE),
                        );
                    });
            }
        }
    }

    fn allowed_in_windows(&self, _tab: &mut Doc) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Doc) -> [bool; 2] {
        [false, false]
    }
}

/// The open documents of one editor, or a hint when none is open.
pub fn documents(ui: &mut Ui, kind: Kind, dock: &mut DockState<Doc>, viewer: &mut Viewer) {
    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(MAIN))
        .show(ui, |ui| {
            if dock.iter_all_tabs().next().is_none() {
                empty(
                    ui,
                    &format!("No {} open", kind.plural()),
                    "Pick one in the library; it opens as a tab here.",
                );
                return;
            }
            DockArea::new(dock)
                .id(Id::new(("dock", kind)))
                .style(theme::dock(ui.style()))
                .show_add_buttons(false)
                .show_close_buttons(true)
                .show_leaf_close_all_buttons(false)
                .show_leaf_collapse_buttons(false)
                .tab_context_menus(false)
                .show_inside(ui, viewer);
        });
}
