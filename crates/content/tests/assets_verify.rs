//! `assets::verify_outputs`: published files against the manifest's sizes and hashes.

use canopy_content::assets::verify_outputs;
use serde_json::json;
use std::path::{Path, PathBuf};

/// A data folder holding `assets/manifest.json` with one output per file.
fn fixture(name: &str, files: &[(&str, &[u8], usize, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("canopy-verify-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let outputs: Vec<_> = files
        .iter()
        .map(|(path, bytes, size, sha256)| {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, bytes).unwrap();
            json!({"role": "data", "path": path, "sha256": sha256, "bytes": size})
        })
        .collect();
    let manifest = json!({"version": 1, "records": [{
        "id": "asset.test", "name": "Test", "kind": "data", "tags": [], "revision": 1,
        "profile": "data", "outputs": outputs, "render": [], "scenery": [],
    }]});
    std::fs::write(
        root.join("assets/manifest.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
    root
}

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn matching_files_pass() {
    let root = fixture("ok", &[("assets/library/a/data.bin", b"abc", 3, ABC)]);
    assert_eq!(verify_outputs(&root), vec![]);
}

#[test]
fn size_and_hash_mismatches_are_reported() {
    let other = "0".repeat(64);
    let root = fixture(
        "mismatch",
        &[
            ("assets/library/a/size.bin", b"abc", 4, ABC),
            ("assets/library/a/hash.bin", b"abc", 3, &other),
        ],
    );
    let errors = verify_outputs(&root);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert_eq!(errors[0].file, "assets/library/a/size.bin");
    assert!(errors[0].message.contains("bytes"));
    assert_eq!(errors[1].file, "assets/library/a/hash.bin");
    assert!(errors[1].message.contains("SHA-256"));
}

#[test]
fn missing_files_and_lfs_pointers_fail() {
    let pointer = format!("version https://git-lfs.github.com/spec/v1\noid sha256:{ABC}\nsize 3\n");
    let root = fixture("missing", &[("assets/library/a/data.bin", b"abc", 3, ABC)]);
    std::fs::remove_file(root.join("assets/library/a/data.bin")).unwrap();
    let errors = verify_outputs(&root);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].file, "assets/library/a/data.bin");

    let root = fixture(
        "pointer",
        &[("assets/library/a/data.bin", pointer.as_bytes(), 3, ABC)],
    );
    let errors = verify_outputs(&root);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("LFS"), "{errors:?}");
}

/// The real published files. This repository's checkout may hold only Git LFS pointers
/// (CI skips LFS), so point `CANOPY_DATA` at a checkout with the files, for example the
/// main one: `CANOPY_DATA=<checkout> cargo test -p canopy-content -- --ignored`.
#[test]
#[ignore = "hashes every published file (~750 MB); needs real files, not LFS pointers"]
fn shipped_outputs_match_the_manifest() {
    let data = std::env::var_os("CANOPY_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")).into());
    let errors = verify_outputs(&data);
    assert!(
        errors.is_empty(),
        "{}",
        canopy_content::Errors(errors).to_string()
    );
}
