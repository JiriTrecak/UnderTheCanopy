//! Loading a match for viewing: content, the cooked pack, the biome's configuration, the
//! simulation world with its local session, the static scene and the live presenter.

use crate::paths::Paths;
use anyhow::{Context, Result, anyhow};
use canopy_assets::Pack;
use canopy_content::{Content, map};
use canopy_net::Session;
use canopy_render::{Environment, environment::DaytimeLut};
use canopy_scene::{Look, MapScene, live::Presenter};
use std::sync::Arc;

/// What a view of a match needs.
pub struct MatchSpec<'a> {
    pub map: &'a str,
    /// Hour of the day; the map's own when `None`.
    pub hour: Option<f32>,
    /// Computer players on the first starts; every start when `None`.
    pub players: Option<usize>,
    /// Another biome's look, to preview it on this map.
    pub biome: Option<&'a str>,
    /// Other weather than the map's (`rain`, `snow`, `godRays`, `clear`).
    pub weather: Option<&'a str>,
    /// A measured battle instead of a match: this many soldiers for each of eight players,
    /// facing each other around the middle of the starts (no computer players).
    pub crowd: Option<u32>,
    /// One of every unit and building (`canopy_sim::crowd::gallery`).
    pub gallery: bool,
    /// The gallery keeps only definitions whose id contains this.
    pub only: Option<&'a str>,
    /// Player colour (index into `config/players.json`) of each slot, when not its own.
    pub colors: Option<Vec<usize>>,
    /// Race of each slot in order, when not the start's own.
    pub races: Option<Vec<String>>,
    /// The local player's slot (a human on that start); `None` spectates computer
    /// players.
    pub human: Option<u8>,
    /// The game data to play; read from the data folder when `None`.
    pub content: Option<Arc<Content>>,
    /// Trying an ability: this unit near the local player's start, with allies and
    /// creeps (`canopy_sim::crowd::encounter`).
    pub encounter: Option<&'a str>,
    /// How much of the map the players know at the start, when not the map's own (a scene
    /// preview shows a mission as a player who walked it knows it).
    pub exploration: Option<map::Exploration>,
    /// The mission's autoplay plan steers the local player, when the map has one.
    pub autoplay: bool,
}

/// What the renderer draws of a map: the scene, its lighting and the asset pack.
pub struct Loaded {
    pub scene: MapScene,
    pub environment: Environment,
    pub pack: Arc<Pack>,
    /// The data folder (the world UI's label font).
    pub data: std::path::PathBuf,
    /// The match's day cycle (`None` when the hour stays fixed).
    pub day: Option<DayCycle>,
    /// A sandbox map (no victory; the old game revealed it to a local player).
    pub sandbox: bool,
    /// Scenery and live models that stop a close camera's boom.
    pub obstacles: canopy_scene::obstruction::Obstacles,
    /// The map's music and its biome's ambience (sound event ids, D97).
    pub music: Vec<String>,
    pub ambience: Option<canopy_render::environment::Ambience>,
}

/// The hour `seconds` of match time after `start` when a day lasts `day_seconds` (old
/// `Sky.tick`, whose day was ten minutes).
pub fn day_hour(start: f32, seconds: f32, day_seconds: f32) -> f32 {
    canopy_render::environment::wrap_hour(start + seconds / day_seconds.max(1.0) * 24.0)
}

/// The old sky's day cycle in a match: the hour starts at the map's and advances 24 hours
/// every `day_seconds` of match time (the map's `dayMinutes`, ten by default); sandboxes
/// and a requested hour stay fixed.
#[derive(Debug, Clone)]
pub struct DayCycle {
    pub start_hour: f32,
    /// Seconds of match time per day.
    pub day_seconds: f32,
    pub lighting: canopy_render::environment::LightingConfig,
    pub biome: canopy_render::environment::BiomeConfig,
    /// The map's weather, which tones the light too.
    pub weather: canopy_render::environment::Weather,
}

