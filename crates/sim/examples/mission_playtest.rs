//! Plays a mission map headless with player 1 as the skirmish computer player, held to the
//! mission's tech, and reports how the mission goes: its lines as they are said, its
//! objectives, both sides' strength every so often, the script's errors, and the outcome.
//! A pacing check for mission authors, not a test: a skirmish AI is no human, but if it can
//! build up, hold the waves and win, the mission is winnable and its waves are fair.
//!
//! ```text
//! cargo run --release -p canopy-sim --example mission_playtest -- --data . \
//!     --map base/campaigns/<campaign>.utcc/missions/<map>.utcm [--minutes 30] \
//!     [--seed 1] [--tech id,id,…] [--style standard|rush|expand|tech] [--every 60]
//! ```
//!
//! `--push 12:322,86;20:88,92` sends player 1's army (every unit that fights, heroes too)
//! attack-moving to a cell from a minute on, the next push replacing it: the skirmish AI
//! does not look for a mission's enemies, so the pushes stand in for the player's attacks.
//!
//! `--click alate:3@1` has player 1 click the placed unit `alate` three times at minute 1
//! (easter eggs, `mission.clicks`), and `--skip 0:20` skips the scene playing at 0:20.
//!
//! `--reach from:x,y;name:x,y;…` only checks the ground: which named cells a ground unit
//! can walk to from the first (terrain, water, cliffs and scenery; buildings, rubble and
//! units left out), then exits.
//!
//! `--tech` lists what player 1 may build, train and research (the campaign's tech up to the
//! mission); everything else of its race is locked, as the campaign would lock it. Without
//! it nothing is locked. `--human` seats player 1 as a human who does nothing, to see what
//! the waves do to an idle colony.
#![allow(clippy::disallowed_methods)] // wall-clock timing of the run

use canopy_content::{Content, Id, map};
use canopy_sim::{MatchSetup, Slot, SlotKind, World, campaign::CampaignStart};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Instant};

const TICKS_PER_SECOND: u32 = 40;

struct Args {
    data: PathBuf,
    map: PathBuf,
    minutes: u32,
    seed: u32,
    tech: Option<Vec<String>>,
    style: Option<canopy_sim::ai::AiStyle>,
    every: u32,
    human: bool,
    pushes: Vec<(u32, u32, u32)>,
    reach: Vec<(String, i64, i64)>,
    clicks: Vec<(String, u32, u32)>,
    skips: Vec<u32>,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        data: ".".into(),
        map: PathBuf::new(),
        minutes: 30,
        seed: 1,
        tech: None,
        style: None,
        every: 60,
        human: false,
        pushes: Vec::new(),
        reach: Vec::new(),
        clicks: Vec::new(),
        skips: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--data" => out.data = value()?.into(),
            "--map" => out.map = value()?.into(),
            "--minutes" => out.minutes = value()?.parse().map_err(|e| format!("--minutes: {e}"))?,
            "--seed" => out.seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--every" => out.every = value()?.parse().map_err(|e| format!("--every: {e}"))?,
            "--tech" => {
                out.tech = Some(value()?.split(',').map(|s| s.trim().to_string()).collect())
            }
            "--style" => {
                let v = value()?;
                out.style = Some(
                    canopy_sim::ai::AiStyle::parse(&v).ok_or(format!("--style {v}: unknown"))?,
                );
            }
            "--human" => out.human = true,
            "--click" => {
                let v = value()?;
                let parse = || -> Option<(String, u32, u32)> {
                    let (id, rest) = v.split_once(':')?;
                    let (times, minute) = rest.split_once('@')?;
                    Some((id.to_string(), times.parse().ok()?, minute.parse().ok()?))
                };
                out.clicks
                    .push(parse().ok_or(format!("--click {v}: id:times@minute"))?);
            }
            "--skip" => {
                let v = value()?;
                let (m, sec) = v.split_once(':').ok_or("--skip m:ss")?;
                let at = m.parse::<u32>().map_err(|e| e.to_string())? * 60
                    + sec.parse::<u32>().map_err(|e| e.to_string())?;
                out.skips.push(at * TICKS_PER_SECOND);
            }
            "--reach" => {
                for part in value()?.split(';') {
                    let parse = || -> Option<(String, i64, i64)> {
                        let (name, cell) = part.split_once(':')?;
                        let (x, y) = cell.split_once(',')?;
                        Some((
                            name.trim().to_string(),
                            x.trim().parse().ok()?,
                            y.trim().parse().ok()?,
                        ))
                    };
                    out.reach
                        .push(parse().ok_or(format!("--reach {part}: name:x,y"))?);
                }
            }
            "--push" => {
                for part in value()?.split(';') {
                    let parse = || -> Option<(u32, u32, u32)> {
                        let (minute, cell) = part.split_once(':')?;
                        let (x, y) = cell.split_once(',')?;
                        Some((
                            minute.trim().parse().ok()?,
                            x.trim().parse().ok()?,
                            y.trim().parse().ok()?,
                        ))
                    };
                    out.pushes
                        .push(parse().ok_or(format!("--push {part}: minute:x,y"))?);
                }
                out.pushes.sort();
            }
            other => return Err(format!("unknown flag {other}")),
        }
    }
    if out.map.as_os_str().is_empty() {
        return Err("--map <package path> is required".into());
    }
    Ok(out)
}

