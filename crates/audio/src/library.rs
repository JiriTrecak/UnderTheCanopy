//! Sounds in memory, ready to play: the pack's 16-bit PCM converted once to the mixer's
//! stereo frames.

use canopy_assets::{EntryKind, Pack};
use canopy_content::assets::Manifest;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use std::{collections::HashMap, sync::Arc};

/// A loaded sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SoundId(u32);

impl SoundId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One loaded sound.
pub struct Clip {
    /// The pack entry name (`<asset id>/audio.wav`).
    pub name: String,
    /// Length in seconds at the original pitch.
    pub duration: f64,
    pub(crate) data: StaticSoundData,
}

/// Every sound the game can play, by pack entry name.
#[derive(Default)]
pub struct SoundLibrary {
    clips: Vec<Clip>,
    by_name: HashMap<String, SoundId>,
}

impl SoundLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every sound entry of a pack.
    pub fn from_pack(pack: &Pack) -> Result<Self, canopy_assets::Error> {
        let mut library = Self::new();
        library.load_pack(pack)?;
        Ok(library)
    }

    /// Adds every sound entry of a pack (replacing sounds of the same name); returns how
    /// many were loaded.
    pub fn load_pack(&mut self, pack: &Pack) -> Result<usize, canopy_assets::Error> {
        let _span = tracing::info_span!("load_sounds").entered();
        let mut count = 0;
        for entry in pack.entries().iter().filter(|e| e.kind == EntryKind::Sound) {
            let sound = pack.blob(entry)?.into_sound()?;
            let meta = &sound.meta;
            self.insert_pcm(
                &entry.name,
                meta.sample_rate,
                meta.channels,
                sound.samples(),
            );
            count += 1;
        }
        Ok(count)
    }

    /// Adds interleaved signed 16-bit PCM under `name`, replacing a sound of that name.
    ///
    /// Mono sounds are stored at −3 dB on both channels, so that with the mixer's
    /// equal-power panning a centred mono sound is exactly as loud as in the old game
    /// (a Web Audio stereo panner). Beyond two channels only the first two are kept.
    pub fn insert_pcm(
        &mut self,
        name: &str,
        sample_rate: u32,
        channels: u16,
        samples: &[i16],
    ) -> SoundId {
        let channels = usize::from(channels.max(1));
        let scale = 1.0 / 32768.0;
        let frames: Arc<[kira::Frame]> = samples
            .chunks_exact(channels)
            .map(|frame| {
                let left = f32::from(frame[0]) * scale;
                match frame.get(1) {
                    Some(&right) => kira::Frame::new(left, f32::from(right) * scale),
                    None => kira::Frame::from_mono(left * std::f32::consts::FRAC_1_SQRT_2),
                }
            })
            .collect();
        let sample_rate = sample_rate.max(1);
        let clip = Clip {
            name: name.to_owned(),
            duration: frames.len() as f64 / f64::from(sample_rate),
            data: StaticSoundData {
                sample_rate,
                frames,
                settings: StaticSoundSettings::default(),
                slice: None,
            },
        };
        match self.by_name.get(name) {
            Some(&id) => {
                self.clips[id.index()] = clip;
                id
            }
            None => {
                let id = SoundId(self.clips.len() as u32);
                self.clips.push(clip);
                self.by_name.insert(name.to_owned(), id);
                id
            }
        }
    }

    /// The sound of a pack entry name.
    pub fn find(&self, name: &str) -> Option<SoundId> {
        self.by_name.get(name).copied()
    }

    /// The published audio output `index` (one-based) of the asset record `asset`: the file
    /// `audio.<ext>` for index 1 and `audio_<index>.<ext>` after that, as effect sound
    /// layers reference them.
    pub fn find_published(&self, manifest: &Manifest, asset: &str, index: u32) -> Option<SoundId> {
        let prefix = match index {
            1 => "audio.".to_owned(),
            index => format!("audio_{index}."),
        };
        let record = manifest.records.iter().find(|r| r.id.as_str() == asset)?;
        record
            .outputs
            .iter()
            .filter(|o| {
                o.path
                    .rsplit('/')
                    .next()
                    .is_some_and(|f| f.starts_with(&prefix))
            })
            .find_map(|o| self.find(&o.path))
    }

    pub fn clip(&self, id: SoundId) -> &Clip {
        &self.clips[id.index()]
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Every sound with its id, in load order.
    pub fn iter(&self) -> impl Iterator<Item = (SoundId, &Clip)> {
        self.clips
            .iter()
            .enumerate()
            .map(|(i, clip)| (SoundId(i as u32), clip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_becomes_stereo_frames() {
        let mut library = SoundLibrary::new();
        let mono = library.insert_pcm("a", 10, 1, &[16384, -32768, 0, 0, 0]);
        let stereo = library.insert_pcm("b", 4, 2, &[16384, -16384, 0, 32767]);
        let clip = library.clip(mono);
        assert_eq!(clip.duration, 0.5);
        let half = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((clip.data.frames[0].left - half).abs() < 1e-6);
        assert_eq!(clip.data.frames[0].left, clip.data.frames[0].right);
        let clip = library.clip(stereo);
        assert_eq!(clip.data.frames.len(), 2);
        assert_eq!(clip.data.frames[0], kira::Frame::new(0.5, -0.5));
        // Replacing by name keeps the id.
        assert_eq!(library.insert_pcm("a", 10, 1, &[0; 20]), mono);
        assert_eq!(library.clip(mono).duration, 2.0);
        assert_eq!(library.find("b"), Some(stereo));
        assert_eq!(library.len(), 2);
    }
}
