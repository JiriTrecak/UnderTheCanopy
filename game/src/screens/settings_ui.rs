//! The settings dialog: graphics, audio, interface, keyboard shortcuts and the match
//! server, applied as they change and written when the dialog closes.
//!
//! The old game showed one stacked section with a collapsible shortcut list; with more
//! settings here, the dialog has tabs (review).

use super::{ui_host::UiHost, widgets};
use crate::settings::{
    Chord, Haze, RESOLUTIONS, Settings, WindowMode,
    keys::{BindError, Entry},
};
use canopy_audio::Bus;
use canopy_ui::{Action, ActionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Graphics,
    Audio,
    Interface,
    Keys,
    Network,
}

const TABS: [(Tab, &str, &str); 5] = [
    (Tab::Graphics, "tab-graphics", "graphics"),
    (Tab::Audio, "tab-audio", "audio"),
    (Tab::Interface, "tab-interface", "interface"),
    (Tab::Keys, "tab-keys", "keys"),
    (Tab::Network, "tab-network", "network"),
];

/// What a settings change touches, for the app to apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changed {
    pub graphics: bool,
    pub vsync: bool,
    pub window: bool,
    pub audio: bool,
    pub hud: bool,
    pub server: bool,
    pub any: bool,
}

#[derive(Default)]
pub struct SettingsUi {
    pub tab: Tab,
    /// The shortcut waiting for a key.
    pub capturing: Option<String>,
    pub message: String,
    /// The shortcut search (old "Find a shortcut"): entries whose name contains it.
    pub filter: String,
    /// Fields need their text pushed (opening the dialog).
    fresh: bool,
}

impl SettingsUi {
    /// The catalogue entries the keyboard list shows, by catalogue index.
    pub fn shown(&self, catalog: &[Entry]) -> Vec<usize> {
        let needle = self.filter.trim().to_lowercase();
        (0..catalog.len())
            .filter(|&i| needle.is_empty() || catalog[i].label.to_lowercase().contains(&needle))
            .collect()
    }
}

fn volume_id(bus: Option<Bus>) -> &'static str {
    match bus {
        None => "master",
        Some(Bus::Effects) => "effects",
        Some(Bus::Interface) => "interface",
        Some(Bus::Music) => "music",
    }
}

impl SettingsUi {
    pub fn opened(&mut self) {
        self.fresh = true;
        self.capturing = None;
        self.message.clear();
        self.filter.clear();
    }

