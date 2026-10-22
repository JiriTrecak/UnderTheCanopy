//! `canopy shot --play --map <mission> --scene <name> --out <dir> [--sheet] [--compare <rev>]
//! [--set <variable>=<value>]…`: plays one of a mission's scenes from its start, at normal
//! speed, and writes a picture of each spoken line (docs/campaign.md "Scenes"; skill
//! `canopy-scenes`, "Seeing it"). `--scene list` prints the mission's scenes; `--scene all`
//! plays each into its own folder of `--out`, and `--sheet` adds one sheet of them all.
//!
//! The opening (the scene `on_start` plays) plays as a new mission starts. Any other scene
//! plays mid-mission, so the preview stages it as the mission would reach it: the opening
//! is skipped, and then
//! - the script-activated units the scene names, and those the script spawns just before
//!   it plays the scene, appear;
//! - the variables the script sets just before it plays the scene are set, and the ones
//!   the scene reads but nothing set get a default: a unit's position for the variables
//!   the script fills from `mission.position`, else `true` (the scene's main branch);
//!   `--set` overrides any;
//! - the objectives the scene completes or fails are active;
//! - a dry run of the scene up to its first line learns where it walks its cast; each
//!   unit then starts a few cells short of that mark (or on it), so the scene's walks are
//!   short and its waits end as they would.
//!
//! The map shows explored, as to a player who walked the mission. Frames:
//! `00-establishing.png` once the scene's first shot has faded in, then `NN-<speaker>.png`
//! a moment after each line starts, and `lines.txt` listing them (tick, speaker, text,
//! camera). `--sheet` writes `sheet.png`, a labelled contact sheet; `--compare <rev>`
//! plays the same scene from the map as it was at Git revision `rev` into `before/` and
//! writes `pair-NN.png`, before on the left.

mod lua;
mod sheet;

use super::capture::Rig;
use crate::{options::Options, paths::Paths, world};
use anyhow::{Context, Result, anyhow, bail};
use canopy_content::placement::Activation;
use canopy_sim::{
    World,
    entity::Milli,
    mission::{
        CAMPAIGN_PLAYER, MissionEffect, MissionState,
        cinema::{Pose, SceneMove, ScenePlay},
        state::{ObjectiveState, Variable},
    },
    spatial::Spot,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

/// A line's picture is taken this long after it starts, ticks (0.75 s).
const LINE_DELAY: u32 = 30;
/// The establishing picture: at least this long into the scene, once any fade-in is
/// nearly over, and at most this long in (ticks).
const ESTABLISH: (u32, u32) = (40, 200);
/// Fade opacity (thousandths) below which the picture counts as faded in.
const FADED_IN: u16 = 80;
/// The longest scene a preview plays, ticks (five minutes).
const LONGEST: u32 = 5 * 60 * 40;
/// How far short of its mark a staged unit starts, cells.
const APPROACH: f64 = 5.0;
/// A unit a scene turns but does not walk stands within this many cells of what it turns
/// to; one farther away is put this many cells from it (`FACE_DISTANCE`).
const FACE_REACH: f64 = 30.0;
const FACE_DISTANCE: f64 = 8.0;
/// How long the dry run looks for the scene's first line, ticks.
const DRY_RUN: u32 = 90 * 40;

/// A picture of the scene.
struct Frame {
    file: PathBuf,
    /// The lockstep clock (`--tick` takes it) and the game tick (it stands still while
    /// cinematic lines play).
    tick: u32,
    game_tick: u32,
    /// The line spoken, `None` for the establishing picture.
    line: Option<(String, String)>,
    camera: String,
}

impl Frame {
    fn heading(&self, index: usize) -> String {
        let what = match &self.line {
            Some((speaker, _)) => speaker.as_str(),
            None => "establishing",
        };
        format!(
            "{index:02} · {what} · tick {} · camera {}",
            self.tick, self.camera
        )
    }

    fn caption(&self) -> String {
        self.line
            .as_ref()
            .map_or_else(String::new, |(_, text)| text.clone())
    }

    fn tile(&self, index: usize) -> sheet::Tile<'_> {
        sheet::Tile {
            image: Some(&self.file),
            heading: self.heading(index),
            caption: self.caption(),
        }
    }
}

