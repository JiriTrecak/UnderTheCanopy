//! Playing a replay (D60): the world rebuilt from the replay's start and fed the recorded
//! commands at their ticks; computer players decide again inside the simulation.
//!
//! **Keyframes.** A background thread plays the whole replay headless as soon as it is
//! opened ([`Keyframes::analyse`]): it checks every recorded fingerprint and keeps the
//! world's save (D16) with the observers' tally, compressed, every [`KEYFRAME_TICKS`]
//! (5 s). Long, crowded matches have large saves: past [`KEYFRAME_BUDGET`] bytes every
//! other keyframe goes and the spacing doubles, so memory stays bounded.
//!
//! **Seeking** ([`Playback::seek`]) moves the shown world to a keyframe with
//! `World::rewind`, which keeps the world's navigation and syncs it to the saved state
//! (tens of milliseconds where a full restore takes 0.2–0.7 s), then simulates to the
//! target without presenting. When simulating there would take longer than
//! [`SEEK_BUDGET_MS`] (a busy match costs milliseconds a tick), the seek lands on the
//! keyframe instead: at most one keyframe spacing before the target, which the timeline
//! cannot point at more finely anyway. A target just ahead is simulated from the world
//! as it is.

use super::{
    format::{Body, Replay},
    tally::Tally,
};
use anyhow::{Context, Result, anyhow};
use canopy_content::{Content, map::UtcMap};
use canopy_sim::{MatchSetup, Queued, World};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

/// Ticks between keyframes at first (5 s of match time).
pub const KEYFRAME_TICKS: u32 = 200;
/// Compressed keyframe bytes kept at most before their spacing doubles.
pub const KEYFRAME_BUDGET: usize = 160 << 20;
/// The longest a seek may simulate past its keyframe before it lands on the keyframe.
pub const SEEK_BUDGET_MS: f32 = 200.0;

/// The replay's commands by tick and its fingerprints.
#[derive(Debug, Default)]
pub struct Script {
    /// Ordered by tick, as recorded.
    commands: Vec<Queued>,
    fingerprints: BTreeMap<u32, u32>,
    pub start_tick: u32,
    pub end_tick: u32,
}

impl Script {
    pub fn new(body: &Body, start_tick: u32, end_tick: u32) -> Self {
        let mut commands = body.commands.clone();
        // Recorded in order already; a stable sort keeps each tick's order.
        commands.sort_by_key(|c| c.tick);
        Self {
            commands,
            fingerprints: body.fingerprints.iter().copied().collect(),
            start_tick,
            end_tick,
        }
    }

    /// The commands the world executed at clock `tick`.
    pub fn at(&self, tick: u32) -> &[Queued] {
        let from = self.commands.partition_point(|c| c.tick < tick);
        let to = self.commands.partition_point(|c| c.tick <= tick);
        &self.commands[from..to]
    }

    /// The recorded fingerprint after stepping to `tick`, if one was kept.
    pub fn fingerprint(&self, tick: u32) -> Option<u32> {
        self.fingerprints.get(&tick).copied()
    }

    pub fn fingerprint_count(&self) -> usize {
        self.fingerprints.len()
    }
}

/// Steps `world` once with the script's commands; `Err(tick)` when the world's
/// fingerprint differs from the recorded one there.
pub fn step(world: &mut World, script: &Script) -> Result<(), u32> {
    let tick = world.tick();
    for command in script.at(tick) {
        // Rejected commands are rejected again, identically: the session skipped them too.
        let _ = world.enqueue(command.clone());
    }
    world.step();
    let now = world.tick();
    match script.fingerprint(now) {
        Some(f) if f != world.fingerprint() => Err(now),
        _ => Ok(()),
    }
}

/// What rebuilds the replay's first world.
pub struct Origin {
    pub content: Arc<Content>,
    pub map: Arc<UtcMap>,
    pub setup: MatchSetup,
    pub start: Option<canopy_sim::Save>,
}

