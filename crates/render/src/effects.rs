//! Effects on the GPU (renderer §4.8): particles, decals and the pillar evaluated in the
//! vertex shader from compact emitter records (`shaders/effects.wgsl`), ribbons, and the
//! effect lights the caller passes with them. Meshes are live models
//! ([`crate::actors::ActorInstance::tint`]); sounds belong to `canopy-audio`.
//!
//! Effects draw in their own pass after the forward pass, into the resolved scene colour
//! without a depth attachment: every fragment reads the scene's linear depth instead, so
//! sprites hide behind surfaces and fade softly where they meet them (soft particles).
//! Every part blends premultiplied (`one, one − src alpha`) with its own additivity: 1
//! adds light, 0 covers what is behind, anything between does both. Curves over life
//! ([`EffectsGpu::load_curves`]) and the paths of moving emitters ([`EffectFrame::paths`])
//! sit in storage buffers beside the emitters.
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
/// Effect lights at once (old `EffectLights`: a fixed pool of 4; 8 since buildings light
/// their work, D65).
pub const MAX_EFFECT_LIGHTS: usize = 8;
/// Parts of one emitter (the part index has 8 bits).
pub const MAX_EMITTER_PARTS: u32 = 256;
/// Path samples a frame may use (moving world-space emitters).
pub const MAX_PATH_SAMPLES: usize = 16384;
/// Samples of a moving emitter's path.
pub const PATH_SAMPLES: u32 = 32;
/// Samples per curve row; a layer's curve block is [`CURVE_ROWS`] rows.
pub const CURVE_SAMPLES: u32 = 32;
/// Rows of a curve block: colour and opacity; size, additivity, erosion and heat; ramp.
pub const CURVE_ROWS: u32 = 3;
/// No curve block.
pub const NO_CURVES: u32 = u32::MAX;

/// Which curves of a block a layer authored (`ints2[1]`), mirroring `C_*` in
/// effects.wgsl; the others keep the emitter's linear values.
pub mod curve {
    pub const COLOUR: u32 = 1;
    pub const OPACITY: u32 = 2;
    pub const SIZE: u32 = 4;
    pub const ADDITIVE: u32 = 8;
    pub const EROSION: u32 = 16;
    pub const HEAT: u32 = 32;
    pub const RAMP: u32 = 64;
}

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
    /// Falling rain shards: one part per drop.
    pub const RAIN: u32 = 13;
    /// Splash fragments where rain lands: `impact count` parts per drop.
    pub const RAIN_SPLASH: u32 = 14;
    /// The flash on the ground where a drop lands: one part per drop.
    pub const RAIN_FLASH: u32 = 15;
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
    /// The flipbook spans each particle's life.
    pub const OVER_LIFE: u32 = 256;
    /// Flipbook frames cross-fade.
    pub const FLIP_BLEND: u32 = 512;
    /// Particles stay where they were born, on the emitter's path.
    pub const WORLD: u32 = 1024;
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
    /// Every kind: white-hot core (0–1), streak seconds along the velocity, 0, 0.
    pub e5: [f32; 4],
    /// Curve block ([`NO_CURVES`]: none), [`curve`] flags, first path sample, samples.
    pub ints2: [u32; 4],
    /// Seconds of the first path sample, seconds between samples, inherited velocity,
    /// soft fade (m).
    pub e6: [f32; 4],
    /// Turbulence strength (m), cell (m), churn (1/s); landing friction (< 0: none).
    pub e7: [f32; 4],
    /// Inner cone (rad), inner spawn share, erosion softness, ground height (world y).
    pub e8: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<Emitter>() == 304);

impl Emitter {
    /// No curves, no path, no landing, the default softness.
    pub const NONE_EXTRA: ([u32; 4], [f32; 4], [f32; 4], [f32; 4]) = (
        [NO_CURVES, 0, 0, 0],
        [0.0, 0.0, 0.0, 0.4],
        [0.0, 1.0, 0.0, -1.0],
        [0.0, 0.0, 0.1, -1.0e9],
    );

    /// An emitter with nothing but the extras set to their neutral values.
    pub fn neutral() -> Self {
        let (ints2, e6, e7, e8) = Self::NONE_EXTRA;
        Self {
            ints2,
            e6,
            e7,
            e8,
            ..Self::default()
        }
    }
}

