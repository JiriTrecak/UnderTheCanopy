//! The format rules of the old `readUtcMap` (src/shared/map/utcmap.ts) and the schemas it
//! applies, beyond what the types already enforce.

use super::{
    BIOMES, Mission, PlayerStart, Stamp, UtcMap,
    authoring::{AuthoredObject, SavedScene},
    stamp::STAMP_KEYS,
};
use crate::{
    error::Error,
    placement::{Camp, Placement, Point},
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

struct Report<'a> {
    file: &'a str,
    errors: &'a mut Vec<Error>,
}

impl Report<'_> {
    fn fail(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.errors.push(Error::new(self.file, location, message));
    }

    /// Fails unless `ok`.
    fn expect(&mut self, ok: bool, location: impl Into<String>, message: impl Into<String>) {
        if !ok {
            self.fail(location, message);
        }
    }

    /// A string whose length (in UTF-16 units, as JavaScript counts) is within `min..=max`.
    fn text(&mut self, location: &str, text: &str, min: usize, max: usize) {
        let length = utf16_len(text);
        let message = if max == usize::MAX {
            format!("must have at least {min} character(s)")
        } else {
            format!("length must be {min} to {max}, got {length}")
        };
        self.expect((min..=max).contains(&length), location, message);
    }

    fn range(&mut self, location: &str, value: f64, min: f64, max: f64) {
        self.expect(
            (min..=max).contains(&value),
            location,
            format!("must be {min} to {max}, got {value}"),
        );
    }

    fn count(&mut self, location: &str, count: usize, min: usize, max: usize) {
        self.expect(
            (min..=max).contains(&count),
            location,
            if max == usize::MAX {
                format!("must have at least {min} entries")
            } else {
                format!("must have {min} to {max} entries, got {count}")
            },
        );
    }
}

/// Number of UTF-16 code units, JavaScript's `String.length`.
pub(super) fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// JavaScript's `String.prototype.trim`.
pub(super) fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    })
}

/// A multiple of 0.5.
fn half_step(value: f64) -> bool {
    (value * 2.0).fract() == 0.0
}

/// `^[a-zA-Z][\w.-]*$`
fn script_id(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// The authoring id rule: 1 to 160 characters, `^[a-zA-Z0-9][a-zA-Z0-9._-]*$`.
fn authoring_id(text: &str) -> bool {
    let mut chars = text.chars();
    text.len() <= 160
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn unique<T: Ord>(items: impl IntoIterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    items.into_iter().all(|item| seen.insert(item))
}

const LANDSCAPE_KEYS: [&str; 3] = ["environment", "water", "decals"];
const ENVIRONMENT_KEYS: [&str; 3] = ["hour", "playing", "weather"];
const DECAL_KEYS: [&str; 8] = [
    "locked", "id", "kind", "x", "z", "size", "rotation", "opacity",
];

/// Rules on the raw document that the types cannot express: retired keys, keys the old
/// parser rejects with a specific reason, and `null`, which it accepts nowhere it reads.
/// Offending keys are removed so the typed parse can report everything else.
pub(super) fn raw(value: &mut Value, file: &str, errors: &mut Vec<Error>) {
    let mut report = Report { file, errors };
    let Some(document) = value.as_object_mut() else {
        return;
    };
    if document.remove("height").is_some() | document.remove("waterLevel").is_some() {
        report.fail(
            "",
            "Terrain must be stored as committed cells, not legacy height/water fields",
        );
    }
    if let Some(Value::Object(landscape)) = document.get_mut("landscape") {
        let foreign: Vec<String> = landscape
            .keys()
            .filter(|k| !LANDSCAPE_KEYS.contains(&k.as_str()))
            .cloned()
            .collect();
        if !foreign.is_empty() {
            report.fail(
                "landscape",
                "only environment, water appearance and placed decals belong in a saved map",
            );
            for key in foreign {
                landscape.remove(&key);
            }
        }
    }
    for (key, value) in document.iter() {
        match (key.as_str(), value) {
            ("stamps", Value::Array(stamps)) => {
                for (i, stamp) in stamps.iter().enumerate() {
                    if let Value::Object(fields) = stamp {
                        known_nulls(&mut report, &format!("stamps[{i}]"), fields, &STAMP_KEYS);
                    }
                }
            }
            ("landscape", Value::Object(landscape)) => {
                for (key, value) in landscape {
                    let location = format!("landscape.{key}");
                    match (key.as_str(), value) {
                        // Retired: anything goes.
                        ("water", _) => {}
                        ("environment", Value::Object(environment)) => {
                            known_nulls(&mut report, &location, environment, &ENVIRONMENT_KEYS);
                        }
                        ("decals", Value::Array(decals)) => {
                            for (i, decal) in decals.iter().enumerate() {
                                if let Value::Object(fields) = decal {
                                    let location = format!("{location}[{i}]");
                                    known_nulls(&mut report, &location, fields, &DECAL_KEYS);
                                }
                            }
                        }
                        _ => no_nulls(&mut report, &location, value),
                    }
                }
            }
            _ => no_nulls(&mut report, key, value),
        }
    }
}

fn has_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.iter().any(has_null),
        Value::Object(fields) => fields.values().any(has_null),
        _ => false,
    }
}

