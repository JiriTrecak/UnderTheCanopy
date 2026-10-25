//! Water on the GPU: the surface (chunked index lists over the terrain's vertex grid,
//! vertex-pulled from level and depth textures), the ripple field and the caustics.

use super::{
    WaterDesc, WaterMesh, f16_bits, hash_lattice, profile_rows,
    ripples::{self, RingInstance, Ripples},
    vertex_fields,
};
use crate::{
    camera::CameraView,
    device::GpuFeatures,
    targets,
    terrain::{CHUNK, TerrainDesc, box_visible},
    texture,
};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;

/// Texels per side of the caustics texture (old: 256).
pub const CAUSTICS_SIZE: u32 = 256;
/// Formats of the generated fields.
pub const RIPPLE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const CAUSTICS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// The old woodland reflection cube in the pack: an entry of RGBA8 sRGB faces (+X, −X,
/// +Y, −Y, +Z, −Z) for every level, largest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionCube {
    pub entry: String,
    pub size: u32,
    pub levels: u32,
}

/// Pack textures the water samples. Absent ones fall back to neutral values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WaterTextures {
    pub reflection: Option<ReflectionCube>,
    /// The caustics source image (RGB channels used as distortion and light).
    pub caustics: Option<String>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct WaterInfo {
    grid: [f32; 4],
    ripples: [f32; 4],
    ambient: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FieldUniform {
    bounds: [f32; 4],
}

/// Counts of the last prepared frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WaterStats {
    pub cells: u32,
    pub draws: u32,
    pub rings: u32,
}

pub struct WaterGpu {
    mesh: WaterMesh,
    origin: f32,
    index_buffer: wgpu::Buffer,
    absorb: wgpu::RenderPipeline,
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    info: wgpu::Buffer,
    draws: Vec<(u32, u32)>,
    ripple_view: wgpu::TextureView,
    ripple_pipeline: wgpu::RenderPipeline,
    ripple_group: wgpu::BindGroup,
    ripple_buffer: wgpu::Buffer,
    ripple_uniform: wgpu::Buffer,
    rings: Vec<RingInstance>,
    /// The field holds rings from an earlier frame and must be cleared.
    ripples_dirty: bool,
    caustics_view: wgpu::TextureView,
    caustics_pipeline: wgpu::RenderPipeline,
    caustics_group: wgpu::BindGroup,
    caustics_uniform: wgpu::Buffer,
    /// Bytes of GPU memory.
    pub bytes: u64,
    pub stats: WaterStats,
}

fn texture_2d(
    device: &wgpu::Device,
    label: &str,
    format: wgpu::TextureFormat,
    size: [u32; 2],
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
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
        usage,
        view_formats: &[],
    })
}

fn write_2d(queue: &wgpu::Queue, texture: &wgpu::Texture, bytes: &[u8], row: u32, size: [u32; 2]) {
    queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(row),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

fn entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    ty: wgpu::BindingType,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    }
}

fn tex(
    sample_type: wgpu::TextureSampleType,
    view_dimension: wgpu::TextureViewDimension,
) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type,
        view_dimension,
        multisampled: false,
    }
}

const FILTERABLE: wgpu::TextureSampleType = wgpu::TextureSampleType::Float { filterable: true };
const UNFILTERED: wgpu::TextureSampleType = wgpu::TextureSampleType::Float { filterable: false };
const D2: wgpu::TextureViewDimension = wgpu::TextureViewDimension::D2;

fn uniform() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn storage() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

/// The reflection cube from the pack, or a neutral grey sky.
fn reflection_cube(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pack: &canopy_assets::Pack,
    cube: Option<&ReflectionCube>,
) -> Result<(wgpu::TextureView, u64)> {
    let (size, levels, bytes) = match cube {
        Some(c) => {
            let data = pack
                .get(&c.entry)
                .and_then(|b| b.bytes().map(<[u8]>::to_vec))
                .with_context(|| c.entry.clone())?;
            let expected: u64 = (0..c.levels)
                .map(|l| 6 * u64::from((c.size >> l).max(1)).pow(2) * 4)
                .sum();
            if data.len() as u64 != expected {
                anyhow::bail!(
                    "{}: {} bytes, a {}² cube with {} levels has {expected}",
                    c.entry,
                    data.len(),
                    c.size,
                    c.levels
                );
            }
            (c.size, c.levels, data)
        }
        None => (1, 1, [140u8, 150, 150, 255].repeat(6)),
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("water reflection"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut offset = 0usize;
    for level in 0..levels {
        let s = (size >> level).max(1);
        let face = (s * s * 4) as usize;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes[offset..offset + face * 6],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(s * 4),
                rows_per_image: Some(s),
            },
            wgpu::Extent3d {
                width: s,
                height: s,
                depth_or_array_layers: 6,
            },
        );
        offset += face * 6;
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    });
    Ok((view, bytes.len() as u64))
}

