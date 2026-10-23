//! A campaign's view (docs/campaign.md "The editor"): its steps as one list, chapters as
//! headings over their Story, Mission and Cinematic rows (each with its kind, title,
//! picture and what it holds), reordered by dragging, added from the bar above them;
//! every step with Play, missions with Edit. The panel on the right shows the campaign's
//! settings with nothing selected, a step's fields with one selected. Every change is an
//! undo step on the campaign's documents.

use super::art::{Choices, Thumbs, paint_cover, picker};
use super::{StepKind, Tech, mission_numbers, steps, text};
use crate::document::{Command, Context, Store};
use crate::icons::{self, Icon};
use crate::library::Kind as Editor;
use crate::tabs::Edit;
use crate::theme::*;
use crate::widgets::*;
use canopy_content::Content;
use egui::{Align2, Color32, FontId, Frame, Margin, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// What a campaign view asks of the shell.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Play from step `step` of the campaign `document`.
    Play { document: String, step: usize },
    /// Open the mission map at `root` in the Maps editor, in its Tech view when `tech`.
    Map { root: String, tech: bool },
}

/// One campaign view's state between frames.
#[derive(Debug, Default, Clone)]
pub struct State {
    pub selected: Option<usize>,
    /// The step being dragged.
    drag: Option<usize>,
    /// The art picker's search.
    search: String,
}

/// What every campaign view shares: thumbnails, each campaign's state, and the requests
/// made this frame.
#[derive(Default)]
pub struct Shared {
    pub thumbs: Thumbs,
    pub states: HashMap<String, State>,
    pub requests: Vec<Request>,
}

/// What the view reads and writes.
pub struct Ctx<'a> {
    /// The campaign's campaign.json.
    pub document: &'a str,
    pub store: &'a mut Store,
    pub content: Option<&'a Content>,
    /// Every element by id: the art and sounds pickers offer.
    pub index: &'a BTreeMap<String, crate::overview::Entry>,
    /// The package folder (art is read from it).
    pub root: Option<&'a std::path::Path>,
    /// The maps of the campaign's `missions/`.
    pub maps: Vec<String>,
    /// The base package's version (a new mission map depends on it).
    pub version: String,
    pub shared: &'a mut Shared,
    pub edits: &'a mut Vec<Edit>,
}

