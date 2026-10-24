//! The player's settings: graphics, audio, HUD, gameplay, keyboard shortcuts, the multiplayer
//! server and the player name, kept as `settings.json` in the user folder (D6) and
//! applied as they change.
//!
//! The file is versioned and lenient: missing fields take their defaults, so new
//! settings never invalidate an old file; a file that cannot be read is moved aside
//! (`settings.json.bad`) and the defaults are used. Writes go through a temporary file
//! and a rename, so a crash never leaves half a file.

pub mod keys;

use anyhow::{Context, Result};
use canopy_audio::AudioSettings;
use canopy_render::{HazeQuality, Preset, Quality};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use keys::{Chord, KeyBindings};

/// The settings file's format version.
pub const VERSION: u32 = 1;
/// File name in the user folder.
pub const FILE: &str = "settings.json";
/// Longest player name (old `maxlength`).
pub const MAX_NAME: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Haze {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

impl Haze {
    pub const ALL: [Haze; 4] = [Haze::Off, Haze::Low, Haze::Medium, Haze::High];

    pub fn quality(self) -> HazeQuality {
        match self {
            Self::Off => HazeQuality::Off,
            Self::Low => HazeQuality::Low,
            Self::Medium => HazeQuality::Medium,
            Self::High => HazeQuality::High,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

impl Haze {
    fn of(quality: HazeQuality) -> Self {
        match quality {
            HazeQuality::Off => Self::Off,
            HazeQuality::Low => Self::Low,
            HazeQuality::Medium => Self::Medium,
            HazeQuality::High => Self::High,
        }
    }
}

/// The graphics preset (D42). `Auto` takes the adapter's preset at every start, so a
/// settings file moved to another machine still fits; `Custom` uses the individual
/// settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum GraphicsPreset {
    #[default]
    Auto,
    Low,
    Medium,
    High,
    Ultra,
    Custom,
}

impl GraphicsPreset {
    pub const ALL: [GraphicsPreset; 6] = [
        Self::Auto,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Ultra,
        Self::Custom,
    ];

    fn fixed(self) -> Option<Preset> {
        match self {
            Self::Low => Some(Preset::Low),
            Self::Medium => Some(Preset::Medium),
            Self::High => Some(Preset::High),
            Self::Ultra => Some(Preset::Ultra),
            Self::Auto | Self::Custom => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WindowMode {
    #[default]
    Windowed,
    /// A borderless window covering the monitor at the desktop's own resolution.
    #[serde(alias = "fullscreen")]
    Borderless,
    /// The game takes the monitor over in its own video mode (the desktop's size at its
    /// highest refresh rate).
    Exclusive,
}

impl WindowMode {
    pub const ALL: [Self; 3] = [Self::Windowed, Self::Borderless, Self::Exclusive];

    /// The settings screen's text key.
    pub fn label(self) -> &'static str {
        match self {
            Self::Windowed => "settings.windowed",
            Self::Borderless => "settings.borderless",
            Self::Exclusive => "settings.exclusive",
        }
    }
}

/// The old "Game resolution" choices: the scene renders at most this size (the
/// interface stays at native resolution).
pub const RESOLUTIONS: [[u32; 2]; 8] = [
    [1280, 720],
    [1280, 800],
    [1600, 900],
    [1920, 1080],
    [1920, 1200],
    [2560, 1440],
    [2560, 1600],
    [3840, 2160],
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Graphics {
    /// The preset; the individual settings below mirror it unless it is `Custom`.
    pub preset: GraphicsPreset,
    /// The adapter's preset, found at start (`Graphics::detect`); `Auto` uses it.
    #[serde(skip)]
    pub detected: Option<Preset>,
    /// Internal resolution over output resolution, 0.5–1 (D34).
    pub render_scale: f32,
    /// A cap on the scene's resolution (`None`: native).
    pub resolution: Option<[u32; 2]>,
    pub shadows: bool,
    pub contact_shadows: bool,
    pub bloom: bool,
    pub haze: Haze,
    pub vsync: bool,
    pub window: WindowMode,
}

impl Default for Graphics {
    fn default() -> Self {
        let q = Quality::default();
        Self {
            preset: GraphicsPreset::Auto,
            detected: None,
            render_scale: q.render_scale,
            resolution: None,
            shadows: true,
            contact_shadows: q.contact_shadows,
            bloom: q.bloom,
            haze: Haze::Medium,
            vsync: true,
            window: WindowMode::Windowed,
        }
    }
}

impl Graphics {
    /// Notes the adapter's preset (`Gpu::preset`) for `Auto` and mirrors the active
    /// preset into the individual settings.
    pub fn detect(&mut self, adapter: Preset) {
        self.detected = Some(adapter);
        self.mirror();
    }

    /// The preset in effect: `None` for custom settings. `Auto` before detection is High.
    pub fn active_preset(&self) -> Option<Preset> {
        match self.preset {
            GraphicsPreset::Auto => Some(self.detected.unwrap_or(Preset::High)),
            GraphicsPreset::Custom => None,
            fixed => fixed.fixed(),
        }
    }

    /// Chooses a preset; its values show in the individual settings.
    pub fn set_preset(&mut self, preset: GraphicsPreset) {
        self.preset = preset;
        self.mirror();
    }

    /// The individual settings take the active preset's values (a custom change then
    /// starts from them).
    fn mirror(&mut self) {
        if let Some(preset) = self.active_preset() {
            let q = preset.quality();
            self.render_scale = q.render_scale;
            self.contact_shadows = q.contact_shadows;
            self.bloom = q.bloom;
            self.haze = Haze::of(q.haze);
        }
    }

    /// The renderer's quality for a window of `size` physical pixels: the preset's (or
    /// the custom settings), with the render scale lowered further when a resolution
    /// cap is smaller than the window.
    pub fn quality(&self, size: [u32; 2]) -> Quality {
        if let Some(preset) = self.active_preset() {
            let mut q = preset.quality();
            if let Some([w, h]) = self.resolution {
                let cap = (w as f32 / size[0].max(1) as f32).min(h as f32 / size[1].max(1) as f32);
                q.render_scale =
                    (q.render_scale * cap.min(1.0)).max(canopy_render::quality::MIN_RENDER_SCALE);
            }
            return q;
        }
        let min = canopy_render::quality::MIN_RENDER_SCALE;
        let mut scale = if self.render_scale.is_finite() {
            self.render_scale.clamp(min, 1.0)
        } else {
            1.0
        };
        if let Some([w, h]) = self.resolution {
            let cap = (w as f32 / size[0].max(1) as f32).min(h as f32 / size[1].max(1) as f32);
            scale = (scale * cap.min(1.0)).max(min);
        }
        Quality {
            render_scale: scale,
            contact_shadows: self.contact_shadows,
            bloom: self.bloom,
            haze: self.haze.quality(),
            ..Quality::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hud {
    /// HUD size in percent, 30–100 (old default 65).
    pub scale: u32,
    /// Spread along the screen's width, or compact (at most 1240 px wide, centred).
    pub layout: HudLayout,
    /// Health bars over every unit, not only hovered, selected and damaged ones.
    pub always_show_health_bars: bool,
    pub tooltips: bool,
}

impl Default for Hud {
    fn default() -> Self {
        Self {
            scale: 65,
            layout: HudLayout::Spread,
            always_show_health_bars: false,
            tooltips: true,
        }
    }
}

/// The old "HUD layout" setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum HudLayout {
    /// Screen edges: the dock spans the window (old default).
    #[default]
    Spread,
    /// Centred: the dock is at most [`COMPACT_HUD_WIDTH`] logical pixels wide.
    Compact,
}

/// The compact dock's widest (old `min(100vw - 12px, 1240px)`).
pub const COMPACT_HUD_WIDTH: f32 = 1240.0;

impl Hud {
    pub fn scale(&self) -> f32 {
        self.scale.clamp(30, 100) as f32 / 100.0
    }

    /// The dock's scale in a window `width` logical pixels wide: the HUD size, and in
    /// the compact layout the narrower dock (the dock keeps its 3:1 shape and scales
    /// about its bottom centre, so a narrower dock is the same dock scaled down).
    pub fn dock_scale(&self, width: f32) -> f32 {
        let full = (width - 12.0).max(1.0);
        match self.layout {
            HudLayout::Spread => self.scale(),
            HudLayout::Compact => self.scale() * (COMPACT_HUD_WIDTH / full).min(1.0),
        }
    }
}

/// How the match shows what happens in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Gameplay {
    /// Goods arriving in the player's stock rise as "+N" where they arrive: a worker's
    /// load at the hall, a well's Substance.
    pub delivery_text: bool,
}

impl Default for Gameplay {
    fn default() -> Self {
        Self {
            delivery_text: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Multiplayer {
    /// Overrides the deployed match server (`CANOPY_MATCH_SERVER` overrides this).
    pub server: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub player_name: String,
    pub graphics: Graphics,
    pub audio: AudioSettings,
    pub hud: Hud,
    pub gameplay: Gameplay,
    pub keys: KeyBindings,
    pub multiplayer: Multiplayer,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            player_name: String::new(),
            graphics: Graphics::default(),
            audio: AudioSettings::default(),
            hud: Hud::default(),
            gameplay: Gameplay::default(),
            keys: KeyBindings::default(),
            multiplayer: Multiplayer::default(),
        }
    }
}

/// What reading the file found besides the settings.
#[derive(Debug, Default)]
pub struct LoadNote {
    /// The file existed but could not be read; it was moved here.
    pub moved_aside: Option<PathBuf>,
    pub error: Option<String>,
}

impl Settings {
    pub fn path(user: &Path) -> PathBuf {
        user.join(FILE)
    }

    /// The name shown to others: trimmed, at most [`MAX_NAME`] characters, `player` when
    /// empty (old `settlers.guest`).
    pub fn display_name(&self) -> String {
        let name: String = self.player_name.trim().chars().take(MAX_NAME).collect();
        if name.is_empty() {
            "player".into()
        } else {
            name
        }
    }

    /// Reads the settings, or the defaults when there is no file. A broken file is moved
    /// aside and reported, never fatal.
    pub fn load(user: &Path) -> (Self, LoadNote) {
        let path = Self::path(user);
        let mut note = LoadNote::default();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), note),
            Err(e) => {
                note.error = Some(format!("{}: {e}", path.display()));
                return (Self::default(), note);
            }
        };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(mut settings) => {
                settings.version = VERSION;
                settings.keys.prune(&keys::catalog());
                (settings, note)
            }
            Err(e) => {
                let aside = path.with_extension("json.bad");
                let _ = std::fs::rename(&path, &aside);
                note.moved_aside = Some(aside);
                note.error = Some(format!("{}: {e}", path.display()));
                (Self::default(), note)
            }
        }
    }

    /// Writes the settings atomically.
    pub fn save(&self, user: &Path) -> Result<()> {
        let path = Self::path(user);
        let text = serde_json::to_string_pretty(self)?;
        write_atomic(&path, text.as_bytes())
    }
}

/// Writes `bytes` to `path` through a temporary file in the same folder and a rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("a file in a folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let temp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&temp, bytes).with_context(|| format!("cannot write {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("cannot replace {}", path.display()))
}

#[cfg(test)]
pub(crate) fn temp_folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "canopy-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_the_user_folder() {
        let user = temp_folder("settings");
        let (fresh, note) = Settings::load(&user);
        assert_eq!(fresh, Settings::default());
        assert!(note.error.is_none());
        let catalog = keys::catalog();
        let mut settings = Settings {
            player_name: "  Jiří of the Canopy ".into(),
            ..Settings::default()
        };
        settings.graphics.render_scale = 0.75;
        settings.graphics.resolution = Some([1920, 1080]);
        settings.graphics.haze = Haze::High;
        settings.graphics.window = WindowMode::Exclusive;
        settings.audio.music = 0.2;
        settings.hud.scale = 80;
        settings.multiplayer.server = Some("http://127.0.0.1:8787".into());
        settings
            .keys
            .bind(&catalog, "interface.gameMenu", Chord::key("F9"))
            .unwrap();
        settings.save(&user).unwrap();
        let (back, note) = Settings::load(&user);
        assert!(note.error.is_none());
        assert_eq!(back, settings);
        assert_eq!(back.display_name(), "Jiří of the Canopy");
        assert_eq!(Settings::default().display_name(), "player");
    }

    #[test]
    fn missing_fields_take_defaults_and_broken_files_are_moved_aside() {
        let user = temp_folder("settings-lenient");
        std::fs::write(Settings::path(&user), r#"{"hud": {"scale": 40}}"#).unwrap();
        let (settings, _) = Settings::load(&user);
        assert_eq!(settings.hud.scale, 40);
        assert!(settings.hud.tooltips);
        assert_eq!(settings.graphics, Graphics::default());
        std::fs::write(Settings::path(&user), "{ not json").unwrap();
        let (settings, note) = Settings::load(&user);
        assert_eq!(settings, Settings::default());
        assert!(note.error.is_some());
        assert!(note.moved_aside.unwrap().is_file());
        assert!(!Settings::path(&user).exists());
    }

    #[test]
    fn the_compact_hud_is_at_most_1240_pixels_wide() {
        let mut hud = Hud::default();
        assert_eq!(hud.dock_scale(2560.0), 0.65);
        hud.layout = HudLayout::Compact;
        assert!((hud.dock_scale(2492.0) - 0.65 * 0.5).abs() < 1e-6);
        // Narrow windows keep the spread width.
        assert_eq!(hud.dock_scale(1000.0), 0.65);
    }

    #[test]
    fn presets_set_the_quality_until_a_custom_change() {
        let mut g = Graphics::default();
        g.detect(Preset::Medium);
        assert_eq!(g.active_preset(), Some(Preset::Medium));
        assert_eq!(g.quality([1920, 1080]), Preset::Medium.quality());
        // The individual settings mirror the preset.
        assert!(!g.contact_shadows);
        assert_eq!(g.haze, Haze::Low);
        g.set_preset(GraphicsPreset::Ultra);
        assert_eq!(g.quality([1920, 1080]).haze, HazeQuality::High);
        g.set_preset(GraphicsPreset::Custom);
        g.bloom = false;
        assert!(!g.quality([1920, 1080]).bloom);
        assert_eq!(g.active_preset(), None);
    }

    #[test]
    fn a_resolution_cap_lowers_the_render_scale() {
        let mut g = Graphics {
            preset: GraphicsPreset::Custom,
            ..Graphics::default()
        };
        assert_eq!(g.quality([2560, 1440]).render_scale, 1.0);
        g.resolution = Some([1920, 1080]);
        assert_eq!(g.quality([2560, 1440]).render_scale, 0.75);
        assert_eq!(g.quality([1280, 720]).render_scale, 1.0);
        g.render_scale = 0.6;
        assert_eq!(g.quality([3840, 2160]).render_scale, 0.5);
    }
}
