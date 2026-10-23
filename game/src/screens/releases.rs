//! The main menu's release plaque, What's new and the update details (D43).
//!
//! The plaque (bottom right of the main menu, the old zinc version indicator) shows the
//! installed version and what the update check found; clicking it opens the update
//! details (and retries a failed check), "What's new" opens the release history. Both
//! are one dialog (`layouts/releases.json`): What's new is a timeline of the bundled
//! releases beside the selected release's notes and gallery; the update view shows the
//! check, download and install with the offered version's notes. What's new is a new
//! design (D43, review); the update flow behaves as the old menu's.

use super::ui_host::UiHost;
use super::widgets;
use crate::update::{Phase, State, Updater, history, http::Http};
use canopy_release::{BASE_URL, ChangeKind, ReleaseEntry, ReleaseImage, Version};
use canopy_render::Gpu;
use canopy_ui::{Color, ImageKey, Tooltip, Ui};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Which face of the dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The bundled release history.
    Notes,
    /// The update check, download and install.
    Updates,
}

/// How a kind of change looks: its heading, glyph and colour.
fn tone(kind: ChangeKind) -> (String, &'static str, [u8; 3]) {
    match kind {
        ChangeKind::Added => (tr!("releases.new"), "+", [0x8f, 0xd1, 0x8a]),
        ChangeKind::Improved => (tr!("releases.improved"), "↑", [0xe9, 0xb6, 0x5c]),
        ChangeKind::Fixed => (tr!("releases.fixed"), "✓", [0x7c, 0xc4, 0xd2]),
        ChangeKind::KnownIssues => (tr!("releases.known-issues"), "!", [0xe9, 0x8b, 0x6d]),
    }
}

fn rgb([r, g, b]: [u8; 3], a: u8) -> Color {
    Color::rgba(r, g, b, a)
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Escapes markup in text from the release notes.
fn plain(text: &str) -> String {
    text.replace('[', "[[")
}

/// `2026-10-13T15:44:03.334Z` → `Oct 13, 2026` (the old `toLocaleDateString`).
pub fn date(iso: &str) -> String {
    const MONTHS: [&str; 12] = [
        "date.jan", "date.feb", "date.mar", "date.apr", "date.may", "date.jun", "date.jul",
        "date.aug", "date.sep", "date.oct", "date.nov", "date.dec",
    ];
    let part = |r: std::ops::Range<usize>| iso.get(r).and_then(|s| s.parse::<usize>().ok());
    match (part(0..4), part(5..7), part(8..10)) {
        (Some(y), Some(m), Some(d)) if (1..=12).contains(&m) => {
            let month = crate::text::tr(MONTHS[m - 1]);
            tr!("date.short", month = &month, day = d, year = y)
        }
        _ => iso.to_owned(),
    }
}

/// `"34 new"`, `"2 known issues"`.
fn count(kind: ChangeKind, n: usize) -> String {
    match kind {
        ChangeKind::Added => tr!("releases.count-new", count = n),
        ChangeKind::Improved => tr!("releases.count-improved", count = n),
        ChangeKind::Fixed => tr!("releases.count-fixed", count = n),
        ChangeKind::KnownIssues => tr!("releases.count-known-issues", count = n),
    }
}

fn megabytes(bytes: u64) -> String {
    tr!(
        "releases.megabytes",
        amount = &format!("{:.1}", bytes as f64 / 1_048_576.0)
    )
}

/// What the plaque says about the check (old `draw`): text, colour, tooltip.
pub fn plaque_status(state: &State) -> (String, Tooltip) {
    let tip = |title: &str, body: &str| Tooltip {
        title: title.into(),
        body: body.into(),
        ..Tooltip::default()
    };
    let updates = tr!("releases.desktop-updates");
    if state.available() {
        let version = state.version.map(|v| v.to_string()).unwrap_or_default();
        return (
            format!(
                "[color=#34d399]{}[/color]",
                tr!("releases.update-available")
            ),
            tip(
                &updates,
                &tr!("releases.version-available", version = &version),
            ),
        );
    }
    match state.phase {
        Phase::Current => (tr!("releases.latest"), tip(&updates, "")),
        Phase::Checking => (tr!("releases.checking"), tip(&updates, "")),
        Phase::Error => (
            tr!("releases.check-unavailable"),
            tip(&updates, &tr!("releases.check-failed-tip")),
        ),
        _ => (tr!("releases.check-for-updates"), tip(&updates, "")),
    }
}

/// The update view's status line for a phase (old `messages`).
fn headline(state: &State, installed: Version) -> String {
    let version = state.version.map(|v| v.to_string()).unwrap_or_default();
    match state.phase {
        Phase::Idle => tr!("releases.not-checked"),
        Phase::Checking => tr!("releases.checking-updates"),
        Phase::Current => tr!("releases.up-to-date", version = &installed.to_string()),
        Phase::Available => tr!("releases.version-available", version = &version),
        Phase::Downloading => tr!("releases.downloading"),
        Phase::Ready => tr!("releases.ready"),
        Phase::Installing => tr!("releases.installing"),
        Phase::Restart => tr!("releases.restarting"),
        Phase::Error => tr!("releases.check-unavailable"),
    }
}

/// The primary action of the update view for a phase (old `action`).
pub fn primary_action(state: &State) -> Option<String> {
    match state.phase {
        Phase::Available if state.blocked.is_none() => Some(tr!("releases.download-update")),
        Phase::Ready => Some(tr!("releases.install-restart")),
        Phase::Restart => Some(tr!("releases.restart-now")),
        Phase::Current => Some(tr!("releases.check-again")),
        Phase::Error => Some(tr!("releases.try-again")),
        _ => None,
    }
}

/// Reads plain notes (the feed's, as `ReleaseEntry::plain_notes` writes them: the title,
/// then each kind's heading and `- ` bullets) back into a release, so an offer without
/// its structured changelog still shows by kind. `None` if they do not have that shape.
pub fn from_plain(version: Version, notes: &str) -> Option<ReleaseEntry> {
    let mut lines = notes.lines();
    let title = lines.next()?.trim();
    if title.is_empty() {
        return None;
    }
    let mut changes = canopy_release::ChangeSet::default();
    let mut kind = None;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(k) = ChangeKind::ALL.into_iter().find(|k| k.heading() == line) {
            kind = Some(k);
        } else if let (Some(k), Some(note)) = (kind, line.strip_prefix("- ")) {
            changes.notes_mut(k).push(note.to_owned());
        } else {
            return None;
        }
    }
    if changes.is_empty() {
        return None;
    }
    Some(ReleaseEntry {
        changes,
        version,
        published_at: String::new(),
        title: title.to_owned(),
    })
}

