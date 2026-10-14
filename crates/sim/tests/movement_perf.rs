//! Movement and path-finding cost. Measurements, not behaviour tests; run them in release:
//!
//! `cargo test --release -p canopy-sim --test movement_perf -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // wall-clock timing of the benchmark itself

mod movement_fixture;

use canopy_content::{
    Content,
    conditions::Locomotion,
    map,
    placement::{Owner, Placement, Point},
};
use canopy_sim::{
    Action, MatchSetup, Queued, Slot, SlotKind, World,
    action::CellPoint,
    catalog::Catalog,
    spatial::{Body, Spot},
    spawn,
};
use std::{path::Path, sync::Arc, time::Instant};

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn report(name: &str, samples: &[f64]) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    println!(
        "{name}: mean {mean:.3} ms, p50 {:.3}, p99 {:.3}, max {:.3} ({} samples)",
        at(0.5),
        at(0.99),
        at(1.0),
        samples.len()
    );
}

/// A simple deterministic generator for sample positions.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % u64::from(bound)) as u32
    }
}

/// Free cells for `count` bodies spaced `step` apart in a square spiral around `centre`.
fn free_cells(
    world: &World,
    body: &Body,
    centre: (i32, i32),
    step: i32,
    count: usize,
    taken: &mut Vec<(i32, i32)>,
) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for r in 0i32..200 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r || out.len() == count {
                    continue;
                }
                let (x, y) = (centre.0 + dx * step, centre.1 + dy * step);
                let spot = Spot::new(x, y, None);
                let clear = taken
                    .iter()
                    .all(|&(tx, ty)| (tx - x).pow(2) + (ty - y).pow(2) >= step * step);
                if clear
                    && world.spatial().valid_spot(spot)
                    && world.spatial().unit_walkable(spot, body)
                {
                    out.push((x, y));
                    taken.push((x, y));
                }
            }
        }
    }
    out
}