impl Ctx<'_> {
    fn campaign_root(&self) -> &str {
        super::root_of(self.document).unwrap_or_default()
    }

    fn context(&self, pointer: Option<String>) -> Context {
        Context {
            editor: Editor::Campaigns,
            document: self.document.to_string(),
            pointer,
        }
    }

    /// One step changing campaign.json at `pointer`; typing and dragging fold in.
    fn set(&mut self, label: &str, pointer: &str, value: Value, merge: bool) {
        self.edits.push(Edit {
            label: label.to_string(),
            context: self.context(Some(pointer.to_string())),
            commands: vec![Command::Set {
                document: self.document.to_string(),
                pointer: pointer.to_string(),
                value,
                at: None,
            }],
            merge: merge.then(|| format!("{}{pointer}", self.document)),
        });
    }

    /// Sets an optional field, or removes it when `value` is empty.
    fn set_optional(&mut self, label: &str, step: &Value, at: &str, field: &str, value: &str) {
        let pointer = format!("{at}/{field}");
        if value.is_empty() {
            if step.get(field).is_some() {
                self.edits.push(Edit {
                    label: label.to_string(),
                    context: self.context(Some(pointer.clone())),
                    commands: vec![Command::Remove {
                        document: self.document.to_string(),
                        pointer,
                    }],
                    merge: None,
                });
            }
        } else {
            self.set(label, &pointer, json!(value), true);
        }
    }

    /// The campaign's text file (D96): its steps' titles, pages, lines and notes.
    fn text_file(&self) -> String {
        super::text_file(self.campaign_root())
    }

    fn library(&mut self) -> Option<Value> {
        self.store.get(&self.text_file()).ok().flatten().cloned()
    }

    /// What a step field shows: the library's text when it holds a key, else itself.
    fn say(&mut self, value: &str) -> String {
        self.library()
            .and_then(|t| t.get(value).and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| value.to_string())
    }

    /// A free key under the step for `name` (`campaign.<campaign>.<step>.<name>`).
    fn key(&mut self, step: &Value, name: &str) -> String {
        let manifest = format!("{}/package.json", self.campaign_root());
        let campaign = self
            .store
            .get(&manifest)
            .ok()
            .flatten()
            .and_then(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| "campaign".into());
        let base = format!("campaign.{campaign}.{}.{name}", text(step, "id"));
        let library = self.library().unwrap_or_default();
        let mut key = base.clone();
        let mut n = 2;
        while library.get(&key).is_some() {
            key = format!("{base}-{n}");
            n += 1;
        }
        key
    }

    /// The commands that write `entries` into the text file, creating it when missing.
    fn put_text(&mut self, entries: Vec<(String, String)>) -> Vec<Command> {
        let file = self.text_file();
        if self.library().is_some() {
            entries
                .into_iter()
                .map(|(key, value)| Command::Set {
                    document: file.clone(),
                    pointer: crate::text::pointer(&key),
                    value: json!(value),
                    at: None,
                })
                .collect()
        } else {
            let map: serde_json::Map<String, Value> =
                entries.into_iter().map(|(k, v)| (k, json!(v))).collect();
            vec![Command::Create {
                document: file,
                value: Value::Object(map),
            }]
        }
    }

    /// The command that drops a key's text, when the library holds it.
    fn drop_text(&mut self, key: &str) -> Option<Command> {
        self.library()?.get(key)?;
        Some(Command::Remove {
            document: self.text_file(),
            pointer: crate::text::pointer(key),
        })
    }

    /// Edits the text a field of `holder` (at `at`) names: the library entry when the field
    /// holds a key, else a new entry `name` under the step with the field set to its key.
    /// An empty optional field is removed with its text.
    fn edit_text(
        &mut self,
        label: &str,
        (step, holder, at): (&Value, &Value, &str),
        field: &str,
        name: &str,
        value: &str,
        optional: bool,
    ) {
        let held = text(holder, field).to_string();
        let keyed = self.library().is_some_and(|t| t.get(&held).is_some());
        let pointer = format!("{at}/{field}");
        let merge = Some(format!("{}{pointer}", self.document));
        let commands = if value.is_empty() && optional {
            let mut commands: Vec<Command> = self.drop_text(&held).into_iter().collect();
            if holder.get(field).is_some() {
                commands.push(Command::Remove {
                    document: self.document.to_string(),
                    pointer: pointer.clone(),
                });
            }
            commands
        } else if keyed {
            self.put_text(vec![(held, value.to_string())])
        } else {
            let key = self.key(step, name);
            let mut commands = self.put_text(vec![(key.clone(), value.to_string())]);
            commands.push(Command::Set {
                document: self.document.to_string(),
                pointer: pointer.clone(),
                value: json!(key),
                at: None,
            });
            commands
        };
        if commands.is_empty() {
            return;
        }
        self.edits.push(Edit {
            label: label.to_string(),
            context: self.context(Some(pointer)),
            commands,
            merge,
        });
    }

    fn step(&mut self, label: &str, commands: Vec<Command>) {
        if commands.is_empty() {
            return;
        }
        self.edits.push(Edit {
            label: label.to_string(),
            context: self.context(None),
            commands,
            merge: None,
        });
    }

    /// The images and sounds the pickers offer.
    fn choices(&self, folder: &str) -> Choices {
        self.index
            .iter()
            .filter(|(_, e)| e.document.starts_with(folder))
            .map(|(id, e)| (id.clone(), e.document.clone()))
            .collect()
    }

    fn thumb(&mut self, ui: &Ui, id: &str) -> Option<egui::TextureId> {
        if id.is_empty() {
            return None;
        }
        let document = self.index.get(id).map(|e| e.document.clone());
        self.shared
            .thumbs
            .get(ui.ctx(), id, document.as_deref(), self.root)
    }
}

/// The picture a step shows: its art, a story's first page.
fn step_art(step: &Value) -> &str {
    match StepKind::of(step) {
        Some(StepKind::Story) => step
            .pointer("/pages/0/art")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        _ => text(step, "art"),
    }
}

/// What a step's row says under its title.
fn subtitle(
    campaign: &Value,
    index: usize,
    step: &Value,
    maps: &[String],
    say: impl Fn(&str) -> String,
) -> String {
    match StepKind::of(step) {
        Some(StepKind::Mission) => {
            let map = text(step, "map");
            let file = if map.is_empty() {
                "no map".to_string()
            } else if maps.iter().any(|m| m == map) {
                format!("{map}.utcm")
            } else {
                format!("{map}.utcm (missing)")
            };
            format!("{file}  ·  {}", Tech::at(campaign, index).summary())
        }
        Some(StepKind::Story) => {
            let pages = step
                .get("pages")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            match pages {
                1 => "1 page".into(),
                n => format!("{n} pages"),
            }
        }
        Some(StepKind::Cinematic) => match say(text(step, "note")).as_str() {
            "" => "A placeholder card for now".into(),
            note => note.lines().next().unwrap_or_default().to_string(),
        },
        Some(StepKind::Chapter) => String::new(),
        None => format!("unknown kind {:?}", text(step, "kind")),
    }
}

/// The campaign's view; the panel on the right first.
pub fn show(ui: &mut Ui, ctx: &mut Ctx) {
    ctx.shared.thumbs.poll(ui.ctx());
    let campaign = match ctx.store.get(ctx.document) {
        Ok(Some(value)) => value.clone(),
        _ => {
            empty(ui, "This campaign does not read", ctx.document);
            return;
        }
    };
    let root = ctx.campaign_root().to_string();
    let manifest = ctx
        .store
        .get(&format!("{root}/package.json"))
        .ok()
        .flatten()
        .cloned()
        .unwrap_or_default();
    let mut state = ctx
        .shared
        .states
        .get(ctx.document)
        .cloned()
        .unwrap_or_default();
    if state.selected.is_some_and(|s| s >= steps(&campaign).len()) {
        state.selected = None;
    }
    egui::Panel::right(egui::Id::new(("campaign-panel", ctx.document)))
        .exact_size(340.0)
        .resizable(false)
        .show_separator_line(true)
        .frame(Frame::new().fill(PANEL).inner_margin(Margin::same(14)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.selected {
                    Some(i) => step_panel(ui, ctx, &campaign, i, &mut state),
                    None => settings_panel(ui, ctx, &campaign, &manifest, &mut state),
                });
        });
    egui::CentralPanel::default()
        .frame(
            Frame::new()
                .fill(MAIN)
                .inner_margin(Margin::symmetric(28, 20)),
        )
        .show(ui, |ui| {
            steps_list(ui, ctx, &campaign, &manifest, &mut state);
        });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.ctx().egui_wants_keyboard_input() {
        state.selected = None;
    }
    ctx.shared.states.insert(ctx.document.to_string(), state);
}