/// A ribbon quad. Mirrors `Ribbon` in effects.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Pod, Zeroable)]
pub struct RibbonQuad {
    pub p: [[f32; 4]; 4],
    /// rgb; a: the mode. 0: a camera-facing trail segment (`p[0]`, `p[1]` with fades in
    /// w, width in `p[2].x`); 1: an explicit quad (corners with alphas in w); 2: a
    /// textured strip segment ([`RibbonQuad::strip`]).
    pub color: [f32; 4],
    /// Mode 0: opacity. Mode 2: opacity at `a`, additivity, soft fade (m), 0.
    pub look: [f32; 4],
    /// Mode 2: colour and opacity at `b`.
    pub end: [f32; 4],
}

/// One point of a textured strip.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StripPoint {
    pub position: [f32; 3],
    /// Unit direction of the strip here.
    pub tangent: [f32; 3],
    pub width: f32,
    /// Texture coordinate along the strip.
    pub u: f32,
    pub color: [f32; 3],
    pub opacity: f32,
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

    /// A textured strip segment from `a` to `b`, facing the camera across its tangents.
    pub fn strip(a: &StripPoint, b: &StripPoint, additive: f32, soft: f32) -> Self {
        let t = |p: &StripPoint, u: f32| [p.tangent[0], p.tangent[1], p.tangent[2], u];
        Self {
            p: [
                [a.position[0], a.position[1], a.position[2], a.width],
                [b.position[0], b.position[1], b.position[2], b.width],
                t(a, a.u),
                t(b, b.u),
            ],
            color: [a.color[0], a.color[1], a.color[2], 2.0],
            look: [a.opacity, additive, soft, 0.0],
            end: [b.color[0], b.color[1], b.color[2], b.opacity],
        }
    }
}

/// A contiguous range of [`EffectFrame::ribbons`] drawn with one texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonDraw {
    pub texture: Option<u32>,
    pub first: u32,
    pub count: u32,
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
    /// Ribbons grouped by texture (ribbons outside every group draw untextured, first).
    pub ribbon_draws: Vec<RibbonDraw>,
    /// Path samples of moving emitters (xyz world, w unused).
    pub paths: Vec<[f32; 4]>,
    /// At most [`MAX_EFFECT_LIGHTS`] are used.
    pub lights: Vec<PointLight>,
}

