//! The match's own sounds (D97): what the player's units answer, the race's spoken
//! alerts, the music, the biome's ambience, a mission's spoken lines and the end's sting.
//! Units' and buildings' own sounds come with the scene ([`canopy_scene`]); everything is
//! named by content, nothing here names a sound.

use super::{Answer, Cue, know::Knowledge};
use canopy_audio::{At, Audio, LoopGroup};
use canopy_content::Content;
use canopy_render::environment::Ambience;

/// Hours the biome sounds of its day: from dawn to dusk.
const DAY: std::ops::Range<f32> = 6.0..20.0;

pub struct MatchSounds {
    /// The local player's race (the default race for observers).
    race: String,
    /// The map's own music, over the race's.
    music: Vec<String>,
    ambience: Option<Ambience>,
    /// The spoken line playing, by its dialogue id.
    line: Option<u32>,
    ended: bool,
}

impl MatchSounds {
    pub fn new(
        content: &Content,
        setup: &canopy_sim::MatchSetup,
        me: Option<u8>,
        music: Vec<String>,
        ambience: Option<Ambience>,
    ) -> Self {
        let race = me
            .and_then(|me| setup.slots.iter().find(|slot| slot.player == me))
            .and_then(|slot| slot.race.clone())
            .unwrap_or_else(|| content.rules.default_race.clone());
        Self {
            race,
            music,
            ambience,
            line: None,
            ended: false,
        }
    }

    /// The match begins: its music (the map's, else the race's).
    pub fn begin(&self, audio: &mut Audio, content: &Content) {
        let race = content
            .rules
            .races
            .get(&self.race)
            .and_then(|r| r.sounds.as_ref());
        let music: Vec<&str> = if self.music.is_empty() {
            race.map(|s| s.music.iter().map(|m| m.as_str()).collect())
                .unwrap_or_default()
        } else {
            self.music.iter().map(String::as_str).collect()
        };
        if let Some(playlist) = audio.playlist(music) {
            audio.play_music(playlist);
        }
    }

    /// The biome's air at `hour`, held as one loop.
    pub fn ambience(&self, audio: &mut Audio, hour: f32) {
        let wanted = self.ambience.as_ref().and_then(|a| {
            let day = DAY.contains(&hour);
            let id = if day { &a.day } else { &a.night };
            id.as_ref().or(a.day.as_ref()).or(a.night.as_ref())
        });
        let event = wanted.and_then(|id| audio.event(id));
        audio.hold(
            LoopGroup::Ambience,
            event.map(|e| (u64::from(day_key(hour)), e, At::Listener)),
        );
    }

    /// Plays what the player asked for: units answering, messages a race voices.
    pub fn cues(&self, audio: &mut Audio, content: &Content, know: &Knowledge, cues: Vec<Cue>) {
        for cue in cues {
            let id = match &cue {
                Cue::Answer { unit, answer } => know
                    .def_of(*unit)
                    .and_then(|d| d.presentation.as_ref()?.voice.as_ref())
                    .and_then(|v| match answer {
                        Answer::Select => v.select.as_ref(),
                        Answer::Order => v.order.as_ref().or(v.select.as_ref()),
                        Answer::Attack => v.attack.as_ref().or(v.order.as_ref()),
                        Answer::Annoyed => v.annoyed.as_ref().or(v.select.as_ref()),
                        Answer::Ready => v.ready.as_ref(),
                    })
                    .map(|id| id.to_string()),
                Cue::Alert(key) => content
                    .rules
                    .races
                    .get(&self.race)
                    .and_then(|r| r.sounds.as_ref()?.alerts.get(key))
                    .map(|id| id.to_string()),
            };
            if let Some(event) = id.and_then(|id| audio.event(&id)) {
                audio.play_event(event, At::Listener);
            }
        }
    }

    /// A mission's line: its voice (the sound event `sound.<line key>`) as it starts.
    pub fn dialogue(&mut self, audio: &mut Audio, line: Option<(u32, &str)>) {
        let Some((id, key)) = line else {
            self.line = None;
            return;
        };
        if self.line == Some(id) {
            return;
        }
        self.line = Some(id);
        if let Some(event) = audio.event(&format!("sound.{key}")) {
            audio.play_event(event, At::Listener);
        }
    }

    /// The match is over: the music gives way to the sting of the player's end.
    pub fn end(&mut self, audio: &mut Audio, content: &Content, won: bool) {
        if self.ended {
            return;
        }
        self.ended = true;
        let rules = &content.rules.sounds;
        let sting = if won { &rules.victory } else { &rules.defeat };
        if let Some(event) = sting.as_ref().and_then(|id| audio.event(id.as_str())) {
            audio.stop_music(1.0);
            audio.play_event(event, At::Listener);
        }
    }
}

/// The ambience loop's key: day and night are different loops, so the change crossfades.
fn day_key(hour: f32) -> u8 {
    u8::from(DAY.contains(&hour))
}

/// Plays what the scene asked units and buildings to sound like: one tick's one-shots, or
/// (`loops`) the loops of the newest tick, held as one group.
pub fn bodies(audio: &mut Audio, cues: &[canopy_scene::sounds::SoundCue], loops: bool) {
    if loops {
        let held: Vec<(u64, canopy_audio::EventId, At)> = cues
            .iter()
            .filter_map(|c| {
                let key = c.hold?;
                Some((key, audio.event(&c.sound)?, At::ground(c.at[0], c.at[1])))
            })
            .collect();
        audio.hold(LoopGroup::Bodies, held);
        return;
    }
    for cue in cues.iter().filter(|c| c.hold.is_none()) {
        if let Some(event) = audio.event(&cue.sound) {
            audio.play_event(event, At::ground(cue.at[0], cue.at[1]));
        }
    }
}