fn steps_list(ui: &mut Ui, ctx: &mut Ctx, campaign: &Value, manifest: &Value, state: &mut State) {
    let list = steps(campaign);
    let numbers = mission_numbers(campaign);
    // The head: the card art, the name, the race, what it holds.
    let (head, response) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::click());
    if response.clicked() {
        state.selected = None;
    }
    let card = Rect::from_min_size(head.min, vec2(84.0, 84.0));
    let t = ctx.thumb(ui, text(campaign, "card"));
    paint_cover(ui, card, t, "No card");
    let painter = ui.painter();
    let x = card.right() + 18.0;
    painter.text(
        pos2(x, head.top() + 8.0),
        Align2::LEFT_TOP,
        "CAMPAIGN",
        FontId::proportional(9.5),
        FAINT,
    );
    let name = manifest["name"]
        .as_str()
        .unwrap_or(super::id_of(ctx.campaign_root()));
    painter.text(
        pos2(x, head.top() + 24.0),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(22.0),
        TEXT,
    );
    let race = text(campaign, "race");
    let race_name = ctx
        .content
        .and_then(|c| c.rules.races.get(race))
        .map_or(race.to_string(), |r| r.name.clone());
    let count = |k: StepKind| list.iter().filter(|s| StepKind::of(s) == Some(k)).count();
    painter.text(
        pos2(x, head.top() + 58.0),
        Align2::LEFT_TOP,
        format!(
            "{race_name}  ·  {} chapters  ·  {} missions  ·  {} stories  ·  {} cinematics",
            count(StepKind::Chapter),
            count(StepKind::Mission),
            count(StepKind::Story),
            count(StepKind::Cinematic)
        ),
        FontId::proportional(12.0),
        MUTED,
    );
    ui.add_space(16.0);
    // The bar that adds steps, after the selected one (else at the end).
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let at = state.selected.map_or(list.len(), |s| s + 1);
        for kind in StepKind::ALL {
            let mission = kind == StepKind::Mission;
            let enabled = !mission || ctx.content.is_some();
            let hint = match kind {
                StepKind::Chapter => "A title card heading the steps under it",
                StepKind::Story => "Full-screen pages of art and text",
                StepKind::Mission => "A mission and its own new map in the campaign's missions/",
                StepKind::Cinematic => "A placeholder card for a cinematic to come",
            };
            if ui
                .add_enabled(enabled, egui::Button::new(format!("+ {}", kind.label())))
                .on_hover_text(hint)
                .on_disabled_hover_text("A new map needs a package that has passed the check")
                .clicked()
            {
                add_step(ctx, campaign, kind, at, state);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("Drag a row to move it · Esc: the campaign's settings")
                    .size(11.0)
                    .color(FAINT),
            );
        });
    });
    ui.add_space(10.0);
    if list.is_empty() {
        ui.add_space(30.0);
        empty(
            ui,
            "No steps yet",
            "A campaign plays its steps in order: add a chapter, a story, a mission or a cinematic.",
        );
        return;
    }
    let mut rows: Vec<Rect> = Vec::new();
    let mut drop = None;
    let mut in_chapter = false;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (i, step) in list.iter().enumerate() {
                let kind = StepKind::of(step);
                if kind == Some(StepKind::Chapter) {
                    in_chapter = true;
                    ui.add_space(if i == 0 { 0.0 } else { 10.0 });
                }
                let indent = if in_chapter && kind != Some(StepKind::Chapter) {
                    20.0
                } else {
                    0.0
                };
                let (rect, response) = row(ui, ctx, campaign, i, step, numbers[i], indent, state);
                rows.push(rect);
                if response.drag_started() {
                    state.drag = Some(i);
                }
                if response.clicked() {
                    state.selected = Some(i);
                }
                if response.drag_stopped() && state.drag == Some(i) {
                    drop = Some(i);
                }
            }
            ui.add_space(40.0);
        });
    // Dragging: a line where the row will land.
    if let Some(from) = state.drag {
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let target = pointer.map(|p| {
            rows.iter()
                .position(|r| p.y < r.center().y)
                .unwrap_or(rows.len())
        });
        if let Some(target) = target {
            let y = match rows.get(target) {
                Some(r) => r.top() - 3.0,
                None => rows.last().map_or(0.0, |r| r.bottom() + 3.0),
            };
            let x = rows.first().map_or(0.0, |r| r.left());
            let w = rows.first().map_or(0.0, |r| r.width());
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("campaign-drop"),
            ));
            painter.line_segment([pos2(x, y), pos2(x + w, y)], Stroke::new(2.0, TEXT));
            painter.circle_filled(pos2(x, y), 3.5, TEXT);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if drop.is_some() {
            if let Some(to) = target {
                let commands = super::move_step(ctx.document, campaign, from, to);
                if !commands.is_empty() {
                    let title = ctx.say(text(&list[from], "title"));
                    ctx.step(&format!("Move {title}"), commands);
                    let landed = if to > from { to - 1 } else { to };
                    state.selected = Some(landed);
                }
            }
            state.drag = None;
        } else if !ui.input(|i| i.pointer.any_down()) {
            state.drag = None;
        }
    }
}

