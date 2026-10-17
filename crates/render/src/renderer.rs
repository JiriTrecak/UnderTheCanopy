//! The renderer: owns the frame graph's resources and records its passes in order.

use crate::{
    actors::{ActorInstance, ActorsGpu},
    camera::{CameraView, frustum_planes},
    device::Gpu,
    effects::{EffectFrame, EffectsGpu, MAX_EFFECT_LIGHTS},
    environment::{BiomeConfig, DaytimeLut, Lighting, LightingConfig},
    fog::FogGpu,
    frame::{self, FrameParams, FrameUniform, MAX_POINT_LIGHTS, PointLight},
    graph::{self, FrameGraph, PassKind, passes},
    minimap::{MinimapFrame, MinimapGpu},
    portrait::PortraitGpu,
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
    world_ui::{AtlasImage, WorldUiFrame, WorldUiGpu},
};
use anyhow::Result;
use glam::{Mat4, Vec3};
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

/// The model the selection portrait shows: a posed live model (its transform's scale
/// and pose are kept; the portrait places and frames it) and whether it is a building
/// (framed from higher up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortraitSubject {
    pub instance: ActorInstance,
    pub building: bool,
}

/// What is drawn.
#[derive(Debug, Clone, Default)]
pub struct World {
    pub terrain: Option<TerrainDesc>,
    pub statics: Vec<StaticInstance>,
    /// Glows by model entry.
    pub glows: std::collections::BTreeMap<String, crate::statics::Glow>,
    pub point_lights: Vec<PointLight>,
    /// Live models to load up front (pack entries; their order gives their indices).
    pub actor_models: Vec<String>,
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
    /// Pass timestamps attribute work to neighbouring passes on Apple GPUs (D34): use
    /// these for relative hints only, and [`FrameReport::gpu_frame_ms`] for budgets.
    pub gpu_ms: Option<Vec<f32>>,
    /// The frame's GPU time from submit to idle, ms, when this frame was sampled
    /// ([`Renderer::sample_gpu_time`]): the queue is drained first, then the frame is
    /// submitted and waited for. Trustworthy on every GPU; costs one stall.
    pub gpu_frame_ms: Option<f32>,
    /// CPU time of the whole `render` call, ms.
    pub cpu_total_ms: f32,
    pub draws: u32,
    pub terrain_triangles: u64,
    pub terrain_chunks: u32,
    /// Building foundations drawn (D44).
    pub foundations: u32,
    /// Live models of the frame.
    pub actors: crate::actors::ActorStats,
    /// Effects of the frame.
    pub effects: crate::effects::EffectStats,
    pub water: WaterStats,
    /// Triangles submitted, by kind (statics only when read back by a benchmark).
    pub triangles: Triangles,
    /// Submit-to-idle time of each pass recorded alone ([`Renderer::isolate_passes`]),
    /// ms; empty otherwise.
    pub isolated_ms: Vec<f32>,
}

/// Triangles a frame submits. Statics are culled on the GPU, so their counts are known
/// only when a benchmark reads the indirect arguments back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Triangles {
    pub terrain: u64,
    pub statics: u64,
    pub actors: u64,
    pub water: u64,
    /// Particles and ribbons (two per quad).
    pub effects: u64,
    /// Everything drawn into the sun's shadow map.
    pub shadow: u64,
}

impl Triangles {
    /// Triangles of the camera view (the forward pass).
    pub fn forward(&self) -> u64 {
        self.terrain + self.statics + self.actors + self.water + self.effects
    }
}

