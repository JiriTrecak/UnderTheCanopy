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
    /// A measured battle instead of a match: this many soldiers for each of eight players,
    /// facing each other around the middle of the starts (no computer players).
    pub crowd: Option<u32>,
    /// Player colour (index into `config/players.json`) of each slot, when not its own.
    pub colors: Option<Vec<usize>>,
}

/// The building-grid position nearest to (x, z) for `d`'s footprint.
fn aligned(
    d: &canopy_content::game::Definition,
    x: f64,
    z: f64,
) -> canopy_content::placement::Point {
    let (x, z) = (x.floor(), z.floor());
    let steps = (0..8).map(|i| f64::from(i) * 0.5);
    let (dx, dz) = steps
        .clone()
        .flat_map(|dx| steps.clone().map(move |dz| (dx, dz)))
        .find(|(dx, dz)| {
            canopy_sim::terrain::footprint::placement_geometry_error(d, x + dx, z + dz, 0.0)
                .is_none()
        })
        .unwrap_or_default();
    canopy_content::placement::Point {
        x: x + dx,
        y: z + dz,
        surface: None,
    }
}

/// Players of a crowd battle.
pub const CROWD_PLAYERS: u8 = 8;

/// Eight groups of `per_player` soldiers in a ring around the middle of the starts, each
/// facing the centre: the race's animated, trainable, non-hero fighters in turn.
fn crowd(
    map: &mut map::UtcMap,
    content: &Content,
    per_player: u32,
) -> Result<canopy_sim::MatchSetup> {
    use canopy_content::{
        game::Kind,
        placement::{Owner, Placement, Point},
    };
    let race = content
        .rules
        .races
        .get(&content.rules.default_race)
        .context("the default race")?;
    let fighters: Vec<&canopy_content::Id> = race
        .roster
        .iter()
        .filter(|id| {
            content.definition(id.as_str()).is_some_and(|d| {
                d.kind == Kind::Unit
                    && !d.is_hero()
                    && d.behaviors.combat.is_some()
                    && d.behaviors.work.is_none()
                    && content
                        .asset(d.asset.as_str())
                        .is_some_and(|a| a.character.is_some())
            })
        })
        .collect();
    anyhow::ensure!(!fighters.is_empty(), "the default race has no soldiers");
    // A colony without buildings is defeated at once (skirmish rules): each player keeps a
    // small building behind its soldiers.
    let shelter = race
        .roster
        .iter()
        .filter_map(|id| content.definition(id.as_str()))
        .filter(|d| d.kind == Kind::Building)
        .min_by_key(|d| d.footprint.as_ref().map_or(u32::MAX, |f| f.width * f.depth))
        .context("the default race has no building")?;
    let n = map.player_starts.len().max(1) as f64;
    let cx = map.player_starts.iter().map(|s| s.x).sum::<f64>() / n;
    let cz = map.player_starts.iter().map(|s| s.z).sum::<f64>() / n;
    let side = (f64::from(per_player)).sqrt().ceil() as u32;
    for player in 0..CROWD_PLAYERS {
        let angle = f64::from(player) / f64::from(CROWD_PLAYERS) * std::f64::consts::TAU;
        let (dx, dz) = (angle.sin(), angle.cos());
        let (gx, gz) = (cx + dx * 24.0, cz + dz * 24.0);
        let facing = (-dx).atan2(-dz).to_degrees().rem_euclid(360.0);
        map.entities.push(Placement {
            activation: None,
            id: format!("crowd-{player}-shelter"),
            map_knowledge: None,
            definition: shelter.id.clone(),
            position: aligned(shelter, gx + dx * 22.0, gz + dz * 22.0),
            rotation: 0.0,
            owner: Owner::Player(player),
            initial_state: None,
            appearance: None,
        });
        for i in 0..per_player {
            let (row, column) = (i / side, i % side);
            // Rows step away from the centre, columns across.
            let across = f64::from(column) - f64::from(side - 1) / 2.0;
            let back = f64::from(row);
            // One unit per navigation cell, so none waits to be placed.
            let x = (gx + dz * across * 1.5 + dx * back * 1.5).round();
            let z = (gz - dx * across * 1.5 + dz * back * 1.5).round();
            let definition = fighters[(i as usize + player as usize) % fighters.len()].clone();
            map.entities.push(Placement {
                activation: None,
                id: format!("crowd-{player}-{i}"),
                map_knowledge: None,
                definition,
                position: Point {
                    x,
                    y: z,
                    surface: None,
                },
                rotation: facing,
                owner: Owner::Player(player),
                initial_state: None,
                appearance: None,
            });
        }
    }
    Ok(canopy_sim::MatchSetup {
        seed: 1,
        slots: (0..CROWD_PLAYERS)
            .map(|player| canopy_sim::Slot {
                player,
                kind: canopy_sim::SlotKind::Human,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    })
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
    let look = Look::load(&paths.data, map.biome())?;
    let setup = match spec.crowd {
        Some(n) => crowd(&mut map, &content, n)?,
        None => canopy_scene::spectator_setup(&map, spec.players),
    };
    let session = Session::local(setup, None);
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