/// One step's row; returns its rect and response (click selects, drag moves).
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut Ui,
    ctx: &mut Ctx,
    campaign: &Value,
    i: usize,
    step: &Value,
    number: Option<usize>,
    indent: f32,
    state: &mut State,
) -> (Rect, egui::Response) {
    let kind = StepKind::of(step);
    let chapter = kind == Some(StepKind::Chapter);
    let height = if chapter { 52.0 } else { 76.0 };
    let width = ui.available_width();
    let (full, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let rect = Rect::from_min_max(full.min + vec2(indent, 0.0), full.max);
    let selected = state.selected == Some(i);
    let dragged = state.drag == Some(i);
    let hovered = response.hovered();
    let painter = ui.painter();
    let alpha = if dragged { 0.45 } else { 1.0 };
    if chapter {
        if selected || hovered {
            painter.rect_filled(rect, 8, if selected { SELECTED } else { RAISED });
        }
        painter.line_segment(
            [
                rect.left_bottom() + vec2(10.0, -1.0),
                rect.right_bottom() - vec2(10.0, 1.0),
            ],
            Stroke::new(1.0, BORDER),
        );
    } else {
        painter.rect(
            rect,
            8,
            if selected { SELECTED } else { RAISED }.gamma_multiply(alpha),
            Stroke::new(
                1.0,
                if selected {
                    SOFT
                } else if hovered {
                    CONTROL
                } else {
                    BORDER
                },
            ),
            egui::StrokeKind::Inside,
        );
    }
    // The handle.
    let handle = rect.left() + 12.0;
    for k in 0..3 {
        for c in 0..2 {
            painter.circle_filled(
                pos2(
                    handle + c as f32 * 5.0,
                    rect.center().y - 5.0 + k as f32 * 5.0,
                ),
                1.3,
                if hovered { MUTED } else { STRONG },
            );
        }
    }
    let mut x = rect.left() + 30.0;
    if chapter {
        let art = step_art(step).to_string();
        if !art.is_empty() {
            let thumb = Rect::from_min_size(pos2(x, rect.top() + 8.0), vec2(64.0, 36.0));
            let t = ctx.thumb(ui, &art);
            paint_cover(ui, thumb, t, "");
            x = thumb.right() + 14.0;
        }
        let painter = ui.painter();
        painter.text(
            pos2(x, rect.top() + 8.0),
            Align2::LEFT_TOP,
            "CHAPTER",
            FontId::proportional(9.5),
            DIRTY.gamma_multiply(0.8),
        );
        painter.text(
            pos2(x, rect.top() + 22.0),
            Align2::LEFT_TOP,
            match ctx.say(text(step, "title")).as_str() {
                "" => "Untitled chapter".to_string(),
                t => t.to_string(),
            },
            FontId::proportional(17.0),
            TEXT,
        );
    } else {
        let thumb = Rect::from_min_size(pos2(x, rect.top() + 8.0), vec2(107.0, 60.0));
        let art = step_art(step).to_string();
        let t = ctx.thumb(ui, &art);
        let empty = match kind {
            Some(StepKind::Cinematic) => "cinematic",
            _ => "no art",
        };
        paint_cover(ui, thumb, t, empty);
        if kind == Some(StepKind::Cinematic) && t.is_none() {
            icons::paint(
                ui.painter(),
                icons::square(thumb.center() - vec2(0.0, 8.0), 18.0),
                Icon::Effects,
                FAINT,
            );
        }
        x = thumb.right() + 16.0;
        let label = match (kind, number) {
            (Some(StepKind::Mission), Some(n)) => format!("MISSION {n}"),
            (Some(k), _) => k.label().to_uppercase(),
            (None, _) => "STEP".into(),
        };
        let color = match kind {
            Some(StepKind::Mission) => super::tech::NEW,
            Some(StepKind::Story) => Color32::from_rgb(0x93, 0xc5, 0xfd),
            _ => Color32::from_rgb(0xc4, 0xb5, 0xfd),
        };
        let painter = ui.painter();
        painter.text(
            pos2(x, rect.top() + 11.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(9.5),
            color.gamma_multiply(0.9),
        );
        let title = match ctx.say(text(step, "title")).as_str() {
            "" => "Untitled".to_string(),
            t => t.to_string(),
        };
        let library = ctx.library();
        let right = rect.right() - 170.0;
        let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right, rect.bottom()));
        let painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
        painter.text(
            pos2(x, rect.top() + 25.0),
            Align2::LEFT_TOP,
            title,
            FontId::proportional(15.0),
            TEXT,
        );
        painter.text(
            pos2(x, rect.top() + 48.0),
            Align2::LEFT_TOP,
            subtitle(campaign, i, step, &ctx.maps, |v| {
                library
                    .as_ref()
                    .and_then(|t| t.get(v).and_then(Value::as_str))
                    .unwrap_or(v)
                    .to_string()
            }),
            FontId::proportional(11.5),
            MUTED,
        );
    }
    // Its actions, on the right.
    let mut right = rect.right() - 10.0;
    let button = |ui: &mut Ui, right: &mut f32, label: &str, hint: &str| {
        let w = 14.0 + label.len() as f32 * 7.0;
        let r = Rect::from_min_size(pos2(*right - w, rect.center().y - 13.0), vec2(w, 26.0));
        *right = r.left() - 6.0;
        ui.place(r, egui::Button::new(RichText::new(label).size(12.0)))
            .on_hover_text(hint)
            .clicked()
    };
    let delete = {
        let r = Rect::from_min_size(pos2(right - 26.0, rect.center().y - 13.0), vec2(26.0, 26.0));
        right = r.left() - 6.0;
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(r));
        icon_button(&mut ui, Icon::Close, false, true)
            .on_hover_text("Remove this step (a mission's map stays in missions/)")
            .clicked()
    };
    if kind == Some(StepKind::Mission)
        && button(ui, &mut right, "Edit", "Open the map in the Maps editor")
    {
        let map = text(step, "map");
        if !map.is_empty() {
            ctx.shared.requests.push(Request::Map {
                root: super::map_root(ctx.campaign_root(), map),
                tech: false,
            });
        }
    }
    let play_hint = match kind {
        Some(StepKind::Mission) => "Play this mission",
        _ => "Play the campaign from this step",
    };
    if button(ui, &mut right, "Play", play_hint) {
        ctx.shared.requests.push(Request::Play {
            document: ctx.document.to_string(),
            step: i,
        });
    }
    if delete {
        let title = ctx.say(text(step, "title"));
        ctx.step(
            &format!("Remove {title}"),
            vec![Command::Remove {
                document: ctx.document.to_string(),
                pointer: format!("/steps/{i}"),
            }],
        );
        state.selected = None;
    }
    let response = response.on_hover_cursor(egui::CursorIcon::Grab);
    (full, response)
}

