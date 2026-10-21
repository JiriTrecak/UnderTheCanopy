//! Importing StarCraft II melee maps (docs/editor.md, "StarCraft II maps") as block maps
//! (D80): the archive (`mpq`), its terrain, playable area and objects (`read`), and their
//! conversion into a map package of blocks, starts, amber nodes and substance deposits.
//!
//! The scale comes from the two games' sizes: their town hall is 5 × 5 cells and ours
//! 20 × 20, their barracks 3 × 3 and ours 12 × 12, their supply depot and turret 2 × 2
//! and our house and tower 8 × 8, their marine and worker 0.375 cells in radius and ours
//! 1.5: everything is four times as large here. So one of their cells is [`SCALE`] × 4
//! cells, one block: their cliff levels become block levels cell for cell, their ramps
//! ramp blocks falling the way their cliffs fall (as wide for our units as theirs are
//! for theirs), their chasms (cliff level 0) deep water. Mineral fields become amber
//! nodes on the building lattice, geysers substance deposits, start locations starts;
//! rocks, towers and decoration are left out.

mod explode;
pub mod mpq;

use canopy_content::Content;
use canopy_content::map::blocks::{BLOCK, Block, BlockKind, Blocks};
use canopy_content::map::{MAP_HALO, UtcMap};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Cells here per StarCraft II cell: one block (see the module's note).
pub const SCALE: f64 = 4.0;

/// What a StarCraft II map holds that the importer uses.
#[derive(Debug, Clone)]
pub struct Source {
    pub name: String,
    /// Cells across and up.
    pub width: usize,
    pub height: usize,
    /// The playable area: left, bottom, right, top (cells).
    pub bounds: [f64; 4],
    /// Cliff level per cell, 64 a level (in between: a ramp), row 0 the bottom.
    pub cliff: Vec<u16>,
    pub starts: Vec<[f64; 2]>,
    /// Mineral fields: where, and whether rich.
    pub minerals: Vec<([f64; 2], f64)>,
    pub geysers: Vec<[f64; 2]>,
}

impl Source {
    fn cliff_at(&self, x: i64, y: i64) -> u16 {
        let x = x.clamp(0, self.width as i64 - 1) as usize;
        let y = y.clamp(0, self.height as i64 - 1) as usize;
        self.cliff[y * self.width + x]
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
}

/// The attribute `name` of an XML element's text `tag`.
fn attribute<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

fn position(tag: &str) -> Option<[f64; 2]> {
    let mut parts = attribute(tag, "Position")?.split(',');
    Some([parts.next()?.parse().ok()?, parts.next()?.parse().ok()?])
}

/// Reads a StarCraft II map archive (`.SC2Map` or a Battle.net cache `.s2ma`).
pub fn read(bytes: &[u8], fallback: &str) -> Result<Source, String> {
    let archive = mpq::Archive::open(bytes)?;
    let cliffs = archive.read("t3SyncCliffLevel")?;
    if cliffs.get(..4) != Some(b"CLIF") {
        return Err("t3SyncCliffLevel: not a cliff map".into());
    }
    let width = u32_at(&cliffs, 8).ok_or("t3SyncCliffLevel: truncated")? as usize;
    let height = u32_at(&cliffs, 12).ok_or("t3SyncCliffLevel: truncated")? as usize;
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return Err(format!("unsupported map size {width} × {height}"));
    }
    let cliff: Vec<u16> = cliffs
        .get(32..32 + width * height * 2)
        .ok_or("t3SyncCliffLevel: truncated")?
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let name = archive
        .read("enUS.SC2Data\\LocalizedData\\GameStrings.txt")
        .ok()
        .and_then(|g| {
            let text = String::from_utf8_lossy(&g).into_owned();
            text.lines()
                .find_map(|l| l.strip_prefix("DocInfo/Name="))
                .map(|n| n.trim().to_string())
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback.to_string());
    // The playable area: the camera bounds MapInfo keeps after the tileset's name.
    let terrain = archive
        .read("t3Terrain.xml")
        .map(|t| String::from_utf8_lossy(&t).into_owned())
        .unwrap_or_default();
    let tileset = terrain
        .find("tileSet=\"")
        .and_then(|i| attribute(&terrain[i - 1..], "tileSet"))
        .unwrap_or_default()
        .to_string();
    let info = archive.read("MapInfo")?;
    let mut bounds = [0.0, 0.0, width as f64, height as f64];
    if !tileset.is_empty() {
        let needle = [tileset.as_bytes(), &[0u8]].concat();
        if let Some(at) = info
            .windows(needle.len())
            .position(|w| w == needle.as_slice())
        {
            let at = at + needle.len();
            let b: Vec<f64> = (0..4)
                .filter_map(|i| u32_at(&info, at + i * 4).map(f64::from))
                .collect();
            if b.len() == 4
                && b[0] < b[2]
                && b[1] < b[3]
                && b[2] <= width as f64
                && b[3] <= height as f64
            {
                bounds = [b[0], b[1], b[2], b[3]];
            }
        }
    }
    let objects = archive
        .read("Objects")
        .map(|o| String::from_utf8_lossy(&o).into_owned())
        .unwrap_or_default();
    let (mut starts, mut minerals, mut geysers) = (Vec::new(), Vec::new(), Vec::new());
    for tag in objects.split('<').filter(|t| !t.starts_with('/')) {
        if tag.starts_with("ObjectPoint") && attribute(tag, "Type") == Some("StartLoc") {
            starts.extend(position(tag));
        } else if tag.starts_with("ObjectUnit") {
            let kind = attribute(tag, "UnitType").unwrap_or_default();
            let Some(at) = position(tag) else { continue };
            if kind.contains("MineralField") {
                let amount = if kind.contains("Rich") {
                    2.0
                } else if kind.contains("750") || kind.contains("450") {
                    0.6
                } else {
                    1.0
                };
                minerals.push((at, amount));
            } else if kind.contains("Geyser") {
                geysers.push(at);
            }
        }
    }
    if starts.is_empty() {
        return Err(format!("{name} has no start locations: not a melee map"));
    }
    Ok(Source {
        name,
        width,
        height,
        bounds,
        cliff,
        starts,
        minerals,
        geysers,
    })
}

/// A StarCraft II map the importer can read: its file, name and players.
#[derive(Debug, Clone)]
pub struct Found {
    pub file: PathBuf,
    pub name: String,
    pub players: usize,
}

/// StarCraft II melee maps on this machine: the game's own `Maps` folder and the
/// Battle.net cache (downloaded ladder maps), newest names first.
pub fn find() -> Vec<Found> {
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/Users/Shared/Blizzard/Battle.net/Cache"),
        PathBuf::from("/Applications/StarCraft II/Maps"),
        PathBuf::from("C:\\ProgramData\\Blizzard Entertainment\\Battle.net\\Cache"),
        PathBuf::from("C:\\Program Files (x86)\\StarCraft II\\Maps"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(Path::new(&home).join("Documents/StarCraft II/Maps"));
    }
    let mut files = Vec::new();
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
            if path.is_dir() && depth > 0 && ext.as_deref() != Some("sc2map") {
                walk(&path, depth - 1, out);
            } else if path.is_file() && matches!(ext.as_deref(), Some("s2ma" | "sc2map")) {
                out.push(path);
            }
        }
    }
    for root in &roots {
        walk(root, 4, &mut files);
    }
    let mut found: Vec<Found> = files
        .into_iter()
        .filter(|f| {
            std::fs::metadata(f).is_ok_and(|m| m.len() < 24 << 20)
                && !f.to_string_lossy().contains("Campaign")
        })
        .filter_map(|file| {
            let bytes = std::fs::read(&file).ok()?;
            let archive = mpq::Archive::open(&bytes).ok()?;
            if !archive.has("t3SyncCliffLevel") {
                return None;
            }
            let stem = file.file_stem()?.to_string_lossy().into_owned();
            let source = read(&bytes, &stem).ok()?;
            // Melee maps: two to eight starts and mineral fields to mine.
            (source.starts.len() >= 2 && source.starts.len() <= 8 && !source.minerals.is_empty())
                .then_some(Found {
                    file,
                    name: source.name,
                    players: source.starts.len(),
                })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found.dedup_by(|a, b| a.name == b.name);
    found
}

/// A ramp of theirs: the level it falls from and which way it falls here (1–8).
struct Ramp {
    high: u8,
    dir: u8,
}

fn cell_index(source: &Source, x: i64, y: i64) -> usize {
    let x = x.clamp(0, source.width as i64 - 1) as usize;
    let y = y.clamp(0, source.height as i64 - 1) as usize;
    y * source.width + x
}

/// Their ramps (the connected runs of cells between two cliff levels) and which ramp each
/// cell is on; `ours` maps their cells to ours, for the way each falls here.
fn ramps(
    source: &Source,
    ours: &dyn Fn([f64; 2]) -> (f64, f64),
) -> (Vec<Ramp>, Vec<Option<usize>>) {
    let mut ramp_of = vec![None; source.cliff.len()];
    let (w, h) = (source.width as i64, source.height as i64);
    let is_ramp = |x: i64, y: i64| !source.cliff_at(x, y).is_multiple_of(64);
    let mut seen = vec![false; source.cliff.len()];
    let mut out = Vec::new();
    for y0 in 0..h {
        for x0 in 0..w {
            let i0 = (y0 * w + x0) as usize;
            if seen[i0] || !is_ramp(x0, y0) {
                continue;
            }
            let mut cells = vec![(x0, y0)];
            seen[i0] = true;
            let mut k = 0;
            while k < cells.len() {
                let (x, y) = cells[k];
                k += 1;
                for (dx, dy) in [
                    (1, 0),
                    (-1, 0),
                    (0, 1),
                    (0, -1),
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                ] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let j = (ny * w + nx) as usize;
                    if !seen[j] && is_ramp(nx, ny) {
                        seen[j] = true;
                        cells.push((nx, ny));
                    }
                }
            }
            if cells.len() < 3 {
                continue;
            }
            let high = cells
                .iter()
                .map(|&(x, y)| source.cliff_at(x, y).div_ceil(64))
                .max()
                .unwrap_or(1);
            // The ground round it: on the level it falls from, and the one below.
            let (mut top, mut foot) = ((0.0, 0.0, 0.0), (0.0, 0.0, 0.0));
            for &(x, y) in &cells {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if is_ramp(nx, ny) {
                        continue;
                    }
                    let level = source.cliff_at(nx, ny) / 64;
                    let p = (nx as f64 + 0.5, ny as f64 + 0.5);
                    if level == high {
                        top = (top.0 + p.0, top.1 + p.1, top.2 + 1.0);
                    } else if level + 1 == high {
                        foot = (foot.0 + p.0, foot.1 + p.1, foot.2 + 1.0);
                    }
                }
            }
            if top.2 == 0.0 || foot.2 == 0.0 {
                continue;
            }
            let (tx, tz) = ours([top.0 / top.2, top.1 / top.2]);
            let (fx, fz) = ours([foot.0 / foot.2, foot.1 / foot.2]);
            let dir =
                canopy_content::map::blocks::ramp_direction((fx - tx) as f32, (fz - tz) as f32);
            for &(x, y) in &cells {
                ramp_of[cell_index(source, x, y)] = Some(out.len());
            }
            out.push(Ramp {
                high: high as u8,
                dir,
            });
        }
    }
    (out, ramp_of)
}

