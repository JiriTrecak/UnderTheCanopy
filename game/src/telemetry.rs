//! Logging and profiling. `RUST_LOG` filters logs; the `profile` feature streams spans
//! and frame marks to Tracy. [`span_times`] sums the time spent in each span by name while
//! [`time_spans`] has it on (the editor's frame log and benchmark). `CANOPY_SPANS=1` has
//! it on from the start, trace-level spans too whatever `RUST_LOG` says, and prints the
//! table ([`print_span_times`]): a headless profile of a shot, a benchmark or a match.
//!
//! `CANOPY_HITCHES=<ms>` logs every window frame longer than `<ms>` with the slowest
//! spans that closed on the window thread during it: what a first-use stall was doing
//! (a pipeline built, a texture uploaded, an effect compiled) without Tracy; the first
//! frame counts from the start. `CANOPY_QUIT_AFTER=<s>` closes the window that many
//! seconds after the start (timed runs of the windowed game).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::span::{self, Attributes, Id};
use tracing_subscriber::{
    EnvFilter, Layer, filter::Targets, fmt, layer::Context, layer::SubscriberExt,
    registry::LookupSpan, util::SubscriberInitExt,
};

/// Logs too noisy at their own levels: symphonia reports every MP3's header and every
/// recoverable bitstream glitch at INFO and WARN (a target prefix covers its crates).
const QUIET: &str = "wgpu_core=warn,wgpu_hal=warn,naga=warn,symphonia=error";

fn filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(format!("info,{QUIET}")))
}

pub fn init() {
    let _ = STARTED.set(Instant::now());
    let hitches = std::env::var("CANOPY_HITCHES")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    if let Some(ms) = hitches {
        let _ = HITCH_MS.set(ms);
    }
    let every_span = std::env::var_os("CANOPY_SPANS").is_some();
    let registry = tracing_subscriber::registry()
        .with(hitches.map(|_| Hitches.with_filter(filter())))
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
    if FIRST_FRAME.set(()).is_ok()
        && let Some(started) = STARTED.get()
    {
        tracing::info!(ms = started.elapsed().as_millis() as u64, "first frame");
    }
    if let Some(&limit) = HITCH_MS.get() {
        FRAME.with_borrow_mut(|frame| frame.end(limit));
    }
}

static HITCH_MS: OnceLock<f32> = OnceLock::new();
static STARTED: OnceLock<Instant> = OnceLock::new();
static FIRST_FRAME: OnceLock<()> = OnceLock::new();

/// Whether `CANOPY_QUIT_AFTER` seconds have passed since the start.
pub fn quit_due() -> bool {
    static AFTER: OnceLock<Option<f32>> = OnceLock::new();
    let after = AFTER.get_or_init(|| {
        std::env::var("CANOPY_QUIT_AFTER")
            .ok()
            .and_then(|v| v.parse().ok())
    });
    match (after, STARTED.get()) {
        (Some(after), Some(started)) => started.elapsed().as_secs_f32() >= *after,
        _ => false,
    }
}

/// Spans shorter than this are not listed.
const SPAN_MIN_MS: f32 = 0.5;
/// Spans listed per slow frame.
const SPANS_SHOWN: usize = 14;

thread_local! {
    static FRAME: RefCell<Frame> = RefCell::new(Frame::default());
}

/// The window thread's spans since the last frame mark.
#[derive(Default)]
struct Frame {
    marked: Option<Instant>,
    first: Option<Instant>,
    spans: Vec<(&'static str, f32)>,
}

impl Frame {
    fn end(&mut self, limit: f32) {
        let now = Instant::now();
        let first = *self.first.get_or_insert(now);
        // The first frame counts from the start: what the window waited for.
        if let Some(previous) = self.marked.or_else(|| STARTED.get().copied()) {
            let ms = (now - previous).as_secs_f32() * 1000.0;
            if ms > limit {
                self.spans.sort_by(|a, b| b.1.total_cmp(&a.1));
                let spans: Vec<String> = self
                    .spans
                    .iter()
                    .take(SPANS_SHOWN)
                    .map(|(name, ms)| format!("{name} {ms:.1}"))
                    .collect();
                tracing::info!(
                    ms = ms.round(),
                    at_s = format!("{:.2}", (now - first).as_secs_f32()),
                    spans = spans.join(", "),
                    "hitch"
                );
            }
        }
        self.marked = Some(now);
        self.spans.clear();
    }
}

struct Hitches;

/// When a span opened (the hitch log's own, in its extensions).
struct Opened(Instant);

impl<S: tracing::Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Hitches {
    fn on_new_span(&self, _: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Opened(Instant::now()));
        }
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(opened) = span.extensions().get::<Opened>().map(|o| o.0) else {
            return;
        };
        let ms = opened.elapsed().as_secs_f32() * 1000.0;
        if ms < SPAN_MIN_MS {
            return;
        }
        let name = span.name();
        // The window thread's, from the start (other threads never mark a frame).
        if std::thread::current().name() == Some("main") {
            FRAME.with_borrow_mut(|frame| frame.spans.push((name, ms)));
        }
    }
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
