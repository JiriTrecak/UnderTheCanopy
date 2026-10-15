//! Live models: units, buildings, items, felled trees, corpses, construction scaffolds,
//! carried goods and projectiles — everything that moves, animates or changes during a
//! match (renderer §4.5).
//!
//! The caller lists the models once (`sync_models`, append-only: new entries load on
//! demand) and hands over the frame's posed instances (`set_instances`). Each frame the
//! instances are culled on the CPU against the camera and the sun and sorted by model,
//! camera-visible first; a compute pass ([`skin`](ActorsGpu::skin)) blends each one's
//! joint palette from the baked clips (Q13: model-space 3×4 matrices at 30 Hz, up to three
//! poses each between two frames); then one instanced draw per model section reads its
//! contiguous range. Draw calls depend on the models in view, never on how many units use
//! them. Rigid models skin with a single identity joint.

use crate::{
    camera::frustum_planes,
    device::GpuFeatures,
    material::{GpuMaterial, Textures, Variant},
    statics::{Glow, SHADOW_MIN_HEIGHT, sphere_visible, world_sphere},
    targets,
};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use canopy_assets::{CLIP_FRAME_RATE, IndexFormat, Pack, SkinVertex};
use glam::{Mat4, Vec3};
use std::collections::{BTreeMap, HashMap};
use wgpu::util::DeviceExt;

/// Poses blended per instance: the current clip and two fading ones (old baked crowd).
pub const MAX_POSES: usize = 3;
const MODEL_STRIDE: u64 = 256;
const SKIN_GROUP: u32 = 64;
/// Levels of detail a model may have: the full mesh and the cooker's coarser ones.
pub const MAX_LODS: usize = 5;
/// Shadow-map texels a shadow caster's level of detail may deviate by (the PCF filter
/// spreads every edge over several texels).
const SHADOW_LOD_TEXELS: f32 = 1.0;

/// How the frame chooses levels of detail: the coarsest level whose deviation, projected
/// at the instance's view depth, stays within `max_pixels`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodView {
    /// World → view (looks down −z).
    pub view: Mat4,
    /// Pixels a world unit covers at view depth 1 (`height / (2 tan(fov_y / 2))`).
    pub pixels_per_unit: f32,
    /// Largest projected deviation allowed, pixels; 0 draws every model in full.
    pub max_pixels: f32,
    /// World units per shadow-map texel.
    pub shadow_texel: f32,
}

/// One level of detail of a loaded model: index ranges over the full mesh's vertices.
struct ModelLod {
    /// Largest deviation from the full mesh, model units (0 for the full mesh).
    error: f32,
    /// First index and count per section.
    sections: Vec<(u32, u32)>,
    /// First index and count in the depth stream.
    depth: (u32, u32),
}

/// One sampled clip of a posed instance.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pose {
    /// Clip index in the model.
    pub clip: u16,
    /// Seconds into the clip (clamped to its length).
    pub time: f32,
    /// Blend weight; the poses of an instance are normalised.
    pub weight: f32,
}

/// A live model placed and posed for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorInstance {
    /// Index into the list given to [`ActorsGpu::sync_models`].
    pub model: u32,
    /// Model space → world (affine, may scale non-uniformly).
    pub transform: Mat4,
    /// The owner's linear colour for team-coloured materials.
    pub owner_color: Option<[f32; 3]>,
    /// 1 opaque; lower values dissolve with a screen-door pattern (concealment).
    pub opacity: f32,
    pub poses: [Pose; MAX_POSES],
    /// Poses in use; 0 draws the bind pose.
    pub pose_count: u8,
    /// Multiplies the shaded colour (rgb) and coverage (a). Effect models below full
    /// coverage blend over the scene and cast no shadow (old effect model materials).
    pub tint: [f32; 4],
}

/// The tint of models drawn as they are.
pub const NO_TINT: [f32; 4] = [1.0; 4];

impl ActorInstance {
    /// A model in its rest pose.
    pub fn rest(model: u32, transform: Mat4) -> Self {
        Self {
            model,
            transform,
            owner_color: None,
            opacity: 1.0,
            poses: [Pose::default(); MAX_POSES],
            pose_count: 0,
            tint: NO_TINT,
        }
    }

    /// Blends over the scene instead of drawing opaque.
    pub fn translucent(&self) -> bool {
        self.tint[3] < 0.999
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ModelUniform {
    position_min: [f32; 4],
    position_extent: [f32; 4],
    uv: [f32; 4],
    wind: [f32; 4],
}

/// Mirrors `Actor` in actors.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ActorGpu {
    rows: [[f32; 4]; 3],
    color: [f32; 4],
    palette: [u32; 4],
    look: [f32; 4],
    tint: [f32; 4],
}

/// Mirrors `Job` in skin.wgsl.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct SkinJob {
    header: [u32; 4],
    poses: [[u32; 4]; MAX_POSES],
}

struct ClipRange {
    /// First matrix of frame 0, joint 0, in the clip buffer.
    base: u32,
    frames: u32,
    duration: f32,
}

