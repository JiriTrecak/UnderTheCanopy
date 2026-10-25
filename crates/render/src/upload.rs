//! Per-frame buffer uploads through a reused staging belt.
//!
//! `Queue::write_buffer` makes and frees a staging buffer for every call; a frame makes
//! dozens (uniforms, instances, emitters). [`Uploads`] copies them into the chunks of a
//! [`wgpu::util::StagingBelt`] the renderer keeps, records the copies into a command
//! buffer submitted just before the frame's, and recalls the chunks once the frame is
//! submitted. What the GPU reads is the same: every copy lands before the frame's passes.

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

/// One frame's uploads: buffer writes go through the belt, texture writes (rare) to the
/// queue.
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
        self.queue.write_texture(texture, data, layout, size);
    }
}