impl EffectFrame {
    pub fn clear(&mut self) {
        self.emitters.clear();
        self.parts.clear();
        self.draws.clear();
        self.ribbons.clear();
        self.ribbon_draws.clear();
        self.paths.clear();
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
    /// Quads and the pillar; every part blends premultiplied with its own additivity.
    pipelines: [wgpu::RenderPipeline; 2],
    ribbon_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    data_layout: wgpu::BindGroupLayout,
    depth_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// One bind group per loaded texture, then the white fallback.
    textures: Vec<wgpu::BindGroup>,
    white: wgpu::BindGroup,
    emitters: wgpu::Buffer,
    parts: wgpu::Buffer,
    ribbons: wgpu::Buffer,
    paths: wgpu::Buffer,
    curves: wgpu::Buffer,
    view: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// The scene's linear depth ([`EffectsGpu::bind_depth`]).
    depth: Option<wgpu::BindGroup>,
    draws: Vec<EffectDraw>,
    ribbon_draws: Vec<RibbonDraw>,
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
        let storage = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
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
                storage(0, vertex),
                storage(1, vertex),
                storage(2, vertex),
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
                storage(4, vertex | frag),
                storage(5, vertex),
            ],
        });
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: frag,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
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
                Some(&depth_layout),
            ],
            immediate_size: 0,
        });
        // Premultiplied: the shader's alpha is coverage × (1 − additivity), so additive
        // parts leave what is behind them and alpha-blended parts cover it. The target's
        // alpha is kept.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = |label, entry: &str, fs: &str| {
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
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: targets::HDR_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = [
            pipeline("effect quads", "vs_quad", "fs_main"),
            pipeline("effect pillar", "vs_pillar", "fs_main"),
        ];
        let ribbon_pipeline = pipeline("effect ribbons", "vs_ribbon", "fs_ribbon");
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
        let paths = buffer("effect paths", MAX_PATH_SAMPLES * 16, storage_usage);
        let curves = buffer("effect curves", 16, storage_usage);
        let view = buffer(
            "effect view",
            std::mem::size_of::<ViewUniform>(),
            wgpu::BufferUsages::UNIFORM,
        );
        let group = data_group(
            device,
            &data_layout,
            [&emitters, &parts, &ribbons, &view, &curves, &paths],
        );
        Self {
            features,
            pipelines,
            ribbon_pipeline,
            texture_layout,
            data_layout,
            depth_layout,
            sampler,
            textures: Vec::new(),
            white,
            emitters,
            parts,
            ribbons,
            paths,
            curves,
            view,
            group,
            depth: None,
            draws: Vec::new(),
            ribbon_draws: Vec::new(),
            ribbon_count: 0,
            stats: EffectStats::default(),
            bytes: 0,
        }
    }

    /// Binds the scene's resolved linear depth (`targets::Targets::linear_resolved`);
    /// again whenever the targets are remade.
    pub fn bind_depth(&mut self, device: &wgpu::Device, linear_depth: &wgpu::TextureView) {
        self.depth = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effect depth"),
            layout: &self.depth_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(linear_depth),
            }],
        }));
    }

    /// Loads the effects' curve blocks ([`CURVE_ROWS`] × [`CURVE_SAMPLES`] samples each,
    /// emitters refer to them by block).
    pub fn load_curves(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, curves: &[[f32; 4]]) {
        let size = (curves.len().max(1) * 16) as u64;
        self.curves = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effect curves"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !curves.is_empty() {
            queue.write_buffer(&self.curves, 0, bytemuck::cast_slice(curves));
        }
        self.group = data_group(
            device,
            &self.data_layout,
            [
                &self.emitters,
                &self.parts,
                &self.ribbons,
                &self.view,
                &self.curves,
                &self.paths,
            ],
        );
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

    /// Uploads a frame's emitters, parts, ribbons and paths (clipped to the budgets).
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
        let paths = &frame.paths[..frame.paths.len().min(MAX_PATH_SAMPLES)];
        if !paths.is_empty() {
            queue.write_buffer(&self.paths, 0, bytemuck::cast_slice(paths));
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
        // Ribbons outside every group draw untextured, first.
        self.ribbon_draws.clear();
        let grouped_from = frame
            .ribbon_draws
            .iter()
            .map(|d| d.first)
            .min()
            .unwrap_or(self.ribbon_count)
            .min(self.ribbon_count);
        if grouped_from > 0 {
            self.ribbon_draws.push(RibbonDraw {
                texture: None,
                first: 0,
                count: grouped_from,
            });
        }
        for d in &frame.ribbon_draws {
            if d.first >= self.ribbon_count {
                continue;
            }
            self.ribbon_draws.push(RibbonDraw {
                count: d.count.min(self.ribbon_count - d.first),
                ..*d
            });
        }
        self.stats = EffectStats {
            emitters: emitters.len() as u32,
            parts: parts.len() as u32,
            ribbons: self.ribbon_count,
            lights: frame.lights.len().min(MAX_EFFECT_LIGHTS) as u32,
            draws: (self.draws.len() + self.ribbon_draws.len()) as u32,
        };
    }

    /// Whether the latest prepared frame draws anything (the effects pass is skipped
    /// otherwise).
    pub fn has_draws(&self) -> bool {
        !self.draws.is_empty() || !self.ribbon_draws.is_empty()
    }

    /// Draws the effects (in their own pass over the resolved scene colour, after the
    /// forward pass).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        let Some(depth) = &self.depth else { return 0 };
        if !self.has_draws() {
            return 0;
        }
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(2, &self.group, &[]);
        pass.set_bind_group(3, depth, &[]);
        let mut draws = 0;
        for d in &self.draws {
            let (pipeline, vertices) = match d.geometry {
                Geometry::Quad => (&self.pipelines[0], 6),
                Geometry::Pillar => (&self.pipelines[1], 24 * 6),
            };
            pass.set_pipeline(pipeline);
            let texture = d
                .texture
                .and_then(|t| self.textures.get(t as usize))
                .unwrap_or(&self.white);
            pass.set_bind_group(1, texture, &[]);
            pass.draw(0..vertices, d.first..d.first + d.count);
            draws += 1;
        }
        if !self.ribbon_draws.is_empty() {
            pass.set_pipeline(&self.ribbon_pipeline);
            for d in &self.ribbon_draws {
                let texture = d
                    .texture
                    .and_then(|t| self.textures.get(t as usize))
                    .unwrap_or(&self.white);
                pass.set_bind_group(1, texture, &[]);
                pass.draw(0..6, d.first..d.first + d.count);
                draws += 1;
            }
        }
        draws
    }
}

fn data_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: [&wgpu::Buffer; 6],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("effects"),
        layout,
        entries: &entries,
    })
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
