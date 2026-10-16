//! Map packages (`.utcm`, docs/packages.md): a map as a folder (or, later, a zip of one).
//!
//! ```text
//! echo-isles.utcm/
//! ├─ package.json      id, name, description, version, dependencies (kind "map")
//! ├─ map.json          size, biome, starts, placements, camps, environment, mission
//! │                    (without its script), terrain metadata and the ordered paint layers
//! ├─ scenery.json      {"objects": […]}: placed scenery, in placement order
//! ├─ terrain/          heights, water-heights, water-flow, grass, rock,
//! │                    paint-<n>-weights and paint-<n>-variants (.grid, see [`super::grid`])
//! ├─ mission.lua       the mission script, when there is a mission
//! └─ preview.webp      the menus' picture
//! ```
//!
//! In memory a map is still one [`UtcMap`]; [`load`] assembles it from the package and
//! checks it as before, [`write`] splits one into a package.

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
pub const PREVIEW_FILE: &str = "preview.webp";
pub const TERRAIN_DIR: &str = "terrain";

/// The grids of a terrain: (file stem, key in the terrain document, element, per vertex).
const GRIDS: [(&str, &str, Element, bool); 5] = [
    ("heights", "heights", Element::F32, true),
    ("water-heights", "waterHeights", Element::F32, true),
    ("water-flow", "waterFlow", Element::Rgba8, false),
    ("grass", "grass", Element::F32, true),
    ("rock", "rock", Element::F32, true),
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
    // The terrain document, its grids filled in below once the map has its size.
    let mut terrain = match document.remove("terrain") {
        Some(Value::Object(fields)) => fields,
        _ => {
            return Err(one(Error::new(
                &package.label(MAP_FILE),
                "terrain",
                "missing terrain",
            )));
        }
    };
    let size = document.get("size").cloned().unwrap_or(Value::Null);
    terrain.insert("version".into(), json!(1));
    terrain.insert("size".into(), size);
    for (_, key, _, _) in GRIDS {
        terrain.insert(key.into(), json!(""));
    }
    let painted: Vec<bool> = terrain
        .get("paint")
        .and_then(Value::as_array)
        .map(|layers| {
            layers
                .iter()
                .map(|l| l.get("variants").and_then(Value::as_bool) == Some(true))
                .collect()
        })
        .unwrap_or_default();
    if let Some(Value::Array(layers)) = terrain.get_mut("paint") {
        for layer in layers {
            layer["weights"] = json!("");
            if layer.get("variants").and_then(Value::as_bool) == Some(true) {
                layer["variants"] = json!("");
            } else if let Some(object) = layer.as_object_mut() {
                object.remove("variants");
            }
        }
    }
    if let Some(Value::Object(mission)) = document.get_mut("mission") {
        let script = package.read_text(MISSION_SCRIPT).map_err(|e| {
            one(Error::new(
                &package.label(MISSION_SCRIPT),
                "",
                e.to_string(),
            ))
        })?;
        mission.insert("script".into(), Value::String(script));
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
    let texture = terrain.texture_layout();
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
            "waterFlow" => terrain.water_flow = grid,
            "grass" => terrain.grass = grid,
            _ => terrain.rock = grid,
        }
    }
    for (i, layer) in terrain.paint.iter_mut().enumerate() {
        let n = i + 1;
        layer.weights = read_grid(
            &format!("paint-{n}-weights"),
            Element::F32,
            (vertices, vertices),
        );
        if painted.get(i) == Some(&true) {
            layer.variants = Some(read_grid(
                &format!("paint-{n}-variants"),
                Element::U8,
                (texture.cells[0], texture.cells[1]),
            ));
        }
    }
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
    if document
        .get("stamps")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        document.remove("stamps");
    }
    let terrain = &map.authoring.terrain;
    let mut meta = Map::new();
    if let Some(grid) = &terrain.texture_grid {
        meta.insert(
            "textureGrid".into(),
            serde_json::to_value(grid).expect("serialises"),
        );
    }
    meta.insert("waterProfiles".into(), json!(terrain.water_profiles));
    meta.insert(
        "paint".into(),
        Value::Array(
            terrain
                .paint
                .iter()
                .map(|p| json!({"material": p.material, "variants": p.variants.is_some()}))
                .collect(),
        ),
    );
    document.insert("terrain".into(), Value::Object(meta));
    std::fs::write(dir.join(MAP_FILE), pretty(&Value::Object(document)))?;
    std::fs::write(
        dir.join(SCENERY_FILE),
        pretty(&json!({"objects": map.authoring.objects})),
    )?;
    let (vertices, span) = (terrain.vertices(), terrain.span());
    let texture = terrain.texture_layout();
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
    write_grid("grass", &terrain.grass, Element::F32, (vertices, vertices))?;
    write_grid("rock", &terrain.rock, Element::F32, (vertices, vertices))?;
    for (i, layer) in terrain.paint.iter().enumerate() {
        let n = i + 1;
        write_grid(
            &format!("paint-{n}-weights"),
            &layer.weights,
            Element::F32,
            (vertices, vertices),
        )?;
        if let Some(variants) = &layer.variants {
            write_grid(
                &format!("paint-{n}-variants"),
                variants,
                Element::U8,
                (texture.cells[0], texture.cells[1]),
            )?;
        }
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
    pub mission: Option<MissionSummary>,
    /// Whether the package has [`PREVIEW_FILE`].
    pub preview: bool,
}

/// The mission fields the menus show.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissionSummary {
    pub campaign: String,
    pub order: u32,
    pub title: String,
    pub presentation: Option<MissionPresentation>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissionPresentation {
    /// `asset.interface.*` image ids.
    pub loading_background: Option<String>,
    pub selection_background: Option<String>,
    pub chapter: Option<String>,
    pub briefing: Option<String>,
    pub tips: Option<Vec<String>>,
    pub features: Option<Vec<String>>,
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
        mission: Option<MissionSummary>,
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
        mission: document.mission,
        preview: package.root.join(PREVIEW_FILE).is_file(),
    })
}
