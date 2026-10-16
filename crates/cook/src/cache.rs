//! The incremental cache under `<out>/cache`:
//!
//! - `stamps.json`: size and modification time → BLAKE3 of every source file, so a warm
//!   cook hashes only files that changed.
//! - `jobs/<job hash>.json`: what a job produced (entry names and blob keys) and what it
//!   reported. A job hash covers the cooker version, the job's parameters and its source
//!   bytes, so a hit is always valid.
//! - `blobs/<key>.blob`: cooked blobs, content-addressed. Textures are keyed by their
//!   image bytes and parameters, so models sharing an image encode it once.

use crate::diag::Diagnostic;
use canopy_assets::EntryKind;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Bump whenever cooked output for the same input changes.
pub const COOK_VERSION: u32 = 3;

const LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/";

pub type Hash = [u8; 32];

pub fn hex(hash: &Hash) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    len: u64,
    modified_ns: u128,
    hash: String,
    lfs_pointer: bool,
}

/// A source file's identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceHash {
    pub hash: Hash,
    pub lfs_pointer: bool,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobEntry {
    pub name: String,
    pub kind: EntryKind,
    pub blob: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRecord {
    pub entries: Vec<JobEntry>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Cache {
    root: PathBuf,
    stamps: BTreeMap<String, Stamp>,
}

impl Cache {
    pub fn open(out: &Path) -> io::Result<Self> {
        let root = out.join("cache");
        fs::create_dir_all(root.join("jobs"))?;
        fs::create_dir_all(root.join("blobs"))?;
        let stamps = fs::read(root.join("stamps.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Ok(Self { root, stamps })
    }

    /// Hashes the files of pack entries `paths`, reusing stamps of unchanged files.
    pub fn hash_sources(
        &mut self,
        data: &crate::plan::Locator,
        paths: &BTreeSet<&str>,
    ) -> io::Result<BTreeMap<String, SourceHash>> {
        let results: Vec<io::Result<(String, Stamp)>> = paths
            .par_iter()
            .map(|&path| {
                let file = data.path(path);
                let meta = fs::metadata(&file)
                    .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", file.display())))?;
                let modified_ns = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                if let Some(stamp) = self.stamps.get(path)
                    && stamp.len == meta.len()
                    && stamp.modified_ns == modified_ns
                {
                    return Ok((path.to_owned(), stamp.clone()));
                }
                let mut hasher = blake3::Hasher::new();
                let mut reader = fs::File::open(&file)?;
                let mut head = [0u8; 64];
                let read = read_up_to(&mut reader, &mut head)?;
                hasher.update(&head[..read]);
                io::copy(&mut reader, &mut hasher)?;
                Ok((
                    path.to_owned(),
                    Stamp {
                        len: meta.len(),
                        modified_ns,
                        hash: hasher.finalize().to_hex().to_string(),
                        lfs_pointer: head[..read].starts_with(LFS_POINTER),
                    },
                ))
            })
            .collect();
        let mut out = BTreeMap::new();
        for result in results {
            let (path, stamp) = result?;
            let mut hash = [0u8; 32];
            for (i, byte) in hash.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&stamp.hash[i * 2..i * 2 + 2], 16).unwrap_or(0);
            }
            out.insert(
                path.clone(),
                SourceHash {
                    hash,
                    lfs_pointer: stamp.lfs_pointer,
                    len: stamp.len,
                },
            );
            self.stamps.insert(path, stamp);
        }
        Ok(out)
    }

    pub fn save_stamps(&self) -> io::Result<()> {
        write_atomic(
            &self.root.join("stamps.json"),
            &serde_json::to_vec(&self.stamps).expect("stamps serialise"),
        )
    }

    pub fn job(&self, hash: &Hash) -> Option<JobRecord> {
        let bytes = fs::read(self.job_path(hash)).ok()?;
        let record: JobRecord = serde_json::from_slice(&bytes).ok()?;
        record
            .entries
            .iter()
            .all(|e| self.blob_path(&e.blob).is_file())
            .then_some(record)
    }

    pub fn store_job(&self, hash: &Hash, record: &JobRecord) -> io::Result<()> {
        write_atomic(
            &self.job_path(hash),
            &serde_json::to_vec(record).expect("records serialise"),
        )
    }

    pub fn has_blob(&self, key: &str) -> bool {
        self.blob_path(key).is_file()
    }

    pub fn store_blob(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        write_atomic(&self.blob_path(key), bytes)
    }

    pub fn blob_path(&self, key: &str) -> PathBuf {
        self.root.join("blobs").join(format!("{key}.blob"))
    }

    fn job_path(&self, hash: &Hash) -> PathBuf {
        self.root.join("jobs").join(format!("{}.json", hex(hash)))
    }

    /// Removes job records and blobs no current job refers to.
    pub fn collect_garbage(
        &self,
        jobs: &BTreeSet<String>,
        blobs: &BTreeSet<String>,
    ) -> io::Result<()> {
        for (dir, keep, suffix) in [("jobs", jobs, ".json"), ("blobs", blobs, ".blob")] {
            for entry in fs::read_dir(self.root.join(dir))? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let key = name.strip_suffix(suffix).unwrap_or(&name);
                if !keep.contains(key) {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        Ok(())
    }
}

fn read_up_to(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Writes through a temporary file and a rename, so readers never see half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp{}-{unique}", std::process::id()));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}