impl Origin {
    pub fn new(content: Arc<Content>, map: Arc<UtcMap>, replay: &Replay) -> Self {
        Self {
            content,
            map,
            setup: replay.body.setup.clone(),
            start: replay.body.start.clone(),
        }
    }

    pub fn world(&self) -> Result<World> {
        match &self.start {
            Some(save) => World::restore(self.content.clone(), &self.map, save.clone()),
            None => World::new(self.content.clone(), &self.map, self.setup.clone()),
        }
        .map_err(|e| anyhow!("{}", e.0))
    }
}

/// A world's save, compressed, with the tally at that moment.
pub struct Keyframe {
    pub tick: u32,
    save: Vec<u8>,
    pub tally: Tally,
}

impl Keyframe {
    fn of(world: &World, tally: &Tally) -> Result<Self> {
        let json = serde_json::to_vec(&world.save())?;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&json)?;
        Ok(Self {
            tick: world.tick(),
            save: encoder.finish()?,
            tally: tally.clone(),
        })
    }

    /// The save, decompressed.
    fn save(&self) -> Result<canopy_sim::Save> {
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(self.save.as_slice()).read_to_end(&mut json)?;
        Ok(serde_json::from_slice(&json)?)
    }

    /// Compressed bytes held.
    pub fn bytes(&self) -> usize {
        self.save.len()
    }
}

/// What the background analysis has found so far.
struct Found {
    frames: BTreeMap<u32, Arc<Keyframe>>,
    /// Ticks between keyframes now (doubles as the budget runs out).
    spacing: u32,
    bytes: usize,
    /// The tick the analysis has simulated to.
    analysed: u32,
    /// The first tick whose fingerprint differed.
    desync: Option<u32>,
    /// The outcome the analysis reached and when.
    outcome: Option<(canopy_sim::state::Outcome, u32)>,
    done: bool,
    failed: Option<String>,
    /// How long the analysis has taken, and its simulation alone over `stepped` ticks.
    took: std::time::Duration,
    stepping: std::time::Duration,
    stepped: u32,
}

impl Default for Found {
    fn default() -> Self {
        Self {
            frames: BTreeMap::new(),
            spacing: KEYFRAME_TICKS,
            bytes: 0,
            analysed: 0,
            desync: None,
            outcome: None,
            done: false,
            failed: None,
            took: std::time::Duration::ZERO,
            stepping: std::time::Duration::ZERO,
            stepped: 0,
        }
    }
}

impl Found {
    /// Keeps a keyframe; past the budget every other one goes and the spacing doubles.
    fn keep(&mut self, k: Keyframe, start: u32) {
        self.bytes += k.bytes();
        self.frames.insert(k.tick, Arc::new(k));
        while self.bytes > KEYFRAME_BUDGET && self.spacing < u32::MAX / 4 {
            self.spacing *= 2;
            let spacing = self.spacing;
            self.frames
                .retain(|&t, _| t == start || (t - start).is_multiple_of(spacing));
            self.bytes = self.frames.values().map(|k| k.bytes()).sum();
        }
    }
}

/// Keyframes shared between the analysis thread and the viewer.
#[derive(Default)]
pub struct Keyframes {
    found: Mutex<Found>,
}

/// Where the analysis stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Analysis {
    pub analysed: u32,
    pub desync: Option<u32>,
    pub outcome: Option<(canopy_sim::state::Outcome, u32)>,
    pub done: bool,
    pub failed: Option<String>,
    pub keyframes: usize,
    pub keyframe_bytes: usize,
    /// Ticks between keyframes.
    pub spacing: u32,
    pub took: std::time::Duration,
    /// The simulation's mean cost per tick, milliseconds.
    pub tick_ms: f32,
}

impl Keyframes {
    fn lock(&self) -> std::sync::MutexGuard<'_, Found> {
        self.found.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The newest keyframe at or before `tick`.
    pub fn before(&self, tick: u32) -> Option<Arc<Keyframe>> {
        self.lock()
            .frames
            .range(..=tick)
            .next_back()
            .map(|(_, k)| k.clone())
    }

