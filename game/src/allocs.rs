#![allow(unsafe_code)] // D104: the opt-in counting allocator (feature `count-allocs`).
//! Heap allocations made by the calling thread, for `canopy bench-render --scenes match`
//! (renderer §9): with the `count-allocs` feature the game runs on a counting allocator
//! (the system's, plus a thread-local count); without it nothing is counted.
//!
//! [`trace`] prints the call stacks of the next allocations counted, to find them (the
//! benchmark's `CANOPY_ALLOC_TRACE=<n>`); their own allocations are counted too.

/// Allocations and bytes counted on this thread since [`start`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Count {
    pub allocations: u64,
    pub bytes: u64,
}

#[cfg(feature = "count-allocs")]
mod counting {
    use super::Count;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct Counting;

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
        static COUNT: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
        static TRACES: Cell<u32> = const { Cell::new(0) };
        static INSIDE: Cell<bool> = const { Cell::new(false) };
    }

    fn note(size: usize) {
        let _ = ON.try_with(|on| {
            if !on.get() {
                return;
            }
            let _ = COUNT.try_with(|c| {
                let (n, b) = c.get();
                c.set((n + 1, b + size as u64));
            });
            let _ = TRACES.try_with(|left| {
                if left.get() == 0 || INSIDE.try_with(Cell::get).unwrap_or(true) {
                    return;
                }
                left.set(left.get() - 1);
                let _ = INSIDE.try_with(|i| i.set(true));
                let trace = std::backtrace::Backtrace::force_capture();
                eprintln!("allocation of {size} bytes:\n{trace}");
                let _ = INSIDE.try_with(|i| i.set(false));
            });
        });
    }

    // SAFETY: every call is forwarded to the system allocator unchanged.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            note(layout.size());
            // SAFETY: the caller's contract is passed through.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            note(layout.size());
            // SAFETY: the caller's contract is passed through.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            note(new_size);
            // SAFETY: the caller's contract is passed through.
            unsafe { System.realloc(ptr, layout, new_size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller's contract is passed through.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    pub fn trace(n: u32) {
        TRACES.with(|t| t.set(n));
    }

    pub fn start() {
        COUNT.with(|c| c.set((0, 0)));
        ON.with(|on| on.set(true));
    }

    pub fn stop() -> Count {
        ON.with(|on| on.set(false));
        let (allocations, bytes) = COUNT.with(Cell::get);
        Count { allocations, bytes }
    }
}

/// Whether allocations are counted (the `count-allocs` feature).
pub const COUNTING: bool = cfg!(feature = "count-allocs");

/// Starts counting this thread's allocations.
pub fn start() {
    #[cfg(feature = "count-allocs")]
    counting::start();
}

/// Prints the call stacks of the next `n` allocations counted on this thread.
pub fn trace(n: u32) {
    #[cfg(feature = "count-allocs")]
    counting::trace(n);
    #[cfg(not(feature = "count-allocs"))]
    let _ = n;
}

/// Stops counting and returns what this thread allocated since [`start`].
pub fn stop() -> Count {
    #[cfg(feature = "count-allocs")]
    return counting::stop();
    #[cfg(not(feature = "count-allocs"))]
    Count::default()
}
