//! Audio: playback, positional world sounds, voice limiting, buses, music and the sound
//! layers of ability effects. Design and decisions: `docs/audio.md`.
//!
//! The main thread owns an [`Audio`] and drives it once per frame:
//!
//! 1. Sources say what they want to be heard: effect cues ([`EffectSounds`], fed by
//!    simulation views), sounds the game plays ([`Audio::play`]), interface sounds and
//!    music ([`Playlist`]). Each request carries its offset in the sound, rate, gain
//!    (volume, fades, distance attenuation) and pan, computed from the listener
//!    ([`Listener`]: the camera's focus) with the old game's model ([`spatial`]).
//! 2. The selector ([`limits`]) gives voices to the most audible requests per category
//!    within budgets; the rest stay virtual and keep their timeline.
//! 3. The output reconciles kira's playing sounds with the granted requests. kira
//!    mixes on its own thread without allocating; buses are kira tracks with the
//!    player's volumes ([`AudioSettings`]).
//!
//! Nothing here needs an audio device: [`Audio::headless`] runs the whole pipeline on
//! kira's mock backend, and without a device [`Audio::new`] runs silent.

mod audio;
pub mod effects;
mod library;
pub mod limits;
mod music;
mod observe;
mod output;
mod settings;
mod sources;
pub mod spatial;

pub use audio::{Audio, Frame, Stats};
pub use effects::{DeliverySighting, EffectSounds, ReturnSighting, StatusSighting, Trigger};
pub use library::{Clip, SoundId, SoundLibrary};
pub use limits::{Budget, Category, Limits};
pub use music::Playlist;
pub use observe::{effect_event, trigger};
pub use settings::{AudioSettings, Bus};
pub use sources::{Handle, PlayOptions};
pub use spatial::{At, Distances, Listener};
