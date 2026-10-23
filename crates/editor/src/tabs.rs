//! Document tabs (egui_dock): each open document in its view (a map, an overview, an
//! ability's stage, or the form built from its schema when it has nothing better), its
//! form and JSON text a click away in a modal (the switches sit in the tab bar). Changes
//! come back as [`Edit`]s for the shell to run as undo steps.

use crate::document::{Command, Context, Store};
use crate::forms::Form;
use crate::library::{Element, Kind};
use crate::theme::{self, *};
use crate::widgets::*;
use egui::{
    Align, Align2, Frame, Id, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit, Ui, vec2,
};
use egui_dock::{DockArea, DockState, TabViewer};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// A map's view: the viewport and the map tools.
    Map,
    Overview,
    /// An ability's stage: cast on test targets with the real simulation.
    Stage,
    /// A campaign's steps and settings (`crate::campaign::view`).
    Campaign,
    /// The form, for documents with no view of their own.
    Form,
}

/// A document's form or JSON text, in a modal over its view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Form,
    Json,
}

/// The views a document switches between, the first its own.
fn views(document: &str) -> &'static [(View, &'static str)] {
    if crate::map::root_of(document).is_some() {
        &[(View::Map, "Map")]
    } else if crate::campaign::root_of(document).is_some() {
        &[(View::Campaign, "Campaign")]
    } else if is_ability(document) {
        &[(View::Overview, "Overview"), (View::Stage, "Stage")]
    } else if crate::overview::has(document) {
        &[(View::Overview, "Overview")]
    } else {
        &[(View::Form, "Form")]
    }
}

/// One open document.
pub struct Doc {
    /// The element's name when it was opened (its `name` field wins while it has one).
    pub name: String,
    pub id: String,
    /// Path from the package root.
    pub document: String,
    view: View,
    /// The form or JSON modal, while open.
    sheet: Option<Sheet>,
    /// The document's schema, once looked up (`None` inside: the format has none).
    schema: Option<Option<Value>>,
    /// The JSON view: the text of the revision shown, and the user's edit of it.
    shown: Option<u64>,
    text: String,
    edit: String,
    error: Option<String>,
    /// The inspector's name field while it is not being typed in.
    pub name_field: String,
    /// The description being typed (a named element's, from the text library).
    pub description_field: String,
}

impl Doc {
    pub fn new(element: &Element) -> Self {
        Self {
            name: element.name.clone(),
            id: element.id.clone(),
            document: element.document.clone(),
            view: views(&element.document)[0].0,
            sheet: None,
            schema: None,
            shown: None,
            text: String::new(),
            edit: String::new(),
            error: None,
            name_field: String::new(),
            description_field: String::new(),
        }
    }

    /// Opens the form or JSON modal over the document.
    pub fn show_sheet(&mut self, sheet: Option<Sheet>) {
        self.sheet = sheet;
    }

    /// Shows the ability stage (abilities only).
    pub fn show_stage(&mut self) {
        if is_ability(&self.document) {
            self.view = View::Stage;
        }
    }
}

/// Whether `document` is an ability (it has a stage).
pub fn is_ability(document: &str) -> bool {
    document.starts_with("abilities/") && document.ends_with(".json")
}

