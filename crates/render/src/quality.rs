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
        }
    }
}

/// Projected deviation of levels of detail at the default quality: half a pixel, below
/// what MSAA edges show.
pub const DEFAULT_LOD_PIXELS: f32 = 0.5;

impl Quality {
    /// The internal (scene) size for an output of `size`.
    pub fn scene_size(&self, size: [u32; 2]) -> [u32; 2] {
        let s = self.render_scale.clamp(MIN_RENDER_SCALE, 1.0);
        size.map(|v| ((v as f32 * s).round() as u32).max(1))
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
        assert_eq!(HazeQuality::parse("ultra"), None);
    }
}
