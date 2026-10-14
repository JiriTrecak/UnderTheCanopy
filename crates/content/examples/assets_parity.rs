//! Compares the Rust reader of published asset data with the old TypeScript game on the
//! same data (`just parity-assets`).
//!
//! Usage: assets_parity <data folder> <dump from tools/parity/assets-ts.mts>

use canopy_content::{
    Content,
    assets::{self, landscape, models, previews, textures},
};
use serde_json::{Value, json};
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, dump] = args.as_slice() else {
        eprintln!("usage: assets_parity <data folder> <ts dump>");
        return ExitCode::FAILURE;
    };
    let data = PathBuf::from(data);
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(dump).expect("readable dump"))
            .expect("JSON dump");
    let content = match Content::load(&data) {
        Ok(content) => content,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };
    let mut errors = Vec::new();
    let landscape = landscape::load(&data, &mut errors);
    let models = models::load(&data, &mut errors);
    let previews = previews::load(&data, &mut errors);
    let file = |key: &str| expected[key]["file"].as_str().expect("dump names the file");
    let grades = textures::load_daytime_grades(&data, file("daytimeGrades"), &mut errors);
    let cube = textures::load_cube_metadata(&data, file("reflectionCube"), &mut errors);
    let brdf = textures::load_table_metadata(&data, file("brdfTable"), &mut errors);
    if let Some(landscape) = &landscape {
        landscape::validate(
            landscape,
            &content.manifest,
            &content.definitions,
            &mut errors,
        );
    }
    if let Some(models) = &models {
        models::validate(models, &content.manifest, &mut errors);
    }
    for error in &errors {
        eprintln!("{error}");
    }

    let actual = json!({
        "manifest": content.manifest,
        "renderAssets": assets::render_assets(&content.manifest),
        "scenery": assets::scenery(&content.manifest).collect::<Vec<_>>(),
        "landscape": landscape,
        "models": models,
        "mapPreviews": previews,
        "daytimeGrades": {"file": file("daytimeGrades"), "value": grades},
        "reflectionCube": {"file": file("reflectionCube"), "value": cube},
        "brdfTable": {"file": file("brdfTable"), "value": brdf},
    });
    let mut differences = Vec::new();
    compare("", &expected, &actual, &mut differences);
    for difference in differences.iter().take(50) {
        println!("{difference}");
    }
    if differences.is_empty() && errors.is_empty() {
        println!("parity-assets: identical");
        ExitCode::SUCCESS
    } else {
        println!(
            "parity-assets: {} difference(s), {} error(s)",
            differences.len(),
            errors.len()
        );
        ExitCode::FAILURE
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
