//! The renderer: owns the frame graph's resources and records its passes in order.

use crate::{
    camera::{CameraView, frustum_planes},
    device::Gpu,
    environment::{BiomeConfig, DaytimeLut, GRADE_SIZE, Lighting, LightingConfig},
    frame::{self, FrameParams, FrameUniform, MAX_POINT_LIGHTS, PointLight},
    graph::{self, FrameGraph, PassKind, passes},
    shadow::{self, ShadowView},
    statics::{StaticInstance, StaticsGpu},
    targets::{SHADOW_FORMAT, Targets},
    terrain::{TerrainDesc, gpu::TerrainGpu},
    timing::GpuTimer,
};
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::time::Instant;

/// Size of the canopy pattern texture.
const CANOPY_SIZE: u32 = 256;

/// The lighting environment of a world: resolved light, grade and canopy pattern.
#[derive(Debug, Clone)]
pub struct Environment {
    pub lighting: Lighting,
    pub shadow: crate::environment::ShadowConfig,
    /// The baked grade, [`GRADE_SIZE`]³ RGBA8.
    pub grade: Vec<u8>,
    /// R8 canopy shade pattern, [`CANOPY_SIZE`]².
    pub canopy: Vec<u8>,
    /// Linear haze colour and distances (surroundings).
    pub haze: (Vec3, [f32; 2]),
}

impl Environment {
    /// A biome's environment at `hour`; `daytime` is the pack's daytime lookup (`#day`)
    /// when the biome mixes one in.
    pub fn new(
        lighting: &LightingConfig,
        biome: &BiomeConfig,
        hour: f32,
        daytime: Option<&DaytimeLut<'_>>,
    ) -> Self {
        let resolved = Lighting::resolve(lighting, biome, hour);
        let s = &biome.surroundings;
        Self {
            grade: crate::environment::bake_grade(&biome.grade, daytime),
            canopy: crate::environment::canopy_pattern(&biome.canopy, CANOPY_SIZE),
            haze: (s.haze_color.linear(), [s.haze_start, s.haze_end]),
            lighting: resolved,
            shadow: lighting.shadow,
        }
    }
}

/// What is drawn.
#[derive(Debug, Clone, Default)]
pub struct World {
    pub terrain: Option<TerrainDesc>,
    pub statics: Vec<StaticInstance>,
    pub point_lights: Vec<PointLight>,
}

/// Measurements of one frame.
#[derive(Debug, Clone, Default)]
pub struct FrameReport {
    /// Frame-graph pass names, in order.
    pub passes: Vec<&'static str>,
    /// CPU time recording each pass, ms.
    pub cpu_ms: Vec<f32>,
    /// GPU time of each pass from the latest measured frame, ms.
    pub gpu_ms: Option<Vec<f32>>,
    /// CPU time of the whole `render` call, ms.
    pub cpu_total_ms: f32,
    pub draws: u32,
    pub terrain_triangles: u64,
    pub terrain_chunks: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PostUniform {
    params: [f32; 4],
}

struct Post {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    grade: wgpu::TextureView,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
}

pub struct Renderer {
    graph: FrameGraph,
    frame_layout: wgpu::BindGroupLayout,
    frame_buffer: wgpu::Buffer,
    shadow_frame_buffer: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    shadow_frame_group: wgpu::BindGroup,
    light_buffer: wgpu::Buffer,
    point_lights: u32,
    shadow_view: wgpu::TextureView,
    shadow_resolution: u32,
    canopy_texture: wgpu::Texture,
    targets: Targets,
    post: Post,
    output_format: wgpu::TextureFormat,
    environment: Environment,
    terrain: Option<TerrainGpu>,
    statics: Option<StaticsGpu>,
    timer: Option<GpuTimer>,
    /// Shadows on (a setting).
    pub shadows: bool,
    /// Bytes of GPU memory the world uses.
    pub world_bytes: u64,
}

fn grade_texture(device: &wgpu::Device, queue: &wgpu::Queue, volume: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: GRADE_SIZE,
        height: GRADE_SIZE,
        depth_or_array_layers: GRADE_SIZE,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grade"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        volume,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(GRADE_SIZE * 4),
            rows_per_image: Some(GRADE_SIZE),
        },
        size,
    );
    texture.create_view(&Default::default())
}