/// Whether `document`'s element has unsaved changes: for a map, any file of its package
/// (its blocks, its scenery, its name).
pub fn unsaved(store: &Store, document: &str) -> bool {
    match crate::map::root_of(document).or_else(|| crate::campaign::root_of(document)) {
        Some(root) => {
            let folder = format!("{root}/");
            store.dirty().iter().any(|path| path.starts_with(&folder))
        }
        None => store.is_dirty(document),
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
    /// The split view's generated world: its last picture and its pane's size.
    pub generated_texture: Option<egui::TextureId>,
    pub generated_size: &'a mut Option<(String, [u32; 2])>,
    /// A map whose picker preview was asked for.
    pub preview: &'a mut Option<String>,
    /// The package folder on disk.
    pub root: Option<std::path::PathBuf>,
    /// Abilities' stages by document, what their grounds are built from (the pack and
    /// the looks loaded so far; a look the stage needs and lacks is asked for), the
    /// stage's size this frame, and whether the content lags behind the edits (the
    /// latest ones do not pass the check).
    pub encounters: &'a mut std::collections::HashMap<String, crate::encounter::Session>,
    pub pack: Option<&'a std::sync::Arc<canopy_assets::Pack>>,
    pub looks: &'a std::collections::HashMap<String, canopy_scene::Look>,
    pub wanted_looks: &'a mut Vec<String>,
    pub encounter_size: &'a mut Option<(String, [u32; 2])>,
    pub stale: bool,
    /// Campaign views' thumbnails, states and requests; each campaign's mission maps (by
    /// its folder); the base package's version (new mission maps depend on it).
    pub campaigns: &'a mut crate::campaign::view::Shared,
    pub campaign_maps: &'a std::collections::BTreeMap<String, Vec<String>>,
    pub version: &'a str,
}

impl TabViewer for Viewer<'_> {
    type Tab = Doc;

    fn id(&mut self, tab: &mut Doc) -> Id {
        Id::new(&tab.document)
    }

    fn title(&mut self, tab: &mut Doc) -> egui::WidgetText {
        let name = name_in(self.store, tab);
        if unsaved(self.store, &tab.document) {
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
        self.bar(ui, tab);
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
                    (Some(map), Some(scenery)) => Some(crate::map::view::Docs {
                        map,
                        scenery,
                        package: self
                            .store
                            .get(&format!("{root}/package.json"))
                            .ok()
                            .flatten()
                            .cloned()
                            .unwrap_or_default(),
                    }),
                    _ => None,
                };
                // A campaign's mission: its campaign and step, for the Tech view and the
                // Players section.
                let mission = crate::campaign::mission_of(&root).and_then(|(campaign, map)| {
                    let value = self
                        .store
                        .get(&crate::campaign::document(campaign))
                        .ok()
                        .flatten()?
                        .clone();
                    let library = self
                        .store
                        .get(&crate::campaign::text_file(campaign))
                        .ok()
                        .flatten()
                        .cloned()
                        .unwrap_or_default();
                    Some(crate::campaign::MissionLink {
                        campaign: campaign.to_string(),
                        step: crate::campaign::step_of_map(&value, map),
                        map: map.to_string(),
                        value,
                        library,
                    })
                });
                let session = self
                    .maps
                    .entry(root.clone())
                    .or_insert_with(|| crate::map::Session::new(&root));
                session.mission = mission;
                let out = crate::world::show(
                    ui,
                    session,
                    self.content.as_deref(),
                    docs.as_ref(),
                    self.scene_texture,
                    self.generated_texture,
                    self.root.as_deref(),
                );
                self.edits.extend(out.edits);
                if out.preview {
                    *self.preview = Some(root.clone());
                }
                if let Some(size) = out.size_generated {
                    *self.generated_size = Some((root.clone(), size));
                }
                if let Some(size) = out.size {
                    *self.map_size = Some((root, size));
                }
            }
            View::Stage => {
                let id = value.get("id").and_then(Value::as_str).unwrap_or(&tab.id);
                let session = self
                    .encounters
                    .entry(tab.document.clone())
                    .or_insert_with(|| crate::encounter::Session::new(id));
                // A renamed ability starts its stage again.
                if session.ability != id {
                    *session = crate::encounter::Session::new(id);
                }
                if let (Some(content), Some(pack)) = (&self.content, self.pack) {
                    match self.looks.get(session.look_biome()) {
                        Some(look) => session.prepare(content, pack, look),
                        None => self.wanted_looks.push(session.look_biome().to_string()),
                    }
                }
                let out = crate::encounter::show(
                    ui,
                    session,
                    self.content.as_deref(),
                    self.scene_texture,
                    self.stale,
                );
                if let Some(size) = out.size {
                    *self.encounter_size = Some((tab.document.clone(), size));
                }
            }
            View::Overview => {
                let uses = self
                    .uses
                    .filter(|(document, _)| *document == tab.document)
                    .map_or(&[][..], |(_, uses)| uses);
                let mut overview = crate::overview::Overview::new(&tab.document, self.index, uses);
                overview.root = self.root.as_deref();
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
                if let Some(unit) = overview.stage_with {
                    let id = value.get("id").and_then(Value::as_str).unwrap_or(&tab.id);
                    self.encounters
                        .entry(tab.document.clone())
                        .or_insert_with(|| crate::encounter::Session::new(id))
                        .set_caster(&unit);
                    tab.view = View::Stage;
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
            View::Campaign => {
                let maps = crate::campaign::root_of(&tab.document)
                    .and_then(|root| self.campaign_maps.get(root))
                    .cloned()
                    .unwrap_or_default();
                let mut ctx = crate::campaign::view::Ctx {
                    document: &tab.document,
                    store: &mut *self.store,
                    content: self.content.as_deref(),
                    index: self.index,
                    root: self.root.as_deref(),
                    maps,
                    version: self.version.to_string(),
                    shared: &mut *self.campaigns,
                    edits: &mut *self.edits,
                };
                crate::campaign::view::show(ui, &mut ctx);
            }
            View::Form => self.form(ui, tab, &value, f32::INFINITY),
        }
        if let Some(sheet) = tab.sheet {
            self.sheet(ui.ctx(), tab, sheet, &value);
        }
    }

    fn allowed_in_windows(&self, _tab: &mut Doc) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Doc) -> [bool; 2] {
        [false, false]
    }
}

