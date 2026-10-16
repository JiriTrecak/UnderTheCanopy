//! The editor's frame (docs/editor.md, M1): the rail that switches editors, the header,
//! each editor's library, document tabs and inspector, and the assistant sidebar on the
//! right. Documents open read-only; editing, the viewport, MCP and the assistant come
//! later and show here as disabled controls.

use crate::icons::{self, Icon};
use crate::library::{Element, KINDS, Kind, Library, Node};
use crate::theme::{self, *};
use canopy_content::package::Package;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Layout, Margin, Rect, Response,
    RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};
use egui_dock::{DockArea, DockState, TabViewer};
use std::collections::{HashMap, HashSet};

/// One open document.
pub struct Doc {
    name: String,
    id: String,
    /// Path from the package root.
    document: String,
    text: String,
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
}

pub struct Shell {
    package: Result<Package, String>,
    current: Kind,
    workspaces: HashMap<Kind, Workspace>,
    assistant: bool,
    /// The editor asked to close (back to the game's menu).
    pub close: bool,
}

impl Shell {
    pub fn new(package: Result<Package, String>) -> Self {
        Self {
            package,
            current: Kind::Abilities,
            workspaces: HashMap::new(),
            assistant: false,
            close: false,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        self.rail(ui);
        if self.assistant {
            self.assistant(ui);
        }
        self.header(ui);
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
        library_panel(ui, kind, workspace, package);
        inspector(ui, kind, workspace);
        documents(ui, kind, workspace);
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
            open_document(&mut workspace.dock, package, element);
            // Show where it lives in the tree.
            let mut folder = String::new();
            for part in element
                .path
                .split('/')
                .collect::<Vec<_>>()
                .iter()
                .rev()
                .skip(1)
                .rev()
            {
                folder = if folder.is_empty() {
                    (*part).to_string()
                } else {
                    format!("{folder}/{part}")
                };
                workspace.expanded.insert(folder.clone());
            }
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
                            self.close = true;
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
            .map(|(_, doc)| doc.name.clone());
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
                    chip(ui, &package, "Read-only").on_hover_text(
                        "The repository's base package. Editing comes with the data editors; documents open read-only.",
                    );
                    if let Some(title) = &title {
                        ui.add_space(4.0);
                        ui.label(RichText::new(title).color(TEXT));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if icon_button(ui, Icon::Panel, self.assistant, true)
                            .on_hover_text("Assistant")
                            .clicked()
                        {
                            self.assistant = !self.assistant;
                        }
                        ui.add_enabled(false, primary("Save"))
                            .on_disabled_hover_text("Saving comes with the data editors.");
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
                        icon_button(ui, Icon::Redo, false, false).on_disabled_hover_text("Redo");
                        icon_button(ui, Icon::Undo, false, false).on_disabled_hover_text("Undo");
                    });
                });
            });
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
                                ui.label(RichText::new("Assistant").size(13.0).color(TEXT).strong());
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
                                    .on_disabled_hover_text("New thread");
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
                                        ui.label(RichText::new("Claude API").color(FAINT).size(11.0));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            icon_button(ui, Icon::Send, false, false);
                                        });
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

fn library_panel(ui: &mut Ui, kind: Kind, workspace: &mut Workspace, package: &Package) {
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
                        .on_disabled_hover_text("Creating elements comes with the data editors.");
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
                    if filter.is_empty() {
                        tree(
                            ui,
                            &workspace.library.nodes,
                            0,
                            &mut workspace.expanded,
                            active.as_deref(),
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
                            if element_row(ui, 0, element, active.as_deref()).clicked() {
                                open = Some(element.clone());
                            }
                        }
                    }
                });
            if let Some(element) = open {
                open_document(&mut workspace.dock, package, &element);
            }
        });
}

