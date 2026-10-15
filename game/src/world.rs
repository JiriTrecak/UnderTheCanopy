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
    /// A measured battle instead of a match: this many soldiers for each of eight players,
    /// facing each other around the middle of the starts (no computer players).
    pub crowd: Option<u32>,
    /// Player colour (index into `config/players.json`) of each slot, when not its own.
    pub colors: Option<Vec<usize>>,
    /// The local player's slot (a human on that start); `None` spectates computer
    /// players.
    pub human: Option<u8>,
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
}

/// Seconds of match time per in-game day (old `DAY_CYCLE_SECONDS`).
pub const DAY_SECONDS: f32 = 600.0;

/// The hour `seconds` of match time after `start` (old `Sky.tick`).
pub fn day_hour(start: f32, seconds: f32) -> f32 {
    canopy_render::environment::wrap_hour(start + seconds / DAY_SECONDS * 24.0)
}

/// The old sky's day cycle in a match: the hour starts at the map's and advances 24 hours
/// every [`DAY_SECONDS`] of match time; sandboxes and a requested hour stay fixed.
#[derive(Debug, Clone)]
pub struct DayCycle {
    pub start_hour: f32,
    pub lighting: canopy_render::environment::LightingConfig,
    pub biome: canopy_render::environment::BiomeConfig,
}

impl DayCycle {
    /// The hour after `seconds` of match time.
    pub fn hour(&self, seconds: f32) -> f32 {
        day_hour(self.start_hour, seconds)
    }

    /// The light at `seconds` of match time.
    pub fn lighting(&self, seconds: f32) -> canopy_render::environment::Lighting {
        canopy_render::environment::Lighting::resolve(
            &self.lighting,
            &self.biome,
            self.hour(seconds),
        )
    }
}

/// A match: the world, its session and its presenter (they move to the simulation
/// thread together).
pub struct Match {
    pub world: canopy_sim::World,
    pub session: Session,
    pub presenter: Presenter,
}

/// Loads a match on map `spec.map` with its biome's look.
pub fn load(paths: &Paths, spec: &MatchSpec<'_>) -> Result<(Loaded, Match)> {
    let id = spec.map;
    let _span = tracing::info_span!("load_map", map = id).entered();
    let started = std::time::Instant::now();
    // A packaged game ships the cooked pack instead of `assets/library` (D26).
    let content = if crate::data::is_packaged(&paths.data) {
        Content::load_packaged(&paths.data)
    } else {
        Content::load(&paths.data)
    };
    let content = Arc::new(content.map_err(|e| anyhow!("content: {e}"))?);
    let library = map::library(&paths.data).map_err(|e| anyhow!("maps: {e}"))?;
    let file = library.iter().find(|m| m.id == id).with_context(|| {
        let ids: Vec<&str> = library.iter().map(|m| m.id.as_str()).collect();
        format!("no map {id} (maps: {})", ids.join(", "))
    })?;
    let mut map = file.load().map_err(|e| anyhow!("{}: {e}", file.file))?;
    let pack_path =
        canopy_assets::find_pack(&paths.data).context("no cooked asset pack (run `just cook`)")?;
    let pack = Arc::new(Pack::open(&pack_path).with_context(|| pack_path.display().to_string())?);
    let look = Look::load(&paths.data, spec.biome.unwrap_or(map.biome()))?;
    let setup = match spec.crowd {
        Some(n) => canopy_sim::crowd::crowd(&mut map, &content, n).map_err(|e| anyhow!(e))?,
        None => {
            let mut setup = canopy_scene::spectator_setup(&map, spec.players);
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
    let human = spec
        .human
        .filter(|p| spec.crowd.is_none() && setup.slots.iter().any(|s| s.player == *p));
    let session = Session::local(setup, human);
    let setup = session
        .setup()
        .context("a local session has its setup")?
        .clone();
    let world = canopy_sim::World::new(content, &map, setup)
        .map_err(|e| anyhow!("cannot start the match: {}", e.0))?;
    let (mut loaded, game) = assemble(
        &map,
        pack,
        &look,
        world,
        session,
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
fn assemble(
    map: &map::UtcMap,
    pack: Arc<Pack>,
    look: &Look,
    world: canopy_sim::World,
    session: Session,
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
    let day = (hour.is_none() && !map.sandbox).then(|| DayCycle {
        start_hour: scene.hour,
        lighting: look.lighting.clone(),
        biome: look.biome.clone(),
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
    let environment = Environment::new(&look.lighting, &look.biome, hour, lut.as_ref());
    Ok((
        Loaded {
            scene,
            environment,
            pack,
            // Set by the caller.
            data: std::path::PathBuf::new(),
            day,
            sandbox: map.sandbox,
        },
        Match {
            world,
            session,
            presenter,
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
    pub fn label(self) -> &'static str {
        match self {
            Self::Map => "Reading the battlefield",
            Self::World => "Waking the colonies",
            Self::Scene => "Raising the canopy",
            Self::Gpu => "Preparing terrain and forest",
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
}

/// A prepared match and what the game needs to know about it.
pub struct Prepared {
    pub loaded: Loaded,
    pub game: Match,
    pub map_name: String,
    /// Bytes of the `.utcmap` file (identities for saves and multiplayer).
    pub map_bytes: Vec<u8>,
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
    let entry = data
        .map(&request.map)
        .with_context(|| format!("no map {}", request.map))?;
    let map_bytes = std::fs::read(&entry.file.path)
        .with_context(|| format!("cannot read {}", entry.file.file))?;
    let map = map::UtcMap::parse(&map_bytes, &entry.file.file)
        .map_err(|e| anyhow!("{}: {e}", entry.file.file))?;
    let look = Look::load(&paths.data, map.biome())?;
    progress(Stage::World, 0.25);
    let (world, session) = match request.resume {
        Some(save) => {
            let setup = save.setup.clone();
            let world = canopy_sim::World::restore(data.content.clone(), &map, save)
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
            let world = canopy_sim::World::new(data.content.clone(), &map, setup)
                .map_err(|e| anyhow!("cannot start the match: {}", e.0))?;
            (world, session)
        }
    };
    progress(Stage::Scene, 0.6);
    let (mut loaded, game) = assemble(&map, data.pack.clone(), &look, world, session, None, None)?;
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
        map_name: entry.name.clone(),
        map_bytes,
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
    fn a_match_day_lasts_ten_minutes() {
        use super::day_hour;
        assert_eq!(day_hour(9.5, 0.0), 9.5);
        assert!((day_hour(9.5, 150.0) - 15.5).abs() < 1e-4);
        assert!((day_hour(9.5, 600.0) - 9.5).abs() < 1e-3);
        assert!((day_hour(20.0, 300.0) - 8.0).abs() < 1e-4);
    }
}
