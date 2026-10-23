//! Every message key the simulation names (`"message.…"`, `"status.…"`, `msg!("…")`) is in
//! the base package's text library (D96), so players never read a raw key.

use canopy_content::{package::Package, text::Text};
use std::path::Path;

#[test]
fn every_message_key_is_in_the_base_library() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let base = Package::open(&root.join("../../base")).expect("base package");
    let mut errors = Vec::new();
    let text = Text::read(&[base], "en", &mut errors);
    assert!(errors.is_empty(), "{errors:?}");
    let mut missing = Vec::new();
    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            let source = source.split("#[cfg(test)]").next().unwrap_or_default();
            for prefix in ["\"message.", "\"status."] {
                for (at, _) in source.match_indices(prefix) {
                    let key: String = source[at + 1..].chars().take_while(|c| *c != '"').collect();
                    if key.contains(char::is_whitespace) || key.ends_with('.') {
                        continue;
                    }
                    if text.get(&key).is_none() {
                        missing.push(format!("{}: {key}", path.display()));
                    }
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "keys missing from base/text/en: {missing:#?}"
    );
}
