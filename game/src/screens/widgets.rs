//! Pieces several screens share: map previews with start markers, player colours, race
//! and hero choices, small UI helpers.

use super::ui_host::UiHost;
use crate::data::{GameData, MapEntry};
use canopy_render::Gpu;
use canopy_ui::{Color, Ui};

/// Player colours by slot (`config/players.json`), sRGB, and their names.
#[derive(Debug, Clone)]
pub struct Palette(pub Vec<[u8; 3]>, pub Vec<String>);

impl Palette {
    pub fn load(data: &std::path::Path) -> Self {
        #[derive(serde::Deserialize)]
        struct File {
            colors: Vec<String>,
            #[serde(default)]
            names: Vec<String>,
        }
        let file = std::fs::read(data.join("config/players.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok());
        let colors = file
            .as_ref()
            .map(|f| f.colors.iter().filter_map(|c| hex(c)).collect::<Vec<_>>())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| vec![[0xff, 0x30, 0x38], [0x33, 0x8d, 0xff]]);
        Self(colors, file.map(|f| f.names).unwrap_or_default())
    }

    pub fn color(&self, slot: u8) -> [u8; 3] {
        self.0[usize::from(slot) % self.0.len()]
    }

    /// How many colours a lobby offers.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The lobby's name for colour `index`.
    pub fn name(&self, index: u8) -> String {
        self.1
            .get(usize::from(index))
            .cloned()
            .unwrap_or_else(|| format!("Colour {}", index + 1))
    }

    /// The marker colour: the player colour mixed 55 % with the panel (old badge).
    pub fn badge(&self, slot: u8) -> Color {
        let [r, g, b] = self.color(slot);
        let mix = |c: u8, base: u8| (f32::from(c) * 0.55 + f32::from(base) * 0.45) as u8;
        Color::rgba(mix(r, 0x10), mix(g, 0x1a), mix(b, 0x1a), 255)
    }

    pub fn solid(&self, slot: u8) -> Color {
        let [r, g, b] = self.color(slot);
        Color::rgba(r, g, b, 255)
    }
}

fn hex(text: &str) -> Option<[u8; 3]> {
    let t = text.strip_prefix('#')?;
    if t.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// A node by path; a path no layout has is reported once (a layout and the code
/// disagree).
pub fn find(ui: &Ui, path: &str) -> Option<canopy_ui::NodeId> {
    let found = ui.find(path);
    if found.is_none() {
        static MISSING: std::sync::Mutex<std::collections::BTreeSet<String>> =
            std::sync::Mutex::new(std::collections::BTreeSet::new());
        // Entries of lists that are not built yet are expected; only fixed paths warn.
        let in_list = path.split('/').any(|part| part.parse::<usize>().is_ok());
        if !in_list
            && let Ok(mut missing) = MISSING.lock()
            && missing.insert(path.to_string())
        {
            tracing::warn!(path, "no UI node at this path");
        }
    }
    found
}

/// Sets a label's text by path (no-op when the node is missing).
pub fn text(ui: &mut Ui, path: &str, value: &str) {
    if let Some(id) = find(ui, path) {
        ui.set_text(id, value);
    }
}

pub fn visible(ui: &mut Ui, path: &str, value: bool) {
    if let Some(id) = find(ui, path) {
        ui.set_visible(id, value);
    }
}

pub fn enabled(ui: &mut Ui, path: &str, value: bool) {
    if let Some(id) = find(ui, path) {
        ui.set_enabled(id, value);
    }
}

pub fn checked(ui: &mut Ui, path: &str, value: bool) {
    if let Some(id) = find(ui, path) {
        ui.set_checked(id, value);
    }
}

/// Sets a dropdown's options and selection.
pub fn options(ui: &mut Ui, path: &str, options: &[String], selected: usize) {
    if let Some(id) = find(ui, path) {
        ui.set_options(id, options);
        ui.set_selected(id, selected.min(options.len().saturating_sub(1)));
    }
}

/// `"{w} × {h} C · {n} players"` (old map meta).
/// A map's side in building cells, as the menus show sizes (`160 × 160 C`).
pub fn cells(map: &MapEntry) -> u32 {
    (f64::from(map.size) / canopy_sim::terrain::footprint::BUILDING_CELL) as u32
}

/// `"{n} players · {w} × {h} C"` (old map list line).
pub fn map_meta(map: &MapEntry) -> String {
    let c = cells(map);
    if map.sandbox {
        format!("Single-player testbed · {c} × {c} C")
    } else {
        format!("{} players · {c} × {c} C", map.starts.len())
    }
}

/// Fills a map list (`sk-map` entries) under `list`.
pub fn map_list(
    host: &mut UiHost,
    gpu: &Gpu,
    data: &GameData,
    list: &str,
    maps: &[&MapEntry],
    selected: Option<usize>,
) {
    let Some(id) = host.ui.find(list) else {
        return;
    };
    host.ui.set_count(id, maps.len());
    for (i, map) in maps.iter().enumerate() {
        let base = format!("{list}/{i}");
        let image = map
            .preview
            .as_deref()
            .and_then(|entry| host.pack_image(gpu, &data.pack, entry));
        if let Some(node) = host.ui.find(&format!("{base}/thumb")) {
            host.ui.set_image(node, image);
        }
        text(&mut host.ui, &format!("{base}/name"), &map.name);
        text(&mut host.ui, &format!("{base}/meta"), &map_meta(map));
        checked(&mut host.ui, &base, selected == Some(i));
    }
}

/// Shows a map in a preview block (`preview`, `title`, `meta`, `description` under
/// `base`), with numbered start markers coloured by slot.
pub fn map_preview(
    host: &mut UiHost,
    gpu: &Gpu,
    data: &GameData,
    palette: &Palette,
    base: &str,
    map: Option<&MapEntry>,
    mine: Option<u8>,
) {
    let image = map
        .and_then(|m| m.preview.as_deref())
        .and_then(|entry| host.pack_image(gpu, &data.pack, entry));
    if let Some(node) = host.ui.find(&format!("{base}/preview/image")) {
        host.ui.set_image(node, image);
    }
    visible(
        &mut host.ui,
        &format!("{base}/preview/empty"),
        map.is_none() || image.is_none(),
    );
    // Screens show what they have of the title, meta and description.
    let quiet = |ui: &mut Ui, path: String, value: &str| {
        if let Some(id) = ui.find(&path) {
            ui.set_text(id, value);
        }
    };
    if let Some(map) = map {
        quiet(&mut host.ui, format!("{base}/title"), &map.name);
        quiet(
            &mut host.ui,
            format!("{base}/meta"),
            &if map.sandbox {
                format!("{0} × {0} C · Single-player testbed", cells(map))
            } else {
                format!(
                    "{0} × {0} C · {1} players · {2} neutral camps",
                    cells(map),
                    map.starts.len(),
                    map.camps
                )
            },
        );
        quiet(
            &mut host.ui,
            format!("{base}/description"),
            map.description.as_deref().unwrap_or(
                "A frontier beneath the canopy. Establish your colony, explore the wilds, and overcome your rivals.",
            ),
        );
    } else {
        for part in ["title", "meta", "description"] {
            quiet(&mut host.ui, format!("{base}/{part}"), "");
        }
    }
    // A published preview shows every start already; then only the player's own is
    // marked (old `mapPreview`).
    markers(host, palette, base, map, mine, image.is_some());
}

/// Places the start markers over the preview (from its box in the last frame).
pub fn markers(
    host: &mut UiHost,
    palette: &Palette,
    base: &str,
    map: Option<&MapEntry>,
    mine: Option<u8>,
    only_mine: bool,
) {
    let list = format!("{base}/preview/markers");
    let Some(id) = host.ui.find(&list) else {
        return;
    };
    let starts = map.map_or(&[][..], |m| &m.start_cells[..]);
    host.ui.set_count(id, starts.len());
    let (Some(rect), Some(map)) = (host.ui.rect(id), map) else {
        return;
    };
    let dpi = host.ui_scale();
    let (w, h) = (rect.w / dpi, rect.h / dpi);
    for (i, [x, z]) in starts.iter().enumerate() {
        let Some(node) = host.ui.child(id, i) else {
            continue;
        };
        let player = map.starts.get(i).copied().unwrap_or(i as u8 + 1);
        let slot = player.saturating_sub(1);
        host.ui.set_text(node, &format!("{player}"));
        let mut color = palette.badge(slot);
        if mine == Some(slot) {
            color = palette.solid(slot);
        }
        host.ui.set_color(node, Some(color));
        host.ui.set_visible(node, !only_mine || mine == Some(slot));
        let size = map.size.max(1) as f32;
        host.ui
            .set_shift(node, x / size * w - 13.0, z / size * h - 13.0);
    }
}

/// Race choices `(id, name)`: the content's races, or its default race alone.
pub fn races(data: &GameData) -> Vec<(String, String)> {
    let rules = &data.content.rules;
    if rules.races.is_empty() {
        return vec![(rules.default_race.clone(), capital(&rules.default_race))];
    }
    rules
        .races
        .iter()
        .map(|(id, race)| (id.clone(), race.name.clone()))
        .collect()
}

/// Hero choices `(id, name)` of a race and its default.
pub fn heroes(data: &GameData, race: &str) -> (Vec<(String, String)>, Option<String>) {
    let rules = &data.content.rules;
    let setup = rules
        .race(race)
        .map_or(&rules.starting_setup, |p| p.starting_setup);
    let Some(hero) = &setup.hero else {
        return (Vec::new(), None);
    };
    let choices = hero
        .choices
        .iter()
        .map(|id| {
            let name = data
                .content
                .definition(id.as_str())
                .map_or_else(|| id.to_string(), |d| d.name.clone());
            (id.to_string(), name)
        })
        .collect();
    (choices, Some(hero.default.to_string()))
}

fn capital(text: &str) -> String {
    let mut c = text.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// The image path of an asset (definition icons, interface art), if published: its
/// render binding's image, else the record's first image output.
pub fn asset_image(data: &GameData, asset: &str) -> Option<String> {
    data.content
        .asset(asset)
        .and_then(|a| a.image.clone())
        .or_else(|| {
            let record = data
                .content
                .manifest
                .records
                .iter()
                .find(|r| r.id == *asset)?;
            record
                .outputs
                .iter()
                .find(|o| o.role == canopy_content::assets::OutputRole::Image)
                .map(|o| o.path.clone())
        })
}
