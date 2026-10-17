//! `canopy replay <file> --verify` (D60): plays a replay headless to its end, checks every
//! recorded fingerprint and the outcome, and times the playback and a few seeks, so a bug
//! report's replay is reproduced without a window.

use super::{
    format::Replay,
    playback::{Keyframes, Origin, Playback, Script},
};
use crate::paths::Paths;
use anyhow::{Context, Result, anyhow, bail};
use canopy_content::{Content, map::UtcMap};
use canopy_sim::World;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Instant;

/// The content and map a replay plays on, found the way the game finds them: the map by
/// id among the game's maps (checked against the replay's digest), else the package the
/// replay carries.
pub fn resolve(
    paths: &Paths,
    replay: &Replay,
    maps: &[canopy_content::map::MapFile],
) -> Result<std::path::PathBuf> {
    let id = &replay.header.identity.map_id;
    if let Some(file) = maps.iter().find(|m| &m.id == id) {
        let digest = crate::saves::digest(&file.digest().map_err(|e| anyhow!("{e}"))?);
        if digest == replay.header.identity.map_digest {
            return Ok(file.path.clone());
        }
        if replay.body.map.is_none() {
            bail!("the map {id} has changed since the replay was recorded");
        }
    }
    match &replay.body.map {
        Some(package) => super::unpack_map(&replay.header, package, &paths.cache),
        None => bail!("the map {id} is not in this game"),
    }
}

/// Loads the content and the replay's map.
pub fn load(paths: &Paths, replay: &Replay) -> Result<(Arc<Content>, Arc<UtcMap>)> {
    let content = if crate::data::is_packaged(&paths.data) {
        Content::load_packaged(&paths.data)
    } else {
        Content::load(&paths.data)
    }
    .map_err(|e| anyhow!("content: {e}"))?;
    let maps = crate::data::map_files(paths)?;
    let dir = resolve(paths, replay, &maps)?;
    let map = canopy_content::map::package::load(&dir).map_err(|e| anyhow!("{e}"))?;
    Ok((Arc::new(content), Arc::new(map)))
}

/// Runs the check; prints the report and fails when the replay does not reproduce.
pub fn run(paths: &Paths, file: &std::path::Path) -> Result<()> {
    let replay = Replay::read(file)?;
    let h = &replay.header;
    println!(
        "{}: {} on {} ({}), recorded by Under the Canopy {}, {} players, {} commands, {} ({} ticks)",
        file.display(),
        h.mode.name(),
        h.map_name,
        h.identity.map_id,
        h.game_version,
        h.players.len(),
        replay.body.commands.len(),
        super::clock(h.duration()),
        h.duration(),
    );
    let content_digest = crate::data::content_digest(paths)?;
    if content_digest != h.identity.content_digest {
        println!(
            "warning: recorded with other game content (version {}); it will not reproduce",
            h.game_version
        );
    }
    let (content, map) = load(paths, &replay).context("cannot play the replay")?;
    let script = Script::new(&replay.body, h.start_tick, h.end_tick);
    let origin = Origin::new(content.clone(), map.clone(), &replay);
    let keyframes = Keyframes::default();
    let started = Instant::now();
    keyframes.analyse(&origin, &script, &AtomicBool::new(false));
    let took = started.elapsed();
    let a = keyframes.analysis();
    if let Some(error) = &a.failed {
        bail!("the replay could not start: {error}");
    }
    let seconds = took.as_secs_f64();
    println!(
        "played {} ticks in {:.2} s ({:.0} ticks/s, {:.0}× real time); {} fingerprints checked",
        a.analysed,
        seconds,
        f64::from(a.analysed) / seconds.max(1e-9),
        f64::from(a.analysed) * 0.025 / seconds.max(1e-9),
        script.fingerprint_count(),
    );
    println!(
        "keyframes: {} ({} KB compressed)",
        a.keyframes,
        a.keyframe_bytes / 1024
    );
    let mut ok = true;
    match a.desync {
        Some(t) => {
            ok = false;
            println!("OUT OF SYNC from {} (tick {t})", super::clock(t));
        }
        None => println!("fingerprints: all match"),
    }
    let reached = a.outcome.as_ref().map(|(o, _)| {
        let slot = |o: &canopy_content::placement::Owner| match o {
            canopy_content::placement::Owner::Player(p) => Some(*p),
            canopy_content::placement::Owner::Nobody => None,
        };
        super::format::Ending {
            winner: o.winner.as_ref().and_then(slot),
            defeated: o.defeated.iter().filter_map(slot).collect(),
        }
    });
    match (&h.ending, &reached) {
        (Some(want), Some(got)) if want == got => println!("outcome: as recorded ({})", h.result),
        (None, None) => println!("outcome: undecided, as recorded ({})", h.result),
        (want, got) => {
            ok = false;
            println!("OUTCOME DIFFERS: recorded {want:?}, played {got:?}");
        }
    }
    ok &= seek_timings(&replay, content, map)?;
    if !ok {
        bail!("the replay does not reproduce");
    }
    Ok(())
}