impl WaterGpu {
    /// Uploads the water of `terrain`; `None` when nothing is wet.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        pack: &canopy_assets::Pack,
        terrain: &TerrainDesc,
        water: &WaterDesc,
        textures: &WaterTextures,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Result<Option<Self>> {
        let _span = tracing::info_span!("water_upload").entered();
        water.validate(terrain).map_err(anyhow::Error::msg)?;
        let mesh = WaterMesh::build(terrain);
        if mesh.indices.is_empty() {
            return Ok(None);
        }
        let mut bytes = 0u64;
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let n = terrain.vertices();
        let span = terrain.span;

        let (levels, depths) = vertex_fields(terrain);
        let level_texture = texture_2d(
            device,
            "water levels",
            wgpu::TextureFormat::R32Float,
            [n, n],
            usage,
        );
        write_2d(
            queue,
            &level_texture,
            bytemuck::cast_slice(&levels),
            n * 4,
            [n, n],
        );
        // Depth and distance to the shore, together.
        let shore = crate::water::shore_distance(&depths, n);
        let depth_bits: Vec<u16> = depths
            .iter()
            .zip(&shore)
            .flat_map(|(&d, &s)| [f16_bits(d), f16_bits(s)])
            .collect();
        // Rows of 2-byte texels must be a multiple of 256 bytes for buffer copies, not for
        // `write_texture`, so the grid uploads as is.
        let depth_texture = texture_2d(
            device,
            "water depth",
            wgpu::TextureFormat::Rg16Float,
            [n, n],
            usage,
        );
        write_2d(
            queue,
            &depth_texture,
            bytemuck::cast_slice(&depth_bits),
            n * 4,
            [n, n],
        );
        let flow_texture = texture_2d(
            device,
            "water flow",
            wgpu::TextureFormat::Rgba8Unorm,
            [span, span],
            usage,
        );
        write_2d(
            queue,
            &flow_texture,
            bytemuck::cast_slice(&water.flow),
            span * 4,
            [span, span],
        );
        let hash_texture = texture_2d(
            device,
            "water hash",
            wgpu::TextureFormat::Rg8Unorm,
            [256, 256],
            usage,
        );
        write_2d(queue, &hash_texture, &hash_lattice(), 512, [256, 256]);
        bytes += u64::from(n * n) * 8 + u64::from(span * span) * 4 + 256 * 256 * 2;

        let (reflection, cube_bytes) =
            reflection_cube(device, queue, pack, textures.reflection.as_ref())?;
        bytes += cube_bytes;
        let caustics_source = match &textures.caustics {
            Some(name) => {
                let t = pack.texture(name).with_context(|| name.clone())?;
                bytes += t.meta.mips.iter().map(|m| m.len as u64).sum::<u64>();
                texture::upload(device, queue, features, &t)
            }
            None => texture::solid(
                device,
                queue,
                "no caustics",
                wgpu::TextureFormat::Rgba8Unorm,
                [128, 128, 0, 255],
            ),
        };
        let rows: Vec<[[f32; 4]; 8]> = water.profiles.iter().map(profile_rows).collect();
        let profiles = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water profiles"),
            contents: bytemuck::cast_slice(&rows),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let info = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water info"),
            size: std::mem::size_of::<WaterInfo>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        bytes += mesh.indices.len() as u64 * 4;

        let clamp_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let repeat_linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Ripple field and caustics targets.
        let target_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let r = ripples::RESOLUTION;
        let ripple_view = texture_2d(device, "ripple field", RIPPLE_FORMAT, [r, r], target_usage)
            .create_view(&Default::default());
        let caustics_view = texture_2d(
            device,
            "caustics",
            CAUSTICS_FORMAT,
            [CAUSTICS_SIZE, CAUSTICS_SIZE],
            target_usage,
        )
        .create_view(&Default::default());
        bytes += u64::from(r * r) * 8 + u64::from(CAUSTICS_SIZE * CAUSTICS_SIZE) * 2;

        let frag = wgpu::ShaderStages::FRAGMENT;
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water"),
            entries: &[
                entry(0, vf, tex(UNFILTERED, D2)),
                entry(1, vf, tex(FILTERABLE, D2)),
                entry(2, vf, tex(FILTERABLE, D2)),
                entry(3, vf, tex(FILTERABLE, D2)),
                entry(4, frag, tex(FILTERABLE, D2)),
                entry(5, frag, tex(FILTERABLE, D2)),
                entry(6, frag, tex(FILTERABLE, wgpu::TextureViewDimension::Cube)),
                entry(7, vf, storage()),
                entry(8, vf, uniform()),
                entry(
                    9,
                    vf,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let views = [
            view(&level_texture),
            view(&depth_texture),
            view(&flow_texture),
            view(&hash_texture),
        ];
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&views[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&caustics_view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&ripple_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&reflection),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: profiles.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: info.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Sampler(&clamp_linear),
                },
            ],
        });
        let module = crate::shader::module(
            device,
            "water",
            crate::shader::LIT,
            include_str!("../../shaders/water.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let keep_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        // First draw: the bed times the absorption's hue.
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::Src,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: keep_alpha,
        };
        // Second draw: premultiplied surface over the dimmed bed.
        let over = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: keep_alpha,
        };
        let surface = |entry: &str, blend: wgpu::BlendState, depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("water"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(targets::scene_depth(false)),
                multisample: targets::scene_multisample(false),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &targets::forward_targets(Some(blend), depth),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let absorb = surface("fs_absorb", multiply, false);
        // The surface writes its depth: post effects see the water, not the bed.
        let pipeline = surface("fs_main", over, true);

        // Ripples: one instanced additive draw.
        let ripple_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ripples"),
            entries: &[
                entry(0, wgpu::ShaderStages::VERTEX, uniform()),
                entry(1, wgpu::ShaderStages::VERTEX, storage()),
            ],
        });
        let ripple_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rings"),
            size: (ripples::CAPACITY * std::mem::size_of::<RingInstance>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ripple_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ripple field"),
            size: std::mem::size_of::<FieldUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        bytes += ripple_buffer.size();
        let ripple_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ripples"),
            layout: &ripple_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ripple_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ripple_buffer.as_entire_binding(),
                },
            ],
        });
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let ripple_pipeline = fullscreen_like(
            device,
            "ripples",
            &[],
            include_str!("../../shaders/ripples.wgsl"),
            &ripple_layout,
            RIPPLE_FORMAT,
            Some(wgpu::BlendState {
                color: additive,
                alpha: additive,
            }),
        );

        // Caustics.
        let caustics_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("caustics"),
            entries: &[
                entry(0, frag, tex(FILTERABLE, D2)),
                entry(
                    1,
                    frag,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(2, frag, uniform()),
            ],
        });
        let caustics_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("caustics"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let caustics_source_view = caustics_source.create_view(&Default::default());
        let caustics_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("caustics"),
            layout: &caustics_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&caustics_source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&repeat_linear),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: caustics_uniform.as_entire_binding(),
                },
            ],
        });
        let caustics_pipeline = fullscreen_like(
            device,
            "caustics",
            &[crate::shader::Module::Color],
            include_str!("../../shaders/caustics.wgsl"),
            &caustics_layout,
            CAUSTICS_FORMAT,
            None,
        );
        tracing::info!(cells = mesh.cells(), "water");
        Ok(Some(Self {
            draws: Vec::with_capacity(mesh.ranges.len()),
            origin: terrain.origin,
            mesh,
            index_buffer,
            absorb,
            pipeline,
            group,
            info,
            ripple_view,
            ripple_pipeline,
            ripple_group,
            ripple_buffer,
            ripple_uniform,
            rings: Vec::with_capacity(ripples::CAPACITY),
            ripples_dirty: true,
            caustics_view,
            caustics_pipeline,
            caustics_group,
            caustics_uniform,
            bytes,
            stats: WaterStats::default(),
        }))
    }

    /// Culls chunks for `camera` and uploads this frame's values and live rings.
    pub fn prepare(
        &mut self,
        queue: &impl crate::upload::Writes,
        camera: &CameraView,
        seconds: f32,
        ripples: &Ripples,
        daylight_ambient: Vec3,
    ) -> WaterStats {
        let _span = tracing::trace_span!("water_prepare").entered();
        let planes = camera.frustum();
        let (chunks, origin) = (self.mesh.chunks as usize, self.origin);
        let size = CHUNK as f32;
        let levels = &self.mesh.levels;
        // Swells stay within half a unit; the margin also covers their crests.
        self.mesh.draw_ranges(
            |c| {
                let (cx, cz) = ((c % chunks) as f32, (c / chunks) as f32);
                let [low, high] = levels[c];
                box_visible(
                    &planes,
                    Vec3::new(origin + cx * size, low - 1.0, origin + cz * size),
                    Vec3::new(
                        origin + (cx + 1.0) * size,
                        high + 1.0,
                        origin + (cz + 1.0) * size,
                    ),
                )
            },
            &mut self.draws,
        );
        let half = ripples::SPAN / 2.0;
        let focus = [camera.target.x - half, camera.target.z - half];
        let span = self.mesh.chunks * CHUNK;
        let info = WaterInfo {
            grid: [origin, span as f32, (span + 1) as f32, 0.0],
            ripples: [focus[0], focus[1], 1.0 / ripples::SPAN, 0.0],
            ambient: [
                daylight_ambient.x,
                daylight_ambient.y,
                daylight_ambient.z,
                0.0,
            ],
        };
        queue.write_buffer(&self.info, 0, bytemuck::bytes_of(&info));
        queue.write_buffer(
            &self.ripple_uniform,
            0,
            bytemuck::bytes_of(&FieldUniform {
                bounds: [focus[0], focus[1], ripples::SPAN, 0.0],
            }),
        );
        ripples.live(seconds, &mut self.rings);
        if !self.rings.is_empty() {
            queue.write_buffer(&self.ripple_buffer, 0, bytemuck::cast_slice(&self.rings));
        }
        queue.write_buffer(
            &self.caustics_uniform,
            0,
            bytemuck::cast_slice(&[seconds, 0.0, 0.0, 0.0]),
        );
        self.stats = WaterStats {
            cells: self.draws.iter().map(|d| d.1 / 6).sum(),
            draws: self.draws.len() as u32,
            rings: self.rings.len() as u32,
        };
        self.stats
    }

    /// Whether the ripple pass has work: live rings, or a field to clear.
    pub fn ripples_pending(&self) -> bool {
        !self.rings.is_empty() || self.ripples_dirty
    }

    /// Draws the live rings into the ripple field (cleared first).
    pub fn encode_ripples(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ripples"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.ripple_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.ripples_dirty = !self.rings.is_empty();
        if self.rings.is_empty() {
            return 0;
        }
        rp.set_pipeline(&self.ripple_pipeline);
        rp.set_bind_group(0, &self.ripple_group, &[]);
        rp.draw(0..6, 0..self.rings.len() as u32);
        1
    }

    /// Renders this frame's caustics.
    pub fn encode_caustics(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> u32 {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("caustics"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.caustics_view,
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
        rp.set_pipeline(&self.caustics_pipeline);
        rp.set_bind_group(0, &self.caustics_group, &[]);
        rp.draw(0..3, 0..1);
        1
    }

    /// The water surface, after every opaque surface of the forward pass: the
    /// absorption over the bed, then the surface.
    /// Triangles of the latest prepared frame (both draws).
    pub fn triangles(&self) -> u64 {
        2 * self
            .draws
            .iter()
            .map(|&(_, n)| u64::from(n / 3))
            .sum::<u64>()
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) -> u32 {
        if self.draws.is_empty() {
            return 0;
        }
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.group, &[]);
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for pipeline in [&self.absorb, &self.pipeline] {
            pass.set_pipeline(pipeline);
            for &(start, count) in &self.draws {
                pass.draw_indexed(start..start + count, 0, 0..1);
            }
        }
        2 * self.draws.len() as u32
    }
}

/// A pipeline drawing into one single-sample target with no vertex buffers.
fn fullscreen_like(
    device: &wgpu::Device,
    label: &str,
    modules: &[crate::shader::Module],
    source: &str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let module = crate::shader::module(device, label, modules, source);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
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
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
