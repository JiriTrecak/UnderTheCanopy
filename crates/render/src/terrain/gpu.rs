//! Terrain on the GPU: textures, chunk lists and the floor around the map.

use super::{
    CHUNK, ChunkDraw, ChunkGrid, LODS, LayerKind, MAX_LAYERS, NO_LAYER, TerrainDesc, box_visible,
    foundations::{self, Foundation},
};
use crate::{camera::CameraView, device::GpuFeatures, targets, texture};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::Vec4;
use wgpu::util::DeviceExt;

/// Projected height error a chunk level may have, in pixels.
pub const MAX_ERROR_PIXELS: f32 = 1.0;
/// Level used for the terrain in the shadow map.
pub const SHADOW_LOD: u8 = 1;

const NONE: u32 = 0xffff;
const KIND_BASE: u32 = 0;
const KIND_TILED: u32 = 1;
const KIND_ATLAS: u32 = 2;
const KIND_CLIFF: u32 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct LayerUniform {
    kind: [u32; 4],
    params: [f32; 4],
    tint: [f32; 4],
    corners: [[u32; 4]; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct TerrainUniform {
    grid: [f32; 4],
    cells: [f32; 4],
    counts: [u32; 4],
    /// Painted ground: corner x, z, 1 / width, 1 / depth; zero width when absent.
    overlay: [f32; 4],
    layers: [LayerUniform; MAX_LAYERS],
}

/// One foundation in view (`foundations.rs`), as the vertex shader reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FoundationRecord {
    /// Centre x, z and half width, depth.
    rect: [f32; 4],
    /// Base height.
    base: [f32; 4],
}

impl FoundationRecord {
    fn of(f: &Foundation) -> Self {
        Self {
            rect: [f.center[0], f.center[1], f.half[0], f.half[1]],
            base: [f.base, 0.0, 0.0, 0.0],
        }
    }
}

/// The foundations' GPU side: the records in view and their pipelines.
struct FoundationsGpu {
    shown: Vec<Foundation>,
    layout: wgpu::BindGroupLayout,
    main: (wgpu::Buffer, wgpu::BindGroup),
    shadow: (wgpu::Buffer, wgpu::BindGroup),
    capacity: usize,
    main_count: u32,
    shadow_count: u32,
    forward: wgpu::RenderPipeline,
    depth: wgpu::RenderPipeline,
    records: Vec<FoundationRecord>,
}

impl FoundationsGpu {
    fn buffers(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        capacity: usize,
    ) -> [(wgpu::Buffer, wgpu::BindGroup); 2] {
        ["foundations", "foundation shadows"].map(|label| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (capacity * std::mem::size_of::<FoundationRecord>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                }],
            });
            (buffer, group)
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ChunkRecord {
    position: u32,
    levels: u32,
    extra: u32,
    pad: u32,
}

impl ChunkRecord {
    fn of(c: &ChunkDraw) -> Self {
        let [l, r, t, b] = c.neighbours;
        Self {
            position: c.x | (c.z << 16),
            levels: u32::from(c.lod)
                | (u32::from(l) << 8)
                | (u32::from(r) << 16)
                | (u32::from(t) << 24),
            extra: u32::from(b),
            pad: 0,
        }
    }
}

/// Index ranges of the chunk grid at each level.
fn lod_indices() -> (Vec<u32>, [(u32, u32); LODS]) {
    let mut indices = Vec::new();
    let mut ranges = [(0, 0); LODS];
    for (lod, range) in ranges.iter_mut().enumerate() {
        let n = CHUNK >> lod;
        let start = indices.len() as u32;
        let row = n + 1;
        for j in 0..n {
            for i in 0..n {
                let a = j * row + i;
                let b = a + 1;
                let c = a + row;
                let d = c + 1;
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
        *range = (start, indices.len() as u32 - start);
    }
    (indices, ranges)
}

/// Per-vertex normals from central differences, as the old ground normal
/// (≈ (−1.02 ∂h/∂x, 1, −1.02 ∂h/∂z)), packed as RGBA8 snorm.
fn normals(desc: &TerrainDesc) -> Vec<[i8; 4]> {
    let w = desc.vertices() as i64;
    let mut out = Vec::with_capacity((w * w) as usize);
    for z in 0..w {
        for x in 0..w {
            let gx = (desc.height_at(x + 1, z) - desc.height_at(x - 1, z)) * 0.5;
            let gz = (desc.height_at(x, z + 1) - desc.height_at(x, z - 1)) * 0.5;
            let n = glam::Vec3::new(-1.02 * gx, 1.0, -1.02 * gz).normalize();
            out.push([
                (n.x * 127.0).round() as i8,
                (n.y * 127.0).round() as i8,
                (n.z * 127.0).round() as i8,
                0,
            ]);
        }
    }
    out
}

/// The surroundings floor: three rows around the map at 15.9, 80 and 420 units beyond
/// the playable edge, sloping gently down (old `ForestSurroundingsLayer.floorGeometry`).
fn floor_mesh(desc: &TerrainDesc) -> Vec<[f32; 3]> {
    const ROWS: [f32; 3] = [15.9, 80.0, 420.0];
    const SEGMENTS: usize = 32;
    let size = desc.span as f32 - 2.0 * (-desc.origin);
    let point = |side: usize, t: f32, d: f32| -> [f32; 3] {
        let along = -d + (size + 2.0 * d) * t;
        let (x, z) = match side {
            0 => (along, -d),
            1 => (size + d, along),
            2 => (size - along, size + d),
            _ => (-d, size - along),
        };
        let y = desc.sample(x.clamp(0.0, size), z.clamp(0.0, size)) - 0.25 - (d - 16.0) * 0.008;
        [x, y, z]
    };
    let mut out = Vec::new();
    for side in 0..4 {
        for s in 0..SEGMENTS {
            let (t0, t1) = (s as f32 / SEGMENTS as f32, (s + 1) as f32 / SEGMENTS as f32);
            for r in 0..ROWS.len() - 1 {
                let (d0, d1) = (ROWS[r], ROWS[r + 1]);
                let (a, b) = (point(side, t0, d0), point(side, t1, d0));
                let (c, d) = (point(side, t0, d1), point(side, t1, d1));
                out.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
    }
    out
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FloorUniform {
    floor_color: [f32; 4],
}

/// Terrain resources and pipelines.
pub struct TerrainGpu {
    grid: ChunkGrid,
    /// The lowest ground, the floor around the map included, less a metre.
    lowest: f32,
    index_buffer: wgpu::Buffer,
    ranges: [(u32, u32); LODS],
    bind_group: wgpu::BindGroup,
    main_chunks: wgpu::Buffer,
    shadow_chunks: wgpu::Buffer,
    main_group: wgpu::BindGroup,
    shadow_group: wgpu::BindGroup,
    forward: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    floor: Option<(wgpu::Buffer, u32, wgpu::RenderPipeline)>,
    aux_group: wgpu::BindGroup,
    foundations: FoundationsGpu,
    // Scratch, reused every frame.
    draws: Vec<ChunkDraw>,
    shadow_draws: Vec<ChunkDraw>,
    levels: Vec<u8>,
    records: Vec<ChunkRecord>,
    main_counts: [(u32, u32); LODS],
    shadow_count: u32,
    /// Bytes of GPU memory.
    pub bytes: u64,
    /// Cells whose layer list had to drop a layer.
    pub crowded_cells: usize,
}

/// Counts of the last prepared frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerrainStats {
    pub chunks: u32,
    pub shadow_chunks: u32,
    pub triangles: u64,
    pub shadow_triangles: u64,
    pub draws: u32,
    /// Building foundations in view.
    pub foundations: u32,
}

impl TerrainGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        pack: &canopy_assets::Pack,
        desc: &TerrainDesc,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        let _span = tracing::info_span!("terrain_upload").entered();
        desc.validate().map_err(anyhow::Error::msg)?;
        let vertices = desc.vertices();
        let mut bytes = 0u64;
        let tex2d = |label: &str, format, w: u32, h: u32, layers: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let write = |tex: &wgpu::Texture, layer: u32, data: &[u8], row: u32, w: u32, h: u32| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        };

        let heights = tex2d(
            "terrain heights",
            wgpu::TextureFormat::R32Float,
            vertices,
            vertices,
            1,
        );
        write(
            &heights,
            0,
            bytemuck::cast_slice(&desc.heights),
            vertices * 4,
            vertices,
            vertices,
        );
        let normal_tex = tex2d(
            "terrain normals",
            wgpu::TextureFormat::Rgba8Snorm,
            vertices,
            vertices,
            1,
        );
        write(
            &normal_tex,
            0,
            bytemuck::cast_slice(&normals(desc)),
            vertices * 4,
            vertices,
            vertices,
        );
        bytes += u64::from(vertices * vertices) * 8;

        let layer_count = desc.layers.len() as u32;
        let masks = tex2d(
            "terrain masks",
            wgpu::TextureFormat::R8Unorm,
            vertices,
            vertices,
            layer_count,
        );
        for (i, layer) in desc.layers.iter().enumerate() {
            write(&masks, i as u32, &layer.mask, vertices, vertices, vertices);
        }
        bytes += u64::from(vertices * vertices * layer_count);

        let atlas_layers: Vec<usize> = desc
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l.kind, LayerKind::Atlas { .. }))
            .map(|(i, _)| i)
            .collect();
        let [cx, cz] = desc.grid.cells;
        let atlas = tex2d(
            "terrain atlas cells",
            wgpu::TextureFormat::Rg8Uint,
            cx.max(1),
            cz.max(1),
            atlas_layers.len().max(1) as u32,
        );
        for (slot, &i) in atlas_layers.iter().enumerate() {
            write(
                &atlas,
                slot as u32,
                bytemuck::cast_slice(&desc.layers[i].cells),
                cx * 2,
                cx,
                cz,
            );
        }

        let (selection, crowded_cells) = super::select_layers(desc);
        if crowded_cells > 0 {
            tracing::warn!(crowded_cells, "terrain cells reached by more than 4 layers");
        }
        let select_tex = tex2d(
            "terrain layer lists",
            wgpu::TextureFormat::Rgba8Uint,
            desc.span,
            desc.span,
            1,
        );
        write(
            &select_tex,
            0,
            bytemuck::cast_slice(&selection),
            desc.span * 4,
            desc.span,
            desc.span,
        );
        bytes += u64::from(desc.span * desc.span * 4);

        // Tile arrays: only layers some cell uses are loaded.
        let used: Vec<bool> = (0..desc.layers.len())
            .map(|i| {
                selection
                    .iter()
                    .any(|s| s.contains(&(i as u8)) && i as u8 != NO_LAYER)
            })
            .collect();
        let mut albedo_names: Vec<String> = Vec::new();
        let mut normal_names: Vec<String> = Vec::new();
        let mut occlusion_names: Vec<String> = Vec::new();
        let index_of = |names: &mut Vec<String>, name: &str| -> u32 {
            match names.iter().position(|n| n == name) {
                Some(i) => i as u32,
                None => {
                    names.push(name.to_owned());
                    names.len() as u32 - 1
                }
            }
        };
        let mut layer_uniforms = [LayerUniform::zeroed(); MAX_LAYERS];
        let mut atlas_slot = 0u32;
        for (i, layer) in desc.layers.iter().enumerate() {
            let (kind, params, corners) = match &layer.kind {
                LayerKind::Base { tiling } => (KIND_BASE, [*tiling, -1.0, 0.0, 0.0], [0u8; 16]),
                LayerKind::Tiled {
                    tiling,
                    blend,
                    edge,
                    breakup,
                } => (KIND_TILED, [*tiling, *blend, *edge, *breakup], [0; 16]),
                LayerKind::Atlas {
                    columns,
                    rows,
                    corners,
                } => {
                    let slot = atlas_slot;
                    atlas_slot += 1;
                    (
                        KIND_ATLAS,
                        [*columns as f32, *rows as f32, slot as f32, 0.0],
                        *corners,
                    )
                }
                LayerKind::Cliff {
                    start_slope,
                    full_slope,
                    vertical_size,
                } => (
                    KIND_CLIFF,
                    [*start_slope, *full_slope, *vertical_size, 0.0],
                    [0; 16],
                ),
            };
            let (ar, nh, om) = if used[i] {
                (
                    index_of(&mut albedo_names, &layer.albedo),
                    index_of(&mut normal_names, &layer.normal),
                    layer
                        .occlusion
                        .as_deref()
                        .map_or(NONE, |name| index_of(&mut occlusion_names, name)),
                )
            } else {
                (NONE, NONE, NONE)
            };
            let mut packed = [[0u32; 4]; 4];
            for (m, &tile) in corners.iter().enumerate() {
                packed[m / 4][m % 4] = u32::from(tile);
            }
            layer_uniforms[i] = LayerUniform {
                kind: [kind, ar, nh, om],
                params,
                tint: [
                    layer.tint[0],
                    layer.tint[1],
                    layer.tint[2],
                    if layer.painted { 1.0 } else { 0.0 },
                ],
                corners: packed,
            };
        }
        let array = |label: &str, names: &[String]| -> Result<wgpu::Texture> {
            if names.is_empty() {
                return Ok(texture::solid(
                    device,
                    queue,
                    label,
                    wgpu::TextureFormat::Rgba8Unorm,
                    [255, 0, 0, 255],
                ));
            }
            let (tex, size) = texture::upload_array(device, queue, features, pack, label, names)?;
            tracing::debug!(label, layers = names.len(), size, "terrain tiles");
            Ok(tex)
        };
        let albedo = array("terrain albedo", &albedo_names)?;
        let normal = array("terrain normal", &normal_names)?;
        let occlusion = array("terrain occlusion", &occlusion_names)?;
        for (names, t) in [
            (&albedo_names, &albedo),
            (&normal_names, &normal),
            (&occlusion_names, &occlusion),
        ] {
            let per_layer: u64 = (0..t.mip_level_count())
                .map(|l| {
                    let (w, h) = ((t.width() >> l).max(1), (t.height() >> l).max(1));
                    u64::from(w.div_ceil(4) * h.div_ceil(4) * 16)
                })
                .sum();
            bytes += per_layer * names.len().max(1) as u64;
        }

        let uniform = TerrainUniform {
            grid: [
                desc.origin,
                desc.span as f32,
                vertices as f32,
                1.0 / vertices as f32,
            ],
            cells: [
                desc.grid.start[0],
                desc.grid.start[1],
                desc.grid.cell_size,
                super::selection_offset(desc),
            ],
            counts: [cx, cz, layer_count, 0],
            overlay: desc.overlay.as_ref().map_or([0.0; 4], |o| {
                [o.rect[0], o.rect[1], 1.0 / o.rect[2], 1.0 / o.rect[3]]
            }),
            layers: layer_uniforms,
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let clamp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("terrain clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let tile_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("terrain tiles"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let tex_entry = |binding, visibility, sample_type, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: dim,
                multisampled: false,
            },
            count: None,
        };
        let filterable = wgpu::TextureSampleType::Float { filterable: true };
        let d2 = wgpu::TextureViewDimension::D2;
        let d2a = wgpu::TextureViewDimension::D2Array;
        let frag = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain"),
            entries: &[
                tex_entry(
                    0,
                    vf,
                    wgpu::TextureSampleType::Float { filterable: false },
                    d2,
                ),
                tex_entry(1, frag, filterable, d2),
                tex_entry(2, frag, filterable, d2a),
                tex_entry(3, frag, wgpu::TextureSampleType::Uint, d2a),
                tex_entry(4, frag, wgpu::TextureSampleType::Uint, d2),
                tex_entry(5, frag, filterable, d2a),
                tex_entry(6, frag, filterable, d2a),
                tex_entry(7, frag, filterable, d2a),
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(11, frag, filterable, d2),
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: vf,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let array_view = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let plain = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let views = [
            plain(&heights),
            plain(&normal_tex),
            array_view(&masks),
            array_view(&atlas),
            plain(&select_tex),
            array_view(&albedo),
            array_view(&normal),
            array_view(&occlusion),
        ];
        let mut entries: Vec<wgpu::BindGroupEntry> = views
            .iter()
            .enumerate()
            .map(|(i, v)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: wgpu::BindingResource::TextureView(v),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 8,
            resource: wgpu::BindingResource::Sampler(&clamp_sampler),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 9,
            resource: wgpu::BindingResource::Sampler(&tile_sampler),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 10,
            resource: uniform_buffer.as_entire_binding(),
        });
        let overlay_texture = match &desc.overlay {
            Some(o) => texture::upload(
                device,
                queue,
                features,
                &pack
                    .texture(&o.texture)
                    .with_context(|| o.texture.clone())?,
            ),
            None => texture::solid(
                device,
                queue,
                "no overlay",
                wgpu::TextureFormat::Rgba8UnormSrgb,
                [0; 4],
            ),
        };
        let overlay_view = overlay_texture.create_view(&Default::default());
        entries.push(wgpu::BindGroupEntry {
            binding: 11,
            resource: wgpu::BindingResource::TextureView(&overlay_view),
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout: &layout,
            entries: &entries,
        });

        let chunk_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain chunks"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let grid = ChunkGrid::new(desc);
        let capacity =
            u64::from(grid.chunks * grid.chunks) * std::mem::size_of::<ChunkRecord>() as u64;
        let chunk_buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let main_chunks = chunk_buffer("terrain chunks");
        let shadow_chunks = chunk_buffer("terrain shadow chunks");
        let group = |buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("terrain chunks"),
                layout: &chunk_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let main_group = group(&main_chunks);
        let shadow_group = group(&shadow_chunks);

        let (indices, ranges) = lod_indices();
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        bytes += (indices.len() * 4) as u64 + capacity * 2;

        let shader = crate::shader::module(
            device,
            "terrain",
            crate::shader::LIT,
            include_str!("../../shaders/terrain.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout), Some(&chunk_layout)],
            immediate_size: 0,
        });
        let primitive = wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        };
        let forward = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive,
            depth_stencil: Some(targets::scene_depth(true)),
            multisample: targets::scene_multisample(false),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &targets::forward_targets(None, true),
            }),
            multiview_mask: None,
            cache: None,
        });
        let shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terrain shadow"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(targets::shadow_depth()),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        let foundation_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("foundations"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let foundation_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("foundations"),
                bind_group_layouts: &[Some(frame_layout), Some(&layout), Some(&foundation_layout)],
                immediate_size: 0,
            });
        let foundation_forward = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("foundations"),
            layout: Some(&foundation_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_foundation"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(targets::scene_depth(true)),
            multisample: targets::scene_multisample(false),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_foundation"),
                compilation_options: Default::default(),
                targets: &targets::forward_targets(None, true),
            }),
            multiview_mask: None,
            cache: None,
        });
        let foundation_depth = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("foundation shadows"),
            layout: Some(&foundation_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_foundation_shadow"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(targets::shadow_depth()),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let capacity = 64;
        let [foundation_main, foundation_shadow] =
            FoundationsGpu::buffers(device, &foundation_layout, capacity);
        let foundations = FoundationsGpu {
            shown: Vec::new(),
            layout: foundation_layout,
            main: foundation_main,
            shadow: foundation_shadow,
            capacity,
            main_count: 0,
            shadow_count: 0,
            forward: foundation_forward,
            depth: foundation_depth,
            records: Vec::new(),
        };

        let simple = |label: &str,
                      source: &str,
                      blend: Option<wgpu::BlendState>,
                      write_depth: bool,
                      layouts: &[Option<&wgpu::BindGroupLayout>]| {
            let module = crate::shader::module(device, label, crate::shader::LIT, source);
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 12,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(targets::scene_depth(write_depth)),
                multisample: targets::scene_multisample(false),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &targets::forward_targets(blend, blend.is_none()),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let floor_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("floor"),
            entries: &[
                tex_entry(
                    0,
                    frag,
                    wgpu::TextureSampleType::Float { filterable: false },
                    d2,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: frag,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(2, frag, filterable, d2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: vf,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let floor_color = desc
            .surroundings
            .as_ref()
            .map_or([1.0; 3], |s| s.floor_color);
        let floor_uniform = FloorUniform {
            floor_color: [floor_color[0], floor_color[1], floor_color[2], 0.0],
        };
        let floor_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("floor look"),
            contents: bytemuck::bytes_of(&floor_uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let floor_texture = match &desc.surroundings {
            Some(s) => Some(texture::upload(
                device,
                queue,
                features,
                &pack
                    .texture(&s.floor_texture)
                    .with_context(|| s.floor_texture.clone())?,
            )),
            None => None,
        };
        let floor_view = floor_texture
            .as_ref()
            .unwrap_or(&texture::solid(
                device,
                queue,
                "no floor",
                wgpu::TextureFormat::Rgba8UnormSrgb,
                [255; 4],
            ))
            .create_view(&Default::default());
        let floor_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("floor"),
            layout: &floor_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: floor_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&floor_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&tile_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });
        let mut lowest = desc.heights.iter().copied().fold(f32::MAX, f32::min);
        let floor = desc.surroundings.as_ref().map(|_| {
            let mesh = floor_mesh(desc);
            lowest = mesh.iter().fold(lowest, |low, v| low.min(v[1]));
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("floor"),
                contents: bytemuck::cast_slice(&mesh),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let pipeline = simple(
                "floor",
                include_str!("../../shaders/floor.wgsl"),
                None,
                true,
                &[Some(frame_layout), Some(&floor_layout)],
            );
            (buffer, mesh.len() as u32, pipeline)
        });

        Ok(Self {
            lowest: lowest - 1.0,
            draws: Vec::with_capacity((grid.chunks * grid.chunks) as usize),
            shadow_draws: Vec::with_capacity((grid.chunks * grid.chunks) as usize),
            levels: Vec::with_capacity((grid.chunks * grid.chunks) as usize),
            records: Vec::with_capacity((grid.chunks * grid.chunks) as usize),
            grid,
            index_buffer,
            ranges,
            bind_group,
            main_chunks,
            shadow_chunks,
            main_group,
            shadow_group,
            forward,
            shadow,
            floor,
            aux_group: floor_group,
            foundations,
            main_counts: [(0, 0); LODS],
            shadow_count: 0,
            bytes,
            crowded_cells,
        })
    }

    /// Chooses chunks and levels for the camera and the shadow view and uploads them.
    /// The lowest ground, the floor around the map included, less a metre: nothing below
    /// it can be seen.
    pub fn lowest(&self) -> f32 {
        self.lowest
    }

    pub fn prepare(
        &mut self,
        queue: &impl crate::upload::Writes,
        camera: &CameraView,
        screen_height: f32,
        shadow_planes: Option<&[Vec4; 6]>,
    ) -> TerrainStats {
        let _span = tracing::trace_span!("terrain_prepare").entered();
        self.grid.select(
            camera,
            screen_height,
            MAX_ERROR_PIXELS,
            &mut self.draws,
            &mut self.levels,
        );
        self.records.clear();
        self.records.extend(self.draws.iter().map(ChunkRecord::of));
        if !self.records.is_empty() {
            queue.write_buffer(&self.main_chunks, 0, bytemuck::cast_slice(&self.records));
        }
        self.main_counts = [(0, 0); LODS];
        let mut start = 0u32;
        for lod in 0..LODS {
            let count = self.draws.iter().filter(|d| d.lod as usize == lod).count() as u32;
            self.main_counts[lod] = (start, count);
            start += count;
        }
        self.shadow_count = 0;
        if let Some(planes) = shadow_planes {
            self.grid
                .visible_in(planes, SHADOW_LOD, &mut self.shadow_draws);
            self.records.clear();
            self.records
                .extend(self.shadow_draws.iter().map(ChunkRecord::of));
            if !self.records.is_empty() {
                queue.write_buffer(&self.shadow_chunks, 0, bytemuck::cast_slice(&self.records));
            }
            self.shadow_count = self.records.len() as u32;
        }
        self.prepare_foundations(queue, camera, shadow_planes);
        let triangles = self
            .main_counts
            .iter()
            .enumerate()
            .map(|(lod, (_, n))| u64::from(*n) * u64::from(self.ranges[lod].1 / 3))
            .sum();
        TerrainStats {
            chunks: self.draws.len() as u32,
            shadow_chunks: self.shadow_count,
            triangles,
            shadow_triangles: u64::from(self.shadow_count)
                * u64::from(self.ranges[SHADOW_LOD as usize].1 / 3),
            draws: self.main_counts.iter().filter(|c| c.1 > 0).count() as u32,
            foundations: self.foundations.main_count,
        }
    }

    /// The building foundations to draw (D44); only a changed list costs anything.
    pub fn set_foundations(&mut self, device: &wgpu::Device, list: &[Foundation]) {
        let f = &mut self.foundations;
        if f.shown == list {
            return;
        }
        if list.len() > f.capacity {
            f.capacity = list.len().next_power_of_two();
            [f.main, f.shadow] = FoundationsGpu::buffers(device, &f.layout, f.capacity);
        }
        f.shown.clear();
        f.shown.extend_from_slice(list);
    }

    /// Culls the foundations against the camera and the sun's view and uploads them.
    fn prepare_foundations(
        &mut self,
        queue: &impl crate::upload::Writes,
        camera: &CameraView,
        shadow_planes: Option<&[Vec4; 6]>,
    ) {
        let f = &mut self.foundations;
        let mut upload = |planes: &[Vec4; 6], buffer: &wgpu::Buffer| -> u32 {
            f.records.clear();
            f.records.extend(
                f.shown
                    .iter()
                    .filter(|x| {
                        let (min, max) = x.bounds();
                        box_visible(planes, min, max)
                    })
                    .map(FoundationRecord::of),
            );
            if !f.records.is_empty() {
                queue.write_buffer(buffer, 0, bytemuck::cast_slice(&f.records));
            }
            f.records.len() as u32
        };
        f.main_count = upload(&camera.frustum(), &f.main.0);
        f.shadow_count = match shadow_planes {
            Some(planes) => upload(planes, &f.shadow.0),
            None => 0,
        };
    }

    pub fn draw_forward<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame: &'a wgpu::BindGroup,
    ) -> u32 {
        pass.set_pipeline(&self.forward);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        pass.set_bind_group(2, &self.main_group, &[]);
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let mut draws = 0;
        for (lod, &(start, count)) in self.main_counts.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let (first, n) = self.ranges[lod];
            pass.draw_indexed(first..first + n, 0, start..start + count);
            draws += 1;
        }
        let f = &self.foundations;
        if f.main_count > 0 {
            pass.set_pipeline(&f.forward);
            pass.set_bind_group(2, &f.main.1, &[]);
            pass.draw(0..foundations::VERTICES, 0..f.main_count);
            draws += 1;
        }
        if let Some((buffer, n, pipeline)) = &self.floor {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(1, &self.aux_group, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..*n, 0..1);
            draws += 1;
        }
        draws
    }

    pub fn draw_shadow<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame: &'a wgpu::BindGroup,
    ) -> u32 {
        let mut draws = 0;
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);
        if self.shadow_count > 0 {
            pass.set_pipeline(&self.shadow);
            pass.set_bind_group(2, &self.shadow_group, &[]);
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            let (first, n) = self.ranges[SHADOW_LOD as usize];
            pass.draw_indexed(first..first + n, 0, 0..self.shadow_count);
            draws += 1;
        }
        let f = &self.foundations;
        if f.shadow_count > 0 {
            pass.set_pipeline(&f.depth);
            pass.set_bind_group(2, &f.shadow.1, &[]);
            pass.draw(0..foundations::VERTICES, 0..f.shadow_count);
            draws += 1;
        }
        draws
    }

    /// The focus ground height and terrain queries for cameras.
    pub fn chunk_grid(&self) -> &ChunkGrid {
        &self.grid
    }
}