/// What an import did.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Report {
    pub name: String,
    pub size: u32,
    pub starts: usize,
    pub amber: usize,
    pub deposits: usize,
    pub warnings: Vec<String>,
}

/// The block map of `source`: blocks, starts, amber nodes and deposits.
pub fn convert(
    source: &Source,
    content: &Content,
    biome: &str,
) -> Result<(UtcMap, Report), String> {
    let [left, bottom, right, top] = source.bounds;
    // Their whole map, border and all, one of their cells a block of ours; square, the
    // shorter side centred.
    let (w, h) = (source.width as f64, source.height as f64);
    let size = ((w.max(h) * SCALE) as u32).div_ceil(32) * 32;
    if !canopy_content::map::valid_size(size) {
        return Err(format!(
            "{} is too large: {size} cells across (at most 1024)",
            source.name
        ));
    }
    let block = f64::from(BLOCK);
    let centre = |extent: f64| ((f64::from(size) - extent * SCALE) / 2.0 / block).floor() * block;
    let (ox, oz) = (centre(w), centre(h));
    // Our cell (x, z) ↔ their cell: x keeps its way, north (their +y) is up (our −z).
    let theirs = |x: f64, z: f64| ((x - ox) / SCALE, h - (z - oz) / SCALE);
    let ours = |p: [f64; 2]| (ox + p[0] * SCALE, oz + (h - p[1]) * SCALE);
    let (px, pz) = ours([left, top]);
    let playable_w = ((right - left) * SCALE) as u32;
    let playable_h = ((top - bottom) * SCALE) as u32;
    let mut blocks = Blocks::flat(size);
    let side = blocks.side as i32;
    let halo = MAP_HALO as f64;
    let b = f64::from(BLOCK);
    let level_of = |v: u16| (f64::from(v) / 64.0).round() as u8;
    // Their ramps, and which ramp each of their cells is on.
    let (ramps, ramp_of) = ramps(source, &ours);
    let mut ramp_columns: Vec<Vec<(i32, i32)>> = vec![Vec::new(); ramps.len()];
    // Their cells line up with our blocks.
    debug_assert!((SCALE - b).abs() < f64::EPSILON);
    for cz in 0..side {
        for cx in 0..side {
            // The one cell of theirs this block is.
            let (mx, mz) = (
                f64::from(cx) * b - halo + b / 2.0,
                f64::from(cz) * b - halo + b / 2.0,
            );
            let (sx, sy) = theirs(mx, mz);
            let (ix, iy) = (sx.floor() as i64, sy.floor() as i64);
            let v = source.cliff_at(ix, iy);
            let inside = sx > left && sx < right && sy > bottom && sy < top;
            let mut block = match ramp_of[cell_index(source, ix, iy)] {
                // A ramp: its blocks all with its level and its fall, one plane from
                // its top to its foot.
                Some(r) if !v.is_multiple_of(64) => {
                    let ramp = &ramps[r];
                    let mut block = Block::ground(ramp.high.max(1));
                    block.kind = BlockKind::Ramp;
                    block.ramp = ramp.dir;
                    ramp_columns[r].push((cx, cz));
                    block
                }
                _ => match level_of(v) {
                    // A chasm: deep water a level down.
                    0 => {
                        let mut block = Block::ground(1);
                        block.kind = BlockKind::Deep;
                        block
                    }
                    level => Block::ground(level),
                },
            };
            if !inside {
                block.kind = BlockKind::Wall;
                block.ramp = 0;
                block.level = block.level.max(1);
            }
            blocks.set(cx, cz, block);
        }
    }
    // Diagonal outlines: their cliffs run at 45°, so plateaus lose their box corners.
    let all: Vec<(i32, i32)> = (0..side)
        .flat_map(|z| (0..side).map(move |x| (x, z)))
        .collect();
    crate::world::round_columns(&mut blocks, &all);
    // A diagonal ramp's staircase of blocks loses its teeth.
    for (ramp, columns) in ramps.iter().zip(&ramp_columns) {
        crate::world::ramp_teeth(&mut blocks, columns, ramp.high, ramp.dir);
    }
    let terrain = blocks.terrain(size);
    let mut map: UtcMap = serde_json::from_value(json!({
        "v": canopy_content::map::UTCMAP_VERSION,
        "name": source.name,
        "size": size,
        "biome": biome,
        "playerStarts": [],
        "entities": [],
        "camps": [],
        "playable": {"x": px as u32, "z": pz as u32, "width": playable_w, "height": playable_h},
        "authoring": {"version": 1, "objects": [], "terrain": {
            "version": 1, "size": size,
            "heights": "", "waterHeights": "", "waterFlow": "",
            "waterProfiles": [], "grass": "", "rock": "", "paint": []
        }}
    }))
    .map_err(|e| format!("the converted map: {e}"))?;
    map.authoring.terrain = terrain;
    let mut report = Report {
        name: source.name.clone(),
        size,
        ..Default::default()
    };
    // Starts, on the building lattice, their halls facing the middle.
    let setup = content
        .rules
        .races
        .values()
        .filter_map(|r| r.starting_setup.as_ref())
        .next()
        .unwrap_or(&content.rules.starting_setup);
    for (i, start) in source.starts.iter().enumerate().take(8) {
        let (x, z) = ours(*start);
        let (x, z) = crate::map::tools::start_position(x, z);
        let player = i + 1;
        let rotation = crate::map::tools::facing_middle(content, setup.fort.as_str(), x, z, size);
        map.player_starts.push(
            serde_json::from_value(json!({
                "mainFort": format!("start.player.{player}/main-fort"),
                "player": player, "setup": setup.id.as_str(), "x": x, "z": z,
                "rotation": rotation,
            }))
            .map_err(|e| e.to_string())?,
        );
    }
    report.starts = map.player_starts.len();
    // The ground as the game will see it, to set nodes and deposits where they may stand.
    let terrain =
        canopy_sim::terrain::Terrain::compile(&map, content).map_err(|e| format!("{e:?}"))?;
    // Amber nodes and deposits, each on the lattice, none over another or a start's hall.
    let mut taken: Vec<[f64; 4]> = map
        .player_starts
        .iter()
        .map(|s| [s.x - 12.0, s.z - 12.0, s.x + 12.0, s.z + 12.0])
        .collect();
    let mut place = |definition: &canopy_content::game::Definition,
                     at: (f64, f64),
                     id: String,
                     amount: Option<u32>|
     -> Option<Value> {
        let (w, h) = definition
            .footprint
            .as_ref()
            .map_or((4, 4), |f| (f.width, f.depth));
        let lattice = |v: f64, s: u32| {
            let offset = f64::from(s) / 2.0 - 0.5;
            ((v - offset) / 4.0).round() * 4.0 + offset
        };
        let (hw, hh) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        // The nearest lattice spot where it may stand, clear of what is placed.
        let mut spots: Vec<(f64, f64)> = (-2i32..=2)
            .flat_map(|j| (-2i32..=2).map(move |i| (i, j)))
            .map(|(i, j)| {
                (
                    lattice(at.0, w) + f64::from(i) * 4.0,
                    lattice(at.1, h) + f64::from(j) * 4.0,
                )
            })
            .collect();
        spots.sort_by(|a, b| {
            let d = |p: &(f64, f64)| (p.0 - at.0).hypot(p.1 - at.1);
            d(a).total_cmp(&d(b))
        });
        let (x, z, rect) = spots.into_iter().find_map(|(x, z)| {
            let rect = [x - hw, z - hh, x + hw, z + hh];
            let clear = !taken
                .iter()
                .any(|t| rect[0] < t[2] && t[0] < rect[2] && rect[1] < t[3] && t[1] < rect[3]);
            let fits =
                canopy_sim::spawn::site_problem(&map, &terrain, definition, x, z, 0.0).is_none();
            (clear && fits).then_some((x, z, rect))
        })?;
        taken.push(rect);
        let mut entity = json!({
            "definition": definition.id.as_str(),
            "id": id,
            "owner": "none",
            "position": {"x": x, "y": z},
            "rotation": 0.0,
            "initialState": {"construction": "complete"},
        });
        if let Some(amount) = amount {
            entity["initialState"]["amount"] = json!(amount);
        }
        Some(entity)
    };
    let mine = content
        .definitions
        .iter()
        .find(|d| crate::world::catalog::is_mine(d));
    let deposit = content
        .definitions
        .iter()
        .filter_map(|d| d.placement_on.as_ref().map(|p| p.source.clone()))
        .find_map(|id| content.definition(id.as_str()));
    let mut entities = Vec::new();
    if let Some(mine) = mine {
        for (i, (at, rich)) in source.minerals.iter().enumerate() {
            let amount = (1500.0 * rich).round() as u32;
            if let Some(e) = place(mine, ours(*at), format!("amber.{}", i + 1), Some(amount)) {
                entities.push(e);
                report.amber += 1;
            }
        }
    } else {
        report.warnings.push("the content has no amber mine".into());
    }
    if let Some(deposit) = deposit {
        for (i, at) in source.geysers.iter().enumerate() {
            if let Some(e) = place(deposit, ours(*at), format!("deposit.{}", i + 1), None) {
                entities.push(e);
                report.deposits += 1;
            }
        }
    }
    for entity in entities {
        map.entities
            .push(serde_json::from_value(entity).map_err(|e| e.to_string())?);
    }
    Ok((map, report))
}

