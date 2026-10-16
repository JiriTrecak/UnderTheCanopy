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
    fn copy(from: &Path, to: &Path) {
        if from.is_dir() {
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                copy(&entry.path(), &to.join(entry.file_name()));
            }
        } else {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(from, to).unwrap();
        }
    }
    for file in [
        canopy_content::BASE_DIR,
        canopy_content::MANIFEST_FILE,
        "assets/authoring/catalogue.json",
    ] {
        copy(&data().join(file), &root.join(file));
    }
    assert!(
        Content::load(&root).is_err(),
        "the development loader requires the files"
    );
    Content::load_packaged(&root).unwrap_or_else(|errors| panic!("{errors}"));
}