pub fn run(
    options: &Options,
    paths: &Paths,
    out: &Path,
    size: [u32; 2],
    scene: &str,
) -> Result<()> {
    let map = options.map.as_deref().context("--scene needs --map")?;
    let source = script(paths, map)?;
    let scenes = lua::scenes(&source);
    if scene == "list" {
        let opening = opening(&source);
        for name in &scenes {
            let note = if opening.contains(name) {
                "  (on_start plays it)"
            } else {
                ""
            };
            println!("{name}{note}");
        }
        return Ok(());
    }
    if scene != "all" && !scenes.iter().any(|s| s == scene) {
        bail!(
            "{map} has no scenes.{scene} (scenes: {})",
            scenes.join(", ")
        );
    }
    let mut painter = (options.sheet || options.compare.is_some()).then(sheet::Painter::new);
    // The map as it was at `--compare`'s revision.
    let checkout = options
        .compare
        .as_deref()
        .map(|rev| Checkout::new(paths, map, rev))
        .transpose()?;
    let old = match &checkout {
        Some(c) => {
            let map = c.map.to_string_lossy().into_owned();
            let source = script(paths, &map)?;
            Some((map, source))
        }
        None => None,
    };
    let mission = Mission {
        options,
        paths,
        map,
        source: &source,
        old: old.as_ref(),
        size,
    };
    if scene != "all" {
        mission.scene(scene, out, &mut painter)?;
        return Ok(());
    }
    // Every scene in its own folder, and one sheet of them all.
    let mut all = Vec::new();
    for name in &scenes {
        all.push((name, mission.scene(name, &out.join(name), &mut painter)?));
    }
    if let Some(painter) = painter.as_mut().filter(|_| options.sheet) {
        let tiles: Vec<_> = all
            .iter()
            .flat_map(|(name, frames)| {
                frames.iter().enumerate().map(move |(i, f)| sheet::Tile {
                    image: Some(&f.file),
                    heading: format!("{name} {}", f.heading(i)),
                    caption: f.caption(),
                })
            })
            .collect();
        let title = format!("{} · every scene", map_name(map));
        let file = out.join("sheet.png");
        sheet::contact(painter, &title, &tiles, 4, 480, aspect(size), &file)?;
        println!("{}", file.display());
    }
    Ok(())
}

fn aspect(size: [u32; 2]) -> f32 {
    size[0] as f32 / size[1] as f32
}

/// A mission whose scenes a preview plays, and the same map at `--compare`'s revision.
struct Mission<'a> {
    options: &'a Options,
    paths: &'a Paths,
    map: &'a str,
    source: &'a str,
    /// The old map's package path and script.
    old: Option<&'a (String, String)>,
    size: [u32; 2],
}

impl Mission<'_> {
    /// Plays `scene` into `out`, with its sheet and its pairs when asked for.
    fn scene(
        &self,
        scene: &str,
        out: &Path,
        painter: &mut Option<sheet::Painter>,
    ) -> Result<Vec<Frame>> {
        let (options, map, size) = (self.options, self.map, self.size);
        let frames = preview(options, self.paths, map, self.source, scene, out, size)?;
        if let Some(painter) = painter.as_mut().filter(|_| options.sheet) {
            let tiles: Vec<_> = frames.iter().enumerate().map(|(i, f)| f.tile(i)).collect();
            let title = format!("{} · scenes.{scene}", map_name(map));
            let file = out.join("sheet.png");
            sheet::contact(painter, &title, &tiles, 3, 640, aspect(size), &file)?;
            println!("{}", file.display());
        }
        let (Some(rev), Some((old_map, old_source))) = (&options.compare, self.old) else {
            return Ok(frames);
        };
        if !lua::scenes(old_source).iter().any(|s| s == scene) {
            eprintln!("{map} had no scenes.{scene} at {rev}: no pairs");
            return Ok(frames);
        }
        let painter = painter.as_mut().expect("a painter for pairs");
        let before = out.join("before");
        let old = preview(
            options, self.paths, old_map, old_source, scene, &before, size,
        )?;
        if options.sheet {
            let tiles: Vec<_> = old.iter().enumerate().map(|(i, f)| f.tile(i)).collect();
            let title = format!("{} at {rev} · scenes.{scene}", map_name(map));
            let file = before.join("sheet.png");
            sheet::contact(painter, &title, &tiles, 3, 640, aspect(size), &file)?;
        }
        for i in 0..old.len().max(frames.len()) {
            let file = out.join(format!("pair-{i:02}.png"));
            sheet::pair(
                painter,
                &side(&old, i, rev),
                &side(&frames, i, "now"),
                960,
                aspect(size),
                &file,
            )?;
            println!("{}", file.display());
        }
        Ok(frames)
    }
}