/// Records a replay without a player (`canopy replay <out> --record <map> [--ticks n]`):
/// the first start's commands come from a computer player in a second world, sent through
/// a local session exactly as a human's would be, against computer players. For measuring
/// replay sizes and seeks on real matches.
pub fn record_stand_in(
    paths: &Paths,
    map_id: &str,
    ticks: u32,
    out: &std::path::Path,
) -> Result<()> {
    use canopy_content::placement::Owner;
    let content = Arc::new(
        if crate::data::is_packaged(&paths.data) {
            Content::load_packaged(&paths.data)
        } else {
            Content::load(&paths.data)
        }
        .map_err(|e| anyhow!("content: {e}"))?,
    );
    let maps = crate::data::map_files(paths)?;
    let file = maps
        .iter()
        .find(|m| m.id == map_id)
        .with_context(|| format!("no map {map_id}"))?;
    let map = file.load().map_err(|e| anyhow!("{e}"))?;
    let brains = canopy_scene::spectator_setup(&map, None);
    let me = brains.slots[0].player;
    let mut setup = brains.clone();
    setup.slots[0].kind = canopy_sim::SlotKind::Human;
    setup.slots[0].name = Some("Stand-in".into());
    let mut brain = World::new(content.clone(), &map, brains).map_err(|e| anyhow!("{}", e.0))?;
    let mut world =
        World::new(content.clone(), &map, setup.clone()).map_err(|e| anyhow!("{}", e.0))?;
    let mut session = canopy_net::Session::local(setup.clone(), Some(me));
    let commands = session.commands();
    let recorder = super::record::Recorder::new(&world);
    let started = Instant::now();
    let mut decided_at = None;
    while world.tick() < ticks {
        let turn = session
            .advance(&mut world)
            .context("the local session has the next beat")?;
        recorder.stepped(&turn.commands, &world);
        brain.step();
        let next = brain.tick();
        for q in brain
            .pending()
            .iter()
            .filter(|q| q.player == me && q.tick == next)
        {
            commands.send(q.action.clone());
        }
        if decided_at.is_none() && world.state().outcome.is_some() {
            decided_at = Some(world.tick());
        }
        // A decided match plays ten seconds on, then the recording ends.
        if decided_at.is_some_and(|t| world.tick() >= t + 400) {
            break;
        }
    }
    let recording = recorder.snapshot();
    let data_digest = crate::data::content_digest(paths)?;
    let map_digest = crate::saves::digest(&file.digest().map_err(|e| anyhow!("{e}"))?);
    let summary = canopy_content::map::package::summary(&file.path).map_err(|e| anyhow!("{e}"))?;
    let info = super::MatchInfo {
        mode: super::format::Mode::Skirmish,
        map_name: summary.name,
        identity: crate::saves::MatchIdentity {
            map_id: map_id.into(),
            map_digest,
            content_digest: data_digest,
        },
        map_package: None,
        players: setup
            .slots
            .iter()
            .map(|s| super::format::Player {
                slot: s.player,
                name: s
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Computer {}", s.player + 1)),
                computer: s.kind == canopy_sim::SlotKind::Ai,
                race: s.race.clone(),
                hero: s.hero.clone(),
                team: s.team,
                color: usize::from(s.player),
            })
            .collect(),
        recorder: Some(me),
        colors: None,
    };
    let result = super::result_word(recording.outcome.as_ref(), Some(me), &setup, false);
    let replay = super::build(&recording, &info, result, crate::saves::now_ms())?;
    let bytes = replay.to_bytes()?;
    std::fs::write(out, &bytes).with_context(|| format!("cannot write {}", out.display()))?;
    let lost = world
        .state()
        .entities
        .iter()
        .filter(|e| e.owner == Owner::Player(me))
        .count();
    println!(
        "recorded {} ({} ticks, {}) in {:.1} s: {} commands, {} fingerprints, result {result}; {} entities of the stand-in's at the end",
        out.display(),
        recording.tick,
        super::clock(recording.tick),
        started.elapsed().as_secs_f32(),
        recording.commands.len(),
        recording.fingerprints.len(),
        lost
    );
    println!(
        "replay file: {} bytes ({:.1} KB)",
        bytes.len(),
        bytes.len() as f64 / 1024.0
    );
    Ok(())
}