/// The release dialog's model.
pub struct Releases {
    pub view: View,
    /// The release shown in What's new (an index into the history).
    pub selected: usize,
    /// The gallery image shown enlarged, if any.
    pub viewer: Option<usize>,
    history: Vec<&'static ReleaseEntry>,
    installed: Version,
    /// The offered version's plain notes read by kind.
    offered: Option<ReleaseEntry>,
    /// What the article holds (rebuilt when this changes).
    shown: String,
    pub gallery: Gallery,
}

impl Releases {
    pub fn new(data: &Path) -> Self {
        Self {
            view: View::Notes,
            selected: 0,
            viewer: None,
            history: history::installed_history(),
            installed: history::installed(),
            offered: None,
            shown: String::new(),
            gallery: Gallery::new(data),
        }
    }

    /// Shows a different history (captures).
    pub fn set_history(&mut self, history: Vec<&'static ReleaseEntry>, installed: Version) {
        self.history = history;
        self.installed = installed;
        self.selected = 0;
        self.shown.clear();
    }

    pub fn open(&mut self, view: View) {
        self.view = view;
        self.viewer = None;
        if view == View::Notes {
            self.selected = 0;
        }
        self.shown.clear();
    }

    /// Whether the dialog may close now (old: not while busy or restarting).
    pub fn closable(updater: &Updater) -> bool {
        !updater.busy() && updater.state().phase != Phase::Restart
    }

    pub fn select(&mut self, index: usize) {
        if index < self.history.len() {
            self.selected = index;
            self.viewer = None;
        }
    }

