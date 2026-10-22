//! Map packages (`.utcm`, docs/packages.md): a map as a folder (or, later, a zip of one).
//!
//! ```text
//! frozen-dream.utcm/
//! ├─ package.json      id, name, description, version, dependencies (kind "map")
//! ├─ map.json          size, biome, starts, placements, camps, environment, mission
//! │                    (without its script)
//! ├─ scenery.json      {"objects": […]}: placed scenery, in placement order
//! ├─ terrain/          blocks and (block-paint), what the map is made of (D80), and what
//! │                    they make: heights, water-heights, water-flow and (blocked)
//! │                    (.grid, see [`super::grid`])
//! ├─ mission.lua       the mission script, when there is a mission
//! ├─ autoplay.lua      a mission's autoplay plan, when it has one (not in the identity)
//! └─ preview.webp      the menus' picture
//! ```
//!
//! In memory a map is still one [`UtcMap`]; [`load`] assembles it from the package and
//! checks it as before, [`write`] splits one into a package. A map's ground (paint layers,
//! grass, rock, shade) is not stored: its biome generates it from the blocks at load
//! (`canopy_scene::generate`), so in memory those layers start empty.

use super::{
    UtcMap, check,
    grid::{Element, GRID_EXTENSION, Grid},
};
use crate::{
    error::{Error, Errors, parse},
    package::{MANIFEST, Manifest, Package, PackageKind},
};
use serde_json::{Map, Value, json};
use std::path::Path;

pub const MAP_FILE: &str = "map.json";
pub const SCENERY_FILE: &str = "scenery.json";
pub const MISSION_SCRIPT: &str = "mission.lua";
/// A mission's autoplay plan (docs/campaign.md "Autoplay"), outside the map's identity.
pub const AUTOPLAY_SCRIPT: &str = "autoplay.lua";
pub const PREVIEW_FILE: &str = "preview.webp";
pub const TERRAIN_DIR: &str = "terrain";

/// The grids the blocks make: (file stem, key in the terrain document, element, per
/// vertex).
const GRIDS: [(&str, &str, Element, bool); 3] = [
    ("heights", "heights", Element::F32, true),
    ("water-heights", "waterHeights", Element::F32, true),
    ("water-flow", "waterFlow", Element::Rgba8, false),
];

fn grid_path(stem: &str) -> String {
    format!("{TERRAIN_DIR}/{stem}.{GRID_EXTENSION}")
}

