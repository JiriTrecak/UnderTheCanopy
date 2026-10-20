//! An AI league for balance (D75): many one-on-one matches between computer players of
//! every race and style, on the skirmish maps, run in parallel; then who wins and why.
//! Measures balance; not a test.
//!
//! ```text
//! cargo run --release -p canopy-sim --example ai_league -- --data . [--matches 100] \
//!     [--races ants,beetles] [--styles standard,rush,expand,tech] [--maps a,b] \
//!     [--minutes 40] [--threads N] [--json out.json]
//! ```
//!
//! Matches cycle through every pairing of race and style on both sides, every skirmish
//! map in turn and a fresh seed each, so 100 matches cover the field evenly. A match that
//! reaches `--minutes` is a draw, and the side with more standing value (units and
//! buildings at cost) is noted as ahead.
//!
//! The report:
//! - win rates by race, by style and by race and style against each other;
//! - how quickly each style wins and loses, and how often rushes end the match early;
//! - every unit: how many were trained per match, how much of what they cost they killed
//!   in enemy value (kills credited to the unit that struck the lethal blow, summons to
//!   their kind), their losses and the win rate of sides that fielded many of them;
//! - every hero: picks, win rate with it, its level at the end; every ability: casts.
//!
//! `--json` writes every match's raw record as well.
#![allow(clippy::disallowed_methods)] // wall-clock timing of the runs

use canopy_content::{Content, game::Definition, map, map::UtcMap, placement::Owner};
use canopy_sim::{MatchSetup, Slot, SlotKind, World, ai::AiStyle, catalog::Catalog};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

struct Args {
    data: PathBuf,
    matches: usize,
    races: Vec<String>,
    styles: Vec<AiStyle>,
    maps: Vec<String>,
    minutes: u32,
    threads: usize,
    json: Option<PathBuf>,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        data: ".".into(),
        matches: 100,
        races: vec!["ants".into(), "beetles".into()],
        styles: AiStyle::ALL.to_vec(),
        maps: Vec::new(),
        minutes: 40,
        threads: std::thread::available_parallelism().map_or(4, |n| n.get()),
        json: None,
    };
    let list = |v: String| -> Vec<String> { v.split(',').map(|s| s.trim().to_string()).collect() };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--data" => out.data = value()?.into(),
            "--matches" => out.matches = value()?.parse().map_err(|e| format!("--matches: {e}"))?,
            "--races" => out.races = list(value()?),
            "--styles" => {
                out.styles = list(value()?)
                    .iter()
                    .map(|s| AiStyle::parse(s).ok_or(format!("unknown style {s}")))
                    .collect::<Result<_, _>>()?
            }
            "--maps" => out.maps = list(value()?),
            "--minutes" => out.minutes = value()?.parse().map_err(|e| format!("--minutes: {e}"))?,
            "--threads" => out.threads = value()?.parse().map_err(|e| format!("--threads: {e}"))?,
            "--json" => out.json = Some(value()?.into()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(out)
}

