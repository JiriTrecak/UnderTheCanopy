//! Logging and profiling. `RUST_LOG` filters logs; the `profile` feature streams spans
//! and frame marks to Tracy.

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"));
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer());
    #[cfg(feature = "profile")]
    let registry = registry.with(tracing_tracy::TracyLayer::default());
    registry.init();
}

/// Marks the end of a presented frame.
pub fn frame_mark() {
    #[cfg(feature = "profile")]
    tracing_tracy::client::frame_mark();
}
