//! Logging and profiling. `RUST_LOG` filters logs; the `profile` feature streams spans
//! and frame marks to Tracy. [`span_times`] sums the time spent in each span by name while
//! [`time_spans`] has it on (the editor's frame log and benchmark). `CANOPY_SPANS=1` has
//! it on from the start, trace-level spans too whatever `RUST_LOG` says, and prints the
//! table ([`print_span_times`]): a headless profile of a shot, a benchmark or a match.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::span::{Attributes, Id};
use tracing_subscriber::{
    EnvFilter, Layer, filter::Targets, fmt, layer::Context, layer::SubscriberExt,
    registry::LookupSpan, util::SubscriberInitExt,
};

fn filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"))
}

pub fn init() {
    let every_span = std::env::var_os("CANOPY_SPANS").is_some();
    let registry = tracing_subscriber::registry()
        .with(fmt::layer().with_filter(filter()))
        .with((!every_span).then(|| SpanTimes.with_filter(filter())))
        .with(every_span.then(|| {
            SpanTimes.with_filter(
                Targets::new()
                    .with_default(tracing::Level::WARN)
                    .with_target("canopy", tracing::Level::TRACE),
            )
        }));
    #[cfg(feature = "profile")]
    let registry = registry.with(tracing_tracy::TracyLayer::default().with_filter(filter()));
    registry.init();
    if every_span {
        time_spans(true);
    }
}

/// Prints the spans' times since the last [`span_times`] (with `CANOPY_SPANS`), the most
/// expensive first, and starts again.
pub fn print_span_times() {
    if std::env::var_os("CANOPY_SPANS").is_none() {
        return;
    }
    let times = span_times();
    let mut rows: Vec<_> = times.iter().collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.1.total));
    eprintln!(
        "{:<32} {:>9} {:>11} {:>10} {:>10}",
        "span", "count", "total ms", "mean µs", "max µs"
    );
    for (name, t) in rows {
        eprintln!(
            "{name:<32} {:>9} {:>11.2} {:>10.2} {:>10.0}",
            t.count,
            t.total.as_secs_f64() * 1000.0,
            t.total.as_secs_f64() * 1e6 / f64::from(t.count.max(1)),
            t.max.as_secs_f64() * 1e6
        );
    }
}

/// Marks the end of a presented frame.
pub fn frame_mark() {
    #[cfg(feature = "profile")]
    tracing_tracy::client::frame_mark();
}

/// Time spent in spans of one name since the last [`span_times`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SpanTime {
    pub count: u32,
    pub total: Duration,
    /// The longest single span.
    pub max: Duration,
}

static TIMING: AtomicBool = AtomicBool::new(false);
static TIMES: Mutex<BTreeMap<&'static str, SpanTime>> = Mutex::new(BTreeMap::new());

/// Starts (or stops) summing span times; off, spans cost one atomic load more.
pub fn time_spans(on: bool) {
    TIMING.store(on, Ordering::Relaxed);
}

/// The spans' times since the last call, by name, and starts again.
pub fn span_times() -> BTreeMap<&'static str, SpanTime> {
    std::mem::take(&mut *TIMES.lock().unwrap_or_else(|e| e.into_inner()))
}

/// When a span was entered (in its extensions).
struct Entered(Instant);

struct SpanTimes;

impl<S: tracing::Subscriber + for<'a> LookupSpan<'a>> Layer<S> for SpanTimes {
    fn on_new_span(&self, _: &Attributes<'_>, _: &Id, _: Context<'_, S>) {}

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if !TIMING.load(Ordering::Relaxed) {
            return;
        }
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().replace(Entered(Instant::now()));
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        if !TIMING.load(Ordering::Relaxed) {
            return;
        }
        let Some(span) = ctx.span(id) else {
            return;
        };
        let Some(Entered(at)) = span.extensions_mut().remove::<Entered>() else {
            return;
        };
        let took = at.elapsed();
        let mut times = TIMES.lock().unwrap_or_else(|e| e.into_inner());
        let entry = times.entry(span.metadata().name()).or_default();
        entry.count += 1;
        entry.total += took;
        entry.max = entry.max.max(took);
    }
}
