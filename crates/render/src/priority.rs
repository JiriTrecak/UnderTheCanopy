//! Raised scheduling priority for the threads a frame waits on: the simulation, the
//! ragdoll stepper, the encoder workers and the simulation's worker pool.
//!
//! Threads spawned on macOS start at the default quality of service, which lets the
//! scheduler park them on efficiency cores while the render thread (the main thread, at
//! user-interactive) waits for them. This gives the calling thread the render thread's
//! class on macOS and an above-normal priority on Windows; elsewhere it does nothing.
//! Priority changes when work finishes, never what it computes (D4).
#![allow(unsafe_code)] // FFI to the platform's thread scheduling calls, nothing else.

/// Raises the calling thread's priority to that of the render thread. Returns whether the
/// platform took it (`false` where there is nothing to do).
pub fn raise_current_thread() -> bool {
    let raised = platform::raise();
    if !raised {
        tracing::debug!(
            thread = std::thread::current().name().unwrap_or("unnamed"),
            "thread priority left as it is"
        );
    }
    raised
}

#[cfg(target_vendor = "apple")]
mod platform {
    /// `QOS_CLASS_USER_INTERACTIVE` (`<sys/qos.h>`).
    const USER_INTERACTIVE: u32 = 0x21;

    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }

    pub fn raise() -> bool {
        // SAFETY: changes only the calling thread's scheduling class; takes no pointers.
        unsafe { pthread_set_qos_class_self_np(USER_INTERACTIVE, 0) == 0 }
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;

    /// `THREAD_PRIORITY_ABOVE_NORMAL` (`processthreadsapi.h`).
    const ABOVE_NORMAL: i32 = 1;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut c_void;
        fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
    }

    pub fn raise() -> bool {
        // SAFETY: the calling thread's pseudo handle needs no closing and stays valid for
        // the call.
        unsafe { SetThreadPriority(GetCurrentThread(), ABOVE_NORMAL) != 0 }
    }
}

#[cfg(not(any(target_vendor = "apple", windows)))]
mod platform {
    pub fn raise() -> bool {
        false
    }
}
