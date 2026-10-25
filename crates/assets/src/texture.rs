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

/// Whether a BC7 block decodes every texel with an alpha of exactly 255: modes 0–3 store
/// no alpha; modes 4–7 do, and are opaque when every alpha endpoint is 255 (any blend of
/// 255 and 255 is 255) and, in modes 4 and 5, no channel rotation swaps a colour into
/// alpha. Conservative: a block it calls translucent may still decode opaque.
pub(crate) fn bc7_block_opaque(block: &[u8]) -> bool {
    let bits = u128::from_le_bytes(block.try_into().expect("a 16-byte block"));
    let field = |at: u32, len: u32| ((bits >> at) & ((1u128 << len) - 1)) as u32;
    match block[0].trailing_zeros() {
        0..=3 => true,
        // Mode 4: rotation (bits 5–6), index mode, colours 6 × 5 bits, alpha 2 × 6 bits.
        4 => field(5, 2) == 0 && field(38, 6) == 63 && field(44, 6) == 63,
        // Mode 5: rotation (6–7), colours 6 × 7 bits, alpha 2 × 8 bits.
        5 => field(6, 2) == 0 && field(50, 8) == 255 && field(58, 8) == 255,
        // Mode 6: colours 6 × 7 bits, alpha 2 × 7 bits, two p-bits.
        6 => field(49, 7) == 127 && field(56, 7) == 127 && field(63, 2) == 3,
        // Mode 7: partition, colours 12 × 5 bits, alpha 4 × 5 bits, four p-bits.
        7 => (0..4).all(|i| field(74 + 5 * i, 5) == 31) && field(94, 4) == 15,
        // A reserved mode decodes as zero.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wherever the check says opaque, the decoder agrees, over every mode.
    #[test]
    fn opaque_blocks_decode_opaque() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut opaque = [0u32; 8];
        for i in 0..200_000u32 {
            let mut block = [0u8; 16];
            for b in block.chunks_exact_mut(8) {
                b.copy_from_slice(&next().to_le_bytes());
            }
            let mode = i % 8;
            let low = ((2u16 << mode) - 1) as u8;
            block[0] = (block[0] & !low) | (1 << mode);
            let bits = u128::from_le_bytes(block);
            let set = |bits: u128, at: u32, len: u32| bits | (((1u128 << len) - 1) << at);
            // Half the blocks get opaque alpha endpoints and no rotation.
            let bits = if i % 16 < 8 {
                match mode {
                    4 => set(set(bits & !(3 << 5), 38, 6), 44, 6),
                    5 => set(set(bits & !(3 << 6), 50, 8), 58, 8),
                    6 => set(set(set(bits, 49, 7), 56, 7), 63, 2),
                    7 => (0..4).fold(set(bits, 94, 4), |b, k| set(b, 74 + 5 * k, 5)),
                    _ => bits,
                }
            } else {
                bits
            };
            let block = bits.to_le_bytes();
            if !bc7_block_opaque(&block) {
                continue;
            }
            opaque[mode as usize] += 1;
            let rgba = decode(TextureFormat::Bc7RgbaUnorm, 4, 4, 1, &block);
            assert!(
                rgba.chunks_exact(4).all(|t| t[3] == 255),
                "mode {mode} block {block:02x?} decodes translucent"
            );
        }
        assert!(opaque.iter().all(|&n| n > 1000), "{opaque:?}");
    }
}
