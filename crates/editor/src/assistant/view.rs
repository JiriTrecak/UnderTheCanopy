//! The assistant's sidebar: the thread (what the user said, what Claude wrote, each tool
//! call with its answer), the turn streaming in, a call waiting for approval, the thread
//! list, the settings, and the composer.

use super::{Assistant, Env, Key, api};
use crate::icons::Icon;
use crate::theme::*;
use crate::widgets::{eyebrow, icon_button, primary};
use egui::{
    Align, FontId, Frame, Key as Keys, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit, Ui,
    text::LayoutJob,
};
use serde_json::Value;

/// What the user asked of the editor from the sidebar.
pub enum Action {
    Close,
}

impl Assistant {
    /// Draws the sidebar's content into `ui` (the panel is the shell's).
    pub fn show(&mut self, ui: &mut Ui, env: &Env) -> Option<Action> {
        if self.poll() || self.busy() {
            ui.ctx().request_repaint();
        }
        let mut action = None;
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
                        ui.add_space(11.0);
                        ui.label(RichText::new("Assistant").size(13.0).color(TEXT).strong());
                        let title = if self.thread.title.is_empty() {
                            "New thread"
                        } else {
                            self.thread.title.as_str()
                        };
                        ui.add(
                            egui::Label::new(RichText::new(title).size(10.0).color(MUTED))
                                .truncate(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, false, true)
                            .on_hover_text("Close")
                            .clicked()
                        {
                            action = Some(Action::Close);
                        }
                        if icon_button(ui, Icon::Plus, false, !self.busy())
                            .on_hover_text("New thread")
                            .clicked()
                        {
                            self.new_thread();
                            self.settings_open = false;
                        }
                        if ui
                            .selectable_label(self.settings_open, "Settings")
                            .on_hover_text("Model, effort and the API key")
                            .clicked()
                        {
                            self.settings_open = !self.settings_open;
                            self.list_open = false;
                        }
                        if ui
                            .selectable_label(self.list_open, "Threads")
                            .on_hover_text("Earlier threads")
                            .clicked()
                        {
                            self.list_open = !self.list_open;
                            self.settings_open = false;
                        }
                    });
                });
            });
        egui::Panel::bottom("assistant-composer")
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().inner_margin(Margin::same(12)))
            .show(ui, |ui| self.composer(ui, env));
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if self.settings_open {
                self.settings(ui);
            } else if self.list_open {
                self.list(ui);
            } else {
                self.conversation(ui);
            }
        });
        action
    }

    fn composer(&mut self, ui: &mut Ui, env: &Env) {
        if let Some(approval) = self.running.as_ref().and_then(|r| r.approval.as_ref()) {
            let what = describe_call(&approval.name, &approval.input);
            let mut answer = None;
            Frame::new()
                .fill(RAISED)
                .stroke(Stroke::new(1.0, DIRTY))
                .corner_radius(8)
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.label(RichText::new("Allow this?").color(TEXT).strong());
                    ui.label(RichText::new(what).color(SOFT));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary("Allow")).clicked() {
                            answer = Some(true);
                        }
                        if ui.button("Decline").clicked() {
                            answer = Some(false);
                        }
                    });
                });
            if let Some(allow) = answer {
                self.approve(allow);
            }
            ui.add_space(8.0);
        }
        let busy = self.busy();
        Frame::new()
            .fill(RAISED)
            .stroke(Stroke::new(1.0, STRONG))
            .corner_radius(10)
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                let hint = match self.key {
                    Key::Missing => "Add an API key in Settings to begin",
                    _ => "Ask about anything in the open package…",
                };
                let edit = ui.add(
                    TextEdit::multiline(&mut self.composer)
                        .hint_text(hint)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .frame(Frame::NONE),
                );
                // Enter sends; Shift+Enter starts a new line.
                let enter = edit.has_focus()
                    && ui.input(|i| i.key_pressed(Keys::Enter) && !i.modifiers.shift);
                if enter {
                    // The key press already added a line break.
                    let trimmed = self.composer.trim_end_matches('\n').to_string();
                    self.composer = trimmed;
                    self.send(env);
                }
                ui.horizontal(|ui| {
                    let model = api::MODELS
                        .iter()
                        .find(|m| m.0 == self.settings.model)
                        .map_or("Claude", |m| m.1);
                    ui.label(
                        RichText::new(format!("{model} · {} effort", self.settings.effort))
                            .color(FAINT)
                            .size(11.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if busy {
                            if ui
                                .button("Stop")
                                .on_hover_text("Stop the assistant")
                                .clicked()
                            {
                                self.stop();
                            }
                        } else if icon_button(
                            ui,
                            Icon::Send,
                            false,
                            !self.composer.trim().is_empty(),
                        )
                        .on_hover_text("Send (Enter)")
                        .clicked()
                        {
                            self.send(env);
                        }
                    });
                });
            });
    }

    fn conversation(&mut self, ui: &mut Ui) {
        let follow = std::mem::take(&mut self.follow);
        let width = ui.available_width();
        ScrollArea::vertical()
            .id_salt("assistant-thread")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.set_width(width);
                ui.add_space(12.0);
                Frame::new()
                    .inner_margin(Margin::symmetric(14, 0))
                    .show(ui, |ui| {
                        if self.thread.messages.is_empty() && self.running.is_none() {
                            intro(ui);
                        }
                        let messages = self.thread.messages.clone();
                        for (i, message) in messages.iter().enumerate() {
                            let next = messages.get(i + 1);
                            match message["role"].as_str() {
                                Some("user") => user_message(ui, message),
                                Some("assistant") => assistant_message(ui, message, next),
                                _ => {}
                            }
                        }
                        if let Some(running) = &self.running {
                            if !running.note.trim().is_empty() {
                                note(ui, running.note.trim());
                            }
                            if !running.text.is_empty() {
                                markdown(ui, &running.text);
                            }
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(12.0).color(MUTED));
                                let doing = match &running.tool {
                                    Some(tool) => format!("Calling {tool}…"),
                                    None if running.approval.is_some() => "Waiting for you…".into(),
                                    None => "Thinking…".into(),
                                };
                                ui.label(RichText::new(doing).color(MUTED).size(11.0));
                            });
                        }
                        if let Some(error) = &self.error {
                            ui.add_space(6.0);
                            ui.label(RichText::new(error).color(ERROR).size(12.0));
                        }
                        if self.thread.usage.iter().any(|n| *n > 0) {
                            ui.add_space(8.0);
                            let tokens: u64 = self.thread.usage.iter().sum();
                            ui.label(
                                RichText::new(format!(
                                    "{} tokens · about ${:.2}",
                                    thousands(tokens),
                                    self.thread.cost
                                ))
                                .color(FAINT)
                                .size(10.0),
                            );
                        }
                        ui.add_space(12.0);
                    });
                if follow {
                    ui.scroll_to_cursor(Some(Align::Max));
                }
            });
    }

    fn list(&mut self, ui: &mut Ui) {
        ScrollArea::vertical()
            .id_salt("assistant-threads")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        ui.label(eyebrow("Threads"));
                        ui.add_space(4.0);
                        if self.threads.is_empty() {
                            ui.label(RichText::new("No threads yet").color(FAINT));
                        }
                        let mut open = None;
                        let mut delete = None;
                        for head in &self.threads {
                            ui.horizontal(|ui| {
                                let current = head.id == self.thread.id;
                                if ui
                                    .selectable_label(current, &head.title)
                                    .on_hover_text("Open this thread")
                                    .clicked()
                                {
                                    open = Some(head.id.clone());
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if icon_button(ui, Icon::Close, false, !self.busy())
                                        .on_hover_text("Delete this thread")
                                        .clicked()
                                    {
                                        delete = Some(head.id.clone());
                                    }
                                });
                            });
                        }
                        if let Some(id) = open {
                            self.open_thread(&id);
                        }
                        if let Some(id) = delete {
                            self.delete_thread(&id);
                        }
                    });
            });
    }

    fn settings(&mut self, ui: &mut Ui) {
        ScrollArea::vertical()
            .id_salt("assistant-settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        ui.label(eyebrow("Model"));
                        let mut changed = false;
                        for (id, label, rates) in api::MODELS {
                            let text = format!("{label}  ·  ${} / ${} per million tokens in / out", rates[0], rates[1]);
                            if ui.radio(self.settings.model == id, text).clicked() {
                                self.settings.model = id.to_string();
                                changed = true;
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(eyebrow("Effort"));
                        ui.horizontal(|ui| {
                            for effort in api::EFFORTS {
                                if ui.selectable_label(self.settings.effort == effort, effort).clicked() {
                                    self.settings.effort = effort.to_string();
                                    changed = true;
                                }
                            }
                        });
                        ui.label(
                            RichText::new("Higher effort thinks longer and costs more; high suits most editing.")
                                .color(FAINT)
                                .size(11.0),
                        );
                        if changed {
                            self.store_settings();
                        }
                        ui.add_space(12.0);
                        ui.label(eyebrow("API key"));
                        match self.key.clone() {
                            Key::Environment(_) => {
                                ui.label(RichText::new("From ANTHROPIC_API_KEY in the environment.").color(SOFT));
                            }
                            Key::Stored(_) => {
                                ui.label(RichText::new("Kept in your user folder, readable by you only.").color(SOFT));
                                if ui.button("Remove the key").clicked() {
                                    self.forget_key();
                                }
                            }
                            Key::Missing => {
                                ui.label(
                                    RichText::new("The assistant calls the Claude API with your key (a Claude Code subscription does not cover it). It stays in your user folder and is sent only to the API.")
                                        .color(SOFT),
                                );
                                ui.add(
                                    TextEdit::singleline(&mut self.key_entry)
                                        .password(true)
                                        .hint_text("sk-ant-…")
                                        .desired_width(f32::INFINITY),
                                );
                                let entered = !self.key_entry.trim().is_empty();
                                if ui.add_enabled(entered, primary("Keep the key")).clicked() {
                                    let key = std::mem::take(&mut self.key_entry);
                                    if let Err(error) = self.store_key(&key) {
                                        self.error = Some(format!("Cannot keep the key: {error}"));
                                    } else {
                                        self.error = None;
                                        self.settings_open = false;
                                    }
                                }
                            }
                        }
                        ui.add_space(12.0);
                        ui.label(eyebrow("Tools and skills"));
                        ui.label(
                            RichText::new(format!(
                                "{} tools (the editor's MCP tools) and {} skills from the repository. Saving, cooking, imports and generating sounds ask you first; everything else is an undo step.",
                                super::agent::tools().len(),
                                self.skill_count()
                            ))
                            .color(SOFT),
                        );
                    });
            });
    }
}

