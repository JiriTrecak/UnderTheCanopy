//! Formats and the frame's render targets.

/// HDR scene colour.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Scene depth: reverse-Z (1 near, 0 far), compared with `Greater`.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The sun's map: standard depth (0 at the sun side).
pub const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// MSAA samples of the scene (D24, Q2).
pub const SAMPLES: u32 = 4;
/// Linear view depth of opaque surfaces, written by every opaque forward pipeline as its
/// second colour target and resolved with the colour: `rg` = the depth rounded to half
/// precision and the remainder, so interior pixels keep full precision through the
/// (averaging) resolve. Read by water-free post passes (contact shadows, haze).
pub const LINEAR_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// Images the renderer makes for the HUD (minimap, portrait): sRGB with premultiplied
/// alpha, for `canopy-ui` image nodes (`ImageDesc { srgb_view: true, premultiplied: true }`).
pub const UI_IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Linear depth of the background (no surface), metres.
pub const FAR_DEPTH: f64 = 60000.0;

/// The forward pass's colour targets for a pipeline: HDR colour with `blend`, and the
/// linear depth, written only by opaque surfaces (`depth`). Every pipeline drawn in the
/// forward pass uses this list.
pub fn forward_targets(
    blend: Option<wgpu::BlendState>,
    depth: bool,
) -> [Option<wgpu::ColorTargetState>; 2] {
    [
        Some(wgpu::ColorTargetState {
            format: HDR_FORMAT,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        }),
        Some(wgpu::ColorTargetState {
            format: LINEAR_DEPTH_FORMAT,
            blend: None,
            write_mask: if depth {
                wgpu::ColorWrites::ALL
            } else {
                wgpu::ColorWrites::empty()
            },
        }),
    ]
}

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

/// The scene's colour and depth at the internal resolution (output × render scale).
pub struct Targets {
    pub size: [u32; 2],
    pub color: wgpu::TextureView,
    pub resolved: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    /// Multisampled linear depth (transient) and its resolve.
    pub linear: wgpu::TextureView,
    pub linear_resolved: wgpu::TextureView,
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
        // Multisampled colour and depth live only inside the forward pass: transient
        // attachments stay in tile memory on Apple GPUs (memoryless).
        let transient = attachment | wgpu::TextureUsages::TRANSIENT_ATTACHMENT;
        let pixels = u64::from(extent.width) * u64::from(extent.height);
        Self {
            size,
            color: make("scene colour", HDR_FORMAT, SAMPLES, transient),
            resolved: make(
                "scene resolved",
                HDR_FORMAT,
                1,
                attachment | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            depth: make("scene depth", DEPTH_FORMAT, SAMPLES, transient),
            linear: make("linear depth", LINEAR_DEPTH_FORMAT, SAMPLES, transient),
            linear_resolved: make(
                "linear depth resolved",
                LINEAR_DEPTH_FORMAT,
                1,
                attachment | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            // Transient attachments take no memory where they are memoryless.
            bytes: pixels * 12,
        }
    }

    /// Memory of the targets that must exist in memory (the resolved colour and depth).
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
