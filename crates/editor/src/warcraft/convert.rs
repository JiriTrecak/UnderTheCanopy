//! A Warcraft III melee map as a map of this game (docs/editor.md, "Warcraft maps"): its
//! terrain at 5 cells a tile with its cliffs, ramps and water; its ground tiles
//! substituted by kind with the ground this game has (only Lordaeron Summer's exist as
//! they are), in a biome picked by its tileset; its trees as the biome's trees; its gold
//! mines as amber sites, its creeps as camps of the same strength, its start locations as
//! starts. Ported from the old importer (`src/editor/import/warcraft/convert.ts` there),
//! which refused maps whose tiles it lacked.

use super::read::{Corner, Map, Terrain};
use crate::map::tools::{self, Stroke, Tier, Tool};
use canopy_content::Content;
use canopy_content::map::grid::Grid;
use canopy_content::map::{DRY_WATER_HEIGHT, HEIGHT_MAX, HEIGHT_MIN, MAP_HALO, MAP_SIZES, UtcMap};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Source units a world cell (a 128-unit tile is 5 cells).
const SCALE: f64 = 25.6;
const CELL: f64 = 128.0 / SCALE;
/// How deep units wade, in world units.
const WADING: f64 = 0.6;

/// What the old importer kept as data (`tools/warcraft-import/settings/`): unit levels,
/// which destructibles are trees, each tileset's water offset, each cliff's ground tile.
#[derive(Debug, Deserialize)]
struct Source {
    units: HashMap<String, SourceUnit>,
    trees: Vec<String>,
    #[serde(rename = "waterOffsets")]
    water_offsets: HashMap<String, f64>,
    cliffs: HashMap<String, SourceCliff>,
}

#[derive(Debug, Deserialize)]
struct SourceUnit {
    level: u32,
}

#[derive(Debug, Deserialize)]
struct SourceCliff {
    #[serde(rename = "groundTile")]
    ground_tile: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Policy {
    tree_scale: f64,
    camp_link_distance: f64,
    camp_max_diameter: f64,
    start_creep_clear_radius: f64,
}

pub struct Settings {
    source: Source,
    policy: Policy,
}

impl Settings {
    /// From the repository's `tools/warcraft-import/settings/`.
    pub fn load(data: &Path) -> Result<Self, String> {
        let dir = data.join("tools/warcraft-import/settings");
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name))
                .map_err(|e| format!("{}: {e}", dir.join(name).display()))
        };
        Ok(Self {
            source: serde_json::from_str(&read("source.json")?)
                .map_err(|e| format!("source.json: {e}"))?,
            policy: serde_json::from_str(&read("policy.json")?)
                .map_err(|e| format!("policy.json: {e}"))?,
        })
    }
}

/// What a ground tile becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ground {
    /// One of the Warcraft paint materials (`asset.terrain.warcraft-…`).
    Paint(&'static str),
    /// The biome's own grass, soil (snow in winter) or rock.
    Grass,
    Soil,
    Rock,
}

impl Ground {
    fn describe(self) -> String {
        match self {
            Ground::Paint(m) => m.trim_start_matches("asset.terrain.").to_string(),
            Ground::Grass => "biome grass".into(),
            Ground::Soil => "biome soil".into(),
            Ground::Rock => "biome rock".into(),
        }
    }
}

const DIRT: &str = "asset.terrain.warcraft-ldrt";
const ROUGH: &str = "asset.terrain.warcraft-ldro";
const GRASSY_DIRT: &str = "asset.terrain.warcraft-ldrg";
const ROCK: &str = "asset.terrain.warcraft-lrok";
const GRASS: &str = "asset.terrain.warcraft-lgrs";
const DARK_GRASS: &str = "asset.terrain.warcraft-lgrd";
const SAND: &str = "asset.terrain.warcraft-zsan";
const CLIFF: &str = "asset.terrain.warcraft-cliff-cldi";

/// The biome a tileset looks most like.
pub fn biome(tileset: char) -> &'static str {
    match tileset {
        'W' | 'N' | 'I' => "frozen-forest",
        'F' | 'Q' => "autumn-forest",
        'A' | 'C' => "deep-forest",
        _ => "vibrant-forest",
    }
}

