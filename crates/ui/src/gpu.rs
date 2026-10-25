//! The reference wgpu pass (feature `wgpu`): draws a [`DrawList`] in one render pass,
//! one pipeline, one instanced draw per batch.
//!
//! The game's renderer may embed this or reimplement it; it is also what the widget
//! gallery uses offscreen. WebGPU baseline only (D2): no optional features or limits.
//!
//! Textures registered with [`UiRenderer::set_texture`] must be views that return the
//! stored sRGB-encoded values (a non-sRGB view, e.g. `Bc7RgbaUnorm` of a
//! `Bc7RgbaUnormSrgb` texture created with that view format), because the pass blends in
//! sRGB space like a browser.

use crate::draw::{DrawList, Quad, TextureId};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub struct UiRenderer {
    pipeline: wgpu::RenderPipeline,
    globals_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    sampler: wgpu::Sampler,
    atlas: Option<(wgpu::Texture, u32)>,
    globals_group: Option<wgpu::BindGroup>,
    instances: Option<wgpu::Buffer>,
    textures: HashMap<TextureId, wgpu::BindGroup>,
    white: wgpu::BindGroup,
    srgb_target: bool,
    /// The globals and the quads last written.
    written_globals: Option<[u32; 4]>,
    written_quads: Vec<crate::Quad>,
}

impl UiRenderer {
    /// A pass drawing into targets of `format`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui globals"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2),
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui image"),
            entries: &[texture_entry(0)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&globals_layout), Some(&image_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x4,
            1 => Float32x4,
            2 => Unorm8x4,
            3 => Unorm8x4,
            4 => Float32x4,
            5 => Float32,
            6 => Uint32,
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Quad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ui"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white_texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("ui white"),
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
            &[255, 255, 255, 255],
        );
        let white = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui white"),
            layout: &image_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &white_texture.create_view(&Default::default()),
                ),
            }],
        });
        Self {
            pipeline,
            globals_layout,
            image_layout,
            globals,
            sampler,
            atlas: None,
            globals_group: None,
            written_globals: None,
            written_quads: Vec::new(),
            instances: None,
            textures: HashMap::new(),
            white,
            srgb_target: format.is_srgb(),
        }
    }

    /// Binds a texture view to an id used by [`crate::ImageDesc`].
    pub fn set_texture(&mut self, device: &wgpu::Device, id: TextureId, view: &wgpu::TextureView) {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui image"),
            layout: &self.image_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            }],
        });
        self.textures.insert(id, group);
    }

    pub fn remove_texture(&mut self, id: TextureId) {
        self.textures.remove(&id);
    }

    /// Uploads a frame: atlas changes, instances and globals.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, list: &DrawList) {
        let size = list.atlas.size.max(1);
        let recreate = list.atlas.reset || self.atlas.as_ref().is_none_or(|(_, s)| *s != size);
        if recreate {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("ui glyph atlas"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.globals_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ui globals"),
                layout: &self.globals_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            }));
            self.atlas = Some((texture, size));
        }
        if let Some((texture, _)) = &self.atlas {
            for u in &list.atlas.updates {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: u.x,
                            y: u.y,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &u.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(u.width),
                        rows_per_image: Some(u.height),
                    },
                    wgpu::Extent3d {
                        width: u.width,
                        height: u.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        let globals: [u32; 4] = [
            (list.viewport[0] as f32).to_bits(),
            (list.viewport[1] as f32).to_bits(),
            self.srgb_target as u32,
            0,
        ];
        if self.written_globals != Some(globals) {
            queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&globals));
            self.written_globals = Some(globals);
        }
        let bytes: &[u8] = bytemuck::cast_slice(&list.quads);
        let needed = bytes.len().max(64) as u64;
        if self.instances.as_ref().is_none_or(|b| b.size() < needed) {
            self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ui quads"),
                size: needed.next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.written_quads.clear();
        }
        // A frame with the same quads as the buffer holds (a still screen) uploads none.
        if !bytes.is_empty()
            && bytemuck::cast_slice::<_, u8>(&self.written_quads) != bytes
            && let Some(buffer) = &self.instances
        {
            queue.write_buffer(buffer, 0, bytes);
            self.written_quads.clone_from(&list.quads);
        }
    }

    /// Records the frame's draws. `target` is the pass's target size in pixels.
    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>, list: &DrawList) {
        let (Some(globals), Some(instances)) = (&self.globals_group, &self.instances) else {
            return;
        };
        if list.quads.is_empty() {
            return;
        }
        let [vw, vh] = list.viewport;
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, globals, &[]);
        pass.set_vertex_buffer(0, instances.slice(..));
        // Scissor and image change only where they differ from the batch before.
        let mut scissor = None;
        let mut bound: Option<&wgpu::BindGroup> = None;
        for batch in &list.batches {
            let (x, y, w, h) = match batch.scissor {
                Some([x, y, w, h]) => {
                    let x = x.min(vw);
                    let y = y.min(vh);
                    (x, y, w.min(vw - x), h.min(vh - y))
                }
                None => (0, 0, vw, vh),
            };
            if w == 0 || h == 0 || batch.quads.is_empty() {
                continue;
            }
            if scissor != Some((x, y, w, h)) {
                pass.set_scissor_rect(x, y, w, h);
                scissor = Some((x, y, w, h));
            }
            let group = batch
                .texture
                .and_then(|t| self.textures.get(&t))
                .unwrap_or(&self.white);
            if !bound.is_some_and(|b| std::ptr::eq(b, group)) {
                pass.set_bind_group(1, group, &[]);
                bound = Some(group);
            }
            pass.draw(0..6, batch.quads.clone());
        }
    }
}

/// Uploads straight-alpha RGBA8 pixels (sRGB-encoded) as a premultiplied, mipmapped
/// `Rgba8Unorm` texture for the pass. For tools and the gallery; the game binds
/// textures from the asset pack.
pub fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> wgpu::TextureView {
    let mut level: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            [
                ((p[0] as u32 * a + 127) / 255) as u8,
                ((p[1] as u32 * a + 127) / 255) as u8,
                ((p[2] as u32 * a + 127) / 255) as u8,
                p[3],
            ]
        })
        .collect();
    let mips = 32 - width.max(height).max(1).leading_zeros();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (mut w, mut h) = (width, height);
    for mip in 0..mips {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        // Box-filter the next level.
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += level[((sy * w + sx) * 4 + c) as usize] as u32;
                    }
                    next[((y * nw + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        level = next;
        w = nw;
        h = nh;
    }
    texture.create_view(&Default::default())
}

/// Draws a draw list into a new offscreen `Rgba8Unorm` target cleared to `clear`
/// (sRGB) and reads it back as tightly packed RGBA8 rows.
pub fn render_to_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut UiRenderer,
    list: &DrawList,
    clear: [u8; 4],
) -> Vec<u8> {
    let [width, height] = list.viewport;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    renderer.prepare(device, queue, list);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ui offscreen"),
    });
    {
        let c = |v: u8| v as f64 / 255.0;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: c(clear[0]),
                        g: c(clear[1]),
                        b: c(clear[2]),
                        a: c(clear[3]),
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        renderer.render(&mut pass, list);
    }
    let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui readback"),
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("mapped readback");
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let start = (y * row) as usize;
        out.extend_from_slice(&data[start..start + (width * 4) as usize]);
    }
    out
}

/// A headless device with WebGPU's default limits, or `None` without a GPU adapter.
pub fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("ui headless"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    }))
    .ok()
}
