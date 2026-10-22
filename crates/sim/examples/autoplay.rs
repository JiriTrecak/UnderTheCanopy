//! Plays a mission headless with its autoplay plan (`autoplay.lua`, docs/campaign.md
//! "Autoplay") steering player 1, as its campaign starts it (the tech up to the mission,
//! the hero level cap; no carried heroes), and prints the plan's lines, the objectives,
//! the time and the outcome. Scenes play out in full (the plan waits through them).
//!
//! ```text
//! just autoplay hollow-brood               # a mission of a shipped campaign, by map or step id
//! just autoplay path/to/mission.utcm       # any mission map with a plan
//! just campaign-test                       # every shipped mission with a plan (--all)
//! ```
//!
//! Options: `--seed <n>` (1), `--minutes <n>` (60, the limit), `--talk` (the mission's
//! lines too), `--plan <file>` (play another plan on the map, to try changes), `--verify` (replays the run's commands in a fresh match and compares the
//! fingerprints). Exits non-zero unless every mission played is won.
#![allow(clippy::disallowed_methods)] // wall-clock timing of the run

use canopy_content::{
    Content,
    campaign::CampaignPackage,
    map::{UtcMap, package},
    placement::Owner,
};
use canopy_sim::{
    MatchSetup, Queued, Slot, SlotKind, World,
    autoplay::{Autoplay, Status},
    campaign::CampaignStart,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

const SECOND: u32 = 40;

struct Args {
    data: PathBuf,
    missions: Vec<String>,
    all: bool,
    seed: u32,
    minutes: u32,
    talk: bool,
    verify: bool,
    /// A plan file to play instead of the map's own (trying changes).
    plan: Option<PathBuf>,
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        data: ".".into(),
        missions: Vec::new(),
        all: false,
        seed: 1,
        minutes: 60,
        talk: false,
        verify: false,
        plan: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--data" => out.data = value()?.into(),
            "--seed" => out.seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--minutes" => out.minutes = value()?.parse().map_err(|e| format!("--minutes: {e}"))?,
            "--all" => out.all = true,
            "--talk" => out.talk = true,
            "--verify" => out.verify = true,
            "--plan" => out.plan = Some(value()?.into()),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            mission => out.missions.push(mission.to_string()),
        }
    }
    if out.missions.is_empty() && !out.all {
        return Err("name a mission (its map or step id, or a .utcm path), or --all".into());
    }
    Ok(out)
}

/// A mission to play: its map, and the campaign step it is when it is one.
struct Mission {
    label: String,
    path: PathBuf,
    map: UtcMap,
    campaign: Option<(CampaignPackage, usize)>,
}

fn clock(tick: u32) -> String {
    let s = tick / SECOND;
    format!("{:2}:{:02}", s / 60, s % 60)
}

fn find(campaigns: &[CampaignPackage], name: &str) -> Result<Mission, String> {
    let as_path = Path::new(name);
    let by_path = as_path.extension().is_some_and(|e| e == "utcm");
    for c in campaigns {
        for (index, step) in c.campaign.steps.iter().enumerate() {
            let Some(mission) = step.mission() else {
                continue;
            };
            let Some(file) = c.mission_map(mission.map.as_str()) else {
                continue;
            };
            let matches = if by_path {
                as_path
                    .canonicalize()
                    .ok()
                    .zip(file.path.canonicalize().ok())
                    .is_some_and(|(a, b)| a == b)
            } else {
                name == mission.map.as_str() || name == step.id().as_str()
            };
            if matches {
                let map = package::load(&file.path).map_err(|e| e.to_string())?;
                return Ok(Mission {
                    label: format!(
                        "{} ({}, step {} of {})",
                        step.title(),
                        file.id,
                        index + 1,
                        c.id()
                    ),
                    path: file.path.clone(),
                    map,
                    campaign: Some((c.clone(), index)),
                });
            }
        }
    }
    if by_path {
        let map = package::load(as_path).map_err(|e| e.to_string())?;
        return Ok(Mission {
            label: name.to_string(),
            path: as_path.to_path_buf(),
            map,
            campaign: None,
        });
    }
    Err(format!("no mission {name} in the shipped campaigns"))
}

