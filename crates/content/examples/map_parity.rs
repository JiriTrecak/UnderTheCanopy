//! Compares the Rust map reader with the old TypeScript parser on every shipped map
//! (`just parity-maps`), and reports how long each load takes.
//!
//! Usage: map_parity <data folder> <dump from tools/parity/maps-ts.mts>

use canopy_content::{Content, map};
use serde_json::Value;
use std::{path::PathBuf, process::ExitCode, time::Instant};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, dump] = args.as_slice() else {
        eprintln!("usage: map_parity <data folder> <ts dump>");
        return ExitCode::FAILURE;
    };
    let data = PathBuf::from(data);
    let content = match Content::load(&data) {
        Ok(content) => content,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };
    let maps = match map::library(&data) {
        Ok(maps) => maps,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(dump).expect("readable dump"))
            .expect("JSON dump");
    let mut expected = match expected {
        Value::Object(mut root) => match root.remove("maps") {
            Some(Value::Object(maps)) => maps,
            _ => panic!("dump without maps"),
        },
        _ => panic!("dump is not an object"),
    };

    let mut failed = false;
    for file in &maps {
        let Some(ts) = expected.remove(&file.id) else {
            println!("{}: missing in the TypeScript dump", file.id);
            failed = true;
            continue;
        };
        let started = Instant::now();
        let loaded = file.load();
        let load_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let decoded = loaded.as_ref().ok().map(|m| m.authoring.terrain.decode());
        let decode_ms = started.elapsed().as_secs_f64() * 1000.0;
        let ts_ms = ts["parseMs"].as_f64().unwrap_or(f64::NAN);
        println!(
            "{}: load {load_ms:.0} ms, grid decode {decode_ms:.0} ms (TS parse {ts_ms:.0} ms)",
            file.id
        );
        let mut differences = Vec::new();
        match (&loaded, ts.get("map")) {
            (Ok(map), Some(expected)) => {
                let mut expected = expected.clone();
                normalise_ts(&mut expected);
                let actual = serde_json::to_value(map).expect("serialisable map");
                compare("", &expected, &actual, &mut differences);
                if let Some(Err(error)) = decoded {
                    differences.push(format!("grids do not decode: {error}"));
                }
                let errors = map::validate(map, &content);
                match ts["playable"].as_str() {
                    None => {
                        for error in &errors {
                            differences.push(format!("Rust rejects a playable map: {error}"));
                        }
                    }
                    Some(reason) if errors.is_empty() => println!(
                        "  TS rejects ({reason}); Rust passes: expected only for checks the \
                         simulation owns"
                    ),
                    Some(_) => {}
                }
            }
            (Err(errors), None) => println!(
                "  both reject: TS {:?}; Rust {errors}",
                ts["error"].as_str().unwrap_or("?")
            ),
            (Ok(_), None) => differences.push(format!("TS rejects: {}", ts["error"])),
            (Err(errors), Some(_)) => differences.push(format!("Rust rejects: {errors}")),
        }
        for difference in differences.iter().take(50) {
            println!("  {difference}");
        }
        if differences.is_empty() {
            println!("  identical");
        } else {
            println!("  {} difference(s)", differences.len());
            failed = true;
        }
    }
    for id in expected.keys() {
        println!("{id}: only in the TypeScript dump");
        failed = true;
    }
    if failed {
        println!("parity-maps: differences found");
        ExitCode::FAILURE
    } else {
        println!("parity-maps: identical ({} maps)", maps.len());
        ExitCode::SUCCESS
    }
}

/// The TypeScript parser adds the generator inputs `strokes` and `cover` to a saved
/// landscape, always empty (a saved map may not carry them); the Rust type omits them.
fn normalise_ts(map: &mut Value) {
    if let Some(Value::Object(landscape)) = map.get_mut("landscape") {
        for key in ["strokes", "cover"] {
            if landscape.get(key) == Some(&Value::Array(Vec::new())) {
                landscape.remove(key);
            }
        }
    }
}

/// Structural comparison where numbers compare by value (`1` equals `1.0`).
fn compare(path: &str, expected: &Value, actual: &Value, out: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => {}
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                match b.get(key) {
                    Some(other) => compare(&format!("{path}.{key}"), value, other, out),
                    None => out.push(format!(
                        "{path}.{key}: missing in Rust (TS {})",
                        short(value)
                    )),
                }
            }
            for key in b.keys().filter(|k| !a.contains_key(*k)) {
                out.push(format!("{path}.{key}: only in Rust ({})", short(&b[key])));
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                compare(&format!("{path}[{index}]"), x, y, out);
            }
        }
        _ if expected == actual => {}
        _ => out.push(format!(
            "{path}: TS {} ≠ Rust {}",
            short(expected),
            short(actual)
        )),
    }
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(120) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    }
}