/// Times typical seeks on a viewer's playback (after its analysis has finished).
fn seek_timings(replay: &Replay, content: Arc<Content>, map: Arc<UtcMap>) -> Result<bool> {
    let mut playback = Playback::new(replay, content, map);
    while !playback.analysis().done {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let end = playback.end_tick();
    let start = playback.script.start_tick;
    if end <= start + 1600 {
        return Ok(true);
    }
    let mut world = playback.first_world()?;
    let span = end - start;
    // Play to a little past the middle as a viewer would.
    let middle = start + span / 2 + 137;
    while world.tick() < middle {
        playback.advance(&mut world);
    }
    let mut lines = Vec::new();
    let mut ms = Vec::new();
    let mut seek = |label: &str, target: u32, world: &mut canopy_sim::World| -> Result<()> {
        let s = playback.seek(world, target)?;
        ms.push(s.ms);
        lines.push(format!(
            "  {label:<16} {:>6} → {:>6} (landed {:>6})  {:>6.1} ms  from {:?}, {} ticks simulated",
            super::clock(s.from),
            super::clock(s.target),
            super::clock(s.to),
            s.ms,
            s.source,
            s.ticks
        ));
        Ok(())
    };
    let now = world.tick();
    seek("back 10 s", now.saturating_sub(400), &mut world)?;
    let now = world.tick();
    seek("forward 10 s", now + 400, &mut world)?;
    let now = world.tick();
    seek("forward 2 s", now + 80, &mut world)?;
    for (label, part) in [
        ("to 23 %", 23),
        ("to 71 %", 71),
        ("to 9 %", 9),
        ("to 88 %", 88),
        ("to 47 %", 47),
    ] {
        seek(label, start + span * part / 100 + 61, &mut world)?;
    }
    let now = world.tick();
    seek("back 10 s", now.saturating_sub(400), &mut world)?;
    let a = playback.analysis();
    println!(
        "seeks (this machine; a base M1 is about 1.7× slower; simulation {:.2} ms a tick, keyframes every {} ticks):",
        a.tick_ms, a.spacing
    );
    for line in lines {
        println!("{line}");
    }
    ms.sort_by(f32::total_cmp);
    println!(
        "  median {:.1} ms, slowest {:.1} ms",
        ms[ms.len() / 2],
        ms.last().copied().unwrap_or(0.0)
    );
    // Every seek went through keyframes and rewinds: the rest must still play as recorded.
    while playback.advance(&mut world).is_some() {}
    match playback.desync {
        None => {
            println!("after the seeks, played on to the end: fingerprints all match");
            Ok(true)
        }
        Some(t) => {
            println!("AFTER THE SEEKS, OUT OF SYNC from {}", super::clock(t));
            Ok(false)
        }
    }
}
