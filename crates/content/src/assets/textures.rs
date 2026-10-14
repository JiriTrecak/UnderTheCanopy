//! JSON outputs of texture records that the renderer imports by path: daytime colour
//! grades (`src/render/atmosphere/daytimeLuts.ts`) and the metadata of gzip-compressed
//! reflection cubes and BRDF tables (`src/render/terrain/sourceReflection.ts`). TypeScript
//! types them by inference from the JSON; these mirror those shapes. Which file is which
//! is the renderer's choice, so the loaders take the path.

use super::read;
use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Daytime colour grades: one 3D lookup table per phase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaytimeGrades {
    pub day: ColorVolume,
    #[serde(rename = "day_to_night")]
    pub day_to_night: ColorVolume,
    pub night: ColorVolume,
    #[serde(rename = "night_to_day")]
    pub night_to_day: ColorVolume,
    /// Replaces `day` in winter.
    #[serde(rename = "winter_day")]
    pub winter_day: ColorVolume,
}

/// A `size`³ RGBA8 volume, base64 encoded, red varying fastest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ColorVolume {
    pub size: u32,
    pub rgba: String,
}

/// A prefiltered cube map: `levels` mips from `size`, faces in `faceOrder`, RGBA8.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CubeMetadata {
    pub size: u32,
    pub levels: u32,
    pub decoded_bytes: u64,
    pub face_order: Vec<String>,
    pub method: String,
}

/// A 2D RGBA8 lookup table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TableMetadata {
    pub width: u32,
    pub height: u32,
    pub decoded_bytes: u64,
    pub method: String,
    pub samples: u32,
}

/// Reads daytime grades from `file` (relative to `data`); every volume decodes to
/// `size`³ RGBA texels.
pub fn load_daytime_grades(
    data: &Path,
    file: &str,
    errors: &mut Vec<Error>,
) -> Option<DaytimeGrades> {
    let grades: DaytimeGrades = read(data, file, errors)?;
    for (name, volume) in [
        ("day", &grades.day),
        ("day_to_night", &grades.day_to_night),
        ("night", &grades.night),
        ("night_to_day", &grades.night_to_day),
        ("winter_day", &grades.winter_day),
    ] {
        let texels = u64::from(volume.size).pow(3);
        if base64_len(&volume.rgba) != Some(texels * 4) {
            errors.push(Error::new(
                file,
                name,
                format!("rgba must be base64 of {texels} RGBA texels"),
            ));
        }
    }
    Some(grades)
}

/// Reads reflection cube metadata from `file`; `decodedBytes` covers every face of every mip.
pub fn load_cube_metadata(
    data: &Path,
    file: &str,
    errors: &mut Vec<Error>,
) -> Option<CubeMetadata> {
    let cube: CubeMetadata = read(data, file, errors)?;
    let expected: u64 = (0..cube.levels)
        .map(|level| u64::from(cube.size >> level).pow(2) * 6 * 4)
        .sum();
    if cube.decoded_bytes != expected {
        errors.push(Error::new(
            file,
            "decodedBytes",
            format!(
                "{} bytes, but the cube needs {expected}",
                cube.decoded_bytes
            ),
        ));
    }
    if cube.face_order.len() != 6 {
        errors.push(Error::new(file, "faceOrder", "a cube has six faces"));
    }
    Some(cube)
}

/// Reads lookup table metadata from `file`; `decodedBytes` is `width`×`height` RGBA texels.
pub fn load_table_metadata(
    data: &Path,
    file: &str,
    errors: &mut Vec<Error>,
) -> Option<TableMetadata> {
    let table: TableMetadata = read(data, file, errors)?;
    let expected = u64::from(table.width) * u64::from(table.height) * 4;
    if table.decoded_bytes != expected {
        errors.push(Error::new(
            file,
            "decodedBytes",
            format!(
                "{} bytes, but the table needs {expected}",
                table.decoded_bytes
            ),
        ));
    }
    Some(table)
}

/// Decoded length of standard padded base64, or `None` when `text` is not base64.
fn base64_len(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let padding = bytes.iter().rev().take_while(|&&b| b == b'=').count();
    let body = &bytes[..bytes.len() - padding];
    let valid = padding <= 2
        && body
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'));
    valid.then(|| (bytes.len() / 4 * 3 - padding) as u64)
}

#[cfg(test)]
mod tests {
    use super::base64_len;

    #[test]
    fn base64_lengths() {
        assert_eq!(base64_len(""), Some(0));
        assert_eq!(base64_len("AAAA"), Some(3));
        assert_eq!(base64_len("AAA="), Some(2));
        assert_eq!(base64_len("AA=="), Some(1));
        assert_eq!(base64_len("AAA"), None);
        assert_eq!(base64_len("AA*A"), None);
    }
}