/// One side of pair `i`: `label`'s frame, or an empty panel when it has fewer.
fn side<'a>(frames: &'a [Frame], i: usize, label: &str) -> sheet::Tile<'a> {
    match frames.get(i) {
        Some(f) => sheet::Tile {
            image: Some(&f.file),
            heading: format!("{label} · {}", f.heading(i)),
            caption: f.caption(),
        },
        None => sheet::Tile {
            image: None,
            heading: format!("{label} · {i:02}"),
            caption: String::new(),
        },
    }
}

/// The mission script of `map`.
fn script(paths: &Paths, map: &str) -> Result<String> {
    world::read_map(paths, map)?
        .mission
        .map(|m| m.script)
        .with_context(|| format!("{map} is not a mission"))
}

/// The scenes `on_start` plays.
fn opening(source: &str) -> Vec<String> {
    lua::body(source, "on_start").map_or_else(Vec::new, |b| lua::calls(b, "play"))
}

fn map_name(map: &str) -> String {
    Path::new(map)
        .file_stem()
        .map_or_else(|| map.to_owned(), |s| s.to_string_lossy().into_owned())
}

/// The scene's name in the mission state, while one plays.
fn playing(world: &World) -> Option<String> {
    let m = world.state().mission.as_ref()?;
    m.cinema.scene.as_ref().map(|s| s.name.clone())
}

fn mission(world: &World) -> Result<&MissionState> {
    let m = world.state().mission.as_ref().context("not a mission")?;
    match &m.error {
        Some(error) => bail!("the mission script stopped: {error}"),
        None => Ok(m),
    }
}

/// Plays `scene` and writes its frames into `out`.
fn preview(
    options: &Options,
    paths: &Paths,
    map: &str,
    source: &str,
    scene: &str,
    out: &Path,
    size: [u32; 2],
) -> Result<Vec<Frame>> {
    let _span = tracing::info_span!("scene_preview", scene).entered();
    let opening = opening(source).iter().any(|s| s == scene);
    let exploration = (!opening).then_some(canopy_content::map::Exploration::Explored);
    let mut rig = Rig::new(options, paths, map, size, false, exploration)?;
    // `on_start`.
    rig.step()?;
    mission(&rig.game.world)?;
    if playing(&rig.game.world).as_deref() != Some(scene) {
        if playing(&rig.game.world).is_some() {
            canopy_sim::mission::skip(rig.game.world.state_mut(), CAMPAIGN_PLAYER);
            for _ in 0..40 {
                rig.step()?;
                if playing(&rig.game.world).is_none() {
                    break;
                }
            }
            mission(&rig.game.world)?;
        }
        stage(&mut rig, source, scene, &options.variables)?;
        // The staged state reaches the picture.
        rig.step()?;
    }
    clear(out)?;

    let start = rig.game.world.tick();
    let mut frames: Vec<Frame> = Vec::new();
    let mut spoken = None;
    let mut due: Vec<(u32, (String, String))> = Vec::new();
    let mut established = false;
    // The scene's first shot so far: kept until the scene cuts away for its first line.
    let mut opening_shot: Option<Picture> = None;
    let mut failed = None;
    loop {
        let world = &rig.game.world;
        let m = world.state().mission.as_ref().context("not a mission")?;
        failed.clone_from(&m.error);
        let game_tick = world.state().tick;
        let elapsed = world.tick() - start;
        let line = m
            .dialogue
            .as_ref()
            .filter(|d| spoken != Some(d.id))
            .map(|d| (d.id, d.speaker.clone(), d.text.clone()));
        let fade = m.cinema.fade.as_ref().map_or(0, |f| f.at(game_tick));
        let over = failed.is_some() || m.cinema.scene.is_none() || world.state().outcome.is_some();
        if !established {
            let settled = elapsed >= ESTABLISH.0 && (fade <= FADED_IN || elapsed >= ESTABLISH.1);
            if line.is_some() || over {
                // The shot just before the scene cut to its first line.
                let picture = match opening_shot.take() {
                    Some(p) => p,
                    None => Picture::take(&mut rig)?,
                };
                keep(&rig, out, &mut frames, picture, None)?;
                established = true;
            } else if settled {
                let picture = Picture::take(&mut rig)?;
                keep(&rig, out, &mut frames, picture, None)?;
                established = true;
            } else {
                opening_shot = Some(Picture::take(&mut rig)?);
            }
        }
        if let Some((id, speaker, text)) = line {
            spoken = Some(id);
            due.push((elapsed + LINE_DELAY, (speaker, text)));
        }
        let now: Vec<_> = due
            .iter()
            .filter(|(at, _)| *at <= elapsed || over)
            .cloned()
            .collect();
        due.retain(|(at, _)| *at > elapsed && !over);
        for (_, line) in now {
            let picture = Picture::take(&mut rig)?;
            keep(&rig, out, &mut frames, picture, Some(line))?;
        }
        if let Some(error) = &failed {
            eprintln!("the mission script stopped during scenes.{scene}: {error}");
        }
        if over {
            break;
        }
        if elapsed > LONGEST {
            eprintln!(
                "scenes.{scene} still plays after {} s: stopped",
                LONGEST / 40
            );
            break;
        }
        rig.step()?;
        rig.advance_clock(Duration::from_millis(25));
    }

    let mut text = format!(
        "# scenes.{scene} of {map}: frame, tick (the clock `--tick` takes), game tick, speaker, line, camera\n"
    );
    for (i, f) in frames.iter().enumerate() {
        let (speaker, line) = f
            .line
            .clone()
            .unwrap_or_else(|| ("(establishing)".into(), String::new()));
        let name = f
            .file
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        text.push_str(&format!(
            "{i:02}\t{name}\t{}\t{}\t{speaker}\t{line}\t{}\n",
            f.tick, f.game_tick, f.camera
        ));
    }
    std::fs::write(out.join("lines.txt"), text)?;
    Ok(frames)
}

