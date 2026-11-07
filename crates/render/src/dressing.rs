//! What a map's blocks are dressed with beyond the terrain (D80): the generated cliff
//! faces and rubble, one textured mesh lit like scenery and casting shadows, and decals,
//! flat cut-outs pressed onto the ground in one instanced draw per atlas. The scene
//! (`canopy_scene::generate`) builds both from the blocks; the renderer only draws them.
//! Render only.
//!
//! A map's cliff meshes hold every cliff, rock and stone of the map (about 900,000
//! triangles on frozen-dream). Their index lists are cut into runs of consecutive,
//! neighbouring triangles when they load, and the sun's map draws only the runs whose
//! shadow can reach the camera's view ([`CasterCull`]): a depth map keeps the nearest
//! depth whatever the order or grouping of its triangles, so it comes out the same. The
//! camera's view still draws each mesh whole in one call: Apple GPUs resolve some pixels
//! where its rocks meet differently once a mesh's triangles are left out or split.

use crate::{device::GpuFeatures, shadow::CasterCull, targets, texture};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use canopy_assets::Pack;
use glam::{Vec3, Vec4};
use std::ops::Range;
use wgpu::util::DeviceExt;

/// Widest a cliff run may spread over the ground, metres.
const RUN_SPAN: f32 = 16.0;
/// Most triangles in a cliff run.
const RUN_TRIANGLES: usize = 1024;
/// Culled triangles the sun's map draws anyway to join two kept runs into one draw: a
/// few more triangles cost less than a draw each (a superset of the kept casters, so the
/// depth map is the same).
const BRIDGE_INDICES: u32 = 4096 * 3;

/// One vertex of the cliff mesh.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct CliffVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// How open the surface is to the sky (0 deep in a groove, 1 open).
    pub occlusion: f32,
}

/// The generated cliff faces and rubble, triangles over `vertices`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CliffMesh {
    pub vertices: Vec<CliffVertex>,
    pub indices: Vec<u32>,
    /// Pack texture entries: albedo (sRGB, roughness in alpha) and normal.
    pub albedo: String,
    pub normal: Option<String>,
    /// Metres one texture tile covers.
    pub tile: f32,
    pub roughness: f32,
    /// How much of the biome's frost it takes (1: all).
    pub frost: f32,
}

/// One decal: a flat cut-out on the ground.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Decal {
    pub position: [f32; 3],
    pub yaw: f32,
    pub size: f32,
    /// Atlas cell.
    pub cell: f32,
    /// How much ice covers it (0–1).
    pub under: f32,
    /// How much rime lies on it (0–1).
    pub frost: f32,
    pub tint: [f32; 4],
}

/// Decals of one atlas.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecalBatch {
    /// Pack texture entry (RGBA cut-outs, `cells` × `cells`).
    pub atlas: String,
    pub cells: u32,
    pub decals: Vec<Decal>,
}