fn intro(ui: &mut Ui) {
    ui.add_space(24.0);
    ui.label(
        RichText::new("Ask, and it works in the editor")
            .color(TEXT)
            .size(14.0)
            .strong(),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("It reads and edits the package through the editor's own commands, so every change is an undo step and shows at once. It sees what you have open.")
            .color(MUTED),
    );
    ui.add_space(8.0);
    for example in [
        "Make the Warrior 10% tougher and tell me what it changes",
        "Why does the check fail?",
        "Add a medium creep camp to the middle of this map",
        "Run the tower lab and compare the approaches",
    ] {
        ui.label(
            RichText::new(format!("“{example}”"))
                .color(FAINT)
                .size(12.0),
        );
    }
}

fn user_message(ui: &mut Ui, message: &Value) {
    // The user's own words; tool results ride in user messages too and show with their
    // calls instead.
    let texts: Vec<&str> = match &message["content"] {
        Value::String(s) => vec![s.as_str()],
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .filter(|t| !t.starts_with("<editor-context>"))
            .collect(),
        _ => Vec::new(),
    };
    if texts.is_empty() {
        return;
    }
    ui.add_space(10.0);
    Frame::new()
        .fill(BUBBLE)
        .corner_radius(10)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for text in texts {
                ui.label(RichText::new(text).color(TEXT));
            }
        });
    ui.add_space(6.0);
}

