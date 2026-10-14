//! Published asset records (`assets/manifest.json`).
//!
//! Placeholder until the full format lands: only `render` entries are read.

use crate::{Content, error::Error, game::RenderAsset};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub records: Vec<Record>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub render: Vec<RenderAsset>,
}

/// Reads `assets/manifest.json`.
pub(crate) fn load(data: &Path, errors: &mut Vec<Error>) -> Option<Manifest> {
    crate::error::read_json(data, crate::MANIFEST_FILE)
        .and_then(|value| crate::error::parse(value, crate::MANIFEST_FILE, ""))
        .map_err(|error| errors.push(error))
        .ok()
}

/// Every render asset the published records declare, unsorted.
pub(crate) fn render_assets(manifest: &Manifest) -> Vec<RenderAsset> {
    manifest
        .records
        .iter()
        .flat_map(|r| r.render.iter().cloned())
        .collect()
}

/// Asset rules that need the whole content.
pub(crate) fn validate(_content: &Content, _errors: &mut Vec<Error>) {}