/// A picture of the scene with what it shows.
struct Picture {
    pixels: Vec<u8>,
    tick: u32,
    game_tick: u32,
    camera: String,
}

impl Picture {
    fn take(rig: &mut Rig) -> Result<Self> {
        let pixels = rig.render(false)?;
        let world = &rig.game.world;
        let game_tick = world.state().tick;
        Ok(Self {
            pixels,
            tick: world.tick(),
            game_tick,
            camera: camera(world.state().mission.as_ref(), game_tick),
        })
    }
}

/// Writes `picture` as the next frame: the establishing one, or `line`'s.
fn keep(
    rig: &Rig,
    out: &Path,
    frames: &mut Vec<Frame>,
    picture: Picture,
    line: Option<(String, String)>,
) -> Result<()> {
    let name = match &line {
        Some((speaker, _)) => format!("{:02}-{}.png", frames.len(), slug(speaker)),
        None => format!("{:02}-establishing.png", frames.len()),
    };
    let file = out.join(name);
    rig.save(&file, &picture.pixels)?;
    println!("{}", file.display());
    frames.push(Frame {
        file,
        tick: picture.tick,
        game_tick: picture.game_tick,
        line,
        camera: picture.camera,
    });
    Ok(())
}

/// Removes an earlier preview's pictures from `out`.
fn clear(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).with_context(|| out.display().to_string())?;
    for entry in std::fs::read_dir(out)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let numbered = name.len() > 3
            && name.as_bytes()[..2].iter().all(u8::is_ascii_digit)
            && name.as_bytes()[2] == b'-';
        let ours = name.ends_with(".png") && (numbered || name.starts_with("pair-"))
            || name == "sheet.png"
            || name == "lines.txt";
        if ours && path.is_file() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// A file name's part for a speaker.
fn slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').chars().take(24).collect()
}