/// One side's record of a match.
#[derive(Debug, Clone, Default, Serialize)]
struct Side {
    race: String,
    style: String,
    /// Heroes fielded, with the level each ended on (or died at last).
    heroes: BTreeMap<String, u32>,
    trained: BTreeMap<String, u32>,
    lost: BTreeMap<String, u32>,
    /// Enemy units and buildings killed, by the killer's kind: (count, value).
    kills: BTreeMap<String, (u32, u32)>,
    casts: BTreeMap<String, u32>,
    /// Standing value at the end (units and buildings at cost).
    value: u32,
    /// Most workers at once, and the minute the second main hall stood.
    workers: u32,
    expanded: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
struct Match {
    map: String,
    seed: u32,
    minutes: f64,
    /// The winning side (0 or 1); `None` for a draw at the time cap.
    winner: Option<usize>,
    sides: [Side; 2],
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

/// What one of a definition costs: its creation's goods, split over a brood.
fn value(d: &Definition) -> u32 {
    d.creation.as_ref().map_or(0, |c| {
        c.items().iter().map(|i| i.amount).sum::<u32>() / c.count()
    })
}

fn short(id: &str) -> String {
    id.strip_prefix("unit.").unwrap_or(id).to_string()
}

/// A summon's kind is its own (a turret is credited as a turret); the killer is resolved
/// at the moment of the death, while its entity still stands.
fn play(
    content: Arc<Content>,
    map: &UtcMap,
    seed: u32,
    races: [&str; 2],
    styles: [AiStyle; 2],
    cap: u32,
) -> Match {
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
                race: Some(races[i].to_string()),
            })
            .collect(),
    };
    let mut world =
        World::new(content, &map, setup).unwrap_or_else(|e| panic!("{}: {e}", map.name));
    for (i, &p) in players.iter().enumerate() {
        world.set_ai_style(p, styles[i]);
    }
    let side_of = |o: Owner| match o {
        Owner::Player(p) => players.iter().position(|&x| x == p),
        Owner::Nobody => None,
    };
    let mut sides: [Side; 2] = Default::default();
    for i in 0..2 {
        sides[i].race = races[i].to_string();
        sides[i].style = styles[i].name().to_string();
    }
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    // Every entity's kind, so a killer gone by its victim's death (a Burster in its own
    // blast) is still credited.
    let mut kinds: BTreeMap<u32, canopy_sim::catalog::Def> = BTreeMap::new();
    let mut halls: [BTreeSet<u32>; 2] = Default::default();
    let mut death = 0;
    let mut ticks = 0;
    for tick in 0..cap {
        world.step();
        ticks = tick + 1;
        for event in world.drain_events() {
            let catalog: &Catalog = world.catalog();
            if event.kind != canopy_sim::abilities::EventKind::Released {
                continue;
            }
            let owner = world.state().entities.get(event.caster).map(|e| e.owner);
            if let Some(side) = owner.and_then(side_of) {
                let id = catalog.content.abilities.abilities[usize::from(event.ability.0)]
                    .id
                    .to_string();
                *sides[side].casts.entry(id).or_default() += 1;
            }
        }
        let catalog: &Catalog = world.catalog();
        let since = death;
        for f in world.events().deaths().filter(|f| f.sequence > since) {
            death = f.sequence;
            let d = catalog.get(f.definition);
            if let Some(side) = side_of(f.owner)
                && d.kind == canopy_content::game::Kind::Unit
            {
                *sides[side].lost.entry(short(d.id.as_str())).or_default() += 1;
            }
            let (Some(killer), Some(source)) = (f.killer.and_then(side_of), f.source) else {
                continue;
            };
            if side_of(f.owner).is_none_or(|v| v == killer) {
                continue;
            }
            let Some(kind) = world
                .state()
                .entities
                .get(source)
                .map(|k| k.definition)
                .or_else(|| kinds.get(&source.0).copied())
            else {
                continue;
            };
            let entry = sides[killer]
                .kills
                .entry(short(catalog.id(kind).as_str()))
                .or_default();
            entry.0 += 1;
            entry.1 += value(d);
        }
        if tick % 40 == 0 {
            let mut workers = [0u32; 2];
            for e in world.state().entities.iter() {
                kinds.insert(e.id.0, e.definition);
                let Some(side) = side_of(e.owner) else {
                    continue;
                };
                let d = catalog.get(e.definition);
                if !e.hp.is_some_and(|hp| hp > 0) {
                    continue;
                }
                if e.unit.is_some() {
                    if d.behaviors.work.is_some() {
                        workers[side] += 1;
                    }
                    if d.is_hero() {
                        let level = canopy_sim::stats::level(catalog, e);
                        sides[side].heroes.insert(short(d.id.as_str()), level);
                    }
                    if tick > 0 && seen.insert(e.id.0) {
                        *sides[side].trained.entry(short(d.id.as_str())).or_default() += 1;
                    } else if tick == 0 {
                        seen.insert(e.id.0);
                    }
                } else if e.construction.is_none()
                    && d.behaviors
                        .storage
                        .as_ref()
                        .is_some_and(|s| s.dropoff == Some(true) && s.gathering.is_some())
                    && halls[side].insert(e.id.0)
                    && halls[side].len() == 2
                {
                    sides[side].expanded = Some(f64::from(tick) / 2400.0);
                }
            }
            for side in 0..2 {
                sides[side].workers = sides[side].workers.max(workers[side]);
            }
        }
        if world.state().outcome.is_some() {
            break;
        }
    }
    let catalog = world.catalog();
    for e in world.state().entities.iter() {
        if let Some(side) = side_of(e.owner)
            && e.hp.is_some_and(|hp| hp > 0)
        {
            sides[side].value += value(catalog.get(e.definition));
        }
    }
    let winner = world
        .state()
        .outcome
        .as_ref()
        .and_then(|o| o.winner)
        .and_then(side_of);
    Match {
        map: map.name.clone(),
        seed,
        minutes: f64::from(ticks) / 2400.0,
        winner,
        sides,
    }
}

