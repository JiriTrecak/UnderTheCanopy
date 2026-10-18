//! Ambient motes (`shaders/motes.wgsl`): dust glinting where light falls through the
//! canopy, and leaves drifting down, from a biome's `motes`. Both are generated on the GPU
//! from the instance index in fields that follow the camera and wrap around it, so they
//! need no buffers beyond one uniform: the dust only for close cameras (it is too small to
//! see from the RTS view), the leaves for every view. Render only.

use crate::{
    camera::CameraView, device::GpuFeatures, environment::Motes, surroundings::view_fade, targets,
    texture,
};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use canopy_assets::Pack;
use glam::Vec3;
use wgpu::util::DeviceExt;

/// The dust field ahead of a close camera: its centre's distance and half size (metres),
/// so the dust floats from a few metres out to just before depth of field softens the
/// view (post.wgsl), where blurred bright points would read as discs.
const DUST_AHEAD: f32 = 18.0;
const DUST_FIELD: f32 = 14.0;
/// Height of the leaves' field above the camera's target (metres).
const LEAF_HEIGHT: f32 = 50.0;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct MotesUniform {
    dust: [f32; 4],
    leaves: [f32; 4],
    counts: [f32; 4],
    params: [f32; 4],
    color: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
}

pub struct MotesGpu {
    config: Motes,
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
    dust: wgpu::RenderPipeline,
    leaves: wgpu::RenderPipeline,
    /// This frame's close-view share (no dust at RTS pitches).
    close: f32,
    pub bytes: u64,
}

impl MotesGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        pack: &Pack,
        config: &Motes,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        let _span = tracing::info_span!("motes_upload").entered();
        let texture = match (&config.leaf_texture, config.leaves) {
            (Some(name), n) if n > 0 => {
                let tex = pack.texture(name).with_context(|| name.clone())?;
                texture::upload(device, queue, features, &tex)
            }
            _ => device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("motes leaf"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[255, 255, 255, 0],
            ),
        };
        let bytes = u64::from(texture.width()) * u64::from(texture.height());
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("motes"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("motes"),
            contents: bytemuck::bytes_of(&MotesUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("motes"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("motes"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let module = crate::shader::module(
            device,
            "motes",
            crate::shader::LIT,
            include_str!("../shaders/motes.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("motes"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |vs: &str, fs: &str, blend: Option<wgpu::BlendState>, opaque: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("motes"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(targets::scene_depth(opaque)),
                multisample: targets::scene_multisample(false),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &targets::forward_targets(blend, opaque),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        Ok(Self {
            config: config.clone(),
            buffer,
            group,
            dust: pipeline("vs_dust", "fs_dust", Some(additive), false),
            leaves: pipeline("vs_leaf", "fs_leaf", None, true),
            close: 0.0,
            bytes,
        })
    }

    /// This frame's fields around `camera`; `light` is the linear colour of light through
    /// the canopy and `elevation` the shafts' own elevation in degrees (`None`: the sun's).
    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        camera: &CameraView,
        seconds: f32,
        light: Vec3,
        elevation: Option<f32>,
    ) {
        let close = view_fade(camera);
        self.close = close;
        let forward = (camera.target - camera.eye).normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward).normalize_or(Vec3::Y);
        let dust = camera.eye + forward * DUST_AHEAD;
        let field = 110.0 + (60.0 - 110.0) * close;
        let c = &self.config;
        let uniform = MotesUniform {
            dust: [dust.x, dust.y, dust.z, DUST_FIELD],
            leaves: [
                camera.target.x,
                camera.target.y - 4.0,
                camera.target.z,
                field,
            ],
            counts: [c.dust as f32, c.leaves as f32, c.leaf_size, close],
            params: [
                elevation.map_or(0.0, |e| e.to_radians().sin()),
                LEAF_HEIGHT,
                c.dust_brightness,
                seconds,
            ],
            color: [light.x, light.y, light.z, 0.0],
            right: [right.x, right.y, right.z, 0.0],
            up: [up.x, up.y, up.z, 0.0],
        };
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// The falling leaves (opaque, alpha-tested); returns the draw count.
    pub fn draw_leaves<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame: &'a wgpu::BindGroup,
    ) -> u32 {
        if self.config.leaves == 0 || self.config.leaf_texture.is_none() {
            return 0;
        }
        pass.set_pipeline(&self.leaves);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.group, &[]);
        pass.draw(0..6, 0..self.config.leaves);
        1
    }

    /// The dust in the light (additive, close cameras only); returns the draw count.
    pub fn draw_dust<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        frame: &'a wgpu::BindGroup,
    ) -> u32 {
        if self.config.dust == 0 || self.close < 0.01 {
            return 0;
        }
        pass.set_pipeline(&self.dust);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(1, &self.group, &[]);
        pass.draw(0..6, 0..self.config.dust);
        1
    }
}
