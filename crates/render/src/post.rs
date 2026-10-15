//! Post effects and the final pass (renderer §4.1, step 9; old `AtmospherePass` and
//! `BeautyPass`): contact shading and bloom at a small effect resolution, volumetric
//! haze at the quality's resolution, then one pass that upscales the scene from the
//! render scale and composites everything before tone mapping and grading.
//!
//! Every effect reads the resolved scene colour and linear depth of the forward pass.
//! Which effects run is the frame graph's decision (biome × quality); a disabled effect
//! has no target and its slot in the final pass binds a neutral 1×1 texture.

use crate::{
    camera::CameraView,
    environment::{Atmosphere, Bloom, Contact, DEFAULT_SHAFT_TINT, GRADE_SIZE},
    graph::FrameFeatures,
    quality::{HazeQuality, Quality, effect_size},
    targets::{HDR_FORMAT, Targets},
};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;

/// Size of the canopy openings texture (the canopy pattern's).
pub const OPENINGS_SIZE: u32 = 256;
/// Edge of the mist noise volume.
const NOISE_SIZE: u32 = 32;
const BLOOM_LEVELS: usize = 3;
const CONTACT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// How a biome's post effects look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostLook {
    pub bloom: Bloom,
    pub contact: Contact,
    pub atmosphere: Atmosphere,
}

impl PostLook {
    /// The effects this look uses under `quality`.
    pub fn features(&self, quality: &Quality, water: bool) -> FrameFeatures {
        FrameFeatures {
            water,
            contact: quality.contact_shadows && self.contact.strength > 0.0,
            bloom: quality.bloom && self.bloom.strength > 0.0,
            haze: quality.haze != HazeQuality::Off
                && self.atmosphere.enabled
                && (self.atmosphere.density > 0.0 || self.atmosphere.shaft_density > 0.0),
        }
    }
}

/// The old mist noise: a 32³ repeating lattice of hashed values (R8).
pub fn noise_volume() -> Vec<u8> {
    let n = NOISE_SIZE as usize;
    let fract = |v: f64| v - v.floor();
    let mut out = Vec::with_capacity(n * n * n);
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                let mut a = fract(x as f64 * 0.1031);
                let mut b = fract(y as f64 * 0.1031);
                let mut c = fract(z as f64 * 0.1031);
                let d = a * (b + 33.33) + b * (c + 33.33) + c * (a + 33.33);
                a += d;
                b += d;
                c += d;
                out.push((fract((a + b) * c) * 255.0).round() as u8);
            }
        }
    }
    out
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    inv_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    scene: [f32; 4],
    output: [f32; 4],
    projection: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PostUniform {
    params: [f32; 4],
    effects: [f32; 4],
    sizes: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BloomUniform {
    params: [f32; 4],
    texel: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct HazeUniform {
    fog: [f32; 4],
    sun: [f32; 4],
    shape: [f32; 4],
    noise: [f32; 4],
    wind: [f32; 4],
}

struct Target {
    view: wgpu::TextureView,
    size: [u32; 2],
}

fn target(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) -> Target {
    let view = device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default());
    Target { view, size }
}

#[allow(clippy::too_many_arguments)]
fn texture_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    format: wgpu::TextureFormat,
    size: [u32; 3],
    dimension: wgpu::TextureDimension,
    bytes: &[u8],
    texel: u32,
) -> wgpu::TextureView {
    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: size[2],
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * texel),
            rows_per_image: Some(size[1]),
        },
        extent,
    );
    texture.create_view(&Default::default())
}

/// Splits one pass's timestamps over a sequence of `count` render passes: the start on
/// the first, the end on the last.
fn part_of<'a>(
    writes: &Option<wgpu::RenderPassTimestampWrites<'a>>,
    index: usize,
    count: usize,
) -> Option<wgpu::RenderPassTimestampWrites<'a>> {
    writes.as_ref().and_then(|w| {
        let first = index == 0;
        let last = index + 1 == count;
        (first || last).then_some(wgpu::RenderPassTimestampWrites {
            query_set: w.query_set,
            beginning_of_pass_write_index: if first {
                w.beginning_of_pass_write_index
            } else {
                None
            },
            end_of_pass_write_index: if last {
                w.end_of_pass_write_index
            } else {
                None
            },
        })
    })
}

fn draw_fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    pipeline: &wgpu::RenderPipeline,
    frame: &wgpu::BindGroup,
    group: &wgpu::BindGroup,
) {
    let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
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
    rp.set_pipeline(pipeline);
    rp.set_bind_group(0, frame, &[]);
    rp.set_bind_group(1, group, &[]);
    rp.draw(0..3, 0..1);
}

