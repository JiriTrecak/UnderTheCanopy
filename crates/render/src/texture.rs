//! Texture uploads from the pack and small generated textures.

use crate::device::GpuFeatures;
use canopy_assets::{TextureDimension, TextureFormat};

/// The WebGPU format of a stored (or decoded) pack format.
pub fn wgpu_format(format: TextureFormat) -> wgpu::TextureFormat {
    match format {
        TextureFormat::Bc7RgbaUnorm => wgpu::TextureFormat::Bc7RgbaUnorm,
        TextureFormat::Bc7RgbaUnormSrgb => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
        TextureFormat::R8Unorm => wgpu::TextureFormat::R8Unorm,
        TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
    }
}

/// Uploads every mip of a pack texture. Without BC support the levels are decoded on the
/// CPU (D26).
pub fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    features: GpuFeatures,
    texture: &canopy_assets::Texture<'_>,
) -> wgpu::Texture {
    let meta = &texture.meta;
    let stored = meta.format;
    let format = if stored.is_compressed() && !features.bc {
        stored.uncompressed()
    } else {
        stored
    };
    let dimension = match meta.dimension {
        TextureDimension::D2 => wgpu::TextureDimension::D2,
        TextureDimension::D3 => wgpu::TextureDimension::D3,
    };
    let gpu = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(texture.name()),
        size: wgpu::Extent3d {
            width: meta.width,
            height: meta.height,
            depth_or_array_layers: meta.depth,
        },
        mip_level_count: meta.mips.len() as u32,
        sample_count: 1,
        dimension,
        format: wgpu_format(format),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for level in 0..meta.mips.len() {
        let (w, h, d) = texture.level_extent(level);
        let decoded;
        let bytes = if format == stored {
            texture.level(level)
        } else {
            decoded = texture.decode_level(level);
            &decoded
        };
        write_level(queue, &gpu, format, level as u32, 0, (w, h, d), bytes);
    }
    gpu
}

/// [`upload`] through a staging buffer of its own: the texels are copied into mapped
/// memory on the calling thread and `encoder` records the copies into the texture, so
/// threads upload at once (the queue's `write_texture` copies under one lock). The
/// texture holds the same texels once `encoder` is submitted.
pub fn upload_staged(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    features: GpuFeatures,
    texture: &canopy_assets::Texture<'_>,
) -> wgpu::Texture {
    let meta = &texture.meta;
    let stored = meta.format;
    let format = if stored.is_compressed() && !features.bc {
        stored.uncompressed()
    } else {
        stored
    };
    let dimension = match meta.dimension {
        TextureDimension::D2 => wgpu::TextureDimension::D2,
        TextureDimension::D3 => wgpu::TextureDimension::D3,
    };
    let gpu = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(texture.name()),
        size: wgpu::Extent3d {
            width: meta.width,
            height: meta.height,
            depth_or_array_layers: meta.depth,
        },
        mip_level_count: meta.mips.len() as u32,
        sample_count: 1,
        dimension,
        format: wgpu_format(format),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (levels, size) = staged_levels(texture, format);
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("texture staging"),
        size,
        usage: wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: true,
    });
    {
        let mut mapped = staging
            .slice(..)
            .get_mapped_range_mut()
            .expect("a buffer mapped at creation");
        fill_staged(texture, format, &levels, |at, bytes| {
            mapped.slice(at..at + bytes.len()).copy_from_slice(bytes);
        });
    }
    staging.unmap();
    for (
        level,
        &StagedLevel {
            at,
            padded,
            extent: (w, h, d),
            ..
        },
    ) in levels.iter().enumerate()
    {
        let (copy_w, copy_h) = if format.is_compressed() {
            (w.div_ceil(4) * 4, h.div_ceil(4) * 4)
        } else {
            (w, h)
        };
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: at,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(format.rows(h)),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &gpu,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: copy_w,
                height: copy_h,
                depth_or_array_layers: d,
            },
        );
    }
    gpu
}

/// Where one level of a staged upload lies in its buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StagedLevel {
    at: u64,
    /// Bytes of one row of texels (or blocks), and of the row with its padding.
    row: u32,
    padded: u32,
    extent: (u32, u32, u32),
}

/// The staging layout of `texture`'s levels as `format`: every level at an offset aligned
/// for copies, its rows padded to the copy alignment; and the buffer's size.
fn staged_levels(
    texture: &canopy_assets::Texture<'_>,
    format: TextureFormat,
) -> (Vec<StagedLevel>, u64) {
    let align = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let mut offset = 0u64;
    let levels = (0..texture.meta.mips.len())
        .map(|level| {
            let extent = texture.level_extent(level);
            let row = format.bytes_per_row(extent.0);
            let padded = u64::from(row).div_ceil(align) * align;
            let at = offset.div_ceil(align) * align;
            offset = at + padded * u64::from(format.rows(extent.1)) * u64::from(extent.2);
            StagedLevel {
                at,
                row,
                padded: padded as u32,
                extent,
            }
        })
        .collect();
    (levels, offset.max(align).div_ceil(align) * align)
}

