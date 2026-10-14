//! The offline cooker: reads the published data folder exactly as it is mirrored (D9) and
//! writes the runtime asset pack (`canopy-assets`) to `<out>/canopy.pack`.
//!
//! Cooking is incremental (only jobs whose inputs changed run), parallel (jobs and BC7
//! strips on every core) and deterministic (the same inputs give a byte-identical pack).

// The prebuilt ISPC kernels lack a macOS platform load command; the linker notes it.
#![cfg_attr(test, allow(linker_messages))]

pub mod anim;
mod cache;
pub mod diag;
mod inspect;
pub mod math;
pub mod model;
pub mod plan;
pub mod sound;
pub mod texture;

use anyhow::{Context, Result, bail};
use cache::{COOK_VERSION, Cache, Hash, JobEntry, JobRecord, hex};
use canopy_assets::{BlobBuilder, DataMeta, EntryKind, FORMAT_VERSION, Pack, PackWriter};
use diag::{Diagnostic, Diagnostics, Severity};
use plan::Job;
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::{BufWriter, Read},
    path::{Path, PathBuf},
    time::Instant,
};
use texture::{Image, TextureParams};

pub use inspect::{inspect, stats};

/// File name of the pack inside the output folder.
pub const PACK_FILE: &str = "canopy.pack";

#[derive(Debug, Clone)]
pub struct Options {
    /// The data folder (holding `content/` and `assets/`).
    pub data: PathBuf,
    /// Where the pack and the cache go (`target/cooked`).
    pub out: PathBuf,
    /// Ignore the cache and the up-to-date check.
    pub force: bool,
    /// Skip sources that are Git LFS pointers instead of failing (CI packaging).
    pub allow_lfs_pointers: bool,
}

#[derive(Debug, Default)]
pub struct Timings {
    pub plan: f64,
    pub hash: f64,
    pub cook: f64,
    pub link: f64,
}

#[derive(Debug, Default)]
pub struct Report {
    pub pack: PathBuf,
    pub jobs: usize,
    /// Jobs that ran (the rest came from the cache).
    pub cooked: usize,
    /// Nothing changed: the pack was left alone.
    pub up_to_date: bool,
    pub entries: usize,
    pub pack_bytes: u64,
    /// Bytes of every source file cooked.
    pub source_bytes: u64,
    /// Outputs deliberately not cooked, with the reason.
    pub skipped: Vec<(String, &'static str)>,
    /// Problems in the inputs, from every job (cached ones included).
    pub diagnostics: Vec<Diagnostic>,
    pub timings: Timings,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }
}