    /// The release whose notes show.
    fn release<'a>(&'a self, updater: &'a Updater) -> Option<&'a ReleaseEntry> {
        match self.view {
            View::Notes => self.history.get(self.selected).copied(),
            // An offered version's notes; without an offer, the installed release's (as
            // the old update view showed the installed history).
            View::Updates => match updater.state().version {
                Some(_) => updater.release().or(self.offered.as_ref()),
                None => self.history.first().copied(),
            },
        }
    }

    /// The images of the shown release.
    fn images<'a>(&'a self, updater: &'a Updater) -> &'a [ReleaseImage] {
        self.release(updater)
            .and_then(|r| r.changes.images.as_deref())
            .unwrap_or(&[])
    }

    /// Steps the enlarged image by `step`.
    pub fn step_viewer(&mut self, updater: &Updater, step: isize) {
        let n = self.images(updater).len() as isize;
        if let (Some(i), true) = (self.viewer, n > 0) {
            self.viewer = Some((i as isize + step).rem_euclid(n) as usize);
        }
    }

    pub fn open_viewer(&mut self, updater: &Updater, index: usize) {
        if index < self.images(updater).len() {
            self.viewer = Some(index);
        }
    }

    /// The main menu's plaque.
    pub fn show_plaque(&self, ui: &mut Ui, updater: &Updater) {
        widgets::text(
            ui,
            "main-menu/release/version/number",
            &format!("v{}", self.installed),
        );
        let (status, tooltip) = plaque_status(updater.state());
        widgets::text(ui, "main-menu/release/version/status", &status);
        if let Some(id) = ui.find("main-menu/release/version") {
            ui.set_tooltip(id, Some(tooltip));
        }
    }

    /// The dialog, once per frame while it is open.
    pub fn show(&mut self, host: &mut UiHost, gpu: &Gpu, updater: &Updater) {
        self.gallery.poll(host, gpu);
        let state = updater.state().clone();
        if self.offered.as_ref().map(|r| r.version) != state.version {
            self.offered = state
                .version
                .zip(state.notes.as_deref())
                .and_then(|(v, notes)| from_plain(v, notes));
        }
        let notes = self.view == View::Notes;
        let installed = self.installed;
        {
            let ui = &mut host.ui;
            widgets::text(
                ui,
                "releases/title",
                &if notes {
                    tr!("releases.whats-new")
                } else {
                    tr!("releases.updates")
                },
            );
            let subtitle = if notes {
                tr!("releases.notes-subtitle", version = &installed.to_string())
            } else {
                tr!("releases.installed", version = &installed.to_string())
            };
            widgets::text(ui, "releases/subtitle", &subtitle);
            widgets::visible(ui, "releases/side", notes);
            if notes {
                self.show_timeline(ui);
            }
            widgets::visible(ui, "releases/main/status", !notes);
            if !notes {
                show_status(ui, &state, installed);
            }
            widgets::text(
                ui,
                "releases/hint",
                &if notes {
                    tr!("releases.notes-hint")
                } else {
                    tr!("releases.updates-hint")
                },
            );
            let action = if notes { None } else { primary_action(&state) };
            widgets::visible(ui, "releases/primary", action.is_some());
            if let Some(action) = action {
                widgets::text(ui, "releases/primary", &action);
            }
            let closable = Self::closable(updater);
            widgets::enabled(ui, "releases/close", closable);
            widgets::enabled(ui, "releases/done", closable);
        }
        // The article: rebuilt when what it shows changes.
        let key = match (self.view, self.release(updater)) {
            (View::Notes, Some(r)) => format!("notes:{}", r.version),
            (View::Updates, Some(r)) => format!("update:{}", r.version),
            (View::Updates, None) => match state.version {
                Some(v) => format!("plain:{v}"),
                None => "none".into(),
            },
            (View::Notes, None) => "empty".into(),
        };
        if key != self.shown {
            self.shown = key;
            let release = self.release(updater).cloned();
            let ui = &mut host.ui;
            match &release {
                Some(r) => fill_release(ui, r, r.version == installed),
                None => fill_plain(ui, &state, notes),
            }
            if let Some(main) = ui.find("releases/main") {
                ui.scroll_to_top(main);
            }
        }
        // The gallery's few image entries, not the whole release, each frame.
        let shown = self
            .release(updater)
            .map(|r| (r.version, r.changes.images.clone().unwrap_or_default()));
        match shown {
            Some((version, images)) => self.show_images(host, Some(version), &images),
            None => self.show_images(host, None, &[]),
        }
    }

