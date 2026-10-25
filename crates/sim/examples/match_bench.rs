//! Simulation cost of a real match, per tick and per tick step (`just bench-sim`).
//!
//! ```text
//! cargo run --release -p canopy-sim --example match_bench -- --data . --map frozen-dream \
//!     [--crowd 50 | --ai 2] [--ticks 1600] [--warmup 0] [--repeat 1] [--no-spans] [--top 16] \
//!     [--worst 1] [--save <file>]
//! ```
//!
//! `--crowd N` is the measured battle of `canopy shot --crowd N` (eight players with N
//! soldiers each, `canopy_sim::crowd`); `--ai N` puts computer players on the first N
//! starts instead. Reports the whole tick (step and observer view) as mean, p50, p99 and
//! max, the `tick: …` step spans the world marks, and the costliest inner spans; `--worst N`
//! lists the N costliest ticks with what made them. The
//! span subscriber costs a little; `--no-spans` times whole ticks without it. The
//! digest at the end covers every tick's fingerprint and the final saved state: equal
//! digests mean identical simulations. `--save <file>` writes that final state as JSON and
//! the report adds the fingerprints' digest alone, so a content migration that only
//! renames references can show identical play and diff the saved state. `--repeat N` plays
//! the same match N times and keeps each tick's (and each span's) cheapest run, which
//! filters out the noise of a machine busy with other work; every run must digest equal.
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
    /// Time the whole tick only, without the span subscriber's overhead.
    no_spans: bool,
    /// Inner spans listed.
    top: usize,
    save: Option<PathBuf>,
    /// Runs of the same match; each tick keeps its cheapest.
    repeat: u32,
    /// Costliest ticks listed with their spans.
    worst: usize,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        data: PathBuf::from("."),
        map: String::new(),
        crowd: None,
        ai: None,
        ticks: 1600,
        warmup: 0,
        no_spans: false,
        top: 16,
        save: None,
        repeat: 1,
        worst: 1,
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
            "--no-spans" => out.no_spans = true,
            "--top" => out.top = number(value()?)? as usize,
            "--save" => out.save = Some(value()?.into()),
            "--repeat" => out.repeat = number(value()?)?.max(1),
            "--worst" => out.worst = number(value()?)? as usize,
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

/// One run of the match: its per-tick costs and digests.
struct Run {
    world: World,
    load: Duration,
    ticks: Vec<f64>,
    views: Vec<f64>,
    per_step: BTreeMap<&'static str, Vec<f64>>,
    ticks_digest: u64,
    digest: u64,
}

impl Run {
    /// Each tick's and each span's cheaper cost of two runs of the same match.
    fn cheapest(mut self, other: Run) -> Result<Run, String> {
        if other.digest != self.digest {
            return Err(format!(
                "runs differ: digest {:016x} then {:016x}",
                self.digest, other.digest
            ));
        }
        let min = |a: &mut Vec<f64>, b: &[f64]| {
            a.resize(a.len().max(b.len()), 0.0);
            for (i, x) in a.iter_mut().enumerate() {
                *x = x.min(b.get(i).copied().unwrap_or(0.0));
            }
        };
        min(&mut self.ticks, &other.ticks);
        min(&mut self.views, &other.views);
        for (name, mine) in self.per_step.iter_mut() {
            min(mine, other.per_step.get(name).map_or(&[], Vec::as_slice));
        }
        self.load = self.load.min(other.load);
        Ok(self)
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Plays the match once from a new world.
fn play(
    args: &Args,
    content: &Arc<Content>,
    map: &map::UtcMap,
    setup: &MatchSetup,
    spans: &'static Spans,
) -> Result<Run, String> {
    let started = Instant::now();
    let mut world = World::new(content.clone(), map, setup.clone()).map_err(|e| e.to_string())?;
    let load = started.elapsed();
    for _ in 0..args.warmup {
        world.step();
    }
    spans.inner.lock().unwrap().tick.clear();

    let mut ticks: Vec<f64> = Vec::new();
    let mut views: Vec<f64> = Vec::new();
    let mut per_step: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    let mut digest = 0xcbf2_9ce4_8422_2325;
    for i in 0..args.ticks {
        let started = Instant::now();
        world.step();
        let stepped = Instant::now();
        let view = world.observer_view();
        let viewed = Instant::now();
        std::hint::black_box(&view);
        drop(view);
        digest = fnv(digest, &world.fingerprint().to_le_bytes());
        ticks.push(ms(stepped - started));
        views.push(ms(viewed - stepped));
        let mut inner = spans.inner.lock().unwrap();
        for (name, d) in std::mem::take(&mut inner.tick) {
            let costs = per_step.entry(name).or_default();
            costs.resize(i as usize, 0.0);
            costs.push(ms(d));
        }
    }
    let ticks_digest = digest;
    let save = serde_json::to_vec(&world.save()).map_err(|e| e.to_string())?;
    digest = fnv(digest, &save);
    Ok(Run {
        world,
        load,
        ticks,
        views,
        per_step,
        ticks_digest,
        digest,
    })
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
    let spans: &'static Spans = Box::leak(Box::default());
    let _guard = (!args.no_spans).then(|| tracing::subscriber::set_default(spans));
    let mut best: Option<Run> = None;
    for _ in 0..args.repeat {
        let run = play(&args, &content, &map, &setup, spans)?;
        best = Some(match best {
            None => run,
            Some(best) => best.cheapest(run)?,
        });
    }
    let Run {
        world,
        load,
        ticks,
        views,
        per_step,
        ticks_digest,
        digest,
    } = best.expect("at least one run");
    if let Some(path) = &args.save {
        let pretty = serde_json::to_vec_pretty(&world.save()).map_err(|e| e.to_string())?;
        std::fs::write(path, pretty).map_err(|e| format!("{}: {e}", path.display()))?;
    }

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
    // The costliest ticks, costliest first, with the steps and inner spans that made them.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| ticks[b].total_cmp(&ticks[a]).then(a.cmp(&b)));
    for &index in order.iter().take(args.worst) {
        let spans_at = |steps: bool| {
            let mut at: Vec<(f64, &str)> = per_step
                .iter()
                .filter(|(k, _)| k.starts_with("tick: ") == steps)
                .filter_map(|(k, v)| {
                    v.get(index)
                        .map(|&c| (c, k.strip_prefix("tick: ").unwrap_or(k)))
                })
                .filter(|(c, _)| *c >= 0.05)
                .collect();
            at.sort_by(|a, b| b.0.total_cmp(&a.0));
            at.iter()
                .map(|(c, k)| format!("{k} {c:.2}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!(
            "  worst step at tick {} ({:.2} ms): {}",
            args.warmup + index as u32 + 1,
            ticks[index],
            spans_at(true)
        );
        println!("    inner: {}", spans_at(false));
    }
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
    for (_, name, costs) in inner.iter().take(args.top) {
        report(name, costs);
    }
    println!("fingerprints {ticks_digest:016x}");
    println!("digest {digest:016x}");
    Ok(())
}