impl DayCycle {
    /// The hour after `seconds` of match time.
    pub fn hour(&self, seconds: f32) -> f32 {
        day_hour(self.start_hour, seconds, self.day_seconds)
    }

    /// The light at `seconds` of match time.
    pub fn lighting(&self, seconds: f32) -> canopy_render::environment::Lighting {
        let mut lighting = canopy_render::environment::Lighting::resolve(
            &self.lighting,
            &self.biome,
            self.hour(seconds),
        );
        self.weather.light(&mut lighting);
        lighting
    }
}

/// A match: the world, its session and its presenter (they move to the simulation
/// thread together), and what feeds it: the session, recorded (D60), or a replay.
pub struct Match {
    pub world: canopy_sim::World,
    pub session: Session,
    pub presenter: Presenter,
    pub driver: crate::sim::Driver,
}

impl Match {
    /// The recording of this match, if it is recorded.
    pub fn recorder(&self) -> Option<crate::replay::record::Recorder> {
        match &self.driver {
            crate::sim::Driver::Live { recorder, .. } => recorder.clone(),
            crate::sim::Driver::Replay(_) => None,
        }
    }
}

/// Reads map `id`: a map of the library, or a map package's path (tests and captures of
/// maps the game does not ship).
pub fn read_map(paths: &Paths, id: &str) -> Result<map::UtcMap> {
    let path = std::path::Path::new(id);
    if path
        .extension()
        .is_some_and(|e| e == canopy_content::package::MAP_EXTENSION)
    {
        return map::package::load(path).map_err(|e| anyhow!("{e}"));
    }
    let library = crate::data::map_files(paths)?;
    let file = library.iter().find(|m| m.id == id).with_context(|| {
        let ids: Vec<&str> = library.iter().map(|m| m.id.as_str()).collect();
        format!("no map {id} (maps: {})", ids.join(", "))
    })?;
    file.load().map_err(|e| anyhow!("{}: {e}", file.file))
}

