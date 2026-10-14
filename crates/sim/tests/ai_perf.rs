//! AI cost: seven AI players on a shipped map (the four-start map with three more starts
//! added on open ground) over a 20,000-tick match. A measurement, not a behaviour test;
//! run it in release:
//!
//! `cargo test --release -p canopy-sim --test ai_perf -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // wall-clock timing of the benchmark itself

use canopy_content::{Content, map, map::PlayerStart};
use canopy_sim::{MatchSetup, Slot, SlotKind, World, catalog::Catalog, spawn, terrain::Terrain};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// Times spans by name: totals and every closed span's duration.
#[derive(Default)]
struct Timer {
    names: Mutex<Vec<&'static str>>,
    stack: Mutex<Vec<(u64, Instant)>>,
    samples: Mutex<BTreeMap<&'static str, Vec<f64>>>,
}

impl tracing::Subscriber for Timer {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut names = self.names.lock().unwrap();
        names.push(span.metadata().name());
        tracing::span::Id::from_u64(names.len() as u64)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, span: &tracing::span::Id) {
        self.stack
            .lock()
            .unwrap()
            .push((span.into_u64(), Instant::now()));
    }
    fn exit(&self, _: &tracing::span::Id) {
        let (id, start) = self.stack.lock().unwrap().pop().expect("entered");
        let name = self.names.lock().unwrap()[id as usize - 1];
        self.samples
            .lock()
            .unwrap()
            .entry(name)
            .or_default()
            .push(start.elapsed().as_secs_f64() * 1000.0);
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p) as usize]
}

/// Adds starts beside unclaimed authored resource deposits (so each new colony can earn
/// both goods), on valid open ground, until there are `count`.
fn add_starts(map: &mut map::UtcMap, catalog: &Catalog, terrain: &Terrain, count: usize) {
    let template = map.player_starts[0].clone();
    let deposits: Vec<(f64, f64)> = map
        .entities
        .iter()
        .filter(|p| {
            catalog
                .content
                .definition(p.definition.as_str())
                .is_some_and(|d| d.resource_yield.is_some())
        })
        .map(|p| (p.position.x, p.position.y))
        .collect();
    for &(dx, dy) in &deposits {
        if map.player_starts.len() >= count {
            break;
        }
        let claimed = map
            .player_starts
            .iter()
            .any(|s| (s.x - dx).hypot(s.z - dy) < 120.0);
        if claimed {
            continue;
        }
        'search: for r in [24.0f64, 28.0, 32.0, 36.0] {
            for k in 0..16 {
                let angle = f64::from(k) * std::f64::consts::PI / 8.0;
                let snap = |v: f64| (v / 4.0).round() * 4.0 + 1.5;
                let (cx, cz) = (snap(dx + angle.cos() * r), snap(dy + angle.sin() * r));
                if map
                    .camps
                    .iter()
                    .any(|c| (c.home.x - cx).hypot(c.home.y - cz) < 40.0)
                {
                    continue;
                }
                let player = map.player_starts.len() as u8 + 1;
                let mut candidate = map.clone();
                candidate.player_starts.push(PlayerStart {
                    player,
                    x: cx,
                    z: cz,
                    rotation: None,
                    setup: template.setup.clone(),
                    main_fort: format!("start.player.{player}/main-fort"),
                });
                let setup = setup(candidate.player_starts.len());
                let valid = spawn::placements(&candidate, catalog, terrain, &setup)
                    .and_then(|p| spawn::validate(catalog, terrain, &setup, &p.list));
                if valid.is_ok() {
                    *map = candidate;
                    break 'search;
                }
            }
        }
    }
    assert_eq!(map.player_starts.len(), count, "room for every start");
}

fn setup(players: usize) -> MatchSetup {
    MatchSetup {
        seed: 7,
        slots: (0..players as u8)
            .map(|player| Slot {
                player,
                kind: SlotKind::Ai,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

#[test]
#[ignore = "benchmark; run in release with --ignored --nocapture"]
fn seven_ai_players_over_twenty_thousand_ticks() {
    let content = Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")));
    let catalog = Catalog::new(content.clone());
    let mut map = map::library(data())
        .unwrap_or_else(|errors| panic!("{errors}"))
        .iter()
        .map(|file| file.load().unwrap_or_else(|errors| panic!("{errors}")))
        .max_by_key(|m| (m.player_starts.len(), m.size))
        .expect("a shipped map");
    let terrain = Terrain::compile(&map, &content).unwrap();
    add_starts(&mut map, &catalog, &terrain, 7);
    let starts: Vec<(f64, f64)> = map.player_starts.iter().map(|s| (s.x, s.z)).collect();
    println!("{}: starts {starts:?}", map.name);
    let timer = Arc::new(Timer::default());
    let dispatch = tracing::Dispatch::from(timer.clone());
    let started = Instant::now();
    let mut world = tracing::dispatcher::with_default(&dispatch, || {
        World::new(content.clone(), &map, setup(7)).unwrap_or_else(|e| panic!("{e}"))
    });
    println!(
        "world with 7 AIs created in {:.0} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    timer.samples.lock().unwrap().clear();
    let mut steps = Vec::new();
    let ticks = 20_000;
    tracing::dispatcher::with_default(&dispatch, || {
        for _ in 0..ticks {
            let t = Instant::now();
            world.step();
            steps.push(t.elapsed().as_secs_f64() * 1000.0);
            if world.state().outcome.is_some() {
                break;
            }
        }
    });
    let samples = timer.samples.lock().unwrap();
    let report = |name: &str, list: &[f64], per: usize| {
        let mut sorted = list.to_vec();
        sorted.sort_by(f64::total_cmp);
        let total: f64 = sorted.iter().sum();
        println!(
            "{name:<16} n {:>6}  total {total:>9.1} ms  mean/tick {:>7.4} ms  p50 {:>7.4}  p95 {:>7.4}  p99 {:>7.4}  max {:>7.3} ms",
            sorted.len(),
            total / per as f64,
            percentile(&sorted, 0.5),
            percentile(&sorted, 0.95),
            percentile(&sorted, 0.99),
            sorted.last().copied().unwrap_or(0.0)
        );
    };
    let ran = steps.len();
    report("step (all)", &steps, ran);
    for name in [
        "ai",
        "ai decision",
        "ai frame",
        "ai frame actors",
        "ai frame memory",
        "ai frame colony",
        "ai blockers",
        "ai economy",
        "ai operations",
        "ai safe point",
        "ai site check",
    ] {
        if let Some(list) = samples.get(name) {
            report(name, list, ran);
        }
    }
    for summary in world.ai_summaries() {
        println!(
            "player {}: {} | {} | {:?}",
            summary.player, summary.mission, summary.plan.economy, summary.metrics
        );
    }
    println!("ran {ran} ticks, outcome {:?}", world.state().outcome);
}