/// The camera the scene holds: a saved camera's id (`+pan` when panned onto another
/// point), a shot preset (`single:korrath`, `over:queen>korrath`, `wide:camp`), `a → b`
/// during a blend, or `game` for the game's own.
fn camera(m: Option<&MissionState>, tick: u32) -> String {
    let name = |p: &Pose| {
        if let Some(preset) = &p.preset {
            let kind = match preset.kind {
                canopy_sim::mission::cinema::PresetKind::Single => "single",
                canopy_sim::mission::cinema::PresetKind::Over => "over",
                canopy_sim::mission::cinema::PresetKind::Two => "two",
                canopy_sim::mission::cinema::PresetKind::Wide => "wide",
            };
            let subject = if preset.actors.is_empty() {
                "region".to_owned()
            } else {
                preset.actors.join(">")
            };
            return format!("{kind}:{subject}");
        }
        let id = p.camera.as_deref().unwrap_or("game");
        if p.target.is_some() {
            format!("{id}+pan")
        } else {
            id.to_owned()
        }
    };
    match m.and_then(|m| m.cinema.shot.as_ref()) {
        Some(shot) if shot.progress(tick as f32) < 1.0 => {
            format!("{} → {}", name(&shot.from), name(&shot.to))
        }
        Some(shot) => name(&shot.to),
        None => "game".into(),
    }
}

fn start(world: &mut World, scene: &str, skipping: bool) {
    let tick = world.state().tick;
    if let Some(m) = world.state_mut().mission.as_mut() {
        m.dialogue = None;
        // Long after the opening: its skip's cover, its fade-in and its last shot are over
        // (the scene's first blend starts from the player's camera).
        m.cinema.skipped = None;
        m.cinema.shot = None;
        if m.cinema.fade.as_ref().is_some_and(|f| f.to == 0) {
            m.cinema.fade = None;
        }
        m.cinema.scene = Some(ScenePlay {
            name: scene.to_owned(),
            done: 0,
            since: tick,
            skippable: true,
            skipping,
            moves: Vec::new(),
        });
    }
}

/// The living unit a placement made.
fn unit<'a>(world: &'a World, placement: &str) -> Option<&'a canopy_sim::entity::Entity> {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement) && e.hp.is_none_or(|hp| hp > 0))
}