/// Writes every level's rows of `texture` (decoded when `format` is not the stored one)
/// at their places in `levels` through `write(offset, bytes)`.
fn fill_staged(
    texture: &canopy_assets::Texture<'_>,
    format: TextureFormat,
    levels: &[StagedLevel],
    mut write: impl FnMut(usize, &[u8]),
) {
    for (level, l) in levels.iter().enumerate() {
        let decoded;
        let bytes = if format == texture.meta.format {
            texture.level(level)
        } else {
            decoded = texture.decode_level(level);
            &decoded
        };
        let (at, row, padded) = (l.at as usize, l.row as usize, l.padded as usize);
        let rows = format.rows(l.extent.1) as usize * l.extent.2 as usize;
        if row == padded {
            write(at, &bytes[..rows * row]);
        } else {
            for r in 0..rows {
                write(at + r * padded, &bytes[r * row..(r + 1) * row]);
            }
        }
    }
}

/// Reads and uploads the pack textures `names` across the CPU's cores (see
/// [`upload_staged`]), with each one's size in bytes, in order; the copies are submitted
/// before this returns.
pub fn upload_all(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    features: GpuFeatures,
    pack: &canopy_assets::Pack,
    names: &[&str],
) -> Vec<Result<(wgpu::Texture, u64), canopy_assets::Error>> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(names.len())
        .max(1);
    // Each worker stages its textures and records their copies; one submit.
    let (loaded, commands): (Vec<Vec<_>>, Vec<_>) = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(name) = names.get(i) else {
                            break (out, encoder.finish());
                        };
                        let loaded = pack.texture(name).map(|tex| {
                            let bytes = tex.meta.mips.iter().map(|m| u64::from(m.len)).sum();
                            (upload_staged(device, &mut encoder, features, &tex), bytes)
                        });
                        out.push((i, loaded));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().expect("a texture loads"))
            .unzip()
    });
    queue.submit(commands);
    let mut loaded: Vec<_> = loaded.into_iter().flatten().collect();
    loaded.sort_unstable_by_key(|(i, _)| *i);
    loaded.into_iter().map(|(_, l)| l).collect()
}

/// Writes one level (or one array layer of it) laid out as the format's rows.
pub fn write_level(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    format: TextureFormat,
    level: u32,
    layer: u32,
    (w, h, d): (u32, u32, u32),
    bytes: &[u8],
) {
    let (copy_w, copy_h) = if format.is_compressed() {
        (w.div_ceil(4) * 4, h.div_ceil(4) * 4)
    } else {
        (w, h)
    };
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: layer,
            },
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(format.bytes_per_row(w)),
            rows_per_image: Some(format.rows(h)),
        },
        wgpu::Extent3d {
            width: copy_w,
            height: copy_h,
            depth_or_array_layers: d,
        },
    );
}

/// A 1×1 texture of one RGBA8 colour.
pub fn solid(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    format: wgpu::TextureFormat,
    rgba: [u8; 4],
) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes: &[u8] = if format == wgpu::TextureFormat::R8Unorm {
        &rgba[..1]
    } else {
        &rgba
    };
    queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes.len() as u32),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture
}