#[test]
#[ignore = "benchmark; run in release with --ignored --nocapture"]
fn four_hundred_units_cross_the_largest_shipped_map() {
    let content = Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")));
    let maps = map::library(data()).unwrap_or_else(|errors| panic!("{errors}"));
    let map = maps
        .iter()
        .map(|file| file.load().unwrap_or_else(|errors| panic!("{errors}")))
        .max_by_key(|m| (m.size, m.authoring.objects.len()))
        .expect("a shipped map");
    let players = 4u8;
    let setup = MatchSetup {
        seed: 3,
        slots: (0..players)
            .map(|player| Slot {
                player,
                kind: SlotKind::Human,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    };
    let created = Instant::now();
    let mut world = World::new(content.clone(), &map, setup).unwrap_or_else(|e| panic!("{e}"));
    println!(
        "{}-cell map: world created in {:.1} ms (spatial model and meshes included)",
        map.size,
        created.elapsed().as_secs_f64() * 1000.0
    );
    let catalog = Catalog::new(content.clone());
    // The body of the default starting units.
    let walker = content
        .rules
        .starting_setup
        .units
        .iter()
        .filter_map(|u| content.definition(u.definition.as_str()))
        .find(|d| {
            d.behaviors
                .movement
                .as_ref()
                .is_some_and(|m| m.speed > 0.0 && m.locomotion != Some(Locomotion::Air))
        })
        .expect("a ground starting unit");
    let def = catalog.find(walker.id.as_str()).unwrap();
    let body = world.spatial().profiles.get(def).body;
    let size = world.terrain().size() as i32;
    let corners = [
        (size / 4, size / 4),
        (3 * size / 4, 3 * size / 4),
        (3 * size / 4, size / 4),
        (size / 4, 3 * size / 4),
    ];
    let mut taken = Vec::new();
    let mut groups = Vec::new();
    for (player, &corner) in corners.iter().enumerate() {
        let cells = free_cells(
            &world,
            &body,
            corner,
            (body.spacing.ceil() as i32).max(1),
            100,
            &mut taken,
        );
        let mut ids = Vec::new();
        for (i, (x, y)) in cells.into_iter().enumerate() {
            let placement = Placement {
                activation: None,
                id: format!("bench.{player}.{i}"),
                map_knowledge: None,
                definition: walker.id.clone(),
                position: Point {
                    x: f64::from(x),
                    y: f64::from(y),
                    surface: None,
                },
                rotation: 0.0,
                owner: Owner::Player(player as u8),
                initial_state: None,
                appearance: None,
            };
            ids.push(
                spawn::create(world.state_mut(), &catalog, &placement)
                    .unwrap()
                    .0,
            );
        }
        groups.push(ids);
    }
    // Warm the mesh of this body, as matches with such units already have it.
    let warm = Instant::now();
    let a = world
        .spatial()
        .node(Spot::new(taken[0].0, taken[0].1, None))
        .unwrap();
    let b = world
        .spatial()
        .node(Spot::new(
            taken[taken.len() - 1].0,
            taken[taken.len() - 1].1,
            None,
        ))
        .unwrap();
    world
        .spatial_mut()
        .find_path(a, b, None, i64::MAX, &body, None);
    println!(
        "{} ({} units of radius {}): mesh build and first query {:.1} ms",
        walker.id,
        taken.len(),
        body.radius,
        warm.elapsed().as_secs_f64() * 1000.0
    );
    for (player, ids) in groups.iter().enumerate() {
        let target = corners[(player + 1) % 4];
        for chunk in ids.chunks(canopy_sim::action::MAX_ACTORS) {
            let action = Action::Move {
                actors: chunk.to_vec(),
                destination: CellPoint {
                    x: target.0 as u32,
                    y: target.1 as u32,
                    surface: None,
                },
                attack_move: None,
                append: None,
            };
            world
                .enqueue(Queued {
                    tick: 0,
                    player: player as u8,
                    seq: 0,
                    action,
                })
                .unwrap();
        }
    }
    let mut samples = Vec::new();
    for _ in 0..2400 {
        let start = Instant::now();
        world.step();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    println!("first tick (orders planned) {:.2} ms", samples[0]);
    report("step (movement, vision and everything else)", &samples[1..]);
    let arrived = world
        .state()
        .entities
        .iter()
        .filter(|e| {
            e.placement
                .as_deref()
                .is_some_and(|p| p.starts_with("bench."))
        })
        .filter(|e| e.unit.as_ref().is_some_and(|u| u.order.is_none()))
        .count();
    println!("{arrived} of {} units arrived", taken.len());
    // Path queries for this body between random walkable cells.
    let mut rng = Lcg(7);
    let (mut long, mut short) = (Vec::new(), Vec::new());
    while long.len() < 200 || short.len() < 200 {
        let (x, y) = (rng.next(size as u32) as i32, rng.next(size as u32) as i32);
        let (dx, dy) = (rng.next(81) as i32 - 40, rng.next(81) as i32 - 40);
        let near = short.len() < 200 && long.len() >= short.len();
        let (tx, ty) = if near {
            (x + dx, y + dy)
        } else {
            (rng.next(size as u32) as i32, rng.next(size as u32) as i32)
        };
        let (from, to) = (Spot::new(x, y, None), Spot::new(tx, ty, None));
        if !world.spatial().valid_spot(to)
            || !world.spatial().unit_walkable(from, &body)
            || !world.spatial().unit_walkable(to, &body)
        {
            continue;
        }
        let (s, g) = (
            world.spatial().node(from).unwrap(),
            world.spatial().node(to).unwrap(),
        );
        let start = Instant::now();
        world
            .spatial_mut()
            .find_path(s, g, None, i64::MAX, &body, None);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if near { short.push(ms) } else { long.push(ms) }
    }
    report("path query, random pairs across the map", &long);
    report("path query, pairs within 40 cells", &short);
    println!("routing counters {:?}", world.spatial().stats);
}

#[test]
#[ignore = "benchmark; run in release with --ignored --nocapture"]
fn a_hundred_units_through_a_chokepoint() {
    use movement_fixture::*;
    // A cliff wall at x = 128 with an eight-cell gap; troopers are 3 cells wide.
    let wall = |x: i32, y: i32| (127..=129).contains(&x) && !(124..=131).contains(&y);
    let units: Vec<_> = (0..100)
        .map(|i| {
            unit(
                &format!("w{i}"),
                "unit.trooper",
                40 + (i % 10) * 4,
                108 + (i / 10) * 4,
            )
        })
        .collect();
    let map = map(units, walls(wall), vec![]);
    let mut world = world(&map);
    let west: Vec<u32> = (0..100).map(|i| id(&world, &format!("w{i}"))).collect();
    command(
        &mut world,
        serde_json::json!({"type": "move", "actors": &west[..80], "destination": {"x": 200, "y": 128}}),
    );
    command(
        &mut world,
        serde_json::json!({"type": "move", "actors": &west[80..], "destination": {"x": 210, "y": 140}}),
    );
    let mut samples = Vec::new();
    for _ in 0..4000 {
        let start = Instant::now();
        world.step();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    println!("first tick (orders planned) {:.2} ms", samples[0]);
    report("step", &samples[1..]);
    let through = west
        .iter()
        .filter(|&&id| get(&world, canopy_sim::entity::EntityId(id)).position.x > 130_000)
        .count();
    println!("{through} of 100 passed the gap");
}
