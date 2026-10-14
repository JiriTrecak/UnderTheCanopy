//! Images to GPU-ready textures: resampling to whole blocks, mip chains filtered in
//! linear light, coverage-preserving cutout mips, BC7 encoding, lossless R8 masks.

use canopy_assets::{
    BlobBuilder, EntryKind, TextureAlpha, TextureDimension, TextureFormat, TextureMeta,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// An 8-bit RGBA image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// Decodes PNG, JPEG or WebP.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
        let rgba = image.to_rgba8();
        Ok(Self {
            width: rgba.width(),
            height: rgba.height(),
            pixels: rgba.into_raw(),
        })
    }

    pub fn has_alpha(&self) -> bool {
        self.pixels.chunks_exact(4).any(|p| p[3] != 255)
    }

    /// A copy whose red channel is channel `c` of this image.
    pub fn channel(&self, c: usize) -> Self {
        Self {
            width: self.width,
            height: self.height,
            pixels: self
                .pixels
                .chunks_exact(4)
                .flat_map(|p| [p[c], p[c], p[c], 255])
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorSpace {
    Srgb,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AlphaPolicy {
    /// Opaque when every pixel is, otherwise straight translucency.
    Auto,
    /// Ignore alpha.
    Opaque,
    Cutout(f32),
    /// Alpha is an independent data channel.
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    Bc7,
    /// The red channel, uncompressed: lossless masks.
    R8,
}

/// How to cook one image. Part of every cache key.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextureParams {
    pub color: ColorSpace,
    pub alpha: AlphaPolicy,
    /// RGB holds a tangent-space normal: mips are renormalised.
    pub normal_map: bool,
    pub encoding: Encoding,
}

impl TextureParams {
    pub const fn color(color: ColorSpace) -> Self {
        Self {
            color,
            alpha: AlphaPolicy::Auto,
            normal_map: false,
            encoding: Encoding::Bc7,
        }
    }
}

/// BC7 encoder effort of Intel's ISPC kernels (docs/decisions.md, D26): "fast" is within
/// 0.2 dB of "basic" on colour at half the time; normal maps lose 1.5 dB with it, so
/// they keep "basic".
fn bc7_settings(alpha: bool, normal_map: bool) -> intel_tex_2::bc7::EncodeSettings {
    use intel_tex_2::bc7;
    match (normal_map, alpha) {
        (true, false) => bc7::opaque_basic_settings(),
        (true, true) => bc7::alpha_basic_settings(),
        (false, false) => bc7::opaque_fast_settings(),
        (false, true) => bc7::alpha_fast_settings(),
    }
}

static SRGB_TO_LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let c = i as f64 / 255.0;
        let linear = if c <= 0.04045 {
            c / 12.92
        } else {
            libm::pow((c + 0.055) / 1.055, 2.4)
        };
        linear as f32
    })
});

/// Linear values halfway between consecutive sRGB codes: encoding is a search.
static SRGB_THRESHOLDS: LazyLock<[f32; 255]> = LazyLock::new(|| {
    let table = &*SRGB_TO_LINEAR;
    std::array::from_fn(|i| (table[i] + table[i + 1]) * 0.5)
});

fn linear_to_srgb8(value: f32) -> u8 {
    SRGB_THRESHOLDS.partition_point(|&t| t < value) as u8
}

fn unorm8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// A float RGBA working image: RGB linear (premultiplied when the alpha is translucency).
#[derive(Clone)]
struct Work {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}