struct GpuModel {
    name: String,
    vertices: wgpu::Buffer,
    colors: wgpu::Buffer,
    leaves: wgpu::Buffer,
    skin: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_format: wgpu::IndexFormat,
    depth: Option<DepthBuffers>,
    sections: Vec<canopy_assets::Section>,
    /// Level 0 is the full mesh.
    lods: Vec<ModelLod>,
    materials: Vec<GpuMaterial>,
    masked_shadow_sections: Vec<usize>,
    joints: u32,
    clips: Vec<ClipRange>,
    /// Bounding sphere over every frame (model space).
    sphere: (Vec3, f32),
    /// Rest height, for the shadow height rule of rigid models.
    height: f32,
    /// Bind-pose bounds.
    rest: (Vec3, Vec3),
    animated: bool,
    casts: bool,
}

struct DepthBuffers {
    positions: wgpu::Buffer,
    skin: wgpu::Buffer,
    indices: wgpu::Buffer,
    format: wgpu::IndexFormat,
}

/// A loaded model or one that could not be loaded.
enum Slot {
    Ready(Box<GpuModel>),
    Missing,
}

/// The frame's draw range of one model: camera-visible opaque instances, then those only
/// the sun sees (`all` covers both: the shadow casters), then translucent ones.
#[derive(Debug, Clone, Copy, Default)]
struct Range {
    first: u32,
    camera: u32,
    all: u32,
    translucent: u32,
    /// Camera-visible opaque instances per level of detail, in order (they partition
    /// `camera`).
    lods: [u32; MAX_LODS],
    /// The level the shadow map draws every caster of the model at.
    shadow_lod: u8,
    /// Largest scale of the model's shadow casters.
    shadow_scale: f32,
}

/// Counts of the live scene in the latest frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActorStats {
    pub models: u32,
    pub instances: u32,
    pub visible: u32,
    pub shadow_casters: u32,
    pub joints: u32,
    pub draws: u32,
    /// Triangles drawn for the camera (opaque and translucent) and into the shadow map.
    pub triangles: u64,
    pub shadow_triangles: u64,
}

/// A growable GPU buffer.
struct Grow {
    buffer: wgpu::Buffer,
    capacity: u64,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl Grow {
    fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        size: u64,
    ) -> Self {
        let capacity = size.max(256).next_power_of_two();
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage,
                mapped_at_creation: false,
            }),
            capacity,
            label,
            usage,
        }
    }

    /// Makes room for `size` bytes; true when the buffer was replaced (contents lost).
    fn reserve(&mut self, device: &wgpu::Device, size: u64) -> bool {
        if size <= self.capacity {
            return false;
        }
        *self = Self::new(device, self.label, self.usage, size);
        true
    }
}

pub struct ActorsGpu {
    textures: Textures,
    slots: Vec<Slot>,
    names: Vec<String>,
    glows: BTreeMap<String, Glow>,
    pipelines: HashMap<Variant, wgpu::RenderPipeline>,
    depth_pipeline: wgpu::RenderPipeline,
    masked_pipeline: wgpu::RenderPipeline,
    skin_pipeline: wgpu::ComputePipeline,
    actor_layout: wgpu::BindGroupLayout,
    skin_layout: wgpu::BindGroupLayout,
    /// Baked clip matrices of every loaded model; matrix 0 is the identity.
    clips: Grow,
    clip_len: u32,
    models: Grow,
    actors: Grow,
    jobs: Grow,
    palette: Grow,
    actor_group: Option<wgpu::BindGroup>,
    skin_group: Option<wgpu::BindGroup>,
    /// The caller's instances.
    instances: Vec<ActorInstance>,
    ranges: Vec<Range>,
    job_count: u32,
    max_joints: u32,
    /// Bytes of GPU memory in models and textures.
    pub bytes: u64,
    pub stats: ActorStats,
    staging_actors: Vec<ActorGpu>,
    staging_jobs: Vec<SkinJob>,
    /// The portrait's model, drawn only by the portrait pass.
    portrait: Option<ActorInstance>,
    /// Its model and record index in the latest frame.
    portrait_slot: Option<(usize, u32)>,
}

fn wgpu_index(format: IndexFormat) -> wgpu::IndexFormat {
    match format {
        IndexFormat::U16 => wgpu::IndexFormat::Uint16,
        IndexFormat::U32 => wgpu::IndexFormat::Uint32,
    }
}

const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