/// Loads a match on map `spec.map` with its biome's look.
pub fn load(paths: &Paths, spec: &MatchSpec<'_>) -> Result<(Loaded, Match)> {
    let id = spec.map;
    let _span = tracing::info_span!("load_map", map = id).entered();
    let started = std::time::Instant::now();
    // A packaged game ships the cooked pack instead of `assets/library` (D26).
    let content = match &spec.content {
        Some(content) => Ok((**content).clone()),
        None if crate::data::is_packaged(&paths.data) => Content::load_packaged(&paths.data),
        None => Content::load(&paths.data),
    };
    let content = Arc::new(content.map_err(|e| anyhow!("content: {e}"))?);
    let mut map = read_map(paths, id)?;
    if let Some(exploration) = spec.exploration {
        map.initial_exploration = Some(exploration);
    }
    // Other weather than the map's, to preview it (`--weather`).
    if let Some(weather) = spec.weather {
        let kind: canopy_content::map::WeatherKind =
            serde_json::from_value(serde_json::json!(weather)).context("--weather")?;
        let landscape = map.landscape.get_or_insert(canopy_content::map::Landscape {
            environment: canopy_content::map::EnvironmentConditions {
                hour: 12.0,
                playing: true,
                weather: None,
                day_minutes: None,
            },
            decals: None,
            scatter: None,
            water: None,
        });
        landscape.environment.weather = Some(canopy_content::map::WeatherConditions { kind });
    }
    let pack_path =
        canopy_assets::find_pack(&paths.data).context("no cooked asset pack (run `just cook`)")?;
    let pack = Arc::new(Pack::open(&pack_path).with_context(|| pack_path.display().to_string())?);
    let look = Look::load(&paths.data, spec.biome.unwrap_or(map.biome()))?;
    let setup = match spec.crowd {
        Some(n) => canopy_sim::crowd::crowd_of(
            &mut map,
            &content,
            n,
            spec.races.as_deref().unwrap_or_default(),
        )
        .map_err(|e| anyhow!(e))?,
        None if spec.gallery => {
            canopy_sim::crowd::gallery(&mut map, &content, spec.only).map_err(|e| anyhow!(e))?
        }
        None if let Some(caster) = spec.encounter => {
            canopy_sim::crowd::encounter(&mut map, &content, caster).map_err(|e| anyhow!(e))?
        }
        None => {
            let mut setup = canopy_scene::spectator_setup(&map, spec.players);
            for (slot, race) in setup.slots.iter_mut().zip(spec.races.iter().flatten()) {
                slot.race = Some(race.clone());
            }
            if let Some(slot) = setup
                .slots
                .iter_mut()
                .find(|s| Some(s.player) == spec.human)
            {
                slot.kind = canopy_sim::SlotKind::Human;
                slot.name = Some("You".into());
            }
            setup
        }
    };
    let human = spec.human.filter(|p| {
        spec.crowd.is_none() && !spec.gallery && setup.slots.iter().any(|s| s.player == *p)
    });
    let session = Session::local(setup, human);
    let setup = session
        .setup()
        .context("a local session has its setup")?
        .clone();
    let world = canopy_sim::World::new(content, &map, setup)
        .map_err(|e| anyhow!("cannot start the match: {}", e.0))?;
    // Matches of the shipped kind are recorded; measured scenes and encounters are not.
    let recorded = spec.crowd.is_none() && !spec.gallery && spec.encounter.is_none();
    let driver = crate::sim::Driver::Live {
        recorder: recorded.then(|| crate::replay::record::Recorder::new(&world)),
        autoplay: autoplay_spec(&map, spec.autoplay && human.is_some()),
    };
    let (mut loaded, game) = assemble(
        &map,
        pack,
        &look,
        world,
        session,
        driver,
        spec.colors.as_deref(),
        spec.hour,
    )?;
    loaded.data = paths.data.clone();
    tracing::info!(
        map = id,
        biome = map.biome(),
        players = game.world.setup().slots.len(),
        statics = loaded.scene.world.statics.len(),
        live_models = loaded.scene.world.actor_models.len(),
        ms = started.elapsed().as_millis() as u64,
        "match loaded"
    );
    Ok((loaded, game))
}

/// The static scene, presenter and lighting of a built world.
#[allow(clippy::too_many_arguments)]
fn assemble(
    map: &map::UtcMap,
    pack: Arc<Pack>,
    look: &Look,
    world: canopy_sim::World,
    session: Session,
    driver: crate::sim::Driver,
    colors: Option<&[usize]>,
    hour: Option<f32>,
) -> Result<(Loaded, Match)> {
    let mut scene = canopy_scene::build(&world, map, &pack, look)?;
    if let Some(colors) = colors {
        let palette = scene.colors.clone();
        for (slot, &index) in colors.iter().enumerate() {
            if let (Some(to), Some(&from)) = (scene.colors.get_mut(slot), palette.get(index)) {
                *to = from;
            }
        }
    }
    let presenter = scene.presenter(&world, map, pack.clone());
    // The clock runs unless the map holds its hour (`landscape.environment.playing`).
    let dynamic = map.landscape.as_ref().is_none_or(|l| l.environment.playing);
    let day_minutes = map
        .landscape
        .as_ref()
        .map_or(canopy_content::map::DAY_MINUTES, |l| {
            l.environment.day_minutes()
        });
    let day = (hour.is_none() && !map.sandbox && dynamic).then(|| DayCycle {
        start_hour: scene.hour,
        day_seconds: (day_minutes * 60.0) as f32,
        lighting: look.lighting.clone(),
        biome: look.biome.clone(),
        weather: canopy_scene::weather_of(map),
    });
    let hour = hour.unwrap_or(scene.hour);
    let grade = &look.biome.grade;
    let lut_texture = match (&grade.daytime_lut, grade.daytime_lut_strength > 0.0) {
        (Some(entry), true) => Some(pack.texture(entry).with_context(|| entry.clone())?),
        _ => None,
    };
    let lut_texels = lut_texture.as_ref().map(|t| t.decode_level(0));
    let lut = match (&lut_texture, &lut_texels) {
        (Some(t), Some(rgba)) => Some(DaytimeLut {
            size: t.meta.width,
            rgba,
            strength: grade.daytime_lut_strength,
        }),
        _ => None,
    };
    let environment = Environment::new(&look.lighting, &look.biome, hour, lut.as_ref())
        .with_weather(canopy_scene::weather_of(map));
    let obstacles = canopy_scene::obstruction::Obstacles::new(
        &pack,
        &scene.world.statics,
        &scene.world.actor_models,
    );
    Ok((
        Loaded {
            scene,
            environment,
            obstacles,
            pack,
            // Set by the caller.
            data: std::path::PathBuf::new(),
            day,
            sandbox: map.sandbox,
            music: map
                .music
                .iter()
                .flatten()
                .map(ToString::to_string)
                .collect(),
            ambience: look.biome.ambience.clone(),
        },
        Match {
            world,
            session,
            presenter,
            driver,
        },
    ))
}

