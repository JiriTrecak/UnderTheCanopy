//! The editor's frame (docs/editor.md, M1): the rail that switches editors, the header,
//! each editor's library, document tabs and inspector, and the assistant sidebar on the
//! right. Every edit goes through the [`Store`]: one undo history across editors, Cmd+S
//! saves every changed document, leaving with unsaved changes asks first.

use crate::checker::Checker;
use crate::document::{Command, Context, Outside, Store};
use crate::forms::Form;
use crate::icons::{self, Icon};
use crate::inspector::{self, Inspect, Request};
use crate::library::{Element, KINDS, Kind, Library, Node};
use crate::references::{self, Use};
use crate::tabs::{self, Doc, Edit, Viewer};
use crate::theme::*;
use crate::watch::Watch;
use crate::widgets::*;
use canopy_content::package::{Form as Storage, Package};
use egui::{
    Align, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Ui, pos2, vec2,
};
use egui_dock::DockState;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod serve;

/// One editor's state, kept while switching editors.
struct Workspace {
    library: Library,
    dock: DockState<Doc>,
    filter: String,
    /// Expanded library folders, by path.
    expanded: HashSet<String>,
    /// The field the last undo or redo changed (document, pointer), and when.
    highlight: Option<(String, String, Instant)>,
}

impl Workspace {
    fn read(package: &Package, kind: Kind) -> Self {
        let library = Library::read(package, kind);
        // Several collections show one folder each, open.
        let expanded = if kind.sections().len() > 1 {
            library
                .nodes
                .iter()
                .filter_map(|n| match n {
                    Node::Folder { path, .. } => Some(path.clone()),
                    Node::Element(_) => None,
                })
                .collect()
        } else {
            HashSet::new()
        };
        Workspace {
            library,
            dock: DockState::new(Vec::new()),
            filter: String::new(),
            expanded,
            highlight: None,
        }
    }

    /// Opens (or focuses) `element`'s tab; shows its folder in the library.
    fn open(&mut self, element: &Element) {
        if let Some(path) = self.dock.find_tab_from(|d| d.document == element.document) {
            if let Ok(leaf) = self.dock.leaf_mut(path.node_path())
                && let Some(tab) = leaf.tabs.get_mut(path.tab.0)
            {
                tab.show_form();
            }
            let _ = self.dock.set_active_tab(path);
            self.dock.set_focused_node_and_surface(path.node_path());
        } else {
            self.dock.push_to_focused_leaf(Doc::new(element));
        }
        let parts: Vec<&str> = element.path.split('/').collect();
        for end in 1..parts.len() {
            self.expanded.insert(parts[..end].join("/"));
        }
    }

    /// The element whose document is `document`, from the library (or a stand-in for one
    /// it does not list, such as a new document).
    fn element(&self, document: &str) -> Element {
        let mut all = Vec::new();
        self.library.nodes.iter().for_each(|n| n.elements(&mut all));
        all.into_iter()
            .find(|e| e.document == document)
            .cloned()
            .unwrap_or_else(|| {
                let stem = document
                    .rsplit('/')
                    .next()
                    .unwrap_or(document)
                    .trim_end_matches(".json");
                Element {
                    id: stem.to_string(),
                    name: stem.to_string(),
                    path: document.to_string(),
                    document: document.to_string(),
                }
            })
    }
}

/// A dialog about one element.
enum Dialog {
    Rename {
        document: String,
        old: String,
        new: String,
    },
    Delete {
        document: String,
        id: String,
    },
    Create {
        kind: Kind,
        /// Index into the editor's sections.
        section: usize,
        folder: String,
        id: String,
    },
}

/// Where leaving the editor goes once unsaved changes are settled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Leave {
    Menu,
    Quit,
}

pub struct Shell {
    package: Result<Package, String>,
    store: Option<Store>,
    current: Kind,
    workspaces: HashMap<Kind, Workspace>,
    assistant: bool,
    /// Asking about unsaved changes before leaving.
    leaving: Option<Leave>,
    /// A short message in the header (saved, an error) and when it was set.
    notice: Option<(String, bool, Instant)>,
    /// The loader's check of the package with the unsaved edits, and the generation of
    /// the store it was last asked for.
    checker: Option<Checker>,
    checked: Option<u64>,
    /// The problems list is open.
    problems_open: bool,
    /// Asking whether to save although the check found problems.
    confirm_save: bool,
    /// Files changed on disk.
    watch: Option<Watch>,
    /// Every element id (pickers), and the store's structure count the libraries match.
    ids: Vec<String>,
    structure: Option<u64>,
    /// Uses of an id: (document, store generation, uses).
    uses: Option<(String, u64, Vec<Use>)>,
    /// An open rename, delete or create dialog.
    dialog: Option<Dialog>,
    /// The user folder (the MCP token, the editor's settings).
    user: Option<std::path::PathBuf>,
    /// MCP: tool calls from the server's thread, the server, its token, setting and panel.
    calls: (
        std::sync::mpsc::Sender<crate::mcp::Call>,
        std::sync::mpsc::Receiver<crate::mcp::Call>,
    ),
    server: Option<crate::mcp::Server>,
    token: String,
    autostart: bool,
    mcp_error: Option<String>,
    mcp_open: bool,
    /// The viewport: last frame's picture, why it cannot draw, this frame's request and
    /// its camera.
    pub viewport_texture: Option<egui::TextureId>,
    pub viewport_error: Option<String>,
    pub shot: Option<crate::stage::Shot>,
    orbit: crate::stage::Orbit,
    /// Test in game, asked for this frame.
    pub test: Option<(Arc<canopy_content::Content>, Option<String>)>,
    /// `problems` calls waiting for the current check.
    waiting: Vec<tokio::sync::oneshot::Sender<Result<Value, String>>>,
    /// The editor is done: back to the menu, or quit the game.
    pub close: Option<Leave>,
}

