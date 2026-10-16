//! Abilities (`abilities/` of a package, one file each with its presentation inline) and
//! ability bindings on casters (`behaviors.abilities`).

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

use crate::{Content, error::Error, load::Element};
use std::collections::BTreeMap;

/// Parses every ability (sorted by id) and checks the format's rules; every bad element is
/// reported against its own file.
pub(crate) fn read(
    elements: Vec<Element>,
    files: &BTreeMap<String, String>,
    errors: &mut Vec<Error>,
) -> Option<AbilityLibrary> {
    let before = errors.len();
    let abilities: Vec<Ability> = elements
        .into_iter()
        .filter_map(|e| e.parse(errors))
        .collect();
    if errors.len() != before {
        return None;
    }
    let library = AbilityLibrary { abilities };
    rules::check(&library, files, errors);
    Some(library)
}

/// Ability rules that need the whole content: bindings, damage types, summons, icons.
pub(crate) fn validate(content: &Content, errors: &mut Vec<Error>) {
    references::check(content, errors);
    publication::check_resources(
        &content.abilities,
        &content.effects,
        &content.manifest,
        &content.files,
        errors,
    );
}
