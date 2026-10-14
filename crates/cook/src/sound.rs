//! WAV to 16-bit PCM.
//!
//! Sounds stay uncompressed: today's two effect sounds are 82 KB in total, so decoding a
//! compressed format at load would cost more than it saves. Revisit when music or larger
//! banks arrive (M4).

use canopy_assets::{BlobBuilder, EntryKind, SoundMeta};
use std::io::Cursor;

pub fn cook(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let samples: Vec<i16> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        (hound::SampleFormat::Int, bits @ (8 | 24 | 32)) => reader
            .samples::<i32>()
            .map(|s| {
                s.map(|s| {
                    if bits > 16 {
                        (s >> (bits - 16)) as i16
                    } else {
                        (s << (16 - bits)) as i16
                    }
                })
            })
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .map(|s| s.map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16))
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?,
        (format, bits) => {
            return Err(format!(
                "unsupported WAV sample format {format:?} {bits}-bit"
            ));
        }
    };
    let channels = spec.channels.max(1);
    let mut builder = BlobBuilder::new();
    let meta = SoundMeta {
        sample_rate: spec.sample_rate,
        channels,
        frames: (samples.len() / usize::from(channels)) as u32,
        samples: builder.push(&samples),
    };
    Ok(builder.finish(EntryKind::Sound, &meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_assets::BlobHeader;

    #[test]
    fn pcm_survives_unchanged() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut wav, spec).unwrap();
        let samples: Vec<i16> = (0..64).map(|i| (i * 997 % 65536 - 32768) as i16).collect();
        for &s in &samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        let blob = cook(wav.get_ref()).unwrap();
        let header: BlobHeader = bytemuck::pod_read_unaligned(&blob[..16]);
        let meta: SoundMeta =
            postcard::from_bytes(&blob[16..16 + header.meta_len as usize]).unwrap();
        assert_eq!(
            (meta.sample_rate, meta.channels, meta.frames),
            (22_050, 2, 32)
        );
        let data = &blob[header.data_offset as usize..];
        let stored: &[i16] = bytemuck::cast_slice(&data[..meta.samples.len as usize]);
        assert_eq!(stored, samples.as_slice());
    }
}
