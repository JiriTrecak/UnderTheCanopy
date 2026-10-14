//! Compares the Rust reader with the old TypeScript loader on the same data (`just parity`).
//!
//! Usage: parity <data folder> <dump from tools/parity/dump-ts.mts>

use canopy_content::Content;
use serde_json::{Value, json};
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, dump] = args.as_slice() else {
        eprintln!("usage: parity <data folder> <ts dump>");
        return ExitCode::FAILURE;
    };
    let content = match Content::load(&PathBuf::from(data)) {
        Ok(content) => content,
        Err(errors) => {
            eprintln!("{errors}");
            return ExitCode::FAILURE;
        }
    };
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(dump).expect("readable dump"))
            .expect("JSON dump");
    let actual = json!({
        "definitions": content.definitions,
        "rules": content.rules,
        "actions": content.actions,
        "renderAssets": content.render_assets,
        "abilities": content.abilities,
        "effects": content.effects,
    });
    let mut differences = Vec::new();
    compare("", &expected, &actual, &mut differences);
    for difference in differences.iter().take(50) {
        println!("{difference}");
    }
    if differences.is_empty() {
        println!("parity: identical");
        ExitCode::SUCCESS
    } else {
        println!("parity: {} difference(s)", differences.len());
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
    if text.len() > 120 {
        format!("{}…", &text[..120])
    } else {
        text
    }
}