/// Stages `scene` mid-mission (the module's list) and starts it.
fn stage(rig: &mut Rig, source: &str, scene: &str, overrides: &[(String, String)]) -> Result<()> {
    let world = &mut rig.game.world;
    let body = lua::body(source, &format!("scenes.{scene}"))
        .with_context(|| format!("no scenes.{scene}"))?;
    let script = world.mission().context("not a mission")?.script.clone();
    let lead = lua::lead(source, scene);

    // The cast the script makes appear.
    let spawned_by_scene = lua::calls(body, "mission.spawn");
    let spawned = &mission(world)?.spawned;
    let mut spawns: Vec<String> = Vec::new();
    for id in lead.spawns.iter().chain(&lua::literals(body)) {
        let scripted = script
            .placement(id)
            .is_some_and(|p| p.activation == Some(Activation::Script));
        if scripted
            && !spawned_by_scene.contains(id)
            && !spawned.contains(id)
            && !spawns.contains(id)
        {
            spawns.push(id.clone());
        }
    }
    let effects: Vec<MissionEffect> = spawns
        .iter()
        .map(|id| MissionEffect::Spawn {
            placement: id.clone(),
            camp: script.camp_of(id).map(|c| c.id.clone()),
        })
        .collect();
    world.apply_mission_effects(&effects);

    // Objectives: the ones the scene ends are under way, the ones it begins are not yet,
    // and when it begins a primary one, no other primary one is under way.
    let ended: Vec<String> = lua::calls(body, "mission.complete_objective")
        .into_iter()
        .chain(lua::calls(body, "mission.fail_objective"))
        .collect();
    let begun = lua::calls(body, "mission.begin_objective");
    let advances = script
        .objectives()
        .iter()
        .any(|o| !o.optional && begun.contains(&o.id));
    let objectives: Vec<(String, Option<ObjectiveState>)> = script
        .objectives()
        .iter()
        .filter_map(|o| {
            if ended.contains(&o.id) {
                Some((o.id.clone(), Some(ObjectiveState::Active)))
            } else if begun.contains(&o.id) {
                Some((o.id.clone(), None))
            } else if advances && !o.optional {
                let state = mission(world).ok()?.objective_states.get(&o.id)?;
                (*state == ObjectiveState::Active)
                    .then(|| (o.id.clone(), Some(ObjectiveState::Completed)))
            } else {
                None
            }
        })
        .collect();

    // Variables.
    let positions = lua::position_variables(source);
    let reads = lua::calls(body, "mission.get");
    let tick = world.state().tick;
    let mut values: BTreeMap<String, Option<Variable>> = BTreeMap::new();
    for (name, value) in &lead.sets {
        let value = match value {
            lua::Value::Nil => None,
            lua::Value::Boolean(b) => Some(Variable::Boolean(*b)),
            lua::Value::Integer(n) => Some(Variable::Integer(*n)),
            lua::Value::Number(n) => Some(Variable::Number(*n)),
            lua::Value::Text(t) => Some(Variable::String(t.clone())),
            lua::Value::Alive(u) => Some(Variable::Boolean(unit(world, u).is_some())),
            lua::Value::Met => Some(Variable::Boolean(true)),
            lua::Value::Tick => Some(Variable::Integer(i64::from(tick))),
        };
        values.insert(name.clone(), value);
    }
    let set = &mission(world)?.variables;
    // The units whose positions the scene reads.
    let mut placed_by: Vec<String> = Vec::new();
    for name in &reads {
        if set.contains_key(name) || values.contains_key(name) {
            continue;
        }
        match positions.iter().find(|p| &p.variable == name) {
            Some(p) => {
                if !placed_by.contains(&p.unit) {
                    placed_by.push(p.unit.clone());
                }
            }
            None => {
                values.insert(name.clone(), Some(Variable::Boolean(true)));
            }
        }
    }
    let mut overridden: Vec<&str> = Vec::new();
    for (name, text) in overrides {
        overridden.push(name);
        values.insert(name.clone(), variable(text));
    }
    let positioned: Vec<&lua::PositionVariable> = positions
        .iter()
        .filter(|p| placed_by.contains(&p.unit) && !overridden.contains(&p.variable.as_str()))
        .collect();
    let fill = |world: &mut World, values: &BTreeMap<String, Option<Variable>>| {
        let at: Vec<(String, Option<Variable>)> = positioned
            .iter()
            .filter_map(|p| {
                // A unit gone keeps the value it had.
                let e = unit(world, &p.unit)?;
                let milli = if p.axis == 0 {
                    e.position.x
                } else {
                    e.position.y
                };
                let value = Variable::Number(f64::from(milli) / f64::from(Milli::CELL));
                Some((p.variable.clone(), Some(value)))
            })
            .collect();
        let Some(m) = world.state_mut().mission.as_mut() else {
            return;
        };
        for (name, value) in values.iter().map(|(n, v)| (n.clone(), v.clone())).chain(at) {
            match value {
                Some(value) => {
                    m.variables.insert(name, value);
                }
                None => {
                    m.variables.remove(&name);
                }
            }
        }
        for (id, state) in &objectives {
            match state {
                Some(state) => {
                    m.objective_states.insert(id.clone(), *state);
                }
                None => {
                    m.objective_states.remove(id);
                }
            }
        }
    };
    fill(world, &values);

    // The units the scene's trigger needs gone fall as it starts.
    let gone: Vec<canopy_sim::entity::EntityId> = lead
        .gone
        .iter()
        .filter_map(|g| unit(world, g).map(|e| e.id))
        .collect();
    let state = world.state_mut();
    for &target in &gone {
        state.combat.scripted.push(canopy_sim::combat::ScriptedHit {
            target,
            source: None,
            amount: 1,
        });
        if let Some(e) = state.entities.get_mut(target) {
            e.hp = Some(1);
        }
    }
    if !gone.is_empty() {
        rig.step()?;
    }
    let world = &mut rig.game.world;

    // Where the scene walks its cast before its first line, and the cast there.
    let cast = dry_run(world, scene)?;
    let moves = &cast.moves;
    let mut places = Vec::new();
    let mut placed = Vec::new();
    let mut focus: Vec<Milli> = moves.iter().map(|m| m.destination).collect();
    for (placement, target) in &cast.faces {
        if moves.iter().any(|m| &m.placement == placement) {
            continue;
        }
        let Some(from) = unit(world, placement).map(|e| e.position) else {
            continue;
        };
        if let Some(destination) = near(world, *target, from) {
            placed.push(format!("{placement} near what it turns to"));
            places.push(MissionEffect::Place {
                placement: placement.clone(),
                destination,
                walking: false,
            });
            focus.push(destination);
        }
    }
    for m in moves {
        let Some(from) = unit(world, &m.placement).map(|e| e.position) else {
            continue;
        };
        if let Some(destination) = approach(world, m.destination, from) {
            let how = if destination == m.destination {
                "on its mark"
            } else {
                "short of its mark"
            };
            placed.push(format!("{} {how}", m.placement));
            places.push(MissionEffect::Place {
                placement: m.placement.clone(),
                destination,
                walking: false,
            });
        }
    }
    world.apply_mission_effects(&places);
    fill(world, &values);
    let summary: Vec<String> = values
        .iter()
        .map(|(n, v)| format!("{n}={}", show(v.as_ref())))
        .chain(positioned.iter().map(|p| {
            let value = world
                .state()
                .mission
                .as_ref()
                .and_then(|m| m.variables.get(&p.variable));
            format!("{}={}", p.variable, show(value))
        }))
        .collect();
    println!(
        "staged scenes.{scene}: spawned [{}], felled [{}], placed [{}], set [{}]",
        spawns.join(", "),
        lead.gone.join(", "),
        placed.join(", "),
        summary.join(", ")
    );
    start(world, scene, false);
    // The player's camera looks at the cast, as it would have followed them there: where
    // the scene walks or puts them, else the first unit it names.
    if focus.is_empty()
        && let Some(e) = lua::literals(body).iter().find_map(|id| unit(world, id))
    {
        focus.push(e.position);
    }
    if !focus.is_empty() {
        let n = focus.len() as f32;
        let cell = Milli::CELL as f32;
        let x = focus.iter().map(|p| p.x as f32).sum::<f32>() / n / cell;
        let z = focus.iter().map(|p| p.y as f32).sum::<f32>() / n / cell;
        rig.look_at(x, z);
    }
    Ok(())
}