impl Shell {
    pub fn new(package: Result<Package, String>, user: Option<std::path::PathBuf>) -> Self {
        let store = package.as_ref().ok().map(|p| Store::new(&p.root));
        let checker = package.as_ref().ok().cloned().map(Checker::new);
        let watch = package.as_ref().ok().and_then(|p| Watch::new(&p.root));
        let token = serve::token(user.as_deref());
        let autostart = serve::autostart(user.as_deref());
        Self {
            calls: std::sync::mpsc::channel(),
            server: None,
            token,
            autostart,
            mcp_error: None,
            mcp_open: false,
            waiting: Vec::new(),
            test: None,
            viewport_texture: None,
            viewport_error: None,
            shot: None,
            orbit: crate::stage::Orbit::default(),
            user,
            watch,
            ids: Vec::new(),
            structure: None,
            uses: None,
            dialog: None,
            checker,
            checked: None,
            problems_open: false,
            confirm_save: false,
            package,
            store,
            current: Kind::Abilities,
            workspaces: HashMap::new(),
            assistant: false,
            leaving: None,
            notice: None,
            close: None,
        }
    }

    /// Leaves now, or asks first when there are unsaved changes.
    pub fn leave(&mut self, to: Leave) {
        if self.store.as_ref().is_some_and(|s| !s.dirty().is_empty()) {
            self.leaving = Some(to);
        } else {
            self.close = Some(to);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        if let Some(checker) = &mut self.checker {
            checker.poll();
        }
        self.follow_disk();
        self.shortcuts(ui.ctx());
        self.rail(ui);
        if self.assistant {
            self.assistant(ui);
        }
        self.header(ui);
        self.problems(ui);
        let Ok(package) = &self.package else {
            egui::CentralPanel::no_frame()
                .frame(Frame::new().fill(MAIN))
                .show(ui, |ui| {
                    let error = self.package.as_ref().err().cloned().unwrap_or_default();
                    empty(ui, "The base package could not be opened", &error);
                });
            return;
        };
        let package = package.clone();
        self.refresh_libraries(&package);
        let kind = self.current;
        let active = self.active_document();
        let uses = active.as_deref().map(|d| self.uses_of(d));
        let (can_rename, can_delete) = match active.as_deref().and_then(Kind::collection_of) {
            Some((_, collection)) => (
                matches!(collection.form, Storage::Files | Storage::Folders(_)),
                collection.form == Storage::Files,
            ),
            None => (false, false),
        };
        let subject = active.as_deref().and_then(|d| self.subject(d));
        let workspace = self
            .workspaces
            .get_mut(&kind)
            .expect("every editor's library is read");
        let store = self.store.as_mut().expect("a store with the package");
        let mut edits = Vec::new();
        let create = library_panel(ui, kind, workspace, store, &package);
        let request = inspector::show(
            ui,
            &mut workspace.dock,
            Inspect {
                kind,
                store,
                edits: &mut edits,
                uses: uses.as_deref(),
                can_rename,
                can_delete,
                preview: subject.map(|(model, scale)| inspector::Preview {
                    model,
                    scale,
                    texture: self.viewport_texture,
                    error: self.viewport_error.as_deref(),
                    orbit: &mut self.orbit,
                    shot: &mut self.shot,
                }),
            },
        );
        if workspace
            .highlight
            .as_ref()
            .is_some_and(|h| h.2.elapsed() > Duration::from_secs(2))
        {
            workspace.highlight = None;
        }
        let highlight = workspace.highlight.clone();
        tabs::documents(
            ui,
            kind,
            &mut workspace.dock,
            &mut Viewer {
                kind,
                store,
                edits: &mut edits,
                ids: &self.ids,
                highlight: highlight.as_ref().map(|h| (h.0.as_str(), h.1.as_str())),
            },
        );
        for edit in edits {
            self.run(edit);
        }
        if create {
            self.start_create(kind);
        }
        match request {
            Some(Request::Rename(document)) => {
                let old = self.id_of(&document);
                self.dialog = Some(Dialog::Rename {
                    new: old.clone(),
                    old,
                    document,
                });
            }
            Some(Request::Delete(document)) => {
                let id = self.id_of(&document);
                self.dialog = Some(Dialog::Delete { document, id });
            }
            Some(Request::Reveal(document)) => self.reveal_document(&document),
            None => {}
        }
        self.check();
        self.conflict_dialog(ui.ctx());
        self.leave_dialog(ui.ctx());
        self.save_dialog(ui.ctx());
        self.element_dialog(ui.ctx());
        self.mcp_dialog(ui.ctx());
        self.serve_calls();
    }

    /// Starts the MCP server when the setting asks for it.
    pub fn autostart_mcp(&mut self) {
        if self.autostart {
            self.start_mcp();
        }
    }

    /// The model the open document shows in the viewport, and its scale: a definition's
    /// look or an asset's model, from the last load that passed the check.
    fn subject(&self, document: &str) -> Option<(String, f32)> {
        let content = self.checker.as_ref()?.content()?;
        let (_, collection) = Kind::collection_of(document)?;
        let id = Kind::of_document(document)
            .and_then(|k| self.workspaces.get(&k))
            .map(|w| w.element(document).id)?;
        let look = match collection.dir {
            "units" | "buildings" | "items" | "resources" => content.look(&id),
            "assets" | "scenery" => content.asset(&id),
            _ => None,
        }?;
        Some((look.file.clone()?, look.scale.unwrap_or(1.0) as f32))
    }

    /// The package with the unsaved documents laid over it.
    fn overlaid(&self, package: &Package) -> Package {
        match &self.store {
            Some(store) => package.clone().with_overlay(Arc::new(store.overlay())),
            None => package.clone(),
        }
    }

    /// Reads every editor's library (with unsaved new and deleted documents) when the
    /// documents that exist changed, and gathers every element id.
    fn refresh_libraries(&mut self, package: &Package) {
        let structure = self.store.as_ref().map(Store::structure);
        if self.structure == structure && self.workspaces.len() == KINDS.len() {
            return;
        }
        self.structure = structure;
        let overlaid = self.overlaid(package);
        for kind in KINDS {
            match self.workspaces.get_mut(&kind) {
                Some(workspace) => workspace.library = Library::read(&overlaid, kind),
                None => {
                    self.workspaces
                        .insert(kind, Workspace::read(&overlaid, kind));
                }
            }
        }
        let mut ids: Vec<String> = Vec::new();
        for workspace in self.workspaces.values() {
            let mut elements = Vec::new();
            workspace
                .library
                .nodes
                .iter()
                .for_each(|n| n.elements(&mut elements));
            ids.extend(elements.into_iter().map(|e| e.id.clone()));
        }
        ids.sort();
        ids.dedup();
        self.ids = ids;
    }

    /// Every document the libraries list, and each map's scenery.
    fn all_documents(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (kind, workspace) in &self.workspaces {
            let mut elements = Vec::new();
            workspace
                .library
                .nodes
                .iter()
                .for_each(|n| n.elements(&mut elements));
            for element in elements {
                out.push(element.document.clone());
                if *kind == Kind::Maps {
                    out.push(format!("{}/scenery.json", element.path));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The id of the element `document` holds (its `id`, else its library entry's).
    fn id_of(&mut self, document: &str) -> String {
        let from_value = self
            .store
            .as_mut()
            .and_then(|s| s.get(document).ok().flatten())
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        from_value.unwrap_or_else(|| {
            Kind::of_document(document)
                .and_then(|k| self.workspaces.get(&k))
                .map(|w| w.element(document).id)
                .unwrap_or_default()
        })
    }

    /// Every use of the id held by `document`, across the package (cached per change).
    fn uses_of(&mut self, document: &str) -> Vec<Use> {
        let generation = self.store.as_ref().map_or(0, Store::generation);
        if let Some((cached, at, uses)) = &self.uses
            && cached == document
            && *at == generation
        {
            return uses.clone();
        }
        let id = self.id_of(document);
        let uses = if id.is_empty() {
            Vec::new()
        } else {
            self.find_uses(&id)
        };
        self.uses = Some((document.to_string(), generation, uses.clone()));
        uses
    }

    fn reveal_document(&mut self, document: &str) {
        if let Some(editor) = Kind::of_document(document) {
            self.reveal(&Context {
                editor,
                document: document.to_string(),
                pointer: None,
            });
        }
    }

    /// Opens the dialog that creates an element in `kind`'s first collection of files,
    /// next to the open one.
    fn start_create(&mut self, kind: Kind) {
        let Some(section) = kind
            .sections()
            .iter()
            .position(|(_, c)| c.form == Storage::Files)
        else {
            return;
        };
        let dir = kind.sections()[section].1.dir;
        let folder = self
            .active_document()
            .and_then(|d| {
                d.strip_prefix(&format!("{dir}/"))
                    .and_then(|rest| rest.rsplit_once('/'))
                    .map(|(folder, _)| folder.to_string())
            })
            .unwrap_or_default();
        self.dialog = Some(Dialog::Create {
            kind,
            section,
            folder,
            id: String::new(),
        });
    }

    /// The rename, delete and create dialogs.
    fn element_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        // What the dialog decided: keep it open, close it, or run a step.
        enum Outcome {
            Open,
            Close,
            Run(Edit),
            Reveal(String),
        }
        let mut outcome = Outcome::Open;
        let uses = match &dialog {
            Dialog::Rename { document, .. } | Dialog::Delete { document, .. } => {
                self.uses_of(&document.clone())
            }
            Dialog::Create { .. } => Vec::new(),
        };
        let ids = self.ids.clone();
        let exists = |store: &mut Option<Store>, path: &str| {
            store
                .as_mut()
                .is_some_and(|s| matches!(s.get(path), Ok(Some(_)) | Err(_)))
        };
        let modal = egui::Modal::new(Id::new("element-dialog"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10)
                    .inner_margin(Margin::same(20)),
            );
        let store = &mut self.store;
        let response = modal.show(ctx, |ui| {
            ui.set_width(420.0);
            match &mut dialog {
                Dialog::Rename { document, old, new } => {
                    ui.label(RichText::new("Rename id").size(14.0).color(TEXT));
                    ui.label(RichText::new(old.as_str()).monospace().color(SOFT));
                    ui.add_space(6.0);
                    ui.add(TextEdit::singleline(new).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                    let valid = canopy_content::Id::new(new.as_str());
                    let problem = if new == old {
                        Some("Type the new id.".to_string())
                    } else if let Err(e) = valid {
                        Some(e)
                    } else if ids.binary_search(new).is_ok() {
                        Some("Another element has this id.".into())
                    } else {
                        None
                    };
                    let places = uses.len();
                    let files = {
                        let mut d: Vec<&str> = uses.iter().map(|u| u.document.as_str()).collect();
                        d.dedup();
                        d.len()
                    };
                    ui.label(
                        RichText::new(match &problem {
                            Some(p) => p.clone(),
                            None => format!("Changes {places} uses in {files} documents, in one undo step."),
                        })
                        .color(if problem.is_some() { FAINT } else { MUTED })
                        .size(11.0),
                    );
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(problem.is_none(), primary("Rename")).clicked() {
                            let values = |d: &str| {
                                store.as_mut().and_then(|s| s.get(d).ok().flatten().cloned())
                            };
                            let commands = references::rename(&uses, new, values);
                            outcome = Outcome::Run(Edit {
                                label: format!("Rename {old} to {new}"),
                                context: Context {
                                    editor: Kind::of_document(document).unwrap_or(Kind::Units),
                                    document: document.clone(),
                                    pointer: Some("/id".into()),
                                },
                                commands,
                                merge: None,
                            });
                        }
                        if ui.button("Cancel").clicked() {
                            outcome = Outcome::Close;
                        }
                    });
                }
                Dialog::Delete { document, id } => {
                    let elsewhere: Vec<&Use> = uses.iter().filter(|u| u.document != *document).collect();
                    if elsewhere.is_empty() {
                        ui.label(RichText::new(format!("Delete {id}?")).size(14.0).color(TEXT));
                        ui.label(
                            RichText::new("Nothing uses it. Its file goes on the next save; undo brings it back.")
                                .color(MUTED),
                        );
                        ui.add_space(12.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let delete = egui::Button::new(RichText::new("Delete").color(TEXT))
                                .fill(egui::Color32::from_rgb(0x99, 0x1b, 0x1b))
                                .stroke(Stroke::NONE);
                            if ui.add(delete).clicked() {
                                outcome = Outcome::Run(Edit {
                                    label: format!("Delete {id}"),
                                    context: Context {
                                        editor: Kind::of_document(document).unwrap_or(Kind::Units),
                                        document: document.clone(),
                                        pointer: None,
                                    },
                                    commands: vec![Command::Delete {
                                        document: document.clone(),
                                    }],
                                    merge: None,
                                });
                            }
                            if ui.button("Cancel").clicked() {
                                outcome = Outcome::Close;
                            }
                        });
                    } else {
                        ui.label(RichText::new(format!("{id} is still used")).size(14.0).color(TEXT));
                        ui.label(
                            RichText::new("Remove these uses first; then it can be deleted.").color(MUTED),
                        );
                        ui.add_space(6.0);
                        for u in elsewhere.iter().take(12) {
                            let text = format!("{} · {}", u.document, u.pointer);
                            let label = egui::Label::new(RichText::new(text).monospace().color(SOFT).size(11.0))
                                .truncate()
                                .sense(Sense::click());
                            if ui.add(label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                outcome = Outcome::Reveal(u.document.clone());
                            }
                        }
                        if elsewhere.len() > 12 {
                            ui.label(RichText::new(format!("and {} more", elsewhere.len() - 12)).color(FAINT));
                        }
                        ui.add_space(12.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                outcome = Outcome::Close;
                            }
                        });
                    }
                }
                Dialog::Create { kind, section, folder, id } => {
                    ui.label(RichText::new(format!("New in {}", kind.name())).size(14.0).color(TEXT));
                    ui.add_space(6.0);
                    let files: Vec<usize> = kind
                        .sections()
                        .iter()
                        .enumerate()
                        .filter(|(_, (_, c))| c.form == Storage::Files)
                        .map(|(i, _)| i)
                        .collect();
                    if files.len() > 1 {
                        egui::ComboBox::from_label("Collection")
                            .selected_text(kind.sections()[*section].0)
                            .show_ui(ui, |ui| {
                                for i in &files {
                                    ui.selectable_value(section, *i, kind.sections()[*i].0);
                                }
                            });
                    }
                    ui.label(RichText::new("Id").color(MUTED).size(11.0));
                    ui.add(TextEdit::singleline(id).font(egui::TextStyle::Monospace).hint_text("unit.ants.scout").desired_width(f32::INFINITY));
                    ui.label(RichText::new("Folder").color(MUTED).size(11.0));
                    ui.add(TextEdit::singleline(folder).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                    let dir = kind.sections()[*section].1.dir;
                    let tail = id.rsplit('.').next().unwrap_or_default();
                    let folder_part = folder.trim_matches('/');
                    let path = if folder_part.is_empty() {
                        format!("{dir}/{tail}.json")
                    } else {
                        format!("{dir}/{folder_part}/{tail}.json")
                    };
                    let problem = if id.is_empty() {
                        Some("Type its id.".to_string())
                    } else if let Err(e) = canopy_content::Id::new(id.as_str()) {
                        Some(e)
                    } else if ids.binary_search(id).is_ok() {
                        Some("Another element has this id.".into())
                    } else if exists(store, &path) {
                        Some(format!("{path} already exists."))
                    } else {
                        None
                    };
                    ui.label(
                        RichText::new(problem.clone().unwrap_or_else(|| format!("Creates {path}")))
                            .color(FAINT)
                            .size(11.0),
                    );
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(problem.is_none(), primary("Create")).clicked() {
                            let schema = canopy_content::schema::of_document(&path);
                            let mut value = match &schema {
                                Some(schema) => Form::new(Some(schema), &path, &[]).default_of(schema),
                                None => Value::Object(Default::default()),
                            };
                            if let Value::Object(object) = &mut value {
                                object.shift_insert(0, "id".into(), Value::String(id.clone()));
                                let named = schema
                                    .as_ref()
                                    .and_then(|s| s.pointer("/properties/name"))
                                    .is_some();
                                if named && !object.contains_key("name") {
                                    object.shift_insert(1, "name".into(), Value::String(tail.to_string()));
                                }
                            }
                            outcome = Outcome::Run(Edit {
                                label: format!("Create {id}"),
                                context: Context {
                                    editor: *kind,
                                    document: path.clone(),
                                    pointer: None,
                                },
                                commands: vec![Command::Create {
                                    document: path,
                                    value,
                                }],
                                merge: None,
                            });
                        }
                        if ui.button("Cancel").clicked() {
                            outcome = Outcome::Close;
                        }
                    });
                }
            }
        });
        if response.should_close() && matches!(outcome, Outcome::Open) {
            outcome = Outcome::Close;
        }
        match outcome {
            Outcome::Open => self.dialog = Some(dialog),
            Outcome::Close => {}
            Outcome::Reveal(document) => self.reveal_document(&document),
            Outcome::Run(edit) => {
                let context = edit.context.clone();
                self.run(edit);
                if let Ok(package) = self.package.clone() {
                    self.refresh_libraries(&package);
                }
                self.reveal(&context);
            }
        }
    }

