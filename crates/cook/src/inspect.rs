//! `--inspect` and `--stats`: what a pack holds.

use anyhow::Result;
use canopy_assets::{EntryKind, IndexEntry, Pack, TextureDimension};
use std::{fmt::Write as _, path::Path, time::Instant};

/// Describes every entry matching `query`: an entry name, a published asset id (all of
/// its files), or a scenery prototype id (its model).
pub fn inspect(pack_path: &Path, query: &str) -> Result<String> {
    let pack = Pack::load(pack_path)?;
    let folder = format!("{query}/");
    let mut names: Vec<String> = pack
        .entries()
        .iter()
        .filter(|e| {
            e.name == query
                || e.name.starts_with(&folder)
                || e.name.starts_with(&format!("{query}#"))
        })
        .map(|e| e.name.clone())
        .collect();
    if names.is_empty()
        && let Ok(scenery) = pack.scenery()
        && let Some(prototype) = scenery.prototypes.iter().find(|p| p.id == query)
    {
        names.push(prototype.model.clone());
    }
    if names.is_empty() {
        anyhow::bail!("nothing in {} matches {query:?}", pack_path.display());
    }
    let mut out = String::new();
    for name in &names {
        if let Some(entry) = pack.entry(name) {
            describe(&pack, entry, &mut out)?;
        }
    }
    Ok(out)
}

fn kib(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / f64::from(1 << 20))
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

fn describe(pack: &Pack, entry: &IndexEntry, out: &mut String) -> Result<()> {
    writeln!(out, "{} ({:?}, {})", entry.name, entry.kind, kib(entry.len))?;
    match entry.kind {
        EntryKind::Model => {
            let model = pack.blob(entry)?.into_model()?;
            let m = &model.meta;
            let triangles: u32 = m.sections.iter().map(|s| s.index_count / 3).sum();
            writeln!(out, "  asset {}", m.asset)?;
            let mut streams = vec!["position+wind", "octahedral normal", "uv"];
            if m.colors.is_some() {
                streams.push("colour");
            }
            if m.leaves.is_some() {
                streams.push("leaf pivots");
            }
            if m.skin.is_some() {
                streams.push("skin");
            }
            writeln!(
                out,
                "  {} vertices, {} triangles, {:?} indices; streams: {}",
                m.vertex_count,
                triangles,
                m.index_format,
                streams.join(", ")
            )?;
            writeln!(
                out,
                "  depth: {} vertices, {} triangles{}",
                m.depth.vertex_count,
                m.depth.index_count / 3,
                if m.depth.masked.is_empty() {
                    String::new()
                } else {
                    format!(", alpha-tested sections {:?}", m.depth.masked)
                }
            )?;
            for (i, lod) in m.lods.iter().enumerate() {
                writeln!(
                    out,
                    "  lod {}: {} triangles, depth {}, error {:.4}",
                    i + 1,
                    lod.sections.iter().map(|r| r[1] / 3).sum::<u32>(),
                    lod.depth[1] / 3,
                    lod.error
                )?;
            }
            writeln!(
                out,
                "  bounds {:?} – {:?} (rest {:?} – {:?})",
                m.bounds.min, m.bounds.max, m.rest_bounds.min, m.rest_bounds.max
            )?;
            writeln!(
                out,
                "  ground {:?}, soil offset {:.3} m{}",
                m.ground,
                m.ground_offset,
                if m.canopy { ", canopy" } else { "" }
            )?;
            for section in &m.sections {
                let material = &m.materials[section.material as usize];
                writeln!(
                    out,
                    "  section {:?}: {:?}, {} triangles, {:?}{}{}{}",
                    material.name,
                    material.model,
                    section.index_count / 3,
                    material.alpha,
                    if material.double_sided {
                        ", double-sided"
                    } else {
                        ""
                    },
                    if material.team_color.is_some() {
                        ", team colour"
                    } else {
                        ""
                    },
                    if material.casts_shadow {
                        ""
                    } else {
                        ", no shadow"
                    },
                )?;
                let t = &material.textures;
                for (slot, name) in [
                    ("base", &t.base_color),
                    ("team mask", &t.team_mask),
                    ("normal", &t.normal),
                    ("metal/rough", &t.metallic_roughness),
                    ("occlusion", &t.occlusion),
                    ("emissive", &t.emissive),
                ] {
                    if let Some(name) = name {
                        let texture = pack.texture(name)?;
                        writeln!(
                            out,
                            "    {slot}: {:?} {}×{}, {} mips",
                            texture.meta.format,
                            texture.meta.width,
                            texture.meta.height,
                            texture.meta.mips.len()
                        )?;
                    }
                }
            }
            if let Some(skeleton) = &m.skeleton {
                writeln!(out, "  skeleton: {} joints", skeleton.joints.len())?;
                if let Some(ragdoll) = &skeleton.ragdoll {
                    writeln!(out, "  ragdoll: {} parts", ragdoll.parts.len())?;
                    for part in &ragdoll.parts {
                        let joint = &skeleton.joints[part.joint as usize].name;
                        let parent = part
                            .parent
                            .map(|p| {
                                skeleton.joints[ragdoll.parts[p as usize].joint as usize]
                                    .name
                                    .as_str()
                            })
                            .unwrap_or(if part.parent.is_none() { "-" } else { "" });
                        let shape = match part.shape {
                            canopy_assets::RagdollShape::Capsule { a, b, radius } => {
                                let l = ((a[0] - b[0]).powi(2)
                                    + (a[1] - b[1]).powi(2)
                                    + (a[2] - b[2]).powi(2))
                                .sqrt();
                                format!("capsule {l:.2} × r {radius:.2}")
                            }
                            canopy_assets::RagdollShape::Box { half, .. } => format!(
                                "box {:.2} × {:.2} × {:.2}",
                                2.0 * half[0],
                                2.0 * half[1],
                                2.0 * half[2]
                            ),
                        };
                        writeln!(
                            out,
                            "    {joint} (on {parent}): {shape}, {:.1} kg",
                            part.mass
                        )?;
                    }
                }
            }
            for clip in &m.clips {
                writeln!(
                    out,
                    "  clip {:?}: {:.3} s, {} frames",
                    clip.name, clip.duration, clip.frame_count
                )?;
            }
            for state in &m.states {
                let events: Vec<String> = state
                    .events
                    .iter()
                    .map(|e| format!("{}@{:.2}", e.name, e.time))
                    .collect();
                writeln!(
                    out,
                    "  state {} → {:?}{}{}",
                    state.semantic,
                    m.clips[state.clip as usize].name,
                    if state.looping { " (loop)" } else { "" },
                    if events.is_empty() {
                        String::new()
                    } else {
                        format!(" events {}", events.join(" "))
                    }
                )?;
            }
            for socket in &m.sockets {
                writeln!(
                    out,
                    "  socket {} on {:?} at ({:.3}, {:.3}, {:.3})",
                    socket.name, socket.node, socket.rest[3], socket.rest[7], socket.rest[11]
                )?;
            }
            if let Some(wind) = &m.wind {
                writeln!(
                    out,
                    "  wind: amplitude {:.3} m, {:.2} Hz, capability {:?}",
                    wind.amplitude, wind.speed, wind.mode
                )?;
            }
            let p = &m.prototype;
            if let Some(d) = p.dimensions {
                writeln!(
                    out,
                    "  body: radius {}, height {}, spacing {}",
                    d[0], d[1], d[2]
                )?;
            }
            if let Some(b) = &p.blocker {
                writeln!(out, "  blocker: {:?} {}×{}", b.shape, b.width, b.depth)?;
            }
        }
        EntryKind::Texture => {
            let texture = pack.blob(entry)?.into_texture()?;
            let t = &texture.meta;
            let size = match t.dimension {
                TextureDimension::D2 => format!("{}×{}", t.width, t.height),
                TextureDimension::D3 => format!("{}×{}×{}", t.width, t.height, t.depth),
            };
            let source = if (t.source_width, t.source_height) != (t.width, t.height) {
                format!(" (source {}×{})", t.source_width, t.source_height)
            } else {
                String::new()
            };
            writeln!(
                out,
                "  {:?} {size}{source}, {} mips, alpha {:?}",
                t.format,
                t.mips.len(),
                t.alpha
            )?;
        }
        EntryKind::Sound => {
            let sound = pack.blob(entry)?.into_sound()?;
            let s = &sound.meta;
            writeln!(
                out,
                "  PCM s16, {} Hz, {} channel(s), {} frames ({:.2} s)",
                s.sample_rate,
                s.channels,
                s.frames,
                f64::from(s.frames) / f64::from(s.sample_rate.max(1))
            )?;
        }
        EntryKind::Data => {
            let blob = pack.blob(entry)?;
            writeln!(out, "  {} bytes", blob.bytes()?.len())?;
        }
        EntryKind::Scenery => {
            let table = pack.blob(entry)?.scenery()?;
            writeln!(out, "  {} prototypes", table.prototypes.len())?;
        }
        EntryKind::Effects => {
            let table = pack.blob(entry)?.effects()?;
            let layers: usize = table.effects.iter().map(|e| e.layers.len()).sum();
            writeln!(out, "  {} effects, {layers} layers", table.effects.len())?;
        }
    }
    Ok(())
}

