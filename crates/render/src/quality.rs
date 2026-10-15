//! Graphics quality settings the player controls: the render scale and which post
//! effects run. The biome decides how an effect looks; these decide whether it runs and
//! at what cost.

/// Volumetric haze quality (old `AtmosphereQuality`): the march's resolution scale,
/// samples per ray and a pixel cap that bounds Retina and large windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HazeQuality {
    Off,
    Low,
    #[default]
    Medium,
    High,
}

impl HazeQuality {
    /// Resolution scale, steps and the most pixels the march may cover.
    pub fn spec(self) -> Option<(f32, u32, u32)> {
        match self {
            Self::Off => None,
            Self::Low => Some((0.33, 16, 180_000)),
            Self::Medium => Some((0.5, 24, 360_000)),
            Self::High => Some((0.66, 40, 900_000)),
        }
    }

    /// The march's size for a scene of `size`.
    pub fn size(self, size: [u32; 2]) -> Option<[u32; 2]> {
        let (scale, _, max_pixels) = self.spec()?;
        let pixels = (size[0] as f32 * size[1] as f32).max(1.0);
        let s = scale.min((max_pixels as f32 / pixels).sqrt());
        Some([
            ((size[0] as f32 * s).floor() as u32).max(1),
            ((size[1] as f32 * s).floor() as u32).max(1),
        ])
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "off" => Self::Off,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            _ => return None,
        })
    }
}

/// The renderer's quality settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    /// Internal resolution over output resolution, [`MIN_RENDER_SCALE`]–1. The scene
    /// renders (with MSAA) at the scaled size; the final pass upsamples it.
    pub render_scale: f32,
    /// Unsharp-mask strength of the upscale, 0–1 (unused at scale 1).
    pub sharpen: f32,
    /// Screen-space contact shading where the biome has it.
    pub contact_shadows: bool,
    /// Bloom where the biome has it.
    pub bloom: bool,
    /// Volumetric haze and shafts where the biome has them.
    pub haze: HazeQuality,
    /// The largest deviation a live model's level of detail may show, in pixels of the
    /// scene (0 draws every model in full).
    pub lod_pixels: f32,
    /// The sun's shadow map size, a power of two in 256–4096; 0 takes the lighting
    /// configuration's (`config/lighting.json`, 2048).
    pub shadow_map: u32,
    /// Most pixels the scene may render (0: no limit): lowers the render scale on large
    /// outputs so a preset costs about the same on any display.
    pub max_scene_pixels: u32,
}

pub const MIN_RENDER_SCALE: f32 = 0.5;

impl Default for Quality {
    fn default() -> Self {
        Self {
            render_scale: 1.0,
            sharpen: 0.25,
            contact_shadows: true,
            bloom: true,
            haze: HazeQuality::Medium,
            lod_pixels: DEFAULT_LOD_PIXELS,
            shadow_map: 0,
            max_scene_pixels: 0,
        }
    }
}

/// Projected deviation of levels of detail at the default quality: half a pixel, below
/// what MSAA edges show.
pub const DEFAULT_LOD_PIXELS: f32 = 0.5;

impl Quality {
    /// The shadow map's size under the lighting configuration `config`.
    pub fn shadow_resolution(&self, config: &crate::environment::ShadowConfig) -> u32 {
        if self.shadow_map == 0 {
            config.resolution
        } else {
            self.shadow_map.clamp(256, 4096).next_power_of_two()
        }
    }

    /// The internal (scene) size for an output of `size`: the render scale, lowered to
    /// keep within the pixel budget.
    pub fn scene_size(&self, size: [u32; 2]) -> [u32; 2] {
        let mut s = self.render_scale.clamp(MIN_RENDER_SCALE, 1.0);
        if self.max_scene_pixels > 0 {
            let pixels = (size[0] as f32 * size[1] as f32).max(1.0);
            s = s
                .min((self.max_scene_pixels as f32 / pixels).sqrt())
                .max(MIN_RENDER_SCALE);
        }
        size.map(|v| ((v as f32 * s).round() as u32).max(1))
    }
}

/// Graphics presets (D42): what each one sets, chosen on first run from the adapter
/// ([`Preset::for_adapter`]). High is today's look; the levels below trade detail for
/// speed on smaller GPUs, Ultra adds detail where it costs little.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Preset {
    Low,
    Medium,
    High,
    Ultra,
}

