//! The frame shade: a map's painted darkening, one value per terrain vertex (0 lit as
//! authored, 1 black). `scene_out` (`frame.wgsl`) multiplies every forward surface by it,
//! terrain, scenery, units, water and effects alike, so a map can keep its non-playable
//! frame below its playable ground (docs/art-direction.md). Without a shade the texture is
//! one black texel and the frame uniform turns the lookup off.

/// The shade texture on the GPU.
pub struct ShadeGpu {
    pub view: wgpu::TextureView,
    /// World x and z of vertex 0 and vertices per side; `None` without a shade.
    pub placement: Option<(f32, u32)>,
}

impl ShadeGpu {
    /// No shade: one black (unshaded) texel.
    pub fn none(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            view: upload(device, queue, 1, &[0]),
            placement: None,
        }
    }

    /// `values` are `vertices²` shade values, row index z, vertex 0 at `origin`.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        origin: f32,
        vertices: u32,
        values: &[f32],
    ) -> Self {
        let _span = tracing::info_span!("shade_upload").entered();
        let bytes: Vec<u8> = values
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        Self {
            view: upload(device, queue, vertices, &bytes),
            placement: Some((origin, vertices)),
        }
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    side: u32,
    bytes: &[u8],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: side,
        height: side,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame shade"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side),
            rows_per_image: Some(side),
        },
        size,
    );
    texture.create_view(&Default::default())
}