    fn show_timeline(&self, ui: &mut Ui) {
        let Some(list) = widgets::find(ui, "releases/side") else {
            return;
        };
        let n = self.history.len();
        ui.set_count(list, n);
        for (i, r) in self.history.iter().enumerate() {
            let base = format!("releases/side/{i}");
            widgets::text(ui, &format!("{base}/version"), &format!("v{}", r.version));
            widgets::visible(
                ui,
                &format!("{base}/installed"),
                r.version == self.installed,
            );
            widgets::text(ui, &format!("{base}/title"), &plain(&r.title));
            widgets::text(ui, &format!("{base}/date"), &date(&r.published_at));
            widgets::visible(ui, &format!("{base}/up"), i > 0);
            widgets::visible(ui, &format!("{base}/down"), i + 1 < n);
            widgets::checked(ui, &base, i == self.selected);
        }
    }

    /// The gallery thumbnails and the enlarged image, as their pixels arrive.
    fn show_images(
        &mut self,
        host: &mut UiHost,
        version: Option<Version>,
        images: &[ReleaseImage],
    ) {
        let list = if images.len() == 1 { "wide" } else { "gallery" };
        for (i, image) in images.iter().enumerate() {
            if let Some(v) = version {
                self.gallery.request(v, image);
            }
            let base = format!("releases/main/release/{list}/{i}");
            let slot = self.gallery.slot(&image.file);
            let ui = &mut host.ui;
            let (key, missing) = match slot {
                Some(Slot::Ready { thumb, .. }) => (Some(*thumb), None),
                Some(Slot::Missing) => (None, Some(tr!("releases.image-unavailable"))),
                _ => (None, Some(tr!("releases.loading-image"))),
            };
            if let Some(id) = ui.find(&format!("{base}/frame/image")) {
                ui.set_image(id, key);
            }
            widgets::visible(ui, &format!("{base}/frame/missing"), missing.is_some());
            if let Some(text) = missing {
                widgets::text(ui, &format!("{base}/frame/missing"), &text);
            }
        }
        let ui = &mut host.ui;
        let viewer = self.viewer.filter(|&i| i < images.len());
        widgets::visible(ui, "releases/viewer", viewer.is_some());
        if let Some(i) = viewer {
            let image = &images[i];
            let (key, missing) = match self.gallery.slot(&image.file) {
                Some(Slot::Ready { full, .. }) => (Some(*full), false),
                _ => (None, true),
            };
            if let Some(id) = ui.find("releases/viewer/image") {
                ui.set_image(id, key);
            }
            widgets::visible(ui, "releases/viewer/missing", missing);
            let caption = image.caption.as_deref().unwrap_or(&image.alt);
            widgets::text(ui, "releases/viewer/caption", &plain(caption));
            widgets::text(
                ui,
                "releases/viewer/counter",
                &format!("{} / {}", i + 1, images.len()),
            );
            widgets::enabled(ui, "releases/viewer/previous", images.len() > 1);
            widgets::enabled(ui, "releases/viewer/next", images.len() > 1);
        }
    }
}

/// The update view's status card.
fn show_status(ui: &mut Ui, state: &State, installed: Version) {
    let (glyph, color) = match state.phase {
        Phase::Current => ("✓", [0x8f, 0xd1, 0x8a]),
        Phase::Error => ("!", [0xe9, 0x8b, 0x6d]),
        Phase::Ready | Phase::Restart => ("✓", [0xe9, 0xb6, 0x5c]),
        Phase::Available | Phase::Downloading => ("↓", [0xe9, 0xb6, 0x5c]),
        _ => ("↻", [0x9a, 0xa8, 0x9e]),
    };
    widgets::text(ui, "releases/main/status/emblem", glyph);
    if let Some(id) = ui.find("releases/main/status/emblem") {
        ui.set_color(id, Some(rgb(color, 255)));
    }
    widgets::text(
        ui,
        "releases/main/status/headline",
        &headline(state, installed),
    );
    let detail = match state.phase {
        Phase::Available => state
            .blocked
            .clone()
            .unwrap_or_else(|| tr!("releases.download-note")),
        Phase::Downloading => match state.total {
            Some(total) => tr!(
                "releases.progress",
                received = &megabytes(state.received),
                total = &megabytes(total)
            ),
            None => megabytes(state.received),
        },
        Phase::Ready => tr!("releases.verified"),
        Phase::Checking | Phase::Installing | Phase::Restart => String::new(),
        Phase::Idle | Phase::Current | Phase::Error => String::new(),
    };
    widgets::visible(ui, "releases/main/status/detail", !detail.is_empty());
    widgets::text(ui, "releases/main/status/detail", &detail);
    let downloading = state.phase == Phase::Downloading;
    widgets::visible(ui, "releases/main/status/progress", downloading);
    if let Some(bar) = ui.find("releases/main/status/progress") {
        let value = match state.total {
            Some(total) if total > 0 => state.received as f32 / total as f32,
            _ => 0.0,
        };
        ui.set_value(bar, value.clamp(0.0, 1.0));
    }
    widgets::visible(ui, "releases/main/status/error", state.error.is_some());
    widgets::text(
        ui,
        "releases/main/status/error",
        state.error.as_deref().unwrap_or(""),
    );
}