impl Viewer<'_> {
    /// The document's name: its name in the text library, or its `name` field.
    fn name_of(&mut self, tab: &Doc) -> String {
        name_in(self.store, tab)
    }

    /// The switches in the tab bar, right of the tabs, for the document shown: its views
    /// when it has several, its form (unless that is its view) and its JSON.
    fn bar(&mut self, ui: &Ui, tab: &mut Doc) {
        let body = ui.clip_rect();
        let height = theme::dock(ui.style()).tab_bar.height;
        egui::Area::new(Id::new(("document-bar", &tab.document)))
            .order(egui::Order::Middle)
            .pivot(Align2::RIGHT_CENTER)
            .fixed_pos(egui::pos2(body.right() - 8.0, body.top() - height / 2.0))
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let views = views(&tab.document);
                    if views.len() > 1 {
                        for (view, label) in views {
                            if ui.selectable_label(tab.view == *view, *label).clicked() {
                                tab.view = *view;
                            }
                        }
                        let (line, _) =
                            ui.allocate_exact_size(vec2(1.0, 16.0), egui::Sense::hover());
                        ui.painter().rect_filled(line, 0, BORDER);
                    }
                    if tab.view != View::Form
                        && ui
                            .selectable_label(tab.sheet == Some(Sheet::Form), "Form")
                            .on_hover_text("Every field of the document, as a form")
                            .clicked()
                    {
                        tab.sheet = Some(Sheet::Form);
                    }
                    if ui
                        .selectable_label(tab.sheet == Some(Sheet::Json), "JSON")
                        .on_hover_text("The document's JSON text")
                        .clicked()
                    {
                        tab.sheet = Some(Sheet::Json);
                    }
                });
            });
    }

    /// The document as a form built from its schema (or from its values), at most
    /// `height` tall.
    fn form(&mut self, ui: &mut Ui, tab: &mut Doc, value: &Value, height: f32) {
        let schema = tab
            .schema
            .get_or_insert_with(|| canopy_content::schema::of_document(&tab.document));
        let mut form = Form::new(schema.as_ref(), &tab.document, self.ids);
        form.highlight = self
            .highlight
            .filter(|(document, _)| *document == tab.document)
            .map(|(_, pointer)| pointer);
        ScrollArea::vertical()
            .id_salt(("form", &tab.document))
            .min_scrolled_height(height.min(ui.available_height().max(0.0)))
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if schema.is_none() {
                    ui.label(
                        RichText::new(
                            "This kind of document has no schema yet: the form follows its values.",
                        )
                        .color(FAINT)
                        .size(11.0),
                    );
                }
                form.show(ui, value);
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

    /// The form or JSON modal over the document's view; Esc, the backdrop or the close
    /// button closes it (typed JSON is kept until applied or reverted).
    fn sheet(&mut self, ctx: &egui::Context, tab: &mut Doc, sheet: Sheet, value: &Value) {
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() * 0.7).clamp(420.0, 1100.0),
            (screen.height() * 0.8).max(240.0),
        );
        let name = self.name_of(tab);
        let mut close = false;
        let response = egui::Modal::new(Id::new(("sheet", &tab.document)))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10)
                    .inner_margin(Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.set_width(size.x);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&name).size(14.0).color(TEXT).strong());
                    let what = match sheet {
                        Sheet::Form => "Form",
                        Sheet::Json => "JSON",
                    };
                    ui.label(RichText::new(what).size(12.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, crate::icons::Icon::Close, false, true)
                            .on_hover_text("Close (Esc)")
                            .clicked()
                        {
                            close = true;
                        }
                        if sheet == Sheet::Json {
                            self.json_actions(ui, tab);
                        }
                    });
                });
                ui.add_space(8.0);
                let height = size.y - 48.0;
                match sheet {
                    Sheet::Form => self.form(ui, tab, value, height),
                    Sheet::Json => {
                        ScrollArea::both()
                            .id_salt(("json", &tab.document))
                            .min_scrolled_height(height)
                            .max_height(height)
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
            });
        if close || response.should_close() {
            tab.sheet = None;
        }
    }

    /// The JSON modal's Apply and Revert (right to left), and why Apply failed.
    fn json_actions(&mut self, ui: &mut Ui, tab: &mut Doc) {
        let changed = tab.edit != tab.text;
        if ui
            .add_enabled(changed || tab.error.is_some(), primary("Apply"))
            .clicked()
        {
            match serde_json::from_str::<Value>(&tab.edit) {
                Ok(value) => {
                    tab.error = None;
                    // The text follows the document again, in the file's format.
                    tab.edit = crate::document::format(&value);
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
        if ui
            .add_enabled(changed || tab.error.is_some(), egui::Button::new("Revert"))
            .clicked()
        {
            tab.edit = tab.text.clone();
            tab.error = None;
        }
        if let Some(error) = &tab.error {
            ui.label(RichText::new(error).color(ERROR).size(11.0));
        } else if changed {
            ui.label(RichText::new("Edited").color(DIRTY).size(11.0));
        }
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

/// A tab's name: a named element's from its package's text library (D96), another
/// document's `name` field, or the name it opened with.
fn name_in(store: &mut crate::document::Store, tab: &Doc) -> String {
    let Ok(Some(value)) = store.get(&tab.document).map(|v| v.cloned()) else {
        return tab.name.clone();
    };
    if let Some((key, file)) =
        crate::text::key(&tab.document, &value).zip(crate::text::file(&tab.document))
        && let Ok(Some(text)) = store.get(&file)
        && let Some(name) = text.get(format!("{key}.name")).and_then(Value::as_str)
    {
        return name.to_string();
    }
    value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&tab.name)
        .to_string()
}
