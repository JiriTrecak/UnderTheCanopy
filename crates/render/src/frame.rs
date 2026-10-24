//! The per-frame uniform (group 0 of every pass) and its bind groups.

use crate::{camera::CameraView, environment::Lighting};
use bytemuck::{Pod, Zeroable};
use glam::Mat4;

/// How far receivers step along their normal before reading the sun's map, world units.
pub const NORMAL_OFFSET: f32 = 0.025;

/// Mirrors `Frame` in `shaders/lib/frame.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FrameUniform {
    pub view_proj: [[f32; 4]; 4],
    pub shadow_matrix: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub viewport: [f32; 4],
    pub sun_direction: [f32; 4],
    pub sun: [f32; 4],
    pub ambient: [f32; 4],
    pub sky: [f32; 4],
    pub ground: [f32; 4],
    pub probe_up: [f32; 4],
    pub probe_side: [f32; 4],
    pub probe_down: [f32; 4],
    pub environment: [[f32; 4]; 9],
    pub canopy: [f32; 4],
    pub canopy_offset: [f32; 4],
    pub shadow: [f32; 4],
    /// rgb: background; w: the camera's view fade (0 at RTS pitches, 1 near the horizon,
    /// [`crate::surroundings::view_fade`]).
    pub background: [f32; 4],
    pub haze: [f32; 4],
    pub haze_range: [f32; 4],
    pub counts: [u32; 4],
    /// x: 1 / fog size, y: fog on (0/1), z: fog size.
    pub fog: [f32; 4],
    /// x: scenery cutaway on (0/1) ([`crate::cutaway`]); y: frame shade on (0/1), z: world x
    /// and z of the shade's vertex 0, w: 1 / shade vertices per side ([`crate::shade`]).
    pub cutaway: [f32; 4],
    /// rgb: the night's rim on live models; w: their fill ([`Lighting::unit_rim`]).
    pub units: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<FrameUniform>() == 592);

/// Most point lights the frame buffer holds.
pub const MAX_POINT_LIGHTS: usize = 64;

/// A point light: position, range, linear colour × intensity.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct PointLight {
    pub position: [f32; 3],
    pub range: f32,
    pub color: [f32; 3],
    pub pad: f32,
}

/// Most cells of the light grid on a side.
const LIGHT_GRID: f32 = 64.0;
/// Bytes of the lights buffer: the lights, the grid's header and its largest cell list.
pub const LIGHTS_BYTES: usize = MAX_POINT_LIGHTS * std::mem::size_of::<PointLight>()
    + 32
    + (LIGHT_GRID as usize) * (LIGHT_GRID as usize) * 8;

/// The light grid (`Lights` in frame.wgsl): a grid over the ground covering every
/// light's reach, each cell listing, as a 64-bit mask, the lights whose range reaches
/// it. A surface sums only its cell's lights, in their order, so its light is the same
/// as summing them all (those left out are out of range and add nothing).
pub fn light_buffer_contents(lights: &[PointLight]) -> Vec<u8> {
    let lights = &lights[..lights.len().min(MAX_POINT_LIGHTS)];
    let mut list = [PointLight::zeroed(); MAX_POINT_LIGHTS];
    list[..lights.len()].copy_from_slice(lights);
    let mut bytes = bytemuck::cast_slice::<_, u8>(&list).to_vec();
    // Reach in xz, a little generous so rounding at a cell's edge never drops a light.
    let reach = |l: &PointLight| l.range.max(0.0) * 1.001 + 0.05;
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for l in lights {
        let r = reach(l);
        lo = [lo[0].min(l.position[0] - r), lo[1].min(l.position[2] - r)];
        hi = [hi[0].max(l.position[0] + r), hi[1].max(l.position[2] + r)];
    }
    if lights.is_empty() || !(lo[0].is_finite() && hi[0].is_finite()) {
        bytes.extend_from_slice(bytemuck::cast_slice(&[0.0f32; 4]));
        bytes.extend_from_slice(bytemuck::cast_slice(&[0u32; 4]));
        return bytes;
    }
    let cell = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / LIGHT_GRID).max(0.5);
    let size = [
        (((hi[0] - lo[0]) / cell).ceil() as usize).clamp(1, LIGHT_GRID as usize),
        (((hi[1] - lo[1]) / cell).ceil() as usize).clamp(1, LIGHT_GRID as usize),
    ];
    let mut cells = vec![[0u32; 2]; size[0] * size[1]];
    for (i, l) in lights.iter().enumerate() {
        let r = reach(l);
        let (x, z) = (l.position[0], l.position[2]);
        let first =
            |c: f32, o: f32, n: usize| (((c - r - o) / cell).floor().max(0.0) as usize).min(n - 1);
        let last =
            |c: f32, o: f32, n: usize| (((c + r - o) / cell).floor().max(0.0) as usize).min(n - 1);
        for cz in first(z, lo[1], size[1])..=last(z, lo[1], size[1]) {
            for cx in first(x, lo[0], size[0])..=last(x, lo[0], size[0]) {
                // The cell square's nearest point to the light.
                let nx = x.clamp(lo[0] + cx as f32 * cell, lo[0] + (cx + 1) as f32 * cell);
                let nz = z.clamp(lo[1] + cz as f32 * cell, lo[1] + (cz + 1) as f32 * cell);
                if (nx - x).hypot(nz - z) <= r {
                    cells[cz * size[0] + cx][i / 32] |= 1 << (i % 32);
                }
            }
        }
    }
    bytes.extend_from_slice(bytemuck::cast_slice(&[lo[0], lo[1], 1.0 / cell, 0.0]));
    bytes.extend_from_slice(bytemuck::cast_slice(&[
        size[0] as u32,
        size[1] as u32,
        0,
        0,
    ]));
    bytes.extend_from_slice(bytemuck::cast_slice(&cells));
    bytes
}

