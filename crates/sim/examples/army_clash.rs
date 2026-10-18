//! Two armies of shipped units meet on an open sandbox field and fight it out: who is
//! left, and what it cost. A balance probe for unit stats and abilities; not a test.
//!
//! ```text
//! cargo run --release -p canopy-sim --example army_clash -- --data . \
//!     --a unit.beetles.shieldbearer:6,unit.beetles.bolter:4 \
//!     --b unit.ants.warrior:8,unit.ants.archer:6 [--seeds 4] [--research-a id,id] [--ai]
//! ```
//!
//! Each side attack-moves through the other's start; `--ai` lets the abilities' AI cast
//! (both sides are AI players that only fight). Costs are the units' creation bills.
#![allow(clippy::disallowed_methods)] // wall-clock timing

use canopy_content::{Content, Id, map::UtcMap, placement::Owner};
use canopy_sim::{Action, MatchSetup, Queued, Slot, SlotKind, World};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

const SIZE: u32 = 256;
const HALO: i32 = 16;

fn base64(bytes: &[u8]) -> String {
    const SYMBOLS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(SYMBOLS[(word >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn grid(value: f32) -> String {
    let vertices = SIZE as i32 + 2 * HALO + 1;
    let bytes: Vec<u8> = (0..vertices * vertices)
        .flat_map(|_| value.to_le_bytes())
        .collect();
    base64(&bytes)
}

fn field(entities: Vec<Value>) -> UtcMap {
    let span = (SIZE + 2 * HALO as u32) as usize;
    let flow: Vec<u8> = (0..span * span).flat_map(|_| [128, 128, 0, 0]).collect();
    let zero = grid(0.0);
    let terrain = json!({"version": 1, "size": SIZE, "heights": zero.clone(), "waterHeights": grid(-1.0),
        "waterFlow": base64(&flow), "waterProfiles": [], "grass": zero.clone(), "rock": zero, "paint": []});
    let document = json!({
        "v": 3, "size": SIZE, "name": "Army clash", "stamps": [], "sandbox": true,
        "playerStarts": [], "entities": entities, "camps": [],
        "authoring": {"version": 1, "terrain": terrain, "objects": []},
    });
    UtcMap::from_value(document, "clash.utcmap").unwrap_or_else(|e| panic!("{e}"))
}

fn army(spec: &str) -> Vec<(String, u32)> {
    spec.split(',')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let (id, n) = s.split_once(':').unwrap_or((s, "1"));
            (id.trim().to_string(), n.trim().parse().expect("count"))
        })
        .collect()
}

struct Args {
    data: PathBuf,
    a: Vec<(String, u32)>,
    b: Vec<(String, u32)>,
    seeds: u32,
    research: [Vec<String>; 2],
    ai: bool,
}

fn args() -> Args {
    let mut out = Args {
        data: ".".into(),
        a: Vec::new(),
        b: Vec::new(),
        seeds: 4,
        research: [Vec::new(), Vec::new()],
        ai: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().expect("value");
        match flag.as_str() {
            "--data" => out.data = value().into(),
            "--a" => out.a = army(&value()),
            "--b" => out.b = army(&value()),
            "--seeds" => out.seeds = value().parse().expect("seeds"),
            "--research-a" => out.research[0] = value().split(',').map(str::to_string).collect(),
            "--research-b" => out.research[1] = value().split(',').map(str::to_string).collect(),
            "--ai" => out.ai = true,
            other => panic!("unknown option {other}"),
        }
    }
    out
}

fn cost(content: &Content, id: &str) -> u32 {
    content
        .definition(id)
        .and_then(|d| d.creation.as_ref())
        .map_or(0, |c| c.items().iter().map(|i| i.amount).sum())
}

fn main() {
    let args = args();
    let content = Arc::new(Content::load(&args.data).unwrap_or_else(|e| panic!("{e}")));
    let (west, east) = (100.0, 140.0);
    let mut totals = [0u32; 2];
    for seed in 1..=args.seeds {
        let mut entities = Vec::new();
        for (side, (list, x)) in [(&args.a, west), (&args.b, east)].into_iter().enumerate() {
            let mut n = 0;
            for (id, count) in list {
                for _ in 0..*count {
                    let (row, col) = (n / 6, n % 6);
                    let dx = if side == 0 {
                        -(row as f64) * 4.0
                    } else {
                        row as f64 * 4.0
                    };
                    entities.push(json!({"id": format!("s{side}.{n}"), "definition": id,
                        "position": {"x": x + dx, "y": 116.0 + col as f64 * 4.0 + (seed % 2) as f64},
                        "rotation": if side == 0 { 90 } else { 270 }, "owner": format!("player.{}", side + 1)}));
                    n += 1;
                }
            }
        }
        let map = field(entities);
        let kind = if args.ai {
            SlotKind::Ai
        } else {
            SlotKind::Human
        };
        let setup = MatchSetup {
            seed,
            slots: (0..2)
                .map(|p| Slot {
                    player: p,
                    kind,
                    name: None,
                    team: None,
                    hero: None,
                    race: None,
                })
                .collect(),
        };
        let mut world = World::new(content.clone(), &map, setup).unwrap_or_else(|e| panic!("{e}"));
        for (side, list) in args.research.iter().enumerate() {
            let researched = world
                .state_mut()
                .economy
                .research
                .entry(Owner::Player(side as u8))
                .or_default();
            researched.extend(list.iter().map(|r| Id::new(r).expect("id")));
            researched.sort();
        }
        let ids = |world: &World, side: u8| -> Vec<u32> {
            world
                .state()
                .entities
                .iter()
                .filter(|e| {
                    e.owner == Owner::Player(side)
                        && e.unit.is_some()
                        && e.hp.is_some_and(|h| h > 0)
                })
                .map(|e| e.id.0)
                .collect()
        };
        for (side, x) in [(0u8, east + 20.0), (1u8, west - 20.0)] {
            let action: Action =
                serde_json::from_value(json!({"type": "move", "actors": ids(&world, side),
                "destination": {"x": x as u32, "y": 126}, "attackMove": true, "append": null}))
                .expect("action");
            let tick = world.tick();
            world
                .enqueue(Queued {
                    tick,
                    player: side,
                    seq: 0,
                    action,
                })
                .expect("queued");
        }
        let mut ticks = 0;
        for t in 0..40 * 180 {
            world.step();
            ticks = t;
            if ids(&world, 0).is_empty() || ids(&world, 1).is_empty() {
                break;
            }
        }
        let left = |side: u8| -> (usize, u32) {
            let alive = ids(&world, side);
            let value = alive
                .iter()
                .map(|&id| {
                    let e = world
                        .state()
                        .entities
                        .get(canopy_sim::entity::EntityId(id))
                        .expect("unit");
                    cost(&content, world.catalog().id(e.definition).as_str())
                })
                .sum();
            (alive.len(), value)
        };
        let (a, b) = (left(0), left(1));
        println!(
            "seed {seed}: A {} left (worth {}), B {} left (worth {}) after {:.0} s",
            a.0,
            a.1,
            b.0,
            b.1,
            f64::from(ticks) / 40.0
        );
        totals[0] += a.1;
        totals[1] += b.1;
    }
    let spent =
        |list: &[(String, u32)]| -> u32 { list.iter().map(|(id, n)| cost(&content, id) * n).sum() };
    println!(
        "A spent {} per fight, kept {} on average; B spent {}, kept {}",
        spent(&args.a),
        totals[0] / args.seeds,
        spent(&args.b),
        totals[1] / args.seeds
    );
}