/// Cooks the data folder into `<out>/canopy.pack`.
pub fn cook(options: &Options) -> Result<Report> {
    let _span = tracing::info_span!("cook").entered();
    let mut report = Report {
        pack: options.out.join(PACK_FILE),
        ..Report::default()
    };
    let start = Instant::now();
    let inputs = plan::Inputs::load(&options.data)
        .map_err(|e| anyhow::anyhow!("the published data does not load:\n{e}"))?;
    let plan = plan::plan(&options.data, &inputs);
    report.skipped = plan.skipped;
    for path in plan.unknown {
        report.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            source: path,
            message: "output of an unknown type, not cooked".into(),
        });
    }
    report.timings.plan = start.elapsed().as_secs_f64();

    let start = Instant::now();
    fs::create_dir_all(&options.out)
        .with_context(|| format!("cannot create {}", options.out.display()))?;
    let mut cache = Cache::open(&options.out)?;
    let paths: BTreeSet<&str> = plan.jobs.iter().flat_map(Job::sources).collect();
    let sources = cache.hash_sources(&options.data, &paths)?;
    cache.save_stamps()?;
    report.timings.hash = start.elapsed().as_secs_f64();

    // Jobs whose sources are LFS pointers have no art to cook.
    let mut jobs = Vec::new();
    let mut pointers = Vec::new();
    for job in plan.jobs {
        if job.sources().iter().any(|s| sources[*s].lfs_pointer) {
            pointers.push(job.label().to_owned());
        } else {
            jobs.push(job);
        }
    }
    if !pointers.is_empty() {
        if !options.allow_lfs_pointers {
            bail!(
                "{} sources are Git LFS pointers, not files (first: {}); run `git lfs pull`",
                pointers.len(),
                pointers[0]
            );
        }
        report.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            source: "assets".into(),
            message: format!(
                "{} sources are Git LFS pointers and were left out of the pack",
                pointers.len()
            ),
        });
    }
    report.jobs = jobs.len();
    report.source_bytes = jobs
        .iter()
        .flat_map(Job::sources)
        .map(|s| sources[s].len)
        .sum();

    let hashes: Vec<Hash> = jobs.iter().map(|job| job_hash(job, &sources)).collect();
    let digest = inputs_digest(&hashes);
    if !options.force && Pack::peek_header(&report.pack).is_some_and(|h| h.inputs_digest == digest)
    {
        report.up_to_date = true;
        for hash in &hashes {
            if let Some(record) = cache.job(hash) {
                report.diagnostics.extend(record.diagnostics);
            }
        }
        let pack = Pack::open(&report.pack)?;
        report.entries = pack.entries().len();
        report.pack_bytes = fs::metadata(&report.pack)?.len();
        return Ok(report);
    }

    let start = Instant::now();
    let cached: Vec<Option<JobRecord>> = hashes
        .iter()
        .map(|h| if options.force { None } else { cache.job(h) })
        .collect();
    let mut todo: Vec<usize> = (0..jobs.len()).filter(|&i| cached[i].is_none()).collect();
    // Biggest first, so one large model does not finish alone at the end.
    todo.sort_by_key(|&i| {
        std::cmp::Reverse(
            jobs[i]
                .sources()
                .iter()
                .map(|s| sources[*s].len)
                .sum::<u64>(),
        )
    });
    report.cooked = todo.len();
    let fresh: Vec<(usize, Result<JobRecord>)> = todo
        .par_iter()
        .map(|&i| {
            let record = run(&jobs[i], &options.data, &cache, options.force);
            if let Ok(record) = &record {
                cache.store_job(&hashes[i], record)?;
            }
            Ok((i, record))
        })
        .collect::<Result<_>>()?;
    let mut records = cached;
    for (i, record) in fresh {
        records[i] = Some(record.with_context(|| format!("cooking {}", jobs[i].label()))?);
    }
    let records: Vec<JobRecord> = records.into_iter().map(Option::unwrap_or_default).collect();
    report.timings.cook = start.elapsed().as_secs_f64();
    for record in &records {
        report
            .diagnostics
            .extend(record.diagnostics.iter().cloned());
    }
    if report.errors() > 0 {
        return Ok(report);
    }

    let start = Instant::now();
    let entries = link(&cache, &records, &report.pack, digest)?;
    report.entries = entries;
    report.pack_bytes = fs::metadata(&report.pack)?.len();
    let job_keys: BTreeSet<String> = hashes.iter().map(hex).collect();
    let blob_keys: BTreeSet<String> = records
        .iter()
        .flat_map(|r| &r.entries)
        .map(|e| e.blob.clone())
        .collect();
    cache.collect_garbage(&job_keys, &blob_keys)?;
    report.timings.link = start.elapsed().as_secs_f64();
    Ok(report)
}

fn job_hash(job: &Job, sources: &BTreeMap<String, cache::SourceHash>) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"canopy-cook job");
    hasher.update(&COOK_VERSION.to_le_bytes());
    hasher.update(&FORMAT_VERSION.to_le_bytes());
    hasher.update(&serde_json::to_vec(job).expect("jobs serialise"));
    for source in job.sources() {
        hasher.update(&sources[source].hash);
    }
    *hasher.finalize().as_bytes()
}

fn inputs_digest(hashes: &[Hash]) -> Hash {
    let mut sorted = hashes.to_vec();
    sorted.sort();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"canopy-cook pack");
    hasher.update(&COOK_VERSION.to_le_bytes());
    hasher.update(&FORMAT_VERSION.to_le_bytes());
    for hash in &sorted {
        hasher.update(hash);
    }
    *hasher.finalize().as_bytes()
}