/// A square depth map the sun's shadow renders into and the lit passes sample.
fn shadow_map(device: &wgpu::Device, label: &str, size: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// What the frame bind groups hold besides the per-frame buffers.
struct FrameBindings {
    shadow_sampler: wgpu::Sampler,
    repeat_sampler: wgpu::Sampler,
    clamp_sampler: wgpu::Sampler,
    canopy_view: wgpu::TextureView,
    dummy_shadow: wgpu::TextureView,
    cutaway_view: wgpu::TextureView,
}

impl FrameBindings {
    #[allow(clippy::too_many_arguments)]
    fn group(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        label: &str,
        buffer: &wgpu::Buffer,
        lights: &wgpu::Buffer,
        shadow: &wgpu::TextureView,
        fog: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
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
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.canopy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.repeat_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: lights.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(fog),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&self.clamp_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&self.cutaway_view),
                },
            ],
        })
    }
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
    /// The world's lights; effect lights follow them each frame.
    map_lights: Vec<PointLight>,
    lights_dirty: bool,
    effects: EffectsGpu,
    effect_frame: EffectFrame,
    fog: FogGpu,
    /// The scenery cutaway's mask, its texture and this frame's units.
    cutaway: crate::cutaway::CutawayMask,
    cutaway_texture: wgpu::Texture,
    cutaway_subjects: Vec<crate::cutaway::CutawaySubject>,
    bindings: FrameBindings,
    world_ui: WorldUiGpu,
    world_ui_frame: WorldUiFrame,
    minimap: Option<MinimapGpu>,
    minimap_frame: MinimapFrame,
    portrait: Option<(PortraitGpu, wgpu::BindGroup)>,
    portrait_subject: Option<PortraitSubject>,
    /// Rendering the minimap's background: no fog, vignette or shadows.
    baking: bool,
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
    actors: Option<ActorsGpu>,
    water: Option<WaterGpu>,
    forest: Option<ForestGpu>,
    ripples: Ripples,
    ui: Option<crate::ui::UiLayer>,
    timer: Option<GpuTimer>,
    timestamps: bool,
    /// Submit every pass alone and wait for it (benchmarks: per-pass submit-to-idle
    /// times where Apple GPUs' timestamps attribute work to neighbouring passes).
    pub isolate_passes: bool,
    /// Measure the next frame's GPU time from submit to idle
    /// ([`FrameReport::gpu_frame_ms`]); cleared by `render`. The frame stalls the CPU
    /// until the GPU is done, so sample rarely (the game: once per stats interval).
    pub sample_gpu_time: bool,
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
        let shadow_resolution = quality.shadow_resolution(&environment.shadow);
        let shadow_view = shadow_map(device, "sun shadow", shadow_resolution);
        let dummy_shadow = shadow_map(device, "no shadow", 1);
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
        let clamp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let fog = FogGpu::new(device, queue);
        let cutaway_texture = crate::cutaway::texture(device);
        let bindings = FrameBindings {
            shadow_sampler,
            repeat_sampler,
            clamp_sampler,
            canopy_view,
            dummy_shadow,
            cutaway_view: cutaway_texture.create_view(&Default::default()),
        };
        let frame_group = bindings.group(
            device,
            &frame_layout,
            "frame",
            &frame_buffer,
            &light_buffer,
            &shadow_view,
            &fog.view,
        );
        let shadow_frame_group = bindings.group(
            device,
            &frame_layout,
            "shadow frame",
            &shadow_frame_buffer,
            &light_buffer,
            &bindings.dummy_shadow,
            &fog.view,
        );
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
        let effects = EffectsGpu::new(device, queue, gpu.features, &frame_layout);
        let world_ui = WorldUiGpu::new(device, queue, &frame_layout);
        let mut renderer = Self {
            graph,
            frame_layout,
            frame_buffer,
            shadow_frame_buffer,
            frame_group,
            shadow_frame_group,
            light_buffer,
            point_lights: 0,
            map_lights: Vec::new(),
            lights_dirty: false,
            effects,
            effect_frame: EffectFrame::default(),
            fog,
            cutaway: Default::default(),
            cutaway_texture,
            cutaway_subjects: Vec::new(),
            bindings,
            world_ui,
            world_ui_frame: WorldUiFrame::default(),
            minimap: None,
            minimap_frame: MinimapFrame::default(),
            portrait: None,
            portrait_subject: None,
            baking: false,
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
            actors: None,
            water: None,
            forest: None,
            ripples: Ripples::new(),
            ui: None,
            timer: None,
            timestamps: gpu.features.timestamps,
            isolate_passes: false,
            sample_gpu_time: false,
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
        let mut features = self
            .environment
            .post
            .features(&self.quality, self.water.is_some());
        features.minimap = self.minimap.is_some() && !self.baking;
        features.portrait = self.portrait.is_some() && !self.baking;
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

    /// Changes the quality settings: render scale, shadow map, levels of detail and post
    /// effects.
    pub fn set_quality(&mut self, gpu: &Gpu, quality: Quality) {
        self.quality = quality;
        let scene = quality.scene_size(self.output_size);
        if scene != self.targets.size {
            self.targets = Targets::new(&gpu.device, scene);
        }
        let resolution = quality.shadow_resolution(&self.environment.shadow);
        if resolution != self.shadow_resolution {
            self.shadow_resolution = resolution;
            self.shadow_view = shadow_map(&gpu.device, "sun shadow", resolution);
            self.frame_group = self.bindings.group(
                &gpu.device,
                &self.frame_layout,
                "frame",
                &self.frame_buffer,
                &self.light_buffer,
                &self.shadow_view,
                &self.fog.view,
            );
        }
        self.rebuild(gpu);
    }

    /// The game UI pass, created on first use for this renderer's output format.
    pub fn ui_mut(&mut self, gpu: &Gpu) -> &mut crate::ui::UiLayer {
        let format = self.output_format;
        self.ui
            .get_or_insert_with(|| crate::ui::UiLayer::new(gpu, format))
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

    /// Replaces only the light (the day cycle): no textures, grade or frame graph change.
    pub fn set_lighting(&mut self, lighting: crate::environment::Lighting) {
        self.environment.lighting = lighting;
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
        let mut actors = ActorsGpu::new(
            &gpu.device,
            &gpu.queue,
            gpu.features,
            &self.frame_layout,
            world.glows.clone(),
        );
        actors.sync_models(&gpu.device, &gpu.queue, pack, &world.actor_models);
        self.actors = Some(actors);
        // Room for the effect lights after the map's own.
        let lights = &world.point_lights[..world
            .point_lights
            .len()
            .min(MAX_POINT_LIGHTS - MAX_EFFECT_LIGHTS)];
        self.map_lights = lights.to_vec();
        self.lights_dirty = true;
        self.effect_frame.clear();
        self.rebuild(gpu);
        Ok(())
    }

    /// Loads the textures effect draws refer to by position (pack entry names).
    pub fn load_effect_textures(
        &mut self,
        gpu: &Gpu,
        pack: &canopy_assets::Pack,
        names: &[String],
    ) {
        let before = self.effects.bytes;
        self.effects
            .load_textures(&gpu.device, &gpu.queue, pack, names);
        self.world_bytes = self.world_bytes + self.effects.bytes - before.min(self.effects.bytes);
    }

    /// The effects of the next frame (particles, decals, ribbons, lights). Effect meshes
    /// are live models with a tint ([`Renderer::set_actors`]).
    pub fn set_effects(&mut self, frame: &mut EffectFrame) {
        if !frame.lights.is_empty() || !self.effect_frame.lights.is_empty() {
            self.lights_dirty = true;
        }
        std::mem::swap(&mut self.effect_frame, frame);
    }

    /// The effects of the latest frame.
    pub fn effects(&self) -> &EffectsGpu {
        &self.effects
    }

    /// The viewer's fog of war: `size²` cells (0 unexplored, 1 explored, 2 in sight) and,
    /// when known, the cells that changed since the previous call; `None` shows
    /// everything (observers).
    pub fn set_fog(&mut self, gpu: &Gpu, fog: Option<crate::fog::FogCells<'_>>) {
        let _span = tracing::trace_span!("fog_upload").entered();
        if self.fog.update(&gpu.device, &gpu.queue, fog) {
            self.frame_group = self.bindings.group(
                &gpu.device,
                &self.frame_layout,
                "frame",
                &self.frame_buffer,
                &self.light_buffer,
                &self.shadow_view,
                &self.fog.view,
            );
            self.shadow_frame_group = self.bindings.group(
                &gpu.device,
                &self.frame_layout,
                "shadow frame",
                &self.shadow_frame_buffer,
                &self.light_buffer,
                &self.bindings.dummy_shadow,
                &self.fog.view,
            );
            if let Some(minimap) = &mut self.minimap {
                minimap.rebind(&gpu.device, &self.fog.view);
            }
            if let Some((portrait, group)) = &mut self.portrait {
                *group = self.bindings.group(
                    &gpu.device,
                    &self.frame_layout,
                    "portrait frame",
                    &portrait.frame_buffer,
                    &self.light_buffer,
                    &self.bindings.dummy_shadow,
                    &self.fog.view,
                );
            }
        }
    }

    /// The fog of war on the GPU.
    pub fn fog(&self) -> &FogGpu {
        &self.fog
    }

    /// Renders the minimap's background: the world seen straight down over `map_size`
    /// cells into a `pixels`-square image. Call after `load_world`, before any live models
    /// or effects are set (they would be baked in). The HUD then shows
    /// [`Renderer::minimap_view`].
    /// Bakes the minimap's overview of `area` (x, z of a square's corner and its side, in
    /// cells: the playable area's square) on a map of `map_size` cells.
    pub fn bake_minimap(&mut self, gpu: &Gpu, map_size: u32, area: [f32; 3], pixels: u32) {
        let _span = tracing::info_span!("bake_minimap").entered();
        let (quality, size, shadows) = (self.quality, self.output_size, self.shadows);
        self.baking = true;
        self.shadows = false;
        self.set_quality(
            gpu,
            Quality {
                render_scale: 1.0,
                contact_shadows: false,
                bloom: false,
                haze: crate::HazeQuality::Off,
                ..quality
            },
        );
        self.resize(gpu, [pixels, pixels]);
        let base = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("minimap base"),
            size: wgpu::Extent3d {
                width: pixels,
                height: pixels,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.output_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = base.create_view(&Default::default());
        let half = area[2] * 0.5;
        let centre = Vec3::new(area[0] + half, 0.0, area[1] + half);
        let camera = crate::portrait::orthographic(
            centre + Vec3::Y * 2000.0,
            centre,
            Vec3::NEG_Z,
            half,
            half,
            4000.0,
        );
        self.render(gpu, &camera, 12.0, &view, None);
        self.shadows = shadows;
        self.baking = false;
        self.resize(gpu, size);
        let mut minimap = MinimapGpu::new(&gpu.device, pixels, view, &self.fog.view);
        let size = map_size.max(1) as f32;
        minimap.area = [area[0] / size, area[1] / size, area[2] / size];
        self.minimap = Some(minimap);
        self.world_bytes += u64::from(pixels) * u64::from(pixels) * 8;
        self.set_quality(gpu, quality);
    }

    /// The minimap's markers and camera footprint of the next frame.
    pub fn set_minimap(&mut self, frame: &mut MinimapFrame) {
        std::mem::swap(&mut self.minimap_frame, frame);
    }

    /// The minimap image (sRGB, premultiplied alpha) once baked.
    pub fn minimap_view(&self) -> Option<&wgpu::TextureView> {
        self.minimap.as_ref().map(|m| &m.view)
    }

    pub fn minimap_texture(&self) -> Option<&wgpu::Texture> {
        self.minimap.as_ref().map(MinimapGpu::texture)
    }

    /// The model the portrait shows from the next frame (`None`: no portrait drawn; the
    /// image keeps its last picture). The portrait image is `pixels` square and exists
    /// from the first call.
    pub fn set_portrait(&mut self, gpu: &Gpu, subject: Option<PortraitSubject>, pixels: u32) {
        if self.portrait.as_ref().is_none_or(|(p, _)| p.size != pixels) {
            let portrait = PortraitGpu::new(&gpu.device, pixels);
            let group = self.bindings.group(
                &gpu.device,
                &self.frame_layout,
                "portrait frame",
                &portrait.frame_buffer,
                &self.light_buffer,
                &self.bindings.dummy_shadow,
                &self.fog.view,
            );
            self.portrait = Some((portrait, group));
            self.rebuild(gpu);
        }
        self.portrait_subject = subject;
    }

    /// The portrait image (sRGB, premultiplied alpha) once a subject was set.
    pub fn portrait_view(&self) -> Option<&wgpu::TextureView> {
        self.portrait.as_ref().map(|(p, _)| &p.view)
    }

    pub fn portrait_texture(&self) -> Option<&wgpu::Texture> {
        self.portrait.as_ref().map(|(p, _)| p.texture())
    }

    /// The world UI of the next frame (ground marks and the overlay).
    /// The units giant canopy models open around this frame (old `cutawaySubjects`):
    /// the viewer's observed, living units; none for a close camera.
    pub fn set_cutaway(
        &mut self,
        subjects: impl IntoIterator<Item = crate::cutaway::CutawaySubject>,
    ) {
        self.cutaway_subjects.clear();
        self.cutaway_subjects.extend(subjects);
    }

    pub fn set_world_ui(&mut self, frame: &mut WorldUiFrame) {
        std::mem::swap(&mut self.world_ui_frame, frame);
    }

    /// The world UI's label glyphs (R8) and badge icons (sRGB RGBA8).
    pub fn set_world_ui_atlases(&mut self, gpu: &Gpu, glyphs: &AtlasImage, icons: &AtlasImage) {
        self.world_ui
            .set_atlases(&gpu.device, &gpu.queue, glyphs, icons);
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

    pub fn statics_mut(&mut self) -> Option<&mut StaticsGpu> {
        self.statics.as_mut()
    }

    pub fn actors(&self) -> Option<&ActorsGpu> {
        self.actors.as_ref()
    }

    /// Loads live models listed after those already loaded (the list only grows).
    pub fn sync_actor_models(&mut self, gpu: &Gpu, pack: &canopy_assets::Pack, names: &[String]) {
        if let Some(actors) = &mut self.actors {
            let before = actors.bytes;
            actors.sync_models(&gpu.device, &gpu.queue, pack, names);
            self.world_bytes = self.world_bytes + actors.bytes - before.min(actors.bytes);
        }
    }

    /// The live models of the next frame.
    pub fn set_actors(&mut self, instances: &[ActorInstance]) {
        if let Some(actors) = &mut self.actors {
            actors.set_instances(instances);
        }
    }

    /// The building foundations of the live scene (D44); only a changed list costs
    /// anything.
    pub fn set_foundations(&mut self, gpu: &Gpu, list: &[crate::terrain::foundations::Foundation]) {
        if let Some(terrain) = &mut self.terrain {
            terrain.set_foundations(&gpu.device, list);
        }
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
            texel: shadow_texel,
            ..
        } = shadow::fit(
            camera,
            camera.target.y,
            lighting.sun_direction,
            &crate::environment::ShadowConfig {
                resolution: self.shadow_resolution,
                ..self.environment.shadow
            },
        );
        if self.lights_dirty {
            let mut lights = self.map_lights.clone();
            lights.extend(
                self.effect_frame
                    .lights
                    .iter()
                    .take(MAX_EFFECT_LIGHTS)
                    .copied(),
            );
            if !lights.is_empty() {
                queue.write_buffer(&self.light_buffer, 0, bytemuck::cast_slice(&lights));
            }
            self.point_lights = lights.len() as u32;
            self.lights_dirty = false;
        }
        // The scenery cutaway follows this frame's units (none while baking the minimap).
        let subjects: &[_] = if self.baking {
            &[]
        } else {
            &self.cutaway_subjects
        };
        if let Some(rows) = self.cutaway.update(camera, subjects) {
            crate::cutaway::upload(queue, &self.cutaway_texture, &self.cutaway, rows);
        }
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
            fog: self
                .fog
                .raster
                .as_ref()
                .filter(|_| !self.baking)
                .map(|r| r.size()),
            cutaway: self.cutaway.active,
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
            report.foundations = stats.foundations;
            report.triangles.terrain = stats.triangles;
            report.triangles.shadow += stats.shadow_triangles;
        }
        if let Some(statics) = &self.statics {
            statics.prepare(queue, camera.view_projection, shadow_matrix);
        }
        if let Some(actors) = &mut self.actors {
            let subject = self
                .portrait_subject
                .as_ref()
                .filter(|_| self.portrait.is_some());
            let portrait = subject.and_then(|s| {
                let (min, max) = actors.rest_bounds(s.instance.model)?;
                let transform = Mat4::from_rotation_y(-0.32) * s.instance.transform;
                let corners = (0..8).map(|i| {
                    transform.transform_point3(Vec3::new(
                        if i & 1 == 0 { min.x } else { max.x },
                        if i & 2 == 0 { min.y } else { max.y },
                        if i & 4 == 0 { min.z } else { max.z },
                    ))
                });
                let (lo, hi) = corners.fold(
                    (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                    |(lo, hi), c| (lo.min(c), hi.max(c)),
                );
                Some((
                    crate::actors::ActorInstance {
                        transform,
                        ..s.instance
                    },
                    crate::portrait::frame(lo, hi, s.building, 1.0),
                ))
            });
            actors.set_portrait(portrait.as_ref().map(|p| p.0));
            if let (Some((_, camera)), Some((gpu_portrait, _))) = (&portrait, &self.portrait) {
                gpu_portrait.prepare(queue, camera, &self.environment.lighting);
            }
            let lod = crate::actors::LodView {
                view: camera.view,
                pixels_per_unit: self.targets.size[1] as f32 / (2.0 * (camera.fov_y * 0.5).tan()),
                max_pixels: if self.baking {
                    0.0
                } else {
                    self.quality.lod_pixels
                },
                shadow_texel,
            };
            actors.prepare(
                device,
                queue,
                camera.view_projection,
                self.shadows.then_some(shadow_matrix),
                Some(&lod),
            );
            report.actors = actors.stats;
            report.triangles.actors = actors.stats.triangles;
            report.triangles.shadow += actors.stats.shadow_triangles;
        }
        self.effects.prepare(queue, &self.effect_frame, camera);
        if let Some(minimap) = &mut self.minimap {
            minimap.prepare(queue, &self.minimap_frame, self.fog.enabled());
        }
        self.world_ui.prepare(
            queue,
            &self.world_ui_frame,
            self.output_size,
            self.output_format.is_srgb(),
        );
        report.effects = self.effects.stats;
        report.triangles.effects =
            2 * u64::from(self.effects.stats.parts + self.effects.stats.ribbons);
        if let Some(water) = &mut self.water {
            report.water = water.prepare(
                queue,
                camera,
                seconds,
                &self.ripples,
                lighting.daylight_ambient,
            );
            report.triangles.water = water.triangles();
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
            (
                if self.baking { 0.0 } else { grade.vignette },
                grade.highlight_shoulder,
            ),
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
                (passes::SKIN, PassKind::Compute) => {
                    let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some(passes::SKIN),
                        timestamp_writes: timestamps.and_then(|t| t.compute_writes(index)),
                    });
                    if let Some(actors) = &self.actors {
                        actors.skin(&mut compute);
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
                        if let Some(actors) = &self.actors {
                            report.draws += actors.draw_shadow(&mut rp, &self.shadow_frame_group);
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
                    if let Some(actors) = &self.actors {
                        report.draws += actors.draw_forward(&mut rp, &self.frame_group);
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
                    if let Some(actors) = &self.actors {
                        report.draws += actors.draw_transparent(&mut rp, &self.frame_group);
                    }
                    report.draws += self.world_ui.draw_ground(&mut rp, &self.frame_group);
                    report.draws += self.effects.draw(&mut rp, &self.frame_group);
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
                (passes::MINIMAP, PassKind::Render) => {
                    if let Some(minimap) = &self.minimap {
                        report.draws += minimap.encode(&mut encoder, writes);
                    }
                }
                (passes::PORTRAIT, PassKind::Render) => {
                    if let (Some((portrait, group)), Some(actors)) = (&self.portrait, &self.actors)
                        && actors.has_portrait()
                    {
                        report.draws += portrait
                            .encode(&mut encoder, writes, |rp| actors.draw_portrait(rp, group));
                    }
                }
                (passes::WORLD_UI, PassKind::Render) => {
                    // The minimap bake shows the world alone.
                    if !self.baking && (self.world_ui.has_overlay() || writes.is_some()) {
                        report.draws += self.world_ui.encode_overlay(
                            device,
                            &mut encoder,
                            output,
                            self.output_format,
                            writes,
                        );
                    }
                }
                // The minimap bake leaves the UI's list and target for the next frame.
                (passes::UI, PassKind::Render) => match self.ui.as_mut().filter(|_| !self.baking) {
                    Some(ui) => report.draws += ui.encode(&mut encoder, output, writes),
                    None => drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(passes::UI),
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
                        timestamp_writes: writes,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    })),
                },
                (name, _) => unreachable!("no recorder for pass {name}"),
            }
            report
                .cpu_ms
                .push(pass_start.elapsed().as_secs_f32() * 1000.0);
            if self.isolate_passes {
                let recorded = std::mem::replace(
                    &mut encoder,
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("frame"),
                    }),
                );
                let submitted = Instant::now();
                queue.submit([recorded.finish()]);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                report
                    .isolated_ms
                    .push(submitted.elapsed().as_secs_f32() * 1000.0);
            }
        }
        if let Some(timer) = &self.timer {
            timer.resolve(&mut encoder);
        }
        if let Some(extra) = before_submit {
            extra(&mut encoder);
        }
        let sample = std::mem::take(&mut self.sample_gpu_time);
        if sample {
            // Drain earlier frames, so the wait below measures this frame alone.
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
        let submitted = Instant::now();
        {
            let _span = tracing::trace_span!("submit").entered();
            queue.submit([encoder.finish()]);
        }
        let recorded = start.elapsed();
        if sample {
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            report.gpu_frame_ms = Some(submitted.elapsed().as_secs_f32() * 1000.0);
        }
        if let Some(timer) = &mut self.timer {
            timer.end_frame();
            report.gpu_ms = timer.latest().map(<[f32]>::to_vec);
        }
        // Recording and submitting, without a sampled frame's waits.
        report.cpu_total_ms = recorded.as_secs_f32() * 1000.0;
        report
    }
}
