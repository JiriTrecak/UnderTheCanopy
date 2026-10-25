//! The lengths audio assets record (`seconds` in `asset.json`, D97) match their decoded
//! clips: a spoken line stays on screen as long as that number says.

use canopy_audio::SoundLibrary;
use std::path::{Path, PathBuf};

fn asset_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            asset_files(&path, out);
        } else if path.file_name().is_some_and(|n| n == "asset.json") {
            out.push(path);
        }
    }
}

/// A 16-bit PCM WAV file's length from its header.
fn wav_seconds(bytes: &[u8]) -> f64 {
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let u16_at = |i: usize| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
    let (mut rate, mut block, mut at) = (0, 0, 12);
    while at + 8 <= bytes.len() {
        let size = u32_at(at + 4) as usize;
        match &bytes[at..at + 4] {
            b"fmt " => {
                rate = u32_at(at + 12);
                block = u16_at(at + 20);
            }
            b"data" => return size as f64 / f64::from(block.max(1)) / f64::from(rate.max(1)),
            _ => {}
        }
        at += 8 + size + size % 2;
    }
    panic!("no data chunk");
}

#[test]
fn recorded_clip_lengths_match_the_audio() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../base");
    let mut files = Vec::new();
    asset_files(&base, &mut files);
    let mut checked = 0;
    for file in files {
        let asset: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let Some(recorded) = asset["seconds"].as_f64() else {
            continue;
        };
        let audio = asset["resources"]
            .as_array()
            .and_then(|r| r.iter().find(|r| r["role"] == "audio"))
            .unwrap_or_else(|| panic!("{}: seconds without an audio file", file.display()));
        let format = audio["format"].as_str().unwrap();
        let bytes = std::fs::read(file.with_file_name(format!("audio.{format}"))).unwrap();
        let decoded = match format {
            "wav" => wav_seconds(&bytes),
            _ => {
                let mut library = SoundLibrary::new();
                let id = library.insert_encoded("clip", &bytes).unwrap();
                library.clip(id).duration
            }
        };
        assert!(
            (decoded - recorded).abs() < 0.05,
            "{}: records {recorded} s, the audio is {decoded:.3} s",
            file.display()
        );
        checked += 1;
    }
    assert!(checked > 0, "no audio asset records its length");
}
