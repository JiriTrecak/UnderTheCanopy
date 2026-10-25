//! The CPU fallback for devices without BC texture compression.

use crate::TextureFormat;
use block_compression::{BC7Settings, CompressionVariant, decode::decompress_blocks_as_rgba8};

/// Decodes one level to the format's [`TextureFormat::uncompressed`] layout.
pub(crate) fn decode(
    format: TextureFormat,
    width: u32,
    height: u32,
    depth: u32,
    bytes: &[u8],
) -> Vec<u8> {
    if !format.is_compressed() {
        return bytes.to_vec();
    }
    let (padded_w, padded_h) = (width.div_ceil(4) * 4, height.div_ceil(4) * 4);
    let layer_blocks = format.level_size(width, height, 1);
    let mut out = Vec::with_capacity((width * height * depth * 4) as usize);
    let mut padded = vec![0u8; (padded_w * padded_h * 4) as usize];
    for layer in bytes.chunks_exact(layer_blocks).take(depth as usize) {
        // The decoder writes whole blocks: decode at block size, then crop.
        decompress_blocks_as_rgba8(
            CompressionVariant::BC7(BC7Settings::opaque_basic()),
            padded_w,
            padded_h,
            layer,
            &mut padded,
        );
        for row in padded
            .chunks_exact(padded_w as usize * 4)
            .take(height as usize)
        {
            out.extend_from_slice(&row[..width as usize * 4]);
        }
    }
    out
}