fn main() {
    let args = args().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let content = Arc::new(Content::load(&args.data).unwrap_or_else(|e| panic!("content: {e}")));
    let map = map::package::load(&args.map).unwrap_or_else(|e| panic!("{e}"));
    let race = content.rules.default_race.clone();
    let locked: std::collections::BTreeSet<Id> = match &args.tech {
        Some(allowed) => canopy_content::campaign::race_tech(&content, &race)
            .into_iter()
            .filter(|id| !allowed.iter().any(|a| a == id.as_str()))
            .collect(),
        None => Default::default(),
    };
    let setup = MatchSetup {
        seed: args.seed,
        slots: vec![Slot {
            player: 0,
            kind: if args.human {
                SlotKind::Human
            } else {
                SlotKind::Ai
            },
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    };
    let start = CampaignStart {
        player: 0,
        locked: locked.clone(),
        heroes: Vec::new(),
        hero_level_cap: None,
        variables: BTreeMap::new(),
    };
    let mut world =
        World::new_campaign(content.clone(), &map, setup, &start).unwrap_or_else(|e| panic!("{e}"));
    if !args.human {
        world.set_ai_tech(0, locked.iter().cloned().collect::<Vec<Id>>());
        if let Some(style) = args.style {
            world.set_ai_style(0, style);
        }
    }
    if !args.reach.is_empty() {
        reach(&world, &args.reach);
        return;
    }
    let started = Instant::now();
    let end = args.minutes * 60 * TICKS_PER_SECOND;
    let mut said = u32::MAX;
    let mut objectives = BTreeMap::new();
    let mut error = None;
    let mut seq = 0;
    while world.tick() < end && world.state().outcome.is_none() {
        let now = world.tick();
        if now.is_multiple_of(2 * TICKS_PER_SECOND)
            && let Some(&(_, x, y)) = args
                .pushes
                .iter()
                .rev()
                .find(|p| p.0 * 60 * TICKS_PER_SECOND <= now)
        {
            let catalog = world.catalog();
            let army: Vec<u32> = world
                .state()
                .entities
                .iter()
                .filter(|e| {
                    e.owner == canopy_content::placement::Owner::Player(0)
                        && e.unit.is_some()
                        && e.hp.is_some_and(|hp| hp > 0)
                        && catalog.get(e.definition).behaviors.combat.is_some()
                        && catalog.get(e.definition).behaviors.work.is_none()
                })
                .map(|e| e.id.0)
                .collect();
            for actors in army.chunks(canopy_sim::action::MAX_ACTORS) {
                seq += 1;
                let _ = world.enqueue(canopy_sim::Queued {
                    tick: now + 1,
                    player: 0,
                    seq,
                    action: canopy_sim::action::Action::Move {
                        actors: actors.to_vec(),
                        destination: canopy_sim::action::CellPoint {
                            x,
                            y,
                            surface: None,
                        },
                        attack_move: Some(true),
                        append: None,
                    },
                });
            }
        }
        for (id, times, minute) in &args.clicks {
            if now == minute * 60 * TICKS_PER_SECOND {
                let actor = world
                    .state()
                    .entities
                    .iter()
                    .find(|e| e.placement.as_deref() == Some(id.as_str()))
                    .map(|e| e.id.0);
                for _ in 0..*times {
                    seq += 1;
                    if let Some(actor) = actor
                        && let Err(e) = world.enqueue(canopy_sim::Queued {
                            tick: now + 1,
                            player: 0,
                            seq,
                            action: canopy_sim::action::Action::Clicked { actor },
                        })
                    {
                        println!("click refused: {e}");
                    }
                }
            }
        }
        if args.skips.contains(&now) {
            seq += 1;
            let _ = world.enqueue(canopy_sim::Queued {
                tick: now + 1,
                player: 0,
                seq,
                action: canopy_sim::action::Action::SkipScene,
            });
        }
        world.step();
        let state = world.state();
        let clock = clock(state.tick);
        if let Some(mission) = &state.mission {
            if let Some(line) = mission.dialogue.as_ref().filter(|d| d.id != said) {
                said = line.id;
                println!("{clock}  {}: {}", line.speaker, line.text);
            }
            for (id, objective) in &mission.objective_states {
                if objectives.get(id) != Some(objective) {
                    println!("{clock}  objective {id}: {objective:?}");
                    objectives.insert(id.clone(), *objective);
                }
            }
            if mission.error != error {
                error = mission.error.clone();
                if let Some(e) = &error {
                    println!("{clock}  SCRIPT ERROR: {e}");
                }
            }
        }
        if state.tick.is_multiple_of(args.every * TICKS_PER_SECOND) {
            report(&world);
        }
    }
    report(&world);
    let state = world.state();
    if let Some(mission) = state.mission.as_ref().filter(|m| !m.clicks.is_empty()) {
        println!("        clicks {:?}", mission.clicks);
    }
    match &state.outcome {
        Some(outcome) => println!(
            "{}  outcome: winner {:?}, defeated {:?}",
            clock(state.tick),
            outcome.winner,
            outcome.defeated
        ),
        None => println!("{}  no outcome", clock(state.tick)),
    }
    eprintln!("({:.1} s)", started.elapsed().as_secs_f64());
}

/// Flood-fills the ground from the first point and says which of the others it reaches.
fn reach(world: &World, points: &[(String, i64, i64)]) {
    let terrain = world.terrain();
    let size = terrain.size() as i64;
    let steep = terrain.steep_edges();
    let mut seen = vec![false; (size * size) as usize];
    let (_, sx, sy) = &points[0];
    let mut stack = vec![(*sx, *sy)];
    while let Some((x, y)) = stack.pop() {
        let Some(cell) = terrain.cell(x, y) else {
            continue;
        };
        if seen[cell] || !terrain.walkable(cell) {
            continue;
        }
        seen[cell] = true;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= size || ny >= size {
                continue;
            }
            if steep.entered(nx as usize, ny as usize, dx as i32, dy as i32) {
                continue;
            }
            stack.push((nx, ny));
        }
    }
    let total = seen.iter().filter(|&&s| s).count();
    println!("{total} cells reachable from {} ({sx},{sy})", points[0].0);
    for (name, x, y) in &points[1..] {
        let ok = terrain.cell(*x, *y).is_some_and(|c| seen[c]);
        println!(
            "  {name:12} {x:4},{y:<4} {}",
            if ok { "reached" } else { "NOT REACHED" }
        );
    }
}

fn clock(tick: u32) -> String {
    let s = tick / TICKS_PER_SECOND;
    format!("{:2}:{:02}", s / 60, s % 60)
}

/// Each player's living units and buildings by definition, and player 1's goods.
fn report(world: &World) {
    let state = world.state();
    let catalog = world.catalog();
    let mut players: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for e in state
        .entities
        .iter()
        .filter(|e| e.hp.is_some_and(|hp| hp > 0))
    {
        let owner = format!("{:?}", e.owner);
        if owner.contains("Nobody") {
            continue;
        }
        let id = catalog.id(e.definition).as_str();
        let short = id.rsplit('.').next().unwrap_or(id).to_string();
        let building = e.construction.is_some();
        *players
            .entry(owner)
            .or_default()
            .entry(if building { format!("{short}*") } else { short })
            .or_default() += 1;
    }
    println!("{}  ---", clock(state.tick));
    for (owner, counts) in players {
        let list: Vec<String> = counts.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("        {owner}: {}", list.join(", "));
    }
    if std::env::var("PLAYTEST_WHERE").is_ok() {
        for e in state
            .entities
            .iter()
            .filter(|e| e.hp.is_some_and(|hp| hp > 0))
        {
            let owner = format!("{:?}", e.owner);
            if owner.contains("Nobody") || (e.unit.is_none() && e.construction.is_none()) {
                continue;
            }
            let id = catalog.id(e.definition).as_str();
            println!(
                "          {owner} {id} at {},{}{}",
                e.position.x / 1000,
                e.position.y / 1000,
                if e.construction.is_some() {
                    " (site)"
                } else {
                    ""
                }
            );
        }
    }
    if let Some(wallet) = state.wallets.iter().next() {
        println!("        wallet {:?}", wallet);
    }
}
