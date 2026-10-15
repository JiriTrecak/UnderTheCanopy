//! Effects on the GPU (renderer §4.8): particles, decals and the pillar evaluated in the
//! vertex shader from compact emitter records (`shaders/effects.wgsl`), ribbons, and the
//! effect lights the caller passes with them. Meshes are live models
//! ([`crate::actors::ActorInstance::tint`]); sounds belong to `canopy-audio`.
//!
//! The caller (the scene) evaluates each live cue at the render time into one
//! [`Emitter`] and lists its parts ([`EffectFrame::parts`]) grouped into [`EffectDraw`]s
//! by geometry, blend and texture, so draw calls follow the textures and blends in view,
//! never the number of effects. Every buffer has a fixed budget.

use crate::{device::GpuFeatures, frame::PointLight, targets, texture};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};

/// Emitters (live effect layers) a frame may draw.
pub const MAX_EMITTERS: usize = 2048;
/// Particles a frame may draw (old player: 512 parts per player).
pub const MAX_PARTS: usize = 16384;
/// Ribbon segments a frame may draw.
pub const MAX_RIBBONS: usize = 4096;
/// Effect lights at once (old `EffectLights`: a fixed pool of 4).
pub const MAX_EFFECT_LIGHTS: usize = 4;
/// Parts of one emitter (the part index has 8 bits).
pub const MAX_EMITTER_PARTS: u32 = 256;

/// Emitter kinds (`ints[0]`), mirroring `K_*` in effects.wgsl.
pub mod kind {
    pub const BILLBOARD: u32 = 0;
    pub const BURST: u32 = 1;
    pub const STREAKS: u32 = 2;
    pub const EMITTER: u32 = 3;
    pub const RING: u32 = 4;
    pub const GLOW: u32 = 5;
    pub const PILLAR: u32 = 6;
    pub const MISSILE: u32 = 7;
    pub const WAVEFRONT: u32 = 8;
    pub const SPARK: u32 = 9;
    /// Flames of a burning building (D46).
    pub const FLAME: u32 = 10;
    /// Smoke over a burning building (D46).
    pub const SMOKE: u32 = 11;
    /// Dust of a melee hit (D46).
    pub const DUST: u32 = 12;
}

/// Emitter flags (`ints[1]`), mirroring `F_*` in effects.wgsl.
pub mod flag {
    pub const TEXTURED: u32 = 1;
    pub const GROUND: u32 = 2;
    pub const FRESNEL: u32 = 8;
    pub const SPIN: u32 = 16;
    pub const DISC: u32 = 32;
    pub const NORMAL: u32 = 64;
    pub const RANDOM_START: u32 = 128;
}

/// One live effect layer at the frame's time. Mirrors `Emitter` in effects.wgsl; see there
/// for each field.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct Emitter {
    pub position: [f32; 4],
    pub rotation: [f32; 4],
    pub color: [f32; 4],
    pub accent: [f32; 4],
    pub shape: [f32; 4],
    pub motion: [f32; 4],
    pub ints: [u32; 4],
    pub flip: [u32; 4],
    pub times: [f32; 4],
    pub e0: [f32; 4],
    pub e1: [f32; 4],
    pub e2: [f32; 4],
    pub e3: [f32; 4],
    pub e4: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<Emitter>() == 224);

/// A ribbon quad. Mirrors `Ribbon` in effects.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct RibbonQuad {
    pub p: [[f32; 4]; 4],
    /// rgb; a: 0 for a camera-facing trail segment (`p[0]`, `p[1]` with fades in w,
    /// width in `p[2].x`), 1 for an explicit quad (corners with alphas in w).
    pub color: [f32; 4],
    /// Opacity of a trail segment.
    pub look: [f32; 4],
}

impl RibbonQuad {
    /// A camera-facing trail segment from `a` to `b` with their fades.
    pub fn segment(a: [f32; 3], fade_a: f32, b: [f32; 3], fade_b: f32, width: f32) -> Self {
        Self {
            p: [
                [a[0], a[1], a[2], fade_a],
                [b[0], b[1], b[2], fade_b],
                [width, 0.0, 0.0, 0.0],
                [0.0; 4],
            ],
            ..Self::default()
        }
    }
}

