//! Finishing a frame's command encoders in parallel.
//!
//! wgpu records a pass as a list of commands and encodes it into the backend (Metal,
//! Vulkan, D3D12) only when its encoder is finished: that is most of a frame's CPU cost
//! on the render thread (renderer §9). The renderer records a frame into a few encoders,
//! in order, and [`finish`] encodes them at once: the render thread takes the largest (it
//! starts at once, while the workers wake), a small pool of worker threads the others.
//! The command buffers come back in recording order and are submitted together, so the
//! GPU runs the same commands in the same order.

use std::sync::OnceLock;

/// Worker threads at most: a frame has five encoders, and the render thread finishes one
/// itself.
const WORKERS: usize = 4;

/// The pool shared by every renderer of the process, or `None` on a machine with too few
/// cores to gain from it.
fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        let workers = WORKERS.min(cores.saturating_sub(1));
        (workers > 0)
            .then(|| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .thread_name(|i| format!("canopy-encode-{i}"))
                    .build()
                    .ok()
            })
            .flatten()
    })
    .as_ref()
}

/// Finishes `encoders` (emptying the list), the one at `main` on this thread, and appends
/// their command buffers to `out` in the same order. `slots` is scratch room kept between
/// frames.
pub fn finish(
    encoders: &mut Vec<wgpu::CommandEncoder>,
    main: usize,
    slots: &mut Vec<Option<wgpu::CommandBuffer>>,
    out: &mut Vec<wgpu::CommandBuffer>,
) {
    let _span = tracing::trace_span!("finish").entered();
    slots.clear();
    slots.resize_with(encoders.len(), || None);
    match pool().filter(|_| encoders.len() > 1) {
        Some(pool) => pool.in_place_scope(|scope| {
            let mut own = None;
            for (i, (encoder, slot)) in encoders.drain(..).zip(slots.iter_mut()).enumerate() {
                if i == main {
                    own = Some((encoder, slot));
                } else {
                    scope.spawn(move |_| {
                        let _span = tracing::trace_span!("finish_encoder").entered();
                        *slot = Some(encoder.finish());
                    });
                }
            }
            if let Some((encoder, slot)) = own {
                *slot = Some(encoder.finish());
            }
        }),
        None => {
            for (encoder, slot) in encoders.drain(..).zip(slots.iter_mut()) {
                *slot = Some(encoder.finish());
            }
        }
    }
    out.extend(slots.drain(..).flatten());
}
