//! Simulation cost of a real match, per tick and per tick step (`just bench-sim`).
//!
//! ```text
//! cargo run --release -p canopy-sim --example match_bench -- --data . --map echo-isles \
//!     [--crowd 50 | --ai 2] [--ticks 1600] [--warmup 0]
//! ```
//!
//! `--crowd N` is the measured battle of `canopy shot --crowd N` (eight players with N
//! soldiers each, `canopy_sim::crowd`); `--ai N` puts computer players on the first N
//! starts instead. Reports the whole tick (step and observer view) as mean, p50, p99 and
//! max, the `tick: …` step spans the world marks, and the costliest inner spans. The
//! digest at the end covers every tick's fingerprint and the final saved state: equal
//! digests mean identical simulations.
#![allow(clippy::disallowed_methods)] // wall-clock timing of the benchmark itself

use canopy_content::{Content, map};
use canopy_sim::{MatchSetup, Slot, SlotKind, World};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{
    Event, Metadata, Subscriber,
    span::{Attributes, Id, Record},
};

/// Times every span by name; a name nested in itself counts once (outermost).
#[derive(Default)]
struct Spans {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    next: u64,
    names: BTreeMap<u64, &'static str>,
    /// Open spans: id, name, start.
    open: Vec<(u64, &'static str, Instant)>,
    /// This tick's time per name.
    tick: BTreeMap<&'static str, Duration>,
}

impl Subscriber for &'static Spans {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut inner = self.inner.lock().unwrap();
        inner.next += 1;
        let id = inner.next;
        inner.names.insert(id, span.metadata().name());
        Id::from_u64(id)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, span: &Id) {
        let mut inner = self.inner.lock().unwrap();
        let id = span.into_u64();
        let name = inner.names.remove(&id).unwrap_or("?");
        inner.open.push((id, name, Instant::now()));
    }
    fn exit(&self, span: &Id) {
        let mut inner = self.inner.lock().unwrap();
        let id = span.into_u64();
        let Some(at) = inner.open.iter().rposition(|o| o.0 == id) else {
            return;
        };
        let (_, name, start) = inner.open.remove(at);
        if inner.open.iter().any(|o| o.1 == name) {
            return;
        }
        *inner.tick.entry(name).or_default() += start.elapsed();
    }
}

struct Args {
    data: PathBuf,
    map: String,
    crowd: Option<u32>,
    ai: Option<usize>,
    ticks: u32,
    warmup: u32,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        data: PathBuf::from("."),
        map: String::new(),
        crowd: None,
        ai: None,
        ticks: 1600,
        warmup: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        let number = |v: String| v.parse::<u32>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--data" => out.data = value()?.into(),
            "--map" => out.map = value()?,
            "--crowd" => out.crowd = Some(number(value()?)?),
            "--ai" => out.ai = Some(number(value()?)? as usize),
            "--ticks" => out.ticks = number(value()?)?,
            "--warmup" => out.warmup = number(value()?)?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if out.map.is_empty() {
        return Err("--map <id> is required".into());
    }
    Ok(out)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// FNV-1a over bytes, continuing from `h`.
fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn main() -> Result<(), String> {
    let args = args()?;
    let content = Arc::new(Content::load(&args.data).map_err(|e| format!("content: {e}"))?);
    let library = map::library(&args.data).map_err(|e| format!("maps: {e}"))?;
    let file = library
        .iter()
        .find(|m| m.id == args.map)
        .ok_or(format!("no map {}", args.map))?;
    let mut map = file.load().map_err(|e| format!("{}: {e}", file.file))?;
    let setup = match (args.crowd, args.ai) {
        (Some(n), _) => canopy_sim::crowd::crowd(&mut map, &content, n)?,
        (None, players) => MatchSetup {
            seed: 1,
            slots: map
                .player_starts
                .iter()
                .take(players.unwrap_or(2))
                .map(|start| Slot {
                    player: start.player.saturating_sub(1),
                    kind: SlotKind::Ai,
                    name: None,
                    team: None,
                    hero: None,
                    race: None,
                })
                .collect(),
        },
    };
    let started = Instant::now();
    let mut world = World::new(content, &map, setup).map_err(|e| e.to_string())?;
    let load = started.elapsed();

    let spans: &'static Spans = Box::leak(Box::default());
    let _guard = tracing::subscriber::set_default(spans);
    for _ in 0..args.warmup {
        world.step();
    }
    spans.inner.lock().unwrap().tick.clear();

    let mut ticks: Vec<f64> = Vec::new();
    let mut views: Vec<f64> = Vec::new();
    let mut per_step: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    let mut worst = (0.0, 0);
    let mut digest = 0xcbf2_9ce4_8422_2325;
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    for i in 0..args.ticks {
        let started = Instant::now();
        world.step();
        let stepped = Instant::now();
        let view = world.observer_view();
        let viewed = Instant::now();
        std::hint::black_box(&view);
        drop(view);
        digest = fnv(digest, &world.fingerprint().to_le_bytes());
        let tick = ms(stepped - started);
        if tick > worst.0 {
            worst = (tick, world.tick());
        }
        ticks.push(tick);
        views.push(ms(viewed - stepped));
        let mut inner = spans.inner.lock().unwrap();
        for (name, d) in std::mem::take(&mut inner.tick) {
            let costs = per_step.entry(name).or_default();
            costs.resize(i as usize, 0.0);
            costs.push(ms(d));
        }
    }
    let save = serde_json::to_vec(&world.save()).map_err(|e| e.to_string())?;
    digest = fnv(digest, &save);

    let n = ticks.len();
    let units = world
        .state()
        .entities
        .iter()
        .filter(|e| e.unit.is_some() && e.hp.is_some_and(|hp| hp > 0))
        .count();
    println!(
        "{} {}: {n} ticks after {} warm-up (world built in {:.0} ms), {units} living units at the end",
        args.map,
        match (args.crowd, args.ai) {
            (Some(c), _) => format!("crowd 8 × {c}"),
            (None, p) => format!("{} AI players", p.unwrap_or(2)),
        },
        args.warmup,
        ms(load),
    );
    let report = |name: &str, costs: &[f64]| {
        let mut sorted = costs.to_vec();
        sorted.resize(n, 0.0);
        sorted.sort_by(f64::total_cmp);
        let mean = sorted.iter().sum::<f64>() / n.max(1) as f64;
        println!(
            "  {name:<26} mean {mean:7.3}  p50 {:7.3}  p99 {:7.3}  max {:7.3}",
            percentile(&sorted, 0.5),
            percentile(&sorted, 0.99),
            sorted.last().copied().unwrap_or(0.0),
        );
        mean
    };
    println!("whole tick (ms):");
    report("step", &ticks);
    report("observer view", &views);
    println!("  worst step at tick {} ({:.2} ms)", worst.1, worst.0);
    println!("tick steps (ms per tick):");
    for (name, costs) in per_step.iter().filter(|(k, _)| k.starts_with("tick: ")) {
        report(&name["tick: ".len()..], costs);
    }
    println!("inner spans, costliest first (ms per tick):");
    let mut inner: Vec<_> = per_step
        .iter()
        .filter(|(k, _)| !k.starts_with("tick: "))
        .map(|(k, v)| (v.iter().sum::<f64>() / n.max(1) as f64, *k, v))
        .collect();
    inner.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, name, costs) in inner.iter().take(16) {
        report(name, costs);
    }
    println!("digest {digest:016x}");
    Ok(())
}