impl ActorsGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        frame_layout: &wgpu::BindGroupLayout,
        glows: BTreeMap<String, Glow>,
    ) -> Self {
        let textures = Textures::new(device, queue, features);
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let storage = |binding, visibility, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let actor_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("actors"),
            entries: &[
                storage(0, vf, true),
                storage(1, wgpu::ShaderStages::VERTEX, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: vf,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<ModelUniform>() as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });
        let comp = wgpu::ShaderStages::COMPUTE;
        let skin_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin"),
            entries: &[
                storage(0, comp, true),
                storage(1, comp, true),
                storage(2, comp, false),
            ],
        });
        let module = crate::shader::module(
            device,
            "actors",
            crate::shader::MODEL,
            include_str!("../shaders/actors.wgsl"),
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("actors"),
            bind_group_layouts: &[
                Some(frame_layout),
                Some(textures.layout()),
                Some(&actor_layout),
            ],
            immediate_size: 0,
        });
        let main_attributes =
            wgpu::vertex_attr_array![0 => Unorm16x4, 1 => Snorm16x2, 2 => Unorm16x2];
        let color_attributes = wgpu::vertex_attr_array![3 => Unorm8x4];
        let leaf_attributes = wgpu::vertex_attr_array![4 => Unorm16x4];
        let skin_attributes = wgpu::vertex_attr_array![5 => Uint8x4, 6 => Unorm8x4];
        let vertex = |stride, attributes| {
            Some(wgpu::VertexBufferLayout {
                array_stride: stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes,
            })
        };
        let main_buffers = [
            vertex(16, &main_attributes[..]),
            vertex(4, &color_attributes[..]),
            vertex(8, &leaf_attributes[..]),
            vertex(8, &skin_attributes[..]),
        ];
        let mut pipelines = HashMap::new();
        for variant in Variant::ALL {
            let (cull_mode, a2c, blend) = variant.states();
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("actors"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &main_buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(targets::scene_depth(blend.is_none())),
                multisample: targets::scene_multisample(a2c),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &targets::forward_targets(blend, blend.is_none()),
                }),
                multiview_mask: None,
                cache: None,
            });
            pipelines.insert(variant, pipeline);
        }
        let depth_position = wgpu::vertex_attr_array![0 => Unorm16x4];
        let depth_skin = wgpu::vertex_attr_array![5 => Uint8x4, 6 => Unorm8x4];
        let depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actors depth"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_depth"),
                compilation_options: Default::default(),
                buffers: &[vertex(8, &depth_position[..]), vertex(8, &depth_skin[..])],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(targets::shadow_depth()),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let masked_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actors masked depth"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_masked"),
                compilation_options: Default::default(),
                buffers: &main_buffers,
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(targets::shadow_depth()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_masked"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        });
        let skin_module =
            crate::shader::module(device, "skin", &[], include_str!("../shaders/skin.wgsl"));
        let skin_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skin"),
            bind_group_layouts: &[Some(&skin_layout)],
            immediate_size: 0,
        });
        let skin_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("skin"),
            layout: Some(&skin_pipeline_layout),
            module: &skin_module,
            entry_point: Some("skin"),
            compilation_options: Default::default(),
            cache: None,
        });
        let storage_usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let clips = Grow::new(
            device,
            "actor clips",
            storage_usage | wgpu::BufferUsages::COPY_SRC,
            48,
        );
        queue.write_buffer(&clips.buffer, 0, bytemuck::cast_slice(&IDENTITY));
        Self {
            textures,
            slots: Vec::new(),
            names: Vec::new(),
            glows,
            pipelines,
            depth_pipeline,
            masked_pipeline,
            skin_pipeline,
            actor_layout,
            skin_layout,
            clips,
            clip_len: 1,
            models: Grow::new(
                device,
                "actor models",
                wgpu::BufferUsages::UNIFORM
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                MODEL_STRIDE,
            ),
            actors: Grow::new(device, "actors", storage_usage, 1024),
            jobs: Grow::new(device, "skin jobs", storage_usage, 1024),
            palette: Grow::new(device, "joint palettes", wgpu::BufferUsages::STORAGE, 1024),
            actor_group: None,
            skin_group: None,
            instances: Vec::new(),
            ranges: Vec::new(),
            job_count: 0,
            max_joints: 1,
            bytes: 0,
            stats: ActorStats::default(),
            staging_actors: Vec::new(),
            staging_jobs: Vec::new(),
            portrait: None,
            portrait_slot: None,
        }
    }

    /// The model the portrait shows (posed and placed by the caller), or none.
    pub fn set_portrait(&mut self, instance: Option<ActorInstance>) {
        self.portrait = instance;
    }

    /// Whether the latest frame prepared a portrait.
    pub fn has_portrait(&self) -> bool {
        self.portrait_slot.is_some()
    }

    /// A loaded model's bind-pose bounds (model space).
    pub fn rest_bounds(&self, model: u32) -> Option<(Vec3, Vec3)> {
        match self.slots.get(model as usize) {
            Some(Slot::Ready(m)) => Some(m.rest),
            _ => None,
        }
    }

    /// Draws the portrait's model (inside the portrait pass).
    pub fn draw_portrait(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        let (Some((m, index)), Some(_)) = (self.portrait_slot, &self.actor_group) else {
            return 0;
        };
        let Slot::Ready(model) = &self.slots[m] else {
            return 0;
        };
        pass.set_bind_group(0, frame, &[]);
        self.bind_model(pass, m, model);
        let mut sections: Vec<(Variant, usize)> = model
            .sections
            .iter()
            .enumerate()
            .map(|(s, section)| (model.materials[section.material as usize].variant, s))
            .collect();
        // Opaque first, blended last.
        sections.sort_by_key(|&(v, s)| (v.is_blend(), v, s));
        for &(variant, s) in &sections {
            let section = model.sections[s];
            pass.set_pipeline(&self.pipelines[&variant]);
            pass.set_bind_group(
                1,
                &model.materials[section.material as usize].bind_group,
                &[],
            );
            pass.draw_indexed(
                section.first_index..section.first_index + section.index_count,
                section.base_vertex as i32,
                index..index + 1,
            );
        }
        sections.len() as u32
    }

    /// Model entries by index; models already listed keep their index.
    pub fn models(&self) -> &[String] {
        &self.names
    }

    /// Loads the entries of `names` beyond those already loaded. The list only grows: an
    /// index once given stays that model. Models the pack lacks draw nothing (warned once).
    pub fn sync_models(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &Pack,
        names: &[String],
    ) {
        if names.len() <= self.names.len() {
            return;
        }
        let _span =
            tracing::info_span!("actor_models", count = names.len() - self.names.len()).entered();
        let fresh: Vec<String> = names[self.names.len()..].to_vec();
        // Room for every new clip first, so the clip buffer grows at most once.
        let mut needed = u64::from(self.clip_len);
        for name in &fresh {
            if let Ok(model) = pack.model(name) {
                let joints = model.meta.skeleton.as_ref().map_or(0, |s| s.joints.len()) as u64;
                needed += model
                    .meta
                    .clips
                    .iter()
                    .map(|c| u64::from(c.frame_count) * joints)
                    .sum::<u64>();
            }
        }
        let bytes = needed * 48;
        if bytes > self.clips.capacity {
            let usage = self.clips.usage;
            let old = std::mem::replace(
                &mut self.clips,
                Grow::new(device, "actor clips", usage, bytes),
            );
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(
                &old.buffer,
                0,
                &self.clips.buffer,
                0,
                u64::from(self.clip_len) * 48,
            );
            queue.submit([encoder.finish()]);
            self.skin_group = None;
        }
        for name in fresh {
            let slot = match self.load(device, queue, pack, &name) {
                Ok(model) => Slot::Ready(Box::new(model)),
                Err(error) => {
                    tracing::warn!(model = %name, "live model not shown: {error:#}");
                    Slot::Missing
                }
            };
            self.slots.push(slot);
            self.names.push(name);
        }
        // Model uniforms: rewrite them all into a buffer of the right size.
        let mut uniforms = vec![0u8; self.slots.len() * MODEL_STRIDE as usize];
        for (i, (slot, name)) in self.slots.iter().zip(&self.names).enumerate() {
            if let (Slot::Ready(_), Ok(model)) = (slot, pack.model(name)) {
                let meta = &model.meta;
                let wind = meta.wind.as_ref();
                let uniform = ModelUniform {
                    position_min: [
                        meta.position_min[0],
                        meta.position_min[1],
                        meta.position_min[2],
                        0.0,
                    ],
                    position_extent: [
                        meta.position_extent[0],
                        meta.position_extent[1],
                        meta.position_extent[2],
                        0.0,
                    ],
                    uv: [
                        meta.uv_min[0],
                        meta.uv_min[1],
                        meta.uv_extent[0],
                        meta.uv_extent[1],
                    ],
                    wind: [wind.map_or(0.0, |w| w.amplitude), 0.0, 0.0, 0.0],
                };
                let at = i * MODEL_STRIDE as usize;
                uniforms[at..at + std::mem::size_of::<ModelUniform>()]
                    .copy_from_slice(bytemuck::bytes_of(&uniform));
            }
        }
        self.models.reserve(device, uniforms.len() as u64);
        queue.write_buffer(&self.models.buffer, 0, &uniforms);
        self.actor_group = None;
        self.bytes = self.textures.bytes
            + self
                .slots
                .iter()
                .map(|s| match s {
                    Slot::Ready(m) => m.vertices.size() * 2 + m.indices.size(),
                    Slot::Missing => 0,
                })
                .sum::<u64>()
            + u64::from(self.clip_len) * 48;
    }

    fn load(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &Pack,
        name: &str,
    ) -> Result<GpuModel> {
        let model = pack.model(name).with_context(|| name.to_owned())?;
        let meta = &model.meta;
        let buffer = |label: &str, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: if contents.is_empty() {
                    &[0u8; 16]
                } else {
                    contents
                },
                usage,
            })
        };
        let vertex = wgpu::BufferUsages::VERTEX;
        let n = meta.vertex_count as usize;
        let rigid = SkinVertex {
            joints: [0; 4],
            weights: [255, 0, 0, 0],
        };
        let colors: Vec<[u8; 4]> = model
            .colors()
            .map_or_else(|| vec![[255; 4]; n], <[_]>::to_vec);
        let leaves: Vec<[u16; 4]> = model
            .leaves()
            .map_or_else(|| vec![[0; 4]; n], <[_]>::to_vec);
        let skin: Vec<SkinVertex> = model.skin().map_or_else(|| vec![rigid; n], <[_]>::to_vec);
        let joints = meta.skeleton.as_ref().map_or(1, |s| s.joints.len().max(1)) as u32;
        let animated = meta.skeleton.is_some() && !meta.clips.is_empty();
        let depth = (meta.depth.index_count > 0).then(|| {
            let count = meta.depth.vertex_count as usize;
            let depth_skin: Vec<SkinVertex> = model
                .depth_skin()
                .map_or_else(|| vec![rigid; count], <[_]>::to_vec);
            DepthBuffers {
                positions: buffer(
                    "actor depth",
                    bytemuck::cast_slice(model.depth_positions()),
                    vertex,
                ),
                skin: buffer(
                    "actor depth skin",
                    bytemuck::cast_slice(&depth_skin),
                    vertex,
                ),
                indices: buffer(
                    "actor depth indices",
                    model.depth_indices().bytes(),
                    wgpu::BufferUsages::INDEX,
                ),
                format: wgpu_index(meta.depth.index_format),
            }
        });
        let glow = self.glows.get(name).copied();
        let mut materials = Vec::with_capacity(meta.materials.len());
        for material in &meta.materials {
            materials.push(
                self.textures
                    .material(device, queue, pack, material, glow.as_ref())?,
            );
        }
        let mut clips = Vec::with_capacity(meta.clips.len());
        if meta.skeleton.is_some() {
            for (i, clip) in meta.clips.iter().enumerate() {
                let matrices = model.clip_matrices(i);
                queue.write_buffer(
                    &self.clips.buffer,
                    u64::from(self.clip_len) * 48,
                    bytemuck::cast_slice(matrices),
                );
                clips.push(ClipRange {
                    base: self.clip_len,
                    frames: clip.frame_count.max(1),
                    duration: clip.duration,
                });
                self.clip_len += matrices.len() as u32;
            }
        }
        let min = Vec3::from_array(meta.bounds.min);
        let max = Vec3::from_array(meta.bounds.max);
        self.max_joints = self.max_joints.max(joints);
        let mut lods = vec![ModelLod {
            error: 0.0,
            sections: meta
                .sections
                .iter()
                .map(|s| (s.first_index, s.index_count))
                .collect(),
            depth: (0, meta.depth.index_count),
        }];
        lods.extend(meta.lods.iter().take(MAX_LODS - 1).map(|lod| ModelLod {
            error: lod.error,
            sections: lod.sections.iter().map(|r| (r[0], r[1])).collect(),
            depth: (lod.depth[0], lod.depth[1]),
        }));
        Ok(GpuModel {
            name: name.to_owned(),
            vertices: buffer(
                "actor vertices",
                bytemuck::cast_slice(model.vertices()),
                vertex,
            ),
            colors: buffer("actor colours", bytemuck::cast_slice(&colors), vertex),
            leaves: buffer("actor leaves", bytemuck::cast_slice(&leaves), vertex),
            skin: buffer("actor skin", bytemuck::cast_slice(&skin), vertex),
            indices: buffer(
                "actor indices",
                model.indices().bytes(),
                wgpu::BufferUsages::INDEX,
            ),
            index_format: wgpu_index(meta.index_format),
            depth,
            sections: meta.sections.clone(),
            lods,
            casts: materials.iter().any(|m| m.casts_shadow),
            materials,
            masked_shadow_sections: meta
                .depth
                .masked
                .iter()
                .map(|&s| s as usize)
                .filter(|&s| s < meta.sections.len())
                .collect(),
            joints,
            clips,
            sphere: ((min + max) * 0.5, (max - min).length() * 0.5),
            height: meta.rest_bounds.max[1] - meta.rest_bounds.min[1],
            rest: (
                Vec3::from_array(meta.rest_bounds.min),
                Vec3::from_array(meta.rest_bounds.max),
            ),
            animated,
        })
    }

    /// The frame's instances (any order).
    pub fn set_instances(&mut self, instances: &[ActorInstance]) {
        self.instances.clear();
        self.instances.extend_from_slice(instances);
    }

    /// Culls, sorts and uploads the frame's instances and skinning jobs.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: Mat4,
        sun: Option<Mat4>,
        lod: Option<&LodView>,
    ) {
        let _span = tracing::trace_span!("actors_prepare").entered();
        let camera_planes = frustum_planes(camera);
        let sun_planes = sun.map(frustum_planes);
        self.ranges.clear();
        self.ranges.resize(self.slots.len(), Range::default());
        // (model, 0 camera-visible / 1 sun only / 2 translucent, level of detail, input)
        let mut order: Vec<(u32, u8, u8, u32)> = Vec::with_capacity(self.instances.len());
        let mut casters = 0;
        for (i, instance) in self.instances.iter().enumerate() {
            let Some(Slot::Ready(model)) = self.slots.get(instance.model as usize) else {
                continue;
            };
            let sphere = world_sphere(&instance.transform, model.sphere.0, model.sphere.1);
            let scale_y = instance.transform.y_axis.truncate().length();
            let in_camera = sphere_visible(&camera_planes, sphere);
            if instance.translucent() {
                if in_camera && instance.tint[3] > 0.0 {
                    order.push((instance.model, 2, 0, i as u32));
                }
                continue;
            }
            let casts = model.casts
                && (model.animated || model.height * scale_y >= SHADOW_MIN_HEIGHT)
                && instance.opacity > 0.5;
            let in_sun = casts
                && sun_planes
                    .as_ref()
                    .is_some_and(|p| sphere_visible(p, sphere));
            casters += u32::from(in_sun);
            let scale = max_scale(&instance.transform);
            if in_sun {
                let range = &mut self.ranges[instance.model as usize];
                range.shadow_scale = range.shadow_scale.max(scale);
            }
            if in_camera {
                let level = lod.map_or(0, |view| {
                    let depth = -view.view.transform_point3(sphere.truncate()).z;
                    // World units per pixel at the instance; the allowed deviation in
                    // model units.
                    let allowed = view.max_pixels * depth.max(1e-3)
                        / (view.pixels_per_unit * scale.max(1e-6));
                    choose_lod(&model.lods, allowed)
                });
                order.push((instance.model, 0, level, i as u32));
            } else if in_sun {
                order.push((instance.model, 1, 0, i as u32));
            }
        }
        order.sort_unstable();
        self.staging_actors.clear();
        self.staging_jobs.clear();
        let mut palette_len = 0u32;
        for &(m, view, level, i) in &order {
            let instance = &self.instances[i as usize];
            let Slot::Ready(model) = &self.slots[m as usize] else {
                continue;
            };
            let range = &mut self.ranges[m as usize];
            if range.all + range.translucent == 0 {
                range.first = self.staging_actors.len() as u32;
            }
            match view {
                0 => {
                    range.camera += 1;
                    range.all += 1;
                    range.lods[usize::from(level)] += 1;
                }
                1 => range.all += 1,
                _ => range.translucent += 1,
            }
            stage(
                &mut self.staging_actors,
                &mut self.staging_jobs,
                &mut palette_len,
                model,
                instance,
            );
        }
        // Shadow casters: one level per model, from the shadow map's texel (orthographic,
        // so the same everywhere).
        for (slot, range) in self.slots.iter().zip(self.ranges.iter_mut()) {
            if let (Slot::Ready(model), Some(view)) = (slot, lod)
                && range.all > 0
                && view.max_pixels > 0.0
            {
                let allowed = SHADOW_LOD_TEXELS * view.shadow_texel / range.shadow_scale.max(1e-6);
                range.shadow_lod = choose_lod(&model.lods, allowed);
            }
        }
        // The portrait's model, after everything the views draw.
        self.portrait_slot = None;
        if let Some(instance) = &self.portrait
            && let Some(Slot::Ready(model)) = self.slots.get(instance.model as usize)
        {
            self.portrait_slot = Some((instance.model as usize, self.staging_actors.len() as u32));
            stage(
                &mut self.staging_actors,
                &mut self.staging_jobs,
                &mut palette_len,
                model,
                instance,
            );
        }
        self.job_count = self.staging_jobs.len() as u32;
        let mut rebind = false;
        rebind |= self.actors.reserve(
            device,
            (self.staging_actors.len() * std::mem::size_of::<ActorGpu>()) as u64,
        );
        rebind |= self.jobs.reserve(
            device,
            (self.staging_jobs.len() * std::mem::size_of::<SkinJob>()) as u64,
        );
        rebind |= self.palette.reserve(device, u64::from(palette_len) * 48);
        if rebind {
            self.actor_group = None;
            self.skin_group = None;
        }
        if !self.staging_actors.is_empty() {
            queue.write_buffer(
                &self.actors.buffer,
                0,
                bytemuck::cast_slice(&self.staging_actors),
            );
            queue.write_buffer(
                &self.jobs.buffer,
                0,
                bytemuck::cast_slice(&self.staging_jobs),
            );
        }
        if self.actor_group.is_none() {
            self.actor_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("actors"),
                layout: &self.actor_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.actors.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.palette.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.models.buffer,
                            offset: 0,
                            size: wgpu::BufferSize::new(std::mem::size_of::<ModelUniform>() as u64),
                        }),
                    },
                ],
            }));
        }
        if self.skin_group.is_none() {
            self.skin_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("skin"),
                layout: &self.skin_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.jobs.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.clips.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.palette.buffer.as_entire_binding(),
                    },
                ],
            }));
        }
        self.stats = ActorStats {
            models: self
                .ranges
                .iter()
                .filter(|r| r.camera + r.translucent > 0)
                .count() as u32,
            instances: self.instances.len() as u32,
            visible: self.ranges.iter().map(|r| r.camera + r.translucent).sum(),
            shadow_casters: casters,
            joints: palette_len,
            draws: 0,
            triangles: 0,
            shadow_triangles: 0,
        };
        for (slot, range) in self.slots.iter().zip(&self.ranges) {
            let Slot::Ready(model) = slot else { continue };
            let indices = |lod: &ModelLod| lod.sections.iter().map(|s| u64::from(s.1)).sum::<u64>();
            for (lod, &count) in model.lods.iter().zip(&range.lods) {
                self.stats.triangles += u64::from(count) * indices(lod) / 3;
            }
            self.stats.triangles += u64::from(range.translucent) * indices(&model.lods[0]) / 3;
            if model.casts {
                let lod = &model.lods[usize::from(range.shadow_lod)];
                let depth = if model.depth.is_some() {
                    u64::from(lod.depth.1)
                } else {
                    0
                };
                let masked: u64 = model
                    .masked_shadow_sections
                    .iter()
                    .map(|&s| u64::from(lod.sections[s].1))
                    .sum();
                self.stats.shadow_triangles += u64::from(range.all) * (depth + masked) / 3;
            }
        }
    }

    /// Records the palette blend (inside a compute pass).
    pub fn skin(&self, pass: &mut wgpu::ComputePass<'_>) {
        let Some(group) = &self.skin_group else {
            return;
        };
        if self.job_count == 0 {
            return;
        }
        pass.set_pipeline(&self.skin_pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.dispatch_workgroups(self.max_joints.div_ceil(SKIN_GROUP), self.job_count, 1);
    }

    fn bind_model(&self, pass: &mut wgpu::RenderPass<'_>, index: usize, model: &GpuModel) {
        let group = self.actor_group.as_ref().expect("prepared");
        pass.set_bind_group(2, group, &[(index as u64 * MODEL_STRIDE) as u32]);
        pass.set_vertex_buffer(0, model.vertices.slice(..));
        pass.set_vertex_buffer(1, model.colors.slice(..));
        pass.set_vertex_buffer(2, model.leaves.slice(..));
        pass.set_vertex_buffer(3, model.skin.slice(..));
        pass.set_index_buffer(model.indices.slice(..), model.index_format);
    }

    fn draw_sections(&self, pass: &mut wgpu::RenderPass<'_>, blend: bool) -> u32 {
        // (pipeline, model, section, level, instances): opaque instances draw each section
        // with its own variant, a draw per level of detail; translucent ones draw every
        // section of the full mesh blended.
        let mut draws: Vec<(Variant, usize, usize, usize, std::ops::Range<u32>)> = Vec::new();
        for (m, slot) in self.slots.iter().enumerate() {
            let Slot::Ready(model) = slot else { continue };
            let Some(range) = self.ranges.get(m).copied() else {
                continue;
            };
            for (s, section) in model.sections.iter().enumerate() {
                let variant = model.materials[section.material as usize].variant;
                if range.camera > 0 && variant.is_blend() == blend {
                    let mut first = range.first;
                    for (level, &count) in range.lods.iter().enumerate() {
                        if count > 0 {
                            draws.push((variant, m, s, level, first..first + count));
                        }
                        first += count;
                    }
                }
                if blend && range.translucent > 0 {
                    let first = range.first + range.all;
                    draws.push((variant.blended(), m, s, 0, first..first + range.translucent));
                }
            }
        }
        draws.sort_unstable_by_key(|d| (d.0, d.1, d.2, d.3, d.4.start));
        let mut current = None;
        let mut bound = usize::MAX;
        for (variant, m, s, level, instances) in &draws {
            let Slot::Ready(model) = &self.slots[*m] else {
                continue;
            };
            if current != Some(*variant) {
                pass.set_pipeline(&self.pipelines[variant]);
                current = Some(*variant);
            }
            if bound != *m {
                self.bind_model(pass, *m, model);
                bound = *m;
            }
            let section = model.sections[*s];
            let (first, count) = model.lods[*level].sections[*s];
            pass.set_bind_group(
                1,
                &model.materials[section.material as usize].bind_group,
                &[],
            );
            pass.draw_indexed(
                first..first + count,
                section.base_vertex as i32,
                instances.clone(),
            );
        }
        draws.len() as u32
    }

    pub fn draw_forward(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        if self.actor_group.is_none() {
            return 0;
        }
        pass.set_bind_group(0, frame, &[]);
        self.draw_sections(pass, false)
    }

    pub fn draw_transparent(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        frame: &wgpu::BindGroup,
    ) -> u32 {
        if self.actor_group.is_none() {
            return 0;
        }
        pass.set_bind_group(0, frame, &[]);
        self.draw_sections(pass, true)
    }

    pub fn draw_shadow(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        let Some(group) = &self.actor_group else {
            return 0;
        };
        pass.set_bind_group(0, frame, &[]);
        let mut count = 0;
        for (m, slot) in self.slots.iter().enumerate() {
            let Slot::Ready(model) = slot else { continue };
            let Some(range) = self.ranges.get(m).copied().filter(|r| r.all > 0) else {
                continue;
            };
            if !model.casts {
                continue;
            }
            let instances = range.first..range.first + range.all;
            let lod = &model.lods[usize::from(range.shadow_lod)];
            if let Some(depth) = &model.depth {
                pass.set_pipeline(&self.depth_pipeline);
                pass.set_bind_group(1, &model.materials[0].bind_group, &[]);
                pass.set_bind_group(2, group, &[(m as u64 * MODEL_STRIDE) as u32]);
                pass.set_vertex_buffer(0, depth.positions.slice(..));
                pass.set_vertex_buffer(1, depth.skin.slice(..));
                pass.set_index_buffer(depth.indices.slice(..), depth.format);
                let (first, n) = lod.depth;
                pass.draw_indexed(first..first + n, 0, instances.clone());
                count += 1;
            }
            if !model.masked_shadow_sections.is_empty() {
                pass.set_pipeline(&self.masked_pipeline);
                self.bind_model(pass, m, model);
                for &s in &model.masked_shadow_sections {
                    let section = model.sections[s];
                    let (first, n) = lod.sections[s];
                    pass.set_bind_group(
                        1,
                        &model.materials[section.material as usize].bind_group,
                        &[],
                    );
                    pass.draw_indexed(
                        first..first + n,
                        section.base_vertex as i32,
                        instances.clone(),
                    );
                    count += 1;
                }
            }
        }
        count
    }

    /// Names of loaded models with their instance counts in the latest frame.
    pub fn model_counts(&self) -> impl Iterator<Item = (&str, u32)> {
        self.slots
            .iter()
            .zip(&self.ranges)
            .filter_map(|(slot, range)| match slot {
                Slot::Ready(m) => Some((m.name.as_str(), range.camera + range.translucent)),
                Slot::Missing => None,
            })
    }

    /// Whether model `index` loaded.
    pub fn is_ready(&self, index: usize) -> bool {
        matches!(self.slots.get(index), Some(Slot::Ready(_)))
    }
}

