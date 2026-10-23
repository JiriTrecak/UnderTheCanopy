//! The player's audio settings: one volume per bus.

use serde::{Deserialize, Serialize};

/// A mixer bus: every sound plays on one, and each has its own volume setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bus {
    /// Sounds of the world: ability effects and game sounds.
    Effects,
    /// Menus, buttons and notifications.
    Interface,
    Music,
    /// Units answering and spoken story.
    Voice,
}

impl Bus {
    pub const ALL: [Bus; 4] = [Bus::Effects, Bus::Interface, Bus::Music, Bus::Voice];

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// Volumes are linear amplitudes from 0 to 1, multiplied together (master × bus × sound),
/// as the old game's effect volume was.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    pub master: f32,
    /// The old game's "Spell effect volume" (default 0.65).
    pub effects: f32,
    pub interface: f32,
    pub music: f32,
    /// Units answering and dialogue.
    pub voice: f32,
    /// Silence everything while the window is hidden or minimised, as the old game did.
    pub mute_in_background: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master: 1.0,
            effects: 0.65,
            interface: 0.7,
            music: 0.5,
            voice: 0.8,
            mute_in_background: true,
        }
    }
}

impl AudioSettings {
    /// The volume of a bus before master, clamped to 0–1 (non-finite values count as 0).
    pub fn bus(&self, bus: Bus) -> f32 {
        clamp_volume(match bus {
            Bus::Effects => self.effects,
            Bus::Interface => self.interface,
            Bus::Music => self.music,
            Bus::Voice => self.voice,
        })
    }

    pub fn set_bus(&mut self, bus: Bus, volume: f32) {
        let volume = clamp_volume(volume);
        match bus {
            Bus::Effects => self.effects = volume,
            Bus::Interface => self.interface = volume,
            Bus::Music => self.music = volume,
            Bus::Voice => self.voice = volume,
        }
    }

    pub fn master(&self) -> f32 {
        clamp_volume(self.master)
    }
}

fn clamp_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults_and_values_are_clamped() {
        let settings: AudioSettings = serde_json::from_str(r#"{"effects": 0.3}"#).unwrap();
        assert_eq!(settings.effects, 0.3);
        assert_eq!(settings.master, 1.0);
        assert!(settings.mute_in_background);
        let mut settings = AudioSettings {
            master: 4.0,
            music: f32::NAN,
            ..settings
        };
        assert_eq!(settings.master(), 1.0);
        assert_eq!(settings.bus(Bus::Music), 0.0);
        settings.set_bus(Bus::Interface, -1.0);
        assert_eq!(settings.bus(Bus::Interface), 0.0);
    }
}