/// The content the mission plays on: the base's, or with its campaign and map layered.
fn content(data: &Path, base: &Arc<Content>, mission: &Mission) -> Result<Arc<Content>, String> {
    let Some((campaign, _)) = &mission.campaign else {
        return Ok(base.clone());
    };
    let map = canopy_content::package::Package::open(&mission.path).map_err(|e| e.to_string())?;
    if !campaign.has_content() && !canopy_content::campaign::has_content(&map) {
        return Ok(base.clone());
    }
    let root = canopy_content::package::Package::open(&data.join(canopy_content::BASE_DIR))
        .map_err(|e| e.to_string())?;
    Content::from_packages(
        &[root, campaign.package.clone(), map],
        canopy_content::assets::Files::Check,
    )
    .map(Arc::new)
    .map_err(|e| e.to_string())
}

struct Played {
    won: bool,
    tick: u32,
    seconds: f64,
}

fn play(args: &Args, base: &Arc<Content>, mission: &Mission) -> Result<Played, String> {
    let plan = match &args.plan {
        Some(file) => {
            std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?
        }
        None => mission
            .map
            .mission
            .as_ref()
            .and_then(|m| m.autoplay.clone())
            .ok_or("the map has no autoplay plan (autoplay.lua)")?,
    };
    let content = content(&args.data, base, mission)?;
    let (race, start) = match &mission.campaign {
        Some((c, index)) => (
            Some(c.campaign.race.clone()),
            CampaignStart {
                player: 0,
                locked: c.campaign.locked(&content, *index),
                heroes: Vec::new(),
                hero_level_cap: c.campaign.steps[*index]
                    .mission()
                    .and_then(|m| m.hero_level_cap),
                variables: BTreeMap::new(),
            },
        ),
        None => (None, CampaignStart::default()),
    };
    let mut slots = vec![Slot {
        player: 0,
        kind: SlotKind::Human,
        name: None,
        team: None,
        hero: None,
        race,
    }];
    for start in mission.map.player_starts.iter().filter(|s| s.player > 1) {
        slots.push(Slot {
            player: start.player - 1,
            kind: SlotKind::Ai,
            name: None,
            team: None,
            hero: None,
            race: None,
        });
    }
    let setup = MatchSetup {
        seed: args.seed,
        slots,
    };
    let map = Arc::new(mission.map.clone());
    let mut world = World::new_campaign(content.clone(), &map, setup.clone(), &start)
        .map_err(|e| e.to_string())?;
    let mut autoplay = Autoplay::new(&plan, map.clone())?;
    println!("{} · seed {}", mission.label, args.seed);
    let started = Instant::now();
    let limit = args.minutes * 60 * SECOND;
    let mut sent = Vec::new();
    let mut said = u32::MAX;
    let mut objectives = BTreeMap::new();
    let mut error = None;
    let mut refused = (0u32, String::new(), 0u32);
    while world.tick() < limit && world.state().outcome.is_none() {
        world.step();
        let tick = world.tick();
        for (seq, action) in autoplay.tick(&mut world).into_iter().enumerate() {
            let command = Queued {
                tick,
                player: 0,
                seq: seq as u32,
                action,
            };
            if std::env::var_os("AUTOPLAY_COMMANDS").is_some() {
                println!("{}  {:?}", clock(tick), command.action);
            }
            if let Err(e) = world.enqueue(command.clone()) {
                println!("{}  command refused: {e}", clock(tick));
            }
            sent.push(command);
        }
        for line in autoplay.drain_lines() {
            println!("{line}");
        }
        let state = world.state();
        if let Ok(watch) = std::env::var("AUTOPLAY_WATCH")
            && state.tick.is_multiple_of(10 * SECOND)
            && let Some(e) = state
                .entities
                .iter()
                .find(|e| e.placement.as_deref() == Some(watch.as_str()))
        {
            let u = e.unit.as_ref();
            println!(
                "{}  {watch} at {:.1},{:.1} hp {:?} order {:?} route {} settled {:?} contained {:?} statuses {} tunnel {:?}",
                clock(state.tick),
                f64::from(e.position.x) / 1000.0,
                f64::from(e.position.y) / 1000.0,
                e.hp,
                u.and_then(|u| u.order.as_ref()),
                u.map_or(0, |u| u.route.len()),
                u.map(|u| u.settled),
                u.and_then(|u| u.contained),
                e.statuses.len(),
                u.and_then(|u| u.tunnel.as_ref())
            );
        }
        // Refused commands, each new reason once in a row.
        let after = refused.0;
        for fact in state.facts.iter().filter(|f| {
            f.id > after
                && f.owner == Owner::Player(0)
                && f.kind == canopy_sim::state::FactKind::Error
        }) {
            // Each reason once in a row, or again after ten seconds.
            if fact.message != refused.1 || fact.tick >= refused.2 + 10 * SECOND {
                println!("{}  refused: {}", clock(state.tick), fact.message);
                refused.2 = fact.tick;
                if std::env::var_os("AUTOPLAY_COMMANDS").is_some() {
                    for e in state.entities.iter().filter(|e| {
                        e.owner == Owner::Player(0)
                            && e.unit.as_ref().is_some_and(|u| u.order.is_none())
                            && e.hp.is_some_and(|h| h > 0)
                    }) {
                        println!(
                            "          idle {} {:?} at {},{}",
                            e.id.0,
                            e.placement,
                            e.position.x / 1000,
                            e.position.y / 1000
                        );
                    }
                }
            }
            refused = (fact.id, fact.message.clone(), refused.2);
        }
        if let Some(m) = &state.mission {
            if args.talk
                && let Some(line) = m.dialogue.as_ref().filter(|d| d.id != said)
            {
                said = line.id;
                println!("{}    {}: {}", clock(state.tick), line.speaker, line.text);
            }
            for (id, objective) in &m.objective_states {
                if objectives.get(id) != Some(objective) {
                    println!("{}  objective {id}: {objective:?}", clock(state.tick));
                    objectives.insert(id.clone(), *objective);
                }
            }
            if m.error != error {
                error = m.error.clone();
                if let Some(e) = &error {
                    println!("{}  MISSION SCRIPT ERROR: {e}", clock(state.tick));
                }
            }
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    let state = world.state();
    let won = state
        .outcome
        .as_ref()
        .is_some_and(|o| o.winner == Some(Owner::Player(0)));
    let army = state
        .entities
        .iter()
        .filter(|e| e.owner == Owner::Player(0) && e.unit.is_some() && e.hp.is_some_and(|h| h > 0))
        .count();
    let outcome = match &state.outcome {
        Some(_) if won => "victory".to_string(),
        Some(o) => format!("defeat ({:?})", o.defeated),
        None => format!("no outcome after {} minutes", args.minutes),
    };
    let plan_state = match autoplay.status() {
        Status::Running => "running".to_string(),
        Status::Finished => "done".to_string(),
        Status::Failed(e) => format!("failed: {e}"),
    };
    println!(
        "{}  {outcome} · {} units left · plan {plan_state} · {} commands · {seconds:.1} s",
        clock(state.tick),
        army,
        sent.len(),
    );
    if args.verify {
        let mut again =
            World::new_campaign(content, &map, setup, &start).map_err(|e| e.to_string())?;
        for command in sent {
            while again.tick() < command.tick {
                again.step();
            }
            let _ = again.enqueue(command);
        }
        while again.tick() < world.tick() {
            again.step();
        }
        let same = again.fingerprint() == world.fingerprint();
        println!(
            "        replay of the commands alone: {}",
            if same { "the same match" } else { "DIFFERS" }
        );
        if !same {
            return Err("the replay of the run's commands differs".into());
        }
    }
    Ok(Played {
        won,
        tick: state.tick,
        seconds,
    })
}

fn main() {
    let args = args().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let base = Arc::new(Content::load(&args.data).unwrap_or_else(|e| panic!("content: {e}")));
    let campaigns =
        canopy_content::campaign::library(&args.data).unwrap_or_else(|e| panic!("campaigns: {e}"));
    let mut names = args.missions.clone();
    if args.all {
        for c in &campaigns {
            for step in &c.campaign.steps {
                if let Some(m) = step.mission()
                    && c.mission_map(m.map.as_str())
                        .is_some_and(|f| f.path.join(package::AUTOPLAY_SCRIPT).is_file())
                {
                    names.push(m.map.to_string());
                }
            }
        }
    }
    let mut results = Vec::new();
    for name in &names {
        let result = find(&campaigns, name).and_then(|m| play(&args, &base, &m));
        if let Err(e) = &result {
            println!("{name}: {e}");
        }
        println!();
        results.push((name.clone(), result));
    }
    if results.len() > 1 {
        println!("mission              outcome   game time   run");
        for (name, result) in &results {
            match result {
                Ok(p) => println!(
                    "{name:<20} {:<9} {:>9}   {:>5.1} s",
                    if p.won { "victory" } else { "LOST" },
                    clock(p.tick).trim(),
                    p.seconds
                ),
                Err(_) => println!("{name:<20} ERROR"),
            }
        }
    }
    if !results.iter().all(|(_, r)| r.as_ref().is_ok_and(|p| p.won)) {
        std::process::exit(1);
    }
}