/// Uploads pack textures of one size and format as the layers of a 2D array. Returns the
/// texture and its size in bytes.
pub fn upload_array(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    features: GpuFeatures,
    pack: &canopy_assets::Pack,
    label: &str,
    names: &[String],
) -> anyhow::Result<(wgpu::Texture, u64)> {
    use anyhow::{Context, bail};
    let first = pack
        .texture(names.first().context("an array needs a texture")?)
        .with_context(|| format!("{label}: {}", names[0]))?;
    let meta = first.meta.clone();
    let stored = meta.format;
    let format = if stored.is_compressed() && !features.bc {
        stored.uncompressed()
    } else {
        stored
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: meta.width,
            height: meta.height,
            depth_or_array_layers: names.len() as u32,
        },
        mip_level_count: meta.mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu_format(format),
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut bytes = 0u64;
    for (layer, name) in names.iter().enumerate() {
        let tex = pack
            .texture(name)
            .with_context(|| format!("{label}: {name}"))?;
        let m = &tex.meta;
        if m.width != meta.width
            || m.height != meta.height
            || m.mips.len() != meta.mips.len()
            || m.format != stored
            || m.dimension != canopy_assets::TextureDimension::D2
        {
            bail!(
                "{label}: {name} is {:?} {}×{} with {} mips; the array holds {:?} {}×{} with {}",
                m.format,
                m.width,
                m.height,
                m.mips.len(),
                stored,
                meta.width,
                meta.height,
                meta.mips.len()
            );
        }
        for level in 0..m.mips.len() {
            let (w, h, _) = tex.level_extent(level);
            let decoded;
            let data = if format == stored {
                tex.level(level)
            } else {
                decoded = tex.decode_level(level);
                &decoded
            };
            bytes += data.len() as u64;
            write_level(
                queue,
                &texture,
                format,
                level as u32,
                layer as u32,
                (w, h, 1),
                data,
            );
        }
    }
    Ok((texture, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_assets::{BlobBuilder, EntryKind, Pack, PackWriter, TextureAlpha, TextureMeta};

    fn blob(format: TextureFormat, dimension: TextureDimension, size: (u32, u32, u32)) -> Vec<u8> {
        let mut b = BlobBuilder::new();
        let mut mips = Vec::new();
        let mut level = size;
        let mut seed = 7u32;
        loop {
            let len = format.level_size(level.0, level.1, level.2);
            let texels: Vec<u8> = (0..len)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    (seed >> 16) as u8
                })
                .collect();
            mips.push(b.push(&texels));
            if level == (1, 1, 1) {
                break;
            }
            level = (
                (level.0 / 2).max(1),
                (level.1 / 2).max(1),
                match dimension {
                    TextureDimension::D2 => level.2,
                    TextureDimension::D3 => (level.2 / 2).max(1),
                },
            );
        }
        b.finish(
            EntryKind::Texture,
            &TextureMeta {
                format,
                dimension,
                width: size.0,
                height: size.1,
                depth: size.2,
                source_width: size.0,
                source_height: size.1,
                alpha: TextureAlpha::Opaque,
                mips,
            },
        )
    }

    #[test]
    fn staged_levels_hold_every_row_at_aligned_places() {
        let path = std::env::temp_dir().join(format!("canopy-staged-{}.pack", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = PackWriter::new(std::io::BufWriter::new(file)).unwrap();
        let cases = [
            (
                "bc7",
                TextureFormat::Bc7RgbaUnormSrgb,
                TextureDimension::D2,
                (72, 20, 1),
            ),
            (
                "rgba",
                TextureFormat::Rgba8Unorm,
                TextureDimension::D2,
                (33, 17, 1),
            ),
            (
                "mask",
                TextureFormat::R8Unorm,
                TextureDimension::D2,
                (70, 3, 1),
            ),
            (
                "wide",
                TextureFormat::Rgba8UnormSrgb,
                TextureDimension::D2,
                (256, 2, 1),
            ),
            (
                "volume",
                TextureFormat::Rgba8Unorm,
                TextureDimension::D3,
                (8, 8, 4),
            ),
        ];
        for (name, format, dimension, size) in cases {
            let at = writer.write_blob(&blob(format, dimension, size)).unwrap();
            writer.add_entry(name.into(), EntryKind::Texture, at);
        }
        writer.finish([0; 32]).unwrap();
        let pack = Pack::open(&path).unwrap();
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        for (name, format, ..) in cases {
            let texture = pack.texture(name).unwrap();
            let (levels, size) = staged_levels(&texture, format);
            assert_eq!(levels.len(), texture.meta.mips.len());
            let mut buffer = vec![0u8; size as usize];
            fill_staged(&texture, format, &levels, |at, bytes| {
                buffer[at..at + bytes.len()].copy_from_slice(bytes);
            });
            let mut end = 0;
            for (i, l) in levels.iter().enumerate() {
                let (at, row, padded) = (l.at as usize, l.row as usize, l.padded as usize);
                assert_eq!(at % align, 0, "{name} level {i} offset");
                assert_eq!(padded % align, 0, "{name} level {i} rows");
                assert!(at >= end, "{name} level {i} overlaps the one before");
                let rows = format.rows(l.extent.1) as usize * l.extent.2 as usize;
                let level = texture.level(i);
                assert_eq!(level.len(), rows * row, "{name} level {i} size");
                for r in 0..rows {
                    assert_eq!(
                        &buffer[at + r * padded..at + r * padded + row],
                        &level[r * row..(r + 1) * row],
                        "{name} level {i} row {r}"
                    );
                }
                end = at + rows * padded;
            }
            assert!(end <= size as usize, "{name}: the buffer holds every level");
        }
        let _ = std::fs::remove_file(path);
    }
}
