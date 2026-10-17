//! The replays screen (D60, `layouts/replays.json`): the recorded matches in the replays
//! folder, newest first, with the selected one's details; Watch, Rename, Delete and Show
//! in Finder. Replays this game cannot play (other content, a newer format, a missing or
//! changed map) are listed but cannot be watched, with the reason.

use super::{ui_host::UiHost, widgets, widgets::Palette};
use crate::data::GameData;
use crate::replay::{
    self,
    format::Header,
    library::{Entry, Library},
};
use canopy_render::Gpu;

#[derive(Debug, Default)]
pub struct Replays {
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub status: String,
    /// Renaming the selected replay: the field shows.
    pub renaming: bool,
    pub rename_text: String,
}

/// Why `header` cannot be watched with `data`, if it cannot.
pub fn unplayable(header: &Header, data: &GameData) -> Option<String> {
    if header.identity.content_digest != data.content_digest {
        return Some(if header.game_version == env!("CARGO_PKG_VERSION") {
            format!("Recorded with other game content ({})", header.game_version)
        } else {
            format!("Recorded in {}", header.game_version)
        });
    }
    if header.map_included {
        return None;
    }
    match data.map(&header.identity.map_id) {
        None => Some(format!("The map {} is not in this game", header.map_name)),
        Some(map) => {
            let digest = map.identity().map(|d| crate::saves::digest(&d)).ok();
            (digest.as_deref() != Some(header.identity.map_digest.as_str()))
                .then(|| format!("The map {} has changed since", header.map_name))
        }
    }
}

/// Names and races of the players in a line.
fn players_line(header: &Header) -> String {
    header
        .players
        .iter()
        .map(|p| p.name.clone())
        .collect::<Vec<_>>()
        .join(" · ")
}