/// 1440 × 810: 75 % of 1080p in each direction.
const MEDIUM_PIXELS: u32 = 1440 * 810;
/// 1152 × 648: 60 % of 1080p in each direction.
const LOW_PIXELS: u32 = 1152 * 648;

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Low, Preset::Medium, Preset::High, Preset::Ultra];

    /// The renderer's settings of the preset.
    pub fn quality(self) -> Quality {
        let high = Quality::default();
        match self {
            Self::Ultra => Quality {
                haze: HazeQuality::High,
                lod_pixels: 0.25,
                ..high
            },
            Self::High => high,
            Self::Medium => Quality {
                max_scene_pixels: MEDIUM_PIXELS,
                contact_shadows: false,
                haze: HazeQuality::Low,
                lod_pixels: 1.5,
                shadow_map: 1024,
                ..high
            },
            Self::Low => Quality {
                max_scene_pixels: LOW_PIXELS,
                contact_shadows: false,
                bloom: false,
                haze: HazeQuality::Off,
                lod_pixels: 3.0,
                shadow_map: 1024,
                ..high
            },
        }
    }

    /// The preset for a graphics adapter, by its name: Apple's Pro, Max and Ultra chips
    /// and discrete GPUs get High, Apple's base chips (M1–M4; the base M1 is the
    /// reference machine) Medium, other integrated GPUs Low.
    pub fn for_adapter(name: &str, discrete: bool) -> Self {
        let name = name.to_ascii_lowercase();
        if name.contains("apple") {
            if ["pro", "max", "ultra"].iter().any(|t| name.contains(t)) {
                Self::High
            } else {
                Self::Medium
            }
        } else if discrete {
            Self::High
        } else {
            Self::Low
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(text))
    }
}

/// The post-effect buffers (contact shade, bloom) stay small at any resolution: half the
/// scene at most and about 180,000 pixels (old `beautyBufferSize`).
pub fn effect_size(size: [u32; 2]) -> [u32; 2] {
    let pixels = (size[0] as f32 * size[1] as f32).max(1.0);
    let s = 0.5f32.min((180_000.0 / pixels).sqrt());
    size.map(|v| ((v as f32 * s).floor() as u32).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_follow_the_adapter() {
        assert_eq!(Preset::for_adapter("Apple M1", false), Preset::Medium);
        assert_eq!(Preset::for_adapter("Apple M2", false), Preset::Medium);
        assert_eq!(Preset::for_adapter("Apple M1 Pro", false), Preset::High);
        assert_eq!(Preset::for_adapter("Apple M4 Max", false), Preset::High);
        assert_eq!(
            Preset::for_adapter("NVIDIA GeForce RTX 4070", true),
            Preset::High
        );
        assert_eq!(
            Preset::for_adapter("Intel(R) UHD Graphics", false),
            Preset::Low
        );
        assert_eq!(Preset::High.quality(), Quality::default());
        assert_eq!(Preset::parse("medium"), Some(Preset::Medium));
    }

    #[test]
    fn sizes_follow_the_old_budgets() {
        // 1080p: the post buffers are ≈ 0.295× (565 × 318).
        assert_eq!(effect_size([1920, 1080]), [565, 318]);
        // A small window keeps half resolution.
        assert_eq!(effect_size([400, 300]), [200, 150]);
        // Medium haze is capped by its pixel budget at 1080p, half size below it.
        let [w, h] = HazeQuality::Medium.size([1920, 1080]).unwrap();
        assert!((799..=800).contains(&w) && (449..=450).contains(&h));
        assert_eq!(HazeQuality::Medium.size([800, 600]), Some([400, 300]));
        assert_eq!(HazeQuality::Off.size([800, 600]), None);
        let q = Quality {
            render_scale: 0.75,
            ..Default::default()
        };
        assert_eq!(q.scene_size([2560, 1600]), [1920, 1200]);
        let tiny = Quality {
            render_scale: 0.1,
            ..Default::default()
        };
        assert_eq!(tiny.scene_size([1000, 500]), [500, 250]);
        assert_eq!(HazeQuality::parse("high"), Some(HazeQuality::High));
        // A pixel budget lowers the scale on large outputs only.
        let medium = Preset::Medium.quality();
        assert_eq!(medium.scene_size([1920, 1080]), [1440, 810]);
        assert_eq!(medium.scene_size([1280, 720]), [1280, 720]);
        let [w, h] = medium.scene_size([2560, 1600]);
        assert!((w * h).abs_diff(MEDIUM_PIXELS) < 4000, "{w}×{h}");
        assert_eq!(HazeQuality::parse("ultra"), None);
    }
}