impl Renderer {
    /// A renderer drawing into `output_format` at `size`.
    pub fn new(
        gpu: &Gpu,
        output_format: wgpu::TextureFormat,
        size: [u32; 2],
        environment: Environment,
    ) -> Self {
        let device = &gpu.device;
        let queue = &gpu.queue;
        let graph = graph::world_frame();
        let frame_layout = frame::layout(device);
        let uniform = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<FrameUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let frame_buffer = uniform("frame");
        let shadow_frame_buffer = uniform("shadow frame");
        let light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("point lights"),
            size: (MAX_POINT_LIGHTS * std::mem::size_of::<PointLight>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_resolution = environment.shadow.resolution;
        let depth_texture = |label, size: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SHADOW_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let shadow_view =
            depth_texture("sun shadow", shadow_resolution).create_view(&Default::default());
        let dummy_shadow = depth_texture("no shadow", 1).create_view(&Default::default());
        let canopy_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canopy"),
            size: wgpu::Extent3d {
                width: CANOPY_SIZE,
                height: CANOPY_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let repeat_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let canopy_view = canopy_texture.create_view(&Default::default());
        let group = |label, buffer: &wgpu::Buffer, shadow: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &frame_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(shadow),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&canopy_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&repeat_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: light_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let frame_group = group("frame", &frame_buffer, &shadow_view);
        let shadow_frame_group = group("shadow frame", &shadow_frame_buffer, &dummy_shadow);
        let targets = Targets::new(device, size);
        let post = Self::create_post(device, queue, output_format, &targets, &environment.grade);
        let timer = gpu
            .features
            .timestamps
            .then(|| GpuTimer::new(device, queue, graph.passes().len()));
        let mut renderer = Self {
            graph,
            frame_layout,
            frame_buffer,
            shadow_frame_buffer,
            frame_group,
            shadow_frame_group,
            light_buffer,
            point_lights: 0,
            shadow_view,
            shadow_resolution,
            canopy_texture,
            targets,
            post,
            output_format,
            environment,
            terrain: None,
            statics: None,
            timer,
            shadows: true,
            world_bytes: 0,
        };
        renderer.upload_environment(queue);
        renderer
    }

    fn create_post(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output_format: wgpu::TextureFormat,
        targets: &Targets,
        grade: &[u8],
    ) -> Post {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: frag,
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
            "post",
            &[crate::shader::Module::Color, crate::shader::Module::Dither],
            include_str!("../shaders/post.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(output_format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("grade"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post"),
            size: std::mem::size_of::<PostUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grade = grade_texture(device, queue, grade);
        let group = Self::post_group(device, &layout, targets, &grade, &sampler, &uniform);
        Post {
            pipeline,
            layout,
            group,
            grade,
            sampler,
            uniform,
        }
    }

    fn post_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        targets: &Targets,
        grade: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&targets.resolved),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(grade),
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

    fn upload_environment(&mut self, queue: &wgpu::Queue) {
        queue.write_texture(
            self.canopy_texture.as_image_copy(),
            &self.environment.canopy,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(CANOPY_SIZE),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: CANOPY_SIZE,
                height: CANOPY_SIZE,
                depth_or_array_layers: 1,
            },
        );
        let grade = &self.environment.lighting.grade;
        let post = PostUniform {
            params: [
                grade.vignette,
                grade.highlight_shoulder,
                if self.output_format.is_srgb() {
                    1.0
                } else {
                    0.0
                },
                0.0,
            ],
        };
        queue.write_buffer(&self.post.uniform, 0, bytemuck::bytes_of(&post));
    }

