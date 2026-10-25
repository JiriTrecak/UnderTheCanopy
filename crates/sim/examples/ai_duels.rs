//! AI duels on every shipped map, several seeds each, in parallel: who wins, when, and
//! what each AI trained. Measures AI changes (D52, D53); not a test.
//!
//! ```text
//! cargo run --release -p canopy-sim --example ai_duels -- --data . [--seeds 4] \
//!     [--minutes 60] [--races ants,beetles] [--maps frozen-dream] [--log]
//! ```
//!
//! `--styles a,b` plays the two computer players in those styles (D75: standard, rush,
//! expand, tech) instead of the ones the seed picks.
//!
//! `--heroes a,b` starts the players with those heroes (unit ids; empty for the
//! computer's own pick).
//!
//! `--swap` trades the two starts' places, so player 1 starts where player 2 would: it
//! tells a seat bias (player order) from a map one (start positions).
//!
//! `--mirror-camps 1|2` makes the creeps fair: every camp on the other start's side
//! becomes the mirror image of its partner on that start's side (a mirror across the
//! starts' midline when they share a row, else a point mirror about their midpoint).
//!
//! `--log` also prints each player's milestones: when each kind of building first stands
//! complete and when each research finishes.
//!
//! Each duel takes the two starts farthest apart. `--races a,b` gives the first and second
//! player those races (the default race for both when absent). `--maps` keeps those maps
//! (package ids) only. Per player it also shows the most buildings it held on resources
//! at once (Substance Wells, D62), every good it gathered and spent, and, for a race on a
//! power network (D66), its most nodes and buildings needing power at once and how long
//! those stood unpowered.
#![allow(clippy::disallowed_methods)] // wall-clock timing of the runs

use canopy_content::{Content, map, map::UtcMap};
use canopy_sim::{MatchSetup, Slot, SlotKind, World};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};

struct Args {
    data: PathBuf,
    seeds: u32,
    minutes: u32,
    races: [Option<String>; 2],
    heroes: [Option<String>; 2],
    styles: [Option<canopy_sim::ai::AiStyle>; 2],
    maps: Vec<String>,
    log: bool,
    swap: bool,
    mirror_camps: Option<usize>,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        data: ".".into(),
        seeds: 4,
        minutes: 60,
        races: [None, None],
        heroes: [None, None],
        styles: [None, None],
        maps: Vec::new(),
        log: false,
        swap: false,
        mirror_camps: None,
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
            "--heroes" => {
                let v = value()?;
                let mut parts = v
                    .split(',')
                    .map(|s| Some(s.trim().to_string()).filter(|s| !s.is_empty()));
                out.heroes = [parts.next().flatten(), parts.next().flatten()];
            }
            "--styles" => {
                let v = value()?;
                let mut parts = v
                    .split(',')
                    .map(|s| canopy_sim::ai::AiStyle::parse(s.trim()));
                out.styles = [parts.next().flatten(), parts.next().flatten()];
            }
            "--maps" => out.maps = value()?.split(',').map(|m| m.trim().to_string()).collect(),
            "--log" => out.log = true,
            "--swap" => out.swap = true,
            "--mirror-camps" => {
                out.mirror_camps = Some(
                    value()?
                        .parse()
                        .map_err(|e| format!("--mirror-camps: {e}"))?,
                )
            }
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

/// Replaces the camps nearer the other start with mirror images of those nearer start
/// `side` (one-based, of the two kept starts).
fn mirror(map: &mut UtcMap, side: usize) {
    let [a, b] = [&map.player_starts[0], &map.player_starts[1]];
    let (keep, other) = if side == 1 { (a, b) } else { (b, a) };
    let (keep, other) = ((keep.x, keep.z), (other.x, other.z));
    let (cx, cz) = ((keep.0 + other.0) / 2.0, (keep.1 + other.1) / 2.0);
    let row = (keep.1 - other.1).abs() < 1.0;
    let flip = |x: f64, z: f64| {
        if row {
            (2.0 * cx - x, z)
        } else {
            (2.0 * cx - x, 2.0 * cz - z)
        }
    };
    let d = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).hypot(p.1 - q.1);
    let ours = |c: &canopy_content::placement::Camp| {
        let h = (c.home.x, c.home.y);
        d(h, keep) + 1.0 < d(h, other)
    };
    let sources: Vec<canopy_content::placement::Camp> =
        map.camps.iter().filter(|c| ours(c)).cloned().collect();
    let mut removed = std::collections::BTreeSet::new();
    let mut added = Vec::new();
    for camp in map.camps.iter_mut().filter(|c| !ours(c)) {
        let m = flip(camp.home.x, camp.home.y);
        let Some(partner) = sources
            .iter()
            .min_by(|p, q| d((p.home.x, p.home.y), m).total_cmp(&d((q.home.x, q.home.y), m)))
            .filter(|p| d((p.home.x, p.home.y), m) < 12.0)
        else {
            continue;
        };
        removed.extend(camp.members.iter().cloned());
        let mut members = Vec::new();
        for id in &partner.members {
            let Some(e) = map.entities.iter().find(|e| &e.id == id) else {
                continue;
            };
            let mut e = e.clone();
            (e.position.x, e.position.y) = flip(e.position.x, e.position.y);
            e.id = format!("{}.mirrored", e.id);
            members.push(e.id.clone());
            added.push(e);
        }
        let id = camp.id.clone();
        *camp = partner.clone();
        camp.id = id;
        (camp.home.x, camp.home.y) = flip(partner.home.x, partner.home.y);
        camp.members = members;
    }
    map.entities.retain(|e| !removed.contains(&e.id));
    map.entities.extend(added);
}