fn tree(
    ui: &mut Ui,
    nodes: &[Node],
    depth: usize,
    expanded: &mut HashSet<String>,
    active: Option<&str>,
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
                let response = row(ui, depth, name, Some(*count), Some(is_open), false);
                if response.clicked() {
                    if is_open {
                        expanded.remove(path);
                    } else {
                        expanded.insert(path.clone());
                    }
                }
                if is_open {
                    tree(ui, children, depth + 1, expanded, active, open);
                }
            }
            Node::Element(element) => {
                if element_row(ui, depth, element, active).clicked() {
                    *open = Some(element.clone());
                }
            }
        }
    }
}

fn element_row(ui: &mut Ui, depth: usize, element: &Element, active: Option<&str>) -> Response {
    row(
        ui,
        depth,
        &element.name,
        None,
        None,
        active == Some(element.document.as_str()),
    )
    .on_hover_text(&element.id)
}

/// A library row: a folder (`folder` = whether it is open) or an element.
fn row(
    ui: &mut Ui,
    depth: usize,
    label: &str,
    count: Option<usize>,
    folder: Option<bool>,
    selected: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        SELECTED
    } else if response.hovered() {
        RAISED
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 5, fill);
    let mut x = rect.left() + 8.0 + depth as f32 * 14.0;
    let y = rect.center().y;
    match folder {
        Some(open) => {
            let icon = if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            icons::paint(painter, icons::square(pos2(x + 6.0, y), 12.0), icon, MUTED);
        }
        None => {
            let color = if selected { SOFT } else { FAINT };
            icons::paint(
                painter,
                icons::square(pos2(x + 6.0, y), 13.0),
                Icon::File,
                color,
            );
        }
    }
    x += 18.0;
    let color = if selected { TEXT } else { SOFT };
    let right = match count {
        Some(n) => {
            let r = painter.text(
                pos2(rect.right() - 8.0, y),
                Align2::RIGHT_CENTER,
                n.to_string(),
                FontId::proportional(11.0),
                FAINT,
            );
            r.left() - 6.0
        }
        None => rect.right() - 6.0,
    };
    let galley = painter.layout_no_wrap(label.to_string(), FontId::proportional(12.0), color);
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
    painter
        .with_clip_rect(clip.intersect(painter.clip_rect()))
        .galley(pos2(x, y - galley.size().y / 2.0), galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn open_document(dock: &mut DockState<Doc>, package: &Package, element: &Element) {
    if let Some(path) = dock.find_tab_from(|d| d.document == element.document) {
        let _ = dock.set_active_tab(path);
        dock.set_focused_node_and_surface(path.node_path());
        return;
    }
    let text = std::fs::read_to_string(package.root.join(&element.document))
        .unwrap_or_else(|e| format!("Could not read {}: {e}", element.document));
    dock.push_to_focused_leaf(Doc {
        name: element.name.clone(),
        id: element.id.clone(),
        document: element.document.clone(),
        text,
    });
}

fn inspector(ui: &mut Ui, kind: Kind, workspace: &mut Workspace) {
    egui::Panel::right(Id::new(("inspector", kind)))
        .default_size(INSPECTOR)
        .size_range(260.0..=480.0)
        .resizable(true)
        .show_separator_line(true)
        .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(14)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            match workspace.dock.main_surface_mut().find_active_focused() {
                None => {
                    ui.label(eyebrow("Inspector"));
                    ui.label(RichText::new("Open an element from the library.").color(FAINT));
                }
                Some((_, doc)) => {
                    ui.label(RichText::new("Properties").size(13.0).color(TEXT).strong());
                    field(ui, "Name", &doc.name, false);
                    field(ui, "Id", &doc.id, true);
                    field(ui, "File", &doc.document, true);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Read-only for now: fields, undo and saving come with the data editors.",
                        )
                        .color(FAINT)
                        .size(11.0),
                    );
                }
            }
        });
}

/// A labelled read-only value in an input-like well.
fn field(ui: &mut Ui, label: &str, value: &str, mono: bool) {
    ui.label(RichText::new(label).color(MUTED).size(11.0));
    Frame::new()
        .fill(BG)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(5)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let text = RichText::new(value).color(TEXT);
            ui.add(egui::Label::new(if mono { text.monospace() } else { text }).truncate());
        });
}

struct Viewer;

