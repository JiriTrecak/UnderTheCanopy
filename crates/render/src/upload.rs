//! Per-frame buffer and texture uploads through a reused staging belt.
//!
//! `Queue::write_buffer` and `Queue::write_texture` make and free a staging buffer for
//! every call; a frame makes dozens (uniforms, instances, emitters, the cutaway mask, fog
//! changes). [`Uploads`] copies them into the chunks of a [`wgpu::util::StagingBelt`]
//! the renderer keeps, records the copies at the start of the frame's first encoder, and
//! recalls the chunks once the frame is submitted. What the GPU reads is the same: every
//! copy lands before the frame's passes.

use std::cell::RefCell;

/// Somewhere to write buffers and textures before a frame: the queue itself, or a
/// frame's [`Uploads`].
pub trait Writes {
    fn write_buffer(&self, buffer: &wgpu::Buffer, offset: wgpu::BufferAddress, data: &[u8]);
    fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    );
}

impl Writes for wgpu::Queue {
    fn write_buffer(&self, buffer: &wgpu::Buffer, offset: wgpu::BufferAddress, data: &[u8]) {
        wgpu::Queue::write_buffer(self, buffer, offset, data);
    }

    fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        wgpu::Queue::write_texture(self, texture, data, layout, size);
    }
}

/// Bytes of a staging chunk: a frame's uniforms and instance lists fit in a few.
pub const CHUNK: wgpu::BufferAddress = 256 * 1024;

/// One frame's uploads: buffer writes and texture writes go through the belt.
pub struct Uploads<'a> {
    queue: &'a wgpu::Queue,
    inner: RefCell<(
        &'a mut wgpu::util::StagingBelt,
        Option<wgpu::CommandEncoder>,
    )>,
}

impl<'a> Uploads<'a> {
    pub fn new(
        device: &wgpu::Device,
        queue: &'a wgpu::Queue,
        belt: &'a mut wgpu::util::StagingBelt,
    ) -> Self {
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("uploads"),
        });
        Self {
            queue,
            inner: RefCell::new((belt, Some(encoder))),
        }
    }

    /// The copies recorded so far, once: submit them before anything that reads them.
    pub fn take(&self) -> Option<wgpu::CommandBuffer> {
        let mut inner = self.inner.borrow_mut();
        let encoder = inner.1.take()?;
        inner.0.finish();
        Some(encoder.finish())
    }

    /// The encoder of the copies recorded so far, unfinished, once: finish and submit it
    /// before anything that reads them.
    pub fn take_encoder(&self) -> Option<wgpu::CommandEncoder> {
        let mut inner = self.inner.borrow_mut();
        let encoder = inner.1.take()?;
        inner.0.finish();
        Some(encoder)
    }

    /// Returns the belt's chunks for reuse once their copies are submitted.
    pub fn recall(self) {
        let inner = self.inner.into_inner();
        if inner.1.is_none() {
            inner.0.recall();
        }
    }
}

impl Writes for Uploads<'_> {
    fn write_buffer(&self, buffer: &wgpu::Buffer, offset: wgpu::BufferAddress, data: &[u8]) {
        let Some(size) = wgpu::BufferSize::new(data.len() as u64) else {
            return;
        };
        let mut inner = self.inner.borrow_mut();
        let (belt, encoder) = &mut *inner;
        match encoder {
            Some(encoder) => belt
                .write_buffer(encoder, buffer, offset, size)
                .copy_from_slice(data),
            // After the copies were taken, a late write goes straight to the queue.
            None => self.queue.write_buffer(buffer, offset, data),
        }
    }

    fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        let mut inner = self.inner.borrow_mut();
        let (belt, encoder) = &mut *inner;
        let staged = encoder
            .as_mut()
            .is_some_and(|encoder| stage_texture(belt, encoder, &texture, data, layout, size));
        if !staged {
            self.queue.write_texture(texture, data, layout, size);
        }
    }
}

/// Copies a texture write through the belt, rows padded to the copy alignment, as
/// `Queue::write_texture` does with a staging buffer of its own each time. Returns false
/// (nothing recorded) for what it does not handle: block-compressed, depth and stencil
/// formats, or data shorter than the layout says.
fn stage_texture(
    belt: &mut wgpu::util::StagingBelt,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::TexelCopyTextureInfo<'_>,
    data: &[u8],
    layout: wgpu::TexelCopyBufferLayout,
    size: wgpu::Extent3d,
) -> bool {
    let format = texture.texture.format();
    if format.block_dimensions() != (1, 1)
        || format.has_depth_aspect()
        || format.has_stencil_aspect()
    {
        return false;
    }
    let Some(texel) = format.block_copy_size(Some(texture.aspect)) else {
        return false;
    };
    let row = u64::from(size.width) * u64::from(texel);
    let (rows, layers) = (
        u64::from(size.height),
        u64::from(size.depth_or_array_layers),
    );
    let source_row = layout.bytes_per_row.map_or(row, u64::from);
    let source_image = layout.rows_per_image.map_or(rows, u64::from) * source_row;
    let needed = (layers > 0 && rows > 0)
        .then(|| layout.offset + (layers - 1) * source_image + (rows - 1) * source_row + row);
    if needed.is_none_or(|n| n > data.len() as u64) {
        return false;
    }
    let padded = row.next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
    let Some(bytes) = wgpu::BufferSize::new(padded * rows * layers) else {
        return false;
    };
    let alignment =
        wgpu::BufferSize::new(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)).expect("non-zero");
    let slice = belt.allocate(bytes, alignment);
    let (buffer, offset) = (slice.buffer().clone(), slice.offset());
    {
        let Ok(mut view) = slice.get_mapped_range_mut() else {
            return false;
        };
        for layer in 0..layers {
            for y in 0..rows {
                let from = (layout.offset + layer * source_image + y * source_row) as usize;
                let to = ((layer * rows + y) * padded) as usize;
                view.slice(to..to + row as usize)
                    .copy_from_slice(&data[from..from + row as usize]);
            }
        }
    }
    encoder.copy_buffer_to_texture(
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(size.height),
            },
        },
        *texture,
        size,
    );
    true
}