    /// Takes in files changed on disk: documents reload or become conflicts, and the
    /// libraries that list them are read again.
    fn follow_disk(&mut self) {
        let Some(changed) = self.watch.as_ref().map(Watch::changed) else {
            return;
        };
        if changed.is_empty() {
            return;
        }
        let Ok(package) = &self.package else {
            return;
        };
        // What is on disk changed, so the last check is out of date.
        self.checked = None;
        let mut unreadable = None;
        if let Some(store) = &mut self.store {
            for document in &changed {
                if let Outside::Unreadable(message) = store.outside_change(document) {
                    unreadable = Some(message);
                }
            }
        }
        // Files may have come or gone: read the libraries again.
        self.structure = None;
        let _ = package;
        if let Some(message) = unreadable {
            self.say(format!("Not valid JSON on disk: {message}"), true);
        }
    }

    /// Asks which version wins for a document changed on disk while it had unsaved edits.
    fn conflict_dialog(&mut self, ctx: &egui::Context) {
        let Some(document) = self
            .store
            .as_ref()
            .and_then(|s| s.conflicts().first().map(|d| d.to_string()))
        else {
            return;
        };
        let mut answer = None;
        egui::Modal::new(Id::new("conflict"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10)
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(400.0);
                ui.label(RichText::new("Changed on disk").size(14.0).color(TEXT));
                ui.add_space(4.0);
                ui.label(RichText::new(&document).monospace().color(SOFT));
                ui.label(
                    RichText::new(
                        "The file changed while it had unsaved edits here. Keeping your edits overwrites the file on the next save; taking the file's version drops your edits to it.",
                    )
                    .color(MUTED),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(primary("Keep my edits")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Take the file's version").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if let (Some(keep_mine), Some(store)) = (answer, &mut self.store) {
            store.resolve(&document, keep_mine);
        }
    }

    /// Asks for a check when the documents changed since the last one.
    fn check(&mut self) {
        let (Some(store), Some(checker)) = (&self.store, &mut self.checker) else {
            return;
        };
        let generation = store.generation();
        if self.checked != Some(generation) {
            self.checked = Some(generation);
            checker.request(generation, store.overlay());
        }
    }

    /// The problems the last check found, if one finished.
    fn problem_count(&self) -> Option<usize> {
        Some(self.checker.as_ref()?.report()?.problems.len())
    }

    /// The status bar with the check's outcome, and the problems list when open.
    fn problems(&mut self, ui: &mut Ui) {
        let Some(checker) = &self.checker else {
            return;
        };
        let report = checker.report().cloned();
        let busy = checker.busy()
            || report
                .as_ref()
                .is_some_and(|r| Some(r.generation) != self.checked);
        let count = report.as_ref().map(|r| r.problems.len());
        let mut toggle = false;
        egui::Panel::bottom("status")
            .exact_size(28.0)
            .resizable(false)
            .show_separator_line(true)
            .frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(12, 0)))
            .show(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let dot = |ui: &mut Ui, color| {
                        let (rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 3.0, color);
                    };
                    match count {
                        None => {
                            ui.label(
                                RichText::new("Checking the package…")
                                    .color(FAINT)
                                    .size(11.0),
                            );
                        }
                        Some(0) => {
                            dot(ui, LIVE);
                            ui.label(RichText::new("No problems").color(MUTED).size(11.0));
                        }
                        Some(n) => {
                            dot(ui, ERROR);
                            let text = if n == 1 {
                                "1 problem".into()
                            } else {
                                format!("{n} problems")
                            };
                            let label =
                                egui::Label::new(RichText::new(text).color(SOFT).size(11.0))
                                    .sense(Sense::click());
                            toggle = ui
                                .add(label)
                                .on_hover_text("Show or hide the problems")
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked();
                        }
                    }
                    if busy && count.is_some() {
                        ui.label(RichText::new("· checking…").color(FAINT).size(11.0));
                    }
                    if let Some(report) = &report {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("Checked in {:.0} ms", report.ms))
                                    .color(FAINT)
                                    .size(11.0),
                            );
                        });
                    }
                });
            });
        if toggle {
            self.problems_open = !self.problems_open;
        }
        let Some(report) = report.filter(|r| self.problems_open && !r.problems.is_empty()) else {
            return;
        };
        let mut reveal = None;
        egui::Panel::bottom("problems")
            .resizable(true)
            .default_size(180.0)
            .size_range(80.0..=480.0)
            .show_separator_line(true)
            .frame(
                Frame::new()
                    .fill(SUNK)
                    .inner_margin(Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for problem in &report.problems {
                            let mut job = egui::text::LayoutJob::default();
                            let mono = egui::FontId::monospace(11.0);
                            let place = if problem.location.is_empty() {
                                problem.file.clone()
                            } else {
                                format!("{} · {}", problem.file, problem.location)
                            };
                            job.append(&place, 0.0, egui::TextFormat::simple(mono, FAINT));
                            job.append(
                                &format!("  {}", problem.message),
                                0.0,
                                egui::TextFormat::simple(egui::FontId::proportional(12.0), SOFT),
                            );
                            let label = egui::Label::new(job).truncate();
                            let response = match &problem.document {
                                Some(_) => ui
                                    .add(label.sense(Sense::click()))
                                    .on_hover_cursor(egui::CursorIcon::PointingHand),
                                None => ui.add(label),
                            };
                            if response.clicked() {
                                reveal = problem.document.clone();
                            }
                        }
                    });
            });
        if let Some(document) = reveal
            && let Some(editor) = Kind::of_document(&document)
        {
            self.reveal(&Context {
                editor,
                document,
                pointer: None,
            });
        }
    }

    /// Saves, or asks first when the last check found problems.
    fn request_save(&mut self) {
        if self.store.as_ref().is_none_or(|s| s.dirty().is_empty()) {
            return;
        }
        if self.problem_count().unwrap_or(0) > 0 {
            self.confirm_save = true;
        } else {
            self.save();
        }
    }

    fn save_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_save {
            return;
        }
        let count = self.problem_count().unwrap_or(0);
        let mut answer = None;
        let response = egui::Modal::new(Id::new("save-with-problems"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10)
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("The package has problems").size(14.0).color(TEXT));
                ui.add_space(4.0);
                let what = if count == 1 { "1 problem".to_string() } else { format!("{count} problems") };
                ui.label(
                    RichText::new(format!(
                        "The check found {what}. The game will not load the package until they are fixed. Save anyway?"
                    ))
                    .color(MUTED),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(primary("Save anyway")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if response.should_close() && answer.is_none() {
            answer = Some(false);
        }
        if let Some(save) = answer {
            self.confirm_save = false;
            if save {
                self.save();
            }
        }
    }

    /// The document of the current editor's focused tab.
    pub fn active_document(&mut self) -> Option<String> {
        self.workspaces
            .get_mut(&self.current)?
            .dock
            .main_surface_mut()
            .find_active_focused()
            .map(|(_, d)| d.document.clone())
    }

    /// Runs `commands` as one step whose context is the current editor and the first
    /// command's document.
    pub fn run_step(&mut self, label: &str, commands: Vec<Command>) -> Result<(), String> {
        let store = self.store.as_mut().ok_or("no package")?;
        let document = commands
            .first()
            .map(|c| c.document().to_string())
            .unwrap_or_default();
        let context = Context {
            editor: self.current,
            document,
            pointer: None,
        };
        store
            .run(label, context, commands)
            .map_err(|e| e.to_string())
    }

    fn run(&mut self, edit: Edit) {
        let Some(store) = &mut self.store else {
            return;
        };
        if let Err(error) = store.run_merging(
            &edit.label,
            edit.context,
            edit.commands,
            edit.merge.as_deref(),
        ) {
            self.say(error.to_string(), true);
        }
    }

    fn say(&mut self, text: String, error: bool) {
        if error {
            tracing::warn!(%text, "editor");
        }
        self.notice = Some((text, error, Instant::now()));
    }

    fn undo(&mut self) {
        if let Some(context) = self.store.as_mut().and_then(Store::undo) {
            self.reveal(&context);
        }
    }

    fn redo(&mut self) {
        if let Some(context) = self.store.as_mut().and_then(Store::redo) {
            self.reveal(&context);
        }
    }

    /// Brings back where a step happened: its editor and its document's tab.
    fn reveal(&mut self, context: &Context) {
        let Ok(package) = &self.package else {
            return;
        };
        self.current = context.editor;
        let workspace = self
            .workspaces
            .entry(context.editor)
            .or_insert_with(|| Workspace::read(package, context.editor));
        let element = workspace.element(&context.document);
        workspace.open(&element);
        workspace.highlight = context
            .pointer
            .clone()
            .map(|p| (context.document.clone(), p, Instant::now()));
    }

    fn save(&mut self) {
        let Some(store) = &mut self.store else {
            return;
        };
        match store.save() {
            Ok(written) if written.is_empty() => {}
            Ok(written) => {
                let text = match written.as_slice() {
                    [one] => format!("Saved {one}"),
                    many => format!("Saved {} files", many.len()),
                };
                self.say(text, false);
            }
            Err(error) => self.say(format!("Not saved: {error}"), true),
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let redo = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        if ctx.input_mut(|i| i.consume_shortcut(&save)) {
            self.request_save();
        }
        // A focused text field keeps Cmd+Z for its own text.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&redo)) {
            self.redo();
        } else if ctx.input_mut(|i| i.consume_shortcut(&undo)) {
            self.undo();
        }
    }

    /// Whether the check of the latest edits has not finished yet.
    pub fn checking(&self) -> bool {
        self.checker.as_ref().is_some_and(|c| {
            c.busy()
                || c.report()
                    .is_none_or(|r| Some(r.generation) != self.checked)
        })
    }

    pub fn undo_all(&mut self) {
        while let Some(store) = &mut self.store
            && store.undo().is_some()
        {}
    }

    pub fn checked_content(&self) -> Option<Arc<canopy_content::Content>> {
        self.checker.as_ref().and_then(|c| c.content().cloned())
    }

    pub fn show_problems(&mut self, open: bool) {
        self.problems_open = open;
    }

    /// Switches to `kind`'s editor.
    pub fn select(&mut self, kind: Kind) {
        self.current = kind;
    }

    pub fn set_assistant(&mut self, open: bool) {
        self.assistant = open;
    }

    /// Opens the current editor's first `n` elements in library order (captures).
    pub fn open_first(&mut self, n: usize) {
        let Ok(package) = &self.package else {
            return;
        };
        let kind = self.current;
        let workspace = self
            .workspaces
            .entry(kind)
            .or_insert_with(|| Workspace::read(package, kind));
        let mut elements = Vec::new();
        workspace
            .library
            .nodes
            .iter()
            .for_each(|n| n.elements(&mut elements));
        let first: Vec<Element> = elements.into_iter().take(n).cloned().collect();
        for element in &first {
            workspace.open(element);
        }
    }

    fn rail(&mut self, ui: &mut Ui) {
        egui::Panel::left("rail")
            .exact_size(RAIL)
            .resizable(false)
            .show_separator_line(true)
            .frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(6, 10)))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                    ui.painter().rect_filled(rect, 9, TEXT);
                    icons::paint(
                        ui.painter(),
                        icons::square(rect.center(), 20.0),
                        Icon::Logo,
                        RAISED,
                    );
                    ui.add_space(10.0);
                    for kind in KINDS {
                        let icon = match kind {
                            Kind::Maps => Icon::Map,
                            Kind::Units => Icon::Units,
                            Kind::Abilities => Icon::Abilities,
                            Kind::Effects => Icon::Effects,
                            Kind::Buildings => Icon::Buildings,
                            Kind::Races => Icon::Races,
                            Kind::Assets => Icon::Assets,
                        };
                        if rail_item(ui, icon, kind.label(), self.current == kind)
                            .on_hover_text(kind.name())
                            .clicked()
                        {
                            self.current = kind;
                        }
                    }
                    ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                        if rail_item(ui, Icon::Exit, "Exit", false)
                            .on_hover_text("Back to the main menu")
                            .clicked()
                        {
                            self.leave(Leave::Menu);
                        }
                    });
                });
            });
    }

    fn header(&mut self, ui: &mut Ui) {
        let title = self
            .workspaces
            .get_mut(&self.current)
            .and_then(|w| w.dock.main_surface_mut().find_active_focused())
            .map(|(_, doc)| (doc.name.clone(), doc.document.clone()))
            .map(|(name, document)| {
                let current = self
                    .store
                    .as_mut()
                    .and_then(|s| s.get(&document).ok().flatten())
                    .and_then(|v| v.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                (current.unwrap_or(name), document)
            });
        let (unsaved, undo, redo) = match &self.store {
            Some(s) => (
                s.dirty().len(),
                s.undo_label().map(str::to_string),
                s.redo_label().map(str::to_string),
            ),
            None => (0, None, None),
        };
        if self
            .notice
            .as_ref()
            .is_some_and(|n| n.2.elapsed() > Duration::from_secs(4))
        {
            self.notice = None;
        }
        let notice = self.notice.clone();
        let mut clicked = (false, false, false, false);
        // The data of a check that passed for the latest edits.
        let testable = (self.problem_count() == Some(0) && !self.checking())
            .then(|| self.checker.as_ref().and_then(|c| c.content().cloned()))
            .flatten();
        egui::Panel::top("header")
            .exact_size(HEADER)
            .resizable(false)
            .show_separator_line(true)
            .frame(
                Frame::new()
                    .fill(RAISED)
                    .inner_margin(Margin::symmetric(14, 0)),
            )
            .show(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add_space(13.0);
                        ui.label(
                            RichText::new(self.current.name())
                                .size(13.0)
                                .color(TEXT)
                                .strong(),
                        );
                        ui.label(eyebrow("Under the Canopy editor").size(8.0));
                    });
                    ui.add_space(14.0);
                    let package = match &self.package {
                        Ok(p) => p.manifest.id.to_string(),
                        Err(_) => "no package".into(),
                    };
                    chip(ui, &package, "Repository")
                        .on_hover_text("The repository's base package: saving writes its files.");
                    if let Some((name, document)) = &title {
                        ui.add_space(4.0);
                        ui.label(RichText::new(name).color(TEXT))
                            .on_hover_text(document);
                    }
                    if unsaved > 0 {
                        let text = match unsaved {
                            1 => "1 unsaved".to_string(),
                            n => format!("{n} unsaved"),
                        };
                        badge(ui, &text, Some(DIRTY));
                    }
                    if let Some((text, error, _)) = &notice {
                        let color = if *error { ERROR } else { MUTED };
                        ui.label(RichText::new(text).color(color).size(11.0));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if icon_button(ui, Icon::Panel, self.assistant, true)
                            .on_hover_text("Assistant")
                            .clicked()
                        {
                            self.assistant = !self.assistant;
                        }
                        clicked.0 = ui
                            .add_enabled(unsaved > 0, primary("Save"))
                            .on_hover_text("Save every changed document (Cmd+S)")
                            .clicked();
                        let (text, dot) = match self.server.as_ref().map(|s| s.port) {
                            Some(port) => (format!("MCP :{port}"), LIVE),
                            None => ("MCP off".to_string(), FAINT),
                        };
                        if status(ui, &text, dot)
                            .on_hover_text("The MCP server for agents")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.mcp_open = true;
                        }
                        let ready = testable.is_some();
                        let hint = if ready {
                            "Play a skirmish against the computer with these edits (unsaved ones included); leave the match to come back"
                        } else {
                            "Fix the problems first: the game loads only a package that passes the check"
                        };
                        clicked.3 = ui
                            .add_enabled(ready, egui::Button::new("Test in game"))
                            .on_hover_text(hint)
                            .on_disabled_hover_text(hint)
                            .clicked();
                        let (line, _) = ui.allocate_exact_size(vec2(1.0, 20.0), Sense::hover());
                        ui.painter().rect_filled(line, 0, BORDER);
                        let hint = |verb: &str, label: &Option<String>, keys: &str| match label {
                            Some(l) => format!("{verb} {l} ({keys})"),
                            None => format!("Nothing to {}", verb.to_lowercase()),
                        };
                        clicked.2 = icon_button(ui, Icon::Redo, false, redo.is_some())
                            .on_hover_text(hint("Redo", &redo, "Cmd+Shift+Z"))
                            .clicked();
                        clicked.1 = icon_button(ui, Icon::Undo, false, undo.is_some())
                            .on_hover_text(hint("Undo", &undo, "Cmd+Z"))
                            .clicked();
                    });
                });
            });
        if clicked.0 {
            self.request_save();
        }
        if clicked.1 {
            self.undo();
        }
        if clicked.2 {
            self.redo();
        }
        if clicked.3
            && let Some(content) = testable
        {
            // The open map, when the maps editor shows one.
            let map = (self.current == Kind::Maps)
                .then(|| self.active_document())
                .flatten()
                .and_then(|d| self.workspaces.get(&Kind::Maps).map(|w| w.element(&d).id));
            self.test = Some((content, map));
        }
    }

    fn leave_dialog(&mut self, ctx: &egui::Context) {
        let Some(to) = self.leaving else {
            return;
        };
        let unsaved = self.store.as_ref().map_or(0, |s| s.dirty().len());
        let mut answer = None;
        let response = egui::Modal::new(Id::new("leave"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10)
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Unsaved changes").size(14.0).color(TEXT));
                ui.add_space(4.0);
                let what = match unsaved {
                    1 => "One document has".to_string(),
                    n => format!("{n} documents have"),
                };
                ui.label(
                    RichText::new(format!(
                        "{what} changes that are not saved. Save them before leaving?"
                    ))
                    .color(MUTED),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(primary("Save and leave")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Leave without saving").clicked() {
                        answer = Some(false);
                    }
                    if ui.button("Cancel").clicked() {
                        self.leaving = None;
                    }
                });
            });
        if response.should_close() && answer.is_none() {
            self.leaving = None;
        }
        match answer {
            Some(true) => {
                self.save();
                if self.store.as_ref().is_some_and(|s| s.dirty().is_empty()) {
                    self.leaving = None;
                    self.close = Some(to);
                }
            }
            Some(false) => {
                self.leaving = None;
                self.close = Some(to);
            }
            None => {}
        }
    }

    fn assistant(&mut self, ui: &mut Ui) {
        egui::Panel::right("assistant")
            .exact_size(ASSISTANT)
            .resizable(false)
            .show_separator_line(true)
            .frame(Frame::new().fill(SUNK))
            .show(ui, |ui| {
                egui::Panel::top("assistant-heading")
                    .exact_size(HEADER)
                    .resizable(false)
                    .show_separator_line(true)
                    .frame(Frame::new().inner_margin(Margin {
                        left: 16,
                        right: 12,
                        top: 0,
                        bottom: 0,
                    }))
                    .show(ui, |ui| {
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add_space(13.0);
                                ui.label(
                                    RichText::new("Assistant").size(13.0).color(TEXT).strong(),
                                );
                                ui.label(eyebrow("All editors").size(8.0));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(ui, Icon::Close, false, true)
                                    .on_hover_text("Close")
                                    .clicked()
                                {
                                    self.assistant = false;
                                }
                                icon_button(ui, Icon::Plus, false, false)
                                    .on_hover_text("New thread");
                            });
                        });
                    });
                egui::Panel::top("assistant-threads")
                    .resizable(false)
                    .show_separator_line(true)
                    .frame(Frame::new().inner_margin(Margin::symmetric(16, 12)))
                    .show(ui, |ui| {
                        ui.label(eyebrow("Threads"));
                        ui.label(RichText::new("No threads yet").color(FAINT));
                    });
                egui::Panel::bottom("assistant-composer")
                    .resizable(false)
                    .show_separator_line(false)
                    .frame(Frame::new().inner_margin(Margin::same(12)))
                    .show(ui, |ui| {
                        Frame::new()
                            .fill(RAISED)
                            .stroke(Stroke::new(1.0, STRONG))
                            .corner_radius(10)
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.add_enabled_ui(false, |ui| {
                                    let mut empty = String::new();
                                    ui.add(
                                        TextEdit::multiline(&mut empty)
                                            .hint_text("Ask about anything in the open package…")
                                            .desired_rows(2)
                                            .desired_width(f32::INFINITY)
                                            .frame(Frame::NONE),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Claude API").color(FAINT).size(11.0),
                                        );
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                icon_button(ui, Icon::Send, false, false);
                                            },
                                        );
                                    });
                                });
                            });
                    });
                egui::CentralPanel::no_frame().show(ui, |ui| {
                    empty(
                        ui,
                        "The assistant comes in a later milestone",
                        "One assistant for every editor, with a thread per topic. It edits through the same commands as the editor, so its changes undo like yours.",
                    );
                });
            });
    }
}

