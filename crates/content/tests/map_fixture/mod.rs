//! Test-owned `.utcmap` documents: a flat 256-cell map with two player starts.
#![allow(dead_code)]

use canopy_content::{Errors, map::UtcMap};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub const SIZE: u32 = 256;

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const SYMBOLS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(SYMBOLS[(word >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn floats(values: impl IntoIterator<Item = f32>) -> String {
    let bytes: Vec<u8> = values.into_iter().flat_map(f32::to_le_bytes).collect();
    base64(&bytes)
}

pub fn vertex_count(size: u32) -> usize {
    ((size + 33) as usize).pow(2)
}

/// Flat committed terrain, as a new map gets it: ground 0, dry water at -1, still flow.
pub fn terrain(size: u32) -> Value {
    let n = vertex_count(size);
    let span = (size + 32) as usize;
    let flow: Vec<u8> = (0..span * span).flat_map(|_| [128, 128, 0, 0]).collect();
    let zero = floats(std::iter::repeat_n(0.0, n));
    json!({
        "version": 1,
        "size": size,
        "heights": zero,
        "waterHeights": floats(std::iter::repeat_n(-1.0, n)),
        "waterFlow": base64(&flow),
        "waterProfiles": [],
        "grass": zero,
        "rock": zero,
        "paint": [],
    })
}

/// A valid skirmish map: two starts, no entities, one scenery object.
pub fn map() -> Value {
    static MAP: OnceLock<Value> = OnceLock::new();
    MAP.get_or_init(|| {
        json!({
            "v": 3,
            "size": SIZE,
            "name": "Fixture",
            "stamps": [],
            "playerStarts": [
                {"player": 1, "x": 40.5, "z": 40.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
                {"player": 2, "x": 200.5, "z": 200.5, "setup": "setup.test", "mainFort": "start.player.2/main-fort"},
            ],
            "entities": [],
            "camps": [],
            "authoring": {
                "version": 1,
                "terrain": terrain(SIZE),
                "objects": [{"id": "rock.1", "asset": "asset.models.rock", "x": 10, "z": 12}],
            },
        })
    })
    .clone()
}

pub fn parse(value: Value) -> Result<UtcMap, Errors> {
    UtcMap::from_value(value, "fixture.utcmap")
}

/// Sets `value` at a `/`-separated JSON pointer, creating objects on the way.
pub fn set(document: &mut Value, pointer: &str, value: Value) {
    let (parent, key) = pointer.rsplit_once('/').expect("pointer with a key");
    let mut target = document;
    for part in parent.split('/').filter(|p| !p.is_empty()) {
        target = match target {
            Value::Array(items) => &mut items[part.parse::<usize>().expect("index")],
            Value::Object(fields) => fields.entry(part).or_insert_with(|| json!({})),
            _ => panic!("cannot descend into {pointer}"),
        };
    }
    match target {
        Value::Array(items) => items[key.parse::<usize>().expect("index")] = value,
        Value::Object(fields) => {
            fields.insert(key.into(), value);
        }
        _ => panic!("cannot set {pointer}"),
    }
}

pub fn remove(document: &mut Value, pointer: &str) {
    let (parent, key) = pointer.rsplit_once('/').expect("pointer with a key");
    document
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .expect("parent object")
        .remove(key);
}

/// The map with `pointer` set to `value`.
pub fn with(pointer: &str, value: Value) -> Value {
    let mut document = map();
    set(&mut document, pointer, value);
    document
}