/// Writes the pack: entries sorted by name, each distinct blob once.
fn link(cache: &Cache, records: &[JobRecord], pack: &Path, digest: Hash) -> Result<usize> {
    let _span = tracing::info_span!("link").entered();
    let mut entries: Vec<&JobEntry> = records.iter().flat_map(|r| &r.entries).collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let tmp = pack.with_extension("pack.tmp");
    let file = fs::File::create(&tmp).with_context(|| format!("cannot write {}", tmp.display()))?;
    let mut writer = PackWriter::new(BufWriter::with_capacity(1 << 20, file))?;
    let mut written = HashMap::new();
    for entry in &entries {
        let location = match written.get(&entry.blob) {
            Some(&location) => location,
            None => {
                let bytes = fs::read(cache.blob_path(&entry.blob))?;
                let location = writer.write_blob(&bytes)?;
                written.insert(entry.blob.clone(), location);
                location
            }
        };
        writer.add_entry(entry.name.clone(), entry.kind, location);
    }
    writer
        .finish(digest)?
        .into_inner()
        .map_err(|e| e.into_error())?
        .sync_all()?;
    fs::rename(&tmp, pack)?;
    Ok(entries.len())
}

fn read_source(data: &Path, path: &str) -> Result<Vec<u8>> {
    fs::read(data.join(path)).with_context(|| format!("cannot read {path}"))
}