/// Loading steps of a match started from the menus, for the loading screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Map,
    World,
    Scene,
    Gpu,
}

impl Stage {
    pub fn label(self) -> String {
        match self {
            Self::Map => tr!("loading.stage-map"),
            Self::World => tr!("loading.stage-world"),
            Self::Scene => tr!("loading.stage-scene"),
            Self::Gpu => tr!("loading.stage-gpu"),
        }
    }
}

/// A match the menus ask for.
pub struct MatchRequest {
    pub map: String,
    pub setup: canopy_sim::MatchSetup,
    /// The local player's slot; `None` to watch.
    pub me: Option<u8>,
    /// Continue a saved match instead of starting `setup`.
    pub resume: Option<canopy_sim::Save>,
    /// A networked seat (multiplayer); a local session is made otherwise.
    pub session: Option<Session>,
    /// Palette colour of each player by player number (the lobby's), when not its own.
    pub colors: Option<Vec<usize>>,
    /// Watch this replay instead (D60): its start and commands; no session commands.
    pub replay: Option<Arc<crate::replay::format::Replay>>,
    /// The map package to load instead of the library's map `map` (a replay's own map).
    pub map_path: Option<std::path::PathBuf>,
    /// A campaign mission: what its campaign hands it (tech, carried heroes, level cap,
    /// variables).
    pub campaign: Option<canopy_sim::campaign::CampaignStart>,
    /// The mission's autoplay plan steers the local player (docs/campaign.md "Autoplay").
    pub autoplay: bool,
}

/// The autoplay plan of `map` when it has one and `wanted`.
fn autoplay_spec(map: &map::UtcMap, wanted: bool) -> Option<crate::sim::AutoplaySpec> {
    let source = map.mission.as_ref()?.autoplay.clone().filter(|_| wanted)?;
    Some(crate::sim::AutoplaySpec {
        source,
        map: Arc::new(map.clone()),
    })
}

/// A prepared match and what the game needs to know about it.
pub struct Prepared {
    pub loaded: Loaded,
    pub game: Match,
    pub map_name: String,
    /// The map package's digest (identities for saves and multiplayer).
    pub map_identity: Vec<u8>,
    /// The mission's objectives (titles for the mission HUD).
    pub objectives: Vec<canopy_content::map::Objective>,
}

