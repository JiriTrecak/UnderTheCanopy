//! The game's text (D96): the base package's text library, loaded before anything is
//! shown and readable from anywhere, so code says what it means by key (`tr!`) and never
//! holds player-facing English itself.

use canopy_content::text::{Arg, SOURCE_LANGUAGE, Text};
use std::path::Path;
use std::sync::{Arc, RwLock};

static TEXT: RwLock<Option<Arc<Text>>> = RwLock::new(None);

/// Reads the base package's text library of `language` (the source language when it has
/// none) from the data folder. Fast: no other content is read.
pub fn load(data: &Path, language: &str) -> Text {
    let mut errors = Vec::new();
    let base = canopy_content::package::Package::open(&data.join(canopy_content::BASE_DIR));
    let packages: Vec<_> = base.into_iter().collect();
    let text = Text::read(&packages, language, &mut errors);
    if text.is_empty() && language != SOURCE_LANGUAGE {
        return load(data, SOURCE_LANGUAGE);
    }
    for error in &errors {
        tracing::warn!(%error, "text library");
    }
    text
}

/// Makes `text` the game's text.
pub fn install(text: Text) {
    *TEXT.write().expect("text lock") = Some(Arc::new(text));
}

/// The game's text (empty until [`install`]ed; tests read the repository's on first use).
pub fn get() -> Arc<Text> {
    if let Some(text) = TEXT.read().expect("text lock").clone() {
        return text;
    }
    #[cfg(test)]
    {
        install(load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
            SOURCE_LANGUAGE,
        ));
        if let Some(text) = TEXT.read().expect("text lock").clone() {
            return text;
        }
    }
    Arc::new(Text::default())
}

/// The string for `key` (the key itself where the library lacks it).
pub fn tr(key: &str) -> String {
    get().text(key).to_string()
}

/// The string for `key` with its placeholders filled.
pub fn tr_with(key: &str, args: &[(&str, Arg)]) -> String {
    get().format(key, args)
}

/// `tr!("hud.victory")`, or with placeholders `tr!("card.hero-roster", count = n, capacity = c)`.
macro_rules! tr {
    ($key:literal) => {
        $crate::text::tr($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::text::tr_with($key, &[$((stringify!($name), canopy_content::text::Arg::from($value))),+])
    };
}

/// Text the simulation produced (a key, an encoded message or older plain English), as
/// players read it (D96).
pub fn message(text: &str) -> String {
    get().say(text)
}

/// The library key of a message (the key itself, or an encoded message's): what a race's
/// spoken alerts are keyed by (D97).
pub fn key_of(text: &str) -> String {
    canopy_content::text::Message::decode(text).map_or_else(|| text.to_string(), |m| m.key)
}

/// A layout lookup for the UI ([`canopy_ui::UiConfig::localize`]).
pub fn lookup(key: &str) -> Option<String> {
    get().get(key).map(str::to_string)
}

/// Resolves `config`'s text library keys from the game's text, reading the base package's
/// text from `data` first if none is installed yet (captures and tests).
pub fn localize(config: &mut canopy_ui::UiConfig, data: &Path) {
    if get().is_empty() {
        install(load(data, SOURCE_LANGUAGE));
    }
    config.localize(&lookup);
}

#[cfg(test)]
mod tests {
    /// Every key the game's code names (`tr!("…")`, `text::tr("…")`, `tr_with("…")`) is in
    /// the base text library (D96).
    #[test]
    fn every_key_in_code_is_in_the_library() {
        let text = super::get();
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("text.rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                for marker in ["tr!(\"", "text::tr(\"", "tr_with(\"", "tr_with(\n"] {
                    for (at, _) in source.match_indices(marker) {
                        let before = source[..at].chars().next_back().unwrap_or(' ');
                        if before.is_alphanumeric() || before == '_' {
                            continue;
                        }
                        let rest = source[at + marker.len()..].trim_start_matches([' ', '\n', '"']);
                        let key: String = rest.chars().take_while(|c| *c != '"').collect();
                        if key.contains(char::is_whitespace) || key.contains('{') || key.is_empty()
                        {
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
}
