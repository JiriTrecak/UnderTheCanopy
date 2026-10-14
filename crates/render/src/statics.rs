//! Static models: scenery, resources and buildings, drawn in static poses.
//!
//! Every instance lives in one persistent buffer, sorted by model. Each frame a compute
//! pass culls all of them against the camera and the sun and appends the visible ones
//! to per-model lists; a second dispatch writes each draw's instance count. Draws are
//! indirect, one per model section (plus one depth-stream draw and one per alpha-tested
//! section for shadows), so their number depends on the models in the scene, never on
//! how many instances there are (renderer §4.5). Without `indirect-first-instance`
//! (D2) each list is reached through a dynamic offset instead of a first instance.

use crate::{
    camera::frustum_planes,
    device::GpuFeatures,
    targets::{self, HDR_FORMAT},
    texture,
};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use canopy_assets::{AlphaMode, IndexFormat, Material, ModelMeta, Pack, VertexColor};
use glam::{Mat4, Vec3, Vec4};
use std::collections::{BTreeMap, HashMap};
use wgpu::util::DeviceExt;

/// A placed model.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticInstance {
    /// Pack entry of the model.
    pub model: String,
    /// Model space → world (affine).
    pub transform: Mat4,
    /// The owner's linear colour, for team-coloured materials; `None` uses the
    /// material's own.
    pub owner_color: Option<[f32; 3]>,
}