/// Neutral stand-ins for effects that do not run.
struct Neutral {
    white: wgpu::TextureView,
    black: wgpu::TextureView,
    clear_air: wgpu::TextureView,
}

pub struct PostStack {
    composite_layout: wgpu::BindGroupLayout,
    composite_pipeline: wgpu::RenderPipeline,
    contact_layout: wgpu::BindGroupLayout,
    contact_pipeline: wgpu::RenderPipeline,
    bloom_layout: wgpu::BindGroupLayout,
    bloom_pipeline: wgpu::RenderPipeline,
    haze_layout: wgpu::BindGroupLayout,
    march_pipeline: wgpu::RenderPipeline,
    filter_pipeline: wgpu::RenderPipeline,
    view_buffer: wgpu::Buffer,
    post_buffer: wgpu::Buffer,
    contact_buffer: wgpu::Buffer,
    bloom_buffers: Vec<wgpu::Buffer>,
    haze_buffer: wgpu::Buffer,
    linear_sampler: wgpu::Sampler,
    grade: wgpu::TextureView,
    noise: wgpu::TextureView,
    openings: wgpu::Texture,
    neutral: Neutral,
    features: FrameFeatures,
    contact: Option<Target>,
    bloom: Vec<Target>,
    haze: Option<(Target, Target)>,
    haze_steps: u32,
    composite_group: wgpu::BindGroup,
    contact_group: Option<wgpu::BindGroup>,
    bloom_groups: Vec<wgpu::BindGroup>,
    haze_groups: Option<(wgpu::BindGroup, wgpu::BindGroup)>,
    /// Bytes of the effect targets.
    pub bytes: u64,
}

fn layout_entries(extra: &[(u32, wgpu::BindingType)]) -> Vec<wgpu::BindGroupLayoutEntry> {
    let frag = wgpu::ShaderStages::FRAGMENT;
    let mut entries = vec![
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: frag,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: frag,
            ty: tex(false, wgpu::TextureViewDimension::D2),
            count: None,
        },
    ];
    entries.extend(
        extra
            .iter()
            .map(|&(binding, ty)| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: frag,
                ty,
                count: None,
            }),
    );
    entries
}

fn tex(filterable: bool, view_dimension: wgpu::TextureViewDimension) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable },
        view_dimension,
        multisampled: false,
    }
}

fn uniform() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

const SAMPLER: wgpu::BindingType = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
const D2: wgpu::TextureViewDimension = wgpu::TextureViewDimension::D2;

