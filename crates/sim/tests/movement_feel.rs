//! How movement feels, measured: groups crossing open ground, squeezing through a gap,
//! crossing each other, converging on one point and walking through idle allies. Each
//! scenario reports when the group arrived, how long units stood blocked while ordered to
//! move, how often a unit's direction jerked, how far units walked against the straight
//! distance, how deep bodies overlapped and how much units kept moving after arriving.
//! Measurements, not behaviour tests:
//!
//! `cargo test --release -p canopy-sim --test movement_feel -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // facing measured with platform trigonometry, not simulated

mod movement_fixture;

use canopy_sim::World;
use movement_fixture::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// One group order: placements and where they go.
struct Order {
    ids: Vec<String>,
    to: (i32, i32),
}

struct Scenario {
    name: &'static str,
    units: Vec<Value>,
    terrain: Value,
    orders: Vec<Order>,
    /// Placements that must stay idle (walked through).
    idle: Vec<String>,
}

/// A `cols × rows` block of `definition` at `(x, y)`, `gap` cells apart, named `prefix-n`.
fn block(
    prefix: &str,
    definition: &str,
    x: i32,
    y: i32,
    cols: i32,
    rows: i32,
    gap: i32,
) -> (Vec<Value>, Vec<String>) {
    let mut units = Vec::new();
    let mut ids = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let id = format!("{prefix}-{}", r * cols + c);
            units.push(unit(&id, definition, x + c * gap, y + r * gap));
            ids.push(id);
        }
    }
    (units, ids)
}

fn scenarios() -> Vec<Scenario> {
    let mut out = Vec::new();
    // 25 walkers cross open ground.
    let (units, ids) = block("w", "unit.walker", 40, 36, 5, 5, 2);
    out.push(Scenario {
        name: "open",
        units,
        terrain: flat(),
        orders: vec![Order { ids, to: (100, 40) }],
        idle: vec![],
    });
    // The same group through a 4-cell gap in a wall.
    let (units, ids) = block("w", "unit.walker", 40, 36, 5, 5, 2);
    let wall = walls(|x, y| x == 70 && !(38..=41).contains(&y));
    out.push(Scenario {
        name: "choke",
        units,
        terrain: wall,
        orders: vec![Order { ids, to: (100, 40) }],
        idle: vec![],
    });
    // Two groups pass each other head on.
    let (mut units, a) = block("a", "unit.walker", 40, 36, 4, 4, 2);
    let (b_units, b) = block("b", "unit.walker", 94, 38, 4, 4, 2);
    units.extend(b_units);
    out.push(Scenario {
        name: "cross",
        units,
        terrain: flat(),
        orders: vec![
            Order {
                ids: a,
                to: (100, 40),
            },
            Order {
                ids: b,
                to: (40, 42),
            },
        ],
        idle: vec![],
    });
    // A spread group converges on one point.
    let (units, ids) = block("w", "unit.walker", 30, 20, 6, 5, 6);
    out.push(Scenario {
        name: "converge",
        units,
        terrain: flat(),
        orders: vec![Order { ids, to: (90, 60) }],
        idle: vec![],
    });
    // A group walks through a line of idle allies.
    let (mut units, ids) = block("w", "unit.walker", 40, 36, 4, 4, 2);
    let (line, idle) = block("i", "unit.walker", 66, 32, 1, 10, 2);
    units.extend(line);
    out.push(Scenario {
        name: "through",
        units,
        terrain: flat(),
        orders: vec![Order { ids, to: (96, 40) }],
        idle,
    });
    // Big units through the gap.
    let (units, ids) = block("t", "unit.trooper", 36, 30, 3, 3, 5);
    let wall = walls(|x, y| x == 70 && !(36..=44).contains(&y));
    out.push(Scenario {
        name: "big-choke",
        units,
        terrain: wall,
        orders: vec![Order { ids, to: (104, 40) }],
        idle: vec![],
    });
    out
}

#[derive(Default, Debug)]
struct Metrics {
    arrived_at: Option<u32>,
    stalled: u32,
    jerks: u32,
    walked: f64,
    straight: f64,
    max_overlap: f64,
    after_arrival: f64,
    not_arrived: usize,
    pushed: f64,
    /// Ticks a unit moved more than 90° away from where it faces.
    backwards: u32,
}

fn moving(world: &World, id: u32) -> bool {
    let e = get(world, canopy_sim::entity::EntityId(id));
    e.unit
        .as_ref()
        .is_some_and(|u| !u.route.is_empty() || u.order.is_some())
}

fn pos(world: &World, id: u32) -> (f64, f64) {
    let e = get(world, canopy_sim::entity::EntityId(id));
    (
        f64::from(e.position.x) / 1000.0,
        f64::from(e.position.y) / 1000.0,
    )
}

fn radius(world: &World, id: u32) -> f64 {
    let e = get(world, canopy_sim::entity::EntityId(id));
    world
        .catalog()
        .content
        .definition(world.catalog().get(e.definition).id.as_str())
        .and_then(|d| d.dimensions.as_ref())
        .map_or(0.4, |d| d.radius)
}