/// Cooks an image into a texture blob.
pub fn cook(image: &Image, params: TextureParams) -> Vec<u8> {
    let alpha = match params.alpha {
        AlphaPolicy::Auto if image.has_alpha() => TextureAlpha::Blend,
        AlphaPolicy::Auto | AlphaPolicy::Opaque => TextureAlpha::Opaque,
        AlphaPolicy::Cutout(cutoff) => TextureAlpha::Cutout(cutoff),
        AlphaPolicy::Data => TextureAlpha::Data,
    };
    let (format, block) = match (params.encoding, params.color) {
        (Encoding::R8, _) => (TextureFormat::R8Unorm, 1),
        (Encoding::Bc7, ColorSpace::Srgb) => (TextureFormat::Bc7RgbaUnormSrgb, 4),
        (Encoding::Bc7, ColorSpace::Linear) => (TextureFormat::Bc7RgbaUnorm, 4),
    };
    let srgb = params.color == ColorSpace::Srgb && params.encoding == Encoding::Bc7;
    let premultiplied = alpha == TextureAlpha::Blend;
    let mut work = to_work(image, srgb, premultiplied);
    let (width, height) = (
        whole_blocks(image.width, block),
        whole_blocks(image.height, block),
    );
    let resampled = (width, height) != (image.width, image.height);
    if resampled {
        work = resample(&work, width, height);
    }
    let coverage = match alpha {
        TextureAlpha::Cutout(cutoff) => Some((cutoff, coverage(&work, cutoff, 1.0))),
        _ => None,
    };
    let mut builder = BlobBuilder::new();
    let mut mips = Vec::new();
    // The top level is the source itself unless it was resampled.
    let mut top = (!resampled).then(|| {
        let mut pixels = image.pixels.clone();
        if alpha == TextureAlpha::Opaque {
            pixels.par_chunks_exact_mut(4).for_each(|p| p[3] = 255);
        }
        pixels
    });
    let mut level = work;
    loop {
        let bytes = match top.take() {
            Some(bytes) => bytes,
            None => from_work(&level, srgb, premultiplied, alpha, params.normal_map),
        };
        let encoded = match params.encoding {
            Encoding::R8 => bytes.chunks_exact(4).map(|p| p[0]).collect(),
            Encoding::Bc7 => {
                encode_bc7(&bytes, level.width, level.height, alpha, params.normal_map)
            }
        };
        mips.push(builder.push_bytes(&encoded));
        if level.width == 1 && level.height == 1 {
            break;
        }
        level = downsample(&level);
        if params.normal_map {
            renormalize(&mut level);
        }
        if let Some((cutoff, target)) = coverage {
            preserve_coverage(&mut level, cutoff, target);
        }
    }
    let meta = TextureMeta {
        format,
        dimension: TextureDimension::D2,
        width,
        height,
        depth: 1,
        source_width: image.width,
        source_height: image.height,
        alpha,
        mips,
    };
    builder.finish(EntryKind::Texture, &meta)
}

/// An uncompressed RGBA8 volume texture of `size`³ texels without mips (colour grades).
pub fn cook_volume(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mut builder = BlobBuilder::new();
    let span = builder.push_bytes(rgba);
    let meta = TextureMeta {
        format: TextureFormat::Rgba8Unorm,
        dimension: TextureDimension::D3,
        width: size,
        height: size,
        depth: size,
        source_width: size,
        source_height: size,
        alpha: TextureAlpha::Data,
        mips: vec![span],
    };
    builder.finish(EntryKind::Texture, &meta)
}

/// The nearest positive multiple of `block`.
fn whole_blocks(size: u32, block: u32) -> u32 {
    ((size + block / 2) / block * block).max(block)
}

fn to_work(image: &Image, srgb: bool, premultiplied: bool) -> Work {
    let table = &*SRGB_TO_LINEAR;
    let pixels = image
        .pixels
        .par_chunks_exact(4)
        .map(|p| {
            let a = f32::from(p[3]) / 255.0;
            let mut rgb: [f32; 3] = std::array::from_fn(|c| {
                if srgb {
                    table[p[c] as usize]
                } else {
                    f32::from(p[c]) / 255.0
                }
            });
            if premultiplied {
                rgb = rgb.map(|c| c * a);
            }
            [rgb[0], rgb[1], rgb[2], a]
        })
        .collect();
    Work {
        width: image.width,
        height: image.height,
        pixels,
    }
}

