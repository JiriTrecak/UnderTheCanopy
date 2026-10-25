//! Vision cost on the largest shipped map: 8 players with 50 moving sensors each, moved by
//! setting positions. A measurement, not a behaviour test; run it in release:
//!
//! `cargo test --release -p canopy-sim --test vision_perf -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // wall-clock timing of the benchmark itself

use canopy_content::{
    Content,
    conditions::Locomotion,
    game::Kind,
    map,
    placement::{Owner, Placement, Point},
};
use canopy_sim::{MatchSetup, Slot, SlotKind, World, catalog::Catalog, entity::Milli, math, spawn};
use std::{path::Path, sync::Arc, time::Instant};

const PLAYERS: u8 = 8;
const PER_PLAYER: usize = 50;
const TICKS: u32 = 400;

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn percentile(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[((samples.len() - 1) as f64 * p) as usize]
}

#[test]
#[ignore = "benchmark; run in release with --ignored --nocapture"]
fn vision_cost_with_400_moving_sensors() {
    let content = Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")));
    let maps = map::library(data()).unwrap_or_else(|errors| panic!("{errors}"));
    let map = maps
        .iter()
        .map(|file| file.load().unwrap_or_else(|errors| panic!("{errors}")))
        .max_by_key(|m| (m.size, m.authoring.objects.len()))
        .expect("a shipped map");
    let setup = MatchSetup {
        seed: 3,
        slots: (0..PLAYERS)
            .map(|player| Slot {
                difficulty: Default::default(),
                player,
                // Scripted players: AI players would add their own commands.
                kind: SlotKind::Human,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    };
    let mut world = World::new(content.clone(), &map, setup).unwrap_or_else(|e| panic!("{e}"));
    let catalog = Catalog::new(content.clone());
    let sensor = content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Unit && d.vision.unwrap_or(0) > 0)
        .find(|d| d.behaviors.movement.as_ref().and_then(|m| m.locomotion) != Some(Locomotion::Air))
        .expect("a ground unit with sight");
    let size = map.size as i32;
    let mut sensors = Vec::new();
    for player in 0..PLAYERS {
        for i in 0..PER_PLAYER {
            let n = usize::from(player) * PER_PLAYER + i;
            let placement = Placement {
                activation: None,
                id: format!("bench.{n}"),
                map_knowledge: None,
                definition: sensor.id.clone(),
                position: Point {
                    x: f64::from(40 + (n as i32 * 37) % (size - 80)),
                    y: f64::from(40 + (n as i32 * 53) % (size - 80)),
                    surface: None,
                },
                rotation: 0.0,
                owner: Owner::Player(player),
                initial_state: None,
                appearance: None,
            };
            let id = spawn::create(world.state_mut(), &catalog, &placement).unwrap();
            let p = world.state().entities.get(id).unwrap().position;
            sensors.push((id, p));
        }
    }
    world.step();
    let statics = world.state().entities.len();
    // Each sensor walks a circle of radius 12 cells at about 3 cells per second.
    let drive = |world: &mut World, tick: u32, speed: f64| {
        for (k, &(id, centre)) in sensors.iter().enumerate() {
            let angle = (f64::from(tick) * speed + k as f64) / 12.0;
            let at = Milli {
                x: centre.x + (math::cos(angle) * 12_000.0) as i32,
                y: centre.y + (math::sin(angle) * 12_000.0) as i32,
            };
            world.state_mut().entities.get_mut(id).unwrap().position = at;
        }
    };
    for (label, speed) in [
        ("walking (0.075 cells/tick)", 0.075),
        ("fast (1 cell/tick)", 1.0),
    ] {
        let mut ticks = Vec::new();
        for tick in 0..TICKS {
            drive(&mut world, tick, speed);
            let start = Instant::now();
            world.step();
            ticks.push(start.elapsed().as_secs_f64() * 1e6);
        }
        let mean = ticks.iter().sum::<f64>() / ticks.len() as f64;
        println!(
            "{label}: step (vision only) mean {mean:.1} µs, p50 {:.1} µs, p99 {:.1} µs, max {:.1} µs",
            percentile(&mut ticks, 0.5),
            percentile(&mut ticks, 0.99),
            percentile(&mut ticks, 1.0)
        );
    }
    let mut views = Vec::new();
    let mut entities = 0;
    for _ in 0..200 {
        let start = Instant::now();
        let view = world.view(Owner::Player(0));
        views.push(start.elapsed().as_secs_f64() * 1e6);
        entities = view.entities.len() + view.scenery.len();
    }
    let mut observer = Vec::new();
    for _ in 0..200 {
        let start = Instant::now();
        let view = world.observer_view();
        observer.push(start.elapsed().as_secs_f64() * 1e6);
        std::hint::black_box(view);
    }
    println!(
        "map {} ({}², {statics} entities); player view ({entities} entries) p50 {:.1} µs, p99 {:.1} µs; observer view p50 {:.1} µs",
        map.name,
        map.size,
        percentile(&mut views, 0.5),
        percentile(&mut views, 0.99),
        percentile(&mut observer, 0.5)
    );
    let start = Instant::now();
    let save = world.save();
    let text = serde_json::to_string(&save.state.knowledge).unwrap();
    println!(
        "knowledge save: {} KiB ({:.1} ms to serialize state)",
        text.len() / 1024,
        start.elapsed().as_secs_f64() * 1e3
    );
}