/// Load time and totals by kind.
pub fn stats(pack_path: &Path) -> Result<String> {
    let start = Instant::now();
    let pack = Pack::load(pack_path)?;
    let read = start.elapsed();
    let start = Instant::now();
    let mut decoded = 0usize;
    for entry in pack.entries() {
        let blob = pack.blob(entry)?;
        match entry.kind {
            EntryKind::Model => decoded += blob.into_model()?.vertices().len(),
            EntryKind::Texture => decoded += blob.into_texture()?.level(0).len(),
            EntryKind::Sound => decoded += blob.into_sound()?.samples().len(),
            EntryKind::Data => decoded += blob.bytes()?.len(),
            EntryKind::Scenery => decoded += blob.scenery()?.prototypes.len(),
            EntryKind::Effects => decoded += blob.effects()?.effects.len(),
        }
    }
    let views = start.elapsed();
    let mut out = String::new();
    let len = std::fs::metadata(pack_path)?.len();
    writeln!(
        out,
        "{}: {}, {} entries",
        pack_path.display(),
        kib(len),
        pack.entries().len()
    )?;
    for kind in [
        EntryKind::Model,
        EntryKind::Texture,
        EntryKind::Sound,
        EntryKind::Data,
        EntryKind::Scenery,
        EntryKind::Effects,
    ] {
        let entries: Vec<&IndexEntry> = pack.entries().iter().filter(|e| e.kind == kind).collect();
        let mut offsets: Vec<u64> = entries.iter().map(|e| e.offset).collect();
        offsets.sort();
        offsets.dedup();
        let bytes: u64 = entries
            .iter()
            .filter(|e| offsets.binary_search(&e.offset).is_ok())
            .map(|e| (e.offset, e.len))
            .collect::<std::collections::BTreeMap<_, _>>()
            .values()
            .sum();
        writeln!(out, "  {kind:?}: {} entries, {}", entries.len(), kib(bytes))?;
    }
    writeln!(
        out,
        "load: one read {:.1} ms; decoding every entry's metadata and views {:.1} ms ({decoded} items)",
        read.as_secs_f64() * 1e3,
        views.as_secs_f64() * 1e3
    )?;
    Ok(out)
}
