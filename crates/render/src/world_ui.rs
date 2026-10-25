//! World UI (renderer §4.9): one quad shader in two draws. *Ground* triangles (selection
//! circles, rally routes and rings) are drawn inside the forward pass, depth-tested
//! against the scene; the *overlay* (health and mana bars, labels, status badges, rally
//! flags, progress bars) is drawn after the final pass straight onto the output in
//! pixels, over everything, as the old sprites with `depthTest: false` were. The caller
//! (the scene) builds both lists every frame; draw calls never depend on how many bars or
//! circles there are.

use crate::targets;
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

/// Vertices of each list a frame may draw.
pub const MAX_GROUND_VERTICES: usize = 1 << 17;
pub const MAX_OVERLAY_VERTICES: usize = 1 << 16;

/// A ground vertex: world position, linear straight-alpha colour and, for selection
/// rings, the position across the ring in line widths from its core (`ring`; the old
/// ring's soft inner glow, solid core and feathered rim come from it per pixel).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct GroundVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    /// Line widths from the ring's core for a selection ring; metres inside the edge for
    /// the network field.
    pub across: f32,
    /// 1 for a selection ring, 2 for the power network's field (D66), 3 for a hero's
    /// glow (`across` 0 at its centre to 1 at its edge), 4 for a building's contact
    /// shadow (`across` 0 at its footprint's edge to 1 where the shadow ends).
    pub ring_profile: f32,
}

/// Overlay shapes (`kind[0]`), mirroring `O_*` in world_ui.wgsl.
pub mod overlay {
    /// A flat colour.
    pub const SOLID: u32 = 0;
    /// A rounded box: `params` = width, height, radius, border (px); `extra` = border
    /// colour; `uv` = pixel position inside the box.
    pub const BOX: u32 = 1;
    /// A glyph: `uv` in the glyph atlas.
    pub const GLYPH: u32 = 2;
    /// An icon: `uv` in the icon atlas.
    pub const ICON: u32 = 3;
    /// The old vital bar: `uv` = CSS pixel inside the bar; `params` = width, height,
    /// health fill, maximum health; `extra` = mana fill, has mana; colour = health colour.
    pub const VITAL: u32 = 4;
}

/// An overlay vertex in output pixels.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct OverlayVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    /// Linear, straight alpha.
    pub color: [f32; 4],
    pub params: [f32; 4],
    pub extra: [f32; 4],
    pub kind: [u32; 4],
}

/// The world UI of one frame.
#[derive(Debug, Clone, Default)]
pub struct WorldUiFrame {
    /// Ground triangles drawn before `ground` that seldom change (buildings' contact
    /// shadows): shared, so a list the GPU already holds is not uploaded again.
    pub under: Option<Arc<Vec<GroundVertex>>>,
    /// Triangles.
    pub ground: Vec<GroundVertex>,
    /// Triangles.
    pub overlay: Vec<OverlayVertex>,
    /// Faint lights the marks give (heroes' glows); at most [`MAX_MARK_LIGHTS`].
    pub lights: Vec<crate::frame::PointLight>,
}

/// Most lights the world UI's marks give at once.
pub const MAX_MARK_LIGHTS: usize = 12;

impl WorldUiFrame {
    pub fn clear(&mut self) {
        self.under = None;
        self.ground.clear();
        self.overlay.clear();
        self.lights.clear();
    }

    /// A quad of the overlay (corners clockwise from the top left).
    pub fn quad(&mut self, corners: [OverlayVertex; 4]) {
        let [a, b, c, d] = corners;
        self.overlay.extend_from_slice(&[a, b, c, a, c, d]);
    }
}

/// An RGBA8 or R8 image for an atlas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AtlasImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct OverlayUniform {
    /// 2 / width, −2 / height, output is sRGB (0/1), 0.
    scale: [f32; 4],
}

pub struct WorldUiGpu {
    ground_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    overlay_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    atlas_layout: wgpu::BindGroupLayout,
    atlas_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    ground: wgpu::Buffer,
    overlay: wgpu::Buffer,
    uniform: wgpu::Buffer,
    /// The `under` list at the start of the ground buffer, and its vertex count.
    under: Option<Arc<Vec<GroundVertex>>>,
    under_count: usize,
    /// The overlay uniform last written.
    written_uniform: Option<[f32; 4]>,
    ground_count: u32,
    overlay_count: u32,
    glyphs: wgpu::TextureView,
    icons: wgpu::TextureView,
}

fn image_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    format: wgpu::TextureFormat,
    image: &AtlasImage,
) -> wgpu::TextureView {
    let bytes_per_texel = if format == wgpu::TextureFormat::R8Unorm {
        1
    } else {
        4
    };
    let (width, height) = (image.width.max(1), image.height.max(1));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let needed = (width * height * bytes_per_texel) as usize;
    let mut pixels = image.pixels.clone();
    pixels.resize(needed, 0);
    queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_texel),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&Default::default())
}