    /// Replaces the lighting environment (same shadow resolution).
    pub fn set_environment(&mut self, gpu: &Gpu, environment: Environment) {
        self.post.grade = grade_texture(&gpu.device, &gpu.queue, &environment.grade);
        self.environment = environment;
        self.post.group = Self::post_group(
            &gpu.device,
            &self.post.layout,
            &self.targets,
            &self.post.grade,
            &self.post.sampler,
            &self.post.uniform,
        );
        self.upload_environment(&gpu.queue);
    }

    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// Uploads a world, replacing the previous one.
    pub fn load_world(
        &mut self,
        gpu: &Gpu,
        pack: &canopy_assets::Pack,
        world: &World,
    ) -> Result<()> {
        let _span = tracing::info_span!("load_world").entered();
        self.terrain = None;
        self.statics = None;
        self.world_bytes = 0;
        if let Some(desc) = &world.terrain {
            let terrain = TerrainGpu::new(
                &gpu.device,
                &gpu.queue,
                gpu.features,
                pack,
                desc,
                &self.frame_layout,
            )?;
            self.world_bytes += terrain.bytes;
            self.terrain = Some(terrain);
        }
        let statics = StaticsGpu::new(
            &gpu.device,
            &gpu.queue,
            gpu.features,
            pack,
            &world.statics,
            &self.frame_layout,
        )?;
        self.world_bytes += statics.bytes;
        self.statics = Some(statics);
        let lights = &world.point_lights[..world.point_lights.len().min(MAX_POINT_LIGHTS)];
        if !lights.is_empty() {
            gpu.queue
                .write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(lights));
        }
        self.point_lights = lights.len() as u32;
        Ok(())
    }

    pub fn statics(&self) -> Option<&StaticsGpu> {
        self.statics.as_ref()
    }

    pub fn terrain(&self) -> Option<&TerrainGpu> {
        self.terrain.as_ref()
    }

    pub fn graph(&self) -> &FrameGraph {
        &self.graph
    }

    /// Bytes of GPU memory in use: world, targets and the shadow map.
    pub fn gpu_bytes(&self) -> u64 {
        let shadow = u64::from(self.shadow_resolution).pow(2) * 4;
        self.world_bytes + self.targets.bytes() + shadow
    }

    pub fn resize(&mut self, gpu: &Gpu, size: [u32; 2]) {
        if size == self.targets.size || size[0] == 0 || size[1] == 0 {
            return;
        }
        self.targets = Targets::new(&gpu.device, size);
        self.post.group = Self::post_group(
            &gpu.device,
            &self.post.layout,
            &self.targets,
            &self.post.grade,
            &self.post.sampler,
            &self.post.uniform,
        );
    }

    pub fn size(&self) -> [u32; 2] {
        self.targets.size
    }

    /// Renders one frame of `camera` at `seconds` (wind, canopy drift) into `output`.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        camera: &CameraView,
        seconds: f32,
        output: &wgpu::TextureView,
        before_submit: Option<&dyn Fn(&mut wgpu::CommandEncoder)>,
    ) -> FrameReport {
        let _span = tracing::trace_span!("render").entered();
        let start = Instant::now();
        let device = &gpu.device;
        let queue = &gpu.queue;
        if let Some(timer) = &mut self.timer {
            timer.begin_frame();
        }
        let lighting = &self.environment.lighting;
        let ShadowView {
            view_projection: shadow_matrix,
            ..
        } = shadow::fit(
            camera,
            camera.target.y,
            lighting.sun_direction,
            &self.environment.shadow,
        );
        let uniform = FrameUniform::new(&FrameParams {
            camera,
            lighting,
            shadow_matrix,
            shadow_texel: 1.0 / self.shadow_resolution as f32,
            shadows: self.shadows,
            seconds,
            viewport: self.targets.size,
            haze: self.environment.haze.0,
            haze_range: self.environment.haze.1,
            point_lights: self.point_lights,
        });
        queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
        queue.write_buffer(
            &self.shadow_frame_buffer,
            0,
            bytemuck::bytes_of(&uniform.for_shadow(shadow_matrix)),
        );
        let shadow_planes = frustum_planes(shadow_matrix);
        let mut report = FrameReport {
            passes: self.graph.passes().iter().map(|p| p.name).collect(),
            ..Default::default()
        };
        if let Some(terrain) = &mut self.terrain {
            let stats = terrain.prepare(
                queue,
                camera,
                self.targets.size[1] as f32,
                self.shadows.then_some(&shadow_planes),
            );
            report.terrain_triangles = stats.triangles;
            report.terrain_chunks = stats.chunks;
        }
        if let Some(statics) = &self.statics {
            statics.prepare(queue, camera.view_projection, shadow_matrix);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        let background = lighting.background;
        for (index, pass) in self.graph.passes().iter().enumerate() {
            let pass_start = Instant::now();
            let _span = tracing::trace_span!("pass", name = pass.name).entered();
            let timestamps = self.timer.as_ref();
            match (pass.name, pass.kind) {
                (passes::CULL, PassKind::Compute) => {
                    if let Some(statics) = &self.statics {
                        statics.clear(&mut encoder);
                    }
                    let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some(passes::CULL),
                        timestamp_writes: timestamps.and_then(|t| t.compute_writes(index)),
                    });
                    if let Some(statics) = &self.statics {
                        statics.cull(&mut compute);
                    }
                }
                (passes::SHADOW, PassKind::Render) => {
                    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(passes::SHADOW),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.shadow_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: timestamps.and_then(|t| t.render_writes(index)),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    if self.shadows {
                        if let Some(terrain) = &self.terrain {
                            report.draws += terrain.draw_shadow(&mut rp, &self.shadow_frame_group);
                        }
                        if let Some(statics) = &self.statics {
                            report.draws += statics.draw_shadow(&mut rp, &self.shadow_frame_group);
                        }
                    }
                }
                (passes::FORWARD, PassKind::Render) => {
                    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(passes::FORWARD),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &self.targets.color,
                            depth_slice: None,
                            resolve_target: Some(&self.targets.resolved),
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: f64::from(background.x),
                                    g: f64::from(background.y),
                                    b: f64::from(background.z),
                                    a: 1.0,
                                }),
                                // Multisampled colour is resolved in the pass (in tile memory
                                // on Apple GPUs) and never stored.
                                store: wgpu::StoreOp::Discard,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.targets.depth,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(0.0),
                                store: wgpu::StoreOp::Discard,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: timestamps.and_then(|t| t.render_writes(index)),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    if let Some(terrain) = &self.terrain {
                        report.draws += terrain.draw_forward(&mut rp, &self.frame_group);
                    }
                    if let Some(statics) = &self.statics {
                        report.draws += statics.draw_forward(&mut rp, &self.frame_group);
                    }
                    if let Some(terrain) = &self.terrain {
                        report.draws += terrain.draw_water(&mut rp, &self.frame_group);
                    }
                    if let Some(statics) = &self.statics {
                        report.draws += statics.draw_transparent(&mut rp, &self.frame_group);
                    }
                }
                (passes::POST, PassKind::Render) => {
                    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(passes::POST),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: output,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: timestamps.and_then(|t| t.render_writes(index)),
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    rp.set_pipeline(&self.post.pipeline);
                    rp.set_bind_group(0, &self.post.group, &[]);
                    rp.draw(0..3, 0..1);
                    report.draws += 1;
                }
                (name, _) => unreachable!("no recorder for pass {name}"),
            }
            report
                .cpu_ms
                .push(pass_start.elapsed().as_secs_f32() * 1000.0);
        }
        if let Some(timer) = &self.timer {
            timer.resolve(&mut encoder);
        }
        if let Some(extra) = before_submit {
            extra(&mut encoder);
        }
        {
            let _span = tracing::trace_span!("submit").entered();
            queue.submit([encoder.finish()]);
        }
        if let Some(timer) = &mut self.timer {
            timer.end_frame();
            report.gpu_ms = timer.latest().map(<[f32]>::to_vec);
        }
        report.cpu_total_ms = start.elapsed().as_secs_f32() * 1000.0;
        report
    }
}
