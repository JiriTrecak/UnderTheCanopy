//! Offscreen targets read back to the CPU (`just shot`, headless tests).

/// An RGBA8 sRGB colour target that can be copied out.
pub struct Capture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// A non-sRGB view of the same texels, for the UI pass (which blends in sRGB space).
    pub ui_view: wgpu::TextureView,
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_row: u32,
}

/// The format captures render to.
pub const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

impl Capture {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAPTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            ui_view,
            buffer,
            width,
            height,
            padded_row,
        }
    }

    /// Copies the target into the read-back buffer (record after rendering into it).
    pub fn copy(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Waits for the GPU and returns tightly packed RGBA8 rows (sRGB encoded).
    pub fn read(&self, device: &wgpu::Device) -> anyhow::Result<Vec<u8>> {
        let (sender, receiver) = std::sync::mpsc::channel();
        self.buffer
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = sender.send(result);
            });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let mut out = Vec::with_capacity((self.width * self.height * 4) as usize);
        {
            let view = self.buffer.get_mapped_range(..)?;
            for row in view.chunks_exact(self.padded_row as usize) {
                out.extend_from_slice(&row[..self.width as usize * 4]);
            }
        }
        self.buffer.unmap();
        Ok(out)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