/// Adds a step of `kind` at `at`; a mission gets a new map in the campaign's missions/.
fn add_step(ctx: &mut Ctx, campaign: &Value, kind: StepKind, at: usize, state: &mut State) {
    let missions = steps(campaign)
        .iter()
        .filter(|s| StepKind::of(s) == Some(StepKind::Mission))
        .count();
    let title = match kind {
        StepKind::Chapter => "New chapter".to_string(),
        StepKind::Story => "New story".to_string(),
        StepKind::Mission => format!("Mission {}", missions + 1),
        StepKind::Cinematic => "New cinematic".to_string(),
    };
    let id = super::unique_step_id(campaign, &title);
    let mut commands = Vec::new();
    let mut map = None;
    if kind == StepKind::Mission {
        let Some(content) = ctx.content else { return };
        let mut map_id = id.clone();
        let mut n = 2;
        while ctx.maps.contains(&map_id) {
            map_id = format!("{id}-{n}");
            n += 1;
        }
        let root = super::map_root(ctx.campaign_root(), &map_id);
        match crate::map::new_map_at(
            content,
            &ctx.version,
            &root,
            &map_id,
            &title,
            512,
            super::mission_biome(ctx.root),
        ) {
            Ok(map_commands) => commands.extend(map_commands),
            Err(error) => {
                tracing::warn!(%error, "editor: new mission map");
                return;
            }
        }
        map = Some(map_id);
    }
    let mut step = super::new_step(kind, &id, &title, map.as_deref());
    let mut entries = Vec::new();
    let key = ctx.key(&step, "title");
    entries.push((key.clone(), title.clone()));
    step["title"] = json!(key);
    if let Some(page) = step.pointer_mut("/pages/0/text") {
        let key = ctx.key(&json!({ "id": id }), "page-1");
        entries.push((key.clone(), page.as_str().unwrap_or_default().to_string()));
        *page = json!(key);
    }
    commands.extend(ctx.put_text(entries));
    commands.push(super::insert_step(ctx.document, campaign, at, step));
    ctx.step(&format!("Add {}", kind.label().to_lowercase()), commands);
    state.selected = Some(at.min(steps(campaign).len()));
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(eyebrow(title));
    ui.add_space(4.0);
}

fn field(ui: &mut Ui, name: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(name).size(11.0).color(MUTED));
}

/// A single-line text field; the new text when typed.
fn line(ui: &mut Ui, value: &str) -> Option<String> {
    let mut text = value.to_string();
    ui.add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY))
        .changed()
        .then_some(text)
}