fn assistant_message(ui: &mut Ui, message: &Value, next: Option<&Value>) {
    let empty = Vec::new();
    let blocks = message["content"].as_array().unwrap_or(&empty);
    for block in blocks {
        match block["type"].as_str() {
            Some("text") => markdown(ui, block["text"].as_str().unwrap_or_default()),
            Some("thinking") => {
                let text = block["thinking"].as_str().unwrap_or_default().trim();
                if !text.is_empty() {
                    note(ui, text);
                }
            }
            Some("tool_use") => tool_row(ui, block, next),
            _ => {}
        }
    }
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(FAINT).italics().size(11.0));
}

/// A tool call as one line (what it did, and whether it worked), opening to its input
/// and answer.
fn tool_row(ui: &mut Ui, block: &Value, next: Option<&Value>) {
    let id = block["id"].as_str().unwrap_or_default();
    let name = block["name"].as_str().unwrap_or_default();
    let input = &block["input"];
    let result = next
        .and_then(|m| m["content"].as_array())
        .and_then(|blocks| blocks.iter().find(|b| b["tool_use_id"] == id));
    // A dot in the call's colour: done, failed or not answered (the font has no ticks).
    let (colour, text) = match result {
        Some(r) if r["is_error"] == true => (ERROR, SOFT),
        Some(_) => (LIVE, MUTED),
        None => (FAINT, FAINT),
    };
    let mut header = LayoutJob::default();
    header.append(
        "●  ",
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(9.0),
            color: colour,
            valign: Align::Center,
            ..Default::default()
        },
    );
    header.append(
        &describe_call(name, input),
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(11.5),
            color: text,
            ..Default::default()
        },
    );
    egui::CollapsingHeader::new(header)
        .id_salt(("tool", id))
        .default_open(false)
        .show(ui, |ui| {
            let pretty = serde_json::to_string_pretty(input).unwrap_or_default();
            code(ui, &cut(&pretty, 1500));
            if let Some(result) = result {
                let text = match &result["content"] {
                    Value::String(s) => s.clone(),
                    Value::Array(parts) if parts.iter().any(|p| p["type"] == "image") => {
                        "(a picture)".into()
                    }
                    other => other.to_string(),
                };
                code(ui, &cut(&text, 3000));
            }
        });
}

