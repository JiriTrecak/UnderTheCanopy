//! The UI on the game's device: `canopy_ui::Ui` with its theme images and on-demand
//! images (map previews, crests, portraits) bound from the cooked pack, its reference
//! pass, and window input.
//!
//! Pack textures are straight alpha (the cooker un-premultiplies) and sRGB colour
//! textures are bound through their sRGB views, which the pass re-encodes (D33). The UI
//! draws into a non-sRGB view of the target so blending happens in sRGB space as in the
//! browser.

use anyhow::{Context, Result};
use canopy_assets::Pack;
use canopy_render::Gpu;
use canopy_ui::{FontSources, ImageDesc, ImageKey, TextureId, Ui, UiConfig, gpu::UiRenderer};
use std::collections::BTreeMap;
use std::path::Path;

pub struct UiHost {
    pub ui: Ui,
    renderer: UiRenderer,
    /// Image names (pack entries or theme names) already bound.
    images: BTreeMap<String, Option<ImageKey>>,
    textures: Vec<wgpu::Texture>,
    next_texture: u32,
    /// Display scale (physical pixels per logical pixel).
    scale: f32,
}

impl UiHost {
    /// Builds the UI from `config/ui` and binds the theme's images from the pack.
    pub fn new(data: &Path, gpu: &Gpu, format: wgpu::TextureFormat, pack: &Pack) -> Result<Self> {
        let config = UiConfig::load(&data.join("config/ui")).context("config/ui")?;
        let ui = Ui::new(
            config,
            FontSources {
                bundled: Vec::new(),
                system: true,
            },
        )
        .map_err(|e| anyhow::anyhow!("ui: {e}"))?;
        let format = format.remove_srgb_suffix();
        let renderer = UiRenderer::new(&gpu.device, &gpu.queue, format);
        let mut host = Self {
            ui,
            renderer,
            images: BTreeMap::new(),
            textures: Vec::new(),
            next_texture: 0,
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
            self.renderer.set_texture(&gpu.device, id, &view);
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

    /// Lays out and paints the UI, uploads its draw list (every frame: it carries glyph
    /// atlas updates).
    pub fn prepare(&mut self, gpu: &Gpu, time: f64) {
        let list = self.ui.frame(time);
        self.renderer.prepare(&gpu.device, &gpu.queue, list);
    }

    /// Records the UI pass into `view` (a non-sRGB view of the target), clearing it first
    /// when `clear` is given (screens without a 3D scene behind).
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: Option<wgpu::Color>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        self.renderer.render(&mut pass, self.ui.draw_list());
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