/// Fills the article with a release: meta, headline, summary, gallery, notes by kind.
fn fill_release(ui: &mut Ui, r: &ReleaseEntry, installed: bool) {
    widgets::visible(ui, "releases/main/release", true);
    widgets::text(
        ui,
        "releases/main/release/badge",
        &format!("v{}", r.version),
    );
    widgets::text(ui, "releases/main/release/date", &date(&r.published_at));
    widgets::visible(ui, "releases/main/release/date", !r.published_at.is_empty());
    widgets::visible(ui, "releases/main/release/installed", installed);
    widgets::text(ui, "releases/main/release/headline", &plain(&r.title));
    widgets::visible(ui, "releases/main/release/plain", false);
    let kinds: Vec<ChangeKind> = ChangeKind::ALL
        .into_iter()
        .filter(|&k| !r.changes.notes(k).is_empty())
        .collect();
    if let Some(list) = ui.find("releases/main/release/summary") {
        ui.set_count(list, kinds.len());
    }
    widgets::visible(ui, "releases/main/release/summary", !kinds.is_empty());
    for (i, &kind) in kinds.iter().enumerate() {
        let base = format!("releases/main/release/summary/{i}");
        let (_, _, color) = tone(kind);
        if let Some(dot) = ui.find(&format!("{base}/dot")) {
            ui.set_color(dot, Some(rgb(color, 255)));
        }
        widgets::text(
            ui,
            &format!("{base}/text"),
            &count(kind, r.changes.notes(kind).len()),
        );
    }
    let images = r.changes.images.as_deref().unwrap_or(&[]);
    let (grid, wide) = if images.len() == 1 {
        (0, 1)
    } else {
        (images.len(), 0)
    };
    for (list, n) in [("gallery", grid), ("wide", wide)] {
        let path = format!("releases/main/release/{list}");
        if let Some(id) = ui.find(&path) {
            ui.set_count(id, n);
        }
        widgets::visible(ui, &path, n > 0);
        for (i, image) in images.iter().enumerate().take(n) {
            let caption = format!("{path}/{i}/caption");
            widgets::visible(ui, &caption, image.caption.is_some());
            widgets::text(ui, &caption, &plain(image.caption.as_deref().unwrap_or("")));
        }
    }
    if let Some(list) = ui.find("releases/main/release/groups") {
        ui.set_count(list, kinds.len());
    }
    widgets::visible(ui, "releases/main/release/groups", !kinds.is_empty());
    for (i, &kind) in kinds.iter().enumerate() {
        let base = format!("releases/main/release/groups/{i}");
        let (label, glyph, color) = tone(kind);
        let notes = r.changes.notes(kind);
        widgets::text(ui, &format!("{base}/icon"), glyph);
        if let Some(icon) = ui.find(&format!("{base}/icon")) {
            ui.set_color(icon, Some(rgb(color, 255)));
        }
        widgets::text(
            ui,
            &format!("{base}/label"),
            &format!("[color={}]{label}[/color]", hex(color)),
        );
        widgets::text(ui, &format!("{base}/count"), &notes.len().to_string());
        if let Some(list) = ui.find(&format!("{base}/notes")) {
            ui.set_count(list, notes.len());
        }
        for (j, note) in notes.iter().enumerate() {
            let entry = format!("{base}/notes/{j}");
            if let Some(bullet) = ui.find(&format!("{entry}/bullet")) {
                ui.set_color(bullet, Some(rgb(color, 0xcc)));
            }
            widgets::text(ui, &format!("{entry}/text"), &plain(note));
        }
    }
}

