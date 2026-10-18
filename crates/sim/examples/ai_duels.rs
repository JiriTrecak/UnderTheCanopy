//! AI duels on every shipped map, several seeds each, in parallel: who wins, when, and
//! what each AI trained. Measures AI changes (D52, D53); not a test.
//!
//! ```text
//! cargo run --release -p canopy-sim --example ai_duels -- --data . [--seeds 4] \
//!     [--minutes 60] [--races ants,beetles] [--maps echo-isles,amberleaf-crossing]
//! ```
//!
//! Each duel takes the two starts farthest apart. `--races a,b` gives the first and second
//! player those races (the default race for both when absent). `--maps` keeps those maps
//! (package ids) only. Per player it also shows the most buildings it held on resources
//! at once (Substance Wells, D62) and every good it gathered and spent.
#![allow(clippy::disallowed_methods)] // wall-clock timing of the runs

use canopy_content::{Content, map, map::UtcMap};
use canopy_sim::{MatchSetup, Slot, SlotKind, World};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};

struct Args {
    data: PathBuf,
    seeds: u32,
    minutes: u32,
    races: [Option<String>; 2],
    maps: Vec<String>,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        data: ".".into(),
        seeds: 4,
        minutes: 60,
        races: [None, None],
        maps: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--data" => out.data = value()?.into(),
            "--seeds" => out.seeds = value()?.parse().map_err(|e| format!("--seeds: {e}"))?,
            "--minutes" => out.minutes = value()?.parse().map_err(|e| format!("--minutes: {e}"))?,
            "--races" => {
                let v = value()?;
                let mut parts = v.split(',').map(|s| Some(s.trim().to_string()));
                out.races = [parts.next().flatten(), parts.next().flatten()];
            }
            "--maps" => out.maps = value()?.split(',').map(|m| m.trim().to_string()).collect(),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(out)
}

/// The two starts farthest apart (one-based players).
fn farthest(map: &UtcMap) -> [u8; 2] {
    let starts = &map.player_starts;
    let mut best = (0.0, [starts[0].player, starts[1].player]);
    for a in starts {
        for b in starts {
            let d = (a.x - b.x).hypot(a.z - b.z);
            if d > best.0 {
                best = (d, [a.player, b.player]);
            }
        }
    }
    best.1
}

struct Duel {
    map: String,
    seed: u32,
    ticks: u32,
    winner: Option<u8>,
    /// Per player: units trained by definition.
    trained: [BTreeMap<String, u32>; 2],
    /// Per player: the most finished buildings on resources (`placementOn`) at once.
    wells: [u32; 2],
    /// Per player and good: gathered, spent.
    goods: [BTreeMap<String, (u32, u32)>; 2],
}

