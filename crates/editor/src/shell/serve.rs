//! The shell's side of MCP (docs/editor.md): answering tool calls with the editor's own
//! state and commands, starting and stopping the server, its token and its setting.

use super::*;
use crate::mcp::{self, Call, Tool};

/// The editor's settings file in the user folder.
const SETTINGS: &str = "editor.json";
/// The per-install token MCP clients must send.
const TOKEN: &str = "editor-mcp-token";

/// A token from the operating system's randomness (std's hasher keys are seeded from it).
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|_| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(std::process::id().into());
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// The install's token, made on first use.
pub(super) fn token(user: Option<&std::path::Path>) -> String {
    let Some(user) = user else {
        return new_token();
    };
    let file = user.join(TOKEN);
    if let Ok(text) = std::fs::read_to_string(&file)
        && text.trim().len() >= 16
    {
        return text.trim().to_string();
    }
    let token = new_token();
    if let Err(error) = std::fs::write(&file, &token) {
        tracing::warn!(%error, "editor: cannot keep the MCP token");
    }
    token
}

/// Whether the server starts with the editor (the setting).
pub(super) fn autostart(user: Option<&std::path::Path>) -> bool {
    user.and_then(|u| std::fs::read_to_string(u.join(SETTINGS)).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.get("mcpAutostart").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn store_autostart(user: Option<&std::path::Path>, on: bool) {
    let Some(user) = user else { return };
    let file = user.join(SETTINGS);
    let mut settings = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    settings["mcpAutostart"] = Value::Bool(on);
    if let Err(error) = std::fs::write(&file, crate::document::format(&settings)) {
        tracing::warn!(%error, "editor: cannot keep its settings");
    }
}

impl Shell {
    /// The port of the running server.
    pub fn mcp_port(&self) -> Option<u16> {
        self.server.as_ref().map(|s| s.port)
    }

    pub fn start_mcp(&mut self) {
        if self.server.is_some() || self.store.is_none() {
            return;
        }
        match mcp::start(mcp::PORT, self.token.clone(), self.calls.0.clone()) {
            Ok(server) => {
                self.server = Some(server);
                self.mcp_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "editor: MCP server");
                self.mcp_error = Some(format!("Port {} is taken: {error}", mcp::PORT));
            }
        }
    }

    pub fn stop_mcp(&mut self) {
        self.server = None;
    }

    /// The command that adds the server to Claude Code.
    pub fn claude_command(&self) -> String {
        mcp::claude_command(mcp::PORT, &self.token)
    }

    /// What the editor does each frame besides drawing: finished checks, files changed on
    /// disk, libraries, the next check and MCP's calls. A headless editor runs only this.
    pub fn tick(&mut self) {
        if let Some(checker) = &mut self.checker {
            checker.poll();
        }
        self.poll_cook();
        self.follow_disk();
        if let Ok(package) = self.package.clone() {
            self.refresh_libraries(&package);
        }
        self.check();
        self.serve_calls();
    }

    /// Answers the tool calls that arrived; `problems` waits for the current check.
    pub fn serve_calls(&mut self) {
        while let Ok(Call { tool, reply }) = self.calls.1.try_recv() {
            if matches!(tool, Tool::Problems) {
                self.waiting.push(reply);
                continue;
            }
            if matches!(tool, Tool::Cook) {
                self.start_cook();
                self.cook_waiting.push(reply);
                continue;
            }
            let answer = self.answer(tool);
            let _ = reply.send(answer);
        }
        if !self.waiting.is_empty() && !self.checking() {
            let answer = self.problems_value();
            for reply in self.waiting.drain(..) {
                let _ = reply.send(Ok(answer.clone()));
            }
        }
    }

    fn problems_value(&self) -> Value {
        let problems: Vec<Value> = self
            .checker
            .as_ref()
            .and_then(|c| c.report())
            .map(|r| {
                r.problems
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "file": p.file,
                            "document": p.document,
                            "location": p.location,
                            "message": p.message,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        serde_json::json!({ "problems": problems })
    }

    /// Every use of `id` in the package.
    pub(super) fn find_uses(&mut self, id: &str) -> Vec<Use> {
        let mut uses = Vec::new();
        let documents = self.all_documents();
        if let Some(store) = &mut self.store {
            for path in documents {
                if let Ok(Some(value)) = store.get(&path) {
                    references::find(&path, value, id, &mut uses);
                }
            }
        }
        uses
    }

    fn answer(&mut self, tool: Tool) -> Result<Value, String> {
        use serde_json::json;
        if self.store.is_none() {
            return Err("The base package could not be opened.".into());
        }
        if matches!(
            tool,
            Tool::MapCreate(_)
                | Tool::MapInfo { .. }
                | Tool::MapPicture { .. }
                | Tool::MapStroke(_)
                | Tool::MapPlace(_)
                | Tool::MapRemove(_)
        ) {
            return self.answer_map(tool);
        }
        match tool {
            Tool::Status => {
                let store = self.store.as_ref().expect("checked");
                Ok(json!({
                    "package": self.package.as_ref().map(|p| p.root.display().to_string()).unwrap_or_default(),
                    "unsaved": store.dirty(),
                    "undo": store.undo_label(),
                    "redo": store.redo_label(),
                    "problems": self.problem_count(),
                    "checking": self.checking(),
                }))
            }
            Tool::List { editor } => {
                let wanted = editor.map(|e| e.to_lowercase());
                let mut out = Vec::new();
                for kind in KINDS {
                    if let Some(wanted) = &wanted
                        && kind.label().to_lowercase() != *wanted
                        && kind.name().to_lowercase() != *wanted
                    {
                        continue;
                    }
                    let Some(workspace) = self.workspaces.get(&kind) else {
                        continue;
                    };
                    let mut elements = Vec::new();
                    workspace
                        .library
                        .nodes
                        .iter()
                        .for_each(|n| n.elements(&mut elements));
                    for e in elements {
                        out.push(json!({
                            "editor": kind.label().to_lowercase(),
                            "id": e.id,
                            "name": e.name,
                            "document": e.document,
                        }));
                    }
                }
                if out.is_empty() && wanted.is_some() {
                    return Err("No such editor: maps, units, abilities, effects, buildings, races or assets.".into());
                }
                Ok(Value::Array(out))
            }
            Tool::Read { document } => {
                let store = self.store.as_mut().expect("checked");
                let value = store
                    .get(&document)
                    .map_err(|e| e.to_string())?
                    .cloned()
                    .ok_or_else(|| format!("{document}: no such document"))?;
                Ok(json!({
                    "document": document,
                    "unsaved": store.is_dirty(&document),
                    "value": value,
                }))
            }
            Tool::Schema { document } => {
                Ok(canopy_content::schema::of_document(&document).unwrap_or(Value::Null))
            }
            Tool::Edit { label, commands } => {
                let document = commands
                    .first()
                    .map(|c| c.document().to_string())
                    .ok_or("No commands.")?;
                let context = Context {
                    editor: Kind::of_document(&document).unwrap_or(self.current),
                    document,
                    pointer: None,
                };
                let store = self.store.as_mut().expect("checked");
                store
                    .run(&label, context, commands)
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "done": label, "unsaved": store.dirty().len() }))
            }
            Tool::Undo => {
                let label = self
                    .store
                    .as_ref()
                    .and_then(|s| s.undo_label().map(str::to_string));
                self.undo();
                label
                    .map(|l| json!({ "undone": l }))
                    .ok_or("Nothing to undo.".into())
            }
            Tool::Redo => {
                let label = self
                    .store
                    .as_ref()
                    .and_then(|s| s.redo_label().map(str::to_string));
                self.redo();
                label
                    .map(|l| json!({ "redone": l }))
                    .ok_or("Nothing to redo.".into())
            }
            Tool::Save { force } => {
                let problems = self.problem_count().unwrap_or(0);
                if problems > 0 && !force {
                    return Err(format!(
                        "The check finds {problems} problems (see problems); fix them or save with force."
                    ));
                }
                let store = self.store.as_mut().expect("checked");
                let written = store.save().map_err(|e| e.to_string())?;
                Ok(json!({ "written": written }))
            }
            Tool::Problems => Ok(self.problems_value()),
            Tool::UnusedAssets => Ok(json!({ "unused": self.unused_assets() })),
            Tool::Cook => unreachable!("cooks answer when they finish"),
            Tool::Import(args) => {
                let request = crate::assets::Import {
                    source: args.source.into(),
                    id: args.id,
                    name: args.name.unwrap_or_default(),
                    kind: args.kind,
                    folder: args.folder,
                };
                let document = self.import_asset(&request)?;
                Ok(
                    json!({ "imported": document, "next": "cook, then the asset can be named by definitions and scenery" }),
                )
            }
            Tool::Uses { id } => {
                let uses: Vec<Value> = self
                    .find_uses(&id)
                    .into_iter()
                    .map(|u| json!({ "document": u.document, "pointer": u.pointer, "key": u.key }))
                    .collect();
                Ok(Value::Array(uses))
            }
            Tool::Rename { document, new } => {
                let old = self.id_of(&document);
                if old.is_empty() {
                    return Err(format!("{document}: no element id"));
                }
                canopy_content::Id::new(&new)?;
                if self.ids.binary_search(&new).is_ok() {
                    return Err(format!("{new} is already an element's id."));
                }
                let uses = self.find_uses(&old);
                let store = self.store.as_mut().expect("checked");
                let commands =
                    references::rename(&uses, &new, |d| store.get(d).ok().flatten().cloned());
                let context = Context {
                    editor: Kind::of_document(&document).unwrap_or(self.current),
                    document,
                    pointer: Some("/id".into()),
                };
                store
                    .run(&format!("Rename {old} to {new}"), context, commands)
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "renamed": old, "to": new, "uses": uses.len() }))
            }
            _ => unreachable!("map tools answer above"),
        }
    }

    /// The MCP panel: server state, start and stop, the setting, and the command for
    /// Claude Code.
    pub(super) fn mcp_dialog(&mut self, ctx: &egui::Context) {
        if !self.mcp_open {
            return;
        }
        let command = self.claude_command();
        let running = self.mcp_port();
        let mut autostart = self.autostart;
        let mut toggle = false;
        let response = egui::Modal::new(Id::new("mcp"))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .frame(
                Frame::new()
                    .fill(RAISED)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(10)
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(RichText::new("MCP server for agents").size(14.0).color(TEXT));
                ui.add_space(4.0);
                let state = match (running, &self.mcp_error) {
                    (Some(port), _) => format!("Running on 127.0.0.1:{port}. Agents edit through the editor's commands: their steps undo like yours."),
                    (None, Some(error)) => error.clone(),
                    (None, None) => "Stopped.".into(),
                };
                ui.label(RichText::new(state).color(MUTED));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if running.is_some() { "Stop" } else { "Start" };
                    toggle = ui.add(primary(label)).clicked();
                    ui.checkbox(&mut autostart, "Start with the editor");
                });
                ui.add_space(10.0);
                ui.label(RichText::new("Add it to Claude Code (in the repository):").color(MUTED).size(11.0));
                Frame::new()
                    .fill(BG)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(5)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(&command).monospace().color(SOFT).size(11.0)).wrap());
                    });
                ui.horizontal(|ui| {
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(command.clone());
                    }
                    ui.label(RichText::new("The token stays in your user folder; keep it private.").color(FAINT).size(11.0));
                });
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        self.mcp_open = false;
                    }
                });
            });
        if response.should_close() {
            self.mcp_open = false;
        }
        if autostart != self.autostart {
            self.autostart = autostart;
            store_autostart(self.user.as_deref(), autostart);
        }
        if toggle {
            if running.is_some() {
                self.stop_mcp();
            } else {
                self.start_mcp();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_differ() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
