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
    // The base package's documents only, as a package ships them.
    fn copy(from: &Path, to: &Path) {
        if from.is_dir() {
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                copy(&entry.path(), &to.join(entry.file_name()));
            }
        } else if from.extension().is_some_and(|e| e == "json") {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(from, to).unwrap();
        }
    }
    copy(
        &data().join(canopy_content::BASE_DIR),
        &root.join(canopy_content::BASE_DIR),
    );
    assert!(
        Content::load(&root).is_err(),
        "the development loader requires the files"
    );
    Content::load_packaged(&root).unwrap_or_else(|errors| panic!("{errors}"));
}

#[test]
fn the_repository_combat_lab_scenarios_are_valid() {
    // Development files beside the package (docs/editor.md, The Combat Lab): each reads and
    // passes its rules against the shipped content.
    let content = Content::load(data()).unwrap_or_else(|errors| panic!("{errors}"));
    for (id, path) in canopy_content::lab::library(data()) {
        let lab = canopy_content::lab::read(&path).unwrap_or_else(|e| panic!("{e}"));
        let problems = lab.problems(&content);
        assert!(problems.is_empty(), "{id}: {problems:?}");
        assert_eq!(
            canopy_content::lab::to_text(&lab),
            std::fs::read_to_string(&path).unwrap(),
            "{id} is in the file format"
        );
    }
}
