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
    let content = Arc::new(Content::load(&paths.data).map_err(|e| anyhow!("content: {e}"))?);
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
    let mut scene = canopy_scene::build(&world, &map, &pack, &look)?;
    if let Some(colors) = &spec.colors {
        let palette = scene.colors.clone();
        for (slot, &index) in colors.iter().enumerate() {
            if let (Some(to), Some(&from)) = (scene.colors.get_mut(slot), palette.get(index)) {
                *to = from;
            }
        }
    }
    let presenter = scene.presenter(&world, &map, pack.clone());
    let hour = spec.hour.unwrap_or(scene.hour);
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
    tracing::info!(
        map = id,
        biome = map.biome(),
        players = world.setup().slots.len(),
        statics = scene.world.statics.len(),
        live_models = scene.world.actor_models.len(),
        ms = started.elapsed().as_millis() as u64,
        "match loaded"
    );
    Ok((
        Loaded {
            scene,
            environment,
            pack,
        },
        Match {
            world,
            session,
            presenter,
        },
    ))
}