/// The ground a Warcraft tile becomes in `biome`: Lordaeron Summer's tiles as they are;
/// the rest by what they show (dirt, rough dirt, grassy dirt, grass, dark grass and
/// leaves, rock and paving, sand, snow and ice), as the nearest ground this game has.
/// Winter and autumn grass and snow take the biome's own ground, so they keep its look.
pub fn substitute(tile: &str, biome: &str) -> Ground {
    match tile {
        "Ldrt" => return Ground::Paint(DIRT),
        "Ldro" => return Ground::Paint(ROUGH),
        "Ldrg" => return Ground::Paint(GRASSY_DIRT),
        "Lrok" => return Ground::Paint(ROCK),
        "Lgrs" => return Ground::Paint(GRASS),
        "Lgrd" => return Ground::Paint(DARK_GRASS),
        "Zsan" => return Ground::Paint(SAND),
        _ => {}
    }
    let kind = tile.get(1..).unwrap_or_default().to_ascii_lowercase();
    let winter = biome == "frozen-forest";
    let autumn = biome == "autumn-forest";
    let has = |words: &[&str]| words.iter().any(|w| kind.contains(w));
    if has(&["snw", "ice", "snr"]) {
        return if kind.contains("snr") {
            Ground::Rock
        } else {
            Ground::Soil
        };
    }
    if has(&["san", "dsr", "dsd", "bch"]) {
        return Ground::Paint(SAND);
    }
    if has(&["sng"]) {
        return if winter {
            Ground::Grass
        } else {
            Ground::Paint(GRASSY_DIRT)
        };
    }
    if has(&["drg", "dgr", "gsd", "vin", "crp"]) {
        return Ground::Paint(GRASSY_DIRT);
    }
    if has(&["grd", "lvs", "hdg", "gdr"]) {
        return if winter || autumn {
            Ground::Grass
        } else {
            Ground::Paint(DARK_GRASS)
        };
    }
    // The Barrens' dry grass reads as grassy earth.
    if tile.starts_with('B') && has(&["grr", "grs"]) {
        return Ground::Paint(GRASSY_DIRT);
    }
    if has(&["grs", "grt", "gsb", "grr", "gra"]) {
        return if winter || autumn {
            Ground::Grass
        } else {
            Ground::Paint(GRASS)
        };
    }
    if has(&["dro", "drh", "drr", "dtr", "rrk"]) {
        return Ground::Paint(ROUGH);
    }
    if has(&[
        "rok", "rck", "rcb", "brk", "bks", "bkl", "sqd", "tsq", "til", "blm", "btl", "rtl", "stp",
        "cbp", "cbl", "flr", "wmb", "mrb", "ibk", "bdh", "lrv",
    ]) {
        return if winter {
            Ground::Rock
        } else {
            Ground::Paint(ROCK)
        };
    }
    Ground::Paint(DIRT)
}

/// The trees of a tileset in its biome (scenery ids), alternated: the Barrens' dry trees
/// golden, the rest the biome's own.
fn trees(tileset: char, biome: &str) -> [&'static str; 2] {
    if tileset == 'B' {
        return ["scenery.autumn-tree-gold", "scenery.autumn-tree-copper"];
    }
    match biome {
        "frozen-forest" => ["scenery.frost-pine-a", "scenery.frost-pine-b"],
        "autumn-forest" => ["scenery.autumn-tree-gold", "scenery.autumn-tree-copper"],
        _ => ["scenery.woodland-pine-a", "scenery.woodland-pine-b"],
    }
}

/// What the conversion did, for the person importing.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Report {
    pub name: String,
    pub size: u32,
    pub biome: String,
    pub starts: usize,
    pub amber_sites: usize,
    pub camps: BTreeMap<String, usize>,
    pub trees: usize,
    /// Source tile → what it became.
    pub ground: Vec<(String, String)>,
    pub cleared_creeps: usize,
    /// Amber sites and creeps moved to fit (our sites are wider than Warcraft's mines).
    pub moved: usize,
    pub warnings: Vec<String>,
}

/// Each corner's ground tile, with Warcraft's rule that cliff cells show their cliff
/// type's ground (the old importer's `warcraftCornerGround`).
fn corner_ground(t: &Terrain, cliffs: &HashMap<String, SourceCliff>) -> Vec<u8> {
    let (w, h) = (t.width, t.height);
    let mut out: Vec<u8> = t.corners.iter().map(|c| c.texture).collect();
    let mut cell = vec![-1i16; (w - 1) * (h - 1)];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let c = &t.corners[y * w + x];
            let level = c.level;
            if [
                t.corners[y * w + x + 1],
                t.corners[(y + 1) * w + x],
                t.corners[(y + 1) * w + x + 1],
            ]
            .iter()
            .all(|o| o.level == level)
            {
                continue;
            }
            let index = if c.cliff_texture == 15 {
                1
            } else {
                c.cliff_texture as usize
            };
            let ground = t
                .cliff_tiles
                .get(index)
                .and_then(|id| cliffs.get(id))
                .and_then(|d| t.ground_tiles.iter().position(|g| *g == d.ground_tile))
                .unwrap_or(0);
            cell[y * (w - 1) + x] = ground as i16;
        }
    }
    for y in 0..h {
        for x in 0..w {
            'found: for dy in [-1isize, 0] {
                for dx in [-1isize, 0] {
                    let (cx, cy) = (x as isize + dx, y as isize + dy);
                    if cx <= 0 || cy <= 0 || cx >= w as isize - 2 || cy >= h as isize - 2 {
                        continue;
                    }
                    let g = cell[cy as usize * (w - 1) + cx as usize];
                    if g >= 0 {
                        out[y * w + x] = g as u8;
                        break 'found;
                    }
                }
            }
        }
    }
    out
}

fn floats(values: &[f32]) -> Grid {
    Grid(values.iter().flat_map(|v| v.to_le_bytes()).collect())
}

