//! Terrain grids: raw little-endian samples. In JSON (the in-memory and test form of a
//! map) a grid is a base64 string; in a map package each grid is a `.grid` file:
//!
//! | bytes | field |
//! |---|---|
//! | 4 | magic `UTCG` |
//! | 1 | format, 1 |
//! | 1 | element: 1 `f32`, 2 `u8`, 3 four `u8` (water flow) |
//! | 1 | codec: 1 zlib |
//! | 1 | filter: bit 0 byte-shuffled |
//! | 4 | width (samples, little-endian `u32`) |
//! | 4 | height |
//! | … | the samples, byte-shuffled (multi-byte elements), zlib-compressed |
//!
//! Byte shuffling stores the first byte of every sample, then every second byte, and so
//! on: neighbouring samples share their high bytes, which compresses far better. The
//! samples come back bit-exact.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::io::{Read, Write};

const MAGIC: &[u8; 4] = b"UTCG";
const FORMAT: u8 = 1;
const CODEC_ZLIB: u8 = 1;
/// Samples stored as they are: only in memory (the editor's unsaved grids), never in a
/// package.
const CODEC_RAW: u8 = 0;
const FILTER_SHUFFLE: u8 = 1;
const HEADER: usize = 16;

/// File extension of a grid in a map package.
pub const GRID_EXTENSION: &str = "grid";

/// What one sample of a grid is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    F32,
    U8,
    /// Four bytes per sample (water flow: x, z, foam, profile).
    Rgba8,
}

impl Element {
    fn code(self) -> u8 {
        match self {
            Self::F32 => 1,
            Self::U8 => 2,
            Self::Rgba8 => 3,
        }
    }

    /// Bytes per sample.
    pub fn size(self) -> usize {
        match self {
            Self::F32 | Self::Rgba8 => 4,
            Self::U8 => 1,
        }
    }
}

/// A grid's raw bytes.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Grid(pub Vec<u8>);

impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Grid({} bytes)", self.0.len())
    }
}

impl Grid {
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// The grid's bytes when it holds exactly `count` of them.
    pub fn exactly(&self, count: usize) -> Result<&[u8], String> {
        if self.0.len() == count {
            Ok(&self.0)
        } else {
            Err("Wrong dimensions".into())
        }
    }

    /// `width × height` samples as an uncompressed grid file: for handing the editor's
    /// unsaved grids to the loader quickly; packages always hold [`Grid::to_file`]'s.
    pub fn to_raw_file(&self, element: Element, width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + self.0.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&[FORMAT, element.code(), CODEC_RAW, 0]);
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&self.0);
        out
    }

    /// `width × height` samples of `element` as a `.grid` file.
    pub fn to_file(&self, element: Element, width: u32, height: u32) -> Vec<u8> {
        let size = element.size();
        let shuffled: Vec<u8> = if size > 1 {
            (0..size)
                .flat_map(|plane| self.0.iter().skip(plane).step_by(size).copied())
                .collect()
        } else {
            self.0.clone()
        };
        let mut out = Vec::with_capacity(HEADER + shuffled.len() / 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&[
            FORMAT,
            element.code(),
            CODEC_ZLIB,
            if size > 1 { FILTER_SHUFFLE } else { 0 },
        ]);
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        let mut encoder = flate2::write::ZlibEncoder::new(out, flate2::Compression::best());
        encoder
            .write_all(&shuffled)
            .expect("writing to memory cannot fail");
        encoder.finish().expect("writing to memory cannot fail")
    }

    /// The sample type and size a `.grid` file declares.
    pub fn header(file: &[u8]) -> Result<(Element, u32, u32), String> {
        if file.len() < HEADER || &file[..4] != MAGIC {
            return Err("not a grid file".into());
        }
        let element = match file[5] {
            1 => Element::F32,
            2 => Element::U8,
            3 => Element::Rgba8,
            other => return Err(format!("unknown sample type {other}")),
        };
        let read =
            |at: usize| u32::from_le_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]);
        Ok((element, read(8), read(12)))
    }

    /// Reads a `.grid` file of whatever type and size it declares.
    pub fn read_file(file: &[u8]) -> Result<(Self, Element, u32, u32), String> {
        let (element, width, height) = Self::header(file)?;
        Ok((
            Self::from_file(file, element, width, height)?,
            element,
            width,
            height,
        ))
    }

    /// Reads a `.grid` file that must hold `width × height` samples of `element`.
    pub fn from_file(
        file: &[u8],
        element: Element,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        if file.len() < HEADER || &file[..4] != MAGIC {
            return Err("not a grid file".into());
        }
        if file[4] != FORMAT {
            return Err(format!("grid format {} is not supported", file[4]));
        }
        if file[5] != element.code() {
            return Err("wrong sample type".into());
        }
        if file[6] != CODEC_ZLIB && file[6] != CODEC_RAW {
            return Err(format!("grid codec {} is not supported", file[6]));
        }
        let read =
            |at: usize| u32::from_le_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]);
        if (read(8), read(12)) != (width, height) {
            return Err(format!(
                "{}×{} samples, expected {width}×{height}",
                read(8),
                read(12)
            ));
        }
        let size = element.size();
        let count = width as usize * height as usize * size;
        if file[6] == CODEC_RAW {
            return if file.len() == HEADER + count {
                Ok(Self(file[HEADER..].to_vec()))
            } else {
                Err("Wrong dimensions".into())
            };
        }
        let mut raw = Vec::with_capacity(count);
        flate2::read::ZlibDecoder::new(&file[HEADER..])
            .take(count as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|e| format!("corrupt grid data: {e}"))?;
        if raw.len() != count {
            return Err("Wrong dimensions".into());
        }
        if file[7] & FILTER_SHUFFLE != 0 && size > 1 {
            let samples = count / size;
            let mut out = vec![0; count];
            for plane in 0..size {
                for (i, &b) in raw[plane * samples..(plane + 1) * samples]
                    .iter()
                    .enumerate()
                {
                    out[i * size + plane] = b;
                }
            }
            raw = out;
        }
        Ok(Self(raw))
    }
}

impl Serialize for Grid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Grid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        super::authoring::decode_base64(&text)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for Grid {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Grid".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let _ = &generator;
        schemars::json_schema!({"type": "string", "contentEncoding": "base64"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_round_trip_bit_exact() {
        let floats: Vec<u8> = [0.0f32, -5.6, 12.4433, f32::MIN_POSITIVE, 1.0, 2.5]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let grid = Grid(floats);
        let file = grid.to_file(Element::F32, 3, 2);
        assert_eq!(Grid::from_file(&file, Element::F32, 3, 2).unwrap(), grid);
        assert_eq!(
            Grid::from_file(&file, Element::F32, 2, 3).unwrap_err(),
            "3×2 samples, expected 2×3"
        );
        assert_eq!(
            Grid::from_file(&file, Element::U8, 3, 2).unwrap_err(),
            "wrong sample type"
        );
        let bytes = Grid(vec![1, 2, 3, 4, 5, 6]);
        let file = bytes.to_file(Element::U8, 6, 1);
        assert_eq!(Grid::from_file(&file, Element::U8, 6, 1).unwrap(), bytes);
        assert!(Grid::from_file(b"nope", Element::U8, 6, 1).is_err());
    }

    #[test]
    fn json_is_base64() {
        let grid = Grid(vec![0, 255, 7]);
        let text = serde_json::to_string(&grid).unwrap();
        assert_eq!(text, "\"AP8H\"");
        assert_eq!(serde_json::from_str::<Grid>(&text).unwrap(), grid);
    }
}
