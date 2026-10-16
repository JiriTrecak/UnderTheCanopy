//! Where an id is used, and the commands that rename it everywhere (docs/editor.md:
//! renames carry their references). Ids are unique strings across a package, so a use is
//! any string value or object key equal to the id.

use crate::document::Command;
use serde_json::Value;

/// One use of an id.
#[derive(Debug, Clone, PartialEq)]
pub struct Use {
    pub document: String,
    /// The value (or, for a key, the entry) that holds it.
    pub pointer: String,
    /// An object key rather than a value.
    pub key: bool,
}

fn token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Every use of `id` in `value`, the document `document`.
pub fn find(document: &str, value: &Value, id: &str, out: &mut Vec<Use>) {
    walk(document, value, id, String::new(), out);
}

fn walk(document: &str, value: &Value, id: &str, pointer: String, out: &mut Vec<Use>) {
    match value {
        Value::String(text) if text == id => out.push(Use {
            document: document.to_string(),
            pointer,
            key: false,
        }),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk(document, item, id, format!("{pointer}/{i}"), out);
            }
        }
        Value::Object(object) => {
            for (key, item) in object {
                let child = format!("{pointer}/{}", token(key));
                if key == id {
                    out.push(Use {
                        document: document.to_string(),
                        pointer: child.clone(),
                        key: true,
                    });
                }
                walk(document, item, id, child, out);
            }
        }
        _ => {}
    }
}

/// The commands that rename every use in `uses` to `new`. `value_of` gives a document's
/// current value. Deeper uses go first, so a renamed key never moves a path still to be
/// rewritten; a moved entry carries the renames made inside it.
pub fn rename(
    uses: &[Use],
    new: &str,
    mut value_of: impl FnMut(&str) -> Option<Value>,
) -> Vec<Command> {
    let mut ordered: Vec<&Use> = uses.iter().collect();
    ordered.sort_by_key(|u| std::cmp::Reverse(u.pointer.matches('/').count()));
    // Working copies, changed as the commands would change them.
    let mut copies: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    let mut commands = Vec::new();
    for u in ordered {
        if !copies.contains_key(&u.document) {
            match value_of(&u.document) {
                Some(value) => copies.insert(u.document.clone(), value),
                None => continue,
            };
        }
        let copy = copies.get_mut(&u.document).expect("just added");
        if !u.key {
            if let Some(slot) = copy.pointer_mut(&u.pointer) {
                *slot = Value::String(new.to_string());
            }
            commands.push(Command::Set {
                document: u.document.clone(),
                pointer: u.pointer.clone(),
                value: Value::String(new.to_string()),
                at: None,
            });
            continue;
        }
        let Some((parent, old)) = u.pointer.rsplit_once('/') else {
            continue;
        };
        let old = old.replace("~1", "/").replace("~0", "~");
        let Some(Value::Object(object)) = copy.pointer_mut(parent) else {
            continue;
        };
        let Some(at) = object.keys().position(|k| *k == old) else {
            continue;
        };
        let value = object.shift_remove(&old).expect("present");
        object.shift_insert(at, new.to_string(), value.clone());
        commands.push(Command::Remove {
            document: u.document.clone(),
            pointer: u.pointer.clone(),
        });
        commands.push(Command::Set {
            document: u.document.clone(),
            pointer: format!("{parent}/{}", token(new)),
            value,
            at: Some(at),
        });
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Context, Store, format};
    use serde_json::json;

    #[test]
    fn renames_values_and_keys_in_place() {
        let root = std::env::temp_dir().join(format!("canopy-refs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("units")).unwrap();
        let unit = json!({"id": "unit.knight", "name": "Knight"});
        let race = json!({
            "id": "ants",
            "units": ["unit.knight", "unit.squire"],
            "costs": {"unit.squire": 1, "unit.knight": {"of": "unit.knight"}, "x": 2}
        });
        std::fs::write(root.join("units/knight.json"), format(&unit)).unwrap();
        std::fs::write(root.join("race.json"), format(&race)).unwrap();
        let mut store = Store::new(&root);
        let mut uses = Vec::new();
        for document in ["units/knight.json", "race.json"] {
            let value = store.get(document).unwrap().unwrap().clone();
            find(document, &value, "unit.knight", &mut uses);
        }
        assert_eq!(uses.len(), 4, "{uses:?}");
        let values: std::collections::BTreeMap<_, _> = ["units/knight.json", "race.json"]
            .iter()
            .map(|d| (d.to_string(), store.get(d).unwrap().unwrap().clone()))
            .collect();
        let commands = rename(&uses, "unit.paladin", |d| values.get(d).cloned());
        let context = Context {
            editor: crate::Kind::Units,
            document: "units/knight.json".into(),
            pointer: None,
        };
        store.run("Rename", context, commands).unwrap();
        assert_eq!(
            store.get("units/knight.json").unwrap().unwrap()["id"],
            "unit.paladin"
        );
        let renamed = store.get("race.json").unwrap().unwrap().clone();
        assert_eq!(
            renamed,
            json!({
                "id": "ants",
                "units": ["unit.paladin", "unit.squire"],
                "costs": {"unit.squire": 1, "unit.paladin": {"of": "unit.paladin"}, "x": 2}
            })
        );
        // Keys keep their place.
        let keys: Vec<_> = renamed["costs"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["unit.squire", "unit.paladin", "x"]);
        store.undo().unwrap();
        assert_eq!(store.get("race.json").unwrap().unwrap(), &race);
        let _ = std::fs::remove_dir_all(&root);
    }
}
