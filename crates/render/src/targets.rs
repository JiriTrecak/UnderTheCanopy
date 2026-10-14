//! Formats and the frame's render targets.

/// HDR scene colour.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Scene depth: reverse-Z (1 near, 0 far), compared with `Greater`.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The sun's map: standard depth (0 at the sun side).
pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// MSAA samples of the scene (D24, Q2).
pub const SAMPLES: u32 = 4;

/// Depth state of scene geometry.
pub fn scene_depth(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(wgpu::CompareFunction::Greater),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// Depth state of shadow casters, with a slope-scaled bias against acne.
pub fn shadow_depth() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: SHADOW_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState {
            constant: 2,
            slope_scale: 2.0,
            clamp: 0.0,
        },
    }
}

pub fn scene_multisample(alpha_to_coverage: bool) -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: SAMPLES,
        mask: !0,
        alpha_to_coverage_enabled: alpha_to_coverage,
    }
}

/// The scene's colour and depth for one output size.
pub struct Targets {
    pub size: [u32; 2],
    pub color: wgpu::TextureView,
    pub resolved: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    bytes: u64,
}

impl Targets {
    pub fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let extent = wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
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
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let pixels = u64::from(extent.width) * u64::from(extent.height);
        Self {
            size,
            color: make("scene colour", HDR_FORMAT, SAMPLES, attachment),
            resolved: make(
                "scene resolved",
                HDR_FORMAT,
                1,
                attachment | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            depth: make("scene depth", DEPTH_FORMAT, SAMPLES, attachment),
            bytes: pixels * (8 * u64::from(SAMPLES) + 8 + 4 * u64::from(SAMPLES)),
        }
    }

    /// Memory of the targets (an estimate: tile GPUs may keep MSAA in tile memory).
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
