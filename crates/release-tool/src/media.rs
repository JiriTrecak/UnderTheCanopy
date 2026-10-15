//! Gallery images: WebP files in `releases/media/` named by their SHA-256, checked at every
//! boundary (check, prepare, build, publish) against their name and declared size.

use anyhow::{Context, Result, bail};
use canopy_release::ReleaseImage;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The largest gallery file.
pub const MAX_BYTES: u64 = 768 * 1024;

/// The start of a Git LFS pointer file (the repository stores `*.webp` in LFS).
const LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v1";

#[derive(Debug, PartialEq, Eq)]
pub enum Checked {
    /// The file matches its name, format and size.
    Verified(PathBuf),
    /// The checkout has only the LFS pointer (CI checkouts skip LFS).
    LfsPointer(PathBuf),
}

/// Verifies `image` in `folder`. An LFS pointer is reported as such: an error unless
/// `allow_pointers` (CI, which has no LFS content).
pub fn verify_image(folder: &Path, image: &ReleaseImage, allow_pointers: bool) -> Result<Checked> {
    if let Some(caption) = &image.caption
        && (caption.trim() != caption || caption.chars().count() > 400)
    {
        bail!(
            "{}: the caption must be trimmed, at most 400 characters",
            image.file
        );
    }
    let path = folder.join(&image.file);
    let bytes =
        fs::read(&path).with_context(|| format!("gallery image {} is missing", path.display()))?;
    if bytes.starts_with(LFS_POINTER) {
        if allow_pointers {
            return Ok(Checked::LfsPointer(path));
        }
        bail!(
            "gallery image {} is a Git LFS pointer, not the image; run `git lfs pull`",
            image.file
        );
    }
    if bytes.len() as u64 > MAX_BYTES {
        bail!("gallery image exceeds 768 KiB: {}", image.file);
    }
    if format!("{}.webp", crate::package::hex(&Sha256::digest(&bytes))) != image.file {
        bail!("gallery image hash mismatch: {}", image.file);
    }
    match webp_size(&bytes) {
        Ok((w, h)) if (w, h) == (image.width, image.height) => Ok(Checked::Verified(path)),
        Ok((w, h)) => bail!(
            "gallery image {} is {w}×{h}, the log says {}×{}",
            image.file,
            image.width,
            image.height
        ),
        Err(e) => bail!("gallery image {}: {e}", image.file),
    }
}

/// The pixel size of a still WebP image; animated or other files are refused.
pub fn webp_size(bytes: &[u8]) -> Result<(u32, u32), &'static str> {
    if bytes.len() < 30 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err("not a WebP file");
    }
    let data = &bytes[20..];
    let u16le = |at: usize| u32::from(u16::from_le_bytes([data[at], data[at + 1]]));
    let u24le = |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], 0]);
    match &bytes[12..16] {
        b"VP8 " => {
            if data[3..6] != [0x9d, 0x01, 0x2a] {
                return Err("damaged lossy WebP header");
            }
            Ok((u16le(6) & 0x3fff, u16le(8) & 0x3fff))
        }
        b"VP8L" => {
            if data[0] != 0x2f {
                return Err("damaged lossless WebP header");
            }
            let bits = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
            Ok(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => {
            if data[0] & 0x02 != 0 {
                return Err("animated WebP is not allowed");
            }
            Ok((u24le(4) + 1, u24le(7) + 1))
        }
        _ => Err("unknown WebP encoding"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn riff(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = b"RIFF\0\0\0\0WEBP".to_vec();
        out.extend_from_slice(fourcc);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out.resize(out.len().max(40), 0);
        out
    }

    #[test]
    fn webp_sizes_read_from_every_encoding() {
        // Lossless, from an independent encoder.
        let mut lossless = Vec::new();
        image::RgbaImage::new(37, 21)
            .write_to(
                &mut std::io::Cursor::new(&mut lossless),
                image::ImageFormat::WebP,
            )
            .unwrap();
        assert_eq!(webp_size(&lossless), Ok((37, 21)));
        // Lossy key frame header: 1280×720.
        let mut vp8 = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        vp8.extend_from_slice(&1280u16.to_le_bytes());
        vp8.extend_from_slice(&720u16.to_le_bytes());
        assert_eq!(webp_size(&riff(b"VP8 ", &vp8)), Ok((1280, 720)));
        // Extended: 1600×900 canvas, still and animated.
        let mut vp8x = vec![0, 0, 0, 0];
        vp8x.extend_from_slice(&1599u32.to_le_bytes()[..3]);
        vp8x.extend_from_slice(&899u32.to_le_bytes()[..3]);
        assert_eq!(webp_size(&riff(b"VP8X", &vp8x)), Ok((1600, 900)));
        vp8x[0] = 0x02;
        assert!(webp_size(&riff(b"VP8X", &vp8x)).is_err());
        assert!(webp_size(b"\x89PNG\r\n\x1a\n not a webp at all, really not").is_err());
    }

    #[test]
    fn images_must_match_their_name_and_size() {
        let dir = std::env::temp_dir().join(format!("canopy-media-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut bytes = Vec::new();
        image::RgbaImage::new(8, 4)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::WebP,
            )
            .unwrap();
        let file = format!("{}.webp", crate::package::hex(&Sha256::digest(&bytes)));
        fs::write(dir.join(&file), &bytes).unwrap();
        let image = |file: &str, width| ReleaseImage {
            file: file.to_owned(),
            alt: "alt".into(),
            caption: None,
            width,
            height: 4,
        };
        assert!(matches!(
            verify_image(&dir, &image(&file, 8), false),
            Ok(Checked::Verified(_))
        ));
        assert!(verify_image(&dir, &image(&file, 9), false).is_err());
        // Renamed: the hash no longer matches.
        let other = format!("{}.webp", "0".repeat(64));
        fs::write(dir.join(&other), &bytes).unwrap();
        assert!(verify_image(&dir, &image(&other, 8), false).is_err());
        // An LFS pointer is reported, and only tolerated when asked.
        fs::write(
            dir.join(&other),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 1\n",
        )
        .unwrap();
        assert!(verify_image(&dir, &image(&other, 8), false).is_err());
        assert!(matches!(
            verify_image(&dir, &image(&other, 8), true),
            Ok(Checked::LfsPointer(_))
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