/// A call in words, for its row and its approval.
fn describe_call(name: &str, input: &Value) -> String {
    let s = |k: &str| input[k].as_str().unwrap_or_default().to_string();
    match name {
        "edit" => {
            let n = input["commands"].as_array().map_or(0, Vec::len);
            format!(
                "Edit: {} ({n} change{})",
                s("label"),
                if n == 1 { "" } else { "s" }
            )
        }
        "read_document" => format!("Read {}", s("document")),
        "document_schema" => format!("Schema of {}", s("document")),
        "list_elements" => match input["editor"].as_str() {
            Some(editor) => format!("List the {editor}"),
            None => "List every element".into(),
        },
        "save" => "Save every changed document to the package's files".into(),
        "cook" => "Cook the changed assets into the pack".into(),
        "import_asset" => format!("Import {} as {}", s("source"), s("id")),
        "import_starcraft" => format!("Import the StarCraft II map {} as {}", s("source"), s("id")),
        "map_preview" => format!("Write the map preview of {}", s("map")),
        "generate_sound" => "Generate sounds with ElevenLabs (costs credits)".into(),
        "pick_sound" => "Pick a generated sound into its asset".into(),
        "keep_voice" => "Keep a designed voice".into(),
        "open" => format!("Open {}", s("document")),
        "read_skill" => match input["name"].as_str() {
            Some(skill) => format!("Read the {skill} skill"),
            None => "List the skills".into(),
        },
        "read_doc" => format!("Read {}", s("path")),
        "lab_run" => format!("Run the lab scenario {}", s("scenario")),
        "map_blocks" => format!("Blocks: {} on {}", s("op"), s("map")),
        "map_place" => format!("Place {} on {}", s("kind"), s("map")),
        "encounter" => format!("Cast {} on the stage", s("ability")),
        other => other.replace('_', " "),
    }
}