    pub fn analysis(&self) -> Analysis {
        let f = self.lock();
        Analysis {
            analysed: f.analysed,
            desync: f.desync,
            outcome: f.outcome.clone(),
            done: f.done,
            failed: f.failed.clone(),
            keyframes: f.frames.len(),
            keyframe_bytes: f.bytes,
            spacing: f.spacing,
            took: f.took,
            tick_ms: if f.stepped > 0 {
                f.stepping.as_secs_f32() * 1000.0 / f.stepped as f32
            } else {
                0.5
            },
        }
    }

    /// Plays the whole replay headless from `origin` (on the calling thread): checks the
    /// fingerprints, keeps keyframes, stops early when `stop`.
    pub fn analyse(&self, origin: &Origin, script: &Script, stop: &AtomicBool) {
        let started = Instant::now();
        let mut world = match origin.world() {
            Ok(w) => w,
            Err(e) => {
                let mut f = self.lock();
                f.failed = Some(format!("{e:#}"));
                f.done = true;
                return;
            }
        };
        let start = world.tick();
        let mut tally = Tally::default();
        if let Ok(k) = Keyframe::of(&world, &tally) {
            self.lock().keep(k, start);
        }
        let mut spacing = KEYFRAME_TICKS;
        while world.tick() < script.end_tick && !stop.load(Ordering::Relaxed) {
            let begun = Instant::now();
            let mismatch = step(&mut world, script).err();
            let took = begun.elapsed();
            tally.observe(&world);
            let tick = world.tick();
            let keyframe = (tick - start)
                .is_multiple_of(spacing)
                .then(|| Keyframe::of(&world, &tally).ok())
                .flatten();
            let mut f = self.lock();
            f.analysed = tick;
            f.stepping += took;
            f.stepped += 1;
            if let Some(t) = mismatch {
                f.desync.get_or_insert(t);
            }
            if f.outcome.is_none()
                && let Some(o) = &world.state().outcome
            {
                f.outcome = Some((o.clone(), tick));
            }
            if let Some(k) = keyframe {
                f.keep(k, start);
                spacing = f.spacing;
            }
            f.took = started.elapsed();
        }
        let mut f = self.lock();
        f.done = true;
        f.took = started.elapsed();
    }
}

/// How a seek went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seeked {
    pub from: u32,
    /// Where it was asked to go, and where it went.
    pub target: u32,
    pub to: u32,
    /// Where the simulation started: the current world, a keyframe or the start.
    pub source: Source,
    pub ticks: u32,
    pub ms: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Current,
    Keyframe,
    Start,
}

/// A replay being watched: the script, the keyframes and the tally of the world shown.
pub struct Playback {
    pub script: Arc<Script>,
    origin: Arc<Origin>,
    keyframes: Arc<Keyframes>,
    stop: Arc<AtomicBool>,
    analysis: Option<std::thread::JoinHandle<()>>,
    pub tally: Tally,
    /// The first tick where the shown world left the recorded match.
    pub desync: Option<u32>,
    /// What moving to a keyframe took last, milliseconds.
    rewind_ms: f32,
}

impl Playback {
    /// Opens `replay` on `content` and `map` and starts the background analysis.
    pub fn new(replay: &Replay, content: Arc<Content>, map: Arc<UtcMap>) -> Self {
        let script = Arc::new(Script::new(
            &replay.body,
            replay.header.start_tick,
            replay.header.end_tick,
        ));
        let origin = Arc::new(Origin::new(content, map, replay));
        let keyframes = Arc::new(Keyframes::default());
        let stop = Arc::new(AtomicBool::new(false));
        let analysis = {
            let (origin, script, keyframes, stop) = (
                origin.clone(),
                script.clone(),
                keyframes.clone(),
                stop.clone(),
            );
            std::thread::Builder::new()
                .name("replay-analysis".into())
                .spawn(move || {
                    let _span = tracing::info_span!("replay_analysis").entered();
                    keyframes.analyse(&origin, &script, &stop);
                    let a = keyframes.analysis();
                    tracing::info!(
                        ticks = a.analysed,
                        keyframes = a.keyframes,
                        keyframe_kb = a.keyframe_bytes / 1024,
                        spacing = a.spacing,
                        ms = a.took.as_millis() as u64,
                        tick_ms = a.tick_ms,
                        desync = ?a.desync,
                        "replay analysed"
                    );
                })
                .ok()
        };
        Self {
            script,
            origin,
            keyframes,
            stop,
            analysis,
            tally: Tally::default(),
            desync: None,
            rewind_ms: 50.0,
        }
    }