/// The largest axis scale of an affine transform.
fn max_scale(transform: &Mat4) -> f32 {
    transform
        .x_axis
        .truncate()
        .length()
        .max(transform.y_axis.truncate().length())
        .max(transform.z_axis.truncate().length())
}

/// The coarsest level whose deviation is within `allowed` model units.
fn choose_lod(lods: &[ModelLod], allowed: f32) -> u8 {
    lods.iter()
        .rposition(|lod| lod.error <= allowed)
        .unwrap_or(0) as u8
}

/// Adds one instance's GPU record and skinning job.
fn stage(
    actors: &mut Vec<ActorGpu>,
    jobs: &mut Vec<SkinJob>,
    palette_len: &mut u32,
    model: &GpuModel,
    instance: &ActorInstance,
) {
    let rows = instance.transform.transpose();
    actors.push(ActorGpu {
        rows: [
            rows.x_axis.to_array(),
            rows.y_axis.to_array(),
            rows.z_axis.to_array(),
        ],
        color: instance
            .owner_color
            .map_or([0.0; 4], |c| [c[0], c[1], c[2], 1.0]),
        palette: [*palette_len, model.joints, 0, 0],
        look: [instance.opacity, 0.0, 0.0, 0.0],
        tint: instance.tint,
    });
    let mut job = SkinJob {
        header: [*palette_len, model.joints, 0, 0],
        poses: [[0; 4]; MAX_POSES],
    };
    if model.animated {
        let used = usize::from(instance.pose_count).min(MAX_POSES);
        let total: f32 = instance.poses[..used]
            .iter()
            .map(|p| p.weight.max(0.0))
            .sum();
        let mut count = 0;
        for pose in &instance.poses[..used] {
            let Some(clip) = model.clips.get(usize::from(pose.clip)) else {
                continue;
            };
            if pose.weight <= 0.0 || total <= 0.0 {
                continue;
            }
            let (a, b, t) = clip_frames(clip.frames, clip.duration, pose.time);
            job.poses[count] = [
                clip.base + a * model.joints,
                clip.base + b * model.joints,
                t.to_bits(),
                (pose.weight / total).to_bits(),
            ];
            count += 1;
        }
        job.header[2] = count as u32;
    }
    jobs.push(job);
    *palette_len += model.joints;
}