fn show(value: Option<&Variable>) -> String {
    match value {
        None => "nil".into(),
        Some(Variable::Boolean(b)) => b.to_string(),
        Some(Variable::Integer(n)) => n.to_string(),
        Some(Variable::Number(n)) => format!("{n:.1}"),
        Some(Variable::String(s)) => format!("{s:?}"),
    }
}

/// `--set` text as a variable: `nil` clears it.
fn variable(text: &str) -> Option<Variable> {
    Some(match text {
        "nil" => return None,
        "true" => Variable::Boolean(true),
        "false" => Variable::Boolean(false),
        _ => match (text.parse::<i64>(), text.parse::<f64>()) {
            (Ok(n), _) => Variable::Integer(n),
            (_, Ok(n)) => Variable::Number(n),
            _ => Variable::String(text.to_owned()),
        },
    })
}

/// Plays the scene on a copy of the world up to its first line and returns the walks it
/// orders on the way (the latest for each unit); the world is as before.
fn dry_run(world: &mut World, scene: &str) -> Result<Cast> {
    let save = world.save();
    start(world, scene, false);
    let mut cast = Cast::default();
    let mut failed = None;
    for _ in 0..DRY_RUN {
        world.step();
        let Some(m) = world.state().mission.as_ref() else {
            break;
        };
        if let Some(error) = &m.error {
            failed = Some(error.clone());
            break;
        }
        let Some(play) = &m.cinema.scene else {
            break;
        };
        for walk in &play.moves {
            cast.moves.retain(|w| w.placement != walk.placement);
            cast.moves.push(walk.clone());
        }
        for turn in &m.facing {
            if !cast.faces.iter().any(|(p, _)| *p == turn.placement) {
                cast.faces.push((turn.placement.clone(), turn.target));
            }
        }
        if m.dialogue.is_some() {
            break;
        }
    }
    world.rewind(save).map_err(|e| anyhow!(e.0))?;
    if let Some(error) = failed {
        bail!(
            "scenes.{scene} stops when staged: {error} (set the variables it needs with --set <name>=<value>)"
        );
    }
    Ok(cast)
}

/// What a scene does with its cast before its first line.
#[derive(Default)]
struct Cast {
    /// The walks it orders (the latest for each unit).
    moves: Vec<SceneMove>,
    /// The points it turns units to (the first for each unit).
    faces: Vec<(String, Milli)>,
}