    /// The replay's first world (the loader builds it once).
    pub fn first_world(&self) -> Result<World> {
        self.origin.world()
    }

    pub fn end_tick(&self) -> u32 {
        self.script.end_tick
    }

    pub fn analysis(&self) -> Analysis {
        self.keyframes.analysis()
    }

    /// Steps the world to its next tick with the recorded commands; `None` at the end.
    pub fn advance(&mut self, world: &mut World) -> Option<()> {
        if world.tick() >= self.script.end_tick {
            return None;
        }
        if let Err(tick) = step(world, &self.script) {
            if self.desync.is_none() {
                tracing::warn!(tick, "the replay is out of sync");
            }
            self.desync.get_or_insert(tick);
        }
        self.tally.observe(world);
        Some(())
    }

    /// Moves `world` to `target` (clamped to the replay), or to the keyframe before it
    /// when simulating the rest would take too long (see the module).
    pub fn seek(&mut self, world: &mut World, target: u32) -> Result<Seeked> {
        let started = Instant::now();
        let from = world.tick();
        let target = target.clamp(self.script.start_tick, self.script.end_tick);
        let _span = tracing::info_span!("replay_seek", from, target).entered();
        let tick_ms = self.keyframes.analysis().tick_ms.max(0.01);
        let keyframe = self.keyframes.before(target);
        // The exact ways there and what they cost, milliseconds.
        let current = (from <= target).then(|| (target - from) as f32 * tick_ms);
        let via_keyframe = keyframe
            .as_ref()
            .map(|k| (k.tick, self.rewind_ms + (target - k.tick) as f32 * tick_ms));
        let (source, land) = match (current, via_keyframe) {
            (Some(c), via) if via.is_none_or(|(_, k)| c <= k) => (Source::Current, target),
            (_, Some((_, k))) if k <= SEEK_BUDGET_MS => (Source::Keyframe, target),
            // Too far to simulate: land on the keyframe, unless the world is already
            // between it and the target (then the world is the nearer start).
            (Some(_), Some((at, _))) if at <= from => (Source::Current, target),
            (_, Some((at, _))) => (Source::Keyframe, at),
            (Some(_), None) => (Source::Current, target),
            (None, None) => (Source::Start, target),
        };
        match source {
            Source::Current => {}
            Source::Keyframe => {
                let k = keyframe.context("a keyframe")?;
                let begun = Instant::now();
                world.rewind(k.save()?).map_err(|e| anyhow!("{}", e.0))?;
                self.rewind_ms = begun.elapsed().as_secs_f32() * 1000.0;
                self.tally = k.tally.clone();
                self.tally.restored();
            }
            Source::Start => {
                *world = self.origin.world()?;
                self.tally = Tally::default();
            }
        }
        let begun = world.tick();
        while world.tick() < land {
            if let Err(tick) = step(world, &self.script) {
                self.desync.get_or_insert(tick);
            }
            self.tally.observe(world);
        }
        let seeked = Seeked {
            from,
            target,
            to: world.tick(),
            source,
            ticks: world.tick() - begun,
            ms: started.elapsed().as_secs_f32() * 1000.0,
        };
        tracing::info!(
            from,
            target,
            to = seeked.to,
            source = ?seeked.source,
            ticks = seeked.ticks,
            ms = seeked.ms,
            "replay seek"
        );
        Ok(seeked)
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.analysis.take() {
            let _ = handle.join();
        }
    }
}
