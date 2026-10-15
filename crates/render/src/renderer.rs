//! The renderer: owns the frame graph's resources and records its passes in order.

use crate::{
    camera::{CameraView, frustum_planes},
    device::Gpu,
    environment::{BiomeConfig, DaytimeLut, Lighting, LightingConfig},
    frame::{self, FrameParams, FrameUniform, MAX_POINT_LIGHTS, PointLight},
    graph::{self, FrameGraph, PassKind, passes},
    post::{OPENINGS_SIZE, PostLook, PostStack},
    quality::Quality,
    shadow::{self, ShadowView},
    statics::{StaticInstance, StaticsGpu},
    surroundings::{ForestDesc, ForestGpu},
    targets::{FAR_DEPTH, SHADOW_FORMAT, Targets},
    terrain::{TerrainDesc, gpu::TerrainGpu},
    timing::GpuTimer,
    water::{
        WaterDesc,
        gpu::{WaterGpu, WaterStats, WaterTextures},
        ripples::Ripples,
    },
};
use anyhow::Result;
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
    /// The biome's post effects.
    pub post: PostLook,
    /// R8 canopy openings where shafts scatter, [`OPENINGS_SIZE`]².
    pub openings: Vec<u8>,
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
            post: PostLook {
                bloom: biome.bloom,
                contact: biome.contact,
                atmosphere: biome.atmosphere,
            },
            openings: crate::environment::canopy_openings(&biome.canopy, OPENINGS_SIZE),
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
    /// Glows by model entry.
    pub glows: std::collections::BTreeMap<String, crate::statics::Glow>,
    pub point_lights: Vec<PointLight>,
    /// Water over the terrain's wet cells (needs `terrain`).
    pub water: Option<WaterDesc>,
    /// Pack textures the water samples.
    pub water_textures: WaterTextures,
    /// The forest around the map (needs `terrain`).
    pub forest: Option<ForestDesc>,
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
    pub water: WaterStats,
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
    post: PostStack,
    output_format: wgpu::TextureFormat,
    output_size: [u32; 2],
    quality: Quality,
    environment: Environment,
    terrain: Option<TerrainGpu>,
    statics: Option<StaticsGpu>,
    water: Option<WaterGpu>,
    forest: Option<ForestGpu>,
    ripples: Ripples,
    timer: Option<GpuTimer>,
    timestamps: bool,
    /// Shadows on (a setting).
    pub shadows: bool,
    /// Bytes of GPU memory the world uses.
    pub world_bytes: u64,
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
        let quality = Quality::default();
        let graph = graph::world_frame(environment.post.features(&quality, false));
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
        let targets = Targets::new(device, quality.scene_size(size));
        let post = PostStack::new(
            device,
            queue,
            output_format,
            &frame_layout,
            &targets,
            &environment.grade,
            &environment.openings,
        );
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
            output_size: size,
            quality,
            environment,
            terrain: None,
            statics: None,
            water: None,
            forest: None,
            ripples: Ripples::new(),
            timer: None,
            timestamps: gpu.features.timestamps,
            shadows: true,
            world_bytes: 0,
        };
        renderer.upload_environment(queue);
        renderer.rebuild(gpu);
        renderer
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
        self.post.set_openings(queue, &self.environment.openings);
    }

    /// Rebuilds the frame graph and the post targets for the current world, biome and
    /// quality.
    fn rebuild(&mut self, gpu: &Gpu) {
        let features = self
            .environment
            .post
            .features(&self.quality, self.water.is_some());
        self.graph = graph::world_frame(features);
        self.timer = self
            .timestamps
            .then(|| GpuTimer::new(&gpu.device, &gpu.queue, self.graph.passes().len()));
        self.post
            .configure(&gpu.device, &self.targets, features, self.quality.haze);
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }

    /// Changes the quality settings: render scale and post effects.
    pub fn set_quality(&mut self, gpu: &Gpu, quality: Quality) {
        self.quality = quality;
        let scene = quality.scene_size(self.output_size);
        if scene != self.targets.size {
            self.targets = Targets::new(&gpu.device, scene);
        }
        self.rebuild(gpu);
    }

    /// The unit wakes: feed it the bodies that wade every frame
    /// ([`Ripples::update`]); the renderer draws what it holds.
    pub fn ripples_mut(&mut self) -> &mut Ripples {
        &mut self.ripples
    }

    /// Replaces the lighting environment (same shadow resolution).
    pub fn set_environment(&mut self, gpu: &Gpu, environment: Environment) {
        self.post
            .set_grade(&gpu.device, &gpu.queue, &environment.grade);
        self.environment = environment;
        self.upload_environment(&gpu.queue);
        self.rebuild(gpu);
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
        self.water = None;
        self.forest = None;
        self.ripples.clear();
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
            if let Some(water) = &world.water {
                self.water = WaterGpu::new(
                    &gpu.device,
                    &gpu.queue,
                    gpu.features,
                    pack,
                    desc,
                    water,
                    &world.water_textures,
                    &self.frame_layout,
                )?;
                self.world_bytes += self.water.as_ref().map_or(0, |w| w.bytes);
            }
            if let Some(forest) = &world.forest {
                let gpu_forest = ForestGpu::new(
                    &gpu.device,
                    &gpu.queue,
                    gpu.features,
                    pack,
                    forest,
                    desc,
                    (self.environment.haze.0.to_array(), self.environment.haze.1),
                    &self.frame_layout,
                )?;
                self.world_bytes += gpu_forest.bytes;
                self.forest = Some(gpu_forest);
            }
        }
        let statics = StaticsGpu::new(
            &gpu.device,
            &gpu.queue,
            gpu.features,
            pack,
            &world.statics,
            &world.glows,
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
        self.rebuild(gpu);
        Ok(())
    }

    pub fn water(&self) -> Option<&WaterGpu> {
        self.water.as_ref()
    }

    pub fn forest(&self) -> Option<&ForestGpu> {
        self.forest.as_ref()
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

    /// Bytes of GPU memory in use: world, targets, post effects and the shadow map.
    pub fn gpu_bytes(&self) -> u64 {
        let shadow = u64::from(self.shadow_resolution).pow(2) * 4;
        self.world_bytes + self.targets.bytes() + self.post.bytes + shadow
    }

    pub fn resize(&mut self, gpu: &Gpu, size: [u32; 2]) {
        if size == self.output_size || size[0] == 0 || size[1] == 0 {
            return;
        }
        self.output_size = size;
        self.targets = Targets::new(&gpu.device, self.quality.scene_size(size));
        self.rebuild(gpu);
    }

    /// The output size.
    pub fn size(&self) -> [u32; 2] {
        self.output_size
    }

    /// The size the scene renders at (output × render scale).
    pub fn scene_size(&self) -> [u32; 2] {
        self.targets.size
    }

    /// Renders one frame of `camera` at `seconds` (wind, canopy drift, water) into
    /// `output`.
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
        if let Some(water) = &mut self.water {
            report.water = water.prepare(
                queue,
                camera,
                seconds,
                &self.ripples,
                lighting.daylight_ambient,
            );
        }
        let sun = (lighting.sun_color, lighting.sun_intensity);
        if let Some(forest) = &mut self.forest {
            forest.prepare(queue, camera, sun.0, sun.1);
        }
        let grade = &lighting.grade;
        self.post.prepare(
            queue,
            camera,
            seconds,
            self.targets.size,
            self.output_size,
            self.output_format.is_srgb(),
            &self.environment.post,
            (grade.vignette, grade.highlight_shoulder),
            &self.quality,
            sun,
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        let background = self.forest.as_ref().map_or(lighting.background, |f| {
            f.background(lighting.background, sun.1)
        });
        for (index, pass) in self.graph.passes().iter().enumerate() {
            let pass_start = Instant::now();
            let _span = tracing::trace_span!("pass", name = pass.name).entered();
            let timestamps = self.timer.as_ref();
            let writes = timestamps.and_then(|t| t.render_writes(index));
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
                        timestamp_writes: writes,
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
                (passes::RIPPLES, PassKind::Render) => {
                    if let Some(water) = &mut self.water {
                        if water.ripples_pending() {
                            report.draws += water.encode_ripples(&mut encoder, writes);
                        } else {
                            // Nothing to draw or clear: keep the pass measurable.
                            drop(encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                                label: Some(passes::RIPPLES),
                                timestamp_writes: timestamps.and_then(|t| t.compute_writes(index)),
                            }));
                        }
                    }
                }
                (passes::CAUSTICS, PassKind::Render) => {
                    if let Some(water) = &self.water {
                        report.draws += water.encode_caustics(&mut encoder, writes);
                    }
                }
                (passes::FORWARD, PassKind::Render) => {
                    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(passes::FORWARD),
                        color_attachments: &[
                            Some(wgpu::RenderPassColorAttachment {
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
                                    // Multisampled colour is resolved in the pass (in tile
                                    // memory on Apple GPUs) and never stored.
                                    store: wgpu::StoreOp::Discard,
                                },
                            }),
                            Some(wgpu::RenderPassColorAttachment {
                                view: &self.targets.linear,
                                depth_slice: None,
                                resolve_target: Some(&self.targets.linear_resolved),
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: FAR_DEPTH,
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.0,
                                    }),
                                    store: wgpu::StoreOp::Discard,
                                },
                            }),
                        ],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.targets.depth,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(0.0),
                                store: wgpu::StoreOp::Discard,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: writes,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    if let Some(terrain) = &self.terrain {
                        report.draws += terrain.draw_forward(&mut rp, &self.frame_group);
                    }
                    if let Some(statics) = &self.statics {
                        report.draws += statics.draw_forward(&mut rp, &self.frame_group);
                    }
                    if let Some(forest) = &self.forest {
                        report.draws += forest.draw(&mut rp, &self.frame_group);
                    }
                    if let Some(water) = &self.water {
                        report.draws += water.draw(&mut rp, &self.frame_group);
                    }
                    if let Some(statics) = &self.statics {
                        report.draws += statics.draw_transparent(&mut rp, &self.frame_group);
                    }
                }
                (passes::CONTACT, PassKind::Render) => {
                    report.draws +=
                        self.post
                            .encode_contact(&mut encoder, &self.frame_group, writes);
                }
                (passes::BLOOM, PassKind::Render) => {
                    report.draws += self
                        .post
                        .encode_bloom(&mut encoder, &self.frame_group, writes);
                }
                (passes::HAZE, PassKind::Render) => {
                    report.draws += self
                        .post
                        .encode_haze(&mut encoder, &self.frame_group, writes);
                }
                (passes::POST, PassKind::Render) => {
                    report.draws +=
                        self.post
                            .encode_final(&mut encoder, &self.frame_group, output, writes);
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
