//! The text library from the editor's side (D96): which elements take their name and
//! description from it, under which keys, and in which file of their package.

use serde_json::Value;

/// The language the editor writes.
pub const LANGUAGE: &str = canopy_content::text::SOURCE_LANGUAGE;

/// Content folders whose elements are named in the library, and the area file their text
/// goes to (as `tools/text/textlib.py`).
const AREAS: &[(&str, &str)] = &[
    ("units", "units"),
    ("buildings", "buildings"),
    ("items", "items"),
    ("resources", "items"),
    ("abilities", "abilities"),
    ("research", "research"),
    ("races", "races"),
];

/// A document path split into its package's prefix (nested map and campaign packages)
/// and the path within that package.
fn split(document: &str) -> (&str, &str) {
    let mut cut = 0;
    for marker in [".utcm/", ".utcc/"] {
        if let Some(at) = document.rfind(marker) {
            cut = cut.max(at + marker.len());
        }
    }
    document.split_at(cut)
}

fn area(document: &str) -> Option<(&'static str, &'static str)> {
    let (_, rel) = split(document);
    let folder = rel.split('/').next()?;
    AREAS.iter().find(|(f, _)| *f == folder).copied()
}

/// The key prefix of a named element (`unit.ants.warrior`, `race.ants`), when the document
/// is one.
pub fn key(document: &str, value: &Value) -> Option<String> {
    let (folder, _) = area(document)?;
    let id = value.get("id")?.as_str()?;
    Some(if folder == "races" {
        format!("race.{id}")
    } else {
        id.to_string()
    })
}

/// The text file a named element's text is written to.
pub fn file(document: &str) -> Option<String> {
    let (_, area) = area(document)?;
    let (prefix, _) = split(document);
    Some(format!("{prefix}text/{LANGUAGE}/{area}.json"))
}

/// A JSON pointer to `key` in a text file.
pub fn pointer(key: &str) -> String {
    format!("/{}", key.replace('~', "~0").replace('/', "~1"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn named_elements_find_their_keys_and_files() {
        let unit = json!({"id": "unit.ants.warrior"});
        assert_eq!(
            key("units/ants/warrior.json", &unit).unwrap(),
            "unit.ants.warrior"
        );
        assert_eq!(
            file("units/ants/warrior.json").unwrap(),
            "text/en/units.json"
        );
        let race = json!({"id": "ants"});
        assert_eq!(key("races/ants.json", &race).unwrap(), "race.ants");
        assert_eq!(
            file("maps/isle.utcm/resources/tree.json").unwrap(),
            "maps/isle.utcm/text/en/items.json"
        );
        assert!(key("effects/burst.json", &json!({"id": "effect.burst"})).is_none());
    }
}
