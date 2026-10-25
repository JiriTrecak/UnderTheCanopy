//! The assistant (docs/editor.md, The assistant): a sidebar over every editor where the
//! user talks with Claude about the open package. It calls the Claude API directly with
//! the user's key and works through the editor's own tools (the MCP server's, so agents
//! outside and the assistant inside do the same things the same way), with a thread per
//! topic kept in the user folder.

pub mod agent;
pub mod api;
pub mod prompt;
pub mod view;

use agent::{Endpoint, Update};
use api::{Delta, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

/// The folder in the user folder that holds the assistant's settings, key and threads.
const FOLDER: &str = "assistant";
/// The environment variable a key may come from instead.
const KEY_VARIABLE: &str = "ANTHROPIC_API_KEY";

/// A conversation: its messages in the API's form, as they were sent and answered.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Thread {
    pub id: String,
    pub title: String,
    /// Milliseconds since 1970 when it began.
    pub created: u64,
    pub messages: Vec<Value>,
    /// Token counts so far: input, output, cache reads, cache writes.
    pub usage: [u64; 4],
    /// What it has cost, in dollars.
    pub cost: f64,
}

/// A thread in the list.
#[derive(Debug, Clone)]
pub struct Head {
    pub id: String,
    pub title: String,
    pub created: u64,
}

/// A tool call waiting for the user's answer.
pub struct Approval {
    pub name: String,
    pub input: Value,
    reply: mpsc::Sender<bool>,
}

/// The loop at work on the current thread.
pub struct Running {
    stop: Arc<AtomicBool>,
    updates: mpsc::Receiver<Update>,
    /// The turn streaming in: its text, its latest progress note, the tool being called.
    pub text: String,
    pub note: String,
    pub tool: Option<String>,
    pub approval: Option<Approval>,
}

/// Where the key comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Environment(String),
    Stored(String),
    Missing,
}

impl Key {
    fn value(&self) -> Option<&str> {
        match self {
            Key::Environment(k) | Key::Stored(k) => Some(k),
            Key::Missing => None,
        }
    }
}

/// What the editor tells the assistant each frame.
pub struct Env<'a> {
    /// The editor's tool channel (MCP's).
    pub calls: &'a mpsc::Sender<crate::mcp::Call>,
    /// What the user is looking at, for the next message.
    pub context: String,
}

pub struct Assistant {
    user: Option<PathBuf>,
    data: Option<PathBuf>,
    pub threads: Vec<Head>,
    pub thread: Thread,
    pub composer: String,
    pub running: Option<Running>,
    pub settings: Settings,
    pub key: Key,
    /// The settings panel, the key typed there, and the thread list are open.
    pub settings_open: bool,
    pub key_entry: String,
    pub list_open: bool,
    /// Why the last turn stopped, when it did not finish.
    pub error: Option<String>,
    /// The Messages endpoint (a local stand-in in tests).
    url: String,
    /// The prompt (built once: the skills do not change while the editor runs).
    system: Option<String>,
    /// Scroll the conversation to its end on the next frame.
    pub follow: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    model: Option<String>,
    effort: Option<String>,
}

impl Assistant {
    pub fn new(user: Option<PathBuf>, data: Option<PathBuf>) -> Self {
        let mut settings = Settings::default();
        if let Some(stored) = user
            .as_deref()
            .and_then(|u| std::fs::read_to_string(u.join(FOLDER).join("settings.json")).ok())
            .and_then(|t| serde_json::from_str::<Stored>(&t).ok())
        {
            if let Some(model) = stored
                .model
                .filter(|m| api::MODELS.iter().any(|x| x.0 == m))
            {
                settings.model = model;
            }
            if let Some(effort) = stored.effort.filter(|e| api::EFFORTS.contains(&e.as_str())) {
                settings.effort = effort;
            }
        }
        let mut me = Self {
            key: Key::Missing,
            user,
            data,
            threads: Vec::new(),
            thread: Thread::default(),
            composer: String::new(),
            running: None,
            settings,
            settings_open: false,
            key_entry: String::new(),
            list_open: false,
            error: None,
            url: api::URL.to_string(),
            system: None,
            follow: false,
        };
        me.key = me.read_key();
        me.load_threads();
        me
    }

    fn folder(&self) -> Option<PathBuf> {
        self.user.as_deref().map(|u| u.join(FOLDER))
    }

    fn threads_folder(&self) -> Option<PathBuf> {
        self.folder().map(|f| f.join("threads"))
    }

    // --- The key -----------------------------------------------------------------------

    fn read_key(&self) -> Key {
        if let Ok(key) = std::env::var(KEY_VARIABLE)
            && !key.trim().is_empty()
        {
            return Key::Environment(key.trim().to_string());
        }
        self.folder()
            .and_then(|f| std::fs::read_to_string(f.join("key")).ok())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .map(Key::Stored)
            .unwrap_or(Key::Missing)
    }

