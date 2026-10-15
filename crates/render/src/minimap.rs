//! The minimap as a render-to-texture output (renderer §6): a north-up overview the HUD
//! shows through a `canopy-ui` image node.
//!
//! Its background is the world itself seen straight down — terrain, water and scenery
//! rendered once by the renderer with an orthographic camera when the world loads
//! ([`crate::Renderer::bake_minimap`]), so it can never disagree with the terrain (one
//! source of truth, renderer §4.4). Each frame the composite darkens it by the fog of war
//! (old minimap: explored 65 %, unexplored black) and draws the caller's markers (units
//! and building footprints in player colours, remembered structures dimmed) and the
//! camera's ground footprint (old `Minimap.paint`).

use crate::targets;
use bytemuck::{Pod, Zeroable};

/// Marker vertices a frame may draw.
pub const MAX_MINIMAP_VERTICES: usize = 1 << 15;

/// A marker vertex in minimap pixels with a linear, straight-alpha colour.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct MinimapVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

/// What the minimap shows this frame besides its background.
#[derive(Debug, Clone, Default)]
pub struct MinimapFrame {
    /// Triangles.
    pub markers: Vec<MinimapVertex>,
}

impl MinimapFrame {
    /// A filled rectangle.
    pub fn rect(&mut self, [x0, y0, x1, y1]: [f32; 4], color: [f32; 4]) {
        let v = |x, y| MinimapVertex {
            position: [x, y],
            color,
        };
        self.markers.extend_from_slice(&[
            v(x0, y0),
            v(x1, y0),
            v(x1, y1),
            v(x0, y0),
            v(x1, y1),
            v(x0, y1),
        ]);
    }

    /// A line of `width` pixels from `a` to `b`.
    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, color: [f32; 4]) {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt().max(1e-4);
        let (nx, ny) = (-dy / len * width * 0.5, dx / len * width * 0.5);
        let v = |x: f32, y: f32| MinimapVertex {
            position: [x, y],
            color,
        };
        self.markers.extend_from_slice(&[
            v(a[0] - nx, a[1] - ny),
            v(a[0] + nx, a[1] + ny),
            v(b[0] + nx, b[1] + ny),
            v(a[0] - nx, a[1] - ny),
            v(b[0] + nx, b[1] + ny),
            v(b[0] - nx, b[1] - ny),
        ]);
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct MinimapUniform {
    /// 2 / size, −2 / size, fog on, 0.
    scale: [f32; 4],
}

pub struct MinimapGpu {
    pub size: u32,
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    base: wgpu::TextureView,
    background: wgpu::RenderPipeline,
    markers: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    vertices: wgpu::Buffer,
    group: wgpu::BindGroup,
    count: u32,
}

impl MinimapGpu {
    /// A minimap of `size` pixels over the baked `base` (sRGB, `size`²), sampling `fog`.
    pub fn new(
        device: &wgpu::Device,
        size: u32,
        base: wgpu::TextureView,
        fog: &wgpu::TextureView,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("minimap"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: targets::UI_IMAGE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let frag = wgpu::ShaderStages::FRAGMENT;
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: frag,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("minimap"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
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
        let module = crate::shader::module(
            device,
            "minimap",
            &[],
            include_str!("../shaders/minimap.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("minimap"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, fs: &str, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("minimap"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: targets::UI_IMAGE_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let background = pipeline("vs_background", "fs_background", &[]);
        let markers = pipeline(
            "vs_marker",
            "fs_marker",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<MinimapVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
            })],
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("minimap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("minimap"),
            size: std::mem::size_of::<MinimapUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("minimap markers"),
            size: (MAX_MINIMAP_VERTICES * std::mem::size_of::<MinimapVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = Self::group(device, &layout, &base, fog, &sampler, &uniform);
        Self {
            size,
            texture,
            view,
            base,
            background,
            markers,
            layout,
            sampler,
            uniform,
            vertices,
            group,
            count: 0,
        }
    }

    fn group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        base: &wgpu::TextureView,
        fog: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("minimap"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(base),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(fog),
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

    /// The fog texture was replaced.
    pub fn rebind(&mut self, device: &wgpu::Device, fog: &wgpu::TextureView) {
        self.group = Self::group(
            device,
            &self.layout,
            &self.base,
            fog,
            &self.sampler,
            &self.uniform,
        );
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn prepare(&mut self, queue: &wgpu::Queue, frame: &MinimapFrame, fog: bool) {
        let markers = &frame.markers[..frame.markers.len().min(MAX_MINIMAP_VERTICES) / 3 * 3];
        if !markers.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(markers));
        }
        self.count = markers.len() as u32;
        let s = self.size as f32;
        let uniform = MinimapUniform {
            scale: [2.0 / s, -2.0 / s, if fog { 1.0 } else { 0.0 }, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("minimap"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_pipeline(&self.background);
        pass.draw(0..3, 0..1);
        if self.count > 0 {
            pass.set_pipeline(&self.markers);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..self.count, 0..1);
            return 2;
        }
        1
    }
}
