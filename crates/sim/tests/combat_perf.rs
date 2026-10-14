//! Combat cost per tick. Measurements, not behaviour tests; run them in release:
//!
//! `cargo test --release -p canopy-sim --test combat_perf -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // wall-clock timing of the benchmark itself

mod combat_fixture;

use canopy_sim::World;
use combat_fixture::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::{
    Event, Metadata, Subscriber,
    span::{Attributes, Id, Record},
};

/// Sums the time spent in each named span.
#[derive(Default)]
struct Spans {
    next: AtomicU64,
    names: Mutex<BTreeMap<u64, &'static str>>,
    open: Mutex<BTreeMap<u64, Instant>>,
    totals: Mutex<BTreeMap<&'static str, Duration>>,
}

impl Subscriber for &'static Spans {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.names
            .lock()
            .unwrap()
            .insert(id, span.metadata().name());
        Id::from_u64(id)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, span: &Id) {
        self.open
            .lock()
            .unwrap()
            .insert(span.into_u64(), Instant::now());
    }
    fn exit(&self, span: &Id) {
        let id = span.into_u64();
        let Some(start) = self.open.lock().unwrap().remove(&id) else {
            return;
        };
        let name = self.names.lock().unwrap().remove(&id).unwrap_or("?");
        *self.totals.lock().unwrap().entry(name).or_default() += start.elapsed();
    }
}

const COMBAT: [&str; 4] = [
    "garrisons",
    "combat planning",
    "combat resolution",
    "deaths",
];

fn measure(name: &str, world: &mut World, ticks: u32) {
    let spans: &'static Spans = Box::leak(Box::default());
    let mut combat = Vec::new();
    let mut total = Vec::new();
    let mut alive = 0;
    let count = |world: &World| {
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.unit.is_some() && e.hp.is_some_and(|hp| hp > 0))
            .count()
    };
    let start_units = count(world);
    let mut crowded = 0;
    tracing::subscriber::with_default(spans, || {
        for _ in 0..ticks {
            if count(world) * 2 >= start_units {
                crowded += 1;
            }
            spans.totals.lock().unwrap().clear();
            let start = Instant::now();
            world.step();
            total.push(start.elapsed().as_secs_f64() * 1000.0);
            let totals = spans.totals.lock().unwrap();
            combat.push(
                COMBAT
                    .iter()
                    .filter_map(|n| totals.get(n))
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .sum::<f64>(),
            );
        }
        alive = count(world);
    });
    let stats = |samples: &[f64]| {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let at = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        format!(
            "mean {mean:.3} ms, p50 {:.3}, p99 {:.3}, max {:.3}",
            at(0.5),
            at(0.99),
            at(1.0)
        )
    };
    println!(
        "{name}, first {crowded} ticks (at least half the units alive): combat {} | whole tick {}",
        stats(&combat[..crowded]),
        stats(&total[..crowded])
    );
    println!(
        "{name}, all {ticks} ticks ({alive} of {start_units} units left): combat {} | whole tick {}",
        stats(&combat),
        stats(&total)
    );
}

const KINDS: [&str; 5] = [
    "unit.swordsman",
    "unit.knight",
    "unit.bowman",
    "unit.lancer",
    "unit.mortar",
];

fn order(world: &mut World, player: u8, actors: Vec<u32>, to: (u32, u32)) {
    for chunk in actors.chunks(160) {
        command(
            world,
            player,
            json!({"type": "move", "actors": chunk, "destination": {"x": to.0, "y": to.1}, "attackMove": true}),
        );
    }
}

#[test]
#[ignore = "benchmark"]
fn eight_armies_of_fifty_in_one_area() {
    let mut entities: Vec<Value> = Vec::new();
    let mut names: Vec<Vec<String>> = vec![Vec::new(); 8];
    for player in 0..8u8 {
        // Clusters on a ring of radius 30 around the centre.
        let (cx, cy) = [
            (158, 128),
            (149, 149),
            (128, 158),
            (107, 149),
            (98, 128),
            (107, 107),
            (128, 98),
            (149, 107),
        ][usize::from(player)];
        for i in 0..50u32 {
            let (dx, dy) = ((i % 7) as i32 * 2 - 6, (i / 7) as i32 * 2 - 7);
            let name = format!("p{player}u{i:02}");
            entities.push(placed(
                &name,
                KINDS[(i as usize + usize::from(player)) % KINDS.len()],
                f64::from(cx + dx),
                f64::from(cy + dy),
                player + 1,
            ));
            names[usize::from(player)].push(name);
        }
    }
    let map = sandbox(entities);
    let mut world = World::new(content(), &map, free_for_all(8)).unwrap();
    for (player, names) in names.iter().enumerate() {
        let actors = names.iter().map(|n| id(&world, n)).collect();
        order(&mut world, player as u8 + 1, actors, (128, 128));
    }
    measure("8 × 50 units, one area", &mut world, 1200);
}

#[test]
#[ignore = "benchmark"]
fn two_armies_of_a_hundred() {
    let mut entities: Vec<Value> = Vec::new();
    let mut names: [Vec<String>; 2] = [Vec::new(), Vec::new()];
    for side in 0..2u8 {
        for i in 0..100u32 {
            let (row, col) = (i / 20, i % 20);
            let x = if side == 0 {
                90 - row as i32 * 2
            } else {
                166 + row as i32 * 2
            };
            let name = format!("s{side}u{i:03}");
            entities.push(placed(
                &name,
                KINDS[(i as usize * 3 + usize::from(side)) % KINDS.len()],
                f64::from(x),
                f64::from(108 + col as i32 * 2),
                side + 1,
            ));
            names[usize::from(side)].push(name);
        }
    }
    let map = sandbox(entities);
    let mut world = World::new(content(), &map, free_for_all(2)).unwrap();
    for (side, names) in names.iter().enumerate() {
        let actors = names.iter().map(|n| id(&world, n)).collect();
        order(
            &mut world,
            side as u8 + 1,
            actors,
            (if side == 0 { 170 } else { 86 }, 128),
        );
    }
    measure("2 × 100 units", &mut world, 1500);
}
