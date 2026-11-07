//! Finishing a frame's command encoders in parallel, and other independent work of a
//! frame on the same few worker threads.
//!
//! wgpu records a pass as a list of commands and encodes it into the backend (Metal,
//! Vulkan, D3D12) only when its encoder is finished: that is most of a frame's CPU cost
//! on the render thread (renderer §9). The renderer records a frame into a few encoders,
//! in order, and a [`Finisher`] encodes them: a small pool of worker threads takes each
//! as soon as it is recorded, while the frame records on, and the render thread finishes
//! the largest itself at the end, then any a worker has not taken yet. The command
//! buffers come back in recording order and are submitted together, so the GPU runs the
//! same commands in the same order.

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

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

/// Runs `work` on a worker thread while `here` runs on this one, and returns what `work`
/// made: for a frame's independent CPU work. When no worker has started it by the time
/// `here` is done, this thread does it; without workers, one after the other.
pub fn alongside<R: Send>(work: impl FnOnce() -> R + Send, here: impl FnOnce()) -> R {
    let Some(pool) = pool() else {
        here();
        return work();
    };
    let work = Mutex::new(Some(work));
    let made = Mutex::new(None);
    let run = || {
        let work = work.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(work) = work {
            let result = work();
            *made.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        }
    };
    pool.in_place_scope(|scope| {
        scope.spawn(|_| run());
        here();
        run();
    });
    made.into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .expect("the scope waits for its work")
}

type Finished = (usize, std::thread::Result<wgpu::CommandBuffer>);

/// What the render thread and the workers share: encoders waiting to be finished, and
/// the command buffers finished.
#[derive(Default)]
struct Shared {
    waiting: Mutex<VecDeque<(usize, wgpu::CommandEncoder)>>,
    finished: Mutex<Vec<Finished>>,
    arrived: Condvar,
}

impl Shared {
    /// Finishes the next waiting encoder, if any is left; false when none was.
    fn finish_next(&self) -> bool {
        let next = self
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        let Some((index, encoder)) = next else {
            return false;
        };
        let finished = std::panic::catch_unwind(AssertUnwindSafe(|| encoder.finish()));
        self.finished
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((index, finished));
        self.arrived.notify_one();
        true
    }
}

/// A frame's encoders in recording order. Those sent wait in a queue a worker takes from
/// as soon as each is sent, while the frame records on; those kept are finished on this
/// thread at the end ([`Self::finish`]), which then also takes whatever is still waiting,
/// so a slow or busy worker never holds the frame up longer than the work itself.
#[derive(Default)]
pub struct Finisher {
    shared: Arc<Shared>,
    /// Encoders kept for this thread, with their place in the frame.
    kept: Vec<(usize, wgpu::CommandEncoder)>,
    /// Encoders handed out (sent and kept) and sent this frame.
    count: usize,
    sent: usize,
    slots: Vec<Option<wgpu::CommandBuffer>>,
}

impl Finisher {
    /// Queues `encoder` for a worker (for this thread at the end without workers).
    pub fn send(&mut self, encoder: wgpu::CommandEncoder) {
        let index = self.count;
        self.count += 1;
        self.sent += 1;
        self.shared
            .waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back((index, encoder));
        if let Some(pool) = pool() {
            let shared = self.shared.clone();
            pool.spawn(move || {
                let _span = tracing::trace_span!("finish_encoder").entered();
                shared.finish_next();
            });
        }
    }

    /// Finishes `encoder` on this thread at the end.
    pub fn keep(&mut self, encoder: wgpu::CommandEncoder) {
        self.kept.push((self.count, encoder));
        self.count += 1;
    }

    /// Finishes the kept encoders and any still waiting, waits for those the workers took,
    /// and appends every command buffer to `out` in recording order. A panic while
    /// finishing on a worker carries on here.
    pub fn finish(&mut self, out: &mut Vec<wgpu::CommandBuffer>) {
        let _span = tracing::trace_span!("finish").entered();
        self.slots.clear();
        self.slots.resize_with(self.count, || None);
        for (index, encoder) in self.kept.drain(..) {
            self.slots[index] = Some(encoder.finish());
        }
        while self.shared.finish_next() {}
        let mut finished = self
            .shared
            .finished
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut received = 0;
        while received < self.sent {
            for (index, buffer) in finished.drain(..) {
                received += 1;
                match buffer {
                    Ok(buffer) => self.slots[index] = Some(buffer),
                    Err(panic) => std::panic::resume_unwind(panic),
                }
            }
            if received < self.sent {
                finished = self
                    .shared
                    .arrived
                    .wait(finished)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        }
        drop(finished);
        (self.count, self.sent) = (0, 0);
        out.extend(self.slots.drain(..).flatten());
    }
}