fn read_json(package: &Package, file: &str) -> Result<Value, Error> {
    let label = package.label(file);
    let text = package
        .read_text(file)
        .map_err(|e| Error::new(&label, "", e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| Error::new(&label, "", e.to_string()))
}

/// Reads and checks the map package at `dir`.
pub fn load(dir: &Path) -> Result<UtcMap, Errors> {
    let package = Package::open(dir).map_err(|e| Errors(vec![e]))?;
    load_package(&package)
}

/// Reads and checks a map package, through its overlay when it has one (the editor's
/// unsaved documents and grids).
pub fn load_package(package: &Package) -> Result<UtcMap, Errors> {
    let package = package.clone();
    let label = package.label("");
    if package.manifest.kind != PackageKind::Map {
        return Err(Errors(vec![Error::new(
            &package.label(MANIFEST),
            "kind",
            "not a map package",
        )]));
    }
    let one = |e: Error| Errors(vec![e]);
    let mut document = match read_json(&package, MAP_FILE).map_err(one)? {
        Value::Object(fields) => fields,
        _ => {
            return Err(one(Error::new(
                &package.label(MAP_FILE),
                "",
                "expected an object",
            )));
        }
    };
    let scenery = read_json(&package, SCENERY_FILE).map_err(one)?;
    let objects = scenery
        .get("objects")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    // The terrain is the grids in terrain/, filled in below once the map has its size.
    if document.contains_key("terrain") {
        return Err(one(Error::new(
            &package.label(MAP_FILE),
            "terrain",
            "a map keeps its terrain as blocks in terrain/ (D80); this map is in the old format",
        )));
    }
    let mut terrain = Map::new();
    let size = document.get("size").cloned().unwrap_or(Value::Null);
    terrain.insert("version".into(), json!(1));
    terrain.insert("size".into(), size);
    for key in ["heights", "waterHeights", "waterFlow", "grass", "rock"] {
        terrain.insert(key.into(), json!(""));
    }
    terrain.insert("waterProfiles".into(), json!([]));
    terrain.insert("paint".into(), json!([]));
    if let Some(Value::Object(mission)) = document.get_mut("mission") {
        let script = package.read_text(MISSION_SCRIPT).map_err(|e| {
            one(Error::new(
                &package.label(MISSION_SCRIPT),
                "",
                e.to_string(),
            ))
        })?;
        mission.insert("script".into(), Value::String(script));
        if let Ok(plan) = package.read_text(AUTOPLAY_SCRIPT) {
            mission.insert("autoplay".into(), Value::String(plan));
        }
    }
    document.insert("v".into(), json!(super::UTCMAP_VERSION));
    document.insert("name".into(), json!(package.manifest.name));
    if !package.manifest.description.is_empty() {
        document.insert("description".into(), json!(package.manifest.description));
    }
    document.insert(
        "authoring".into(),
        json!({"version": 1, "objects": objects, "terrain": Value::Object(terrain)}),
    );
    let mut value = Value::Object(document);
    let mut errors = Vec::new();
    check::raw(&mut value, &label, &mut errors);
    let mut map: UtcMap = match parse(value, &package.label(MAP_FILE), "") {
        Ok(map) => map,
        Err(error) => {
            errors.push(error);
            return Err(Errors(errors));
        }
    };
    let terrain = &mut map.authoring.terrain;
    let (vertices, span) = (terrain.vertices(), terrain.span());
    let mut read_grid = |stem: &str, element: Element, side: (u32, u32)| -> Grid {
        let file = grid_path(stem);
        package
            .read_bytes(&file)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Grid::from_file(&bytes, element, side.0, side.1))
            .unwrap_or_else(|message| {
                errors.push(Error::new(&package.label(&file), "", message));
                Grid::default()
            })
    };
    for (stem, key, element, per_vertex) in GRIDS {
        let side = if per_vertex {
            (vertices, vertices)
        } else {
            (span, span)
        };
        let grid = read_grid(stem, element, side);
        match key {
            "heights" => terrain.heights = grid,
            "waterHeights" => terrain.water_heights = grid,
            _ => terrain.water_flow = grid,
        }
    }
    // The blocks (D80) are what the map is made of.
    let columns = span / super::blocks::BLOCK;
    terrain.blocks = Some(read_grid("blocks", Element::Rgba8, (columns, columns)));
    // Kinds painted cell by cell, and the closed cells, when the map has them.
    if package.read_bytes(&grid_path("block-paint")).is_ok() {
        terrain.block_paint = Some(read_grid("block-paint", Element::U8, (span, span)));
    }
    if package.read_bytes(&grid_path("blocked")).is_ok() {
        terrain.blocked = Some(read_grid("blocked", Element::U8, (span, span)));
    }
    // No painted ground until the biome generates it.
    let zeros = Grid(vec![0; (vertices * vertices * 4) as usize]);
    terrain.grass = zeros.clone();
    terrain.rock = zeros;
    if !errors.is_empty() {
        return Err(Errors(errors));
    }
    check::document(&map, &package.label(MAP_FILE), &mut errors);
    if errors.is_empty() {
        Ok(map.normalised())
    } else {
        Err(Errors(errors))
    }
}

