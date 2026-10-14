//! Whatever maps the repository ships must load and pass every rule. No test here names a
//! shipped map: maps change freely in the editor.

use canopy_content::{Content, map};
use std::path::Path;

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn shipped_maps_are_valid() {
    let content = Content::load(data()).unwrap_or_else(|errors| panic!("{errors}"));
    let maps = map::library(data()).unwrap_or_else(|errors| panic!("{errors}"));
    for file in &maps {
        let loaded = file.load().unwrap_or_else(|errors| panic!("{errors}"));
        let errors = map::validate(&loaded, &content);
        assert!(errors.is_empty(), "{}: {errors:#?}", file.file);
        loaded
            .authoring
            .terrain
            .decode()
            .unwrap_or_else(|error| panic!("{}: {error}", file.file));
    }
}
