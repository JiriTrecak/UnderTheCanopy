//! Whatever assets the repository ships must load and agree with each other and the
//! content. No test here names a shipped id.

use canopy_content::{
    Content, Errors,
    assets::{Files, landscape, models, package, previews},
    package::Package,
};
use std::path::Path;

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn shipped_asset_indexes_are_valid() {
    let content = Content::load(data()).unwrap_or_else(|errors| panic!("{errors}"));
    let mut errors = Vec::new();
    let base = Package::open(&data().join(canopy_content::BASE_DIR)).unwrap();
    let index = package::index(&[base], Files::Check, &mut errors);
    landscape::validate(
        &index.catalogue,
        &content.manifest,
        &content.definitions,
        &mut errors,
    );
    models::validate(&index.models, &content.manifest, &mut errors);
    previews::load(data(), &mut errors);
    assert!(errors.is_empty(), "{}", Errors(errors));
    assert!(!index.models.is_empty());
}
