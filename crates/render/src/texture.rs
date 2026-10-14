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