/// A colour-keyed glow added to a model's emission (old deposit glow): `color ·
/// strength · Π smoothstep(from, to, weights · albedo)` over three terms.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glow {
    pub color: [f32; 3],
    pub strength: f32,
    pub terms: [GlowTerm; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlowTerm {
    pub weights: [f32; 3],
    pub from: f32,
    pub to: f32,
}

/// Visible-list slots are aligned so a dynamic offset can reach them.
const SLOT_ALIGN: u32 = 64;
const MODEL_STRIDE: u64 = 256;
/// Shorter instances cast no shadow (old `SHADOW_MIN_HEIGHT`).
pub const SHADOW_MIN_HEIGHT: f32 = 0.75;

const FLAG_TEAM: u32 = 1;
const FLAG_TEAM_MASK: u32 = 2;
const FLAG_FOLIAGE: u32 = 4;
const FLAG_DOUBLE_SIDED: u32 = 8;
const FLAG_NORMAL_MAP: u32 = 16;
const FLAG_OCCLUSION_MAP: u32 = 32;
const FLAG_PAINTED: u32 = 64;
const FLAG_ALPHA_MASK: u32 = 128;
const FLAG_ALPHA_BLEND: u32 = 256;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    emissive: [f32; 4],
    params: [f32; 4],
    light: [f32; 4],
    team_default: [f32; 4],
    flags: [u32; 4],
    glow: [f32; 4],
    glow_terms: [[f32; 4]; 3],
    glow_to: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ModelUniform {
    position_min: [f32; 4],
    position_extent: [f32; 4],
    uv: [f32; 4],
    wind: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct InstanceGpu {
    rows: [[f32; 4]; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CullItem {
    sphere: [f32; 4],
    info: [u32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CullParams {
    planes: [[f32; 4]; 12],
    counts: [u32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Variant {
    Opaque { double_sided: bool },
    Mask { double_sided: bool },
    Blend { double_sided: bool },
}

impl Variant {
    fn of(material: &Material) -> Self {
        let double_sided = material.double_sided;
        match material.alpha {
            AlphaMode::Opaque => Self::Opaque { double_sided },
            AlphaMode::Mask => Self::Mask { double_sided },
            AlphaMode::Blend => Self::Blend { double_sided },
        }
    }
}

/// One indirect draw.
struct DrawRecord {
    model: usize,
    /// Section index, or `None` for the depth stream.
    section: Option<usize>,
    args: usize,
}

struct GpuMaterial {
    bind_group: wgpu::BindGroup,
    variant: Variant,
    casts_shadow: bool,
}

struct GpuModel {
    name: String,
    vertices: wgpu::Buffer,
    colors: wgpu::Buffer,
    leaves: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_format: wgpu::IndexFormat,
    depth_positions: Option<wgpu::Buffer>,
    depth_indices: Option<(wgpu::Buffer, wgpu::IndexFormat)>,
    sections: Vec<canopy_assets::Section>,
    materials: Vec<GpuMaterial>,
    masked_shadow_sections: Vec<usize>,
    /// First visible slot per view.
    region: [u32; 2],
    depth_index_count: u32,
    instances: u32,
}

/// Counts of the static scene.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticsStats {
    pub models: u32,
    pub instances: u32,
    pub forward_draws: u32,
    pub shadow_draws: u32,
}

pub struct StaticsGpu {
    models: Vec<GpuModel>,
    forward_draws: Vec<DrawRecord>,
    blend_draws: Vec<DrawRecord>,
    shadow_draws: Vec<DrawRecord>,
    pipelines: HashMap<Variant, wgpu::RenderPipeline>,
    depth_pipeline: wgpu::RenderPipeline,
    masked_pipeline: wgpu::RenderPipeline,
    instance_group: wgpu::BindGroup,
    cull_group: wgpu::BindGroup,
    cull_pipeline: wgpu::ComputePipeline,
    finish_pipeline: wgpu::ComputePipeline,
    params: wgpu::Buffer,
    counters: wgpu::Buffer,
    args: wgpu::Buffer,
    items: u32,
    draws: u32,
    /// Bytes of GPU memory (buffers and textures).
    pub bytes: u64,
    pub stats: StaticsStats,
}

fn wgpu_index(format: IndexFormat) -> wgpu::IndexFormat {
    match format {
        IndexFormat::U16 => wgpu::IndexFormat::Uint16,
        IndexFormat::U32 => wgpu::IndexFormat::Uint32,
    }
}

/// Bounding sphere of an axis-aligned box.
fn sphere(meta: &ModelMeta) -> (Vec3, f32) {
    let min = Vec3::from_array(meta.bounds.min);
    let max = Vec3::from_array(meta.bounds.max);
    ((min + max) * 0.5, (max - min).length() * 0.5)
}

/// A world sphere containing the model sphere under `transform`.
pub fn world_sphere(transform: &Mat4, center: Vec3, radius: f32) -> Vec4 {
    let scale = transform
        .x_axis
        .truncate()
        .length()
        .max(transform.y_axis.truncate().length())
        .max(transform.z_axis.truncate().length());
    transform.transform_point3(center).extend(radius * scale)
}

/// Whether a sphere touches the frustum (the shader's test, on the CPU).
pub fn sphere_visible(planes: &[Vec4; 6], sphere: Vec4) -> bool {
    planes
        .iter()
        .all(|p| p.truncate().dot(sphere.truncate()) + p.w >= -sphere.w)
}

struct Textures<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    features: GpuFeatures,
    pack: &'a Pack,
    cache: HashMap<String, wgpu::TextureView>,
    white_srgb: wgpu::TextureView,
    white: wgpu::TextureView,
    white_r8: wgpu::TextureView,
    flat_normal: wgpu::TextureView,
    bytes: u64,
}

impl Textures<'_> {
    fn get(
        &mut self,
        name: Option<&str>,
        fallback: &wgpu::TextureView,
    ) -> Result<wgpu::TextureView> {
        let Some(name) = name else {
            return Ok(fallback.clone());
        };
        if let Some(view) = self.cache.get(name) {
            return Ok(view.clone());
        }
        let tex = self.pack.texture(name).with_context(|| name.to_owned())?;
        self.bytes += tex.meta.mips.iter().map(|m| u64::from(m.len)).sum::<u64>();
        let gpu = texture::upload(self.device, self.queue, self.features, &tex);
        let view = gpu.create_view(&Default::default());
        self.cache.insert(name.to_owned(), view.clone());
        Ok(view)
    }
}

impl StaticsGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        pack: &Pack,
        instances: &[StaticInstance],
        glows: &BTreeMap<String, Glow>,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        let _span = tracing::info_span!("statics_upload").entered();
        let mut by_model: BTreeMap<&str, Vec<&StaticInstance>> = BTreeMap::new();
        for instance in instances {
            by_model.entry(&instance.model).or_default().push(instance);
        }
        let solid = |label, format, rgba| {
            texture::solid(device, queue, label, format, rgba).create_view(&Default::default())
        };
        let mut textures = Textures {
            device,
            queue,
            features,
            pack,
            cache: HashMap::new(),
            white_srgb: solid("white", wgpu::TextureFormat::Rgba8UnormSrgb, [255; 4]),
            white: solid("white linear", wgpu::TextureFormat::Rgba8Unorm, [255; 4]),
            white_r8: solid("white mask", wgpu::TextureFormat::R8Unorm, [255; 4]),
            flat_normal: solid(
                "flat normal",
                wgpu::TextureFormat::Rgba8Unorm,
                [128, 128, 255, 255],
            ),
            bytes: 0,
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let frag = wgpu::ShaderStages::FRAGMENT;
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: frag,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: vf,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                tex_entry(2),
                tex_entry(3),
                tex_entry(4),
                tex_entry(5),
                tex_entry(6),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let storage = |binding, dynamic, visibility, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: dynamic,
                min_binding_size: None,
            },
            count: None,
        };
        let instance_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("static instances"),
            entries: &[
                storage(0, false, vf, true),
                storage(1, true, wgpu::ShaderStages::VERTEX, true),
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

        let mut models = Vec::new();
        let mut gpu_instances = Vec::with_capacity(instances.len());
        let mut items = Vec::with_capacity(instances.len());
        let mut model_uniforms: Vec<u8> = Vec::new();
        let mut slots = 0u32;
        let mut largest = SLOT_ALIGN;
        let mut bytes = 0u64;
        for (model_index, (name, placed)) in by_model.iter().enumerate() {
            let model = pack.model(name).with_context(|| (*name).to_owned())?;
            let meta = &model.meta;
            let buffer = |label: &str, contents: &[u8], usage| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage,
                })
            };
            let vertex = wgpu::BufferUsages::VERTEX;
            let n = meta.vertex_count as usize;
            let colors: Vec<[u8; 4]> = model
                .colors()
                .map_or_else(|| vec![[255; 4]; n], <[_]>::to_vec);
            let leaves: Vec<[u16; 4]> = model
                .leaves()
                .map_or_else(|| vec![[0; 4]; n], <[_]>::to_vec);
            let vertices = buffer(
                "model vertices",
                bytemuck::cast_slice(model.vertices()),
                vertex,
            );
            let colors = buffer("model colours", bytemuck::cast_slice(&colors), vertex);
            let leaves = buffer("model leaves", bytemuck::cast_slice(&leaves), vertex);
            let indices = buffer(
                "model indices",
                model.indices().bytes(),
                wgpu::BufferUsages::INDEX,
            );
            bytes += (n * (16 + 4 + 8)) as u64 + model.indices().bytes().len() as u64;
            let has_depth = meta.depth.index_count > 0;
            let depth_positions = has_depth.then(|| {
                buffer(
                    "model depth",
                    bytemuck::cast_slice(model.depth_positions()),
                    vertex,
                )
            });
            let depth_indices = has_depth.then(|| {
                (
                    buffer(
                        "model depth indices",
                        model.depth_indices().bytes(),
                        wgpu::BufferUsages::INDEX,
                    ),
                    wgpu_index(meta.depth.index_format),
                )
            });

            let glow = glows.get(*name);
            let mut materials = Vec::with_capacity(meta.materials.len());
            for material in &meta.materials {
                let t = &material.textures;
                let mut flags = 0;
                if material.team_color.is_some() {
                    flags |= FLAG_TEAM;
                }
                if t.team_mask.is_some() {
                    flags |= FLAG_TEAM_MASK;
                }
                if material.foliage {
                    flags |= FLAG_FOLIAGE;
                }
                if material.double_sided {
                    flags |= FLAG_DOUBLE_SIDED;
                }
                if t.normal.is_some() {
                    flags |= FLAG_NORMAL_MAP;
                }
                if t.occlusion.is_some() {
                    flags |= FLAG_OCCLUSION_MAP;
                }
                if material.painted {
                    flags |= FLAG_PAINTED;
                }
                match material.alpha {
                    AlphaMode::Mask => flags |= FLAG_ALPHA_MASK,
                    AlphaMode::Blend => flags |= FLAG_ALPHA_BLEND,
                    AlphaMode::Opaque => {}
                }
                let team = material.team_color.as_ref();
                let uniform = MaterialUniform {
                    base_color: material.base_color,
                    emissive: [
                        material.emissive[0],
                        material.emissive[1],
                        material.emissive[2],
                        material.normal_scale,
                    ],
                    params: [
                        material.roughness,
                        material.metalness,
                        material.alpha_cutoff,
                        material.occlusion_strength,
                    ],
                    light: [
                        material.backside_lighting,
                        if material.foliage {
                            material.crown_normal
                        } else {
                            0.0
                        },
                        team.map_or(1.0, |t| t.source_max),
                        0.0,
                    ],
                    team_default: team.map_or([1.0; 4], |t| {
                        [t.default[0], t.default[1], t.default[2], 1.0]
                    }),
                    flags: [
                        match material.vertex_color {
                            VertexColor::None => 0,
                            VertexColor::Tint => 1,
                            VertexColor::Occlusion => 2,
                        },
                        flags,
                        0,
                        0,
                    ],
                    glow: glow.map_or([0.0; 4], |g| {
                        [g.color[0], g.color[1], g.color[2], g.strength]
                    }),
                    glow_terms: glow.map_or([[0.0; 4]; 3], |g| {
                        g.terms
                            .map(|t| [t.weights[0], t.weights[1], t.weights[2], t.from])
                    }),
                    glow_to: glow.map_or([1.0; 4], |g| {
                        [g.terms[0].to, g.terms[1].to, g.terms[2].to, 0.0]
                    }),
                };
                let uniform_buffer = buffer(
                    "material",
                    bytemuck::bytes_of(&uniform),
                    wgpu::BufferUsages::UNIFORM,
                );
                let (white_srgb, white, white_r8, flat) = (
                    textures.white_srgb.clone(),
                    textures.white.clone(),
                    textures.white_r8.clone(),
                    textures.flat_normal.clone(),
                );
                let views = [
                    textures.get(t.base_color.as_deref(), &white_srgb)?,
                    textures.get(t.team_mask.as_deref(), &white_r8)?,
                    textures.get(t.normal.as_deref(), &flat)?,
                    textures.get(t.metallic_roughness.as_deref(), &white)?,
                    textures.get(t.occlusion.as_deref(), &white)?,
                    textures.get(t.emissive.as_deref(), &white_srgb)?,
                ];
                let mut entries = vec![wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }];
                for (i, view) in views.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry {
                        binding: i as u32 + 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    });
                }
                entries.push(wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                });
                materials.push(GpuMaterial {
                    bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&material.name),
                        layout: &material_layout,
                        entries: &entries,
                    }),
                    variant: Variant::of(material),
                    casts_shadow: material.casts_shadow,
                });
            }

            let wind = meta.wind.as_ref();
            let rest_height = meta.rest_bounds.max[1];
            let uniform = ModelUniform {
                position_min: [
                    meta.position_min[0],
                    meta.position_min[1],
                    meta.position_min[2],
                    rest_height,
                ],
                position_extent: [
                    meta.position_extent[0],
                    meta.position_extent[1],
                    meta.position_extent[2],
                    if wind.is_some() { 1.0 } else { 0.0 },
                ],
                uv: [
                    meta.uv_min[0],
                    meta.uv_min[1],
                    meta.uv_extent[0],
                    meta.uv_extent[1],
                ],
                wind: [
                    wind.map_or(0.0, |w| w.amplitude),
                    wind.map_or(0.0, |w| w.speed),
                    0.0,
                    meta.plant_height.unwrap_or(12.0),
                ],
            };
            model_uniforms.extend_from_slice(bytemuck::bytes_of(&uniform));
            model_uniforms.resize(
                model_uniforms.len().next_multiple_of(MODEL_STRIDE as usize),
                0,
            );

            let casts = materials.iter().any(|m| m.casts_shadow);
            let (center, radius) = sphere(meta);
            let height = meta.rest_bounds.max[1] - meta.rest_bounds.min[1];
            for instance in placed {
                let t = instance.transform;
                let rows = t.transpose();
                gpu_instances.push(InstanceGpu {
                    rows: [
                        rows.x_axis.to_array(),
                        rows.y_axis.to_array(),
                        rows.z_axis.to_array(),
                    ],
                    color: instance
                        .owner_color
                        .map_or([0.0; 4], |c| [c[0], c[1], c[2], 1.0]),
                });
                let scale_y = t.y_axis.truncate().length();
                items.push(CullItem {
                    sphere: world_sphere(&t, center, radius).to_array(),
                    info: [
                        model_index as u32,
                        u32::from(casts && height * scale_y >= SHADOW_MIN_HEIGHT),
                        0,
                        0,
                    ],
                });
            }
            let count = placed.len() as u32;
            let padded = count.next_multiple_of(SLOT_ALIGN).max(SLOT_ALIGN);
            largest = largest.max(padded);
            let region = [slots, slots + padded];
            slots += padded * 2;
            let masked_shadow_sections = meta
                .depth
                .masked
                .iter()
                .map(|&s| s as usize)
                .filter(|&s| s < meta.sections.len())
                .collect();
            models.push(GpuModel {
                name: (*name).to_owned(),
                vertices,
                colors,
                leaves,
                indices,
                index_format: wgpu_index(meta.index_format),
                depth_positions,
                depth_indices,
                sections: meta.sections.clone(),
                materials,
                masked_shadow_sections,
                region,
                depth_index_count: meta.depth.index_count,
                instances: count,
            });
        }

        // Draw records and their indirect arguments.
        let mut args: Vec<wgpu::util::DrawIndexedIndirectArgs> = Vec::new();
        let mut draw_info: Vec<[u32; 4]> = Vec::new();
        let mut forward_draws = Vec::new();
        let mut blend_draws = Vec::new();
        let mut shadow_draws = Vec::new();
        for (m, model) in models.iter().enumerate() {
            for (s, section) in model.sections.iter().enumerate() {
                let record = DrawRecord {
                    model: m,
                    section: Some(s),
                    args: args.len(),
                };
                args.push(wgpu::util::DrawIndexedIndirectArgs {
                    index_count: section.index_count,
                    instance_count: 0,
                    first_index: section.first_index,
                    base_vertex: section.base_vertex as i32,
                    first_instance: 0,
                });
                draw_info.push([m as u32, 0, 0, 0]);
                match model.materials[section.material as usize].variant {
                    Variant::Blend { .. } => blend_draws.push(record),
                    _ => forward_draws.push(record),
                }
            }
            if model.depth_index_count > 0 {
                shadow_draws.push(DrawRecord {
                    model: m,
                    section: None,
                    args: args.len(),
                });
                args.push(wgpu::util::DrawIndexedIndirectArgs {
                    index_count: model.depth_index_count,
                    instance_count: 0,
                    first_index: 0,
                    base_vertex: 0,
                    first_instance: 0,
                });
                draw_info.push([m as u32, 1, 0, 0]);
            }
            for &s in &model.masked_shadow_sections {
                let section = model.sections[s];
                shadow_draws.push(DrawRecord {
                    model: m,
                    section: Some(s),
                    args: args.len(),
                });
                args.push(wgpu::util::DrawIndexedIndirectArgs {
                    index_count: section.index_count,
                    instance_count: 0,
                    first_index: section.first_index,
                    base_vertex: section.base_vertex as i32,
                    first_instance: 0,
                });
                draw_info.push([m as u32, 1, 0, 0]);
            }
        }
        // Group draws by pipeline to save state changes.
        forward_draws.sort_by_key(|d| {
            let model = &models[d.model];
            let section = model.sections[d.section.unwrap_or(0)];
            (model.materials[section.material as usize].variant, d.model)
        });

        let storage_init = |label: &str, contents: &[u8], extra| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                // Buffers may not be empty.
                contents: if contents.is_empty() {
                    &[0u8; 64]
                } else {
                    contents
                },
                usage: wgpu::BufferUsages::STORAGE | extra,
            })
        };
        let instance_buffer = storage_init(
            "static instances",
            bytemuck::cast_slice(&gpu_instances),
            wgpu::BufferUsages::empty(),
        );
        let item_buffer = storage_init(
            "static cull items",
            bytemuck::cast_slice(&items),
            wgpu::BufferUsages::empty(),
        );
        let regions: Vec<[u32; 4]> = models
            .iter()
            .map(|m| [m.region[0], m.region[1], 0, 0])
            .collect();
        let region_buffer = storage_init(
            "static regions",
            bytemuck::cast_slice(&regions),
            wgpu::BufferUsages::empty(),
        );
        let draw_buffer = storage_init(
            "static draws",
            bytemuck::cast_slice(&draw_info),
            wgpu::BufferUsages::empty(),
        );
        let arg_bytes: Vec<u8> = args.iter().flat_map(|a| a.as_bytes().to_vec()).collect();
        let args_buffer = storage_init(
            "static draw arguments",
            &arg_bytes,
            wgpu::BufferUsages::INDIRECT,
        );
        let counters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("static counters"),
            size: (models.len().max(1) * 8) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let visible = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("static visible lists"),
            // Every region is bound with the largest region's size, so the last one
            // needs room behind it.
            size: u64::from(slots + largest) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        if model_uniforms.is_empty() {
            model_uniforms.resize(MODEL_STRIDE as usize, 0);
        }
        let model_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("static models"),
            contents: &model_uniforms,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cull params"),
            size: std::mem::size_of::<CullParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        bytes +=
            (gpu_instances.len() * 64 + items.len() * 32 + slots as usize * 4 + args.len() * 20)
                as u64;
        bytes += textures.bytes;

        let instance_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("static instances"),
            layout: &instance_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: instance_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &visible,
                        offset: 0,
                        size: wgpu::BufferSize::new(u64::from(largest) * 4),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &model_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<ModelUniform>() as u64),
                    }),
                },
            ],
        });

        // Culling.
        let comp = wgpu::ShaderStages::COMPUTE;
        let cull_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cull"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: comp,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, false, comp, true),
                storage(2, false, comp, true),
                storage(3, false, comp, false),
                storage(4, false, comp, false),
                storage(5, false, comp, true),
                storage(6, false, comp, false),
            ],
        });
        let cull_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cull"),
            layout: &cull_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: item_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: region_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: counters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: visible.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: draw_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: args_buffer.as_entire_binding(),
                },
            ],
        });
        let cull_module =
            crate::shader::module(device, "cull", &[], include_str!("../shaders/cull.wgsl"));
        let cull_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cull"),
            bind_group_layouts: &[Some(&cull_layout)],
            immediate_size: 0,
        });
        let compute = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&cull_pipeline_layout),
                module: &cull_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let cull_pipeline = compute("cull");
        let finish_pipeline = compute("finish");

        // Drawing.
        let module = crate::shader::module(
            device,
            "statics",
            crate::shader::LIT,
            include_str!("../shaders/statics.wgsl"),
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("statics"),
            bind_group_layouts: &[
                Some(frame_layout),
                Some(&material_layout),
                Some(&instance_layout),
            ],
            immediate_size: 0,
        });
        let main_attributes =
            wgpu::vertex_attr_array![0 => Unorm16x4, 1 => Snorm16x2, 2 => Unorm16x2];
        let color_attributes = wgpu::vertex_attr_array![3 => Unorm8x4];
        let leaf_attributes = wgpu::vertex_attr_array![4 => Unorm16x4];
        let main_buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &main_attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 4,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &color_attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &leaf_attributes,
            }),
        ];
        let mut pipelines = HashMap::new();
        for variant in [
            Variant::Opaque {
                double_sided: false,
            },
            Variant::Opaque { double_sided: true },
            Variant::Mask {
                double_sided: false,
            },
            Variant::Mask { double_sided: true },
            Variant::Blend {
                double_sided: false,
            },
            Variant::Blend { double_sided: true },
        ] {
            let (double_sided, a2c, blend) = match variant {
                Variant::Opaque { double_sided } => (double_sided, false, None),
                Variant::Mask { double_sided } => (double_sided, true, None),
                Variant::Blend { double_sided } => {
                    (double_sided, false, Some(wgpu::BlendState::ALPHA_BLENDING))
                }
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("statics"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &main_buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: (!double_sided).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(targets::scene_depth(blend.is_none())),
                multisample: targets::scene_multisample(a2c),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            pipelines.insert(variant, pipeline);
        }
        let depth_attributes = wgpu::vertex_attr_array![0 => Unorm16x4];
        let depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("statics depth"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_depth"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &depth_attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(targets::shadow_depth()),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let masked_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("statics masked depth"),
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

        let stats = StaticsStats {
            models: models.len() as u32,
            instances: items.len() as u32,
            forward_draws: (forward_draws.len() + blend_draws.len()) as u32,
            shadow_draws: shadow_draws.len() as u32,
        };
        tracing::info!(?stats, "statics");
        Ok(Self {
            items: items.len() as u32,
            draws: args.len() as u32,
            models,
            forward_draws,
            blend_draws,
            shadow_draws,
            pipelines,
            depth_pipeline,
            masked_pipeline,
            instance_group,
            cull_group,
            cull_pipeline,
            finish_pipeline,
            params,
            counters,
            args: args_buffer,
            bytes,
            stats,
        })
    }

    /// Sets the culling planes of the camera and the sun.
    pub fn prepare(&self, queue: &wgpu::Queue, camera: Mat4, sun: Mat4) {
        let mut planes = [[0.0; 4]; 12];
        for (i, p) in frustum_planes(camera)
            .iter()
            .chain(frustum_planes(sun).iter())
            .enumerate()
        {
            planes[i] = p.to_array();
        }
        let params = CullParams {
            planes,
            counts: [self.items, self.draws, 0, 0],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
    }

    /// Clears the counters (record before the culling pass).
    pub fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.counters, 0, None);
    }

    pub fn cull(&self, pass: &mut wgpu::ComputePass<'_>) {
        if self.items == 0 {
            return;
        }
        pass.set_bind_group(0, &self.cull_group, &[]);
        pass.set_pipeline(&self.cull_pipeline);
        pass.dispatch_workgroups(self.items.div_ceil(64), 1, 1);
        pass.set_pipeline(&self.finish_pipeline);
        pass.dispatch_workgroups(self.draws.div_ceil(64), 1, 1);
    }

    fn offsets(&self, model: usize, view: usize) -> [u32; 2] {
        [
            self.models[model].region[view] * 4,
            (model as u64 * MODEL_STRIDE) as u32,
        ]
    }

    fn draw_sections(&self, pass: &mut wgpu::RenderPass<'_>, draws: &[DrawRecord]) -> u32 {
        let mut current: Option<Variant> = None;
        let mut bound_model = usize::MAX;
        for draw in draws {
            let model = &self.models[draw.model];
            let section = model.sections[draw.section.expect("a section draw")];
            let material = &model.materials[section.material as usize];
            if current != Some(material.variant) {
                pass.set_pipeline(&self.pipelines[&material.variant]);
                current = Some(material.variant);
            }
            if bound_model != draw.model {
                pass.set_bind_group(2, &self.instance_group, &self.offsets(draw.model, 0));
                pass.set_vertex_buffer(0, model.vertices.slice(..));
                pass.set_vertex_buffer(1, model.colors.slice(..));
                pass.set_vertex_buffer(2, model.leaves.slice(..));
                pass.set_index_buffer(model.indices.slice(..), model.index_format);
                bound_model = draw.model;
            }
            pass.set_bind_group(1, &material.bind_group, &[]);
            pass.draw_indexed_indirect(&self.args, (draw.args * 20) as u64);
        }
        draws.len() as u32
    }

    pub fn draw_forward(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        pass.set_bind_group(0, frame, &[]);
        self.draw_sections(pass, &self.forward_draws)
    }

    pub fn draw_transparent(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        frame: &wgpu::BindGroup,
    ) -> u32 {
        pass.set_bind_group(0, frame, &[]);
        self.draw_sections(pass, &self.blend_draws)
    }

    pub fn draw_shadow(&self, pass: &mut wgpu::RenderPass<'_>, frame: &wgpu::BindGroup) -> u32 {
        pass.set_bind_group(0, frame, &[]);
        let mut count = 0;
        for draw in &self.shadow_draws {
            let model = &self.models[draw.model];
            pass.set_bind_group(2, &self.instance_group, &self.offsets(draw.model, 1));
            match draw.section {
                None => {
                    let (Some(positions), Some((indices, format))) =
                        (&model.depth_positions, &model.depth_indices)
                    else {
                        continue;
                    };
                    pass.set_pipeline(&self.depth_pipeline);
                    // The depth pipeline reads no material, but the layout wants one.
                    pass.set_bind_group(1, &model.materials[0].bind_group, &[]);
                    pass.set_vertex_buffer(0, positions.slice(..));
                    pass.set_index_buffer(indices.slice(..), *format);
                }
                Some(s) => {
                    let section = model.sections[s];
                    pass.set_pipeline(&self.masked_pipeline);
                    pass.set_bind_group(
                        1,
                        &model.materials[section.material as usize].bind_group,
                        &[],
                    );
                    pass.set_vertex_buffer(0, model.vertices.slice(..));
                    pass.set_vertex_buffer(1, model.colors.slice(..));
                    pass.set_vertex_buffer(2, model.leaves.slice(..));
                    pass.set_index_buffer(model.indices.slice(..), model.index_format);
                }
            }
            pass.draw_indexed_indirect(&self.args, (draw.args * 20) as u64);
            count += 1;
        }
        count
    }

    /// Instances of each model, for reports.
    pub fn model_counts(&self) -> impl Iterator<Item = (&str, u32)> {
        self.models.iter().map(|m| (m.name.as_str(), m.instances))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{FlatGround, RtsCamera};

    #[test]
    fn spheres_follow_the_transform_and_the_frustum() {
        let t = Mat4::from_scale_rotation_translation(
            Vec3::new(2.0, 3.0, 2.0),
            glam::Quat::from_rotation_y(1.0),
            Vec3::new(100.0, 0.0, 100.0),
        );
        let s = world_sphere(&t, Vec3::new(0.0, 1.0, 0.0), 2.0);
        assert!((s.truncate() - Vec3::new(100.0, 3.0, 100.0)).length() < 1e-4);
        assert_eq!(s.w, 6.0);
        let view = RtsCamera::new([100.0, 100.0], None).view(16.0 / 9.0, &FlatGround);
        let planes = view.frustum();
        assert!(sphere_visible(&planes, s));
        assert!(!sphere_visible(&planes, Vec4::new(400.0, 0.0, 100.0, 5.0)));
        // A sphere just outside a plane but overlapping it by its radius is kept.
        assert!(sphere_visible(&planes, Vec4::new(100.0, 0.0, 30.0, 40.0)));
    }
}