fn cut(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_string();
    }
    let mut out: String = text.chars().take(at).collect();
    out.push_str("\n…");
    out
}

fn code(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(BG)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(5)
        .inner_margin(Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).monospace().size(10.5).color(SOFT)).wrap());
        });
}

fn thousands(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// Claude's text, with the little Markdown it writes: headings, lists, code blocks,
/// **bold** and `code`.
pub fn markdown(ui: &mut Ui, text: &str) {
    let mut in_code = false;
    let mut block = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                code(ui, block.trim_end());
                block.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            block.push_str(line);
            block.push('\n');
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            ui.add_space(4.0);
        } else if let Some(heading) = trimmed.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim();
            ui.add_space(4.0);
            ui.label(RichText::new(heading).color(TEXT).strong());
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let indent = (line.len() - trimmed.len()) as f32 * 4.0;
            ui.horizontal_top(|ui| {
                ui.add_space(indent);
                ui.label(RichText::new("•").color(MUTED));
                inline(ui, item);
            });
        } else {
            inline(ui, trimmed);
        }
    }
    if in_code && !block.is_empty() {
        code(ui, block.trim_end());
    }
}

/// One line with **bold** and `code` spans.
fn inline(ui: &mut Ui, text: &str) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let body = egui::TextFormat {
        font_id: FontId::proportional(13.0),
        color: SOFT,
        ..Default::default()
    };
    // No bold face: strong text is brighter and a touch larger than the body.
    let strong = egui::TextFormat {
        color: TEXT,
        font_id: FontId::proportional(13.5),
        ..Default::default()
    };
    let emphasis = egui::TextFormat {
        italics: true,
        ..body.clone()
    };
    let mono = egui::TextFormat {
        font_id: FontId::monospace(11.5),
        color: SOFT,
        background: BUBBLE,
        ..Default::default()
    };
    let mut rest = text;
    while !rest.is_empty() {
        let bold = rest.find("**");
        let tick = rest.find('`');
        // A single `*` opening a word (not `**`, not a lone star between spaces).
        let star = rest
            .char_indices()
            .find(|&(i, c)| {
                c == '*'
                    && !rest[i..].starts_with("**")
                    && (i == 0 || !rest[..i].ends_with('*'))
                    && rest[i + 1..]
                        .chars()
                        .next()
                        .is_some_and(|n| !n.is_whitespace())
                    && rest[i + 1..].contains('*')
            })
            .map(|(i, _)| i);
        if let Some(s) = star
            && bold.is_none_or(|b| s < b)
            && tick.is_none_or(|t| s < t)
        {
            job.append(&rest[..s], 0.0, body.clone());
            let after = &rest[s + 1..];
            let end = after.find('*').unwrap_or(after.len());
            job.append(&after[..end], 0.0, emphasis.clone());
            rest = after.get(end + 1..).unwrap_or("");
            continue;
        }
        match (bold, tick) {
            (Some(b), t) if t.is_none_or(|t| b < t) => {
                job.append(&rest[..b], 0.0, body.clone());
                let after = &rest[b + 2..];
                match after.find("**") {
                    Some(end) => {
                        job.append(&after[..end], 0.0, strong.clone());
                        rest = &after[end + 2..];
                    }
                    None => {
                        job.append(after, 0.0, body.clone());
                        rest = "";
                    }
                }
            }
            (_, Some(t)) => {
                job.append(&rest[..t], 0.0, body.clone());
                let after = &rest[t + 1..];
                match after.find('`') {
                    Some(end) => {
                        job.append(&after[..end], 0.0, mono.clone());
                        rest = &after[end + 1..];
                    }
                    None => {
                        job.append(after, 0.0, body.clone());
                        rest = "";
                    }
                }
            }
            _ => {
                job.append(rest, 0.0, body.clone());
                rest = "";
            }
        }
    }
    ui.label(job);
}