/// A world's dressing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DressingDesc {
    /// One mesh per face material.
    pub cliffs: Vec<CliffMesh>,
    pub decals: Vec<DecalBatch>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CliffUniform {
    /// 1 / tile, roughness, has normal map, frost share.
    params: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct DecalUniform {
    /// cells, 1 / cells, 0, 0.
    params: [f32; 4],
}

struct GpuCliff {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    group: wgpu::BindGroup,
    /// The index list's runs, in order.
    runs: Vec<CliffRun>,
    /// The index ranges the frame draws into the sun's map (adjacent runs merged).
    shadow: Vec<Range<u32>>,
}

/// Consecutive triangles of a cliff mesh: their bounds and indices.
#[derive(Debug, Clone, PartialEq)]
struct CliffRun {
    lo: Vec3,
    hi: Vec3,
    indices: Range<u32>,
}

/// The index list cut into runs of consecutive triangles (the generator emits them along
/// each contour and rock by rock): a run ends before a triangle that would spread it wider
/// than [`RUN_SPAN`] over the ground, or at [`RUN_TRIANGLES`].
fn runs(vertices: &[CliffVertex], indices: &[u32]) -> Vec<CliffRun> {
    let mut out = Vec::new();
    let empty = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let (mut lo, mut hi) = empty;
    let mut first = 0;
    for (t, triangle) in indices.chunks_exact(3).enumerate() {
        let (mut tlo, mut thi) = empty;
        for &i in triangle {
            let p = Vec3::from(vertices[i as usize].position);
            tlo = tlo.min(p);
            thi = thi.max(p);
        }
        let (grown_lo, grown_hi) = (lo.min(tlo), hi.max(thi));
        let wide = grown_hi.x - grown_lo.x > RUN_SPAN || grown_hi.z - grown_lo.z > RUN_SPAN;
        if t > first && (wide || t - first >= RUN_TRIANGLES) {
            out.push(CliffRun {
                lo,
                hi,
                indices: (first * 3) as u32..(t * 3) as u32,
            });
            first = t;
            (lo, hi) = (tlo, thi);
        } else {
            (lo, hi) = (grown_lo, grown_hi);
        }
    }
    let end = indices.len() / 3;
    if end > first {
        out.push(CliffRun {
            lo,
            hi,
            indices: (first * 3) as u32..(end * 3) as u32,
        });
    }
    out
}

/// Whether a box touches every plane's inner side (n·p + d ≥ 0): it is outside a plane
/// only when its corner farthest along the normal is.
fn box_visible(planes: &[Vec4], lo: Vec3, hi: Vec3) -> bool {
    planes.iter().all(|p| {
        let n = p.truncate();
        let corner = Vec3::select(n.cmpge(Vec3::ZERO), hi, lo);
        n.dot(corner) + p.w >= -0.01
    })
}

struct GpuDecals {
    count: u32,
    group: wgpu::BindGroup,
}

pub struct DressingGpu {
    cliffs: Vec<GpuCliff>,
    /// Cliff triangles the latest frame draws for the camera and into the sun's map.
    pub triangles: u64,
    pub shadow_triangles: u64,
    decals: Vec<GpuDecals>,
    cliff_pipeline: wgpu::RenderPipeline,
    cliff_shadow: wgpu::RenderPipeline,
    decal_pipeline: wgpu::RenderPipeline,
    pub bytes: u64,
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    features: GpuFeatures,
    pack: &Pack,
    name: &str,
) -> Result<(wgpu::TextureView, u64)> {
    let tex = pack.texture(name).with_context(|| name.to_owned())?;
    let bytes = tex.meta.mips.iter().map(|m| u64::from(m.len)).sum();
    let gpu = texture::upload(device, queue, features, &tex);
    Ok((gpu.create_view(&Default::default()), bytes))
}

fn flat_normal(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("flat normal"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[128, 128, 255, 255],
        )
        .create_view(&Default::default())
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    entry(
        binding,
        wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
    )
}

impl DressingGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        pack: &Pack,
        desc: &DressingDesc,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        let _span = tracing::info_span!("dressing_upload").entered();
        let mut bytes = 0;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("dressing"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let clamp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("decals"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let sampler_ty = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
        let cliff_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cliff"),
            entries: &[
                entry(0, uniform),
                texture_entry(1),
                texture_entry(2),
                entry(3, sampler_ty),
            ],
        });
        let decal_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("decals"),
            entries: &[
                entry(0, uniform),
                entry(
                    1,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                texture_entry(2),
                entry(3, sampler_ty),
            ],
        });

        let mut cliffs = Vec::new();
        for mesh in desc.cliffs.iter().filter(|m| !m.indices.is_empty()) {
            {
                let (albedo, b) = upload(device, queue, features, pack, &mesh.albedo)?;
                bytes += b;
                let normal = match &mesh.normal {
                    Some(name) => {
                        let (view, b) = upload(device, queue, features, pack, name)?;
                        bytes += b;
                        view
                    }
                    None => flat_normal(device, queue),
                };
                let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cliff"),
                    contents: bytemuck::bytes_of(&CliffUniform {
                        params: [
                            1.0 / mesh.tile.max(0.1),
                            mesh.roughness,
                            if mesh.normal.is_some() { 1.0 } else { 0.0 },
                            mesh.frost,
                        ],
                    }),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("cliff"),
                    layout: &cliff_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&albedo),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&normal),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                    ],
                });
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cliff"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cliff"),
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                bytes += vertices.size() + indices.size();
                cliffs.push(GpuCliff {
                    vertices,
                    indices,
                    count: mesh.indices.len() as u32,
                    group,
                    runs: runs(&mesh.vertices, &mesh.indices),
                    shadow: Vec::new(),
                });
            }
        }

        let mut decals = Vec::new();
        for batch in &desc.decals {
            if batch.decals.is_empty() {
                continue;
            }
            let (atlas, b) = upload(device, queue, features, pack, &batch.atlas)?;
            bytes += b;
            let cells = batch.cells.max(1) as f32;
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("decals"),
                contents: bytemuck::bytes_of(&DecalUniform {
                    params: [cells, 1.0 / cells, 0.0, 0.0],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let storage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("decals"),
                contents: bytemuck::cast_slice(&batch.decals),
                usage: wgpu::BufferUsages::STORAGE,
            });
            bytes += storage.size();
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("decals"),
                layout: &decal_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: storage.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&atlas),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&clamp),
                    },
                ],
            });
            decals.push(GpuDecals {
                count: batch.decals.len() as u32,
                group,
            });
        }

        let module = crate::shader::module(
            device,
            "dressing",
            crate::shader::LIT,
            include_str!("../shaders/dressing.wgsl"),
        );
        let cliff_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("cliff"),
                bind_group_layouts: &[Some(frame_layout), Some(&cliff_layout)],
                immediate_size: 0,
            });
        let decal_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("decals"),
                bind_group_layouts: &[Some(frame_layout), Some(&decal_layout)],
                immediate_size: 0,
            });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32];
        let buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CliffVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        })];
        let cliff_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cliff"),
            layout: Some(&cliff_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_cliff"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(targets::scene_depth(true)),
            multisample: targets::scene_multisample(false),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_cliff"),
                compilation_options: Default::default(),
                targets: &targets::forward_targets(None, true),
            }),
            multiview_mask: None,
            cache: None,
        });
        let cliff_shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cliff shadow"),
            layout: Some(&cliff_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_cliff_shadow"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(targets::shadow_depth()),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let decal_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("decals"),
            layout: Some(&decal_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_decal"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                bias: wgpu::DepthBiasState {
                    constant: 4,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
                ..targets::scene_depth(false)
            }),
            multisample: targets::scene_multisample(false),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_decal"),
                compilation_options: Default::default(),
                targets: &targets::forward_targets(Some(blend), false),
            }),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            cliffs,
            triangles: 0,
            shadow_triangles: 0,
            decals,
            cliff_pipeline,
            cliff_shadow,
            decal_pipeline,
            bytes,
        })
    }

    /// Chooses the cliff runs the frame draws into the sun's map: with `sun` (the map's
    /// frustum planes and its caster cull) those whose shadow can fall into the camera's
    /// view, none without.
    pub fn prepare(&mut self, sun: Option<(&[Vec4; 6], &CasterCull)>) {
        let _span = tracing::trace_span!("dressing_prepare").entered();
        self.triangles = self.cliffs.iter().map(|c| u64::from(c.count / 3)).sum();
        self.shadow_triangles = 0;
        for c in &mut self.cliffs {
            c.shadow.clear();
            let Some((planes, casters)) = sun else {
                continue;
            };
            for run in &c.runs {
                let centre = (run.lo + run.hi) * 0.5;
                if !box_visible(planes, run.lo, run.hi)
                    || !casters.casts_into_view(centre.extend((run.hi - centre).length()))
                {
                    continue;
                }
                match c.shadow.last_mut() {
                    Some(last) if run.indices.start - last.end <= BRIDGE_INDICES => {
                        last.end = run.indices.end
                    }
                    _ => c.shadow.push(run.indices.clone()),
                }
            }
            self.shadow_triangles += c
                .shadow
                .iter()
                .map(|r| u64::from(r.len() as u32 / 3))
                .sum::<u64>();
        }
    }

    /// The cliffs (opaque); returns the draw count.
    pub fn draw_cliffs<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame: &'a wgpu::BindGroup,
    ) -> u32 {
        if self.cliffs.is_empty() {
            return 0;
        }
        pass.set_pipeline(&self.cliff_pipeline);
        pass.set_bind_group(0, frame, &[]);
        for c in &self.cliffs {
            pass.set_bind_group(1, &c.group, &[]);
            pass.set_vertex_buffer(0, c.vertices.slice(..));
            pass.set_index_buffer(c.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..c.count, 0, 0..1);
        }
        self.cliffs.len() as u32
    }

    /// The cliffs whose shadow can reach the view ([`Self::prepare`]), into the shadow map.
    pub fn draw_shadow<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame: &'a wgpu::BindGroup,
    ) -> u32 {
        let mut draws = 0;
        for c in self.cliffs.iter().filter(|c| !c.shadow.is_empty()) {
            if draws == 0 {
                pass.set_pipeline(&self.cliff_shadow);
                pass.set_bind_group(0, frame, &[]);
            }
            pass.set_bind_group(1, &c.group, &[]);
            pass.set_vertex_buffer(0, c.vertices.slice(..));
            pass.set_index_buffer(c.indices.slice(..), wgpu::IndexFormat::Uint32);
            for run in &c.shadow {
                pass.draw_indexed(run.clone(), 0, 0..1);
                draws += 1;
            }
        }
        draws
    }

    /// The decals (blended over the ground, after the opaque ground); the draw count.
    pub fn draw_decals<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame: &'a wgpu::BindGroup,
    ) -> u32 {
        if self.decals.is_empty() {
            return 0;
        }
        pass.set_pipeline(&self.decal_pipeline);
        pass.set_bind_group(0, frame, &[]);
        for d in &self.decals {
            pass.set_bind_group(1, &d.group, &[]);
            pass.draw(0..6, 0..d.count);
        }
        self.decals.len() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_cover_the_mesh_in_order_and_stay_compact() {
        // A wall 100 m long (two triangles a metre), then a rock back at its start.
        let vertex = |x: f32, y: f32, z: f32| CliffVertex {
            position: [x, y, z],
            normal: [0.0, 0.0, 1.0],
            occlusion: 1.0,
        };
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for i in 0..=100 {
            vertices.push(vertex(i as f32, 0.0, 0.0));
            vertices.push(vertex(i as f32, 4.0, 0.0));
        }
        for i in 0..100u32 {
            let (a, b) = (2 * i, 2 * i + 2);
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
        let rock = vertices.len() as u32;
        vertices.extend([
            vertex(0.0, 0.0, 1.0),
            vertex(1.0, 0.0, 1.0),
            vertex(0.5, 1.0, 1.5),
        ]);
        indices.extend_from_slice(&[rock, rock + 1, rock + 2]);

        let runs = runs(&vertices, &indices);
        let mut next = 0;
        for run in &runs {
            assert_eq!(run.indices.start, next, "runs follow each other");
            next = run.indices.end;
            assert!(run.hi.x - run.lo.x <= RUN_SPAN && run.hi.z - run.lo.z <= RUN_SPAN);
            for &i in &indices[run.indices.start as usize..run.indices.end as usize] {
                let p = Vec3::from(vertices[i as usize].position);
                assert!(
                    p.cmpge(run.lo).all() && p.cmple(run.hi).all(),
                    "bounds hold {p}"
                );
            }
        }
        assert_eq!(next as usize, indices.len(), "every index in a run");
        assert!(runs.len() > 100 / RUN_SPAN as usize);
    }
}