/// `null` is rejected anywhere below a strict schema.
fn no_nulls(report: &mut Report, location: &str, value: &Value) {
    if !has_null(value) {
        return;
    }
    match value {
        Value::Null => report.fail(location, "must not be null"),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                no_nulls(report, &format!("{location}[{i}]"), item);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                no_nulls(report, &format!("{location}.{key}"), item);
            }
        }
        _ => {}
    }
}

/// Lenient objects: unknown keys may hold anything, known ones never `null`.
fn known_nulls(report: &mut Report, location: &str, fields: &Map<String, Value>, known: &[&str]) {
    for (key, value) in fields {
        if known.contains(&key.as_str()) {
            no_nulls(report, &format!("{location}.{key}"), value);
        }
    }
}

/// Every rule `readUtcMap` applies to the typed document.
pub(super) fn document(map: &UtcMap, file: &str, errors: &mut Vec<Error>) {
    let mut report = Report { file, errors };
    let r = &mut report;
    r.expect(
        super::valid_size(map.size),
        "size",
        format!(
            "must be a multiple of 32 from 128 to 1024, got {}",
            map.size
        ),
    );
    if let Some(p) = &map.playable {
        r.expect(
            p.width >= 64
                && p.height >= 64
                && p.x + p.width <= map.size
                && p.z + p.height <= map.size,
            "playable",
            "must be at least 64 cells across and lie on the map",
        );
    }
    if let Some(biome) = &map.biome {
        r.expect(
            BIOMES.contains(&biome.as_str()),
            "biome",
            format!("unknown {biome:?}"),
        );
    }
    if let Some(mission) = &map.mission {
        self::mission(r, mission);
    }
    authoring(r, &map.authoring);
    r.expect(
        map.authoring.terrain.size == map.size,
        "authoring.terrain.size",
        "must match map size",
    );
    r.expect(
        !(map.sandbox && map.mission.is_some()),
        "sandbox",
        "A testbed (sandbox) cannot also be a mission",
    );
    // How few starts a map may have is validation's to say (`validate`, "Place Player 1
    // and Player 2"): a map being edited may lack them for a while.
    let most = if map.sandbox { 1 } else { 8 };
    r.count("playerStarts", map.player_starts.len(), 0, most);
    for (i, start) in map.player_starts.iter().enumerate() {
        player_start(r, &format!("playerStarts[{i}]"), start, map.size);
    }
    if map.sandbox
        && let Some(first) = map.player_starts.first()
    {
        r.expect(
            first.player == 1,
            "playerStarts[0].player",
            "a testbed uses player 1",
        );
    }
    r.expect(
        unique(map.player_starts.iter().map(|s| s.player)),
        "playerStarts",
        "duplicate player number",
    );
    for (i, placement) in map.entities.iter().enumerate() {
        self::placement(r, &format!("entities[{i}]"), placement, map.size);
    }
    for (i, camp) in map.camps.iter().enumerate() {
        self::camp(r, &format!("camps[{i}]"), camp);
    }
    if let Some(description) = &map.description {
        r.expect(
            utf16_len(description) <= 1200,
            "description",
            "must be a string of at most 1200 characters",
        );
    }
    for (i, stamp) in map.stamps.iter().enumerate() {
        if !valid_stamp(stamp) {
            r.fail(
                format!("stamps[{i}]"),
                format!("invalid stamp {:?}", stamp.id),
            );
        }
    }
    if let Some(landscape) = &map.landscape {
        r.range(
            "landscape.environment.hour",
            landscape.environment.hour,
            0.0,
            24.0,
        );
        if let Some(decals) = &landscape.decals {
            r.count("landscape.decals", decals.len(), 0, 2048);
            for (i, d) in decals.iter().enumerate() {
                let valid = (1..=128).contains(&utf16_len(&d.id))
                    && d.x.abs() <= 2048.0
                    && d.z.abs() <= 2048.0
                    && (0.5..=32.0).contains(&d.size)
                    && d.rotation.abs() <= 360.0
                    && (0.0..=1.0).contains(&d.opacity);
                r.expect(valid, format!("landscape.decals[{i}]"), "invalid decal");
            }
            r.expect(
                unique(decals.iter().map(|d| d.id.as_str())),
                "landscape.decals",
                "duplicate decal id",
            );
        }
    }
}

