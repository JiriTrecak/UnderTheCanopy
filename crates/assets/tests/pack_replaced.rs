//! A pack opened for streaming keeps reading the file it opened, even when a newer pack is
//! renamed over its path (the cooker writes a new pack while the game runs).

use canopy_assets::{BlobBuilder, DataMeta, EntryKind, Pack, PackWriter};
use std::{fs, io::Cursor, path::Path};

fn write_pack(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = PackWriter::new(Cursor::new(Vec::new())).unwrap();
    for (name, bytes) in entries {
        let mut blob = BlobBuilder::new();
        let span = blob.push_bytes(bytes);
        let location = writer
            .write_blob(&blob.finish(EntryKind::Data, &DataMeta { bytes: span }))
            .unwrap();
        writer.add_entry((*name).to_string(), EntryKind::Data, location);
    }
    fs::write(path, writer.finish([0; 32]).unwrap().into_inner()).unwrap();
}

#[test]
fn an_open_pack_reads_its_own_file_after_a_newer_one_replaces_it() {
    let folder = std::env::temp_dir().join(format!("canopy-pack-replaced-{}", std::process::id()));
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("canopy.pack");
    write_pack(
        &path,
        &[("a", b"first pack, entry a"), ("b", b"first pack, entry b")],
    );
    let pack = Pack::open(&path).unwrap();
    // The newer pack has other entries before `b`, so the old index points at other bytes.
    let newer = folder.join("canopy.pack.tmp");
    write_pack(
        &newer,
        &[("0", &[7; 300]), ("a", b"newer a"), ("b", b"newer b")],
    );
    fs::rename(&newer, &path).unwrap();
    assert_eq!(
        pack.get("b").unwrap().bytes().unwrap(),
        b"first pack, entry b"
    );
    assert_eq!(
        Pack::open(&path)
            .unwrap()
            .get("b")
            .unwrap()
            .bytes()
            .unwrap(),
        b"newer b"
    );
    fs::remove_dir_all(&folder).unwrap();
}
