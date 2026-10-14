//! The published ability library (`content/abilities/published.json`) and ability bindings
//! on casters (`behaviors.abilities`).
//!
//! Placeholder until the full format lands.

use crate::{Content, error::Error};
use std::path::Path;

/// `behaviors.abilities`.
pub type AbilityCaster = serde_json::Value;

/// The `library` of `content/abilities/published.json`.
pub type AbilityLibrary = serde_json::Value;

/// Reads the published library and checks its envelope.
pub(crate) fn load(data: &Path, errors: &mut Vec<Error>) -> Option<AbilityLibrary> {
    crate::error::read_json(data, crate::ABILITIES_FILE)
        .map(|mut publication| publication["library"].take())
        .map_err(|error| errors.push(error))
        .ok()
}

/// Ability rules that need the whole content: bindings, damage types, summons, icons.
pub(crate) fn validate(_content: &Content, _errors: &mut Vec<Error>) {}
