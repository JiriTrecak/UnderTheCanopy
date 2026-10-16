//! Layering one collection across packages in load order (docs/packages.md):
//!
//! - **The same id in a later package overrides**: its document merges onto the earlier
//!   one. Objects merge field by field, everything else (lists included) replaces whole,
//!   and `null` removes a field.
//! - **`{"id": …, "removed": true}`** deletes the element from that layer on; a later
//!   package may add it again.
//! - **`"base": "<id>"`** starts an element as a copy of another element of the same
//!   collection, after layering: an element based on `warrior` sees a map's changes to
//!   `warrior`. Its own fields merge on top; chains are allowed, cycles are errors.

use super::{Collection, Form, Package};
use crate::error::Error;
use serde_json::Value;
use std::collections::BTreeMap;

/// Merges `patch` onto `target`: objects field by field (`null` removes a field), any
/// other value replaces whole.
pub fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                if value.is_null() {
                    target.remove(key);
                } else if let Some(existing) = target.get_mut(key)
                    && existing.is_object()
                    && value.is_object()
                {
                    merge(existing, value);
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        (target, patch) => *target = patch.clone(),
    }
}

/// One package's document for an element.
#[derive(Debug, Clone, PartialEq)]
pub struct Contribution {
    /// Index of the package in the load order.
    pub package: usize,
    /// Path from that package's root: the file, or the element's folder.
    pub path: String,
    /// The document as that package authors it.
    pub value: Value,
}

/// An element after layering.
#[derive(Debug, Clone, PartialEq)]
pub struct Layered {
    pub id: String,
    /// Every layer merged, `base` resolved; without the `base` field.
    pub value: Value,
    /// The element it was based on, when it was.
    pub base: Option<String>,
    /// The packages that authored it, in load order (since its last removal).
    pub contributions: Vec<Contribution>,
}

impl Layered {
    /// The latest package that authored this element.
    pub fn top(&self) -> &Contribution {
        self.contributions
            .last()
            .expect("a layered element has an author")
    }
}

/// A collection layered across packages: every live element by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layers {
    pub elements: BTreeMap<String, Layered>,
}

fn removal(value: &Value) -> Option<Result<(), &'static str>> {
    let object = value.as_object()?;
    match object.get("removed") {
        None => None,
        Some(Value::Bool(true)) if object.len() == 2 => Some(Ok(())),
        Some(Value::Bool(true)) => Some(Err("a removal holds only id and removed")),
        Some(_) => Some(Err("removed must be true")),
    }
}

impl Layers {
    /// Reads `collection` from every package in order and layers it.
    pub fn build(collection: &Collection, packages: &[Package], errors: &mut Vec<Error>) -> Self {
        let mut layers = Self::default();
        for (index, package) in packages.iter().enumerate() {
            let mut seen: BTreeMap<String, String> = BTreeMap::new();
            for found in collection.read(package, errors) {
                let file = package.label(&found.path);
                if let Some(first) = seen.insert(found.id.clone(), found.path.clone()) {
                    errors.push(Error::new(
                        &file,
                        "id",
                        format!("{} is also defined in {}", found.id, package.label(&first)),
                    ));
                    continue;
                }
                let contribution = Contribution {
                    package: index,
                    path: found.path,
                    value: found.value,
                };
                match removal(&contribution.value) {
                    Some(Ok(())) => {
                        if layers.elements.remove(&found.id).is_none() {
                            errors.push(Error::new(
                                &file,
                                "removed",
                                format!("no earlier package has {}", found.id),
                            ));
                        }
                    }
                    Some(Err(message)) => errors.push(Error::new(&file, "removed", message)),
                    None => match layers.elements.get_mut(&found.id) {
                        Some(element) => {
                            merge(&mut element.value, &contribution.value);
                            element.contributions.push(contribution);
                        }
                        None => {
                            layers.elements.insert(
                                found.id.clone(),
                                Layered {
                                    id: found.id,
                                    value: contribution.value.clone(),
                                    base: None,
                                    contributions: vec![contribution],
                                },
                            );
                        }
                    },
                }
            }
        }
        if !matches!(collection.form, Form::Packages(_)) {
            layers.resolve_bases(packages, errors);
        }
        layers
    }

