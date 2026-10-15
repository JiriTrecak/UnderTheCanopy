//! Materials shared by every model pass (statics and actors): the group-1 layout, one
//! uniform and bind group per cooked material, the texture cache and the pipeline variant
//! a material needs. The WGSL side is `shaders/lib/material.wgsl`.

use crate::{device::GpuFeatures, statics::Glow, texture};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use canopy_assets::{AlphaMode, Material, Pack, VertexColor};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

const FLAG_TEAM: u32 = 1;
const FLAG_TEAM_MASK: u32 = 2;
const FLAG_FOLIAGE: u32 = 4;
const FLAG_DOUBLE_SIDED: u32 = 8;
const FLAG_NORMAL_MAP: u32 = 16;
const FLAG_OCCLUSION_MAP: u32 = 32;
const FLAG_PAINTED: u32 = 64;
const FLAG_ALPHA_MASK: u32 = 128;
const FLAG_ALPHA_BLEND: u32 = 256;

/// Mirrors `Material` in `shaders/lib/material.wgsl`.
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

/// The pipeline a material draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Variant {
    Opaque { double_sided: bool },
    Mask { double_sided: bool },
    Blend { double_sided: bool },
}

impl Variant {
    pub const ALL: [Self; 6] = [
        Self::Opaque {
            double_sided: false,
        },
        Self::Opaque { double_sided: true },
        Self::Mask {
            double_sided: false,
        },
        Self::Mask { double_sided: true },
        Self::Blend {
            double_sided: false,
        },
        Self::Blend { double_sided: true },
    ];

    pub fn of(material: &Material) -> Self {
        let double_sided = material.double_sided;
        match material.alpha {
            AlphaMode::Opaque => Self::Opaque { double_sided },
            AlphaMode::Mask => Self::Mask { double_sided },
            AlphaMode::Blend => Self::Blend { double_sided },
        }
    }

    pub fn is_blend(self) -> bool {
        matches!(self, Self::Blend { .. })
    }

    /// The blended variant with the same faces.
    pub fn blended(self) -> Self {
        match self {
            Self::Opaque { double_sided }
            | Self::Mask { double_sided }
            | Self::Blend { double_sided } => Self::Blend { double_sided },
        }
    }

    /// Back-face culling, alpha-to-coverage and blending of the variant.
    pub fn states(self) -> (Option<wgpu::Face>, bool, Option<wgpu::BlendState>) {
        match self {
            Self::Opaque { double_sided } => {
                ((!double_sided).then_some(wgpu::Face::Back), false, None)
            }
            Self::Mask { double_sided } => {
                ((!double_sided).then_some(wgpu::Face::Back), true, None)
            }
            Self::Blend { double_sided } => (
                (!double_sided).then_some(wgpu::Face::Back),
                false,
                Some(wgpu::BlendState::ALPHA_BLENDING),
            ),
        }
    }
}

/// A material on the GPU.
pub struct GpuMaterial {
    pub bind_group: wgpu::BindGroup,
    pub variant: Variant,
    pub casts_shadow: bool,
}

/// Textures by pack entry, uploaded once, with the fallbacks unset slots use.
pub struct Textures {
    features: GpuFeatures,
    cache: HashMap<String, wgpu::TextureView>,
    white_srgb: wgpu::TextureView,
    white: wgpu::TextureView,
    white_r8: wgpu::TextureView,
    flat_normal: wgpu::TextureView,
    sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    /// Bytes of texture memory uploaded.
    pub bytes: u64,
}

/// The group-1 layout every model pipeline shares: the material uniform, six textures
/// and a sampler.
pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
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
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
    })
}

impl Textures {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, features: GpuFeatures) -> Self {
        let solid = |label, format, rgba| {
            texture::solid(device, queue, label, format, rgba).create_view(&Default::default())
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
        Self {
            features,
            cache: HashMap::new(),
            white_srgb: solid("white", wgpu::TextureFormat::Rgba8UnormSrgb, [255; 4]),
            white: solid("white linear", wgpu::TextureFormat::Rgba8Unorm, [255; 4]),
            white_r8: solid("white mask", wgpu::TextureFormat::R8Unorm, [255; 4]),
            flat_normal: solid(
                "flat normal",
                wgpu::TextureFormat::Rgba8Unorm,
                [128, 128, 255, 255],
            ),
            sampler,
            layout: layout(device),
            bytes: 0,
        }
    }

    /// The shared group-1 layout.
    pub fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    fn get(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &Pack,
        name: Option<&str>,
        fallback: &wgpu::TextureView,
    ) -> Result<wgpu::TextureView> {
        let Some(name) = name else {
            return Ok(fallback.clone());
        };
        if let Some(view) = self.cache.get(name) {
            return Ok(view.clone());
        }
        let tex = pack.texture(name).with_context(|| name.to_owned())?;
        self.bytes += tex.meta.mips.iter().map(|m| u64::from(m.len)).sum::<u64>();
        let gpu = texture::upload(device, queue, self.features, &tex);
        let view = gpu.create_view(&Default::default());
        self.cache.insert(name.to_owned(), view.clone());
        Ok(view)
    }

    /// One material's uniform and bind group (`glow`: the model's colour-keyed glow).
    pub fn material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pack: &Pack,
        material: &Material,
        glow: Option<&Glow>,
    ) -> Result<GpuMaterial> {
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
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let (white_srgb, white, white_r8, flat) = (
            self.white_srgb.clone(),
            self.white.clone(),
            self.white_r8.clone(),
            self.flat_normal.clone(),
        );
        let views = [
            self.get(device, queue, pack, t.base_color.as_deref(), &white_srgb)?,
            self.get(device, queue, pack, t.team_mask.as_deref(), &white_r8)?,
            self.get(device, queue, pack, t.normal.as_deref(), &flat)?,
            self.get(device, queue, pack, t.metallic_roughness.as_deref(), &white)?,
            self.get(device, queue, pack, t.occlusion.as_deref(), &white)?,
            self.get(device, queue, pack, t.emissive.as_deref(), &white_srgb)?,
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
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        });
        Ok(GpuMaterial {
            bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&material.name),
                layout: &self.layout,
                entries: &entries,
            }),
            variant: Variant::of(material),
            casts_shadow: material.casts_shadow,
        })
    }
}