fn from_work(
    work: &Work,
    srgb: bool,
    premultiplied: bool,
    alpha: TextureAlpha,
    normal_map: bool,
) -> Vec<u8> {
    work.pixels
        .par_iter()
        .flat_map_iter(|p| {
            let a = p[3];
            let rgb: [f32; 3] = std::array::from_fn(|c| {
                if premultiplied {
                    if a > 0.0 { p[c] / a } else { 0.0 }
                } else {
                    p[c]
                }
            });
            let encode = |v: f32| {
                if srgb && !normal_map {
                    linear_to_srgb8(v)
                } else {
                    unorm8(v)
                }
            };
            let a8 = if alpha == TextureAlpha::Opaque {
                255
            } else {
                unorm8(a)
            };
            [encode(rgb[0]), encode(rgb[1]), encode(rgb[2]), a8]
        })
        .collect()
}

/// Halves each dimension (to at least 1) with an exact area filter, so odd sizes are
/// filtered correctly too.
fn downsample(src: &Work) -> Work {
    let width = (src.width / 2).max(1);
    let height = (src.height / 2).max(1);
    let weights_x = area_weights(src.width, width);
    let weights_y = area_weights(src.height, height);
    let pixels = (0..height as usize)
        .into_par_iter()
        .flat_map_iter(|y| {
            let (weights_x, weights_y) = (&weights_x, &weights_y);
            (0..width as usize).map(move |x| {
                let mut sum = [0.0f32; 4];
                for &(sy, wy) in &weights_y[y] {
                    let row = sy * src.width as usize;
                    for &(sx, wx) in &weights_x[x] {
                        let p = src.pixels[row + sx];
                        let w = wx * wy;
                        for c in 0..4 {
                            sum[c] += p[c] * w;
                        }
                    }
                }
                sum
            })
        })
        .collect();
    Work {
        width,
        height,
        pixels,
    }
}

/// For each destination texel, the source texels it covers and their area weights.
fn area_weights(src: u32, dst: u32) -> Vec<Vec<(usize, f32)>> {
    let ratio = f64::from(src) / f64::from(dst);
    (0..dst)
        .map(|d| {
            let (start, end) = (f64::from(d) * ratio, f64::from(d + 1) * ratio);
            let mut weights = Vec::new();
            let mut s = start.floor() as u32;
            while f64::from(s) < end && s < src {
                let covered = end.min(f64::from(s + 1)) - start.max(f64::from(s));
                if covered > 0.0 {
                    weights.push((s as usize, (covered / ratio) as f32));
                }
                s += 1;
            }
            weights
        })
        .collect()
}

/// Resamples to `width` × `height` with a separable Catmull-Rom filter (for the small
/// changes that round an image to whole blocks).
fn resample(src: &Work, width: u32, height: u32) -> Work {
    let rows = resample_axis(src, width, true);
    resample_axis(&rows, height, false)
}

fn resample_axis(src: &Work, size: u32, horizontal: bool) -> Work {
    let (src_len, other) = if horizontal {
        (src.width, src.height)
    } else {
        (src.height, src.width)
    };
    let scale = f64::from(src_len) / f64::from(size);
    let taps: Vec<Vec<(usize, f32)>> = (0..size)
        .map(|d| {
            let center = (f64::from(d) + 0.5) * scale - 0.5;
            let base = center.floor();
            let t = center - base;
            let weights = catmull_rom(t);
            let mut taps: Vec<(usize, f32)> = Vec::new();
            for (k, w) in weights.into_iter().enumerate() {
                let index = (base as i64 + k as i64 - 1).clamp(0, i64::from(src_len) - 1) as usize;
                taps.push((index, w as f32));
            }
            taps
        })
        .collect();
    let (width, height) = if horizontal {
        (size, other)
    } else {
        (other, size)
    };
    let mut pixels = vec![[0.0f32; 4]; (width * height) as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let (d, o) = if horizontal { (x, y) } else { (y, x) };
            let mut sum = [0.0f32; 4];
            for &(s, w) in &taps[d] {
                let p = if horizontal {
                    src.pixels[o * src.width as usize + s]
                } else {
                    src.pixels[s * src.width as usize + o]
                };
                for c in 0..4 {
                    sum[c] += p[c] * w;
                }
            }
            pixels[y * width as usize + x] = sum.map(|c| c.max(0.0));
        }
    }
    Work {
        width,
        height,
        pixels,
    }
}