#[derive(Default)]
struct Tally {
    games: u32,
    wins: u32,
    draws: u32,
}

impl Tally {
    fn add(&mut self, won: Option<bool>) {
        self.games += 1;
        match won {
            Some(true) => self.wins += 1,
            None => self.draws += 1,
            _ => {}
        }
    }
    fn rate(&self) -> String {
        let decided = self.games - self.draws;
        if decided == 0 {
            return format!("  -   ({} draws)", self.draws);
        }
        format!(
            "{:>4.0}% of {:>3} decided ({} won, {} draws)",
            100.0 * f64::from(self.wins) / f64::from(decided),
            decided,
            self.wins,
            self.draws
        )
    }
}

fn report(results: &[Match], content: &Content) {
    let won = |m: &Match, side: usize| m.winner.map(|w| w == side);
    let decided = results.iter().filter(|m| m.winner.is_some()).count();
    let minutes: f64 = results.iter().map(|m| m.minutes).sum::<f64>() / results.len().max(1) as f64;
    println!(
        "\n{} matches: {} decided, {} draws; {:.1} minutes on average",
        results.len(),
        decided,
        results.len() - decided,
        minutes
    );
    let mut by_race: BTreeMap<String, Tally> = BTreeMap::new();
    let mut by_style: BTreeMap<String, Tally> = BTreeMap::new();
    let mut by_both: BTreeMap<String, Tally> = BTreeMap::new();
    let mut versus: BTreeMap<(String, String), Tally> = BTreeMap::new();
    let mut race_versus: BTreeMap<(String, String), Tally> = BTreeMap::new();
    let mut by_map: BTreeMap<String, [u32; 3]> = BTreeMap::new();
    let mut length: BTreeMap<String, (f64, u32, f64, u32)> = BTreeMap::new();
    for m in results {
        for side in 0..2 {
            let (s, o) = (&m.sides[side], &m.sides[1 - side]);
            by_race.entry(s.race.clone()).or_default().add(won(m, side));
            by_style
                .entry(s.style.clone())
                .or_default()
                .add(won(m, side));
            by_both
                .entry(format!("{} {}", s.race, s.style))
                .or_default()
                .add(won(m, side));
            versus
                .entry((s.style.clone(), o.style.clone()))
                .or_default()
                .add(won(m, side));
            if s.race != o.race {
                race_versus
                    .entry((
                        format!("{} {}", s.race, s.style),
                        format!("{} {}", o.race, o.style),
                    ))
                    .or_default()
                    .add(won(m, side));
            }
            let l = length.entry(s.style.clone()).or_default();
            match won(m, side) {
                Some(true) => {
                    l.0 += m.minutes;
                    l.1 += 1;
                }
                Some(false) => {
                    l.2 += m.minutes;
                    l.3 += 1;
                }
                None => {}
            }
        }
        let e = by_map.entry(m.map.clone()).or_default();
        match m.winner {
            Some(w) if m.sides[0].race != m.sides[1].race => {
                e[usize::from(m.sides[w].race != "ants")] += 1;
            }
            None => e[2] += 1,
            _ => {}
        }
    }
    println!("\nRaces (mirror matches count a win and a loss):");
    for (k, t) in &by_race {
        println!("  {k:<22} {}", t.rate());
    }
    println!("\nRaces against each other, by style (row against column):");
    for ((a, b), t) in &race_versus {
        if a.starts_with("ants") {
            println!("  {a:<16} vs {b:<16} {}", t.rate());
        }
    }
    println!("\nStyles:");
    for (k, t) in &by_style {
        let (wm, wn, lm, ln) = length.get(k).copied().unwrap_or_default();
        println!(
            "  {k:<22} {}   wins after {:.1} min, losses after {:.1} min",
            t.rate(),
            wm / f64::from(wn.max(1)),
            lm / f64::from(ln.max(1))
        );
    }
    println!("\nStyle against style (row's win rate):");
    for ((a, b), t) in &versus {
        println!("  {a:<9} vs {b:<9} {}", t.rate());
    }
    println!("\nRace and style:");
    for (k, t) in &by_both {
        println!("  {k:<22} {}", t.rate());
    }
    println!("\nMaps (ants wins, beetle wins, draws; mixed-race matches):");
    for (k, [a, b, d]) in &by_map {
        println!("  {k:<26} {a:>3} {b:>3} {d:>3}");
    }
    // Units: trained per side-match, value killed per value fielded, and the win rate of
    // the sides that fielded a large share of them.
    #[derive(Default)]
    struct U {
        race: String,
        trained: u32,
        lost: u32,
        kills: u32,
        killed: u32,
        sides: u32,
        heavy: Tally,
    }
    let mut units: BTreeMap<String, U> = BTreeMap::new();
    let cost = |id: &str| content.definition(&format!("unit.{id}")).map_or(0, value);
    for m in results {
        for side in 0..2 {
            let s = &m.sides[side];
            let army: u32 = s
                .trained
                .iter()
                .filter(|(id, _)| !id.contains("settler") && !id.contains("worker"))
                .map(|(_, n)| n)
                .sum();
            let mut kinds: BTreeSet<&String> = s.trained.keys().collect();
            kinds.extend(s.kills.keys());
            for id in kinds {
                let u = units.entry(id.clone()).or_default();
                u.race = s.race.clone();
                let n = s.trained.get(id).copied().unwrap_or(0);
                u.trained += n;
                u.lost += s.lost.get(id).copied().unwrap_or(0);
                let (k, v) = s.kills.get(id).copied().unwrap_or_default();
                u.kills += k;
                u.killed += v;
                if n > 0 {
                    u.sides += 1;
                }
                if army > 0 && n * 4 >= army && n >= 4 {
                    u.heavy.add(won(m, side));
                }
            }
        }
    }
    let side_matches = (results.len() * 2) as f64;
    println!(
        "\nUnits (per side-match trained; kill value per cost trained; win rate when a quarter of the army):"
    );
    let mut rows: Vec<(&String, &U)> = units.iter().collect();
    rows.sort_by(|a, b| a.1.race.cmp(&b.1.race).then(a.0.cmp(b.0)));
    for (id, u) in rows {
        let spent = u.trained * cost(id);
        let efficiency = if spent > 0 {
            format!("{:>5.2}", f64::from(u.killed) / f64::from(spent))
        } else {
            "    -".into()
        };
        println!(
            "  {id:<30} trained {:>5.1}  lost {:>5}  kills {:>5}  value {efficiency}  heavy {}",
            f64::from(u.trained) / side_matches,
            u.lost,
            u.kills,
            u.heavy.rate()
        );
    }
    let mut heroes: BTreeMap<String, (Tally, u32)> = BTreeMap::new();
    let mut casts: BTreeMap<String, u32> = BTreeMap::new();
    for m in results {
        for side in 0..2 {
            for (h, level) in &m.sides[side].heroes {
                let e = heroes.entry(h.clone()).or_default();
                e.0.add(won(m, side));
                e.1 += level;
            }
            for (a, n) in &m.sides[side].casts {
                *casts.entry(a.clone()).or_default() += n;
            }
        }
    }
    println!("\nHeroes (side-matches fielding it; win rate; average final level):");
    for (h, (t, levels)) in &heroes {
        println!(
            "  {h:<30} {:>4}  {}  L{:.1}",
            t.games,
            t.rate(),
            f64::from(*levels) / f64::from(t.games.max(1))
        );
    }
    println!("\nAbility casts per side-match:");
    for (a, n) in &casts {
        println!("  {a:<44} {:>6.1}", f64::from(*n) / side_matches);
    }
    let expanded: Vec<f64> = results
        .iter()
        .flat_map(|m| m.sides.iter().filter_map(|s| s.expanded))
        .collect();
    println!(
        "\nSecond halls: {} of {} sides, at {:.1} min on average",
        expanded.len(),
        results.len() * 2,
        expanded.iter().sum::<f64>() / expanded.len().max(1) as f64
    );
}

