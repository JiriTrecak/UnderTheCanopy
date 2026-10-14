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

#[test]
fn packaged_content_loads_without_asset_files() {
    // A data folder with only the JSON a package ships: no published output files.
    let root = std::env::temp_dir().join(format!("canopy-packaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for file in [
        canopy_content::GAME_FILE,
        canopy_content::ABILITIES_FILE,
        canopy_content::EFFECTS_FILE,
        canopy_content::MANIFEST_FILE,
        "assets/authoring/catalogue.json",
    ] {
        let target = root.join(file);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(data().join(file), target).unwrap();
    }
    assert!(
        Content::load(&root).is_err(),
        "the development loader requires the files"
    );
    Content::load_packaged(&root).unwrap_or_else(|errors| panic!("{errors}"));
}
