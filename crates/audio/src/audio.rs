//! The game's audio: one value the main thread owns and drives once per frame.

use crate::{
    Category, EffectSounds, SoundId, SoundLibrary,
    effects::TICK_SECONDS,
    limits::{Limits, Request, Selector},
    music::{Music, Playlist},
    output::Output,
    settings::{AudioSettings, Bus},
    sources::{Clocks, Handle, PlayOptions, Sources},
    spatial::{At, Listener},
};
use canopy_assets::Pack;
use canopy_content::Content;
use canopy_sim::{abilities::AbilityEvent, view::View};

/// What the frame looks like to the ears.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Wall-clock seconds since the previous frame.
    pub real_dt: f32,
    /// The renderer's fractional simulation tick (game time).
    pub render_tick: f64,
    /// Game speed: 1 normally, 0 while paused (game sounds stop), above 1 when fast.
    pub speed: f32,
    pub listener: Listener,
}

/// Voices and sources, for the debug overlay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Requests that got a voice, per [`Category`].
    pub voices: [u16; 4],
    /// Requests that wanted a voice and got none.
    pub virtual_voices: u16,
    /// Voices kira is playing.
    pub playing: u16,
    pub sources: u16,
    pub effect_cues: u16,
}

/// Every sound of the game.
///
/// ```ignore
/// // Loading:
/// let mut audio = Audio::new(settings.audio);
/// audio.load(&pack)?;
/// audio.bind(&content);
/// // Each new simulation view (once per tick):
/// audio.observe(&view);
/// // Each rendered frame:
/// audio.update(&Frame { real_dt, render_tick, speed, listener });
/// ```
pub struct Audio {
    output: Option<Output>,
    library: SoundLibrary,
    settings: AudioSettings,
    limits: Limits,
    background: bool,
    clocks: Clocks,
    sources: Sources,
    effects: EffectSounds,
    music: Music,
    keys: u64,
    requests: Vec<Request>,
    granted: Vec<usize>,
    selector: Selector,
    stats: Stats,
}

impl Audio {
    /// Audio on the default output device. Without one (or if it cannot be opened)
    /// everything still runs and nothing is heard; the reason is logged.
    pub fn new(settings: AudioSettings) -> Self {
        let limits = Limits::default();
        let output = match Output::device(&limits) {
            Ok(output) => Some(output),
            Err(e) => {
                tracing::warn!("audio: no output device ({e}); the game runs silent");
                None
            }
        };
        Self::with_output(output, settings, limits)
    }

    /// The whole mixer without an audio device (tests, CI, tools).
    pub fn headless(settings: AudioSettings) -> Self {
        let limits = Limits::default();
        let output = Output::headless(&limits).ok();
        Self::with_output(output, settings, limits)
    }

    /// No output at all.
    pub fn silent(settings: AudioSettings) -> Self {
        Self::with_output(None, settings, Limits::default())
    }

    fn with_output(output: Option<Output>, settings: AudioSettings, limits: Limits) -> Self {
        Self {
            output,
            library: SoundLibrary::new(),
            settings,
            limits,
            background: false,
            clocks: Clocks {
                game: 0.0,
                real: 0.0,
                speed: 1.0,
            },
            sources: Sources::default(),
            effects: EffectSounds::new(),
            music: Music::default(),
            keys: 0,
            requests: Vec::new(),
            granted: Vec::new(),
            selector: Selector::default(),
            stats: Stats::default(),
        }
    }

    /// Whether sound reaches a device (or the mock backend, when headless).
    pub fn has_output(&self) -> bool {
        self.output.is_some()
    }

    /// Runs the mixer for `blocks` blocks of 128 frames, as the audio thread would, when
    /// built with [`Audio::headless`] (tests); does nothing otherwise.
    pub fn render_headless(&mut self, blocks: usize) {
        if let Some(output) = &mut self.output {
            output.render_headless(blocks);
        }
    }

    /// Loads every sound of the pack; returns how many.
    pub fn load(&mut self, pack: &Pack) -> Result<usize, canopy_assets::Error> {
        self.library.load_pack(pack)
    }

    pub fn library(&self) -> &SoundLibrary {
        &self.library
    }

    pub fn library_mut(&mut self) -> &mut SoundLibrary {
        &mut self.library
    }

    /// A sound by pack entry name (`assets/library/<id>/audio.wav`).
    pub fn sound(&self, name: &str) -> Option<SoundId> {
        self.library.find(name)
    }

    /// Binds the effect sound layers of `content` to ability events. Call after
    /// [`Audio::load`]; ends any effect cues.
    pub fn bind(&mut self, content: &Content) {
        self.effects.bind(content, &self.library);
    }

    pub fn effects(&self) -> &EffectSounds {
        &self.effects
    }

    /// The effect sounds, for feeding triggers directly.
    pub fn effects_mut(&mut self) -> &mut EffectSounds {
        &mut self.effects
    }

    /// Follows a new view of the simulation (see [`EffectSounds::observe`]).
    pub fn observe(&mut self, view: &View) {
        self.effects.observe(view);
    }