/// Runs one job, storing its blobs in the cache.
fn run(job: &Job, data: &Path, cache: &Cache, force: bool) -> Result<JobRecord> {
    let _span = tracing::debug_span!("job", source = job.label()).entered();
    let mut diags = Diagnostics::new(job.label());
    let mut entries = Vec::new();
    let store = |name: String, kind: EntryKind, blob: Vec<u8>| -> Result<JobEntry> {
        let key = hex(blake3::hash(&blob).as_bytes());
        if !cache.has_blob(&key) {
            cache.store_blob(&key, &blob)?;
        }
        Ok(JobEntry {
            name,
            kind,
            blob: key,
        })
    };
    match job {
        Job::Model {
            path,
            asset,
            profile,
            faction,
            published,
        } => {
            let bytes = read_source(data, path)?;
            let source = model::ModelSource {
                name: path,
                asset,
                profile,
                faction: *faction,
                published: published.as_deref(),
            };
            if let Some(cooked) = model::cook(&source, &bytes, &mut diags) {
                entries.push(store(path.clone(), EntryKind::Model, cooked.blob)?);
                // A model's textures cook in parallel; their order stays the request order.
                let textures: Vec<Result<Result<JobEntry, String>>> = cooked
                    .textures
                    .into_par_iter()
                    .map(|request| {
                        let key = texture_key(&request.bytes, request.select, request.params);
                        if force || !cache.has_blob(&key) {
                            let image = match Image::decode(&request.bytes) {
                                Ok(image) => image,
                                Err(e) => {
                                    return Ok(Err(format!(
                                        "{}: undecodable image: {e}",
                                        request.name
                                    )));
                                }
                            };
                            let image = match request.select {
                                model::Select::Image => image,
                                model::Select::Alpha => image.channel(3),
                            };
                            cache.store_blob(&key, &texture::cook(&image, request.params))?;
                        }
                        Ok(Ok(JobEntry {
                            name: request.name,
                            kind: EntryKind::Texture,
                            blob: key,
                        }))
                    })
                    .collect();
                for texture in textures {
                    match texture? {
                        Ok(entry) => entries.push(entry),
                        Err(message) => diags.error(message),
                    }
                }
            }
        }
        Job::Image { path, params } => {
            let bytes = read_source(data, path)?;
            match Image::decode(&bytes) {
                Ok(image) => entries.push(store(
                    path.clone(),
                    EntryKind::Texture,
                    texture::cook(&image, *params),
                )?),
                Err(e) => diags.error(format!("undecodable image: {e}")),
            }
        }
        Job::Tile { path, params } => {
            let bytes = gunzip(&read_source(data, path)?)?;
            let side = (bytes.len() / 4).isqrt();
            if side * side * 4 != bytes.len() || side == 0 {
                diags.error(format!(
                    "terrain tile of {} bytes is not a square RGBA8 image",
                    bytes.len()
                ));
            } else {
                let image = Image {
                    width: side as u32,
                    height: side as u32,
                    pixels: bytes,
                };
                entries.push(store(
                    path.clone(),
                    EntryKind::Texture,
                    texture::cook(&image, *params),
                )?);
            }
        }
        Job::Data { path } => {
            let raw = read_source(data, path)?;
            let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
                gunzip(&raw)?
            } else {
                raw
            };
            let mut builder = BlobBuilder::new();
            let meta = DataMeta {
                bytes: builder.push_bytes(&bytes),
            };
            entries.push(store(
                path.clone(),
                EntryKind::Data,
                builder.finish(EntryKind::Data, &meta),
            )?);
        }
        Job::Sound { path } => match sound::cook(&read_source(data, path)?) {
            Ok(blob) => entries.push(store(path.clone(), EntryKind::Sound, blob)?),
            Err(e) => diags.error(format!("unreadable WAV: {e}")),
        },
        Job::Grades { path } => {
            let mut errors = Vec::new();
            let grades =
                canopy_content::assets::textures::load_daytime_grades(data, path, &mut errors)
                    .context("colour grades no longer load")?;
            for (phase, volume) in [
                ("day", &grades.day),
                ("day_to_night", &grades.day_to_night),
                ("night", &grades.night),
                ("night_to_day", &grades.night_to_day),
                ("winter_day", &grades.winter_day),
            ] {
                match base64(&volume.rgba) {
                    Some(rgba) if rgba.len() == (volume.size as usize).pow(3) * 4 => {
                        entries.push(store(
                            format!("{path}#{phase}"),
                            EntryKind::Texture,
                            texture::cook_volume(volume.size, &rgba),
                        )?)
                    }
                    _ => diags.error(format!("grade {phase} is not {}³ RGBA texels", volume.size)),
                }
            }
        }
        Job::Scenery { table } => {
            let blob = BlobBuilder::new().finish(EntryKind::Scenery, table);
            entries.push(store(
                canopy_assets::SCENERY_ENTRY.to_owned(),
                EntryKind::Scenery,
                blob,
            )?);
        }
    }
    Ok(JobRecord {
        entries,
        diagnostics: diags.list,
    })
}

/// Textures are keyed by their image bytes and how they are cooked, so identical images
/// in different models are encoded once.
fn texture_key(bytes: &[u8], select: model::Select, params: TextureParams) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"canopy-cook texture");
    hasher.update(&COOK_VERSION.to_le_bytes());
    hasher.update(&FORMAT_VERSION.to_le_bytes());
    hasher.update(&serde_json::to_vec(&(select, params)).expect("params serialise"));
    hasher.update(bytes);
    hex(hasher.finalize().as_bytes())
}

fn gunzip(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .context("not gzip data")?;
    Ok(out)
}

/// Standard padded base64.
fn base64(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let padding = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        let mut n = 0u32;
        for &c in &chunk[..4 - padding] {
            n = (n << 6) | value(c)?;
        }
        n <<= 6 * padding as u32;
        let decoded = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&decoded[..3 - padding]);
    }
    Some(out)
}

/// The data folder: `start` or the nearest ancestor holding `content/game.json` and
/// `assets/manifest.json`.
pub fn find_data(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| {
            dir.join(canopy_content::GAME_FILE).is_file()
                && dir.join(canopy_content::MANIFEST_FILE).is_file()
        })
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_padding() {
        assert_eq!(base64("aGVsbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(base64("aGk=").as_deref(), Some(&b"hi"[..]));
        assert_eq!(base64("aGV5").as_deref(), Some(&b"hey"[..]));
        assert_eq!(base64("a*=="), None);
    }
}