fn duel(
    content: Arc<Content>,
    map: &UtcMap,
    seed: u32,
    cap: u32,
    races: &[Option<String>; 2],
) -> Duel {
    let pair = farthest(map);
    let mut map = map.clone();
    map.player_starts.retain(|s| pair.contains(&s.player));
    let players: Vec<u8> = map.player_starts.iter().map(|s| s.player - 1).collect();
    let setup = MatchSetup {
        seed,
        slots: players
            .iter()
            .enumerate()
            .map(|(i, &player)| Slot {
                player,
                kind: SlotKind::Ai,
                name: None,
                team: None,
                hero: None,
                race: races[i].clone(),
            })
            .collect(),
    };
    let mut world = World::new(content, &map, setup).unwrap_or_else(|e| panic!("{e}"));
    let mut seen = std::collections::BTreeSet::new();
    let mut trained: [BTreeMap<String, u32>; 2] = Default::default();
    let mut ticks = 0;
    let mut wells = [0u32; 2];
    let mut goods: [BTreeMap<String, (u32, u32)>; 2] = Default::default();
    let mut next_fact = 0;
    let side_of = |o: canopy_content::placement::Owner| match o {
        canopy_content::placement::Owner::Player(p) => players.iter().position(|&x| x == p),
        canopy_content::placement::Owner::Nobody => None,
    };
    for tick in 0..cap {
        world.step();
        ticks = tick + 1;
        let from = next_fact;
        for f in world.state().facts.iter().filter(|f| f.id >= from) {
            next_fact = f.id + 1;
            let (Some(side), Some(item), Some(amount)) = (side_of(f.owner), &f.item, f.amount)
            else {
                continue;
            };
            let entry = goods[side].entry(item.to_string()).or_default();
            match f.kind {
                canopy_sim::state::FactKind::Produced => entry.0 += amount,
                canopy_sim::state::FactKind::Consumed => entry.1 += amount,
                _ => {}
            }
        }
        if tick % 40 == 0 {
            let mut now = [0u32; 2];
            for e in world.state().entities.iter() {
                if e.construction.is_none()
                    && e.hp.is_some_and(|hp| hp > 0)
                    && world.catalog().get(e.definition).placement_on.is_some()
                    && let Some(side) = side_of(e.owner)
                {
                    now[side] += 1;
                }
            }
            for side in 0..2 {
                wells[side] = wells[side].max(now[side]);
            }
        }
        if tick % 40 == 0 {
            for e in world.state().entities.iter() {
                let canopy_content::placement::Owner::Player(p) = e.owner else {
                    continue;
                };
                if e.unit.is_some() && seen.insert(e.id.0) && tick > 0 {
                    let side = players.iter().position(|&x| x == p).unwrap_or(0);
                    *trained[side]
                        .entry(world.catalog().id(e.definition).to_string())
                        .or_default() += 1;
                }
            }
        }
        if world.state().outcome.is_some() {
            break;
        }
    }
    let winner = world.state().outcome.as_ref().and_then(|o| match o.winner {
        Some(canopy_content::placement::Owner::Player(p)) => {
            players.iter().position(|&x| x == p).map(|i| i as u8)
        }
        _ => None,
    });
    Duel {
        map: map.name.clone(),
        seed,
        ticks,
        winner,
        trained,
        wells,
        goods,
    }
}

fn main() -> Result<(), String> {
    let args = args()?;
    let content = Arc::new(Content::load(&args.data).map_err(|e| format!("content: {e}"))?);
    let maps: Vec<UtcMap> = map::library(&args.data)
        .map_err(|e| format!("maps: {e}"))?
        .iter()
        .filter(|f| args.maps.is_empty() || args.maps.contains(&f.id))
        .map(|f| f.load().map_err(|e| format!("{}: {e}", f.file)))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        // A duel needs two starts (the showcase benches have one).
        .filter(|m| m.player_starts.len() >= 2)
        .collect();
    let cap = args.minutes * 60 * 40;
    let started = Instant::now();
    let results: Vec<Duel> = std::thread::scope(|s| {
        let handles: Vec<_> = maps
            .iter()
            .flat_map(|m| (1..=args.seeds).map(move |seed| (m, seed)))
            .map(|(m, seed)| {
                let content = content.clone();
                let races = &args.races;
                s.spawn(move || duel(content, m, seed, cap, races))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("duel"))
            .collect()
    });
    let mut finished = Vec::new();
    for r in &results {
        let minutes = f64::from(r.ticks) / 2400.0;
        let outcome = match r.winner {
            Some(w) => format!("player {} wins", w + 1),
            None if r.ticks >= cap => "cap reached".into(),
            None => "draw".into(),
        };
        if r.ticks < cap {
            finished.push(minutes);
        }
        println!(
            "{:<22} seed {}: {outcome} after {minutes:.1} min",
            r.map, r.seed
        );
        for (side, trained) in r.trained.iter().enumerate() {
            let list: Vec<String> = trained
                .iter()
                .map(|(id, n)| format!("{}×{n}", id.rsplit('.').next().unwrap_or(id)))
                .collect();
            println!("    player {}: {}", side + 1, list.join(" "));
            let goods: Vec<String> = r.goods[side]
                .iter()
                .map(|(id, (got, spent))| {
                    format!("{} +{got} -{spent}", id.rsplit('.').next().unwrap_or(id))
                })
                .collect();
            println!("      wells {}; {}", r.wells[side], goods.join(", "));
        }
    }
    let mean = finished.iter().sum::<f64>() / finished.len().max(1) as f64;
    println!(
        "{} of {} finished, mean {mean:.1} game minutes ({:.0} s)",
        finished.len(),
        results.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
