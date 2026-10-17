//! The feedback screenshot: one frame read back from the GPU and encoded off the frame.
//!
//! Opening the form asks for a grab ([`Grab::request`]). The next frame is drawn without
//! the form and the game menu, into a texture of its own instead of the window (the
//! player keeps seeing the previous frame, so nothing flickers), and [`Grab::record`]
//! copies it into a read-back buffer. [`Grab::poll`] waits for the copy without blocking;
//! a worker thread then converts, scales (at most [`MAX_WIDTH`] wide) and encodes it as a
//! JPEG, and makes the form's thumbnail.

use canopy_render::Gpu;
use std::sync::mpsc;

/// The widest screenshot sent, in pixels.
pub const MAX_WIDTH: u32 = 1600;
/// The thumbnail's width, in pixels.
const THUMB_WIDTH: u32 = 640;
const QUALITY: u8 = 85;
/// The server's limit for a screenshot; a larger JPEG is encoded again more coarsely.
const MAX_BYTES: usize = 2 * 1024 * 1024;

/// An encoded screenshot and its thumbnail.
#[derive(Debug, Clone, Default)]
pub struct Shot {
    pub jpeg: Vec<u8>,
    pub thumb: Thumb,
}

/// sRGB RGBA8 pixels for the form.
#[derive(Debug, Clone, Default)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A texture to draw the grabbed frame into instead of the window: the window's format
/// and size, readable, with a non-sRGB view for the UI.
pub struct Target {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub ui_view: wgpu::TextureView,
}

impl Target {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, size: [u32; 2]) -> Self {
        let ui_format = format.remove_srgb_suffix();
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("feedback frame"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: if ui_format == format {
                &[]
            } else {
                std::slice::from_ref(&ui_format)
            },
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(ui_format),
            ..Default::default()
        });
        Self {
            texture,
            view,
            ui_view,
        }
    }
}

struct Reading {
    buffer: wgpu::Buffer,
    size: [u32; 2],
    padded_row: u32,
    bgra: bool,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

#[derive(Default)]
pub struct Grab {
    due: bool,
    reading: Option<Reading>,
    encoding: Option<mpsc::Receiver<Result<Shot, String>>>,
}

impl Grab {
    /// Takes the next frame (dropping any grab under way).
    pub fn request(&mut self) {
        self.due = true;
        self.reading = None;
        self.encoding = None;
    }

    /// Whether the next frame is to be grabbed: drawn without the form, offscreen.
    pub fn due(&self) -> bool {
        self.due
    }

    /// Whether a screenshot is being taken.
    pub fn busy(&self) -> bool {
        self.due || self.reading.is_some() || self.encoding.is_some()
    }

    /// Copies the frame just drawn into `texture` (with `COPY_SRC`) for reading back.
    pub fn record(&mut self, gpu: &Gpu, texture: &wgpu::Texture) {
        if !std::mem::take(&mut self.due) {
            return;
        }
        let bgra = match texture.format() {
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
            other => {
                let (tx, rx) = mpsc::channel();
                let _ = tx.send(Err(format!("frames in {other:?} cannot be read back")));
                self.encoding = Some(rx);
                return;
            }
        };
        let size = [texture.width(), texture.height()];
        let padded_row = (size[0] * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("feedback screenshot"),
            size: u64::from(padded_row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("feedback screenshot"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = tx.send(result);
        });
        self.reading = Some(Reading {
            buffer,
            size,
            padded_row,
            bgra,
            mapped: rx,
        });
    }

    /// The screenshot once it is ready (or why there is none). Never blocks.
    pub fn poll(&mut self, gpu: &Gpu) -> Option<Result<Shot, String>> {
        if let Some(reading) = &self.reading {
            let _ = gpu.device.poll(wgpu::PollType::Poll);
            match reading.mapped.try_recv() {
                Ok(Ok(())) => {
                    let reading = self.reading.take().expect("checked above");
                    let (tx, rx) = mpsc::channel();
                    let spawned = std::thread::Builder::new()
                        .name("feedback-shot".into())
                        .spawn(move || {
                            let _span = tracing::info_span!("feedback screenshot").entered();
                            let _ = tx.send(encode(&reading));
                        });
                    if let Err(error) = spawned {
                        return Some(Err(error.to_string()));
                    }
                    self.encoding = Some(rx);
                }
                Ok(Err(error)) => {
                    self.reading = None;
                    return Some(Err(error.to_string()));
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.reading = None;
                    return Some(Err("the screenshot was lost".into()));
                }
            }
        }
        let rx = self.encoding.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.encoding = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.encoding = None;
                Some(Err("the screenshot was lost".into()))
            }
        }
    }
}

/// On the worker: the mapped rows to RGB, scaled and encoded.
fn encode(reading: &Reading) -> Result<Shot, String> {
    let [width, height] = reading.size;
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    {
        let mapped = reading
            .buffer
            .get_mapped_range(..)
            .map_err(|e| e.to_string())?;
        for row in mapped.chunks_exact(reading.padded_row as usize) {
            for px in row[..width as usize * 4].chunks_exact(4) {
                if reading.bgra {
                    rgb.extend_from_slice(&[px[2], px[1], px[0]]);
                } else {
                    rgb.extend_from_slice(&px[..3]);
                }
            }
        }
    }
    reading.buffer.unmap();
    let frame = image::RgbImage::from_raw(width, height, rgb).ok_or("bad frame size")?;
    shot_from(frame)
}

/// Scales a frame to at most [`MAX_WIDTH`] wide and encodes it, with a thumbnail.
pub fn shot_from(frame: image::RgbImage) -> Result<Shot, String> {
    use image::imageops::{FilterType, resize};
    let fit = |image: &image::RgbImage, max: u32| -> image::RgbImage {
        if image.width() <= max {
            return image.clone();
        }
        let h = (u64::from(image.height()) * u64::from(max) / u64::from(image.width())).max(1);
        resize(image, max, h as u32, FilterType::Triangle)
    };
    let scaled = fit(&frame, MAX_WIDTH);
    drop(frame);
    let mut jpeg = Vec::new();
    for quality in [QUALITY, 70, 50] {
        jpeg.clear();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode_image(&scaled)
            .map_err(|e| e.to_string())?;
        if jpeg.len() <= MAX_BYTES {
            break;
        }
    }
    let small = fit(&scaled, THUMB_WIDTH);
    let thumb = Thumb {
        width: small.width(),
        height: small.height(),
        rgba: image::DynamicImage::ImageRgb8(small).to_rgba8().into_raw(),
    };
    Ok(Shot { jpeg, thumb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_frames_are_scaled_and_encoded_with_a_thumbnail() {
        let frame = image::RgbImage::from_fn(3200, 1800, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        });
        let shot = shot_from(frame).unwrap();
        assert_eq!(&shot.jpeg[..3], &[0xff, 0xd8, 0xff]);
        assert!(shot.jpeg.len() <= MAX_BYTES);
        assert_eq!((shot.thumb.width, shot.thumb.height), (THUMB_WIDTH, 360));
        assert_eq!(shot.thumb.rgba.len(), (THUMB_WIDTH * 360 * 4) as usize);
        let decoded = image::load_from_memory(&shot.jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (MAX_WIDTH, 900));
        // Small frames keep their size.
        let small = shot_from(image::RgbImage::new(320, 200)).unwrap();
        let decoded = image::load_from_memory(&small.jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 200));
    }
}
