//! Whatever data the repository ships must load and pass every rule. No test here names
//! a shipped id: content changes freely in the editors.

use canopy_content::Content;
use std::path::Path;

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn shipped_content_is_valid() {
    let content = Content::load(data()).unwrap_or_else(|errors| panic!("{errors}"));
    assert!(!content.definitions.is_empty());
}