fn library_panel(
    ui: &mut Ui,
    kind: Kind,
    workspace: &mut Workspace,
    store: &Store,
    package: &Package,
) -> bool {
    let mut create = false;
    egui::Panel::left(Id::new(("library", kind)))
        .default_size(LIBRARY)
        .size_range(180.0..=380.0)
        .resizable(true)
        .show_separator_line(true)
        .frame(
            Frame::new()
                .fill(PANEL)
                .inner_margin(Margin::symmetric(10, 14)),
        )
        .show(ui, |ui| {
            egui::Panel::bottom(Id::new(("library-footer", kind)))
                .resizable(false)
                .show_separator_line(true)
                .frame(Frame::new().inner_margin(Margin {
                    left: 6,
                    right: 6,
                    top: 8,
                    bottom: 0,
                }))
                .show(ui, |ui| {
                    let library = &workspace.library;
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} {} in {}",
                                library.count,
                                kind.plural(),
                                package.manifest.id
                            ))
                            .color(FAINT)
                            .size(11.0),
                        );
                        if !library.problems.is_empty() {
                            ui.label(
                                RichText::new(format!("· {} problems", library.problems.len()))
                                    .color(DIRTY)
                                    .size(11.0),
                            )
                            .on_hover_text(library.problems.join("\n"));
                        }
                    });
                });
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(eyebrow(&format!("{} library", kind.label())));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can = kind
                        .sections()
                        .iter()
                        .any(|(_, c)| c.form == Storage::Files);
                    let hint = if can {
                        format!("New in {}", kind.name())
                    } else {
                        "New elements of this kind come with their editor.".to_string()
                    };
                    create = icon_button(ui, Icon::Plus, false, can)
                        .on_hover_text(hint)
                        .clicked();
                });
            });
            ui.add_space(4.0);
            let search = ui.add(
                TextEdit::singleline(&mut workspace.filter)
                    .hint_text(format!("Search {}…", kind.plural()))
                    .desired_width(f32::INFINITY)
                    .margin(Margin {
                        left: 26,
                        right: 8,
                        top: 6,
                        bottom: 6,
                    }),
            );
            icons::paint(
                ui.painter(),
                icons::square(
                    pos2(search.rect.left() + 14.0, search.rect.center().y),
                    13.0,
                ),
                Icon::Search,
                FAINT,
            );
            ui.add_space(6.0);
            let active = workspace
                .dock
                .main_surface_mut()
                .find_active_focused()
                .map(|(_, d)| d.document.clone());
            let mut open = None;
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let filter = workspace.filter.trim().to_lowercase();
                    let shown = Shown {
                        active: active.as_deref(),
                        store,
                    };
                    if filter.is_empty() {
                        tree(
                            ui,
                            &workspace.library.nodes,
                            0,
                            &mut workspace.expanded,
                            &shown,
                            &mut open,
                        );
                    } else {
                        let mut all = Vec::new();
                        workspace
                            .library
                            .nodes
                            .iter()
                            .for_each(|n| n.elements(&mut all));
                        let matches: Vec<_> = all
                            .into_iter()
                            .filter(|e| {
                                e.name.to_lowercase().contains(&filter)
                                    || e.id.to_lowercase().contains(&filter)
                            })
                            .collect();
                        if matches.is_empty() {
                            ui.label(RichText::new("Nothing matches").color(FAINT));
                        }
                        for element in matches {
                            if element_row(ui, 0, element, &shown).clicked() {
                                open = Some(element.clone());
                            }
                        }
                    }
                });
            if let Some(element) = open {
                workspace.open(&element);
            }
        });
    create
}