impl TabViewer for Viewer {
    type Tab = Doc;

    fn id(&mut self, tab: &mut Doc) -> Id {
        Id::new(&tab.document)
    }

    fn title(&mut self, tab: &mut Doc) -> egui::WidgetText {
        tab.name.as_str().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Doc) {
        ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut tab.text.as_str())
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

fn documents(ui: &mut Ui, kind: Kind, workspace: &mut Workspace) {
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
                .show_inside(ui, &mut Viewer);
        });
}

/// A centred title and explanation for an empty area.
fn empty(ui: &mut Ui, title: &str, body: &str) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space((ui.available_height() / 2.0 - 40.0).max(24.0));
        ui.set_max_width(ui.available_width().min(320.0));
        ui.label(RichText::new(title).color(SOFT).size(13.0));
        ui.add_space(2.0);
        ui.label(RichText::new(body).color(FAINT));
    });
}

/// An uppercase spaced label (the spell studio's section titles).
fn eyebrow(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .size(9.0)
        .color(FAINT)
        .extra_letter_spacing(1.1)
}

fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(RAISED))
        .fill(TEXT)
        .stroke(Stroke::NONE)
}

/// A service's state: a coloured dot and a word, framed like a button.
fn status(ui: &mut Ui, text: &str, dot: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(11.5), MUTED);
    let size = vec2(32.0 + galley.size().x, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, RADIUS, RAISED);
    painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    painter.circle_filled(pos2(rect.left() + 13.0, rect.center().y), 3.0, dot);
    let top = rect.center().y - galley.size().y / 2.0;
    painter.galley(pos2(rect.left() + 22.0, top), galley, MUTED);
    response
}

/// The package chip: a dot, the package and a badge.
fn chip(ui: &mut Ui, name: &str, badge: &str) -> Response {
    let painter = ui.painter();
    let name = painter.layout_no_wrap(name.to_string(), FontId::proportional(11.5), BODY);
    let badge = painter.layout_no_wrap(badge.to_string(), FontId::proportional(10.0), SOFT);
    let badge_width = badge.size().x + 14.0;
    let size = vec2(22.0 + name.size().x + 8.0 + badge_width + 4.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;
    painter.rect_filled(rect, RADIUS, RAISED);
    painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    painter.circle_filled(pos2(rect.left() + 13.0, y), 3.0, TEXT);
    let x = rect.left() + 22.0;
    let name_width = name.size().x;
    painter.galley(pos2(x, y - name.size().y / 2.0), name, BODY);
    let pill = Rect::from_min_size(
        pos2(x + name_width + 8.0, y - 10.0),
        vec2(badge_width, 20.0),
    );
    painter.rect_filled(pill, 5, SELECTED);
    painter.galley(
        pos2(pill.left() + 7.0, y - badge.size().y / 2.0),
        badge,
        SOFT,
    );
    response
}

/// A 28-point square button with an icon.
fn icon_button(ui: &mut Ui, icon: Icon, on: bool, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), sense);
    let painter = ui.painter();
    let hovered = enabled && response.hovered();
    if on || hovered {
        painter.rect_filled(rect, RADIUS, SELECTED);
    }
    if on {
        painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, CONTROL), StrokeKind::Inside);
    }
    let color = match (enabled, on || hovered) {
        (false, _) => CONTROL,
        (true, true) => TEXT,
        (true, false) => MUTED,
    };
    icons::paint(painter, icons::square(rect.center(), 15.0), icon, color);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// An editor's button on the rail: icon over a short label.
fn rail_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(52.0, 48.0), Sense::click());
    let painter = ui.painter();
    let hovered = response.hovered();
    let fill = if selected {
        SELECTED
    } else if hovered {
        RAISED
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, CornerRadius::same(8), fill);
    let color = if selected {
        TEXT
    } else if hovered {
        SOFT
    } else {
        FAINT
    };
    icons::paint(
        painter,
        icons::square(pos2(rect.center().x, rect.top() + 18.0), 19.0),
        icon,
        color,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(9.5),
        color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
