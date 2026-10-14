//! The published ability library (`content/abilities/published.json`) and ability bindings
//! on casters (`behaviors.abilities`).
//!
//! The game reads only the published snapshot, never the per-ability authoring folders.

mod amount;
mod definition;
pub mod literal;
mod operations;
mod publication;
mod references;
mod rules;

pub use amount::{Amount, RankParameters, RankedReference};
pub use definition::*;
pub use operations::*;

use crate::{
    Content,
    error::{Error, parse},
};
use std::path::Path;

/// The engine interface the published abilities must target.
pub const ABILITY_ABI: &str = "abilities-1";

const FILE: &str = crate::ABILITIES_FILE;

/// Reads the published library, checks its envelope and the format's rules, and checks
/// that every resource its presentations use is published.
pub(crate) fn load(data: &Path, errors: &mut Vec<Error>) -> Option<AbilityLibrary> {
    let publication = crate::error::read_json(data, FILE)
        .map_err(|error| errors.push(error))
        .ok()?;
    let library = read_publication(publication, errors)?;
    rules::check(&library, errors);
    publication::check_resources(data, &library, errors);
    Some(library)
}

/// The `library` of a publication, parsed entry by entry so every bad entry is reported.
fn read_publication(
    mut publication: serde_json::Value,
    errors: &mut Vec<Error>,
) -> Option<AbilityLibrary> {
    let abi = publication.get("abi").and_then(serde_json::Value::as_str);
    let version = publication
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64);
    if abi != Some(ABILITY_ABI) || version != Some(1) {
        errors.push(Error::new(FILE, "", "Unsupported ability publication ABI"));
        return None;
    }

    /// The library with its entries still unparsed.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Entries {
        schema_version: literal::SchemaVersion,
        abilities: Vec<serde_json::Value>,
        presentations: Vec<serde_json::Value>,
    }
    let entries: Entries = parse(publication["library"].take(), FILE, "library")
        .map_err(|error| errors.push(error))
        .ok()?;
    fn each<T: serde::de::DeserializeOwned>(
        items: Vec<serde_json::Value>,
        key: &str,
        errors: &mut Vec<Error>,
    ) -> Option<Vec<T>> {
        let before = errors.len();
        let parsed: Vec<T> = items
            .into_iter()
            .enumerate()
            .filter_map(|(index, item)| {
                parse(item, FILE, &format!("library.{key}[{index}]"))
                    .map_err(|error| errors.push(error))
                    .ok()
            })
            .collect();
        (errors.len() == before).then_some(parsed)
    }
    let abilities = each(entries.abilities, "abilities", errors);
    let presentations = each(entries.presentations, "presentations", errors);
    Some(AbilityLibrary {
        schema_version: entries.schema_version,
        abilities: abilities?,
        presentations: presentations?,
    })
}

/// Ability rules that need the whole content: bindings, damage types, summons, icons.
pub(crate) fn validate(content: &Content, errors: &mut Vec<Error>) {
    references::check(content, errors);
}