/// Geometry of a draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Geometry {
    /// Quads: sprites, ground decals, soft spheres, rings, glows.
    Quad,
    /// The 24-sided open column.
    Pillar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blend {
    Additive,
    Normal,
}

/// A contiguous range of [`EffectFrame::parts`] drawn with one pipeline and texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectDraw {
    pub geometry: Geometry,
    pub blend: Blend,
    /// Index into the textures given to [`EffectsGpu::load_textures`]; `None` for
    /// untextured parts.
    pub texture: Option<u32>,
    pub first: u32,
    pub count: u32,
}

/// Everything the effects draw in one frame.
#[derive(Debug, Clone, Default)]
pub struct EffectFrame {
    pub emitters: Vec<Emitter>,
    /// `emitter << 8 | part`, grouped by `draws`.
    pub parts: Vec<u32>,
    pub draws: Vec<EffectDraw>,
    pub ribbons: Vec<RibbonQuad>,
    /// At most [`MAX_EFFECT_LIGHTS`] are used.
    pub lights: Vec<PointLight>,
}

impl EffectFrame {
    pub fn clear(&mut self) {
        self.emitters.clear();
        self.parts.clear();
        self.draws.clear();
        self.ribbons.clear();
        self.lights.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.ribbons.is_empty()
    }
}

/// Counts of the latest frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectStats {
    pub emitters: u32,
    pub parts: u32,
    pub ribbons: u32,
    pub lights: u32,
    pub draws: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    right: [f32; 4],
    up: [f32; 4],
}

pub struct EffectsGpu {
    features: GpuFeatures,
    pipelines: [[wgpu::RenderPipeline; 2]; 2],
    ribbon_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// One bind group per loaded texture, then the white fallback.
    textures: Vec<wgpu::BindGroup>,
    white: wgpu::BindGroup,
    emitters: wgpu::Buffer,
    parts: wgpu::Buffer,
    ribbons: wgpu::Buffer,
    view: wgpu::Buffer,
    group: wgpu::BindGroup,
    draws: Vec<EffectDraw>,
    ribbon_count: u32,
    pub stats: EffectStats,
    /// Bytes of texture memory.
    pub bytes: u64,
}