fn v4(v: glam::Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

/// What a frame needs besides the world.
pub struct FrameParams<'a> {
    pub camera: &'a CameraView,
    pub lighting: &'a Lighting,
    pub shadow_matrix: Mat4,
    pub shadow_texel: f32,
    pub shadows: bool,
    pub seconds: f32,
    pub viewport: [u32; 2],
    pub haze: glam::Vec3,
    pub haze_range: [f32; 2],
    pub point_lights: u32,
    /// Cells per side of the viewer's fog of war; `None` without fog.
    pub fog: Option<u32>,
    /// Some unit covers the scenery cutaway's mask.
    pub cutaway: bool,
    /// The world's frame shade: origin of vertex 0 and vertices per side; `None` without.
    pub shade: Option<(f32, u32)>,
}

impl FrameUniform {
    pub fn new(p: &FrameParams<'_>) -> Self {
        let l = p.lighting;
        let (offset, transmission) = crate::environment::canopy_motion(&l.canopy, p.seconds);
        let mut environment = [[0.0; 4]; 9];
        for (out, c) in environment.iter_mut().zip(l.environment) {
            *out = v4(c, 0.0);
        }
        let (w, h) = (p.viewport[0] as f32, p.viewport[1] as f32);
        Self {
            view_proj: p.camera.view_projection.to_cols_array_2d(),
            shadow_matrix: p.shadow_matrix.to_cols_array_2d(),
            eye: v4(p.camera.eye, p.seconds),
            viewport: [w, h, 1.0 / w, 1.0 / h],
            sun_direction: v4(l.sun_direction, if p.shadows { 1.0 } else { 0.0 }),
            sun: v4(l.sun * transmission, 0.0),
            ambient: v4(l.ambient, 0.0),
            sky: v4(l.sky, 0.0),
            ground: v4(l.ground, 0.0),
            probe_up: v4(l.ground_probe[0], 0.0),
            probe_side: v4(l.ground_probe[1], 0.0),
            probe_down: v4(l.ground_probe[2], 0.0),
            environment,
            canopy: [
                1.0 / l.canopy.scale,
                l.canopy.height,
                l.canopy.strength,
                if l.canopy.strength > 0.0 { 1.0 } else { 0.0 },
            ],
            canopy_offset: [offset[0], offset[1], l.canopy.sky_shade, 0.0],
            shadow: [
                p.shadow_texel,
                l.shadow_softness,
                // Normal offset of receivers, and a constant depth bias.
                NORMAL_OFFSET,
                0.0004,
            ],
            background: v4(l.background, crate::surroundings::view_fade(p.camera)),
            haze: v4(p.haze, 0.0),
            haze_range: [p.haze_range[0], p.haze_range[1], l.frost, l.shadow_ceiling],
            counts: [p.point_lights, 0, 0, 0],
            fog: p.fog.map_or([0.0; 4], |size| {
                [1.0 / size.max(1) as f32, 1.0, size as f32, 0.0]
            }),
            cutaway: match p.shade {
                Some((origin, vertices)) => [
                    if p.cutaway { 1.0 } else { 0.0 },
                    1.0,
                    origin,
                    1.0 / vertices.max(1) as f32,
                ],
                None => [if p.cutaway { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            },
            units: v4(l.unit_rim, l.unit_fill),
        }
    }

    /// The same frame seen from the sun, for the shadow pass.
    pub fn for_shadow(&self, shadow_view_proj: Mat4) -> Self {
        Self {
            view_proj: shadow_view_proj.to_cols_array_2d(),
            ..*self
        }
    }
}

/// Group 0 layout shared by every pass.
pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let all =
        wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
    let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("frame"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: all,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<FrameUniform>() as u64
                    ),
                },
                count: None,
            },
            texture(1, wgpu::TextureSampleType::Depth),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            texture(3, wgpu::TextureSampleType::Float { filterable: true }),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            texture(6, wgpu::TextureSampleType::Float { filterable: true }),
            wgpu::BindGroupLayoutEntry {
                binding: 7,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            texture(8, wgpu::TextureSampleType::Float { filterable: false }),
            texture(9, wgpu::TextureSampleType::Float { filterable: true }),
        ],
    })
}
