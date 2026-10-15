//! The selection portrait as a render-to-texture output (renderer §6, D24 Q14): the
//! selected unit's live model, animated, rendered on its own into a small multisampled HDR
//! target with the portrait's lights, then resolved to an sRGB image with premultiplied
//! alpha that the HUD shows over its panel (old `SelectionPortrait`: an orthographic view
//! fitted to the model's box from the front and a little above, the model turned −0.32
//! rad, warm key light from the upper left, the world's tone mapping).
//!
//! The model is a live model like any other ([`crate::actors::ActorsGpu::set_portrait`]):
//! the same buffers, skinning and materials.

use crate::{
    camera::CameraView,
    environment::Lighting,
    frame::{FrameParams, FrameUniform},
    targets,
};
use glam::{Mat4, Vec3};

/// The old portrait's key light (`#ffe2b3` × 3.2 from (−4, 7, 6)) and ambient
/// (`#ffefd5` × 1.2).
const KEY: (u32, f32) = (0xffe2b3, 3.2);
const AMBIENT: (u32, f32) = (0xffefd5, 1.2);

fn linear(hex: u32) -> Vec3 {
    let c = |s: u32| {
        let v = ((hex >> s) & 255) as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    Vec3::new(c(16), c(8), c(0))
}

/// The portrait's lighting: the world's, with the portrait's own key light and ambient,
/// no hemisphere, canopy or shadows.
pub fn lighting(world: &Lighting) -> Lighting {
    let mut l = world.clone();
    l.sun_direction = Vec3::new(-4.0, 7.0, 6.0).normalize();
    l.sun_color = linear(KEY.0);
    l.sun_intensity = KEY.1;
    l.sun = l.sun_color * l.sun_intensity;
    l.ambient = linear(AMBIENT.0) * AMBIENT.1;
    l.sky = Vec3::ZERO;
    l.ground = Vec3::ZERO;
    l.canopy.strength = 0.0;
    l.canopy.cloud_shadow = 0.0;
    l
}

/// An orthographic reverse-Z camera looking along `forward` at `centre`, showing
/// `half_width` × `half_height` either side.
pub fn orthographic(
    eye: Vec3,
    centre: Vec3,
    up: Vec3,
    half_width: f32,
    half_height: f32,
    far: f32,
) -> CameraView {
    let view = Mat4::look_at_rh(eye, centre, up);
    // Near and far swapped: the near plane maps to depth 1.
    let projection = Mat4::orthographic_rh(
        -half_width,
        half_width,
        -half_height,
        half_height,
        far,
        0.01,
    );
    CameraView {
        view,
        projection,
        view_projection: projection * view,
        eye,
        target: centre,
        fov_y: 0.5,
        aspect: half_width / half_height.max(1e-6),
        near: 0.01,
        far,
    }
}

/// The old portrait framing of a model's world-space box: from the front and a little
/// above (more for buildings), the box filling the view with 8 % to spare.
pub fn frame(min: Vec3, max: Vec3, building: bool, aspect: f32) -> CameraView {
    let centre = (min + max) * 0.5;
    let size = (max - min).length();
    let direction = Vec3::new(0.0, if building { 1.05 } else { 0.28 }, 1.8).normalize();
    let eye = centre + direction * (size * 2.0).max(3.0);
    let view = Mat4::look_at_rh(eye, centre, Vec3::Y);
    let (mut x, mut y) = (0.0f32, 0.0f32);
    for a in [min.x, max.x] {
        for b in [min.y, max.y] {
            for c in [min.z, max.z] {
                let v = view.transform_point3(Vec3::new(a, b, c));
                x = x.max(v.x.abs());
                y = y.max(v.y.abs());
            }
        }
    }
    let half = y.max(x / aspect) * 1.08;
    orthographic(
        eye,
        centre,
        Vec3::Y,
        half * aspect,
        half,
        (size * 6.0).max(100.0),
    )
}

pub struct PortraitGpu {
    pub size: u32,
    color: wgpu::TextureView,
    resolved: wgpu::TextureView,
    linear: wgpu::TextureView,
    linear_resolved: wgpu::TextureView,
    depth: wgpu::TextureView,
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub frame_buffer: wgpu::Buffer,
    blit: wgpu::RenderPipeline,
    blit_group: wgpu::BindGroup,
}

impl PortraitGpu {
    pub fn new(device: &wgpu::Device, size: u32) -> Self {
        let extent = wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        };
        let make = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let sampled = attachment | wgpu::TextureUsages::TEXTURE_BINDING;
        let color = make(
            "portrait colour",
            targets::HDR_FORMAT,
            targets::SAMPLES,
            attachment,
        );
        let resolved = make("portrait resolved", targets::HDR_FORMAT, 1, sampled);
        let linear = make(
            "portrait depth",
            targets::LINEAR_DEPTH_FORMAT,
            targets::SAMPLES,
            attachment,
        );
        let linear_resolved = make(
            "portrait depth resolved",
            targets::LINEAR_DEPTH_FORMAT,
            1,
            attachment,
        );
        let depth = make(
            "portrait z",
            targets::DEPTH_FORMAT,
            targets::SAMPLES,
            attachment,
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("portrait"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: targets::UI_IMAGE_FORMAT,
            usage: sampled | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let frame_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("portrait frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("portrait blit"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("portrait blit"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&resolved),
            }],
        });
        let module = crate::shader::module(
            device,
            "portrait",
            &[],
            include_str!("../shaders/portrait.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("portrait blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("portrait blit"),
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
                    format: targets::UI_IMAGE_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            size,
            color,
            resolved,
            linear,
            linear_resolved,
            depth,
            texture,
            view,
            frame_buffer,
            blit,
            blit_group,
        }
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// The portrait's frame uniform for `camera` under the world's `lighting`.
    pub fn prepare(&self, queue: &wgpu::Queue, camera: &CameraView, world: &Lighting) {
        let light = lighting(world);
        let uniform = FrameUniform::new(&FrameParams {
            camera,
            lighting: &light,
            shadow_matrix: Mat4::IDENTITY,
            shadow_texel: 1.0,
            shadows: false,
            seconds: 0.0,
            viewport: [self.size, self.size],
            haze: Vec3::ZERO,
            haze_range: [1e6, 1e6 + 1.0],
            point_lights: 0,
            fog: None,
            cutaway: false,
        });
        queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// Renders the portrait: `draw` records the model into the forward-like pass.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
        draw: impl FnOnce(&mut wgpu::RenderPass<'_>) -> u32,
    ) -> u32 {
        let mut draws = {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("portrait"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &self.color,
                        depth_slice: None,
                        resolve_target: Some(&self.resolved),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Discard,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &self.linear,
                        depth_slice: None,
                        resolve_target: Some(&self.linear_resolved),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Discard,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timestamps,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw(&mut pass)
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("portrait resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &self.blit_group, &[]);
        pass.draw(0..3, 0..1);
        draws += 1;
        draws
    }
}
