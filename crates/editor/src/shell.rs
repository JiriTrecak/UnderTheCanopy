//! The editor's frame (docs/editor.md, M1): the rail that switches editors, the header,
//! each editor's library, document tabs and inspector, and the assistant sidebar on the
//! right. Every edit goes through the [`Store`]: one undo history across editors, Cmd+S
//! saves every changed document, leaving with unsaved changes asks first.

use crate::checker::Checker;
use crate::document::{Command, Context, Outside, Store};
use crate::icons::{self, Icon};
use crate::library::{Element, KINDS, Kind, Library, Node};
use crate::theme::{self, *};
use crate::watch::Watch;
use crate::widgets::*;
use canopy_content::package::Package;
use egui::{
    Align, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Ui, pos2, vec2,
};
use egui_dock::{DockArea, DockState, TabViewer};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// One open document.
pub struct Doc {
    /// The element's name when it was opened (its `name` field wins while it has one).
    name: String,
    id: String,
    /// Path from the package root.
    document: String,
    /// The JSON view: the text of the revision shown, and the user's edit of it.
    shown: Option<u64>,
    text: String,
    edit: String,
    error: Option<String>,
    /// The inspector's name field while it is not being typed in.
    name_field: String,
}

impl Doc {
    fn new(element: &Element) -> Self {
        Self {
            name: element.name.clone(),
            id: element.id.clone(),
            document: element.document.clone(),
            shown: None,
            text: String::new(),
            edit: String::new(),
            error: None,
            name_field: String::new(),
        }
    }
}

/// One editor's state, kept while switching editors.
struct Workspace {
    library: Library,
    dock: DockState<Doc>,
    filter: String,
    /// Expanded library folders, by path.
    expanded: HashSet<String>,
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
        }
    }

    /// Opens (or focuses) `element`'s tab; shows its folder in the library.
    fn open(&mut self, element: &Element) {
        if let Some(path) = self.dock.find_tab_from(|d| d.document == element.document) {
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

/// An edit a panel asks for this frame; it runs once the frame's panels are drawn.
struct Edit {
    label: String,
    context: Context,
    commands: Vec<Command>,
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
    /// The editor is done: back to the menu, or quit the game.
    pub close: Option<Leave>,
}

impl Shell {
    pub fn new(package: Result<Package, String>) -> Self {
        let store = package.as_ref().ok().map(|p| Store::new(&p.root));
        let checker = package.as_ref().ok().cloned().map(Checker::new);
        let watch = package.as_ref().ok().and_then(|p| Watch::new(&p.root));
        Self {
            watch,
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
        let kind = self.current;
        let workspace = self
            .workspaces
            .entry(kind)
            .or_insert_with(|| Workspace::read(package, kind));
        let store = self.store.as_mut().expect("a store with the package");
        let mut edits = Vec::new();
        library_panel(ui, kind, workspace, store, package);
        inspector(ui, kind, workspace, store, &mut edits);
        documents(ui, kind, workspace, store, &mut edits);
        for edit in edits {
            self.run(edit);
        }
        self.check();
        self.conflict_dialog(ui.ctx());
        self.leave_dialog(ui.ctx());
        self.save_dialog(ui.ctx());
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
        let mut kinds = HashSet::new();
        let mut unreadable = None;
        if let Some(store) = &mut self.store {
            for document in &changed {
                if let Outside::Unreadable(message) = store.outside_change(document) {
                    unreadable = Some(message);
                }
                kinds.extend(Kind::of_document(document));
            }
        }
        for kind in kinds {
            if let Some(workspace) = self.workspaces.get_mut(&kind) {
                workspace.library = Library::read(package, kind);
            }
        }
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
        if let Err(error) = store.run(&edit.label, edit.context, edit.commands) {
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
        let mut clicked = (false, false, false);
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
                        status(ui, "MCP off", FAINT).on_hover_text(
                            "The MCP server for agents comes later in this milestone.",
                        );
                        ui.add_enabled_ui(false, |ui| {
                            ui.button("Test in game").on_disabled_hover_text(
                                "Playing the open package comes later in this milestone.",
                            );
                        });
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
) {
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
                    icon_button(ui, Icon::Plus, false, false)
                        .on_hover_text("Creating elements comes with the data editors.");
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

fn inspector(
    ui: &mut Ui,
    kind: Kind,
    workspace: &mut Workspace,
    store: &mut Store,
    edits: &mut Vec<Edit>,
) {
    egui::Panel::right(Id::new(("inspector", kind)))
        .default_size(INSPECTOR)
        .size_range(260.0..=480.0)
        .resizable(true)
        .show_separator_line(true)
        .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(14)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let Some((_, doc)) = workspace.dock.main_surface_mut().find_active_focused() else {
                ui.label(eyebrow("Inspector"));
                ui.label(RichText::new("Open an element from the library.").color(FAINT));
                return;
            };
            ui.label(RichText::new("Properties").size(13.0).color(TEXT).strong());
            let name = match store.get(&doc.document) {
                Ok(Some(value)) => value.get("name").and_then(Value::as_str).map(str::to_string),
                _ => None,
            };
            match name {
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
                        edits.push(Edit {
                            label: format!("Rename {name}"),
                            context: Context {
                                editor: kind,
                                document: doc.document.clone(),
                                pointer: Some("/name".into()),
                            },
                            commands: vec![Command::Set {
                                document: doc.document.clone(),
                                pointer: "/name".into(),
                                value: Value::String(doc.name_field.clone()),
                                at: None,
                            }],
                        });
                    }
                }
                None => field(ui, "Name", &doc.name, false),
            }
            field(ui, "Id", &doc.id, true);
            field(ui, "File", &doc.document, true);
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Forms for every field come with the schemas; until then, edit the JSON in the tab.",
                )
                .color(FAINT)
                .size(11.0),
            );
        });
}

struct Viewer<'a> {
    kind: Kind,
    store: &'a mut Store,
    edits: &'a mut Vec<Edit>,
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
            Ok(Some(value)) => value,
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
            let text = crate::document::format(value);
            // Typing is kept until applied or reverted; otherwise follow the document.
            if tab.edit == tab.text {
                tab.edit = text.clone();
            }
            tab.text = text;
            tab.shown = Some(revision);
        }
        let changed = tab.edit != tab.text;
        if changed || tab.error.is_some() {
            ui.horizontal(|ui| {
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
            });
            ui.add_space(6.0);
        }
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

    fn allowed_in_windows(&self, _tab: &mut Doc) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Doc) -> [bool; 2] {
        [false, false]
    }
}

fn documents(
    ui: &mut Ui,
    kind: Kind,
    workspace: &mut Workspace,
    store: &mut Store,
    edits: &mut Vec<Edit>,
) {
    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(MAIN))
        .show(ui, |ui| {
            if workspace.dock.iter_all_tabs().next().is_none() {
                empty(
                    ui,
                    &format!("No {} open", kind.plural()),
                    "Pick one in the library; it opens as a tab here.",
                );
                return;
            }
            DockArea::new(&mut workspace.dock)
                .id(Id::new(("dock", kind)))
                .style(theme::dock(ui.style()))
                .show_add_buttons(false)
                .show_close_buttons(true)
                .show_leaf_close_all_buttons(false)
                .show_leaf_collapse_buttons(false)
                .tab_context_menus(false)
                .show_inside(ui, &mut Viewer { kind, store, edits });
        });
}