fn player_start(r: &mut Report, at: &str, start: &PlayerStart, size: u32) {
    r.range(&format!("{at}.player"), f64::from(start.player), 1.0, 8.0);
    for (axis, value) in [("x", start.x), ("z", start.z)] {
        let location = format!("{at}.{axis}");
        r.expect(half_step(value), &location, "must be a multiple of 0.5");
        r.range(&location, value, 0.0, 2047.0);
    }
    if let Some(rotation) = start.rotation {
        r.expect(
            matches!(rotation, 0 | 90 | 180 | 270),
            format!("{at}.rotation"),
            "must be 0, 90, 180 or 270",
        );
    }
    let size = f64::from(size);
    r.expect(
        start.x < size && start.z < size,
        "playerStarts",
        format!(
            "player {} at {},{} is outside the {size} map",
            start.player, start.x, start.z
        ),
    );
}

fn point(r: &mut Report, at: &str, point: &Point, step: fn(f64) -> bool, what: &str) {
    for (axis, value) in [("x", point.x), ("y", point.y)] {
        let location = format!("{at}.{axis}");
        r.expect(step(value), &location, format!("must be {what}"));
        r.range(&location, value, 0.0, 2047.0);
    }
    if let Some(surface) = &point.surface {
        r.text(&format!("{at}.surface"), surface, 1, 160);
    }
}

fn placement(r: &mut Report, at: &str, p: &Placement, size: u32) {
    r.text(&format!("{at}.id"), &p.id, 1, usize::MAX);
    point(
        r,
        &format!("{at}.position"),
        &p.position,
        half_step,
        "a multiple of 0.5",
    );
    if let Some(state) = &p.initial_state {
        r.expect(
            state.health != Some(0),
            format!("{at}.initialState.health"),
            "must be positive",
        );
        r.expect(
            state.quantity != Some(0),
            format!("{at}.initialState.quantity"),
            "must be positive",
        );
    }
    if let Some(scale) = p.appearance.as_ref().and_then(|a| a.scale) {
        r.expect(
            scale > 0.0,
            format!("{at}.appearance.scale"),
            "must be positive",
        );
    }
    let size = f64::from(size);
    r.expect(
        p.position.x < size && p.position.y < size,
        "entities",
        format!(
            "{} at {},{} is outside the {size} map",
            p.id, p.position.x, p.position.y
        ),
    );
}

fn camp(r: &mut Report, at: &str, camp: &Camp) {
    r.text(&format!("{at}.id"), &camp.id, 1, usize::MAX);
    r.count(&format!("{at}.members"), camp.members.len(), 1, usize::MAX);
    point(
        r,
        &format!("{at}.home"),
        &camp.home,
        |v| v.fract() == 0.0,
        "an integer",
    );
    r.range(
        &format!("{at}.aggroRange"),
        f64::from(camp.aggro_range),
        1.0,
        64.0,
    );
    r.range(&format!("{at}.leash"), f64::from(camp.leash), 1.0, 96.0);
    if let Some(drops) = &camp.fixed_drops {
        r.count(&format!("{at}.fixedDrops"), drops.len(), 0, 12);
    }
    r.expect(
        !(camp.loot_pool.is_some() && camp.fixed_drops.is_some()),
        at,
        "Choose a loot pool or fixed drops, not both",
    );
}

fn mission(r: &mut Report, m: &Mission) {
    if let Some(objectives) = &m.objectives {
        r.count("mission.objectives", objectives.len(), 0, 64);
        for (i, objective) in objectives.iter().enumerate() {
            let at = format!("mission.objectives[{i}]");
            r.expect(script_id(&objective.id), format!("{at}.id"), "invalid id");
            r.text(&format!("{at}.title"), &objective.title, 1, 120);
            r.text(&format!("{at}.description"), &objective.description, 1, 500);
        }
        r.expect(
            unique(objectives.iter().map(|o| o.id.as_str())),
            "mission",
            "Objective IDs must be unique",
        );
    }
    r.text("mission.script", &m.script, 1, 64000);
    r.count("mission.regions", m.regions.len(), 0, 128);
    for (i, region) in m.regions.iter().enumerate() {
        let at = format!("mission.regions[{i}]");
        r.expect(script_id(&region.id), format!("{at}.id"), "invalid id");
        r.range(&format!("{at}.x"), f64::from(region.x), 0.0, 2047.0);
        r.range(&format!("{at}.y"), f64::from(region.y), 0.0, 2047.0);
        r.expect(
            region.radius > 0.0 && region.radius <= 64.0,
            format!("{at}.radius"),
            "must be above 0 and at most 64",
        );
    }
    r.expect(
        unique(m.regions.iter().map(|g| g.id.as_str())),
        "mission",
        "Region IDs must be unique",
    );
    for (at, message) in super::players::check(m) {
        r.fail(at, message);
    }
}

