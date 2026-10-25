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
        desc.opaque = source.exactly_opaque();
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
        let load = match clear {
            Some(clear) => wgpu::LoadOp::Clear(self.take_backdrop().unwrap_or(clear)),
            None => wgpu::LoadOp::Load,
        };
        self.record(encoder, view, load, None)
    }

    /// A screen drawn alone often starts with an opaque solid box over the whole target:
    /// it then becomes the clear colour and is not drawn (the same pixels: inside its
    /// edges the box covers whole with alpha 1, and the target's unorm channels hold its
    /// sRGB bytes either way). Returns that colour, having dropped the box from the list.
    fn take_backdrop(&mut self) -> Option<wgpu::Color> {
        use canopy_ui::QuadKind;
        if !self.pending {
            return None;
        }
        let q = self.list.quads.first()?;
        let batch = self.list.batches.first_mut()?;
        let [w, h] = self.list.viewport;
        let solid = q.kind() == QuadKind::Shape as u32
            && q.color[3] == 255
            && q.radius <= 0.0
            && q.border <= 0.0
            && q.shade == 1.0
            && q.flags & !0xff == 0;
        let whole = q.rect[0] <= 0.0
            && q.rect[1] <= 0.0
            && q.rect[2] >= w as f32
            && q.rect[3] >= h as f32
            && batch
                .scissor
                .is_none_or(|[x, y, sw, sh]| x == 0 && y == 0 && sw >= w && sh >= h)
            && batch.quads.start == 0
            && batch.quads.end > 0;
        if !solid || !whole {
            return None;
        }
        batch.quads.start = 1;
        let c = |v: u8| f64::from(v) / 255.0;
        Some(wgpu::Color {
            r: c(q.color[0]),
            g: c(q.color[1]),
            b: c(q.color[2]),
            a: 1.0,
        })
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
