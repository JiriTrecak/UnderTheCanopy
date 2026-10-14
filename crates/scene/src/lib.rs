//! `canopy-scene`: the only bridge between the simulation (and maps) and the renderer
//! (docs/structure.md). It turns a map and a match's state into the renderer's plain
//! world description; later it will follow simulation snapshots every tick.

pub mod statics;
pub mod terrain;

use anyhow::{Context, Result, bail};
use canopy_content::{Content, map::UtcMap};
use canopy_render::{
    World,
    environment::{BiomeConfig, CONFIG_FOLDER, Hex, LightingConfig},
    statics::Glow,
    terrain::TerrainDesc,
};
use canopy_sim::{MatchSetup, Slot, SlotKind};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// `config/players.json`: player colours by slot.
pub const PLAYERS_FILE: &str = "players.json";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Players {
    pub colors: Vec<Hex>,
}

impl Players {
    pub fn load(data: &Path) -> Result<Self> {
        let path = data.join(CONFIG_FOLDER).join(PLAYERS_FILE);
        let players: Self = serde_json::from_slice(
            &std::fs::read(&path).with_context(|| path.display().to_string())?,
        )
        .with_context(|| path.display().to_string())?;
        if players.colors.len() < 8 {
            bail!(
                "{}: needs a colour for each of the 8 players",
                path.display()
            );
        }
        Ok(players)
    }
}

/// `config/glow.json`: models that glow by their colour (old deposit glow).
pub const GLOW_FILE: &str = "glow.json";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlowEntry {
    /// Render assets whose models glow.
    pub assets: Vec<String>,
    pub color: [f32; 3],
    pub strength: f32,
    pub terms: [canopy_render::statics::GlowTerm; 3],
}

impl GlowEntry {
    pub fn glow(&self) -> Glow {
        Glow {
            color: self.color,
            strength: self.strength,
            terms: self.terms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glows {
    pub glows: Vec<GlowEntry>,
}

impl Glows {
    pub fn load(data: &Path) -> Result<Self> {
        let path = data.join(CONFIG_FOLDER).join(GLOW_FILE);
        let glows: Self = serde_json::from_slice(
            &std::fs::read(&path).with_context(|| path.display().to_string())?,
        )
        .with_context(|| path.display().to_string())?;
        for entry in &glows.glows {
            let g = entry.glow();
            if !(0.0..=8.0).contains(&g.strength) || g.terms.iter().any(|t| t.to <= t.from) {
                bail!(
                    "{}: glow strengths are 0–8 and every term ends after it starts",
                    path.display()
                );
            }
        }
        Ok(glows)
    }

    /// Glows by model entry, through the content's render assets.
    pub fn by_model(&self, content: &Content) -> Result<BTreeMap<String, Glow>> {
        let mut out = BTreeMap::new();
        for entry in &self.glows {
            for id in &entry.assets {
                let asset = content
                    .asset(id)
                    .with_context(|| format!("glow: unknown render asset {id}"))?;
                let file = asset
                    .file
                    .as_ref()
                    .with_context(|| format!("glow: {id} has no model"))?;
                out.insert(file.clone(), entry.glow());
            }
        }
        Ok(out)
    }
}

/// The configuration a map's look needs.
pub struct Look {
    pub lighting: LightingConfig,
    pub biome: BiomeConfig,
    pub players: Players,
    pub glows: Glows,
}

impl Look {
    /// Reads `config/` for the map's biome.
    pub fn load(data: &Path, biome: &str) -> Result<Self> {
        let lighting = LightingConfig::load(data)?;
        let biome = BiomeConfig::load(data, biome, &lighting)?;
        Ok(Self {
            lighting,
            biome,
            players: Players::load(data)?,
            glows: Glows::load(data)?,
        })
    }
}

/// Every start of the map as a computer player: what a spectator watches.
pub fn spectator_setup(map: &UtcMap) -> MatchSetup {
    MatchSetup {
        seed: 1,
        slots: map
            .player_starts
            .iter()
            .map(|start| Slot {
                player: start.player.saturating_sub(1),
                kind: SlotKind::Ai,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

/// A map as the renderer draws it at match start.
pub struct MapScene {
    pub world: World,
    /// Hour of the day the map sets (noon when it sets none).
    pub hour: f32,
    /// Playable cells per side.
    pub size: u32,
    /// Player starts (x, z), in player order.
    pub starts: Vec<[f32; 2]>,
    /// What could not be shown (missing models), for warnings.
    pub missing: Vec<String>,
}

impl MapScene {
    pub fn terrain(&self) -> &TerrainDesc {
        self.world
            .terrain
            .as_ref()
            .expect("map scenes have terrain")
    }
}

/// Builds the scene of `map` at match start: compiles the match like the simulation
/// (starting setups included) and describes terrain and static models.
pub fn build(
    content: Arc<Content>,
    map: &UtcMap,
    pack: &canopy_assets::Pack,
    look: &Look,
) -> Result<MapScene> {
    let _span = tracing::info_span!("scene_build").entered();
    if map.size > 1024 {
        bail!("maps larger than 1024 cells are not supported (D24)");
    }
    let world = canopy_sim::World::new(content.clone(), map, spectator_setup(map))
        .map_err(|e| anyhow::anyhow!("cannot start the match: {}", e.0))?;
    let terrain =
        terrain::build(map, &content.landscape, &look.biome).map_err(anyhow::Error::msg)?;
    let scenery: statics::SceneryModels = pack
        .scenery()?
        .prototypes
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let colors: Vec<[f32; 3]> = look
        .players
        .colors
        .iter()
        .map(|c| c.linear().to_array())
        .collect();
    let (instances, lights, missing) = statics::build(&world, &content, map, &scenery, &colors);
    for m in &missing {
        tracing::warn!(what = %m, "not shown");
    }
    let hour = map
        .landscape
        .as_ref()
        .map_or(12.0, |l| l.environment.hour as f32);
    Ok(MapScene {
        world: World {
            terrain: Some(terrain),
            statics: instances,
            glows: look.glows.by_model(&content)?,
            point_lights: lights,
            actor_models: Vec::new(),
        },
        hour,
        size: map.size,
        starts: map
            .player_starts
            .iter()
            .map(|s| [s.x as f32, s.z as f32])
            .collect(),
        missing,
    })
}