struct Duel {
    map: String,
    seed: u32,
    ticks: u32,
    winner: Option<u8>,
    /// Per player: the starting hero.
    started: [String; 2],
    /// Per player: units trained by definition.
    trained: [BTreeMap<String, u32>; 2],
    /// Per player: (minute, what) as buildings first stand complete and research finishes.
    milestones: [Vec<(f64, String)>; 2],
    /// Every two minutes (or `DUEL_EVERY` seconds), per player and keyed by second: workers, army units, the army's cost, banked goods.
    snapshots: Vec<(u32, [String; 2])>,
    /// Per player: how often the AI decided each thing.
    decisions: [BTreeMap<String, u32>; 2],
    /// Per player: abilities released, by id.
    casts: [BTreeMap<String, u32>; 2],
    /// Per player: the most finished buildings on resources (`placementOn`) at once.
    wells: [u32; 2],
    /// Per player and good: gathered, spent.
    goods: [BTreeMap<String, (u32, u32)>; 2],
    /// Per player, power network (D66): the most finished nodes and finished buildings
    /// that need power at once, and the building-seconds such buildings stood unpowered.
    network: [(u32, u32, u32); 2],
}

fn duel(content: Arc<Content>, map: &UtcMap, seed: u32, cap: u32, args: &Args) -> Duel {
    let (races, swap, mirror_camps) = (&args.races, args.swap, args.mirror_camps);
    let pair = farthest(map);
    let mut map = map.clone();
    map.player_starts.retain(|s| pair.contains(&s.player));
    if let Some(side) = mirror_camps {
        mirror(&mut map, side);
    }
    if swap && let [a, b] = map.player_starts.as_mut_slice() {
        std::mem::swap(&mut a.x, &mut b.x);
        std::mem::swap(&mut a.z, &mut b.z);
        std::mem::swap(&mut a.rotation, &mut b.rotation);
    }
    let players: Vec<u8> = map.player_starts.iter().map(|s| s.player - 1).collect();
    let setup = MatchSetup {
        seed,
        slots: players
            .iter()
            .enumerate()
            .map(|(i, &player)| Slot {
                difficulty: Default::default(),
                player,
                kind: SlotKind::Ai,
                name: None,
                team: None,
                hero: args.heroes[i].clone(),
                race: races[i].clone(),
            })
            .collect(),
    };
    let mut world = World::new(content, &map, setup).unwrap_or_else(|e| panic!("{e}"));
    for (i, &p) in players.iter().enumerate() {
        if let Some(style) = args.styles[i] {
            world.set_ai_style(p, style);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut trained: [BTreeMap<String, u32>; 2] = Default::default();
    let mut milestones: [Vec<(f64, String)>; 2] = Default::default();
    let mut built: [std::collections::BTreeSet<String>; 2] = Default::default();
    // Main halls seen standing, by entity id (the first one is the start's).
    let mut halls: [std::collections::BTreeSet<u32>; 2] = Default::default();
    // The hero each side starts with.
    let mut started: [String; 2] = Default::default();
    let mut learned: [usize; 2] = [0, 0];
    let mut snapshots = Vec::new();
    // Every AI decision of the match, counted by reason (and refusals).
    let mut decisions: [BTreeMap<String, u32>; 2] = Default::default();
    let mut decided: [u32; 2] = [0, 0];
    // Abilities released per side, by ability id.
    let mut casts: [BTreeMap<String, u32>; 2] = Default::default();
    // Army losses per side (summons aside): to camp creatures, to players.
    let mut alive_at: BTreeMap<u32, (usize, canopy_sim::entity::Milli)> = BTreeMap::new();
    let mut losses = [[0u32; 2]; 2];
    let mut ticks = 0;
    let mut wells = [0u32; 2];
    let mut network = [(0u32, 0u32, 0u32); 2];
    let mut goods: [BTreeMap<String, (u32, u32)>; 2] = Default::default();
    let mut next_fact = 0;
    // Snapshot interval: two minutes, or DUEL_EVERY seconds.
    let every: u32 = std::env::var("DUEL_EVERY")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .map_or(4800, |s| (s * 40).max(40));
    let side_of = |o: canopy_content::placement::Owner| match o {
        canopy_content::placement::Owner::Player(p) => players.iter().position(|&x| x == p),
        canopy_content::placement::Owner::Nobody => None,
    };
    for tick in 0..cap {
        world.step();
        ticks = tick + 1;
        for event in world.drain_events() {
            if event.kind != canopy_sim::abilities::EventKind::Released {
                continue;
            }
            let owner = world.state().entities.get(event.caster).map(|e| e.owner);
            let side = match owner {
                Some(canopy_content::placement::Owner::Player(p)) => {
                    players.iter().position(|&x| x == p)
                }
                _ => None,
            };
            if let Some(side) = side {
                let id = world.catalog().content.abilities.abilities[usize::from(event.ability.0)]
                    .id
                    .to_string();
                *casts[side].entry(id).or_default() += 1;
            }
        }
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
            let mut nodes = [0u32; 2];
            let mut needing = [0u32; 2];
            for e in world.state().entities.iter() {
                let d = world.catalog().get(e.definition);
                if e.construction.is_some()
                    || e.hp.is_none_or(|hp| hp == 0)
                    || d.kind != canopy_content::game::Kind::Building
                {
                    continue;
                }
                let Some(side) = side_of(e.owner) else {
                    continue;
                };
                if d.network
                    .as_ref()
                    .is_some_and(|n| n.role == canopy_content::game::NetworkRole::Node)
                {
                    nodes[side] += 1;
                }
                if d.power.is_some() {
                    needing[side] += 1;
                    if world.economy().network().unpowered(e.id) {
                        network[side].2 += 1;
                    }
                }
            }
            for side in 0..2 {
                network[side].0 = network[side].0.max(nodes[side]);
                network[side].1 = network[side].1.max(needing[side]);
            }
        }
        if tick % 40 == 0 {
            for s in world.ai_summaries() {
                let Some(side) = players.iter().position(|&x| x == s.player) else {
                    continue;
                };
                for t in s.trace.iter().filter(|t| t.tick >= decided[side]) {
                    let key = format!(
                        "{}{}",
                        t.reason,
                        if t.accepted == Some(false) {
                            " (refused)"
                        } else {
                            ""
                        }
                    );
                    *decisions[side].entry(key).or_default() += 1;
                }
                decided[side] = tick + 1;
            }
            let mut now: BTreeMap<u32, (usize, canopy_sim::entity::Milli)> = BTreeMap::new();
            let mut creeps: Vec<canopy_sim::entity::Milli> = Vec::new();
            for e in world.state().entities.iter() {
                if e.unit.is_none() || !e.hp.is_some_and(|h| h > 0) {
                    continue;
                }
                let canopy_content::placement::Owner::Player(p) = e.owner else {
                    creeps.push(e.position);
                    continue;
                };
                let d = world.catalog().get(e.definition);
                let summoned = e.links.as_ref().is_some_and(|l| l.summoned.is_some());
                if d.behaviors.combat.is_some() && d.behaviors.work.is_none() && !summoned {
                    let side = players.iter().position(|&x| x == p).unwrap_or(0);
                    now.insert(e.id.0, (side, e.position));
                }
            }
            for (id, (side, at)) in &alive_at {
                if !now.contains_key(id) {
                    // Lost to creeps when a camp creature stands within 14 cells.
                    let by_creeps = creeps.iter().any(|c| {
                        let (dx, dy) = (i64::from(c.x - at.x), i64::from(c.y - at.y));
                        dx * dx + dy * dy < 14_000 * 14_000
                    });
                    losses[*side][usize::from(!by_creeps)] += 1;
                }
            }
            alive_at = now;
            for e in world.state().entities.iter() {
                let canopy_content::placement::Owner::Player(p) = e.owner else {
                    continue;
                };
                let side = players.iter().position(|&x| x == p).unwrap_or(0);
                let id = world.catalog().id(e.definition).to_string();
                if e.unit.is_some() && seen.insert(e.id.0) {
                    if tick > 0 {
                        *trained[side].entry(id.clone()).or_default() += 1;
                    } else if world.catalog().get(e.definition).is_hero() {
                        started[side] = id.rsplit('.').next().unwrap_or("").to_string();
                    }
                }
                if e.unit.is_none()
                    && e.construction.is_none()
                    && e.hp.is_some_and(|hp| hp > 0)
                    && tick > 0
                {
                    if built[side].insert(id.clone()) {
                        halls[side].insert(e.id.0);
                        milestones[side].push((f64::from(tick) / 2400.0, id));
                    } else if world
                        .catalog()
                        .get(e.definition)
                        .behaviors
                        .storage
                        .as_ref()
                        .is_some_and(|s| s.dropoff == Some(true) && s.gathering.is_some())
                        && halls[side].insert(e.id.0)
                    {
                        // A further main hall: an expansion.
                        milestones[side]
                            .push((f64::from(tick) / 2400.0, format!("{id} (another)")));
                    }
                }
            }
            if tick % every == 0 && tick > 0 {
                let catalog = world.catalog();
                let line = |p: u8| {
                    let owner = canopy_content::placement::Owner::Player(p);
                    let (mut workers, mut army, mut value) = (0, 0, 0);
                    let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
                    for e in world.state().entities.iter() {
                        let d = catalog.get(e.definition);
                        if e.owner != owner || e.unit.is_none() || !e.hp.is_some_and(|hp| hp > 0) {
                            continue;
                        }
                        if d.behaviors.work.is_some() {
                            workers += 1;
                        } else if d.behaviors.combat.is_some() {
                            army += 1;
                            let mut name =
                                d.id.as_str().rsplit('.').next().unwrap_or("").to_string();
                            if d.is_hero() {
                                name += &format!("L{}", canopy_sim::stats::level(catalog, e));
                            }
                            *kinds.entry(name).or_default() += 1;
                            value += d
                                .creation
                                .as_ref()
                                .map_or(0, |c| c.items().iter().map(|i| i.amount).sum::<u32>());
                        }
                    }
                    // Buildings: unpowered (!) or under construction (~).
                    let mut built: Vec<String> = Vec::new();
                    for e in world.state().entities.iter() {
                        let d = catalog.get(e.definition);
                        if e.owner != owner || e.unit.is_some() || !e.hp.is_some_and(|hp| hp > 0) {
                            continue;
                        }
                        let mark = if e.construction.is_some() {
                            "~"
                        } else if world.economy().network().unpowered(e.id) {
                            "!"
                        } else {
                            ""
                        };
                        let queued = e.production.as_ref().map_or(0, |q| q.queue.len());
                        let status = e
                            .production
                            .as_ref()
                            .filter(|_| queued > 0)
                            .map(|q| format!("{:?}", q.status))
                            .unwrap_or_default();
                        built.push(format!(
                            "{mark}{}{}",
                            d.id.as_str().rsplit('.').next().unwrap_or(""),
                            if queued > 0 {
                                format!("[{queued} {status}]")
                            } else {
                                String::new()
                            }
                        ));
                    }
                    let wallet = world.state().wallets.get(&owner);
                    let bank = |item: &str| {
                        wallet
                            .and_then(|w| w.iter().find(|(i, _)| i.as_str() == item))
                            .map_or(0, |(_, &n)| n)
                    };
                    let summaries = world.ai_summaries();
                    let doing = summaries
                        .iter()
                        .find(|s| s.player == p)
                        .map(|s| {
                            let last: Vec<String> = s
                                .trace
                                .iter()
                                .rev()
                                .filter(|t| {
                                    std::env::var("DUEL_ALL").is_ok()
                                        || t.accepted != Some(true)
                                        || !t.reason.contains("fight")
                                })
                                .take(if std::env::var("DUEL_ALL").is_ok() {
                                    40
                                } else {
                                    3
                                })
                                .map(|t| {
                                    format!(
                                        "{}{}",
                                        t.reason,
                                        if t.accepted == Some(false) {
                                            " (refused)"
                                        } else {
                                            ""
                                        }
                                    )
                                })
                                .collect();
                            format!("{} / {}", s.plan.economy, last.join("; "))
                        })
                        .unwrap_or_default();
                    let kinds: Vec<String> =
                        kinds.iter().map(|(k, n)| format!("{k}×{n}")).collect();
                    format!(
                        "{workers}w {army}u worth {value}, bank {}a {}s ({}) {{{}}} [{doing}]",
                        bank("item.amber"),
                        bank("item.root"),
                        kinds.join(" "),
                        built.join(" ")
                    )
                };
                let [l0, l1] = [line(players[0]), line(players[1])];
                snapshots.push((
                    tick / 40,
                    [
                        format!("{l0} lost to creeps/players {:?}", losses[0]),
                        format!("{l1} lost {:?}", losses[1]),
                    ],
                ));
            }
            for (side, &p) in players.iter().enumerate() {
                let done = world
                    .state()
                    .economy
                    .researched(canopy_content::placement::Owner::Player(p));
                // Research lists stay sorted; report what is new since the last look.
                if done.len() > learned[side] {
                    for id in done {
                        let name = id.to_string();
                        if !milestones[side].iter().any(|(_, m)| *m == name) {
                            milestones[side].push((f64::from(tick) / 2400.0, name));
                        }
                    }
                    learned[side] = done.len();
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
        started,
        trained,
        milestones,
        snapshots,
        decisions,
        casts,
        wells,
        goods,
        network,
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
                let args = &args;
                s.spawn(move || duel(content, m, seed, cap, args))
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
            println!(
                "    player {} ({}): {}",
                side + 1,
                r.started[side],
                list.join(" ")
            );
            let goods: Vec<String> = r.goods[side]
                .iter()
                .map(|(id, (got, spent))| {
                    format!("{} +{got} -{spent}", id.rsplit('.').next().unwrap_or(id))
                })
                .collect();
            println!("      wells {}; {}", r.wells[side], goods.join(", "));
            let (nodes, needing, dark) = r.network[side];
            if nodes + needing > 0 {
                println!(
                    "      network: nodes {nodes}, needing power {needing}, unpowered {dark} building-s"
                );
            }
        }
        if args.log {
            for (side, steps) in r.milestones.iter().enumerate() {
                let list: Vec<String> = steps
                    .iter()
                    .map(|(m, id)| format!("{m:.1} {}", id.rsplit('.').next().unwrap_or(id)))
                    .collect();
                println!("    player {} milestones: {}", side + 1, list.join(", "));
            }
            for (side, cast) in r.casts.iter().enumerate() {
                let list: Vec<String> = cast.iter().map(|(k, n)| format!("{k}×{n}")).collect();
                println!("    player {} casts: {}", side + 1, list.join(" "));
            }
            for (side, made) in r.decisions.iter().enumerate() {
                let mut list: Vec<(&String, &u32)> = made.iter().collect();
                list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                let list: Vec<String> = list.iter().map(|(k, n)| format!("{n}× {k}")).collect();
                println!("    player {} decisions: {}", side + 1, list.join("; "));
            }
            for (minute, sides) in &r.snapshots {
                println!(
                    "    {:>2}:{:02}: {} | {}",
                    minute / 60,
                    minute % 60,
                    sides[0],
                    sides[1]
                );
            }
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