/// The two baked frames around `time` and the mix between them (frame `f` is time
/// `min(f / rate, duration)`).
pub fn clip_frames(frames: u32, duration: f32, time: f32) -> (u32, u32, f32) {
    let last = frames.saturating_sub(1);
    let t = time.clamp(0.0, duration.max(0.0)) * CLIP_FRAME_RATE;
    let a = (t.floor() as u32).min(last);
    let b = (a + 1).min(last);
    let mix = if b == a {
        0.0
    } else {
        (t - a as f32).clamp(0.0, 1.0)
    };
    (a, b, mix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_times_map_to_baked_frames() {
        // 1 s at 30 Hz: 31 frames, the last at exactly 1 s.
        assert_eq!(clip_frames(31, 1.0, 0.0), (0, 1, 0.0));
        let (a, b, t) = clip_frames(31, 1.0, 0.5 / 30.0);
        assert_eq!((a, b), (0, 1));
        assert!((t - 0.5).abs() < 1e-5);
        assert_eq!(clip_frames(31, 1.0, 1.0), (30, 30, 0.0));
        // Past the end and before the start clamp.
        assert_eq!(clip_frames(31, 1.0, 7.0), (30, 30, 0.0));
        assert_eq!(clip_frames(31, 1.0, -1.0), (0, 1, 0.0));
        assert_eq!(clip_frames(1, 0.0, 0.3), (0, 0, 0.0));
    }
}