    /// Keeps `key` in the user folder, readable by this user only. It is never shown,
    /// logged or sent anywhere but the API.
    pub fn store_key(&mut self, key: &str) -> Result<(), String> {
        let folder = self
            .folder()
            .ok_or("There is no user folder to keep it in.")?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let file = folder.join("key");
        std::fs::write(&file, key.trim()).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
        }
        self.key = self.read_key();
        Ok(())
    }

    pub fn forget_key(&mut self) {
        if let Some(folder) = self.folder() {
            let _ = std::fs::remove_file(folder.join("key"));
        }
        self.key = self.read_key();
    }

    pub fn store_settings(&self) {
        let Some(folder) = self.folder() else { return };
        let stored = Stored {
            model: Some(self.settings.model.clone()),
            effort: Some(self.settings.effort.clone()),
        };
        if std::fs::create_dir_all(&folder).is_ok() {
            let _ = std::fs::write(
                folder.join("settings.json"),
                crate::document::format(&serde_json::to_value(&stored).unwrap_or_default()),
            );
        }
    }

    // --- Threads -----------------------------------------------------------------------

    fn load_threads(&mut self) {
        self.threads.clear();
        let Some(folder) = self.threads_folder() else {
            return;
        };
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if let Ok(thread) = serde_json::from_str::<Thread>(&text) {
                self.threads.push(Head {
                    id: thread.id,
                    title: thread.title,
                    created: thread.created,
                });
            }
        }
        self.threads.sort_by_key(|h| std::cmp::Reverse(h.created));
    }

    fn save_thread(&mut self) {
        if self.thread.messages.is_empty() {
            return;
        }
        let Some(folder) = self.threads_folder() else {
            return;
        };
        if std::fs::create_dir_all(&folder).is_err() {
            return;
        }
        let file = folder.join(format!("{}.json", self.thread.id));
        let text = serde_json::to_string(&self.thread).unwrap_or_default();
        if let Err(error) = std::fs::write(file, text) {
            tracing::warn!(%error, "editor: cannot keep the assistant's thread");
        }
        if !self.threads.iter().any(|h| h.id == self.thread.id) {
            self.threads.insert(
                0,
                Head {
                    id: self.thread.id.clone(),
                    title: self.thread.title.clone(),
                    created: self.thread.created,
                },
            );
        }
    }

    pub fn new_thread(&mut self) {
        if self.running.is_some() {
            return;
        }
        self.thread = Thread::default();
        self.error = None;
        self.list_open = false;
    }

    pub fn open_thread(&mut self, id: &str) {
        if self.running.is_some() {
            return;
        }
        let Some(file) = self.threads_folder().map(|f| f.join(format!("{id}.json"))) else {
            return;
        };
        if let Some(thread) = std::fs::read_to_string(file)
            .ok()
            .and_then(|t| serde_json::from_str::<Thread>(&t).ok())
        {
            self.thread = thread;
            self.error = None;
            self.list_open = false;
            self.follow = true;
        }
    }

    pub fn delete_thread(&mut self, id: &str) {
        if self.running.is_some() && self.thread.id == id {
            return;
        }
        if let Some(folder) = self.threads_folder() {
            let _ = std::fs::remove_file(folder.join(format!("{id}.json")));
        }
        self.threads.retain(|h| h.id != id);
        if self.thread.id == id {
            self.thread = Thread::default();
        }
    }

    // --- Talking -----------------------------------------------------------------------

    /// Sends the composer's text with what the user is looking at, and starts the loop.
    pub fn send(&mut self, env: &Env) {
        let text = self.composer.trim().to_string();
        if text.is_empty() || self.running.is_some() {
            return;
        }
        let Some(key) = self.key.value().map(str::to_string) else {
            self.settings_open = true;
            self.error = Some("Add an API key to talk with the assistant.".into());
            return;
        };
        if self.thread.id.is_empty() {
            let now = now_millis();
            self.thread.id = format!("{now}");
            self.thread.created = now;
            self.thread.title = title_of(&text);
        }
        self.thread.messages.push(json!({
            "role": "user",
            "content": [
                { "type": "text", "text": format!("<editor-context>\n{}\n</editor-context>", env.context) },
                { "type": "text", "text": text },
            ],
        }));
        self.composer.clear();
        self.error = None;
        self.save_thread();
        let system = self
            .system
            .get_or_insert_with(|| {
                // The content skills; the repository's workflow skills (building,
                // releasing) are for agents with a shell, not for the editor.
                let skills: Vec<(String, String)> = self
                    .data
                    .as_deref()
                    .map(crate::shell::skills)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(name, _)| name.starts_with("canopy-"))
                    .collect();
                prompt::system(&skills)
            })
            .clone();
        let (updates, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        agent::start(
            Endpoint {
                url: self.url.clone(),
                key,
            },
            self.settings.clone(),
            system,
            self.thread.messages.clone(),
            env.calls.clone(),
            updates,
            stop.clone(),
        );
        self.running = Some(Running {
            stop,
            updates: receiver,
            text: String::new(),
            note: String::new(),
            tool: None,
            approval: None,
        });
        self.follow = true;
    }

    pub fn stop(&mut self) {
        if let Some(running) = &mut self.running {
            running.stop.store(true, Ordering::Relaxed);
            if let Some(approval) = running.approval.take() {
                let _ = approval.reply.send(false);
            }
        }
    }

    /// Answers the tool call waiting for the user.
    pub fn approve(&mut self, allow: bool) {
        if let Some(approval) = self.running.as_mut().and_then(|r| r.approval.take()) {
            let _ = approval.reply.send(allow);
        }
    }

    /// Takes in what the loop reported since the last frame. Whether anything changed.
    pub fn poll(&mut self) -> bool {
        let Some(running) = &mut self.running else {
            return false;
        };
        let mut changed = false;
        let mut finished = None;
        while let Ok(update) = running.updates.try_recv() {
            changed = true;
            match update {
                Update::Delta(Delta::Text(text)) => {
                    running.text.push_str(&text);
                    running.tool = None;
                }
                Update::Delta(Delta::Note(note)) => running.note.push_str(&note),
                Update::Delta(Delta::Tool(name)) => running.tool = Some(name),
                Update::Turn(turn) => {
                    self.thread.messages.push(turn);
                    running.text.clear();
                    running.note.clear();
                    running.tool = None;
                }
                Update::Approve {
                    name, input, reply, ..
                } => {
                    running.approval = Some(Approval { name, input, reply });
                }
                Update::Usage { model, usage } => {
                    for (total, n) in self.thread.usage.iter_mut().zip(usage) {
                        *total += n;
                    }
                    self.thread.cost += api::cost(&model, usage);
                }
                Update::Finished(outcome) => finished = Some(outcome),
            }
        }
        if changed {
            self.follow = true;
        }
        if let Some(outcome) = finished {
            self.running = None;
            self.error = outcome.filter(|e| e != "Stopped.");
            self.save_thread();
        }
        changed
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// The skills the prompt lists (for the settings panel).
    pub fn skill_count(&self) -> usize {
        self.data
            .as_deref()
            .map(crate::shell::skills)
            .map_or(0, |s| {
                s.iter()
                    .filter(|(name, _)| name.starts_with("canopy-"))
                    .count()
            })
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A thread's title from its first message: its first line, cut to a few words.
fn title_of(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= 48 {
        return line.to_string();
    }
    let mut title: String = line.chars().take(46).collect();
    if let Some(space) = title.rfind(' ') {
        title.truncate(space);
    }
    title + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_from_the_first_line() {
        assert_eq!(
            title_of("Make the Warrior tougher\nthanks"),
            "Make the Warrior tougher"
        );
        let long = title_of(
            "Please rebalance every ant unit so that the early game feels slower and more deliberate",
        );
        assert!(long.ends_with('…') && long.chars().count() <= 48);
    }

    #[test]
    fn threads_keep_in_the_user_folder_and_list_newest_first() {
        let user = std::env::temp_dir().join(format!("canopy-assistant-{}", now_millis()));
        let mut assistant = Assistant::new(Some(user.clone()), None);
        for (id, created) in [("1", 10), ("2", 20)] {
            assistant.thread = Thread {
                id: id.into(),
                title: format!("thread {id}"),
                created,
                messages: vec![json!({"role":"user","content":"hi"})],
                ..Thread::default()
            };
            assistant.save_thread();
        }
        let again = Assistant::new(Some(user.clone()), None);
        let ids: Vec<&str> = again.threads.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["2", "1"]);
        let mut again = again;
        again.open_thread("1");
        assert_eq!(again.thread.title, "thread 1");
        again.delete_thread("1");
        assert_eq!(again.threads.len(), 1);
        let _ = std::fs::remove_dir_all(user);
    }

    #[test]
    fn a_stored_key_is_private_and_can_be_forgotten() {
        if std::env::var(KEY_VARIABLE).is_ok() {
            return; // The environment's key wins; nothing to test here.
        }
        let user = std::env::temp_dir().join(format!("canopy-key-{}", now_millis()));
        let mut assistant = Assistant::new(Some(user.clone()), None);
        assert_eq!(assistant.key, Key::Missing);
        assistant.store_key("  sk-test  ").unwrap();
        assert_eq!(assistant.key, Key::Stored("sk-test".into()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(user.join(FOLDER).join("key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "only its owner reads it");
        }
        assistant.forget_key();
        assert_eq!(assistant.key, Key::Missing);
        let _ = std::fs::remove_dir_all(user);
    }
}
