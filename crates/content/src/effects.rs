//! The published visual effect library (`content/effects/published.json`).
//!
//! Placeholder until the full format lands.

use crate::error::Error;
use std::path::Path;

/// One entry of `effects` in `content/effects/published.json`.
pub type VisualEffect = serde_json::Value;

/// Reads the published effects.
pub(crate) fn load(data: &Path, errors: &mut Vec<Error>) -> Option<Vec<VisualEffect>> {
    crate::error::read_json(data, crate::EFFECTS_FILE)
        .and_then(|value| {
            crate::error::parse::<Vec<VisualEffect>>(
                value["effects"].clone(),
                crate::EFFECTS_FILE,
                "effects",
            )
        })
        .map_err(|error| errors.push(error))
        .ok()
}