/// Imports the StarCraft II map `source` as the map package `maps/<folder>/<id>.utcm` in
/// the package at `package_root`, with its preview. Returns what it did; the map's
/// problems, if the check finds any, are among the warnings, to fix in the map editor.
pub fn import(
    source: &Path,
    id: &str,
    folder: &str,
    package_root: &Path,
    data: &Path,
    content: &std::sync::Arc<Content>,
) -> Result<Report, String> {
    canopy_content::Id::new(id)?;
    if id.contains('.') {
        return Err("a map id has no dots".into());
    }
    let folder = folder.trim_matches('/');
    let dir = package_root
        .join("maps")
        .join(folder)
        .join(format!("{id}.utcm"));
    if dir.exists() {
        return Err(format!("maps/{folder}/{id}.utcm already exists"));
    }
    let bytes = std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(id)
        .to_string();
    let parsed = read(&bytes, &stem)?;
    let biome = canopy_content::map::BIOMES[0];
    let (map, mut report) = convert(&parsed, content, biome)?;
    // The checks the editor runs on every map.
    for error in canopy_content::map::validate(&map, content) {
        report
            .warnings
            .push(format!("{}: {}", error.location, error.message));
    }
    // The game's own checks: every start fits, and armies can reach each other.
    let setup = canopy_scene::spectator_setup(&map, None);
    match canopy_sim::World::new(content.clone(), &map, setup) {
        Ok(world) => {
            for check in [
                canopy_sim::spawn::playable(&map, world.catalog(), world.terrain()),
                canopy_sim::spawn::connected(&map, world.terrain()),
            ] {
                if let Err(error) = check {
                    report.warnings.push(error);
                }
            }
        }
        Err(error) => report.warnings.push(error.0),
    }
    canopy_content::map::package::write(&map, id, "1.0.0", &dir).map_err(|e| e.to_string())?;
    // Its picture, from the ground the biome generates.
    let generated = canopy_scene::Look::load(data, biome)
        .ok()
        .and_then(|look| canopy_scene::generate::generate(&map, &look.biome))
        .map_or(map.clone(), |g| g.map);
    let trees = crate::map::tools::tree_kinds(content);
    if let Ok(image) = crate::map::preview::render(&generated, &|a| trees.iter().any(|t| t == a)) {
        let _ =
            crate::map::preview::webp(&image).map(|b| std::fs::write(dir.join("preview.webp"), b));
    }
    Ok(report)
}