fn main() -> Result<(), String> {
    let args = args()?;
    let content = Arc::new(Content::load(&args.data).map_err(|e| format!("content: {e}"))?);
    let maps: Vec<UtcMap> = map::library(&args.data)
        .map_err(|e| format!("maps: {e}"))?
        .iter()
        .filter(|f| f.group == "skirmish")
        .filter(|f| args.maps.is_empty() || args.maps.contains(&f.id))
        .map(|f| f.load().map_err(|e| format!("{}: {e}", f.file)))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|m| m.player_starts.len() >= 2)
        .collect();
    if maps.is_empty() {
        return Err("no skirmish maps".into());
    }
    // Every pairing of race and style, each side, cycled (races fastest); maps cycle on
    // their own.
    let mut pairings = Vec::new();
    for &sa in &args.styles {
        for &sb in &args.styles {
            for ra in &args.races {
                for rb in &args.races {
                    pairings.push(([ra.as_str(), rb.as_str()], [sa, sb]));
                }
            }
        }
    }
    let jobs: Vec<(usize, u32, [&str; 2], [AiStyle; 2])> = (0..args.matches)
        .map(|i| {
            let (races, styles) = pairings[i % pairings.len()];
            (i % maps.len(), 1 + i as u32, races, styles)
        })
        .collect();
    let cap = args.minutes * 60 * 40;
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Match>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..args.threads.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(map, seed, races, styles)) = jobs.get(i) else {
                        break;
                    };
                    let m = play(content.clone(), &maps[map], seed, races, styles, cap);
                    let outcome = match m.winner {
                        Some(w) => format!("{} {} wins", m.sides[w].race, m.sides[w].style),
                        None => "draw".into(),
                    };
                    eprintln!(
                        "[{:>3}/{}] {:<22} {} {} vs {} {}: {outcome} after {:.1} min ({:.0} s)",
                        i + 1,
                        jobs.len(),
                        m.map,
                        m.sides[0].race,
                        m.sides[0].style,
                        m.sides[1].race,
                        m.sides[1].style,
                        m.minutes,
                        started.elapsed().as_secs_f64()
                    );
                    results.lock().expect("results").push(m);
                }
            });
        }
    });
    let mut results = results.into_inner().expect("results");
    results.sort_by_key(|m| m.seed);
    report(&results, &content);
    if let Some(path) = &args.json {
        let text = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    println!("\n{:.0} s wall clock", started.elapsed().as_secs_f64());
    Ok(())
}