    pub fn show(&mut self, host: &mut UiHost, settings: &Settings, catalog: &[Entry]) {
        let ui = &mut host.ui;
        for (tab, button, page) in TABS {
            widgets::checked(ui, &format!("settings/{button}"), self.tab == tab);
            widgets::visible(ui, &format!("settings/{page}"), self.tab == tab);
        }
        let g = &settings.graphics;
        let mut resolutions = vec!["Native".to_string()];
        resolutions.extend(RESOLUTIONS.iter().map(|[w, h]| format!("{w} × {h}")));
        let res = g
            .resolution
            .and_then(|r| RESOLUTIONS.iter().position(|x| *x == r))
            .map_or(0, |i| i + 1);
        widgets::options(ui, "settings/graphics/resolution", &resolutions, res);
        if let Some(n) = ui.find("settings/graphics/render-scale") {
            ui.set_value(n, (g.render_scale * 100.0).round());
        }
        widgets::text(
            ui,
            "settings/graphics/render-scale-value",
            &format!("{}%", (g.render_scale * 100.0).round()),
        );
        let haze: Vec<String> = Haze::ALL.iter().map(|h| h.label().to_string()).collect();
        let haze_index = Haze::ALL.iter().position(|h| *h == g.haze).unwrap_or(2);
        widgets::options(ui, "settings/graphics/haze", &haze, haze_index);
        widgets::checked(ui, "settings/graphics/shadows", g.shadows);
        widgets::checked(ui, "settings/graphics/contact", g.contact_shadows);
        widgets::checked(ui, "settings/graphics/bloom", g.bloom);
        widgets::checked(ui, "settings/graphics/vsync", g.vsync);
        widgets::options(
            ui,
            "settings/graphics/window",
            &["Window".to_string(), "Fullscreen".to_string()],
            usize::from(g.window == WindowMode::Fullscreen),
        );
        let a = &settings.audio;
        for (bus, value) in [
            (None, a.master),
            (Some(Bus::Effects), a.effects),
            (Some(Bus::Interface), a.interface),
            (Some(Bus::Music), a.music),
        ] {
            let id = volume_id(bus);
            if let Some(n) = ui.find(&format!("settings/audio/{id}")) {
                ui.set_value(n, (value * 100.0).round());
            }
            widgets::text(
                ui,
                &format!("settings/audio/{id}-value"),
                &format!("{}%", (value * 100.0).round()),
            );
        }
        widgets::checked(ui, "settings/audio/mute", a.mute_in_background);
        if let Some(n) = ui.find("settings/interface/hud-scale") {
            ui.set_value(n, settings.hud.scale as f32);
        }
        widgets::text(
            ui,
            "settings/interface/hud-scale-value",
            &format!("{}%", settings.hud.scale),
        );
        widgets::options(
            ui,
            "settings/interface/hud-layout",
            &[
                "Spread · screen edges".to_string(),
                "Compact · centered".to_string(),
            ],
            usize::from(settings.hud.layout == crate::settings::HudLayout::Compact),
        );
        widgets::checked(
            ui,
            "settings/interface/health-bars",
            settings.hud.always_show_health_bars,
        );
        widgets::checked(ui, "settings/interface/tooltips", settings.hud.tooltips);
        if self.fresh {
            widgets::text(ui, "settings/interface/name", &settings.player_name);
            widgets::text(ui, "settings/keys/search", &self.filter);
            widgets::text(
                ui,
                "settings/network/server",
                settings.multiplayer.server.as_deref().unwrap_or(""),
            );
            self.fresh = false;
        }
        let server_note = match canopy_net::ServerAddress::resolve(
            settings.multiplayer.server.as_deref(),
        ) {
            Ok(address) => format!(
                "Lobbies and matches go to {}. CANOPY_MATCH_SERVER overrides this. Plain http:// is allowed only for a local test server.",
                address.http("").trim_end_matches('/')
            ),
            Err(e) => format!("Not a server address: {e}"),
        };
        widgets::text(ui, "settings/network/server-note", &server_note);
        // Shortcuts.
        let conflicts = settings.keys.conflicts(catalog);
        if let Some(list) = ui.find("settings/keys/list") {
            let shown = self.shown(catalog);
            ui.set_count(list, shown.len());
            let mut group = "";
            for (i, entry) in shown.iter().map(|&c| &catalog[c]).enumerate() {
                let base = format!("settings/keys/list/{i}");
                let first = entry.group != group;
                group = entry.group;
                widgets::visible(ui, &format!("{base}/group"), first);
                if first {
                    widgets::text(ui, &format!("{base}/group"), entry.group);
                }
                let clash = conflicts
                    .iter()
                    .any(|(a, b)| a.id == entry.id || b.id == entry.id);
                let name = if clash {
                    format!("{}  [color=#f5a07e](shared key)[/color]", entry.label)
                } else {
                    entry.label.clone()
                };
                widgets::text(ui, &format!("{base}/name"), &name);
                let capturing = self.capturing.as_deref() == Some(entry.id.as_str());
                let key = if capturing {
                    "Press a key…".to_string()
                } else {
                    settings
                        .keys
                        .chord(catalog, &entry.id)
                        .map_or_else(|| "Unbound".to_string(), Chord::label)
                };
                widgets::text(ui, &format!("{base}/key"), &key);
                widgets::checked(ui, &format!("{base}/key"), capturing);
            }
        }
        widgets::text(ui, "settings/keys/message", &self.message);
        widgets::visible(ui, "settings/keys/message", !self.message.is_empty());
    }

    /// A key pressed while capturing; `true` when it was taken.
    pub fn capture(
        &mut self,
        settings: &mut Settings,
        catalog: &[Entry],
        chord: Option<Chord>,
    ) -> bool {
        let Some(id) = self.capturing.take() else {
            return false;
        };
        let Some(chord) = chord else {
            self.message = BindError::Unsupported.to_string();
            return true;
        };
        if chord.code == "Escape" && !chord.ctrl && !chord.alt && !chord.shift && !chord.meta {
            self.message.clear();
            return true;
        }
        match settings.keys.bind(catalog, &id, chord) {
            Ok(()) => self.message = "Shortcut saved.".into(),
            Err(e) => self.message = e.to_string(),
        }
        true
    }