impl Replays {
    /// Reads the folder again, keeping the selection by name where it can.
    pub fn refresh(&mut self, library: &Library) {
        let selected = self.entries.get(self.selected).map(|e| e.name.clone());
        self.entries = library.list();
        self.selected = selected
            .and_then(|n| self.entries.iter().position(|e| e.name == n))
            .unwrap_or(0);
        self.renaming = false;
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// Whether the selected replay can be watched; the reason when it cannot.
    pub fn watchable(&self, data: &GameData) -> Result<(), String> {
        let entry = self.selected().ok_or("No replay selected")?;
        let header = entry.header.as_ref().map_err(Clone::clone)?;
        match unplayable(header, data) {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    pub fn show(&self, host: &mut UiHost, gpu: &Gpu, data: &GameData, palette: &Palette) {
        if let Some(list) = host.ui.find("replays/list") {
            host.ui.set_count(list, self.entries.len());
        }
        widgets::text(
            &mut host.ui,
            "replays/count",
            &match self.entries.len() {
                0 => String::new(),
                1 => "1 replay".into(),
                n => format!("{n} replays"),
            },
        );
        widgets::visible(&mut host.ui, "replays/empty", self.entries.is_empty());
        for (i, entry) in self.entries.iter().enumerate() {
            let base = format!("replays/list/{i}");
            let (meta, players, thumb, playable) = match &entry.header {
                Ok(h) => (
                    format!(
                        "{} · {} · {} · {}",
                        h.map_name,
                        crate::saves::format_time(h.recorded_at),
                        replay::clock(h.duration()),
                        h.result
                    ),
                    players_line(h),
                    data.map(&h.identity.map_id).and_then(|m| m.preview.clone()),
                    unplayable(h, data).is_none(),
                ),
                Err(_) => ("Unreadable".into(), String::new(), None, false),
            };
            let image = thumb.and_then(|p| host.pack_image(gpu, &data.pack, &p));
            if let Some(node) = host.ui.find(&format!("{base}/thumb")) {
                host.ui.set_image(node, image);
            }
            let ui = &mut host.ui;
            widgets::text(ui, &format!("{base}/name"), &plain(&entry.name));
            widgets::text(ui, &format!("{base}/meta"), &plain(&meta));
            widgets::text(ui, &format!("{base}/players"), &plain(&players));
            widgets::checked(ui, &base, i == self.selected);
            if let Some(node) = ui.find(&base) {
                ui.set_opacity(node, if playable { 1.0 } else { 0.55 });
            }
        }
        self.show_details(host, gpu, data, palette);
    }

    fn show_details(&self, host: &mut UiHost, gpu: &Gpu, data: &GameData, palette: &Palette) {
        let entry = self.selected();
        let header = entry.and_then(|e| e.header.as_ref().ok());
        let preview = header
            .and_then(|h| data.map(&h.identity.map_id))
            .and_then(|m| m.preview.clone())
            .and_then(|p| host.pack_image(gpu, &data.pack, &p));
        if let Some(node) = host.ui.find("replays/preview/image") {
            host.ui.set_image(node, preview);
        }
        let ui = &mut host.ui;
        widgets::visible(ui, "replays/preview/empty", preview.is_none());
        widgets::text(
            ui,
            "replays/title",
            &entry.map_or_else(String::new, |e| plain(&e.name)),
        );
        let meta = match (entry, header) {
            (Some(e), Some(h)) => format!(
                "{} · {} · {}\n{} · {} long · version {} · {} KB",
                h.mode.name(),
                plain(&h.map_name),
                h.result,
                crate::saves::format_time(h.recorded_at),
                replay::clock(h.duration()),
                h.game_version,
                e.bytes.div_ceil(1024)
            ),
            (Some(e), None) => format!(
                "This file cannot be read: {}",
                plain(e.header.as_ref().err().map_or("", String::as_str))
            ),
            _ => String::new(),
        };
        widgets::text(ui, "replays/meta", &meta);
        widgets::visible(ui, "replays/rename", self.renaming);
        let players = header.map_or(&[][..], |h| &h.players[..]);
        if let Some(list) = ui.find("replays/players") {
            ui.set_count(list, players.len());
        }
        for (i, p) in players.iter().enumerate() {
            let base = format!("replays/players/{i}");
            if let Some(n) = ui.find(&format!("{base}/swatch")) {
                ui.set_color(n, Some(palette.solid(p.color as u8)));
            }
            let me = header.and_then(|h| h.recorder) == Some(p.slot);
            widgets::text(
                ui,
                &format!("{base}/name"),
                &format!("{}{}", plain(&p.name), if me { " (recorded)" } else { "" }),
            );
            let race = p
                .race
                .as_deref()
                .and_then(|r| data.content.rules.races.get(r))
                .map(|r| r.name.clone());
            let hero = p
                .hero
                .as_deref()
                .and_then(|h| data.content.definition(h))
                .map(|d| d.name.clone());
            let mut sub = vec![if p.computer { "Computer" } else { "Human" }.to_string()];
            sub.extend(race);
            sub.extend(hero);
            sub.push(match p.team {
                Some(t) => format!("Team {}", t + 1),
                None => "No team".into(),
            });
            sub.push(palette.name(p.color as u8));
            widgets::text(ui, &format!("{base}/sub"), &plain(&sub.join(" · ")));
            let result = header.and_then(|h| h.ending.as_ref()).map_or("", |e| {
                if e.winner == Some(p.slot) {
                    "Won"
                } else if e.defeated.contains(&p.slot) {
                    "Defeated"
                } else {
                    ""
                }
            });
            widgets::text(ui, &format!("{base}/result"), result);
        }
        let reason = match (entry, header) {
            (_, Some(h)) => unplayable(h, data),
            (Some(_), None) => Some("This file cannot be read".into()),
            (None, None) => None,
        };
        let status = if self.status.is_empty() {
            reason.clone().unwrap_or_default()
        } else {
            self.status.clone()
        };
        widgets::text(ui, "replays/status", &plain(&status));
        widgets::visible(ui, "replays/status", !status.is_empty());
        widgets::enabled(ui, "replays/watch", header.is_some() && reason.is_none());
        widgets::enabled(ui, "replays/rename-open", entry.is_some() && !self.renaming);
        widgets::enabled(ui, "replays/delete", entry.is_some());
    }
}

/// Escapes markup in names (the label's markup brackets).
fn plain(text: &str) -> String {
    text.replace('[', "[[")
}