/// What library rows mark: the open document and unsaved ones.
struct Shown<'a> {
    active: Option<&'a str>,
    store: &'a Store,
}

fn tree(
    ui: &mut Ui,
    nodes: &[Node],
    depth: usize,
    expanded: &mut HashSet<String>,
    shown: &Shown,
    open: &mut Option<Element>,
) {
    for node in nodes {
        match node {
            Node::Folder {
                name,
                path,
                children,
                count,
            } => {
                let is_open = expanded.contains(path);
                let response = row(ui, depth, name, Some(*count), Some(is_open), false, false);
                if response.clicked() {
                    if is_open {
                        expanded.remove(path);
                    } else {
                        expanded.insert(path.clone());
                    }
                }
                if is_open {
                    tree(ui, children, depth + 1, expanded, shown, open);
                }
            }
            Node::Element(element) => {
                if element_row(ui, depth, element, shown).clicked() {
                    *open = Some(element.clone());
                }
            }
        }
    }
}

fn element_row(ui: &mut Ui, depth: usize, element: &Element, shown: &Shown) -> egui::Response {
    row(
        ui,
        depth,
        &element.name,
        None,
        None,
        shown.active == Some(element.document.as_str()),
        shown.store.is_dirty(&element.document),
    )
    .on_hover_text(&element.id)
}