fn catmull_rom(t: f64) -> [f64; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [
        0.5 * (-t3 + 2.0 * t2 - t),
        0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
        0.5 * (-3.0 * t3 + 4.0 * t2 + t),
        0.5 * (t3 - t2),
    ]
}

fn renormalize(work: &mut Work) {
    for p in &mut work.pixels {
        let v = [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 1e-6 {
            for c in 0..3 {
                p[c] = (v[c] / len) * 0.5 + 0.5;
            }
        }
    }
}

fn coverage(work: &Work, cutoff: f32, scale: f32) -> f32 {
    let covered = work
        .pixels
        .iter()
        .filter(|p| p[3] * scale >= cutoff)
        .count();
    covered as f32 / work.pixels.len() as f32
}

/// Scales a mip's alpha so as many texels pass the cutoff as in the top level: foliage
/// keeps its density in the distance.
fn preserve_coverage(work: &mut Work, cutoff: f32, target: f32) {
    let (mut low, mut high) = (0.0f32, 4.0f32);
    for _ in 0..16 {
        let mid = (low + high) * 0.5;
        if coverage(work, cutoff, mid) < target {
            low = mid;
        } else {
            high = mid;
        }
    }
    for p in &mut work.pixels {
        p[3] = (p[3] * high).min(1.0);
    }
}

/// BC7 blocks of an RGBA8 level; edges are padded by replication.
fn encode_bc7(
    rgba: &[u8],
    width: u32,
    height: u32,
    alpha: TextureAlpha,
    normal_map: bool,
) -> Vec<u8> {
    let (padded_w, padded_h) = (width.div_ceil(4) * 4, height.div_ceil(4) * 4);
    let padded = if (padded_w, padded_h) == (width, height) {
        rgba.to_vec()
    } else {
        let mut out = Vec::with_capacity((padded_w * padded_h * 4) as usize);
        for y in 0..padded_h {
            let row = y.min(height - 1) as usize * width as usize * 4;
            out.extend_from_slice(&rgba[row..row + width as usize * 4]);
            let last = &rgba[row + (width as usize - 1) * 4..row + width as usize * 4];
            for _ in width..padded_w {
                out.extend_from_slice(last);
            }
        }
        out
    };
    let settings = bc7_settings(alpha != TextureAlpha::Opaque, normal_map);
    let row_bytes = padded_w as usize * 4;
    let block_row_bytes = padded_w.div_ceil(4) as usize * 16;
    // Strips of 16 block rows encode in parallel; the output is identical to one call.
    let strip_rows = 64;
    let mut out = vec![0u8; block_row_bytes * (padded_h / 4) as usize];
    out.par_chunks_mut(block_row_bytes * strip_rows / 4)
        .zip(padded.par_chunks(row_bytes * strip_rows))
        .for_each(|(blocks, pixels)| {
            let surface = intel_tex_2::RgbaSurface {
                data: pixels,
                width: padded_w,
                height: (pixels.len() / row_bytes) as u32,
                stride: padded_w * 4,
            };
            intel_tex_2::bc7::compress_blocks_into(&settings, &surface, blocks);
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use block_compression::{BC7Settings, CompressionVariant};
    use canopy_assets::{BlobHeader, TextureMeta};

    fn gradient(width: u32, height: u32, alpha: impl Fn(u32, u32) -> u8) -> Image {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    (x * 255 / width) as u8,
                    (y * 255 / height) as u8,
                    128,
                    alpha(x, y),
                ]);
            }
        }
        Image {
            width,
            height,
            pixels,
        }
    }

    fn meta(blob: &[u8]) -> TextureMeta {
        let header: BlobHeader = bytemuck::pod_read_unaligned(&blob[..16]);
        postcard::from_bytes(&blob[16..16 + header.meta_len as usize]).unwrap()
    }

    #[test]
    fn srgb_round_trips_every_code() {
        for code in 0..=255u8 {
            assert_eq!(linear_to_srgb8(SRGB_TO_LINEAR[code as usize]), code);
        }
    }

    #[test]
    fn mip_chains_reach_one_texel_and_round_to_blocks() {
        let blob = cook(
            &gradient(30, 18, |_, _| 255),
            TextureParams::color(ColorSpace::Srgb),
        );
        let meta = meta(&blob);
        assert_eq!((meta.width, meta.height), (32, 20));
        assert_eq!((meta.source_width, meta.source_height), (30, 18));
        assert_eq!(meta.mips.len(), 6);
        assert_eq!(meta.format, TextureFormat::Bc7RgbaUnormSrgb);
        assert_eq!(meta.alpha, TextureAlpha::Opaque);
        let mut size = (32u32, 20u32);
        for span in &meta.mips {
            assert_eq!(span.len as usize, meta.format.level_size(size.0, size.1, 1));
            size = ((size.0 / 2).max(1), (size.1 / 2).max(1));
        }
    }

    #[test]
    fn masks_stay_lossless() {
        let image = gradient(13, 7, |x, y| ((x * 31 + y * 7) % 256) as u8);
        let params = TextureParams {
            color: ColorSpace::Linear,
            alpha: AlphaPolicy::Data,
            normal_map: false,
            encoding: Encoding::R8,
        };
        let blob = cook(&image.channel(3), params);
        let meta = meta(&blob);
        let header: BlobHeader = bytemuck::pod_read_unaligned(&blob[..16]);
        let data = &blob[header.data_offset as usize..];
        let top = &data[meta.mips[0].offset as usize..][..meta.mips[0].len as usize];
        let expected: Vec<u8> = image.pixels.chunks_exact(4).map(|p| p[3]).collect();
        assert_eq!(top, expected.as_slice());
    }

    #[test]
    fn cutout_mips_keep_their_coverage() {
        // Sparse leaves, denser to the right: every averaged texel stays under the cutoff.
        let image = gradient(64, 64, |x, y| {
            let density = 10 + 40 * x / 64;
            if (x * 37 + y * 91) % 100 < density {
                255
            } else {
                0
            }
        });
        let work = to_work(&image, true, false);
        let target = coverage(&work, 0.5, 1.0);
        let mut level = downsample(&downsample(&work));
        assert!(coverage(&level, 0.5, 1.0) < target / 2.0);
        preserve_coverage(&mut level, 0.5, target);
        assert!((coverage(&level, 0.5, 1.0) - target).abs() < 0.1);
    }

    #[test]
    fn identical_input_cooks_identical_bytes() {
        let image = gradient(40, 24, |x, _| (x * 6) as u8);
        let params = TextureParams::color(ColorSpace::Srgb);
        assert_eq!(cook(&image, params), cook(&image, params));
    }

    #[test]
    fn bc7_keeps_detail() {
        let image = gradient(64, 64, |_, _| 255);
        let blob = cook(&image, TextureParams::color(ColorSpace::Linear));
        let meta = meta(&blob);
        let header: BlobHeader = bytemuck::pod_read_unaligned(&blob[..16]);
        let data = &blob[header.data_offset as usize..];
        let top = &data[meta.mips[0].offset as usize..][..meta.mips[0].len as usize];
        let mut decoded = vec![0u8; 64 * 64 * 4];
        block_compression::decode::decompress_blocks_as_rgba8(
            CompressionVariant::BC7(BC7Settings::opaque_basic()),
            64,
            64,
            top,
            &mut decoded,
        );
        let worst = decoded
            .iter()
            .zip(&image.pixels)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 4, "worst error {worst}");
    }
}