/// Writes `map` as the package `id` into `dir` (created; existing files replaced).
pub fn write(map: &UtcMap, id: &str, version: &str, dir: &Path) -> std::io::Result<()> {
    let pretty = |value: &Value| {
        let mut text = serde_json::to_string_pretty(value).expect("maps serialise");
        text.push('\n');
        text
    };
    std::fs::create_dir_all(dir.join(TERRAIN_DIR))?;
    let manifest = Manifest {
        format: crate::package::FORMAT,
        id: crate::Id::new(id).map_err(std::io::Error::other)?,
        kind: PackageKind::Map,
        name: map.name.clone(),
        version: version.into(),
        author: String::new(),
        description: map.description.clone().unwrap_or_default(),
        dependencies: [(
            crate::Id::new("base").expect("valid"),
            format!(">={}", env!("CARGO_PKG_VERSION")),
        )]
        .into_iter()
        .collect(),
        license: String::new(),
    };
    std::fs::write(
        dir.join(MANIFEST),
        pretty(&serde_json::to_value(&manifest).expect("manifests serialise")),
    )?;
    let Value::Object(mut document) = serde_json::to_value(map).expect("maps serialise") else {
        unreachable!("a map is an object");
    };
    for key in ["v", "name", "description", "authoring"] {
        document.remove(key);
    }
    if let Some(Value::Object(mission)) = document.get_mut("mission")
        && let Some(Value::String(script)) = mission.remove("script")
    {
        std::fs::write(dir.join(MISSION_SCRIPT), script)?;
    }
    if let Some(Value::Object(mission)) = document.get_mut("mission")
        && let Some(Value::String(plan)) = mission.remove("autoplay")
    {
        std::fs::write(dir.join(AUTOPLAY_SCRIPT), plan)?;
    }
    if document
        .get("stamps")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        document.remove("stamps");
    }
    let terrain = &map.authoring.terrain;
    let Some(blocks) = &terrain.blocks else {
        return Err(std::io::Error::other(
            "a map is made of blocks (D80): this one has none",
        ));
    };
    std::fs::write(dir.join(MAP_FILE), pretty(&Value::Object(document)))?;
    std::fs::write(
        dir.join(SCENERY_FILE),
        pretty(&json!({"objects": map.authoring.objects})),
    )?;
    let (vertices, span) = (terrain.vertices(), terrain.span());
    let write_grid = |stem: &str, grid: &Grid, element: Element, side: (u32, u32)| {
        std::fs::write(
            dir.join(grid_path(stem)),
            grid.to_file(element, side.0, side.1),
        )
    };
    write_grid(
        "heights",
        &terrain.heights,
        Element::F32,
        (vertices, vertices),
    )?;
    write_grid(
        "water-heights",
        &terrain.water_heights,
        Element::F32,
        (vertices, vertices),
    )?;
    write_grid(
        "water-flow",
        &terrain.water_flow,
        Element::Rgba8,
        (span, span),
    )?;
    if let Some(blocked) = &terrain.blocked {
        write_grid("blocked", blocked, Element::U8, (span, span))?;
    }
    let columns = span / super::blocks::BLOCK;
    write_grid("blocks", blocks, Element::Rgba8, (columns, columns))?;
    if let Some(paint) = &terrain.block_paint {
        write_grid("block-paint", paint, Element::U8, (span, span))?;
    }
    Ok(())
}

/// What the menus show of a map, read from its package without the terrain.
#[derive(Debug, Clone, PartialEq)]
pub struct MapSummary {
    pub name: String,
    pub size: u32,
    pub description: Option<String>,
    /// Player (1-based) and start cell (x, z).
    pub starts: Vec<(u8, f64, f64)>,
    pub camps: u32,
    pub sandbox: bool,
    /// A mission map (it has a `mission` section).
    pub mission: bool,
    /// Players (1-based) the map places entities for, sorted.
    pub placed_players: Vec<u8>,
    /// Whether the package has [`PREVIEW_FILE`].
    pub preview: bool,
    /// A mission with an autoplay plan ([`AUTOPLAY_SCRIPT`]).
    pub autoplay: bool,
}

/// Reads [`MapSummary`] of the map package at `dir`: its manifest and `map.json`, not the
/// terrain or scenery. The map is checked in full when it is played.
pub fn summary(dir: &Path) -> Result<MapSummary, Errors> {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(rename_all = "camelCase")]
    struct Start {
        player: u8,
        x: f64,
        z: f64,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Document {
        size: u32,
        #[serde(default)]
        sandbox: bool,
        player_starts: Vec<Start>,
        #[serde(default)]
        camps: Vec<serde::de::IgnoredAny>,
        #[serde(default)]
        entities: Vec<Placed>,
        mission: Option<serde::de::IgnoredAny>,
    }
    #[derive(serde::Deserialize)]
    struct Placed {
        #[serde(default)]
        owner: Option<String>,
    }
    let package = Package::open(dir).map_err(|e| Errors(vec![e]))?;
    let value = read_json(&package, MAP_FILE).map_err(|e| Errors(vec![e]))?;
    let document: Document =
        parse(value, &package.label(MAP_FILE), "").map_err(|e| Errors(vec![e]))?;
    let manifest = &package.manifest;
    Ok(MapSummary {
        name: manifest.name.clone(),
        size: document.size,
        description: (!manifest.description.is_empty()).then(|| manifest.description.clone()),
        starts: document
            .player_starts
            .iter()
            .map(|s| (s.player, s.x, s.z))
            .collect(),
        camps: document.camps.len() as u32,
        sandbox: document.sandbox,
        mission: document.mission.is_some(),
        placed_players: {
            let mut players: Vec<u8> = document
                .entities
                .iter()
                .filter_map(|e| e.owner.as_deref()?.strip_prefix("player.")?.parse().ok())
                .collect();
            players.sort_unstable();
            players.dedup();
            players
        },
        preview: package.root.join(PREVIEW_FILE).is_file(),
        autoplay: document.mission.is_some() && package.root.join(AUTOPLAY_SCRIPT).is_file(),
    })
}
