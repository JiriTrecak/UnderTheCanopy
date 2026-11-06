//! The effect editor's node previews (D113): effect graph instances simulated by their own
//! [`GraphCompute`] and drawn into the cells of one thumbnail atlas, each cell through its
//! own camera, over a faint ground disc on the zinc stage (shaders/graph_thumbs.wgsl).
//! The editor shows the atlas's cells on its nodes; every frame steps them on, so the
//! previews play.

use crate::graph_fx::{
    BLEND, DrawUniform, GraphCompute, GraphDraw, GraphFrame, UNIFORM_STRIDE, binding, buffer,
    dynamic_uniform, storage, strided,
};
use crate::{device::GpuFeatures, texture};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::collections::BTreeMap;

/// One cell, in pixels.
pub const CELL: [u32; 2] = [256, 160];
/// Cells across and down the atlas.
pub const COLUMNS: u32 = 8;
pub const ROWS: u32 = 12;
/// Cells in all.
pub const CELLS: u32 = COLUMNS * ROWS;
/// The thumbnails' own pool and parts (4 MiB and 1 MiB).
const POOL_VEC4S: u32 = 1 << 18;
const PART_RECORDS: u32 = 1 << 14;
const MAX_DRAWS: usize = 1024;
/// The atlas as egui samples it (gamma-encoded texels, not decoded).
pub const SHOWN: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// The zinc stage (`#18181b`), linear.
const ZINC: wgpu::Color = wgpu::Color {
    r: 0.0091,
    g: 0.0091,
    b: 0.0109,
    a: 1.0,
};

/// A cell's camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbCamera {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub right: Vec3,
    pub up: Vec3,
}

