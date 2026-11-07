//! A map's blockout drawn as blocks (D80): flat-coloured faces with crisp outlines around
//! every block face, so levels, ramps and cuts read at a glance. The caller (the editor)
//! builds the mesh from the blocks; the renderer only draws it, in place of the terrain.
//! Render only.

use crate::targets;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// One vertex of the blockout mesh.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BlockVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Linear colour.
    pub color: [f32; 3],
    /// The face's own coordinates, 0 to 1 across it (outlines at 0 and 1), and the
    /// distance to a diagonal edge in the same units (an outline at 0; large for none).
    pub edge: [f32; 3],
}

/// A blockout mesh: triangles over `vertices`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockoutMesh {
    pub vertices: Vec<BlockVertex>,
    pub indices: Vec<u32>,
}

/// The mesh's vertex and index buffers (written again in place by
/// [`BlockoutGpu::replace`]).
fn buffers(device: &wgpu::Device, mesh: &BlockoutMesh) -> (wgpu::Buffer, wgpu::Buffer) {
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("blockout"),
        contents: bytemuck::cast_slice(&mesh.vertices),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("blockout"),
        contents: bytemuck::cast_slice(&mesh.indices),
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
    });
    (vertices, indices)
}

pub struct BlockoutGpu {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    pub bytes: u64,
}

impl BlockoutGpu {
    pub fn new(
        device: &wgpu::Device,
        mesh: &BlockoutMesh,
        frame_layout: &wgpu::BindGroupLayout,
        kept: &crate::kept::Kept,
    ) -> Option<Self> {
        if mesh.indices.is_empty() {
            return None;
        }
        let _span = tracing::info_span!("blockout_upload").entered();
        let (vertices, indices) = buffers(device, mesh);
        let pipeline = kept.get("blockout", || {
            let module = crate::shader::module(
                device,
                "blockout",
                crate::shader::LIT,
                include_str!("../shaders/blockout.wgsl"),
            );
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("blockout"),
                bind_group_layouts: &[Some(frame_layout)],
                immediate_size: 0,
            });
            let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blockout"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<BlockVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(targets::scene_depth(true)),
                multisample: targets::scene_multisample(false),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &targets::forward_targets(None, true),
                }),
                multiview_mask: None,
                cache: None,
            })
        });
        Some(Self {
            pipeline,
            bytes: vertices.size() + indices.size(),
            vertices,
            indices,
            count: mesh.indices.len() as u32,
        })
    }

    /// Draws `mesh` instead, keeping the pipeline (and the buffers, when it fits them): a
    /// block stroke in the editor uploads its mesh again every rebuild. `false` for an
    /// empty mesh, which [`BlockoutGpu::new`] does not draw either.
    pub fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mesh: &BlockoutMesh,
    ) -> bool {
        if mesh.indices.is_empty() {
            return false;
        }
        let _span = tracing::info_span!("blockout_upload").entered();
        let vertices: &[u8] = bytemuck::cast_slice(&mesh.vertices);
        let indices: &[u8] = bytemuck::cast_slice(&mesh.indices);
        if vertices.len() as u64 <= self.vertices.size()
            && indices.len() as u64 <= self.indices.size()
        {
            queue.write_buffer(&self.vertices, 0, vertices);
            queue.write_buffer(&self.indices, 0, indices);
        } else {
            (self.vertices, self.indices) = buffers(device, mesh);
            self.bytes = self.vertices.size() + self.indices.size();
        }
        self.count = mesh.indices.len() as u32;
        true
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) -> u32 {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, 0..1);
        1
    }
}