/// The article without structured notes: the feed's plain notes of an offered update,
/// or nothing.
fn fill_plain(ui: &mut Ui, state: &State, notes_view: bool) {
    let Some(version) = state.version.filter(|_| !notes_view) else {
        widgets::visible(ui, "releases/main/release", false);
        return;
    };
    widgets::visible(ui, "releases/main/release", true);
    widgets::text(ui, "releases/main/release/badge", &format!("v{version}"));
    widgets::text(ui, "releases/main/release/date", "");
    widgets::visible(ui, "releases/main/release/installed", false);
    widgets::text(
        ui,
        "releases/main/release/headline",
        &tr!("releases.version", version = &version.to_string()),
    );
    for list in ["summary", "gallery", "wide", "groups"] {
        let path = format!("releases/main/release/{list}");
        if let Some(id) = ui.find(&path) {
            ui.set_count(id, 0);
        }
        widgets::visible(ui, &path, false);
    }
    let text = state
        .notes
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .map_or_else(|| tr!("releases.new-version-ready"), str::to_string);
    widgets::visible(ui, "releases/main/release/plain", true);
    widgets::text(ui, "releases/main/release/plain", &plain(&text));
}

/// Widest thumbnail kept (the dialog shows them at up to about 700 logical pixels).
const THUMB_WIDTH: u32 = 1000;

/// A gallery image's state.
pub enum Slot {
    Loading,
    Ready { thumb: ImageKey, full: ImageKey },
    Missing,
}

struct Decoded {
    file: String,
    /// Thumbnail and full image (width, height, RGBA).
    pixels: Option<[(u32, u32, Vec<u8>); 2]>,
    /// Where to try next when the bundled copy is missing (the release host).
    fallback: Option<String>,
}

/// Release gallery images: the bundled copy (`releases/media/` in the data folder, Git
/// LFS) first, else the release host's (old `releaseImageUrl`), decoded off the window
/// thread. A missing image shows as unavailable; the notes never wait for it.
pub struct Gallery {
    media: PathBuf,
    slots: BTreeMap<String, Slot>,
    tx: mpsc::Sender<Decoded>,
    rx: mpsc::Receiver<Decoded>,
    http: Option<Http>,
    /// Never reach the release host (captures).
    pub offline: bool,
}

