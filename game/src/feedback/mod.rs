//! Player feedback (D59, [ui §Submit feedback](../../docs/ui.md)): a report with a
//! screenshot, sent to the match server (`POST /api/feedback`), which files it as an issue
//! in a private repository.
//!
//! - [`build`] puts a report together: the player's words and category, the game version,
//!   the operating system, the graphics adapter and preset, the map and match, this
//!   installation's anonymous id ([`install_id`]) and the screenshot ([`grab`]).
//! - [`Outbox`] keeps every report in the user folder until the server has taken it, so a
//!   report written offline (or while the server is down) is sent later.
//! - [`Courier`] sends: the report the player just submitted at once, the waiting ones on
//!   start and every few minutes, one at a time.

pub mod grab;

use crate::paths;
use anyhow::{Context as _, Result};
use base64::Engine as _;
use canopy_net::feedback::{FeedbackMeta, FeedbackReceipt, FeedbackReport, FeedbackScreenshot};
use canopy_net::{Net, NetError, Pending, ServerAddress};
use grab::Shot;
use sha2::Digest as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The longest description the server takes, in characters.
pub const MAX_DESCRIPTION: usize = 5000;
/// How often waiting reports are sent again.
const RETRY: Duration = Duration::from_secs(5 * 60);

/// What a report is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    Bug,
    Idea,
    Balance,
    Other,
}

impl Category {
    /// In the form's order.
    pub const ALL: [Self; 4] = [Self::Bug, Self::Idea, Self::Balance, Self::Other];

    pub fn id(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Idea => "idea",
            Self::Balance => "balance",
            Self::Other => "other",
        }
    }
}

/// Whether a description may be sent: it has text besides spaces.
pub fn description_ok(text: &str) -> bool {
    !text.trim().is_empty()
}

/// The machine and the game: what every report carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct System {
    pub version: String,
    pub os: String,
    pub gpu: String,
    /// The graphics preset (`High`, or `Custom`).
    pub preset: String,
}

/// Where the player was when they opened the form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Place {
    /// In a match (else the menu).
    pub in_match: bool,
    pub map: Option<String>,
    /// `skirmish`, `campaign` or `multiplayer`.
    pub mode: Option<&'static str>,
    pub tick: Option<u32>,
}

impl Place {
    /// What the screenshot shows, in the form and on the issue.
    pub fn shot_label(&self) -> &'static str {
        if self.in_match {
            "Screenshot of the game"
        } else {
            "Screenshot of the menu"
        }
    }
}

/// A new report.
pub fn build(
    install_id: &str,
    category: Category,
    description: &str,
    system: &System,
    place: &Place,
    shot: Option<&Shot>,
) -> FeedbackReport {
    FeedbackReport {
        report_id: random_id(),
        install_id: install_id.to_string(),
        category: category.id().to_string(),
        description: description.trim().chars().take(MAX_DESCRIPTION).collect(),
        meta: FeedbackMeta {
            version: system.version.clone(),
            os: system.os.clone(),
            gpu: system.gpu.clone(),
            preset: Some(system.preset.clone()),
            screen: Some(if place.in_match { "match" } else { "menu" }.to_string()),
            map: place.map.clone(),
            mode: place.mode.map(str::to_string),
            tick: place.tick,
        },
        screenshot: shot.map(|s| FeedbackScreenshot {
            mime: "image/jpeg".into(),
            data: base64::engine::general_purpose::STANDARD.encode(&s.jpeg),
            label: Some(place.shot_label().to_string()),
        }),
    }
}

/// The operating system and its version (`macOS 15.4 (aarch64)`), looked up once.
pub fn os_description() -> String {
    static OS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    OS.get_or_init(|| {
        let name = os_name();
        format!("{name} ({})", std::env::consts::ARCH)
    })
    .clone()
}

fn os_name() -> String {
    let run = |program: &str, args: &[&str]| -> Option<String> {
        let out = std::process::Command::new(program)
            .args(args)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !text.is_empty()).then_some(text)
    };
    match std::env::consts::OS {
        "macos" => run("sw_vers", &["-productVersion"])
            .map_or_else(|| "macOS".into(), |v| format!("macOS {v}")),
        "windows" => run("cmd", &["/C", "ver"]).unwrap_or_else(|| "Windows".into()),
        "linux" => std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|v| v.trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "Linux".into()),
        other => other.to_string(),
    }
}

/// A random id: 32 hexadecimal digits. The standard library's hasher keys come from the
/// operating system's random source; the time and process id are mixed in.
fn random_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut digest = sha2::Sha256::new();
    for _ in 0..4 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        digest.update(h.finish().to_le_bytes());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    digest.update(nanos.to_le_bytes());
    digest.update(std::process::id().to_le_bytes());
    digest.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// This installation's anonymous id: random, made on first use and kept in the user