fn authoring(r: &mut Report, scene: &SavedScene) {
    let terrain = &scene.terrain;
    let errors_before = r.errors.len();
    let size = terrain.size;
    r.expect(
        (16..=2048).contains(&size) && size.is_multiple_of(4),
        "authoring.terrain.size",
        "must be a multiple of 4 from 16 to 2048",
    );
    if let Some(grid) = &terrain.texture_grid {
        r.range(
            "authoring.terrain.textureGrid.cellSize",
            grid.cell_size,
            1.0,
            64.0,
        );
    }
    r.count(
        "authoring.terrain.waterProfiles",
        terrain.water_profiles.len(),
        0,
        255,
    );
    for (i, profile) in terrain.water_profiles.iter().enumerate() {
        r.text(
            &format!("authoring.terrain.waterProfiles[{i}]"),
            profile,
            1,
            usize::MAX,
        );
    }
    r.count("authoring.terrain.paint", terrain.paint.len(), 0, 256);
    for (i, layer) in terrain.paint.iter().enumerate() {
        r.text(
            &format!("authoring.terrain.paint[{i}].material"),
            &layer.material,
            1,
            usize::MAX,
        );
    }
    // The grid refinement runs only on a structurally valid terrain document.
    if r.errors.len() == errors_before {
        for error in terrain.grid_errors() {
            r.fail(format!("authoring.terrain.{}", error.grid), error.message);
        }
    }
    r.count("authoring.objects", scene.objects.len(), 0, 200_000);
    for (i, object) in scene.objects.iter().enumerate() {
        authored_object(r, &format!("authoring.objects[{i}]"), object);
    }
    r.expect(
        unique(scene.objects.iter().map(|o| o.id.as_str())),
        "authoring.objects",
        "Duplicate object ID",
    );
}

fn authored_object(r: &mut Report, at: &str, o: &AuthoredObject) {
    r.expect(authoring_id(&o.id), format!("{at}.id"), "invalid id");
    r.expect(
        authoring_id(&o.asset),
        format!("{at}.asset"),
        "invalid asset id",
    );
    r.range(&format!("{at}.x"), o.x, -4096.0, 4096.0);
    r.range(&format!("{at}.z"), o.z, -4096.0, 4096.0);
    r.range(&format!("{at}.elevation"), o.elevation, -128.0, 128.0);
    r.expect(
        o.scale > 0.0 && o.scale <= 100.0,
        format!("{at}.scale"),
        "must be above 0 and at most 100",
    );
}

fn valid_stamp(s: &Stamp) -> bool {
    let quarter = std::f64::consts::FRAC_PI_2;
    let axes = [s.height_scale, s.width_scale, s.depth_scale]
        .into_iter()
        .flatten()
        .all(|v| (0.25..=4.0).contains(&v));
    let angles = [s.pitch, s.roll]
        .into_iter()
        .flatten()
        .all(|v| v.abs() <= quarter);
    let elevation = s.elevation.is_none_or(|e| e.abs() <= 32.0);
    let walk = s.walk.as_ref().is_none_or(|w| {
        let level = |l: u32| l <= 31;
        let mesh = w.mesh.as_ref().is_none_or(|m| {
            (3..=4096).contains(&m.positions.len())
                && (3..=24576).contains(&m.indices.len())
                && m.indices.len() % 3 == 0
                && m.indices.iter().all(|&i| (i as usize) < m.positions.len())
        });
        let connections = w
            .connections
            .as_ref()
            .is_none_or(|c| c.start.is_none_or(level) && c.end.is_none_or(level));
        mesh && (1..=31).contains(&w.level)
            && w.height.is_none_or(|h| (-16.0..=64.0).contains(&h))
            && connections
    });
    let transform = s.source_transform.as_ref().is_none_or(|t| {
        let [x, y, z, w] = t.quaternion;
        ((x * x + y * y + z * z + w * w).sqrt() - 1.0).abs() < 0.001
    });
    axes && angles && elevation && walk && transform
}