impl PostStack {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output_format: wgpu::TextureFormat,
        frame_layout: &wgpu::BindGroupLayout,
        targets: &Targets,
        grade: &[u8],
        openings_data: &[u8],
    ) -> Self {
        let layout = |label, extra: &[(u32, wgpu::BindingType)]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &layout_entries(extra),
            })
        };
        let composite_layout = layout(
            "post",
            &[
                (2, tex(true, D2)),
                (3, tex(true, wgpu::TextureViewDimension::D3)),
                (4, SAMPLER),
                (5, uniform()),
                (6, tex(false, D2)),
                (7, tex(true, D2)),
                (8, tex(true, D2)),
                (9, tex(true, D2)),
                (10, tex(false, D2)),
            ],
        );
        let contact_layout = layout("contact", &[(2, uniform())]);
        let bloom_layout = layout("bloom", &[(2, uniform()), (3, tex(true, D2)), (4, SAMPLER)]);
        let haze_layout = layout(
            "haze",
            &[
                (2, uniform()),
                (3, tex(true, wgpu::TextureViewDimension::D3)),
                (4, tex(true, D2)),
                (5, tex(true, D2)),
                (6, SAMPLER),
            ],
        );
        let pipeline = |label: &str,
                        modules: &[crate::shader::Module],
                        source: &str,
                        entry: &str,
                        layout: &wgpu::BindGroupLayout,
                        format: wgpu::TextureFormat| {
            let module = crate::shader::module(device, label, modules, source);
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(frame_layout), Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        use crate::shader::{HAZE, Module, POST};
        let composite_pipeline = pipeline(
            "post",
            POST,
            include_str!("../shaders/post.wgsl"),
            "fs_main",
            &composite_layout,
            output_format,
        );
        let contact_pipeline = pipeline(
            "contact",
            &[Module::View],
            include_str!("../shaders/contact.wgsl"),
            "fs_main",
            &contact_layout,
            CONTACT_FORMAT,
        );
        let bloom_pipeline = pipeline(
            "bloom",
            &[Module::View],
            include_str!("../shaders/bloom.wgsl"),
            "fs_main",
            &bloom_layout,
            HDR_FORMAT,
        );
        let haze_source = include_str!("../shaders/haze.wgsl");
        let march_pipeline = pipeline(
            "haze march",
            HAZE,
            haze_source,
            "fs_march",
            &haze_layout,
            HDR_FORMAT,
        );
        let filter_pipeline = pipeline(
            "haze filter",
            HAZE,
            haze_source,
            "fs_filter",
            &haze_layout,
            HDR_FORMAT,
        );
        let buffer = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let d2 = wgpu::TextureDimension::D2;
        let neutral = Neutral {
            white: texture_with(
                device,
                queue,
                "no contact",
                CONTACT_FORMAT,
                [1, 1, 1],
                d2,
                &[255],
                1,
            ),
            black: texture_with(
                device,
                queue,
                "no bloom",
                HDR_FORMAT,
                [1, 1, 1],
                d2,
                &[0; 8],
                8,
            ),
            // Half-float (0, 0, 0, 1): nothing scattered, everything transmitted.
            clear_air: texture_with(
                device,
                queue,
                "no haze",
                HDR_FORMAT,
                [1, 1, 1],
                d2,
                &[0, 0, 0, 0, 0, 0, 0, 0x3c],
                8,
            ),
        };
        let noise = texture_with(
            device,
            queue,
            "mist noise",
            wgpu::TextureFormat::R8Unorm,
            [NOISE_SIZE; 3],
            wgpu::TextureDimension::D3,
            &noise_volume(),
            1,
        );
        let openings = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canopy openings"),
            size: wgpu::Extent3d {
                width: OPENINGS_SIZE,
                height: OPENINGS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: d2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let grade = Self::grade_texture(device, queue, grade);
        let view_buffer = buffer("post view", std::mem::size_of::<ViewUniform>());
        let post_buffer = buffer("post", std::mem::size_of::<PostUniform>());
        let effects = [
            &neutral.white,
            &neutral.black,
            &neutral.black,
            &neutral.black,
            &neutral.clear_air,
        ];
        let composite_group = Self::composite_bind(
            device,
            &composite_layout,
            &view_buffer,
            &post_buffer,
            targets,
            &grade,
            &linear_sampler,
            effects,
        );
        let mut stack = Self {
            composite_group,
            composite_layout,
            composite_pipeline,
            contact_layout,
            contact_pipeline,
            bloom_layout,
            bloom_pipeline,
            haze_layout,
            march_pipeline,
            filter_pipeline,
            view_buffer,
            post_buffer,
            contact_buffer: buffer("contact", 16),
            bloom_buffers: (0..BLOOM_LEVELS)
                .map(|_| buffer("bloom", std::mem::size_of::<BloomUniform>()))
                .collect(),
            haze_buffer: buffer("haze", std::mem::size_of::<HazeUniform>()),
            linear_sampler,
            grade,
            noise,
            openings,
            neutral,
            features: FrameFeatures::default(),
            contact: None,
            bloom: Vec::new(),
            haze: None,
            haze_steps: 0,
            contact_group: None,
            bloom_groups: Vec::new(),
            haze_groups: None,
            bytes: 0,
        };
        stack.set_openings(queue, openings_data);
        stack
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        view: &wgpu::Buffer,
        post: &wgpu::Buffer,
        targets: &Targets,
        grade: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        effects: [&wgpu::TextureView; 5],
    ) -> wgpu::BindGroup {
        let t = wgpu::BindingResource::TextureView;
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: t(&targets.linear_resolved),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: t(&targets.resolved),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: t(grade),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: post.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: t(effects[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: t(effects[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: t(effects[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: t(effects[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: t(effects[4]),
                },
            ],
        })
    }

    fn grade_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        volume: &[u8],
    ) -> wgpu::TextureView {
        texture_with(
            device,
            queue,
            "grade",
            wgpu::TextureFormat::Rgba8Unorm,
            [GRADE_SIZE; 3],
            wgpu::TextureDimension::D3,
            volume,
            4,
        )
    }

    /// Replaces the baked grade (call `configure` afterwards).
    pub fn set_grade(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, volume: &[u8]) {
        self.grade = Self::grade_texture(device, queue, volume);
    }

    pub fn set_openings(&mut self, queue: &wgpu::Queue, openings: &[u8]) {
        queue.write_texture(
            self.openings.as_image_copy(),
            openings,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(OPENINGS_SIZE),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: OPENINGS_SIZE,
                height: OPENINGS_SIZE,
                depth_or_array_layers: 1,
            },
        );
    }

    /// (Re)creates the effect targets for the scene targets and the effects that run,
    /// and every bind group.
    pub fn configure(
        &mut self,
        device: &wgpu::Device,
        targets: &Targets,
        features: FrameFeatures,
        haze: HazeQuality,
    ) {
        self.features = features;
        let small = effect_size(targets.size);
        self.bytes = 0;
        self.contact = features
            .contact
            .then(|| target(device, "contact", CONTACT_FORMAT, small));
        self.bloom = if features.bloom {
            (0..BLOOM_LEVELS)
                .map(|i| target(device, "bloom", HDR_FORMAT, small.map(|v| (v >> i).max(1))))
                .collect()
        } else {
            Vec::new()
        };
        self.haze = if features.haze {
            haze.size(targets.size).map(|size| {
                (
                    target(device, "haze", HDR_FORMAT, size),
                    target(device, "haze filtered", HDR_FORMAT, size),
                )
            })
        } else {
            None
        };
        self.haze_steps = haze.spec().map_or(0, |s| s.1);
        let px = |s: [u32; 2]| u64::from(s[0]) * u64::from(s[1]);
        self.bytes += self.contact.as_ref().map_or(0, |t| px(t.size));
        self.bytes += self.bloom.iter().map(|t| px(t.size) * 8).sum::<u64>();
        self.bytes += self.haze.as_ref().map_or(0, |t| px(t.0.size) * 16);

        let t = wgpu::BindingResource::TextureView;
        let view = self.view_buffer.as_entire_binding();
        let depth = || t(&targets.linear_resolved);
        self.contact_group = self.contact.as_ref().map(|_| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("contact"),
                layout: &self.contact_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: view.clone(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: depth(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.contact_buffer.as_entire_binding(),
                    },
                ],
            })
        });
        self.bloom_groups = (0..self.bloom.len())
            .map(|i| {
                let source = if i == 0 {
                    &targets.resolved
                } else {
                    &self.bloom[i - 1].view
                };
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bloom"),
                    layout: &self.bloom_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: view.clone(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: depth(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.bloom_buffers[i].as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: t(source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                        },
                    ],
                })
            })
            .collect();
        let openings = self.openings.create_view(&Default::default());
        self.haze_groups = self.haze.as_ref().map(|(raw, _)| {
            let group = |input: &wgpu::TextureView| {
                let t = wgpu::BindingResource::TextureView;
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("haze"),
                    layout: &self.haze_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: view.clone(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: depth(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.haze_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: t(&self.noise),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: t(&openings),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: t(input),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                        },
                    ],
                })
            };
            (group(&self.neutral.clear_air), group(&raw.view))
        });
        let n = &self.neutral;
        let effects = [
            self.contact.as_ref().map_or(&n.white, |c| &c.view),
            self.bloom.first().map_or(&n.black, |b| &b.view),
            self.bloom.get(1).map_or(&n.black, |b| &b.view),
            self.bloom.get(2).map_or(&n.black, |b| &b.view),
            self.haze.as_ref().map_or(&n.clear_air, |h| &h.1.view),
        ];
        self.composite_group = Self::composite_bind(
            device,
            &self.composite_layout,
            &self.view_buffer,
            &self.post_buffer,
            targets,
            &self.grade,
            &self.linear_sampler,
            effects,
        );
    }

    /// Writes this frame's values.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        camera: &CameraView,
        seconds: f32,
        scene: [u32; 2],
        output: [u32; 2],
        output_srgb: bool,
        look: &PostLook,
        grade: (f32, f32),
        quality: &Quality,
        sun: (Vec3, f32),
    ) {
        let size = |s: [u32; 2]| {
            let (w, h) = (s[0] as f32, s[1] as f32);
            [w, h, 1.0 / w, 1.0 / h]
        };
        let forward = (camera.target - camera.eye).normalize_or_zero();
        let view = ViewUniform {
            inv_view_proj: camera.view_projection.inverse().to_cols_array_2d(),
            eye: [camera.eye.x, camera.eye.y, camera.eye.z, seconds],
            forward: [forward.x, forward.y, forward.z, 0.0],
            scene: size(scene),
            output: size(output),
            projection: [camera.projection.y_axis.y, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&view));
        let f = self.features;
        let small = self.contact.as_ref().map_or([1, 1], |c| c.size);
        let haze_size = self.haze.as_ref().map_or([1, 1], |h| h.0.size);
        let post = PostUniform {
            params: [
                grade.0,
                grade.1,
                if output_srgb { 1.0 } else { 0.0 },
                if scene == output {
                    0.0
                } else {
                    quality.sharpen.clamp(0.0, 1.0)
                },
            ],
            effects: [
                if f.contact {
                    look.contact.strength
                } else {
                    0.0
                },
                if f.bloom { look.bloom.strength } else { 0.0 },
                if self.haze.is_some() { 1.0 } else { 0.0 },
                0.0,
            ],
            sizes: [
                small[0] as f32,
                small[1] as f32,
                haze_size[0] as f32,
                haze_size[1] as f32,
            ],
        };
        queue.write_buffer(&self.post_buffer, 0, bytemuck::bytes_of(&post));
        if f.contact {
            let c = &look.contact;
            queue.write_buffer(
                &self.contact_buffer,
                0,
                bytemuck::cast_slice(&[c.radius, c.bias, 0.0, 0.0]),
            );
        }
        if f.bloom {
            let b = &look.bloom;
            for (i, buffer) in self.bloom_buffers.iter().enumerate() {
                let source = if i == 0 {
                    scene
                } else {
                    self.bloom[i - 1].size
                };
                let uniform = BloomUniform {
                    params: [
                        b.threshold,
                        b.knee,
                        if i == 0 { 1.0 } else { b.radius * 2.0 },
                        if i == 0 { 1.0 } else { 0.0 },
                    ],
                    texel: [1.0 / source[0] as f32, 1.0 / source[1] as f32, 0.0, 0.0],
                };
                queue.write_buffer(buffer, 0, bytemuck::bytes_of(&uniform));
            }
        }
        if let Some((raw, _)) = &self.haze {
            let a = &look.atmosphere;
            let (sun_color, intensity) = sun;
            let tint = a.sun_tint.unwrap_or(DEFAULT_SHAFT_TINT).linear();
            let shaft = sun_color.lerp(Vec3::ONE, 0.6 * (intensity / 8.0).min(1.0))
                * tint
                * intensity.min(2.5);
            let fog = a.color.linear();
            let uniform = HazeUniform {
                fog: [fog.x, fog.y, fog.z, a.density],
                sun: [shaft.x, shaft.y, shaft.z, a.shaft_density],
                shape: [
                    a.base_height,
                    a.height_falloff,
                    a.sun_strength,
                    self.haze_steps as f32,
                ],
                noise: [a.noise_scale, a.noise_strength, a.drift_speed, 0.0],
                wind: [
                    a.wind[0],
                    a.wind[1],
                    1.0 / raw.size[0] as f32,
                    1.0 / raw.size[1] as f32,
                ],
            };
            queue.write_buffer(&self.haze_buffer, 0, bytemuck::bytes_of(&uniform));
        }
    }

    pub fn encode_contact(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::BindGroup,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let (Some(target), Some(group)) = (&self.contact, &self.contact_group) else {
            return 0;
        };
        draw_fullscreen(
            encoder,
            "contact",
            &target.view,
            timestamps,
            &self.contact_pipeline,
            frame,
            group,
        );
        1
    }

    pub fn encode_bloom(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::BindGroup,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let n = self.bloom.len();
        for (i, (target, group)) in self.bloom.iter().zip(&self.bloom_groups).enumerate() {
            draw_fullscreen(
                encoder,
                "bloom",
                &target.view,
                part_of(&timestamps, i, n),
                &self.bloom_pipeline,
                frame,
                group,
            );
        }
        n as u32
    }

    pub fn encode_haze(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::BindGroup,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let (Some((raw, filtered)), Some((march, filter))) = (&self.haze, &self.haze_groups) else {
            return 0;
        };
        draw_fullscreen(
            encoder,
            "haze march",
            &raw.view,
            part_of(&timestamps, 0, 2),
            &self.march_pipeline,
            frame,
            march,
        );
        draw_fullscreen(
            encoder,
            "haze filter",
            &filtered.view,
            part_of(&timestamps, 1, 2),
            &self.filter_pipeline,
            frame,
            filter,
        );
        2
    }

    /// The final pass into `output`.
    pub fn encode_final(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::BindGroup,
        output: &wgpu::TextureView,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        draw_fullscreen(
            encoder,
            "post",
            output,
            timestamps,
            &self.composite_pipeline,
            frame,
            &self.composite_group,
        );
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mist_noise_is_the_old_lattice() {
        let noise = noise_volume();
        assert_eq!(noise.len(), 32 * 32 * 32);
        // (0, 0, 0): a = b = c = 0 → d = 0 → fract(0) = 0.
        assert_eq!(noise[0], 0);
        // Spread over the whole range.
        let mean = noise.iter().map(|&v| f64::from(v)).sum::<f64>() / noise.len() as f64;
        assert!((100.0..155.0).contains(&mean), "{mean}");
    }
}