fn multiline(ui: &mut Ui, value: &str, rows: usize) -> Option<String> {
    let mut text = value.to_string();
    ui.add(
        egui::TextEdit::multiline(&mut text)
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    )
    .changed()
    .then_some(text)
}

/// A sound field: a menu of the package's sounds and None.
fn sound(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    choices: &Choices,
) -> Option<String> {
    let mut chosen = None;
    let shown = if current.is_empty() {
        "None".to_string()
    } else {
        current.rsplit('.').next().unwrap_or(current).to_string()
    };
    egui::ComboBox::from_id_salt(salt)
        .selected_text(shown)
        .width(ui.available_width() - 8.0)
        .height(320.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_empty(), "None").clicked() {
                chosen = Some(String::new());
            }
            for id in choices.keys() {
                if ui.selectable_label(id == current, id).clicked() {
                    chosen = Some(id.clone());
                }
            }
        });
    chosen.filter(|c| c != current)
}

/// With nothing selected: the campaign's name, race, background and card art.
fn settings_panel(
    ui: &mut Ui,
    ctx: &mut Ctx,
    campaign: &Value,
    manifest: &Value,
    state: &mut State,
) {
    let root = ctx.campaign_root().to_string();
    ui.label(RichText::new("Campaign").size(15.0).color(TEXT).strong());
    ui.label(RichText::new(&root).size(10.5).color(FAINT).monospace());
    section(ui, "Meta");
    field(ui, "Name");
    let package = format!("{root}/package.json");
    let set_package = |ctx: &mut Ctx, label: &str, pointer: &str, value: String| {
        ctx.edits.push(Edit {
            label: label.into(),
            context: ctx.context(None),
            commands: vec![Command::Set {
                document: package.clone(),
                pointer: pointer.into(),
                value: json!(value),
                at: None,
            }],
            merge: Some(format!("{package}{pointer}")),
        });
    };
    if let Some(name) = line(ui, manifest["name"].as_str().unwrap_or_default()) {
        set_package(ctx, "Campaign name", "/name", name);
    }
    field(ui, "Description");
    if let Some(d) = multiline(ui, manifest["description"].as_str().unwrap_or_default(), 3) {
        set_package(ctx, "Campaign description", "/description", d);
    }
    field(ui, "Race the player plays");
    let race = text(campaign, "race").to_string();
    let races: Vec<(String, String)> = ctx
        .content
        .map(|c| {
            c.rules
                .races
                .iter()
                .map(|(id, r)| (id.clone(), r.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let shown = races
        .iter()
        .find(|(id, _)| *id == race)
        .map_or(race.clone(), |(_, n)| n.clone());
    let mut chosen = None;
    egui::ComboBox::from_id_salt(("campaign-race", ctx.document))
        .selected_text(shown)
        .width(ui.available_width() - 8.0)
        .show_ui(ui, |ui| {
            for (id, name) in &races {
                if ui.selectable_label(*id == race, name).clicked() {
                    chosen = Some(id.clone());
                }
            }
        });
    if let Some(race) = chosen.filter(|r| *r != race) {
        ctx.set("Campaign race", "/race", json!(race), false);
    }
    let images = ctx.choices("assets/interface/");
    section(ui, "Art");
    field(ui, "Background of the campaign screen");
    let background = text(campaign, "background").to_string();
    if let Some(v) = picker(
        ui,
        &format!("{}/background", ctx.document),
        &background,
        &images,
        &mut ctx.shared.thumbs,
        ctx.root,
        16.0 / 9.0,
        &mut state.search,
    ) {
        ctx.set_optional("Campaign background", campaign, "", "background", &v);
    }
    field(ui, "Card for choosing the campaign");
    let card = text(campaign, "card").to_string();
    if let Some(v) = picker(
        ui,
        &format!("{}/card", ctx.document),
        &card,
        &images,
        &mut ctx.shared.thumbs,
        ctx.root,
        1.6,
        &mut state.search,
    ) {
        ctx.set_optional("Campaign card", campaign, "", "card", &v);
    }
    section(ui, "Steps");
    ui.label(
        RichText::new(
            "Select a step to edit it; the order of the list is the order the campaign plays.",
        )
        .size(11.0)
        .color(FAINT),
    );
}

/// A selected step's fields.
fn step_panel(ui: &mut Ui, ctx: &mut Ctx, campaign: &Value, i: usize, state: &mut State) {
    let step = steps(campaign)[i].clone();
    let kind = StepKind::of(&step);
    let at = format!("/steps/{i}");
    let number = mission_numbers(campaign)[i];
    ui.horizontal(|ui| {
        if icon_button(ui, Icon::ChevronLeft, false, true)
            .on_hover_text("The campaign's settings (Esc)")
            .clicked()
        {
            state.selected = None;
        }
        let title = match (kind, number) {
            (Some(StepKind::Mission), Some(n)) => format!("Mission {n}"),
            (Some(k), _) => k.label().to_string(),
            (None, _) => "Step".into(),
        };
        ui.label(RichText::new(title).size(15.0).color(TEXT).strong());
        ui.label(
            RichText::new(format!("step {}", i + 1))
                .size(11.0)
                .color(FAINT),
        );
    });
    section(ui, "Step");
    field(ui, "Id (progress and Play from here name it)");
    let id = text(&step, "id").to_string();
    if let Some(v) = line(ui, &id) {
        ctx.set("Step id", &format!("{at}/id"), json!(super::slug(&v)), true);
    }
    let duplicate = steps(campaign)
        .iter()
        .enumerate()
        .any(|(j, s)| j != i && text(s, "id") == id);
    if duplicate {
        ui.label(
            RichText::new("Another step has this id")
                .size(11.0)
                .color(ERROR),
        );
    }
    field(ui, "Title");
    let shown = ctx.say(text(&step, "title"));
    if let Some(v) = line(ui, &shown) {
        ctx.edit_text(
            "Step title",
            (&step, &step, &at),
            "title",
            "title",
            &v,
            false,
        );
    }
    let images = ctx.choices("assets/interface/");
    let sounds = ctx.choices("assets/audio/");
    let art = |ui: &mut Ui,
               ctx: &mut Ctx,
               state: &mut State,
               label: &str,
               field_name: &str,
               aspect: f32| {
        field(ui, label);
        let current = text(&step, field_name).to_string();
        if let Some(v) = picker(
            ui,
            &format!("{}{at}/{field_name}", ctx.document),
            &current,
            &images,
            &mut ctx.shared.thumbs,
            ctx.root,
            aspect,
            &mut state.search,
        ) {
            ctx.set_optional("Step art", &step, &at, field_name, &v);
        }
    };
    match kind {
        Some(StepKind::Chapter) => {
            art(ui, ctx, state, "Title card art", "art", 16.0 / 9.0);
        }
        Some(StepKind::Story) => {
            section(ui, "Pages");
            let pages = step
                .get("pages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for (p, page) in pages.iter().enumerate() {
                let pointer = format!("{at}/pages/{p}");
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(8)
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Page {}", p + 1)).color(SOFT));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if icon_button(ui, Icon::Close, false, pages.len() > 1)
                                        .on_hover_text("Remove this page")
                                        .clicked()
                                    {
                                        let mut commands: Vec<Command> =
                                            ctx.drop_text(text(page, "text")).into_iter().collect();
                                        commands.push(Command::Remove {
                                            document: ctx.document.to_string(),
                                            pointer: pointer.clone(),
                                        });
                                        ctx.step("Remove page", commands);
                                    }
                                },
                            );
                        });
                        let current = text(page, "art").to_string();
                        if let Some(v) = picker(
                            ui,
                            &format!("{}{pointer}/art", ctx.document),
                            &current,
                            &images,
                            &mut ctx.shared.thumbs,
                            ctx.root,
                            16.0 / 9.0,
                            &mut state.search,
                        ) {
                            ctx.set_optional("Page art", page, &pointer, "art", &v);
                        }
                        field(ui, "Text");
                        let shown = ctx.say(text(page, "text"));
                        if let Some(v) = multiline(ui, &shown, 4) {
                            ctx.edit_text(
                                "Page text",
                                (&step, page, &pointer),
                                "text",
                                &format!("page-{}", p + 1),
                                &v,
                                false,
                            );
                        }
                        if shown.trim().is_empty() {
                            ui.label(
                                RichText::new("A page needs some text")
                                    .size(11.0)
                                    .color(ERROR),
                            );
                        }
                        field(ui, "Narration");
                        if let Some(v) = sound(
                            ui,
                            ("narration", ctx.document, i, p),
                            text(page, "narration"),
                            &sounds,
                        ) {
                            ctx.set_optional("Page narration", page, &pointer, "narration", &v);
                        }
                    });
                ui.add_space(6.0);
            }
            if ui.button("+ Page").clicked() {
                let key = ctx.key(&step, &format!("page-{}", pages.len() + 1));
                let mut commands = ctx.put_text(vec![(key.clone(), "A new page.".into())]);
                commands.push(if step.get("pages").is_some_and(Value::is_array) {
                    Command::Insert {
                        document: ctx.document.to_string(),
                        pointer: format!("{at}/pages"),
                        index: pages.len(),
                        value: json!({ "text": key }),
                    }
                } else {
                    Command::Set {
                        document: ctx.document.to_string(),
                        pointer: format!("{at}/pages"),
                        value: json!([{ "text": key }]),
                        at: None,
                    }
                });
                ctx.step("Add page", commands);
            }
        }
        Some(StepKind::Mission) => {
            section(ui, "Map");
            let map = text(&step, "map").to_string();
            let mut chosen = None;
            egui::ComboBox::from_id_salt(("mission-map", ctx.document, i))
                .selected_text(if map.is_empty() {
                    "None".to_string()
                } else {
                    map.clone()
                })
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for m in &ctx.maps {
                        if ui.selectable_label(*m == map, m).clicked() {
                            chosen = Some(m.clone());
                        }
                    }
                });
            if let Some(m) = chosen.filter(|m| *m != map) {
                ctx.set("Mission map", &format!("{at}/map"), json!(m), false);
            }
            let exists = ctx.maps.contains(&map);
            if !exists {
                ui.label(
                    RichText::new(format!("No map {map} in the campaign's missions/"))
                        .size(11.0)
                        .color(ERROR),
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(exists, egui::Button::new("Edit map"))
                    .on_hover_text("Open the map in the Maps editor")
                    .clicked()
                {
                    ctx.shared.requests.push(Request::Map {
                        root: super::map_root(ctx.campaign_root(), &map),
                        tech: false,
                    });
                }
                if ui
                    .button("Play")
                    .on_hover_text("Play this mission")
                    .clicked()
                {
                    ctx.shared.requests.push(Request::Play {
                        document: ctx.document.to_string(),
                        step: i,
                    });
                }
            });
            section(ui, "Loading screen");
            field(ui, "Chapter line");
            let shown = ctx.say(text(&step, "chapterLine"));
            if let Some(v) = line(ui, &shown) {
                let place = (&step, &step, at.as_str());
                ctx.edit_text(
                    "Chapter line",
                    place,
                    "chapterLine",
                    "chapter-line",
                    &v,
                    true,
                );
            }
            art(ui, ctx, state, "Illustration", "art", 16.0 / 9.0);
            field(ui, "Story");
            let shown = ctx.say(text(&step, "story"));
            if let Some(v) = multiline(ui, &shown, 6) {
                let place = (&step, &step, at.as_str());
                ctx.edit_text("Mission story", place, "story", "story", &v, true);
            }
            section(ui, "Heroes");
            let cap = step.get("heroLevelCap").and_then(Value::as_u64);
            let mut capped = cap.is_some();
            let mut level = cap.unwrap_or(3);
            ui.horizontal(|ui| {
                let toggled = ui
                    .checkbox(&mut capped, "Hero level cap")
                    .on_hover_text("Carried heroes go no higher than this in the mission")
                    .changed();
                let dragged = capped
                    && ui
                        .add(egui::DragValue::new(&mut level).range(1..=10))
                        .changed();
                if toggled && !capped {
                    ctx.set_optional("Hero level cap", &step, &at, "heroLevelCap", "");
                } else if toggled || dragged {
                    ctx.set(
                        "Hero level cap",
                        &format!("{at}/heroLevelCap"),
                        json!(level),
                        dragged,
                    );
                }
            });
            section(ui, "Tech");
            tech_summary(ui, ctx, campaign, i);
            if ui
                .add_enabled(exists, egui::Button::new("Open tech graph"))
                .on_hover_text("The race's whole tree in this mission, in the map's Tech view (U)")
                .clicked()
            {
                ctx.shared.requests.push(Request::Map {
                    root: super::map_root(ctx.campaign_root(), &map),
                    tech: true,
                });
            }
        }
        Some(StepKind::Cinematic) => {
            field(ui, "What it will show");
            let shown = ctx.say(text(&step, "note"));
            if let Some(v) = multiline(ui, &shown, 5) {
                let place = (&step, &step, at.as_str());
                ctx.edit_text("Cinematic note", place, "note", "note", &v, true);
            }
            ui.label(
                RichText::new("For now the game shows a placeholder card; the scene and the cinematic player come later.")
                    .size(11.0)
                    .color(FAINT),
            );
        }
        None => {
            ui.label(RichText::new("This step's kind is unknown; edit it as JSON.").color(ERROR));
        }
    }
    section(ui, "Music");
    if let Some(v) = sound(
        ui,
        ("music", ctx.document, i),
        text(&step, "music"),
        &sounds,
    ) {
        ctx.set_optional("Step music", &step, &at, "music", &v);
    }
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if ui
            .button("Play from here")
            .on_hover_text("Run the game from this step")
            .clicked()
        {
            ctx.shared.requests.push(Request::Play {
                document: ctx.document.to_string(),
                step: i,
            });
        }
        if ui
            .button("Remove step")
            .on_hover_text("A mission's map stays in missions/")
            .clicked()
        {
            ctx.step(
                "Remove step",
                vec![Command::Remove {
                    document: ctx.document.to_string(),
                    pointer: at.clone(),
                }],
            );
            state.selected = None;
        }
    });
}

/// A mission's tech in words: what is new and what is blocked, by name.
fn tech_summary(ui: &mut Ui, ctx: &Ctx, campaign: &Value, i: usize) {
    let tech = Tech::at(campaign, i);
    let name = |id: &str| -> String {
        ctx.content
            .and_then(|c| {
                c.definition(id)
                    .map(|d| d.name.clone())
                    .or_else(|| c.rules.research.get(id).map(|r| r.name.clone()))
            })
            .unwrap_or_else(|| id.to_string())
    };
    ui.label(RichText::new(tech.summary()).color(SOFT));
    for (list, label, color) in [
        (&tech.new, "New", super::tech::NEW),
        (&tech.blocked, "Blocked", super::tech::BLOCKED),
    ] {
        if list.is_empty() {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(label).size(11.0).color(color));
            for id in list {
                badge(ui, &name(id), Some(color));
            }
        });
    }
    ui.label(
        RichText::new(format!(
            "{} unlocked by earlier missions",
            tech.before.len()
        ))
        .size(11.0)
        .color(FAINT),
    );
}