    /// Handles a `settings.*` action. Returns what changed.
    pub fn action(
        &mut self,
        action: &Action,
        settings: &mut Settings,
        catalog: &[Entry],
    ) -> Changed {
        let name = action.name.as_deref().unwrap_or("");
        let mut c = Changed::default();
        let value = |kind: &ActionKind| match kind {
            ActionKind::Value(v) => Some(*v),
            _ => None,
        };
        let toggled = |kind: &ActionKind| match kind {
            ActionKind::Toggled(v) => Some(*v),
            _ => None,
        };
        let selected = |kind: &ActionKind| match kind {
            ActionKind::Selected(i) => Some(*i),
            _ => None,
        };
        let text = |kind: &ActionKind| match kind {
            ActionKind::TextChanged(t) | ActionKind::Submitted(t) => Some(t.clone()),
            _ => None,
        };
        let path = action.path.as_deref().unwrap_or("");
        match name {
            "settings.tab" => {
                if let Some((tab, ..)) = TABS.iter().find(|(_, button, _)| path.ends_with(button)) {
                    self.tab = *tab;
                    self.capturing = None;
                }
            }
            "settings.resolution" => {
                if let Some(i) = selected(&action.kind) {
                    settings.graphics.resolution =
                        i.checked_sub(1).and_then(|i| RESOLUTIONS.get(i).copied());
                    c.graphics = true;
                }
            }
            "settings.render-scale" => {
                if let Some(v) = value(&action.kind) {
                    settings.graphics.render_scale = (v / 100.0).clamp(0.5, 1.0);
                    c.graphics = true;
                }
            }
            "settings.haze" => {
                if let Some(h) = selected(&action.kind).and_then(|i| Haze::ALL.get(i)) {
                    settings.graphics.haze = *h;
                    c.graphics = true;
                }
            }
            "settings.shadows" | "settings.contact" | "settings.bloom" => {
                if let Some(on) = toggled(&action.kind) {
                    match name {
                        "settings.shadows" => settings.graphics.shadows = on,
                        "settings.contact" => settings.graphics.contact_shadows = on,
                        _ => settings.graphics.bloom = on,
                    }
                    c.graphics = true;
                }
            }
            "settings.vsync" => {
                if let Some(on) = toggled(&action.kind) {
                    settings.graphics.vsync = on;
                    c.vsync = true;
                }
            }
            "settings.window" => {
                if let Some(i) = selected(&action.kind) {
                    settings.graphics.window = if i == 1 {
                        WindowMode::Fullscreen
                    } else {
                        WindowMode::Windowed
                    };
                    c.window = true;
                }
            }
            "settings.volume" => {
                if let Some(v) = value(&action.kind) {
                    let v = (v / 100.0).clamp(0.0, 1.0);
                    let a = &mut settings.audio;
                    if path.ends_with("master") {
                        a.master = v;
                    } else if path.ends_with("effects") {
                        a.set_bus(Bus::Effects, v);
                    } else if path.ends_with("interface") {
                        a.set_bus(Bus::Interface, v);
                    } else if path.ends_with("music") {
                        a.set_bus(Bus::Music, v);
                    }
                    c.audio = true;
                }
            }
            "settings.mute" => {
                if let Some(on) = toggled(&action.kind) {
                    settings.audio.mute_in_background = on;
                    c.audio = true;
                }
            }
            "settings.name" => {
                if let Some(t) = text(&action.kind) {
                    settings.player_name = t.chars().take(crate::settings::MAX_NAME).collect();
                }
            }
            "settings.hud-layout" => {
                if let Some(i) = selected(&action.kind) {
                    settings.hud.layout = if i == 1 {
                        crate::settings::HudLayout::Compact
                    } else {
                        crate::settings::HudLayout::Spread
                    };
                    c.hud = true;
                }
            }
            "settings.hud-scale" => {
                if let Some(v) = value(&action.kind) {
                    settings.hud.scale = v.round().clamp(30.0, 100.0) as u32;
                    c.hud = true;
                }
            }
            "settings.health-bars" => {
                if let Some(on) = toggled(&action.kind) {
                    settings.hud.always_show_health_bars = on;
                    c.hud = true;
                }
            }
            "settings.tooltips" => {
                if let Some(on) = toggled(&action.kind) {
                    settings.hud.tooltips = on;
                    c.hud = true;
                }
            }
            "settings.key-search" => {
                if let Some(t) = text(&action.kind) {
                    self.filter = t;
                }
                return c;
            }
            "settings.key" => {
                let shown = self.shown(catalog);
                if let Some(entry) = action
                    .index
                    .and_then(|i| shown.get(i))
                    .map(|&c| &catalog[c])
                {
                    self.capturing = Some(entry.id.clone());
                    self.message.clear();
                }
            }
            "settings.key-clear" => {
                let shown = self.shown(catalog);
                if let Some(entry) = action
                    .index
                    .and_then(|i| shown.get(i))
                    .map(|&c| &catalog[c])
                {
                    settings.keys.clear(catalog, &entry.id);
                    self.capturing = None;
                }
            }
            "settings.keys-defaults" => {
                settings.keys.restore_defaults();
                self.capturing = None;
                self.message = "Default shortcuts restored.".into();
            }
            "settings.server" => {
                if let Some(t) = text(&action.kind) {
                    let t = t.trim();
                    settings.multiplayer.server = (!t.is_empty()).then(|| t.to_string());
                    c.server = true;
                }
            }
            "settings.server-default" => {
                settings.multiplayer.server = None;
                self.fresh = true;
                c.server = true;
            }
            _ => return c,
        }
        c.any = true;
        c
    }
}
