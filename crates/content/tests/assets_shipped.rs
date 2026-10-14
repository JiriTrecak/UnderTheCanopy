//! Whatever published asset indexes the repository ships must load and agree with the
//! manifest and content. No test here names a shipped id.

use canopy_content::{
    Content, Errors,
    assets::{landscape, models, previews},
};
use std::path::Path;

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn shipped_asset_indexes_are_valid() {
    let content = Content::load(data()).unwrap_or_else(|errors| panic!("{errors}"));
    let mut errors = Vec::new();
    let landscape = landscape::load(data(), &mut errors).unwrap_or_default();
    landscape::validate(
        &landscape,
        &content.manifest,
        &content.definitions,
        &mut errors,
    );
    let models = models::load(data(), &mut errors).unwrap_or_default();
    models::validate(&models, &content.manifest, &mut errors);
    previews::load(data(), &mut errors);
    assert!(errors.is_empty(), "{}", Errors(errors));
    assert!(!models.is_empty());
}