/// Builds a match on the game's loaded data (any thread): the map, the world (new or
/// restored), its session, the static scene and the presenter. GPU uploads follow on the
/// window thread.
pub fn prepare(
    paths: &Paths,
    data: &crate::data::GameData,
    request: MatchRequest,
    progress: &dyn Fn(Stage, f32),
) -> Result<Prepared> {
    let _span = tracing::info_span!("prepare_match", map = %request.map).entered();
    let started = std::time::Instant::now();
    progress(Stage::Map, 0.05);
    let (map, map_identity, map_name) = match &request.map_path {
        Some(path) => {
            let package = canopy_content::package::Package::open(path)
                .map_err(|e| anyhow!("{}: {e}", path.display()))?;
            let digest = package.digest()?.to_vec();
            let map = map::package::load(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
            let name = map::package::summary(path)
                .map(|s| s.name)
                .unwrap_or_else(|_| request.map.clone());
            (map, digest, name)
        }
        None => {
            let entry = data
                .map(&request.map)
                .with_context(|| format!("no map {}", request.map))?;
            let map = entry
                .file
                .load()
                .map_err(|e| anyhow!("{}: {e}", entry.file.file))?;
            (map, entry.identity()?, entry.name.clone())
        }
    };
    let look = Look::load(&paths.data, map.biome())?;
    // A campaign mission plays on its campaign's layer when it has one.
    let content = match &request.map_path {
        Some(_) => data.content.clone(),
        None => data.content_for(paths, &request.map)?,
    };
    progress(Stage::World, 0.25);
    let colors = request.colors.clone();
    let mut playback = None;
    // A loaded save plays on by hand: autoplay's plan starts only with a mission.
    let fresh = request.resume.is_none();
    let (world, session) = match request.resume {
        _ if let Some(replay) = &request.replay => {
            let p = crate::replay::playback::Playback::new(
                replay,
                content.clone(),
                Arc::new(map.clone()),
            );
            let world = p.first_world().context("cannot start the replay")?;
            playback = Some(p);
            (world, Session::local(replay.body.setup.clone(), None))
        }
        Some(save) => {
            let setup = save.setup.clone();
            let world = canopy_sim::World::restore(content.clone(), &map, save)
                .map_err(|e| anyhow!("cannot load the save: {}", e.0))?;
            let session = request
                .session
                .unwrap_or_else(|| Session::local_from(setup, request.me, world.tick()));
            (world, session)
        }
        None => {
            let session = request
                .session
                .unwrap_or_else(|| Session::local(request.setup.clone(), request.me));
            let setup = session.setup().cloned().unwrap_or(request.setup);
            let world = match &request.campaign {
                Some(campaign) => {
                    canopy_sim::World::new_campaign(content.clone(), &map, setup, campaign)
                }
                None => canopy_sim::World::new(content.clone(), &map, setup),
            }
            .map_err(|e| anyhow!("cannot start the match: {}", e.0))?;
            (world, session)
        }
    };
    progress(Stage::Scene, 0.6);
    let driver = match playback {
        Some(p) => crate::sim::Driver::Replay(Box::new(p)),
        None => crate::sim::Driver::Live {
            recorder: Some(crate::replay::record::Recorder::new(&world)),
            autoplay: autoplay_spec(&map, request.autoplay && request.me.is_some() && fresh),
        },
    };
    let (mut loaded, game) = assemble(
        &map,
        data.pack.clone(),
        &look,
        world,
        session,
        driver,
        colors.as_deref(),
        None,
    )?;
    loaded.data = paths.data.clone();
    progress(Stage::Gpu, 0.85);
    tracing::info!(
        map = %request.map,
        players = game.world.setup().slots.len(),
        tick = game.world.tick(),
        ms = started.elapsed().as_millis() as u64,
        "match prepared"
    );
    Ok(Prepared {
        loaded,
        game,
        map_name,
        map_identity,

        objectives: map
            .mission
            .as_ref()
            .and_then(|m| m.objectives.clone())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_match_day_lasts_its_map_s_minutes() {
        use super::day_hour;
        // The old game's ten minutes.
        assert_eq!(day_hour(9.5, 0.0, 600.0), 9.5);
        assert!((day_hour(9.5, 150.0, 600.0) - 15.5).abs() < 1e-4);
        assert!((day_hour(9.5, 600.0, 600.0) - 9.5).abs() < 1e-3);
        assert!((day_hour(20.0, 300.0, 600.0) - 8.0).abs() < 1e-4);
        // A long day: three hours of dusk over 25 minutes.
        assert!((day_hour(18.5, 1500.0, 12_000.0) - 21.5).abs() < 1e-3);
    }
}
