//! Repeatable GPU measurements of one view (`just bench-render`, renderer §9).
//!
//! Apple GPUs write pass timestamps at coarse boundaries and attribute work to
//! neighbouring passes (D34), so the benchmark measures wall time instead: a whole frame
//! from the start of recording to idle, and every pass alone, submitted in its own
//! command buffer and waited for. The fixed cost of a submit and a wait (the floor, a
//! command buffer clearing a tiny target) is subtracted from each pass. Isolated pass times also
//! include the driver's encoding of that pass, so they are an upper bound and their sum
//! exceeds the frame's GPU time a little. Times are the fastest fifth of the samples
//! ([`QUANTILE`]): the development machine is shared, and slower samples are mostly other
//! work.

use crate::{CameraView, Gpu, Renderer, renderer::Triangles};
use serde::Serialize;
use std::time::Instant;

/// What one view costs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Benchmark {
    /// Frame-graph passes, in order.
    pub passes: Vec<&'static str>,
    /// Submit-to-idle time of each pass alone ([`QUANTILE`]), minus the floor, ms.
    pub pass_ms: Vec<f32>,
    /// Median submit-and-wait time of a command buffer clearing a tiny target, ms.
    pub floor_ms: f32,
    /// Time from the start of recording a whole frame to idle ([`QUANTILE`]), ms.
    pub frame_ms: f32,
    pub frame_median_ms: f32,
    pub frame_best_ms: f32,
    /// CPU time recording and submitting a frame ([`QUANTILE`]), ms.
    pub cpu_ms: f32,
    /// `frame_ms − cpu_ms`: the GPU's share of a frame after submission, ms.
    pub gpu_ms: f32,
    pub draws: u32,
    pub triangles: Triangles,
    pub forward_triangles: u64,
    pub frames: u32,
}

/// The share of samples a reported time sits at: the fastest fifth, so other work on a
/// shared machine (builds, other GPU clients) disturbs the result less than a median.
pub const QUANTILE: f32 = 0.2;

fn quantile(values: &mut [f32], q: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    values[((values.len() - 1) as f32 * q).round() as usize]
}

fn median(values: &mut [f32]) -> f32 {
    quantile(values, 0.5)
}

impl Renderer {
    /// Measures `frames` frames of `camera` at `seconds` into `output` (after a few
    /// warm-up frames): whole frames, then every pass alone.
    pub fn benchmark(
        &mut self,
        gpu: &Gpu,
        camera: &CameraView,
        seconds: f32,
        output: &wgpu::TextureView,
        frames: u32,
    ) -> Benchmark {
        let frames = frames.max(3);
        let wait = || {
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        };
        let isolate = self.isolate_passes;
        self.isolate_passes = false;
        for _ in 0..5 {
            self.render(gpu, camera, seconds, output, None);
            wait();
        }
        let (mut walls, mut cpus) = (Vec::new(), Vec::new());
        let mut last = None;
        for _ in 0..frames {
            let start = Instant::now();
            let report = self.render(gpu, camera, seconds, output, None);
            wait();
            walls.push(start.elapsed().as_secs_f32() * 1000.0);
            cpus.push(report.cpu_total_ms);
            last = Some(report);
        }
        // The floor: a command buffer clearing a tiny target, so it really reaches the
        // GPU and pays what any pass pays to start and finish.
        let tiny = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("benchmark floor"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let tiny = tiny.create_view(&Default::default());
        let mut floors: Vec<f32> = (0..frames.max(20))
            .map(|_| {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("benchmark floor"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &tiny,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                }));
                let start = Instant::now();
                gpu.queue.submit([encoder.finish()]);
                wait();
                start.elapsed().as_secs_f32() * 1000.0
            })
            .collect();
        let floor_ms = median(&mut floors);
        self.isolate_passes = true;
        let mut per_pass: Vec<Vec<f32>> = Vec::new();
        for _ in 0..frames {
            let report = self.render(gpu, camera, seconds, output, None);
            wait();
            per_pass.resize(report.isolated_ms.len(), Vec::new());
            for (all, ms) in per_pass.iter_mut().zip(&report.isolated_ms) {
                all.push(*ms);
            }
        }
        self.isolate_passes = isolate;
        let last = last.unwrap_or_default();
        let mut triangles = last.triangles;
        if let Some(statics) = self.statics() {
            let (camera, shadow) = statics.read_triangles(&gpu.device, &gpu.queue);
            triangles.statics = camera;
            triangles.shadow += shadow;
        }
        let frame_ms = quantile(&mut walls, QUANTILE);
        let frame_median_ms = median(&mut walls);
        let cpu_ms = quantile(&mut cpus, QUANTILE);
        Benchmark {
            passes: last.passes.clone(),
            pass_ms: per_pass
                .iter_mut()
                .map(|v| (quantile(v, QUANTILE) - floor_ms).max(0.0))
                .collect(),
            floor_ms,
            frame_ms,
            frame_median_ms,
            frame_best_ms: walls.iter().copied().fold(f32::MAX, f32::min),
            cpu_ms,
            gpu_ms: (frame_ms - cpu_ms).max(0.0),
            draws: last.draws,
            forward_triangles: triangles.forward(),
            triangles,
            frames,
        }
    }
}
