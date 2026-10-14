//! GPU time per frame-graph pass, from timestamp queries when the device has them
//! (`timestamp-query`, an optional WebGPU feature; D2). Results arrive a few frames late
//! through a small ring of read-back buffers; a frame that finds no free buffer is simply
//! not measured. Nothing is allocated after creation.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const FREE: u8 = 0;
const PENDING: u8 = 1;
const MAPPED: u8 = 2;
const RING: usize = 3;

struct Readback {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

/// Timestamp queries for a fixed list of passes.
pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    ring: Vec<Readback>,
    passes: usize,
    /// Nanoseconds per timestamp tick.
    period: f32,
    /// The readback the current frame writes, if any.
    current: Option<usize>,
    latest: Vec<f32>,
    valid: bool,
}

impl GpuTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, passes: usize) -> Self {
        let count = (passes * 2) as u32;
        let size = u64::from(count) * 8;
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("pass timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp resolve"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let ring = (0..RING)
            .map(|_| Readback {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("timestamp readback"),
                    size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(FREE)),
            })
            .collect();
        Self {
            queries,
            resolve,
            ring,
            passes,
            period: queue.get_timestamp_period(),
            current: None,
            latest: vec![0.0; passes],
            valid: false,
        }
    }

    /// Collects finished results and picks a buffer for this frame.
    pub fn begin_frame(&mut self) {
        for slot in &self.ring {
            if slot.state.load(Ordering::Acquire) == MAPPED {
                if let Ok(view) = slot.buffer.get_mapped_range(..) {
                    let ticks: &[u64] = bytemuck::cast_slice(&view);
                    for (i, ms) in self.latest.iter_mut().enumerate() {
                        let (start, end) = (ticks[i * 2], ticks[i * 2 + 1]);
                        *ms = end.saturating_sub(start) as f32 * self.period / 1.0e6;
                    }
                    self.valid = true;
                }
                slot.buffer.unmap();
                slot.state.store(FREE, Ordering::Release);
            }
        }
        self.current = self
            .ring
            .iter()
            .position(|s| s.state.load(Ordering::Acquire) == FREE);
    }

    /// Timestamp writes for render pass `pass`, when this frame is measured.
    pub fn render_writes(&self, pass: usize) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        self.current.map(|_| wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(pass as u32 * 2),
            end_of_pass_write_index: Some(pass as u32 * 2 + 1),
        })
    }

    /// Timestamp writes for compute pass `pass`, when this frame is measured.
    pub fn compute_writes(&self, pass: usize) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
        self.current.map(|_| wgpu::ComputePassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(pass as u32 * 2),
            end_of_pass_write_index: Some(pass as u32 * 2 + 1),
        })
    }

    /// Copies this frame's timestamps out; call after the last pass.
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(i) = self.current {
            let count = (self.passes * 2) as u32;
            encoder.resolve_query_set(&self.queries, 0..count, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(
                &self.resolve,
                0,
                &self.ring[i].buffer,
                0,
                u64::from(count) * 8,
            );
        }
    }

    /// Starts reading this frame's results back; call after submitting.
    pub fn end_frame(&mut self) {
        if let Some(i) = self.current.take() {
            let state = self.ring[i].state.clone();
            state.store(PENDING, Ordering::Release);
            self.ring[i]
                .buffer
                .map_async(wgpu::MapMode::Read, .., move |result| {
                    state.store(
                        if result.is_ok() { MAPPED } else { FREE },
                        Ordering::Release,
                    );
                });
        }
    }

    /// GPU milliseconds per pass from the latest measured frame.
    pub fn latest(&self) -> Option<&[f32]> {
        self.valid.then_some(self.latest.as_slice())
    }
}