    /// Replaces every `base` reference by a copy of the resolved base with the element's
    /// own fields merged on top.
    fn resolve_bases(&mut self, packages: &[Package], errors: &mut Vec<Error>) {
        let ids: Vec<String> = self.elements.keys().cloned().collect();
        let mut resolved: BTreeMap<String, Option<Value>> = BTreeMap::new();
        for id in &ids {
            let mut chain = Vec::new();
            self.resolve(id, &mut chain, &mut resolved, packages, errors);
        }
        for (id, value) in resolved {
            let element = self.elements.get_mut(&id).expect("resolved ids exist");
            match value {
                Some(value) => element.value = value,
                // Broken chains keep their own fields; the error is already reported.
                None => {
                    if let Some(object) = element.value.as_object_mut() {
                        object.remove("base");
                    }
                }
            }
        }
    }

    fn resolve(
        &mut self,
        id: &str,
        chain: &mut Vec<String>,
        resolved: &mut BTreeMap<String, Option<Value>>,
        packages: &[Package],
        errors: &mut Vec<Error>,
    ) -> Option<Value> {
        if let Some(done) = resolved.get(id) {
            return done.clone();
        }
        let element = self.elements.get(id)?;
        let Some(base) = element.value.get("base") else {
            return Some(element.value.clone());
        };
        let top = element.top();
        let file = packages[top.package].label(&top.path);
        let Some(base) = base.as_str().map(str::to_string) else {
            errors.push(Error::new(&file, "base", "base must be an id"));
            resolved.insert(id.to_string(), None);
            return None;
        };
        if chain.iter().any(|c| c == id) {
            errors.push(Error::new(
                &file,
                "base",
                format!("base cycle: {} → {id}", chain.join(" → ")),
            ));
            resolved.insert(id.to_string(), None);
            return None;
        }
        if !self.elements.contains_key(&base) {
            errors.push(Error::new(
                &file,
                "base",
                format!("no element {base} to base on"),
            ));
            resolved.insert(id.to_string(), None);
            return None;
        }
        chain.push(id.to_string());
        let parent = self.resolve(&base, chain, resolved, packages, errors);
        chain.pop();
        let value = parent.map(|mut value| {
            let mut own = self.elements[id].value.clone();
            if let Some(object) = own.as_object_mut() {
                object.remove("base");
            }
            merge(&mut value, &own);
            value
        });
        if let Some(element) = self.elements.get_mut(id) {
            element.base = Some(base);
        }
        resolved.insert(id.to_string(), value.clone());
        value
    }

    /// The layered documents, by id.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.elements.values().map(|e| &e.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{Collection, Package};
    use serde_json::json;
    use std::path::PathBuf;