/// Where a unit at `from` that a scene turns to `target` but does not walk stands: where
/// it is when within [`FACE_REACH`], else [`FACE_DISTANCE`] cells from the target on open
/// ground towards where it was.
fn near(world: &World, target: Milli, from: Milli) -> Option<Milli> {
    let cell = f64::from(Milli::CELL);
    let (dx, dy) = (
        f64::from(from.x - target.x) / cell,
        f64::from(from.y - target.y) / cell,
    );
    let distance = dx.hypot(dy);
    if distance <= FACE_REACH {
        return None;
    }
    let (ux, uy) = (dx / distance, dy / distance);
    let spatial = world.spatial();
    (0..8)
        .map(|i| FACE_DISTANCE + f64::from(i))
        .find_map(|along| {
            let (x, y) = (
                f64::from(target.x) / cell + ux * along,
                f64::from(target.y) / cell + uy * along,
            );
            let spot = Spot {
                x: x.floor() as i32,
                y: y.floor() as i32,
                deck: None,
            };
            (spatial.valid_spot(spot) && spatial.node(spot).is_some_and(|n| spatial.walkable(n)))
                .then(|| Milli {
                    x: (x * cell).round() as i32,
                    y: (y * cell).round() as i32,
                })
        })
}

/// Where a unit at `from` starts for its walk to `mark`: a few cells short of it on open
/// ground, else on it; `None` when it is that close already.
fn approach(world: &World, mark: Milli, from: Milli) -> Option<Milli> {
    let cell = f64::from(Milli::CELL);
    let (dx, dy) = (
        f64::from(from.x - mark.x) / cell,
        f64::from(from.y - mark.y) / cell,
    );
    let distance = dx.hypot(dy);
    if distance <= APPROACH {
        return None;
    }
    let (ux, uy) = (dx / distance, dy / distance);
    let spatial = world.spatial();
    let open = (1..=(APPROACH * 2.0) as u32).all(|i| {
        let along = f64::from(i) / 2.0;
        let spot = Spot {
            x: (f64::from(mark.x) / cell + ux * along).floor() as i32,
            y: (f64::from(mark.y) / cell + uy * along).floor() as i32,
            deck: None,
        };
        spatial.valid_spot(spot) && spatial.node(spot).is_some_and(|n| spatial.walkable(n))
    });
    if !open {
        return Some(mark);
    }
    Some(Milli {
        x: mark.x + (ux * APPROACH * cell).round() as i32,
        y: mark.y + (uy * APPROACH * cell).round() as i32,
    })
}

/// The map package as it was at Git revision `rev`, in a scratch folder removed on drop.
struct Checkout {
    folder: PathBuf,
    map: PathBuf,
}

impl Checkout {
    fn new(paths: &Paths, map: &str, rev: &str) -> Result<Self> {
        let repository = paths
            .repository()
            .context("--compare reads Git history: run it in a repository checkout")?
            .canonicalize()?;
        let package = Path::new(map)
            .canonicalize()
            .with_context(|| format!("--compare needs the map package's path, not {map}"))?;
        let relative = package
            .strip_prefix(&repository)
            .with_context(|| format!("{map} is not in {}", repository.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let git = |args: &[&str]| -> Result<Vec<u8>> {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(args)
                .output()
                .context("git")?;
            if !output.status.success() {
                bail!(
                    "git {}: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Ok(output.stdout)
        };
        let listed = git(&["ls-tree", "-r", "--name-only", rev, "--", &relative])?;
        let files: Vec<String> = String::from_utf8_lossy(&listed)
            .lines()
            .map(str::to_owned)
            .collect();
        if files.is_empty() {
            bail!("{relative} did not exist at {rev}");
        }
        let folder = paths
            .scratch()
            .context("--compare needs a repository checkout")?
            .join(format!("scene-compare-{}", std::process::id()));
        let name = package.file_name().context("a package name")?;
        let checkout = Self {
            map: folder.join(name),
            folder,
        };
        for file in files {
            let inside = file
                .strip_prefix(&relative)
                .map(|f| f.trim_start_matches('/'))
                .context("a file of the package")?;
            // `--filters`: Git LFS files come out as their content.
            let content = git(&["cat-file", "--filters", &format!("{rev}:{file}")])?;
            let path = checkout.map.join(inside);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, content).with_context(|| path.display().to_string())?;
        }
        Ok(checkout)
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_frames_by_speaker() {
        assert_eq!(slug("Nurse Ardi"), "nurse-ardi");
        assert_eq!(slug("Coilkeeper Vosk!"), "coilkeeper-vosk");
        assert_eq!(variable("nil"), None);
        assert_eq!(variable("2"), Some(Variable::Integer(2)));
        assert_eq!(variable("0.5"), Some(Variable::Number(0.5)));
        assert_eq!(variable("road"), Some(Variable::String("road".into())));
    }
}