/// folder. It tells reports from one player apart from many; it says nothing about them.
pub fn install_id(user: &Path) -> String {
    let path = user.join(paths::INSTALL_ID);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let id = text.trim();
        if (8..=64).contains(&id.len())
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return id.to_string();
        }
    }
    let id = random_id();
    if let Err(error) = write_atomic(&path, id.as_bytes()) {
        tracing::warn!(%error, "cannot keep the install id");
    }
    id
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes).with_context(|| temp.display().to_string())?;
    std::fs::rename(&temp, path).with_context(|| path.display().to_string())?;
    Ok(())
}

/// Reports not yet taken by the server: `feedback/<report id>.json` in the user folder,
/// the request exactly as it is sent (screenshot included).
#[derive(Debug, Clone)]
pub struct Outbox {
    dir: PathBuf,
}

impl Outbox {
    pub fn new(user: &Path) -> Self {
        Self {
            dir: user.join(paths::FEEDBACK_OUTBOX),
        }
    }

    fn path(&self, report_id: &str) -> PathBuf {
        self.dir.join(format!("{report_id}.json"))
    }

    /// Keeps a report until [`Outbox::remove`].
    pub fn put(&self, report: &FeedbackReport) -> Result<()> {
        write_atomic(&self.path(&report.report_id), &serde_json::to_vec(report)?)
    }

    pub fn remove(&self, report_id: &str) {
        let _ = std::fs::remove_file(self.path(report_id));
    }

    /// The waiting reports, oldest first. Files that do not read as a report are moved
    /// aside (`.bad`) so they are not tried again.
    pub fn waiting(&self) -> Vec<FeedbackReport> {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .map(|p| {
                let time = std::fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                (time, p)
            })
            .collect();
        files.sort();
        files
            .into_iter()
            .filter_map(|(_, path)| {
                let report = std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<FeedbackReport>(&b).ok());
                if report.is_none() {
                    tracing::warn!(path = %path.display(), "unreadable feedback report set aside");
                    let _ = std::fs::rename(&path, path.with_extension("bad"));
                }
                report
            })
            .collect()
    }
}

/// What became of a report sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// The server took it and filed it under `id`.
    Sent { report: String, id: String },
    /// Not now (offline, the server busy or not yet updated): it waits in the outbox.
    Queued { report: String },
    /// The server will never take it (`message` says why); it was dropped.
    Refused { report: String, message: String },
}

/// Sends reports one at a time: the one just submitted first, then the outbox's.
pub struct Courier {
    outbox: Outbox,
    net: Option<Net>,
    sending: Option<(String, Pending<FeedbackReceipt>)>,
    /// Reports to send before looking at the outbox again.
    queue: Vec<FeedbackReport>,
    next_retry: Instant,
}

impl Courier {
    pub fn new(user: &Path) -> Self {
        Self {
            outbox: Outbox::new(user),
            net: None,
            sending: None,
            queue: Vec::new(),
            // The first look at the outbox comes a moment after start.
            next_retry: Instant::now() + Duration::from_secs(10),
        }
    }

    /// Keeps the report in the outbox and sends it next. `Err` when it could not be kept;
    /// it is sent anyway.
    pub fn submit(&mut self, report: FeedbackReport) -> Result<()> {
        let kept = self.outbox.put(&report);
        self.queue.retain(|r| r.report_id != report.report_id);
        self.queue.insert(0, report);
        kept
    }

    /// Advances sending; `server` says where the match server is (asked only when a
    /// report is to be sent). Returns what became of the reports that finished.
    pub fn poll(
        &mut self,
        server: impl FnOnce() -> Result<ServerAddress, canopy_net::server::AddressError>,
    ) -> Vec<Delivery> {
        let mut done = Vec::new();
        if let Some((report, pending)) = &self.sending
            && let Some(result) = pending.poll()
        {
            let report = report.clone();
            self.sending = None;
            done.push(self.finish(report, result));
        }
        if self.sending.is_none() && self.queue.is_empty() && Instant::now() >= self.next_retry {
            self.next_retry = Instant::now() + RETRY;
            self.queue = self.outbox.waiting();
            if !self.queue.is_empty() {
                tracing::info!(waiting = self.queue.len(), "sending waiting feedback");
            }
        }
        if self.sending.is_none() && !self.queue.is_empty() {
            let report = self.queue.remove(0);
            match server()
                .map_err(anyhow::Error::from)
                .and_then(|a| self.net(a))
            {
                Ok(net) => {
                    let pending = net.send_feedback(&report);
                    self.sending = Some((report.report_id, pending));
                }
                Err(error) => {
                    tracing::warn!(%error, "feedback cannot reach the server");
                    self.queue.clear();
                    done.push(Delivery::Queued {
                        report: report.report_id,
                    });
                }
            }
        }
        // The network thread stops while nothing is to be sent.
        if self.sending.is_none() && self.queue.is_empty() {
            self.net = None;
        }
        done
    }