impl ThumbCamera {
    /// Looking at `focus` from `distance` metres, `pitch` radians above the horizon and
    /// `yaw` around it, framed for a cell.
    pub fn orbit(focus: Vec3, distance: f32, yaw: f32, pitch: f32) -> Self {
        let eye = focus
            + Vec3::new(
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                yaw.cos() * pitch.cos(),
            ) * distance;
        let view = Mat4::look_at_rh(eye, focus, Vec3::Y);
        let aspect = CELL[0] as f32 / CELL[1] as f32;
        let proj = Mat4::perspective_rh(0.6, aspect, 0.1, distance * 4.0 + 50.0);
        let inverse = view.inverse();
        Self {
            view_proj: proj * view,
            eye,
            right: inverse.x_axis.truncate(),
            up: inverse.y_axis.truncate(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
}

/// One frame of previews: the simulation (its draws' `cue` names their cell in `cells`)
/// and each shown cell's camera.
#[derive(Debug, Clone, Default)]
pub struct ThumbFrame {
    pub graph: GraphFrame,
    /// Cue → cell.
    pub cells: BTreeMap<u32, u32>,
    pub cameras: BTreeMap<u32, ThumbCamera>,
}

pub struct GraphThumbs {
    features: GpuFeatures,
    compute: GraphCompute,
    pipeline: wgpu::RenderPipeline,
    view_buffer: wgpu::Buffer,
    view_group: wgpu::BindGroup,
    draw_buffer: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::BindGroup,
    textures: Vec<wgpu::BindGroup>,
    names: Vec<String>,
    atlas: wgpu::Texture,
    target: wgpu::TextureView,
}

impl GraphThumbs {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, features: GpuFeatures) -> Self {
        let compute = GraphCompute::new(device, POOL_VEC4S, PART_RECORDS);
        let vertex_fragment = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph thumbs view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: dynamic_uniform(std::mem::size_of::<ViewUniform>() as u64),
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph thumbs texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph thumbs draw"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: storage(true),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: vertex_fragment,
                    ty: dynamic_uniform(std::mem::size_of::<DrawUniform>() as u64),
                    count: None,
                },
            ],
        });
        let view_buffer = buffer(
            device,
            "graph thumbs views",
            u64::from(CELLS) * UNIFORM_STRIDE,
            wgpu::BufferUsages::UNIFORM,
        );
        // Draws, then one ground disc after them.
        let draw_buffer = buffer(
            device,
            "graph thumbs draws",
            (MAX_DRAWS as u64 + 1) * UNIFORM_STRIDE,
            wgpu::BufferUsages::UNIFORM,
        );
        let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph thumbs view"),
            layout: &view_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: binding(
                    &view_buffer,
                    Some(std::mem::size_of::<ViewUniform>() as u64),
                ),
            }],
        });
        let draw_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph thumbs draw"),
            layout: &draw_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: binding(compute.parts(), None),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: binding(
                        &draw_buffer,
                        Some(std::mem::size_of::<DrawUniform>() as u64),
                    ),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("graph_thumbs"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/graph_thumbs.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph thumbs"),
            bind_group_layouts: &[
                Some(&view_layout),
                Some(&texture_layout),
                Some(&draw_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph thumbs"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_thumb"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_thumb"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(BLEND),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("graph thumbs"),
            // Ribbons tile their texture down their length; sprites fall to black at
            // their edges, so wrapping never shows.
            address_mode_u: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white_view = texture::solid(
            device,
            queue,
            "graph thumbs white",
            wgpu::TextureFormat::Rgba8Unorm,
            [255; 4],
        )
        .create_view(&Default::default());
        let white = texture_group(device, &texture_layout, &white_view, &sampler);
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("graph thumbs atlas"),
            size: wgpu::Extent3d {
                width: CELL[0] * COLUMNS,
                height: CELL[1] * ROWS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[SHOWN],
        });
        let target = atlas.create_view(&Default::default());
        Self {
            features,
            compute,
            pipeline,
            view_buffer,
            view_group,
            draw_buffer,
            draw_group,
            texture_layout,
            sampler,
            white,
            textures: Vec::new(),
            names: Vec::new(),
            atlas,
            target,
        }
    }

    /// The atlas for egui: gamma-encoded texels, sampled as they are.
    pub fn atlas(&self) -> &wgpu::Texture {
        &self.atlas
    }

    /// Where `cell` lies in the atlas, as texture coordinates (min, max).
    pub fn uv(cell: u32) -> [[f32; 2]; 2] {
        let (x, y) = ((cell % COLUMNS) as f32, (cell / COLUMNS) as f32);
        let (w, h) = (1.0 / COLUMNS as f32, 1.0 / ROWS as f32);
        [[x * w, y * h], [(x + 1.0) * w, (y + 1.0) * h]]
    }

    /// The compiled layers steps refer to by position; the new ones compile at once, in
    /// parallel (a graph's previews are one program per node).
    pub fn set_programs(&mut self, device: &wgpu::Device, wgsl: &[&str]) {
        self.compute.set_programs(wgsl);
        let all: Vec<u32> = (0..wgsl.len() as u32).collect();
        self.compute.prewarm(device, &all);
    }

    /// The textures draws refer to by position (pack entry names); loaded again only when
    /// the list changed.
    pub fn load_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &canopy_assets::Pack,
        names: &[String],
    ) {
        if self.names == names {
            return;
        }
        self.names = names.to_vec();
        self.textures = names
            .iter()
            .map(|name| match pack.texture(name) {
                Ok(t) => {
                    let view = texture::upload(device, queue, self.features, &t)
                        .create_view(&Default::default());
                    texture_group(device, &self.texture_layout, &view, &self.sampler)
                }
                Err(error) => {
                    tracing::warn!(%name, %error, "preview texture not shown");
                    self.white.clone()
                }
            })
            .collect();
    }

    /// Steps the previews and draws every camera's cell.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &ThumbFrame) {
        let _span = tracing::trace_span!("graph_thumbs").entered();
        self.compute.prepare(device, queue, &frame.graph);
        let cameras: Vec<(u32, ThumbCamera)> = frame
            .cameras
            .iter()
            .take(CELLS as usize)
            .map(|(&c, &v)| (c, v))
            .collect();
        let views = strided(cameras.iter().map(|(_, v)| ViewUniform {
            view_proj: v.view_proj.to_cols_array_2d(),
            eye: v.eye.extend(1.0).to_array(),
            right: v.right.extend(0.0).to_array(),
            up: v.up.extend(0.0).to_array(),
        }));
        if !views.is_empty() {
            queue.write_buffer(&self.view_buffer, 0, &views);
        }
        // Previews draw over a plain stage: nothing behind to bend.
        let draws: Vec<&GraphDraw> = frame
            .graph
            .draws
            .iter()
            .filter(|d| !d.distort)
            .take(MAX_DRAWS)
            .collect();
        let ground = DrawUniform::ground();
        let uniforms = strided(
            draws
                .iter()
                .map(|d| DrawUniform::of(d))
                .chain(std::iter::once(ground))
                .collect::<Vec<_>>()
                .into_iter(),
        );
        queue.write_buffer(&self.draw_buffer, 0, &uniforms);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("graph thumbs"),
        });
        self.compute.simulate(&mut encoder);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("graph thumbs"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(ZINC),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (v, (cell, _)) in cameras.iter().enumerate() {
                let (x, y) = ((cell % COLUMNS) * CELL[0], (cell / COLUMNS) * CELL[1]);
                pass.set_viewport(x as f32, y as f32, CELL[0] as f32, CELL[1] as f32, 0.0, 1.0);
                pass.set_scissor_rect(x, y, CELL[0], CELL[1]);
                pass.set_bind_group(0, &self.view_group, &[(v as u64 * UNIFORM_STRIDE) as u32]);
                pass.set_bind_group(1, &self.white, &[]);
                pass.set_bind_group(
                    2,
                    &self.draw_group,
                    &[(draws.len() as u64 * UNIFORM_STRIDE) as u32],
                );
                pass.draw(0..6, 0..1);
                for (i, d) in draws.iter().enumerate() {
                    if frame.cells.get(&d.cue) != Some(cell) {
                        continue;
                    }
                    let texture = d
                        .texture
                        .and_then(|t| self.textures.get(t as usize))
                        .unwrap_or(&self.white);
                    pass.set_bind_group(1, texture, &[]);
                    pass.set_bind_group(2, &self.draw_group, &[(i as u64 * UNIFORM_STRIDE) as u32]);
                    pass.draw(0..6, 0..d.particles);
                }
            }
        }
        queue.submit([encoder.finish()]);
    }
}

impl DrawUniform {
    /// The ground disc (`MODE_GROUND` in graph_thumbs.wgsl).
    fn ground() -> Self {
        let mut d = Self::of(&GraphDraw {
            cue: 0,
            texture: None,
            mode: crate::graph_fx::GraphMode::Camera,
            additive: false,
            distort: false,
            soft: 0.0,
            flipbook: [1, 1],
            render_base: 0,
            renderer: 0,
            renderers: 1,
            particles: 1,
        });
        d.set_mode(9);
        d
    }
}

fn texture_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("graph thumbs texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