impl EffectsGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let vertex = wgpu::ShaderStages::VERTEX;
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vertex,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let data_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects"),
            entries: &[
                storage(0),
                storage(1),
                storage(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: vertex,
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
            "effects",
            &[crate::shader::Module::Frame],
            include_str!("../shaders/effects.wgsl"),
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects"),
            bind_group_layouts: &[
                Some(frame_layout),
                Some(&texture_layout),
                Some(&data_layout),
            ],
            immediate_size: 0,
        });
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let normal = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: additive.alpha,
        };
        let pipeline = |label, entry: &str, fs: &str, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(targets::scene_depth(false)),
                multisample: targets::scene_multisample(false),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &targets::forward_targets(Some(blend), false),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = [
            [
                pipeline("effect quads", "vs_quad", "fs_main", additive),
                pipeline("effect quads", "vs_quad", "fs_main", normal),
            ],
            [
                pipeline("effect pillar", "vs_pillar", "fs_main", additive),
                pipeline("effect pillar", "vs_pillar", "fs_main", normal),
            ],
        ];
        let ribbon_pipeline = pipeline("effect ribbons", "vs_ribbon", "fs_ribbon", additive);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effects"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white_view = texture::solid(
            device,
            queue,
            "effect white",
            wgpu::TextureFormat::Rgba8Unorm,
            [255; 4],
        )
        .create_view(&Default::default());
        let white = texture_group(device, &texture_layout, &white_view, &sampler);
        let buffer = |label, size: usize, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let storage_usage = wgpu::BufferUsages::STORAGE;
        let emitters = buffer(
            "effect emitters",
            MAX_EMITTERS * std::mem::size_of::<Emitter>(),
            storage_usage,
        );
        let parts = buffer("effect parts", MAX_PARTS * 4, storage_usage);
        let ribbons = buffer(
            "effect ribbons",
            MAX_RIBBONS * std::mem::size_of::<RibbonQuad>(),
            storage_usage,
        );
        let view = buffer(
            "effect view",
            std::mem::size_of::<ViewUniform>(),
            wgpu::BufferUsages::UNIFORM,
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects"),
            layout: &data_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: emitters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: parts.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: ribbons.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: view.as_entire_binding(),
                },
            ],
        });
        Self {
            features,
            pipelines,
            ribbon_pipeline,
            texture_layout,
            sampler,
            textures: Vec::new(),
            white,
            emitters,
            parts,
            ribbons,
            view,
            group,
            draws: Vec::new(),
            ribbon_count: 0,
            stats: EffectStats::default(),
            bytes: 0,
        }
    }

    /// Loads the effect textures by pack entry name; draws refer to them by position.
    /// A texture the pack lacks draws white (warned).
    pub fn load_textures(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &canopy_assets::Pack,
        names: &[String],
    ) {
        self.textures.clear();
        self.bytes = 0;
        for name in names {
            let loaded: Result<wgpu::TextureView> =
                pack.texture(name).with_context(|| name.clone()).map(|t| {
                    self.bytes += t.meta.mips.iter().map(|m| u64::from(m.len)).sum::<u64>();
                    texture::upload(device, queue, self.features, &t)
                        .create_view(&Default::default())
                });
            let group = match loaded {
                Ok(view) => texture_group(device, &self.texture_layout, &view, &self.sampler),
                Err(error) => {
                    tracing::warn!("effect texture not shown: {error:#}");
                    self.white.clone()
                }
            };
            self.textures.push(group);
        }
    }

    /// Uploads a frame's emitters, parts and ribbons (clipped to the budgets).
    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        frame: &EffectFrame,
        camera: &crate::CameraView,
    ) {
        let inverse = camera.view.inverse();
        let view = ViewUniform {
            right: inverse.x_axis.to_array(),
            up: inverse.y_axis.to_array(),
        };
        queue.write_buffer(&self.view, 0, bytemuck::bytes_of(&view));
        let emitters = &frame.emitters[..frame.emitters.len().min(MAX_EMITTERS)];
        if !emitters.is_empty() {
            queue.write_buffer(&self.emitters, 0, bytemuck::cast_slice(emitters));
        }
        let parts = &frame.parts[..frame.parts.len().min(MAX_PARTS)];
        if !parts.is_empty() {
            queue.write_buffer(&self.parts, 0, bytemuck::cast_slice(parts));
        }
        let ribbons = &frame.ribbons[..frame.ribbons.len().min(MAX_RIBBONS)];
        if !ribbons.is_empty() {
            queue.write_buffer(&self.ribbons, 0, bytemuck::cast_slice(ribbons));
        }
        self.ribbon_count = ribbons.len() as u32;
        self.draws.clear();
        let limit = parts.len() as u32;
        for d in &frame.draws {
            if d.first >= limit || (d.first as usize) >= MAX_PARTS {
                continue;
            }
            self.draws.push(EffectDraw {
                count: d.count.min(limit - d.first),
                ..*d
            });
        }
        self.stats = EffectStats {
            emitters: emitters.len() as u32,
            parts: parts.len() as u32,
            ribbons: self.ribbon_count,
            lights: frame.lights.len().min(MAX_EFFECT_LIGHTS) as u32,
            draws: self.draws.len() as u32 + u32::from(self.ribbon_count > 0),
        };
    }

    /// Draws the effects (inside the forward pass, after every other surface).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        if self.draws.is_empty() && self.ribbon_count == 0 {
            return 0;
        }
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(2, &self.group, &[]);
        let mut draws = 0;
        for d in &self.draws {
            let geometry = match d.geometry {
                Geometry::Quad => 0,
                Geometry::Pillar => 1,
            };
            let blend = match d.blend {
                Blend::Additive => 0,
                Blend::Normal => 1,
            };
            pass.set_pipeline(&self.pipelines[geometry][blend]);
            let texture = d
                .texture
                .and_then(|t| self.textures.get(t as usize))
                .unwrap_or(&self.white);
            pass.set_bind_group(1, texture, &[]);
            let vertices = match d.geometry {
                Geometry::Quad => 6,
                Geometry::Pillar => 24 * 6,
            };
            pass.draw(0..vertices, d.first..d.first + d.count);
            draws += 1;
        }
        if self.ribbon_count > 0 {
            pass.set_pipeline(&self.ribbon_pipeline);
            pass.set_bind_group(1, &self.white, &[]);
            pass.draw(0..6, 0..self.ribbon_count);
            draws += 1;
        }
        draws
    }
}

fn texture_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("effect texture"),
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
