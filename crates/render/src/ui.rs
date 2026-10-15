//! The game UI pass (renderer §4.1 step 10, docs/ui.md): the HUD and menus of
//! `canopy-ui` drawn over the finished frame in one batched quad pass.
//!
//! The UI composites in sRGB space like the old browser HUD (D33), so it draws into a
//! non-sRGB view of the output when the caller supplies one ([`UiLayer::set_target`]);
//! otherwise the reference pass linearises its output for an sRGB target.
//!
//! Images: the pack's textures are uploaded once per name ([`UiLayer::load_pack_image`])
//! and bound through their stored (usually sRGB) view, which the pass re-encodes.
//! Render targets other systems draw (the minimap, the live portrait) plug in by id
//! through [`UiLayer::set_texture`] and are registered with the `Ui` under their image
//! name (`@minimap`, `@portrait` in `hud.json`).

use crate::{Gpu, texture};
use canopy_ui::{DrawList, ImageDesc, TextureId, gpu::UiRenderer};
use std::collections::BTreeMap;

/// First texture id handed out for pack images; ids below are free for render targets.
pub const FIRST_PACK_TEXTURE: u32 = 1024;

pub struct UiLayer {
    pass: UiRenderer,
    /// The pass draws in sRGB space into a non-sRGB view (`target`), or linearises.
    format: wgpu::TextureFormat,
    target: Option<wgpu::TextureView>,
    list: DrawList,
    pending: bool,
    images: BTreeMap<String, ImageDesc>,
    textures: Vec<wgpu::Texture>,
    next_id: u32,
}

impl UiLayer {
    /// A UI pass over outputs of `output_format`. With an sRGB output, pass a non-sRGB
    /// view of it every frame through [`Self::set_target`] to blend like the old HUD.
    pub fn new(gpu: &Gpu, output_format: wgpu::TextureFormat) -> Self {
        let format = output_format.remove_srgb_suffix();
        Self {
            pass: UiRenderer::new(&gpu.device, &gpu.queue, format),
            format,
            target: None,
            list: DrawList::default(),
            pending: false,
            images: BTreeMap::new(),
            textures: Vec::new(),
            next_id: FIRST_PACK_TEXTURE,
        }
    }

    /// The format the UI pass draws in (the output's non-sRGB twin).
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The view the next frame's UI draws into (a non-sRGB view of the output).
    pub fn set_target(&mut self, view: Option<wgpu::TextureView>) {
        self.target = view;
    }

    /// Uploads this frame's draw list (atlas changes, quads). Every frame's list must
    /// arrive: atlas updates are incremental.
    pub fn submit(&mut self, gpu: &Gpu, list: &DrawList) {
        self.pass.prepare(&gpu.device, &gpu.queue, list);
        self.list.clone_from(list);
        self.pending = true;
    }

    /// Binds a render target (minimap, portrait) or any texture view to `id`.
    pub fn set_texture(&mut self, gpu: &Gpu, id: TextureId, view: &wgpu::TextureView) {
        self.pass.set_texture(&gpu.device, id, view);
    }

    /// Uploads the pack's texture `entry` (once) and returns how the UI draws it.
    pub fn load_pack_image(
        &mut self,
        gpu: &Gpu,
        pack: &canopy_assets::Pack,
        entry: &str,
    ) -> anyhow::Result<ImageDesc> {
        if let Some(desc) = self.images.get(entry) {
            return Ok(*desc);
        }
        let source = pack.texture(entry)?;
        let meta = &source.meta;
        let gpu_texture = texture::upload(&gpu.device, &gpu.queue, gpu.features, &source);
        let view = gpu_texture.create_view(&Default::default());
        let id = TextureId(self.next_id);
        self.next_id += 1;
        self.pass.set_texture(&gpu.device, id, &view);
        let mut desc = ImageDesc::new(id, meta.source_width as f32, meta.source_height as f32);
        // The cooker filters translucent images premultiplied but stores them straight
        // (D26), so every pack image draws with straight alpha.
        desc.premultiplied = false;
        desc.srgb_view = gpu_texture.format().is_srgb();
        self.textures.push(gpu_texture);
        self.images.insert(entry.to_owned(), desc);
        Ok(desc)
    }

    /// Records the UI alone over `view` (a non-sRGB view of the output), first cleared to
    /// `clear` when given: screens without a world behind them.
    pub fn encode_alone(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: Option<wgpu::Color>,
    ) -> u32 {
        self.target = None;
        let load = clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear);
        self.record(encoder, view, load, None)
    }

    /// Records the UI over `output` (or the target set for this frame) and forgets the
    /// list. Nothing is recorded without a submitted list.
    pub(crate) fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let target = self.target.take();
        let view = target.as_ref().unwrap_or(output);
        self.record(encoder, view, wgpu::LoadOp::Load, timestamps)
    }

    fn record(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let pending = std::mem::take(&mut self.pending);
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(crate::graph::passes::UI),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if !pending {
            return 0;
        }
        self.pass.render(&mut rp, &self.list);
        self.list.batches.len() as u32
    }
}