/// A gallery file name: 64 lowercase hex digits and `.webp` (old `releaseImage`).
fn valid_file(file: &str) -> bool {
    file.strip_suffix(".webp").is_some_and(|stem| {
        stem.len() == 64 && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn decode(bytes: &[u8]) -> Option<[(u32, u32, Vec<u8>); 2]> {
    let full = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = full.dimensions();
    let thumb = if w > THUMB_WIDTH {
        let th = (h as f32 * THUMB_WIDTH as f32 / w as f32).round().max(1.0) as u32;
        image::imageops::resize(
            &full,
            THUMB_WIDTH,
            th,
            image::imageops::FilterType::Triangle,
        )
    } else {
        full.clone()
    };
    let (tw, th) = thumb.dimensions();
    Some([(tw, th, thumb.into_raw()), (w, h, full.into_raw())])
}

impl Gallery {
    pub fn new(data: &Path) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            media: data.join("releases/media"),
            slots: BTreeMap::new(),
            tx,
            rx,
            http: None,
            offline: false,
        }
    }

    pub fn slot(&self, file: &str) -> Option<&Slot> {
        self.slots.get(file)
    }

    /// Starts loading an image of `version` once.
    pub fn request(&mut self, version: Version, image: &ReleaseImage) {
        if self.slots.contains_key(&image.file) {
            return;
        }
        if !valid_file(&image.file) {
            self.slots.insert(image.file.clone(), Slot::Missing);
            return;
        }
        self.slots.insert(image.file.clone(), Slot::Loading);
        let path = self.media.join(&image.file);
        let file = image.file.clone();
        let fallback =
            (!self.offline).then(|| format!("{BASE_URL}/releases/{version}/media/{}", image.file));
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name("canopy-gallery".into())
            .spawn(move || {
                let pixels = std::fs::read(&path).ok().and_then(|b| decode(&b));
                let fallback = if pixels.is_some() { None } else { fallback };
                let _ = tx.send(Decoded {
                    file,
                    pixels,
                    fallback,
                });
            });
        if spawned.is_err() {
            self.slots.insert(image.file.clone(), Slot::Missing);
        }
    }

    /// Binds the images decoded since the last frame.
    pub fn poll(&mut self, host: &mut UiHost, gpu: &Gpu) {
        while let Ok(decoded) = self.rx.try_recv() {
            let Decoded {
                file,
                pixels,
                fallback,
            } = decoded;
            if let Some([(tw, th, thumb), (w, h, full)]) = pixels {
                let thumb = host.rgba_image(gpu, &format!("release-media/{file}"), tw, th, &thumb);
                let full = host.rgba_image(gpu, &format!("release-media/{file}#full"), w, h, &full);
                let slot = match (thumb, full) {
                    (Some(thumb), Some(full)) => Slot::Ready { thumb, full },
                    _ => Slot::Missing,
                };
                self.slots.insert(file, slot);
            } else if let Some(url) = fallback {
                self.fetch(file, url);
            } else {
                self.slots.insert(file, Slot::Missing);
            }
        }
    }

    fn fetch(&mut self, file: String, url: String) {
        if self.http.is_none() {
            self.http = Http::new().ok();
        }
        let Some(http) = &self.http else {
            self.slots.insert(file, Slot::Missing);
            return;
        };
        let tx = self.tx.clone();
        http.get(&url, Duration::from_secs(20), move |body| {
            // Decoding takes a moment: off the network thread.
            let _ = std::thread::Builder::new()
                .name("canopy-gallery".into())
                .spawn(move || {
                    let pixels = body.ok().and_then(|b| decode(&b));
                    let _ = tx.send(Decoded {
                        file,
                        pixels,
                        fallback: None,
                    });
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_as_the_old_menu_showed_them() {
        assert_eq!(date("2026-10-13T15:44:03.334Z"), "Oct 13, 2026");
        assert_eq!(date("2026-01-05T00:00:00Z"), "Jan 5, 2026");
        assert_eq!(date("soon"), "soon");
    }

    #[test]
    fn the_plaque_never_claims_latest_after_a_failed_check() {
        let mut state = State {
            phase: Phase::Error,
            ..State::default()
        };
        assert_eq!(plaque_status(&state).0, "Check unavailable");
        assert_eq!(primary_action(&state).as_deref(), Some("Try again"));
        state.phase = Phase::Current;
        assert!(plaque_status(&state).0.starts_with("Latest"));
        state.phase = Phase::Available;
        state.version = Some("0.3.1".parse().unwrap());
        assert!(plaque_status(&state).0.contains("Update available"));
        assert_eq!(primary_action(&state).as_deref(), Some("Download update"));
        state.blocked = Some("dev build".into());
        assert_eq!(primary_action(&state), None);
        state.phase = Phase::Ready;
        assert_eq!(
            primary_action(&state).as_deref(),
            Some("Install and restart")
        );
    }

    #[test]
    fn plain_notes_read_back_by_kind() {
        let v: Version = "0.3.1".parse().unwrap();
        let mut entry = ReleaseEntry {
            changes: Default::default(),
            version: v,
            published_at: String::new(),
            title: "Faster [battles]".into(),
        };
        entry.changes.added.push("Teams".into());
        entry
            .changes
            .known_issues
            .push("Old saves - do not load".into());
        assert_eq!(from_plain(v, &entry.plain_notes()), Some(entry));
        assert_eq!(from_plain(v, "Just a sentence\nand another"), None);
        assert_eq!(from_plain(v, ""), None);
    }

    #[test]
    fn gallery_files_are_content_addressed_webp() {
        assert!(valid_file(&format!("{}.webp", "a".repeat(64))));
        assert!(!valid_file("../../etc/passwd"));
        assert!(!valid_file(&format!("{}.png", "a".repeat(64))));
    }
}