impl WorldUiGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let module = crate::shader::module(
            device,
            "world ui",
            &[crate::shader::Module::Frame, crate::shader::Module::Color],
            include_str!("../shaders/world_ui.wgsl"),
        );
        let frag = wgpu::ShaderStages::FRAGMENT;
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: frag,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world ui atlases"),
            entries: &[
                texture(0),
                texture(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX | frag,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let ground_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world ui ground"),
            bind_group_layouts: &[Some(frame_layout)],
            immediate_size: 0,
        });
        let overlay_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world ui overlay"),
            bind_group_layouts: &[None, Some(&atlas_layout)],
            immediate_size: 0,
        });
        let ground_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world ui ground"),
            layout: Some(&ground_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_ground"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GroundVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, 1 => Float32x4, 2 => Float32x2
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                bias: wgpu::DepthBiasState {
                    constant: 4,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
                ..targets::scene_depth(false)
            }),
            multisample: targets::scene_multisample(false),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_ground"),
                compilation_options: Default::default(),
                targets: &targets::forward_targets(Some(wgpu::BlendState::ALPHA_BLENDING), false),
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("world ui"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let buffer = |label, size: usize, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let ground = buffer(
            "world ui ground",
            MAX_GROUND_VERTICES * std::mem::size_of::<GroundVertex>(),
            wgpu::BufferUsages::VERTEX,
        );
        let overlay = buffer(
            "world ui overlay",
            MAX_OVERLAY_VERTICES * std::mem::size_of::<OverlayVertex>(),
            wgpu::BufferUsages::VERTEX,
        );
        let uniform = buffer(
            "world ui",
            std::mem::size_of::<OverlayUniform>(),
            wgpu::BufferUsages::UNIFORM,
        );
        let empty = AtlasImage {
            width: 1,
            height: 1,
            pixels: vec![0; 4],
        };
        let glyphs = image_texture(
            device,
            queue,
            "label glyphs",
            wgpu::TextureFormat::R8Unorm,
            &empty,
        );
        let icons = image_texture(
            device,
            queue,
            "badge icons",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &empty,
        );
        let atlas_group = Self::group(device, &atlas_layout, &glyphs, &icons, &sampler, &uniform);
        Self {
            ground_pipeline,
            overlay_pipeline: None,
            overlay_layout,
            module,
            atlas_layout,
            atlas_group,
            sampler,
            ground,
            overlay,
            uniform,
            under: None,
            under_count: 0,
            written_uniform: None,
            ground_count: 0,
            overlay_count: 0,
            glyphs,
            icons,
        }
    }

    fn group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        glyphs: &wgpu::TextureView,
        icons: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world ui atlases"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(glyphs),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(icons),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
            ],
        })
    }

    /// The label glyph coverage (R8) and the badge icons (sRGB RGBA8, straight alpha).
    pub fn set_atlases(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        glyphs: &AtlasImage,
        icons: &AtlasImage,
    ) {
        self.glyphs = image_texture(
            device,
            queue,
            "label glyphs",
            wgpu::TextureFormat::R8Unorm,
            glyphs,
        );
        self.icons = image_texture(
            device,
            queue,
            "badge icons",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            icons,
        );
        self.atlas_group = Self::group(
            device,
            &self.atlas_layout,
            &self.glyphs,
            &self.icons,
            &self.sampler,
            &self.uniform,
        );
    }

    /// Uploads a frame's lists (clipped to the budgets).
    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        frame: &WorldUiFrame,
        output: [u32; 2],
        output_srgb: bool,
    ) {
        // `under` then `ground` share the ground budget as one list would.
        let same = match (&frame.under, &self.under) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        };
        if !same {
            let under = frame
                .under
                .as_deref()
                .map_or(&[][..], |u| &u[..u.len().min(MAX_GROUND_VERTICES) / 3 * 3]);
            if !under.is_empty() {
                queue.write_buffer(&self.ground, 0, bytemuck::cast_slice(under));
            }
            self.under_count = under.len();
            self.under = frame.under.clone();
        }
        let room = MAX_GROUND_VERTICES - self.under_count;
        let ground = &frame.ground[..frame.ground.len().min(room) / 3 * 3];
        let overlay = &frame.overlay[..frame.overlay.len().min(MAX_OVERLAY_VERTICES) / 3 * 3];
        if !ground.is_empty() {
            queue.write_buffer(
                &self.ground,
                (self.under_count * std::mem::size_of::<GroundVertex>()) as u64,
                bytemuck::cast_slice(ground),
            );
        }
        if !overlay.is_empty() {
            queue.write_buffer(&self.overlay, 0, bytemuck::cast_slice(overlay));
        }
        self.ground_count = (self.under_count + ground.len()) as u32;
        self.overlay_count = overlay.len() as u32;
        let uniform = OverlayUniform {
            scale: [
                2.0 / output[0].max(1) as f32,
                -2.0 / output[1].max(1) as f32,
                if output_srgb { 1.0 } else { 0.0 },
                0.0,
            ],
        };
        if self.written_uniform != Some(uniform.scale) {
            queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
            self.written_uniform = Some(uniform.scale);
        }
    }

    /// Draws the ground list (inside the forward pass).
    pub fn draw_ground(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        if self.ground_count == 0 {
            return 0;
        }
        pass.set_pipeline(&self.ground_pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, self.ground.slice(..));
        pass.draw(0..self.ground_count, 0..1);
        1
    }

    /// Whether there is an overlay to draw.
    pub fn has_overlay(&self) -> bool {
        self.overlay_count > 0
    }

    /// Draws the overlay onto `output` (its own pass, after the final pass).
    pub fn encode_overlay(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        if self
            .overlay_pipeline
            .as_ref()
            .is_none_or(|(f, _)| *f != format)
        {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world ui overlay"),
                layout: Some(&self.overlay_layout),
                vertex: wgpu::VertexState {
                    module: &self.module,
                    entry_point: Some("vs_overlay"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<OverlayVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x2, 1 => Float32x2, 2 => Float32x4,
                            3 => Float32x4, 4 => Float32x4, 5 => Uint32x4
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.module,
                    entry_point: Some("fs_overlay"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.overlay_pipeline = Some((format, pipeline));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if self.overlay_count == 0 {
            return 0;
        }
        let (_, pipeline) = self.overlay_pipeline.as_ref().expect("made above");
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, &self.atlas_group, &[]);
        pass.set_vertex_buffer(0, self.overlay.slice(..));
        pass.draw(0..self.overlay_count, 0..1);
        1
    }
}
