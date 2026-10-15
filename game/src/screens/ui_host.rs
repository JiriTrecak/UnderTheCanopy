//! The game's one UI: `canopy_ui::Ui` (menus, overlays and the in-match HUD) with its
//! theme images and on-demand images (map previews, crests, portraits) bound from the
//! cooked pack, drawn by the renderer's UI pass (`canopy_render::ui::UiLayer`) so the
//! HUD, the screens and plugged-in render targets share one texture table and one pass.
//!
//! Pack textures are straight alpha (the cooker un-premultiplies) and sRGB colour
//! textures are bound through their sRGB views, which the pass re-encodes (D33). The UI
//! draws into a non-sRGB view of the target so blending happens in sRGB space as in the
//! browser.

use anyhow::{Context, Result};
use canopy_assets::Pack;
use canopy_render::{Gpu, ui::UiLayer};
use canopy_ui::{FontSources, ImageDesc, ImageKey, TextureId, Ui, UiConfig};
use std::collections::BTreeMap;
use std::path::Path;

/// First texture id the host hands out: above the renderer's render targets and its own
/// pack images.
const FIRST_TEXTURE: u32 = 1 << 20;

pub struct UiHost {
    pub ui: Ui,
    /// Image names (pack entries or theme names) already bound.
    images: BTreeMap<String, Option<ImageKey>>,
    textures: Vec<wgpu::Texture>,
    /// Textures uploaded but not yet bound in the renderer's UI pass.
    pending: Vec<(TextureId, wgpu::TextureView)>,
    next_texture: u32,
    /// Display scale (physical pixels per logical pixel).
    scale: f32,
}

impl UiHost {
    /// Builds the UI from `config/ui` and binds the theme's images from the pack.
    pub fn new(data: &Path, gpu: &Gpu, pack: &Pack) -> Result<Self> {
        let config = UiConfig::load(&data.join("config/ui")).context("config/ui")?;
        let ui = Ui::new(
            config,
            FontSources {
                bundled: Vec::new(),
                system: true,
            },
        )
        .map_err(|e| anyhow::anyhow!("ui: {e}"))?;
        let mut host = Self {
            ui,
            images: BTreeMap::new(),
            textures: Vec::new(),
            pending: Vec::new(),
            next_texture: FIRST_TEXTURE,
            scale: 1.0,
        };
        let theme: Vec<(String, String)> = host
            .ui
            .theme_images()
            .map(|(n, a)| (n.to_owned(), a.to_owned()))
            .collect();
        for (name, asset) in theme {
            if host.bind(gpu, pack, &name, &asset).is_none() {
                tracing::warn!(image = %name, %asset, "theme image missing from the pack");
            }
        }
        Ok(host)
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(0.1);
    }

    /// Physical pixels per logical pixel.
    pub fn ui_scale(&self) -> f32 {
        self.scale
    }

    fn next_id(&mut self) -> TextureId {
        let id = TextureId(self.next_texture);
        self.next_texture += 1;
        id
    }

    /// Binds the pack texture `entry` under `name`; `None` when the pack lacks it.
    fn bind(&mut self, gpu: &Gpu, pack: &Pack, name: &str, entry: &str) -> Option<ImageKey> {
        if let Some(key) = self.images.get(name) {
            return *key;
        }
        let key = pack.texture(entry).ok().map(|texture| {
            let gpu_texture =
                canopy_render::texture::upload(&gpu.device, &gpu.queue, gpu.features, &texture);
            let view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let id = self.next_id();
            self.pending.push((id, view));
            self.textures.push(gpu_texture);
            let meta = &texture.meta;
            let srgb = gpu_texture_is_srgb(meta.format, gpu.features.bc);
            self.ui.register_image(
                name,
                ImageDesc {
                    texture: id,
                    size: [meta.source_width as f32, meta.source_height as f32],
                    uv: [0.0, 0.0, 1.0, 1.0],
                    premultiplied: false,
                    srgb_view: srgb,
                },
            )
        });
        self.images.insert(name.to_owned(), key);
        key
    }

    /// The image of a pack entry, bound on first use.
    pub fn pack_image(&mut self, gpu: &Gpu, pack: &Pack, entry: &str) -> Option<ImageKey> {
        self.bind(gpu, pack, entry, entry)
    }

    /// Binds the images uploaded since the last frame in the renderer's UI pass (before
    /// submitting a draw list that uses them).
    pub fn flush(&mut self, gpu: &Gpu, layer: &mut UiLayer) {
        for (id, view) in self.pending.drain(..) {
            layer.set_texture(gpu, id, &view);
        }
    }
}

/// Whether the uploaded texture of a stored format is sampled through an sRGB view.
fn gpu_texture_is_srgb(format: canopy_assets::TextureFormat, bc: bool) -> bool {
    let stored = if format.is_compressed() && !bc {
        format.uncompressed()
    } else {
        format
    };
    stored.is_srgb()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped theme and layouts read and compile (styles, templates, tooltips), and
    /// every screen lays out.
    #[test]
    fn the_shipped_interface_compiles() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/ui");
        let config = UiConfig::load(&dir).expect("config/ui reads");
        let mut ui = Ui::new(
            config,
            FontSources {
                bundled: Vec::new(),
                system: false,
            },
        )
        .expect("config/ui compiles");
        ui.set_viewport(1600, 900, 1.0);
        for screen in ["main-menu", "saves", "hud", "loading", "mission-hud"] {
            ui.show(screen, true);
            ui.frame(0.0);
            assert!(ui.find(screen).is_some(), "{screen}");
        }
    }
}