fn measure(s: &Scenario, limit: u32) -> Metrics {
    let map = map(s.units.clone(), s.terrain.clone(), vec![]);
    let mut world = world(&map);
    run(&mut world, 2);
    let mut ordered: Vec<u32> = Vec::new();
    let mut goal: BTreeMap<u32, (f64, f64)> = BTreeMap::new();
    for o in &s.orders {
        let ids: Vec<u32> = o.ids.iter().map(|p| id(&world, p)).collect();
        for &i in &ids {
            goal.insert(i, (f64::from(o.to.0), f64::from(o.to.1)));
        }
        ordered.extend(&ids);
        command(
            &mut world,
            json!({"type": "move", "actors": ids, "destination": {"x": o.to.0, "y": o.to.1}}),
        );
    }
    let idle: Vec<u32> = s.idle.iter().map(|p| id(&world, p)).collect();
    let idle_start: Vec<(f64, f64)> = idle.iter().map(|&i| pos(&world, i)).collect();
    let all: Vec<u32> = ordered.iter().chain(&idle).copied().collect();
    let radii: BTreeMap<u32, f64> = all.iter().map(|&i| (i, radius(&world, i))).collect();
    let mut m = Metrics::default();
    for &i in &ordered {
        let (p, g) = (pos(&world, i), goal[&i]);
        m.straight += ((p.0 - g.0).powi(2) + (p.1 - g.1).powi(2)).sqrt();
    }
    let mut last: BTreeMap<u32, (f64, f64)> = all.iter().map(|&i| (i, pos(&world, i))).collect();
    let mut heading: BTreeMap<u32, (f64, f64)> = BTreeMap::new();
    let mut done: BTreeMap<u32, bool> = BTreeMap::new();
    world.step();
    for tick in 0..limit {
        world.step();
        for &i in &ordered {
            let p = pos(&world, i);
            let q = last[&i];
            let (dx, dy) = (p.0 - q.0, p.1 - q.1);
            let d = (dx * dx + dy * dy).sqrt();
            let busy = moving(&world, i);
            if busy {
                m.walked += d;
                if d < 1e-6 {
                    m.stalled += 1;
                }
                if d > 1e-3 {
                    let h = (dx / d, dy / d);
                    let r =
                        f64::from(get(&world, canopy_sim::entity::EntityId(i)).rotation) / 1000.0;
                    let facing = (r.to_radians().sin(), r.to_radians().cos());
                    if facing.0 * h.0 + facing.1 * h.1 < 0.0 {
                        m.backwards += 1;
                    }
                    if let Some(prev) = heading.get(&i)
                        && prev.0 * h.0 + prev.1 * h.1 < 0.5
                    {
                        m.jerks += 1;
                    }
                    heading.insert(i, h);
                }
            } else if *done.get(&i).unwrap_or(&false) {
                m.after_arrival += d;
            }
            if !busy {
                done.insert(i, true);
            }
            last.insert(i, p);
        }
        for &i in &idle {
            last.insert(i, pos(&world, i));
        }
        // Deepest overlap between two bodies, as a share of their radii.
        let ps: Vec<(u32, (f64, f64))> = all.iter().map(|&i| (i, pos(&world, i))).collect();
        for (a, &(ia, pa)) in ps.iter().enumerate() {
            for &(ib, pb) in &ps[a + 1..] {
                let r = radii[&ia] + radii[&ib];
                let d = ((pa.0 - pb.0).powi(2) + (pa.1 - pb.1).powi(2)).sqrt();
                if d < r {
                    m.max_overlap = m.max_overlap.max((r - d) / r);
                }
            }
        }
        if std::env::var_os("FEEL_TRACE").is_some() && tick >= limit - 12 {
            for &i in ordered.iter().filter(|&&i| moving(&world, i)) {
                let e = get(&world, canopy_sim::entity::EntityId(i));
                let u = e.unit.as_ref().unwrap();
                let next = u.route.first().map(|&n| (n % 256, n / 256));
                println!(
                    "  t{tick} {} at {:?} next {:?} rot {}",
                    i,
                    pos(&world, i),
                    next,
                    e.rotation
                );
            }
        }
        if m.arrived_at.is_none() && ordered.iter().all(|&i| !moving(&world, i)) {
            m.arrived_at = Some(tick + 1);
        }
    }
    m.not_arrived = ordered.iter().filter(|&&i| moving(&world, i)).count();
    if std::env::var_os("FEEL_TRACE").is_some() {
        for &i in ordered.iter().filter(|&&i| moving(&world, i)) {
            let e = get(&world, canopy_sim::entity::EntityId(i));
            let u = e.unit.as_ref().unwrap();
            println!(
                "  {} stuck at {:?} goal {:?} route {} last moved {:?} order {:?}",
                s.name,
                pos(&world, i),
                goal[&i],
                u.route.len(),
                u.last_moved_tick,
                u.order.is_some()
            );
        }
    }
    for (k, &i) in idle.iter().enumerate() {
        let (p, q) = (pos(&world, i), idle_start[k]);
        m.pushed += ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
    }
    m
}

#[test]
#[ignore = "measurement; run in release with --ignored --nocapture"]
fn movement_feel() {
    println!(
        "{:<10} {:>8} {:>8} {:>6} {:>7} {:>8} {:>7} {:>6} {:>7} {:>5}",
        "scenario",
        "arrived",
        "stalled",
        "jerks",
        "walked",
        "overlap",
        "after",
        "stuck",
        "pushed",
        "back"
    );
    for s in scenarios() {
        let m = measure(&s, 1200);
        println!(
            "{:<10} {:>8} {:>8} {:>6} {:>6.2}x {:>7.0}% {:>7.2} {:>6} {:>7.1} {:>5}",
            s.name,
            m.arrived_at
                .map_or("-".to_string(), |t| format!("{:.1}s", f64::from(t) / 40.0)),
            m.stalled,
            m.jerks,
            m.walked / m.straight.max(1e-9),
            m.max_overlap * 100.0,
            m.after_arrival,
            m.not_arrived,
            m.pushed,
            m.backwards,
        );
    }
}