/// Converts `source` (named `fallback_name` when it has no name of its own).
pub fn convert(
    source: &Map,
    content: &Content,
    settings: &Settings,
    fallback_name: &str,
) -> Result<(UtcMap, Report), String> {
    let t = &source.terrain;
    // The whole terrain, its margins (outside the camera's reach in Warcraft) kept as the
    // map's scenery around its playable area.
    let (x0, y0, x1, y1) = (0usize, 0usize, t.width - 1, t.height - 1);
    let (width, height) = ((x1 - x0) as f64 * CELL, (y1 - y0) as f64 * CELL);
    let size = MAP_SIZES
        .iter()
        .copied()
        .filter(|&s| s <= 1024)
        .find(|&s| f64::from(s) >= width.max(height))
        .ok_or_else(|| {
            format!("the map is {width}×{height} cells; 1024 is the largest this game draws")
        })?;
    // Centred, keeping the 4-cell building lattice where it was.
    let pad_x = ((f64::from(size) - width) / 8.0).floor() * 4.0;
    let pad_z = ((f64::from(size) - height) / 8.0).floor() * 4.0;
    // Warcraft's margins (left, right, bottom, top tiles; its y runs up, ours down).
    let [left, right, bottom, top] = source.info.margins;
    let playable = canopy_content::map::Playable {
        x: (pad_x + left as f64 * CELL) as u32,
        z: (pad_z + top as f64 * CELL) as u32,
        width: ((x1 - x0).saturating_sub(left + right) as f64 * CELL) as u32,
        height: ((y1 - y0).saturating_sub(top + bottom) as f64 * CELL) as u32,
    };
    let position = |x: f32, y: f32| {
        (
            (f64::from(x) - f64::from(t.offset_x)) / SCALE - x0 as f64 * CELL + pad_x - 0.5,
            height - ((f64::from(y) - f64::from(t.offset_y)) / SCALE - y0 as f64 * CELL) + pad_z
                - 0.5,
        )
    };
    let biome = biome(t.tileset);
    let mut report = Report {
        name: if source.info.name.is_empty() || source.info.name.starts_with("TRIGSTR") {
            fallback_name.to_string()
        } else {
            source.info.name.clone()
        },
        size,
        biome: biome.to_string(),
        ..Report::default()
    };

    // Ground: each source tile's substitute; a paint layer for every material used, in
    // this game's order (the painted ones, then the cliff faces).
    let grounds: Vec<Ground> = t
        .ground_tiles
        .iter()
        .map(|g| substitute(g, biome))
        .collect();
    for (tile, g) in t.ground_tiles.iter().zip(&grounds) {
        report.ground.push((tile.clone(), g.describe()));
    }
    let mut materials: Vec<&'static str> = grounds
        .iter()
        .filter_map(|g| {
            if let Ground::Paint(m) = g {
                Some(*m)
            } else {
                None
            }
        })
        .collect();
    materials.sort();
    materials.dedup();
    materials.retain(|m| content.landscape.iter().any(|a| a.id.as_str() == *m));
    let cliff_present = content.landscape.iter().any(|a| a.id.as_str() == CLIFF);

    let span = size + 2 * MAP_HALO;
    let side = (span + 1) as usize;
    let origin = -f64::from(MAP_HALO);
    let n = side * side;
    let (mut heights, mut water) = (vec![0f32; n], vec![DRY_WATER_HEIGHT as f32; n]);
    let (mut grass, mut rock) = (vec![0f32; n], vec![0f32; n]);
    let mut paint = vec![vec![0f32; n]; materials.len()];
    let mut cliff = vec![0f32; n];
    let ground_of = corner_ground(t, &settings.source.cliffs);
    let water_offset = settings
        .source
        .water_offsets
        .get(&t.tileset.to_string())
        .copied()
        .unwrap_or(-0.7);
    // Relative heights kept, the whole lifted into the editable range. The range is
    // taken without the rarest 0.2 % at either end (mountain peaks nobody walks), which
    // are clamped; a map still too tall is squeezed evenly.
    let mut source_heights: Vec<f64> = t
        .corners
        .iter()
        .map(|c| (f64::from(c.ground) + f64::from(c.level) - 2.0) * CELL)
        .collect();
    source_heights.sort_by(f64::total_cmp);
    let (min, max) = (
        source_heights[source_heights.len() / 500],
        source_heights[source_heights.len() - 1 - source_heights.len() / 500],
    );
    let room = HEIGHT_MAX - HEIGHT_MIN - 1.0;
    let squeeze = if max - min > room {
        report.warnings.push(format!(
            "Heights squeezed to {:.0} % to fit the terrain's range.",
            room / (max - min) * 100.0
        ));
        room / (max - min)
    } else {
        1.0
    };
    let (min, max) = (min * squeeze, max * squeeze);
    let preferred = ((min + max) / 2.0 / CELL).round() * CELL;
    let offset = (max - HEIGHT_MAX).max((min - HEIGHT_MIN).min(preferred));
    for z in 0..side {
        for x in 0..side {
            let (wx, wz) = (x as f64 + origin, z as f64 + origin);
            let sx = (x0 as f64 + (wx + 0.5 - pad_x) / CELL).clamp(x0 as f64, x1 as f64);
            let sy = (y0 as f64 + (height - (wz + 0.5 - pad_z)) / CELL).clamp(y0 as f64, y1 as f64);
            let (ix, iy) = (
                (sx.floor() as usize).min(t.width - 2),
                (sy.floor() as usize).min(t.height - 2),
            );
            let (u, v) = (sx - ix as f64, sy - iy as f64);
            let corners: [&Corner; 4] = [
                t.at(ix as isize, iy as isize),
                t.at(ix as isize + 1, iy as isize),
                t.at(ix as isize, iy as isize + 1),
                t.at(ix as isize + 1, iy as isize + 1),
            ];
            let weights = [(1.0 - u) * (1.0 - v), u * (1.0 - v), (1.0 - u) * v, u * v];
            let blend = |f: &dyn Fn(&Corner) -> f64| {
                corners
                    .iter()
                    .zip(weights)
                    .map(|(c, w)| f(c) * w)
                    .sum::<f64>()
            };
            // Levels as drawn: a ramp's middle corner half a level up.
            let levels = [(ix, iy), (ix + 1, iy), (ix, iy + 1), (ix + 1, iy + 1)]
                .map(|(x, y)| f64::from(t.level(x as isize, y as isize)));
            let level = levels.iter().zip(weights).map(|(l, w)| l * w).sum::<f64>();
            let same = corners.iter().all(|c| c.level == corners[0].level);
            let ramp = corners.iter().any(|c| c.ramp);
            let source_height = blend(&|c| f64::from(c.ground))
                + if same || ramp { level } else { level.round() }
                - 2.0;
            let mut ground = source_height * CELL * squeeze - offset;
            let surface = if corners.iter().any(|c| c.water) {
                (blend(&|c| f64::from(c.water_height)) + water_offset) * CELL * squeeze - offset
            } else {
                DRY_WATER_HEIGHT
            };
            if surface > ground {
                // Warcraft's shallow water (up to half a tile) stays wadeable here.
                let depth = (surface - ground) / CELL;
                ground = surface
                    - if depth <= 0.5 {
                        depth / 0.5 * WADING
                    } else {
                        WADING + (depth - 0.5) * CELL
                    };
            }
            let i = z * side + x;
            heights[i] = ground.clamp(HEIGHT_MIN, HEIGHT_MAX) as f32;
            water[i] = surface.clamp(DRY_WATER_HEIGHT, HEIGHT_MAX) as f32;
            let nearest = ground_of[(sy.round() as usize).min(t.height - 1) * t.width
                + (sx.round() as usize).min(t.width - 1)];
            match grounds[nearest as usize] {
                Ground::Paint(m) => {
                    if let Some(k) = materials.iter().position(|x| *x == m) {
                        paint[k][i] = 1.0;
                    }
                }
                Ground::Grass => grass[i] = 1.0,
                Ground::Soil => {}
                Ground::Rock => rock[i] = 1.0,
            }
            if !same && !ramp {
                cliff[i] = 1.0;
            }
        }
    }

    // The map document, then its grids.
    let mut paint_layers: Vec<Value> = materials
        .iter()
        .map(|m| json!({"material": m, "weights": ""}))
        .collect();
    if cliff_present {
        paint_layers.push(json!({"material": CLIFF, "weights": ""}));
    }
    let mut map: UtcMap = serde_json::from_value(json!({
        "v": canopy_content::map::UTCMAP_VERSION,
        "name": report.name,
        "description": source.info.description,
        "size": size,
        "biome": biome,
        "playerStarts": [],
        "entities": [],
        "camps": [],
        "playable": playable,
        "authoring": {"version": 1, "objects": [], "terrain": {
            "version": 1, "size": size,
            "textureGrid": {"cellSize": CELL, "origin": [pad_x - 0.5, pad_z - 0.5]},
            "heights": "", "waterHeights": "", "waterFlow": "",
            "waterProfiles": [], "grass": "", "rock": "", "paint": paint_layers
        }}
    }))
    .map_err(|e| format!("the converted map: {e}"))?;
    {
        let terrain = &mut map.authoring.terrain;
        terrain.heights = floats(&heights);
        terrain.water_heights = floats(&water);
        terrain.water_flow = Grid([128u8, 128, 0, 0].repeat((span * span) as usize));
        terrain.grass = floats(&grass);
        terrain.rock = floats(&rock);
        for (layer, weights) in terrain
            .paint
            .iter_mut()
            .zip(paint.iter().chain(std::iter::once(&cliff)))
        {
            layer.weights = floats(weights);
        }
        // Each texture cell's tile variation, as Warcraft drew it (the old importer's).
        let layout = terrain.texture_layout();
        let [columns, rows] = layout.cells;
        let mut variation = vec![0u8; layout.count];
        for z in 0..rows as usize {
            for x in 0..columns as usize {
                let wx = layout.start[0] + x as f64 * CELL - (pad_x - 0.5);
                let wz = layout.start[1] + z as f64 * CELL - (pad_z - 0.5);
                let sx = (wx / CELL).round() as isize;
                let sy = ((height - wz) / CELL).round() as isize - 1;
                variation[z * columns as usize + x] = t.at(sx, sy).variation;
            }
        }
        for layer in terrain.paint.iter_mut().filter(|l| l.material != CLIFF) {
            let tiles = content
                .landscape
                .iter()
                .find(|a| a.id.as_str() == layer.material)
                .and_then(|a| a.terrain.as_ref())
                .map_or(1, |g| g.atlas.full_tiles.len().max(1)) as u8;
            layer.variants = Some(Grid(variation.iter().map(|&v| v.min(tiles - 1)).collect()));
        }
    }

    // Trees: the map's tree destructibles (its own changes included) that stand.
    let mut tree_ids: HashSet<String> = settings.source.trees.iter().cloned().collect();
    for change in &source.tree_changes {
        match &change.targets {
            Some(targets) if targets.split(',').any(|t| t == "tree") => {
                tree_ids.insert(change.id.clone());
            }
            Some(_) => {
                tree_ids.remove(&change.id);
            }
            None if tree_ids.contains(&change.base) => {
                tree_ids.insert(change.id.clone());
            }
            None => {}
        }
    }
    let kinds = trees(t.tileset, biome);
    let limit = f64::from(size) - 1.0;
    for (i, d) in source.doodads.iter().enumerate() {
        if !tree_ids.contains(&d.id) || d.life == 0 {
            continue;
        }
        let (x, z) = position(d.x, d.y);
        if x < 1.0 || z < 1.0 || x > limit || z > limit {
            continue;
        }
        map.authoring.objects.push(object(
            &format!("warcraft.tree.{i}"),
            kinds[d.variation.unsigned_abs() as usize % 2],
            x,
            z,
            -f64::from(d.angle),
            (f64::from(d.scale) * settings.policy.tree_scale).clamp(0.1, 10.0),
        ));
    }

    // Starts: the slots people or computers may take.
    let setup = content
        .rules
        .races
        .values()
        .filter_map(|r| r.starting_setup.as_ref())
        .next()
        .unwrap_or(&content.rules.starting_setup);
    let players: Vec<_> = source
        .info
        .players
        .iter()
        .filter(|p| p.kind == 1 || p.kind == 2)
        .collect();
    if !(2..=8).contains(&players.len()) {
        return Err(format!(
            "the map has {} start locations; a skirmish map has 2 to 8",
            players.len()
        ));
    }
    for (k, p) in players.iter().enumerate() {
        let (x, z) = position(p.x, p.y);
        let (x, z) = tools::start_position(x, z);
        let player = k as u8 + 1;
        let rotation = tools::facing_middle(content, setup.fort.as_str(), x, z, size);
        let mut start = json!({"mainFort": format!("start.player.{player}/main-fort"), "player": player, "setup": setup.id.as_str(), "x": x, "z": z});
        if rotation != 0 {
            start["rotation"] = json!(rotation);
        }
        map.player_starts
            .push(serde_json::from_value(start).map_err(|e| e.to_string())?);
    }
    report.starts = map.player_starts.len();

    // Amber: each gold mine as a site. A main's own mine (the nearest within 40 cells) sits
    // 26 cells from its hall towards where the mine was, as our starts place theirs.
    let starts: Vec<(f64, f64)> = map.player_starts.iter().map(|s| (s.x, s.z)).collect();
    let mines: Vec<(usize, f64, f64, i32)> = source
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.id == "ngol")
        .map(|(i, u)| {
            let (x, z) = position(u.x, u.y);
            (i, x, z, u.gold)
        })
        .collect();
    let node_yield = content
        .definitions
        .iter()
        .find(|d| {
            d.harvesting.is_some() && d.resource_yield.is_some() && d.id.as_str().contains("amber")
        })
        .and_then(|d| d.resource_yield)
        .map_or(2500, |y| y.max(500));
    let mut sites: Vec<(f64, f64)> = Vec::new();
    let mut fixed: HashSet<String> = HashSet::new();
    let mut primary: HashMap<usize, (f64, f64)> = HashMap::new();
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (si, s) in starts.iter().enumerate() {
        for (mi, m) in mines.iter().enumerate() {
            let d = (s.0 - m.1).hypot(s.1 - m.2);
            if d <= 40.0 {
                pairs.push((d, si, mi));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut taken_starts = HashSet::new();
    for (_, si, mi) in pairs {
        if !primary.contains_key(&mi) && taken_starts.insert(si) {
            primary.insert(mi, starts[si]);
        }
    }
    for (si, s) in starts.iter().enumerate() {
        if !taken_starts.contains(&si) {
            report.warnings.push(format!(
                "Player {}: no gold mine within 40 cells of the start.",
                si + 1
            ));
            let _ = s;
        }
    }
    for (mi, &(_, x, z, gold)) in mines.iter().enumerate() {
        // Shared over the five nodes, each up to what a node may hold.
        let amount = (gold.max(0) as u32 / 5).clamp(500, node_yield);
        let (cx, cz, toward) = match primary.get(&mi) {
            Some(&(sx, sz)) => {
                let (dx, dz) = (x - sx, z - sz);
                let length = dx.hypot(dz).max(1.0);
                (sx + dx / length * 26.0, sz + dz / length * 26.0, (sx, sz))
            }
            None => {
                let nearest = starts
                    .iter()
                    .copied()
                    .min_by(|a, b| {
                        (a.0 - x)
                            .hypot(a.1 - z)
                            .total_cmp(&(b.0 - x).hypot(b.1 - z))
                    })
                    .unwrap_or((f64::from(size) / 2.0, f64::from(size) / 2.0));
                (x, z, nearest)
            }
        };
        sites.push((cx, cz));
        for node in tools::amber_site(&map, content, cx, cz, toward, amount) {
            if primary.contains_key(&mi)
                && let Some(id) = node["id"].as_str()
            {
                fixed.insert(site_of(id).to_string());
            }
            map.entities
                .push(serde_json::from_value(node).map_err(|e| e.to_string())?);
        }
        report.amber_sites += 1;
    }

    // Creeps: the neutral hostile player's units away from the starts (as Warcraft's
    // melee setup clears them), grouped as the old importer did, each group a camp of the
    // strength its summed levels give (up to 9 easy, up to 19 medium, harder beyond).
    let neutral = if source.info.build >= 132 { 24 } else { 12 };
    let mut levels: HashMap<String, u32> = settings
        .source
        .units
        .iter()
        .map(|(k, v)| (k.clone(), v.level))
        .collect();
    for change in &source.unit_changes {
        if let Some(level) = change.level {
            levels.insert(change.id.clone(), level.max(1) as u32);
        } else if let Some(&base) = levels.get(&change.base) {
            levels.insert(change.id.clone(), base);
        }
    }
    let clear = settings.policy.start_creep_clear_radius;
    let hostile: Vec<_> = source
        .units
        .iter()
        .filter(|u| u.player == neutral && u.id != "ngol")
        .collect();
    let mut remaining: Vec<_> = hostile
        .iter()
        .filter(|u| {
            !players
                .iter()
                .any(|p| (p.x - u.x).hypot(p.y - u.y) as f64 <= clear)
        })
        .copied()
        .collect();
    report.cleared_creeps = hostile.len() - remaining.len();
    let (link, diameter) = (
        settings.policy.camp_link_distance,
        settings.policy.camp_max_diameter,
    );
    while !remaining.is_empty() {
        let mut group = vec![remaining.remove(0)];
        let mut j = 0;
        while j < group.len() {
            let mut k = remaining.len();
            while k > 0 {
                k -= 1;
                let c = remaining[k];
                let near = |u: &super::read::Object| f64::from((c.x - u.x).hypot(c.y - u.y));
                if near(group[j]) <= link && group.iter().all(|u| near(u) <= diameter) {
                    group.push(remaining.remove(k));
                }
            }
            j += 1;
        }
        // Shops, fountains and other buildings are not camps.
        let units: Vec<_> = group
            .iter()
            .filter(|u| levels.contains_key(&u.id) || u.hero_level > 0)
            .collect();
        if units.is_empty() {
            continue;
        }
        let total: u32 = units
            .iter()
            .map(|u| {
                levels
                    .get(&u.id)
                    .copied()
                    .unwrap_or(u.hero_level.max(1) as u32)
            })
            .sum();
        let tier = if total <= 9 {
            Tier::Easy
        } else if total <= 19 {
            Tier::Medium
        } else {
            Tier::Hard
        };
        let n = units.len() as f32;
        let (cx, cz) = position(
            units.iter().map(|u| u.x).sum::<f32>() / n,
            units.iter().map(|u| u.y).sum::<f32>() / n,
        );
        // Clear of the amber sites (ours are wider than Warcraft's mines): the guards
        // stand beside a site, not in it.
        let (mut cx, mut cz) = (cx, cz);
        for &(sx, sz) in &sites {
            let (dx, dz) = (cx - sx, cz - sz);
            let d = dx.hypot(dz);
            if d < 14.0 {
                let (ux, uz) = if d > 0.5 {
                    (dx / d, dz / d)
                } else {
                    (0.0, 1.0)
                };
                cx = sx + ux * 14.0;
                cz = sz + uz * 14.0;
            }
        }
        let (members, camp) = tools::camp(&map, content, cx.round(), cz.round(), tier);
        for m in members {
            map.entities
                .push(serde_json::from_value(m).map_err(|e| e.to_string())?);
        }
        map.camps
            .push(serde_json::from_value(camp).map_err(|e| e.to_string())?);
        *report.camps.entry(tier.name().to_string()).or_default() += 1;
    }

    let (moved, unplaced) = fit(&mut map, content, &fixed)?;
    report.moved = moved;
    report.warnings.extend(unplaced);

    // Room around what was placed: no trees in an amber site, a camp or a start's yard;
    // and level pads under the halls.
    let mut keep_clear: Vec<(f64, f64, f64)> = Vec::new();
    for s in &map.player_starts {
        keep_clear.push((s.x, s.z, 18.0));
    }
    for e in &map.entities {
        let mine = e.definition.as_str().contains("amber");
        keep_clear.push((e.position.x, e.position.y, if mine { 6.0 } else { 3.0 }));
    }
    map.authoring.objects.retain(|o| {
        !keep_clear
            .iter()
            .any(|&(x, z, r)| (o.x - x).powi(2) + (o.z - z).powi(2) <= r * r)
    });
    let pads: Vec<(f64, f64)> = map.player_starts.iter().map(|s| (s.x, s.z)).collect();
    for (x, z) in pads {
        let mut stroke = Stroke::new(Tool::Flatten, 0.0, false);
        tools::level_pad(&mut map, &mut stroke, x as f32, z as f32, 16.0);
    }

    // The border around the playable area: forest on the tree lattice wherever it is dry
    // and level enough (resources like any tree; those at the edge are cut from inside).
    let border = map.playable_area();
    let decoded = map.authoring.terrain.decode().map_err(|e| e.to_string())?;
    let halo = MAP_HALO as usize;
    let mut planted = 0usize;
    for cz in 1..size as i64 - 1 {
        for cx in 1..size as i64 - 1 {
            let lattice = (cx.rem_euclid(5) == 2 && cz.rem_euclid(5) == 2)
                || (cx.rem_euclid(5) == 0 && cz.rem_euclid(5) == 0);
            // Two cells in from the playable edge, so the woods frame it.
            let inside = border.contains(cx as f64 + 2.5, cz as f64 + 2.5)
                && border.contains(cx as f64 - 1.5, cz as f64 - 1.5);
            if !lattice || inside {
                continue;
            }
            let i = (cz as usize + halo) * side + cx as usize + halo;
            let h = decoded.heights[i];
            let steep = [1usize, side].iter().any(|&d| {
                (decoded.heights[i + d] - h).abs() > 0.9 || (decoded.heights[i - d] - h).abs() > 0.9
            });
            if decoded.water_heights[i] > h - 0.2 || steep {
                continue;
            }
            map.authoring.objects.push(object(
                &format!("warcraft.border.{planted}"),
                kinds[planted % 2],
                cx as f64,
                cz as f64,
                (planted % 7) as f64,
                settings.policy.tree_scale * (0.9 + (planted % 5) as f64 * 0.05),
            ));
            planted += 1;
        }
    }
    report.trees = map.authoring.objects.len();
    Ok((map, report))
}

fn object(
    id: &str,
    asset: &str,
    x: f64,
    z: f64,
    yaw: f64,
    scale: f64,
) -> canopy_content::map::AuthoredObject {
    canopy_content::map::AuthoredObject {
        id: id.to_string(),
        asset: asset.to_string(),
        x,
        z,
        elevation: 0.0,
        yaw,
        scale,
        height_mode: Default::default(),
        visible: true,
        locked: false,
    }
}

/// Removes the camps and amber sites armies cannot walk to from player 1's start (air-only
/// islands: this game has no transports), returning what went.
pub fn drop_unreachable(map: &mut UtcMap, content: &Content) -> Result<Vec<String>, String> {
    let Some(first) = map.player_starts.first() else {
        return Ok(Vec::new());
    };
    let terrain =
        canopy_sim::terrain::Terrain::compile(map, content).map_err(|e| format!("{e:?}"))?;
    // Truly cut off: not even a warrior gets there (lanes under 3 cells). Narrower lanes
    // than armies need stay, for the map's check to point out.
    let closed = canopy_sim::spawn::narrowed(&terrain, &canopy_sim::spawn::tree_cells(&terrain), 1);
    let reached = canopy_sim::spawn::reachable_from(&terrain, &closed, first.x, first.z);
    let near = |x: f64, z: f64| canopy_sim::spawn::near(&terrain, &reached, x, z, 10);
    let mut gone = Vec::new();
    let mut members_gone: HashSet<String> = HashSet::new();
    map.camps.retain(|c| {
        let keep = near(c.home.x, c.home.y);
        if !keep {
            gone.push(format!("{} (no way there on foot)", c.id));
            members_gone.extend(c.members.iter().cloned());
        }
        keep
    });
    // Amber sites go whole: `mine.<n>.<k>` nodes by their `mine.<n>`.
    let site = |id: &str| {
        id.rsplit_once('.')
            .map_or(id.to_string(), |(s, _)| s.to_string())
    };
    let mut reachable_sites: HashSet<String> = HashSet::new();
    let mut all_sites: HashSet<String> = HashSet::new();
    for e in map
        .entities
        .iter()
        .filter(|e| e.definition.as_str().contains("amber"))
    {
        all_sites.insert(site(&e.id));
        if near(e.position.x, e.position.y) {
            reachable_sites.insert(site(&e.id));
        }
    }
    let mut lost: Vec<&String> = all_sites.difference(&reachable_sites).collect();
    lost.sort();
    for s in &lost {
        gone.push(format!("{s} (no way there on foot)"));
    }
    let lost: HashSet<String> = lost.into_iter().cloned().collect();
    map.entities.retain(|e| {
        !members_gone.contains(&e.id)
            && !(e.definition.as_str().contains("amber") && lost.contains(&site(&e.id)))
    });
    Ok(gone)
}

/// The `mine.<n>` an amber node `mine.<n>.<k>` belongs to.
fn site_of(id: &str) -> &str {
    id.rsplit_once('.').map_or(id, |(s, _)| s)
}

/// Moves what does not fit where Warcraft had it, as the old importer's
/// `fitImportedPlacements` did: our amber sites are a wider arc than its mines and our
/// creeps differ from its. A site slides whole along the building lattice to the nearest
/// spot within 12 cells where every node stands on dry, flat, open, playable ground clear
/// of the halls and the other sites (a main's own site stays); a creep moves to the
/// nearest open cell clear of the sites and the halls. Returns how many moved and what
/// found no place.
pub fn fit(
    map: &mut UtcMap,
    content: &Content,
    fixed: &HashSet<String>,
) -> Result<(usize, Vec<String>), String> {
    use canopy_sim::terrain::footprint::CellBounds;
    let terrain =
        canopy_sim::terrain::Terrain::compile(map, content).map_err(|e| format!("{e:?}"))?;
    let walkable = |x: i64, z: i64| terrain.cell(x, z).is_some_and(|c| terrain.walkable(c));
    let starts: Vec<(f64, f64)> = map.player_starts.iter().map(|s| (s.x, s.z)).collect();
    // A node 4×4 on the lattice; the hall 20×20; the construction clearance 3.
    let clear_of_halls = |x: f64, z: f64| {
        starts
            .iter()
            .all(|&(sx, sz)| (sx - x).abs() >= 15.0 || (sz - z).abs() >= 15.0)
    };
    let node_bounds = |x: f64, z: f64| CellBounds {
        min_x: (x - 1.5).round() as i64,
        min_y: (z - 1.5).round() as i64,
        max_x: (x + 1.5).round() as i64,
        max_y: (z + 1.5).round() as i64,
    };
    let node_ground = |x: f64, z: f64| {
        let b = node_bounds(x, z);
        terrain.foundation_fits(b)
            && (b.min_y..=b.max_y).all(|cz| (b.min_x..=b.max_x).all(|cx| walkable(cx, cz)))
    };
    let mut moved = 0;
    let mut warnings = Vec::new();
    let mut sites: Vec<String> = map
        .entities
        .iter()
        .filter(|e| e.definition.as_str().contains("amber"))
        .map(|e| site_of(&e.id).to_string())
        .collect();
    sites.dedup();
    let mut deltas: Vec<(i64, i64)> = (-3..=3)
        .flat_map(|i| (-3..=3).map(move |j| (i * 4, j * 4)))
        .collect();
    deltas.sort_by(|a, b| {
        ((a.0 * a.0 + a.1 * a.1), a.1, a.0).cmp(&((b.0 * b.0 + b.1 * b.1), b.1, b.0))
    });
    for site in &sites {
        let nodes: Vec<usize> = (0..map.entities.len())
            .filter(|&i| site_of(&map.entities[i].id) == site)
            .collect();
        let others: Vec<(f64, f64)> = map
            .entities
            .iter()
            .filter(|e| e.definition.as_str().contains("amber") && site_of(&e.id) != site)
            .map(|e| (e.position.x, e.position.y))
            .collect();
        let fits = |dx: f64, dz: f64| {
            nodes.iter().all(|&i| {
                let (x, z) = (
                    map.entities[i].position.x + dx,
                    map.entities[i].position.y + dz,
                );
                node_ground(x, z)
                    && clear_of_halls(x, z)
                    && others
                        .iter()
                        .all(|&(ox, oz)| (ox - x).abs() >= 4.0 || (oz - z).abs() >= 4.0)
            })
        };
        let options: &[(i64, i64)] = if fixed.contains(site) {
            &deltas[..1]
        } else {
            &deltas
        };
        match options.iter().find(|&&(dx, dz)| fits(dx as f64, dz as f64)) {
            Some(&(0, 0)) => {}
            Some(&(dx, dz)) => {
                for &i in &nodes {
                    map.entities[i].position.x += dx as f64;
                    map.entities[i].position.y += dz as f64;
                }
                moved += 1;
            }
            None if !fixed.contains(site) => warnings.push(format!(
                "{site}: no clear ground for its amber nearby; check it in the map editor."
            )),
            None => {}
        }
    }
    // Creeps: on open walkable ground, clear of the sites, the halls' yards and each other.
    let mines: Vec<(f64, f64)> = map
        .entities
        .iter()
        .filter(|e| e.definition.as_str().contains("amber"))
        .map(|e| (e.position.x, e.position.y))
        .collect();
    let mut offsets: Vec<(i64, i64)> = (-10..=10)
        .flat_map(|i| (-10..=10).map(move |j| (i, j)))
        .filter(|(i, j)| i * i + j * j <= 100)
        .collect();
    offsets.sort_by(|a, b| {
        ((a.0 * a.0 + a.1 * a.1), a.1, a.0).cmp(&((b.0 * b.0 + b.1 * b.1), b.1, b.0))
    });
    let members: Vec<usize> = (0..map.entities.len())
        .filter(|&i| !map.entities[i].definition.as_str().contains("amber"))
        .collect();
    let radius = |definition: &str| {
        content
            .definition(definition)
            .and_then(|d| d.dimensions.as_ref())
            .map_or(1.0, |d| d.radius)
    };
    let mut placed: Vec<(f64, f64, f64)> = Vec::new();
    for i in members {
        let (x0, z0) = (map.entities[i].position.x, map.entities[i].position.y);
        let r = radius(map.entities[i].definition.as_str());
        let fits = |x: f64, z: f64| {
            walkable(x.round() as i64, z.round() as i64)
                && mines.iter().all(|&(mx, mz)| {
                    (mx - x).abs() >= 2.0 + r + 0.5 || (mz - z).abs() >= 2.0 + r + 0.5
                })
                && starts
                    .iter()
                    .all(|&(sx, sz)| (sx - x).hypot(sz - z) >= 16.0)
                && placed
                    .iter()
                    .all(|&(px, pz, pr)| (px - x).hypot(pz - z) >= pr + r + 0.1)
        };
        match offsets
            .iter()
            .find(|&&(dx, dz)| fits(x0 + dx as f64, z0 + dz as f64))
        {
            Some(&(dx, dz)) => {
                if (dx, dz) != (0, 0) {
                    map.entities[i].position.x += dx as f64;
                    map.entities[i].position.y += dz as f64;
                    moved += 1;
                }
                placed.push((map.entities[i].position.x, map.entities[i].position.y, r));
            }
            None => {
                warnings.push(format!(
                    "{}: no open ground nearby; check it in the map editor.",
                    map.entities[i].id
                ));
                placed.push((x0, z0, r));
            }
        }
    }
    Ok((moved, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_find_the_nearest_ground_this_game_has() {
        assert_eq!(substitute("Ldrt", "vibrant-forest"), Ground::Paint(DIRT));
        assert_eq!(substitute("Vgrs", "vibrant-forest"), Ground::Paint(GRASS));
        assert_eq!(substitute("Agrd", "deep-forest"), Ground::Paint(DARK_GRASS));
        assert_eq!(substitute("Bdsr", "vibrant-forest"), Ground::Paint(SAND));
        assert_eq!(
            substitute("Bgrr", "vibrant-forest"),
            Ground::Paint(GRASSY_DIRT)
        );
        assert_eq!(substitute("Ysqd", "vibrant-forest"), Ground::Paint(ROCK));
        assert_eq!(substitute("Bdrh", "vibrant-forest"), Ground::Paint(ROUGH));
        // Winter keeps its own look: snow is the biome's soil, its grass the biome's.
        assert_eq!(substitute("Wsnw", "frozen-forest"), Ground::Soil);
        assert_eq!(substitute("Wgrs", "frozen-forest"), Ground::Grass);
        assert_eq!(substitute("Wrok", "frozen-forest"), Ground::Rock);
        // Something never seen still becomes ground.
        assert_eq!(substitute("Qxyz", "vibrant-forest"), Ground::Paint(DIRT));
        assert_eq!(biome('N'), "frozen-forest");
        assert_eq!(biome('L'), "vibrant-forest");
    }
}