    /// Plays the effects bound to one ability event.
    pub fn ability_event(&mut self, event: &AbilityEvent) {
        self.effects.ability_event(event);
    }

    /// Ends every game sound: effect cues and world sounds (a new match, a loaded game).
    /// Interface sounds and music go on.
    pub fn clear_game(&mut self) {
        self.effects.clear();
        self.sources.clear_game();
    }

    /// Plays a sound at a place with the defaults of a world sound (see [`PlayOptions`]).
    pub fn play(&mut self, sound: SoundId, at: At) -> Option<Handle> {
        self.play_with(sound, at, PlayOptions::default())
    }

    /// Plays an interface sound.
    pub fn play_interface(&mut self, sound: SoundId) -> Option<Handle> {
        self.play_with(
            sound,
            At::Listener,
            PlayOptions::category(Category::Interface),
        )
    }

    /// Plays a sound. Returns `None` when it is dropped: the same one-shot started a
    /// moment ago in its category ([`crate::Budget::retrigger`]), or too many sources.
    /// One-shots end by themselves; loops play until [`Audio::stop`].
    pub fn play_with(&mut self, sound: SoundId, at: At, options: PlayOptions) -> Option<Handle> {
        if sound.index() >= self.library.len() {
            return None;
        }
        self.keys += 1;
        self.sources
            .play(self.keys, sound, at, options, &self.clocks, &self.limits)
    }

    /// Stops a sound, fading over `fade` seconds (`None`: the sound's own fade-out).
    pub fn stop(&mut self, handle: Handle, fade: Option<f32>) {
        self.sources.stop(handle, fade, &self.clocks);
    }

    /// Moves a sound (a loop on a moving unit).
    pub fn set_at(&mut self, handle: Handle, at: At) {
        self.sources.set_at(handle, at);
    }

    /// Whether a sound still plays (or waits for a voice).
    pub fn is_live(&self, handle: Handle) -> bool {
        self.sources.is_live(handle)
    }

    /// Starts a playlist, crossfading from the music playing now.
    pub fn play_music(&mut self, playlist: Playlist) {
        self.music.play(playlist, self.clocks.real, &mut self.keys);
    }

    /// The track playing (or fading in) now.
    pub fn current_music(&self) -> Option<SoundId> {
        self.music.current()
    }

    pub fn stop_music(&mut self, fade: f32) {
        self.music.stop(self.clocks.real, fade);
    }

    pub fn settings(&self) -> &AudioSettings {
        &self.settings
    }

    /// New volumes take effect within a few milliseconds.
    pub fn apply_settings(&mut self, settings: AudioSettings) {
        self.settings = settings;
    }

    /// Whether the window is hidden or minimised; with
    /// [`AudioSettings::mute_in_background`] nothing plays meanwhile.
    pub fn set_background(&mut self, background: bool) {
        self.background = background;
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Changes voice budgets (larger budgets need a restart to raise mixer capacity).
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Advances every sound to this frame and updates the voices. Main thread, once per
    /// frame.
    pub fn update(&mut self, frame: &Frame) {
        let _span = tracing::trace_span!("audio_update").entered();
        self.clocks.real += f64::from(frame.real_dt.max(0.0));
        self.clocks.game = frame.render_tick * TICK_SECONDS;
        self.clocks.speed = f64::from(frame.speed.max(0.0));
        self.requests.clear();
        self.effects.requests(
            frame.render_tick,
            self.clocks.speed,
            &frame.listener,
            &self.library,
            &mut self.requests,
        );
        self.sources.requests(
            &self.clocks,
            &frame.listener,
            &self.library,
            &mut self.requests,
        );
        self.music.requests(
            self.clocks.real,
            &self.library,
            &mut self.keys,
            &mut self.requests,
        );

        let master = if self.background && self.settings.mute_in_background {
            0.0
        } else {
            self.settings.master()
        };
        let buses = Bus::ALL.map(|bus| self.settings.bus(bus));
        let audible = |category: Category| master * buses[category.bus().index()] > 0.0;
        let output = &self.output;
        let voices = self.selector.select(
            &self.requests,
            &self.limits,
            audible,
            |key| output.as_ref().is_some_and(|o| o.is_playing(key)),
            &mut self.granted,
        );
        if let Some(output) = &mut self.output {
            output.set_mix(master, buses);
            output.reconcile(
                self.granted.iter().map(|&i| &self.requests[i]),
                &self.library,
            );
        }
        let audible_requests = self
            .requests
            .iter()
            .filter(|r| audible(r.category) && r.gain >= crate::limits::AUDIBLE)
            .count();
        self.stats = Stats {
            voices,
            virtual_voices: audible_requests.saturating_sub(self.granted.len()) as u16,
            playing: self.output.as_ref().map_or(0, |o| o.playing() as u16),
            sources: self.sources.len() as u16,
            effect_cues: self.effects.cue_count() as u16,
        };
    }
}