    /// A test package: id, kind and its files (path, JSON).
    type Spec<'a> = (&'a str, &'a str, &'a [(&'a str, Value)]);

    /// Writes packages under a fresh temporary folder, in load order.
    fn packages(name: &str, list: &[Spec]) -> Vec<Package> {
        let root: PathBuf =
            std::env::temp_dir().join(format!("canopy-layers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        list.iter()
            .map(|(id, kind, files)| {
                let dir = root.join(id);
                std::fs::create_dir_all(&dir).unwrap();
                let manifest =
                    json!({"format": 1, "id": id, "kind": kind, "name": id, "version": "1.0.0"});
                std::fs::write(dir.join("package.json"), manifest.to_string()).unwrap();
                for (path, value) in *files {
                    let file = dir.join(path);
                    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                    std::fs::write(file, value.to_string()).unwrap();
                }
                Package::open(&dir).unwrap()
            })
            .collect()
    }

    const UNITS: Collection = Collection::files("units");

    #[test]
    fn later_packages_merge_objects_and_replace_lists() {
        let p = packages(
            "merge",
            &[
                (
                    "base",
                    "base",
                    &[(
                        "units/a/heroes/knight.json",
                        json!({"id": "knight", "hp": 100, "body": {"armor": 2, "type": "heavy"}, "tags": ["a", "b"], "speed": 5}),
                    )],
                ),
                (
                    "map",
                    "map",
                    &[(
                        "units/knight.json",
                        json!({"id": "knight", "body": {"armor": 4}, "tags": ["c"], "speed": null}),
                    )],
                ),
            ],
        );
        let mut errors = Vec::new();
        let layers = Layers::build(&UNITS, &p, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let knight = &layers.elements["knight"];
        assert_eq!(
            knight.value,
            json!({"id": "knight", "hp": 100, "body": {"armor": 4, "type": "heavy"}, "tags": ["c"]})
        );
        assert_eq!(knight.contributions.len(), 2);
        assert_eq!(knight.contributions[0].path, "units/a/heroes/knight.json");
        assert_eq!(knight.top().package, 1);
    }

    #[test]
    fn removal_deletes_and_a_later_package_may_add_back() {
        let p = packages(
            "removal",
            &[
                (
                    "base",
                    "base",
                    &[("units/knight.json", json!({"id": "knight", "hp": 100}))],
                ),
                (
                    "tuning",
                    "mod",
                    &[(
                        "units/knight.json",
                        json!({"id": "knight", "removed": true}),
                    )],
                ),
                (
                    "map",
                    "map",
                    &[("units/x/knight.json", json!({"id": "knight", "hp": 7}))],
                ),
            ],
        );
        let mut errors = Vec::new();
        let layers = Layers::build(&UNITS, &p[..2], &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(layers.elements.is_empty());
        let layers = Layers::build(&UNITS, &p, &mut errors);
        assert_eq!(
            layers.elements["knight"].value,
            json!({"id": "knight", "hp": 7})
        );
        assert_eq!(layers.elements["knight"].contributions.len(), 1);
    }

    #[test]
    fn bases_resolve_after_layering() {
        let p = packages(
            "bases",
            &[
                (
                    "base",
                    "base",
                    &[
                        (
                            "units/knight.json",
                            json!({"id": "knight", "hp": 100, "body": {"armor": 2}}),
                        ),
                        (
                            "units/elite.json",
                            json!({"id": "elite", "base": "knight", "body": {"armor": 5}}),
                        ),
                        (
                            "units/champion.json",
                            json!({"id": "champion", "base": "elite", "name": "Champion"}),
                        ),
                    ],
                ),
                (
                    "map",
                    "map",
                    &[("units/knight.json", json!({"id": "knight", "hp": 150}))],
                ),
            ],
        );
        let mut errors = Vec::new();
        let layers = Layers::build(&UNITS, &p, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            layers.elements["champion"].value,
            json!({"id": "champion", "hp": 150, "body": {"armor": 5}, "name": "Champion"})
        );
        assert_eq!(layers.elements["champion"].base.as_deref(), Some("elite"));
    }

    #[test]
    fn mistakes_are_reported_with_their_file() {
        let p = packages(
            "mistakes",
            &[(
                "base",
                "base",
                &[
                    ("units/a.json", json!({"id": "a", "base": "b"})),
                    ("units/b.json", json!({"id": "b", "base": "a"})),
                    ("units/c.json", json!({"id": "c", "base": "nobody"})),
                    ("units/x/d.json", json!({"id": "d"})),
                    ("units/y/d.json", json!({"id": "d"})),
                    ("units/notes.txt", json!("not an element")),
                    ("units/e.json", json!({"id": "e", "removed": true})),
                    ("units/f.json", json!({"name": "no id"})),
                ],
            )],
        );
        let mut errors = Vec::new();
        Layers::build(&UNITS, &p, &mut errors);
        let text: Vec<String> = errors.iter().map(ToString::to_string).collect();
        let has = |part: &str| text.iter().any(|t| t.contains(part));
        assert!(has("base cycle"), "{text:#?}");
        assert!(
            has("base/units/c.json · base: no element nobody"),
            "{text:#?}"
        );
        assert!(
            has("base/units/y/d.json · id: d is also defined in base/units/x/d.json"),
            "{text:#?}"
        );
        assert!(has("base/units/notes.txt"), "{text:#?}");
        assert!(
            has("base/units/e.json · removed: no earlier package has e"),
            "{text:#?}"
        );
        assert!(has("base/units/f.json · id: missing id"), "{text:#?}");
    }

    #[test]
    fn folder_elements_and_hidden_markers() {
        let assets = Collection {
            dir: "assets",
            form: Form::Folders("asset.json"),
        };
        let p = packages(
            "folders",
            &[(
                "base",
                "base",
                &[
                    (
                        "assets/units/ants/warrior/asset.json",
                        json!({"id": "warrior-model"}),
                    ),
                    ("assets/units/ants/warrior/geometry.glb", json!("bytes")),
                    ("assets/empty/.folder", json!("")),
                ],
            )],
        );
        let mut errors = Vec::new();
        let layers = Layers::build(&assets, &p, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            layers.elements["warrior-model"].top().path,
            "assets/units/ants/warrior"
        );
    }
}