    fn finish(&mut self, report: String, result: Result<FeedbackReceipt, NetError>) -> Delivery {
        match result {
            Ok(receipt) => {
                tracing::info!(id = %receipt.id, "feedback sent");
                self.outbox.remove(&report);
                Delivery::Sent {
                    report,
                    id: receipt.id,
                }
            }
            Err(error) if error.retryable() => {
                tracing::info!(%error, "feedback waits to be sent");
                // The others would fail the same way: they wait for the next retry.
                self.queue.clear();
                self.next_retry = Instant::now() + RETRY;
                Delivery::Queued { report }
            }
            Err(error) => {
                tracing::warn!(%error, "feedback refused");
                self.outbox.remove(&report);
                Delivery::Refused {
                    report,
                    message: error.to_string(),
                }
            }
        }
    }

    fn net(&mut self, address: ServerAddress) -> Result<&Net> {
        if self.net.as_ref().is_none_or(|n| n.server() != &address) {
            self.net = Some(Net::new(address)?);
        }
        Ok(self.net.as_ref().expect("set above"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "canopy-feedback-{name}-{}-{}",
            std::process::id(),
            random_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn system() -> System {
        System {
            version: "0.3.7".into(),
            os: "macOS 26.0 (aarch64)".into(),
            gpu: "Apple M1 (Metal)".into(),
            preset: "High".into(),
        }
    }

    #[test]
    fn a_description_needs_text() {
        assert!(!description_ok(""));
        assert!(!description_ok("  \t "));
        assert!(description_ok(" x "));
    }

    #[test]
    fn reports_carry_the_match_and_the_screenshot() {
        let place = Place {
            in_match: true,
            map: Some("forest".into()),
            mode: Some("skirmish"),
            tick: Some(1200),
        };
        let shot = Shot {
            jpeg: vec![0xff, 0xd8, 0xff],
            thumb: grab::Thumb::default(),
        };
        let long = "y".repeat(MAX_DESCRIPTION + 10);
        let report = build(
            "install-1",
            Category::Balance,
            &format!("  {long}"),
            &system(),
            &place,
            Some(&shot),
        );
        assert_eq!(report.category, "balance");
        assert_eq!(report.description.chars().count(), MAX_DESCRIPTION);
        assert_eq!(report.meta.screen.as_deref(), Some("match"));
        assert_eq!(report.meta.map.as_deref(), Some("forest"));
        assert_eq!(report.meta.tick, Some(1200));
        let screenshot = report.screenshot.unwrap();
        assert_eq!(screenshot.data, "/9j/");
        assert_eq!(screenshot.label.as_deref(), Some("Screenshot of the game"));
        // From the menu: no match, and the screenshot says so.
        let menu = build("i", Category::Bug, "x", &system(), &Place::default(), None);
        assert_eq!(menu.meta.screen.as_deref(), Some("menu"));
        assert!(menu.meta.map.is_none() && menu.screenshot.is_none());
        assert_eq!(Place::default().shot_label(), "Screenshot of the menu");
        assert_ne!(menu.report_id, report.report_id);
    }

    #[test]
    fn the_install_id_is_made_once_and_kept() {
        let user = folder("install");
        let id = install_id(&user);
        assert_eq!(id.len(), 32);
        assert_eq!(install_id(&user), id);
        // A damaged file is replaced.
        std::fs::write(user.join(paths::INSTALL_ID), "not an id!").unwrap();
        let fresh = install_id(&user);
        assert_ne!(fresh, id);
        assert_eq!(install_id(&user), fresh);
    }

    #[test]
    fn the_outbox_keeps_reports_until_removed() {
        let user = folder("outbox");
        let outbox = Outbox::new(&user);
        assert!(outbox.waiting().is_empty());
        let first = build(
            "i",
            Category::Bug,
            "first",
            &system(),
            &Place::default(),
            None,
        );
        let second = build(
            "i",
            Category::Idea,
            "second",
            &system(),
            &Place::default(),
            None,
        );
        outbox.put(&first).unwrap();
        // Distinct modification times keep the order on coarse file systems.
        std::thread::sleep(Duration::from_millis(20));
        outbox.put(&second).unwrap();
        std::fs::write(user.join(paths::FEEDBACK_OUTBOX).join("junk.json"), "{").unwrap();
        let waiting = outbox.waiting();
        assert_eq!(waiting, vec![first.clone(), second.clone()]);
        assert!(user.join(paths::FEEDBACK_OUTBOX).join("junk.bad").exists());
        outbox.remove(&first.report_id);
        assert_eq!(outbox.waiting(), vec![second]);
    }

    #[test]
    fn an_unreachable_server_leaves_the_report_waiting() {
        let user = folder("courier");
        let mut courier = Courier::new(&user);
        let report = build(
            "i",
            Category::Bug,
            "offline",
            &system(),
            &Place::default(),
            None,
        );
        let id = report.report_id.clone();
        courier.submit(report).unwrap();
        // Nothing listens on this loopback port: the send fails and the report waits.
        let server = || ServerAddress::parse("http://127.0.0.1:9");
        let start = Instant::now();
        let mut done = Vec::new();
        while done.is_empty() && start.elapsed() < Duration::from_secs(20) {
            done = courier.poll(server);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(done, vec![Delivery::Queued { report: id.clone() }]);
        assert!(courier.sending.is_none());
        assert_eq!(Outbox::new(&user).waiting()[0].report_id, id);
    }
}
