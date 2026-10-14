//! The match terrain, compiled once from the map and shared read-only by every system
//! (docs/simulation.md, "Map"). It is derived data: rebuilt from the map and content,
//! never saved.
//!
//! What the old game derived at load (`Spatial`'s terrain construction in
//! `src/sim/game/spatial.ts`, after `compileMapScene`):
//!
//! - per-cell ground and water heights in centimetres, sampled from the committed grids;
//! - ground walkability: ground no more than [`WADING_DEPTH_CM`] below the water surface,
//!   minus the cells scenery blockers close;
//! - resource placements and stamps projected from placed scenery objects;
//! - bridge decks as a layered walk grid with static regions;
//! - height maxima for sight and weapon checks, and directed steep edges for movement.

pub mod blockers;
pub mod bridges;
pub mod clearance;
pub mod field;
pub mod footprint;
pub mod scene;
pub mod surfaces;
pub mod tactical;

pub use bridges::BridgeSurface;
pub use clearance::SteepEdges;
pub use field::HeightField;
pub use footprint::CellBounds;
pub use scene::Scene;
pub use surfaces::{DeckId, SurfaceNode, SurfacePoint, WalkSurfaces};
pub use tactical::{
    MAX_GROUND_STEP_CM, SIGHT_HEIGHT_CM, TacticalTerrain, TerrainPoint, UPHILL_VISIBILITY_CM,
    VisibleSpans,
};

use canopy_content::{
    Content,
    assets::{self, Blocker, SceneryEntry},
    conditions::Locomotion,
    game::Kind,
    map::{GridError, UtcMap},
};
use std::{collections::BTreeMap, sync::Arc};

/// Deepest water a ground unit wades through. Buildings still need dry land.
pub const WADING_DEPTH_CM: i32 = 60;

/// Why a map cannot be compiled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompileError {
    #[error("terrain dimensions ({terrain}) disagree with the map ({map})")]
    Size { terrain: u32, map: u32 },
    #[error("terrain grid {0}")]
    Grid(#[from] GridError),
    #[error("{0}")]
    Scene(String),
    #[error("{0}")]
    Surfaces(String),
}

/// `Math.round`: halves round toward positive infinity.
pub(crate) fn round_half_up(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// The compiled terrain of a match. Cell `(x, y)` of the playable area has index
/// `y * size + x`.
#[derive(Debug, Clone)]
pub struct Terrain {
    size: u32,
    field: HeightField,
    ground: Arc<TacticalTerrain>,
    water: Vec<i16>,
    walkable: Vec<bool>,
    surfaces: Option<WalkSurfaces>,
    steep: SteepEdges,
    scene: Scene,
}

impl Terrain {
    /// Compiles `map` against the content's landscape catalogue and scenery collision.
    pub fn compile(map: &UtcMap, content: &Content) -> Result<Self, CompileError> {
        let data = &map.authoring.terrain;
        if data.size != map.size {
            return Err(CompileError::Size {
                terrain: data.size,
                map: map.size,
            });
        }
        let field = HeightField::decode(data)?;
        let scene = scene::project(map, &content.landscape).map_err(CompileError::Scene)?;
        let size = map.size as usize;
        let mut heights = Vec::with_capacity(size * size);
        let mut water = Vec::with_capacity(size * size);
        let mut walkable = Vec::with_capacity(size * size);
        for y in 0..size {
            for x in 0..size {
                let (fx, fy) = (x as f64, y as f64);
                let ground = round_half_up(field.sample(fx, fy) * 100.0) as i16;
                let surface = round_half_up(field.water_at(fx, fy) * 100.0) as i16;
                heights.push(ground);
                water.push(surface);
                walkable.push(i32::from(ground) >= i32::from(surface) - WADING_DEPTH_CM);
            }
        }
        let collision = Collision::new(content);
        let decks = bridges::bridge_surfaces(
            &scene.stamps,
            |id| collision.decks.get(id).copied(),
            |x, z| field.sample(x, z),
        );
        blockers::apply(
            map.size,
            &scene.stamps,
            |id| collision.blockers.get(id).map_or(&[][..], Vec::as_slice),
            &mut walkable,
        );
        let ground = Arc::new(TacticalTerrain::new(map.size, heights));
        let surfaces = if decks.is_empty() {
            None
        } else {
            let clearance = round_half_up(ground_body_height(content) * 100.0) as i32;
            Some(
                WalkSurfaces::new(ground.clone(), walkable.clone(), decks, clearance)
                    .map_err(CompileError::Surfaces)?,
            )
        };
        let steep = SteepEdges::new(map.size, ground.heights());
        Ok(Self {
            size: map.size,
            field,
            ground,
            water,
            walkable,
            surfaces,
            steep,
            scene,
        })
    }

    /// Playable cells per side.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// The index of cell `(x, y)`, if it is on the map.
    pub fn cell(&self, x: i64, y: i64) -> Option<usize> {
        let size = i64::from(self.size);
        (x >= 0 && y >= 0 && x < size && y < size).then(|| (y * size + x) as usize)
    }

    /// Ground height of a cell in centimetres.
    pub fn height_cm(&self, cell: usize) -> i16 {
        self.ground.heights()[cell]
    }

    /// Water surface over a cell in centimetres (below the ground where dry).
    pub fn water_cm(&self, cell: usize) -> i16 {
        self.water[cell]
    }

    /// Whether ground units may stand on the cell's ground: shallow enough and not closed
    /// by scenery. Buildings, resources and units are the caller's to add.
    pub fn walkable(&self, cell: usize) -> bool {
        self.walkable[cell]
    }

    /// Ground heights of every cell in centimetres.
    pub fn heights(&self) -> &[i16] {
        self.ground.heights()
    }

    pub fn water(&self) -> &[i16] {
        &self.water
    }

    pub fn walkable_cells(&self) -> &[bool] {
        &self.walkable
    }

    /// The committed height and water fields, for sampling between vertices.
    pub fn field(&self) -> &HeightField {
        &self.field
    }

    /// Ground sight and weapon rules.
    pub fn tactical(&self) -> &TacticalTerrain {
        &self.ground
    }

    /// The layered walk grid, on maps with bridge decks.
    pub fn surfaces(&self) -> Option<&WalkSurfaces> {
        self.surfaces.as_ref()
    }

    /// Whether a bridge deck spans the cell.
    pub fn under_deck(&self, cell: usize) -> bool {
        self.surfaces
            .as_ref()
            .is_some_and(|s| s.has_deck(cell as u32))
    }

    pub fn steep_edges(&self) -> &SteepEdges {
        &self.steep
    }

    /// Stamps and resource placements projected from the map's scenery.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Floor height in world units at a placement position: the named deck, or the
    /// sampled ground. `None` for an unknown deck or a point off it.
    pub fn floor(&self, x: f64, y: f64, surface: Option<&str>) -> Option<f64> {
        match surface {
            None => Some(self.field.sample(x, y)),
            Some(name) => {
                let surfaces = self.surfaces.as_ref()?;
                surfaces.deck(surfaces.deck_id(name)?).height_at(x, y)
            }
        }
    }

    /// The deck of a surface name, on maps with decks.
    pub fn deck_id(&self, name: &str) -> Option<DeckId> {
        self.surfaces.as_ref()?.deck_id(name)
    }

    /// Floor height in world units under a body: the deck or bilinear ground on layered
    /// maps, otherwise the nearest cell's ground.
    pub fn height(&self, p: SurfacePoint) -> Option<f64> {
        if let Some(surfaces) = &self.surfaces {
            return surfaces.height(p);
        }
        let cell = self.cell(round_half_up(p.x) as i64, round_half_up(p.y) as i64)?;
        Some(f64::from(self.height_cm(cell)) / 100.0)
    }

    /// The highest physical floor at a point (ground, water or deck), in world units:
    /// flying bodies clear all of them.
    pub fn air_floor(&self, x: f64, y: f64) -> f64 {
        let last = i64::from(self.size) - 1;
        let cx = (round_half_up(x) as i64).clamp(0, last);
        let cy = (round_half_up(y) as i64).clamp(0, last);
        let cell = (cy * i64::from(self.size) + cx) as usize;
        let mut top = i32::from(self.height_cm(cell).max(self.water[cell]));
        if let Some(surfaces) = &self.surfaces {
            for &id in surfaces.column(cell as u32) {
                top = top.max(surfaces.node(id).expect("deck node").height);
            }
        }
        f64::from(top) / 100.0
    }

    /// Whether a straight shot from `a` reaches `b`.
    pub fn shot_clear(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        match &self.surfaces {
            Some(surfaces) => surfaces.shot_clear(a, b),
            None => self.ground.shot_clear(terrain_point(a), terrain_point(b)),
        }
    }

    /// Whether `b` is high enough above `a` to stay hidden from it.
    pub fn visible(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        match &self.surfaces {
            Some(surfaces) => surfaces.visible(a, b),
            None => self.ground.visible(terrain_point(a), terrain_point(b)),
        }
    }

    /// Whether `a` can strike `b` in melee across the terrain between them.
    pub fn melee_clear(&self, a: SurfacePoint, b: SurfacePoint) -> bool {
        match &self.surfaces {
            Some(surfaces) => surfaces.melee_clear(a, b),
            None => self.ground.melee_clear(terrain_point(a), terrain_point(b)),
        }
    }

    /// Whether the ground under `bounds` (on the map) is gentle enough for a foundation.
    pub fn foundation_fits(&self, bounds: CellBounds) -> bool {
        bounds.inside(self.size)
            && footprint::foundation_fits(bounds, |x, y| {
                i32::from(self.heights()[(y * i64::from(self.size) + x) as usize])
            })
    }
}

fn terrain_point(p: SurfacePoint) -> TerrainPoint {
    TerrainPoint {
        x: p.x,
        y: p.y,
        elevation: p.elevation,
    }
}

/// Decks and blockers of the published scenery, by scenery id.
struct Collision<'a> {
    decks: BTreeMap<&'a str, &'a SceneryEntry>,
    blockers: BTreeMap<&'a str, Vec<Blocker>>,
}

impl<'a> Collision<'a> {
    fn new(content: &'a Content) -> Self {
        let mut decks = BTreeMap::new();
        let mut blockers = BTreeMap::new();
        for entry in assets::scenery(&content.manifest) {
            if entry.deck.is_some() {
                decks.insert(entry.id.as_str(), entry);
            }
            let list: Vec<Blocker> = entry
                .blocker
                .iter()
                .chain(entry.blockers.iter().flatten())
                .cloned()
                .collect();
            if !list.is_empty() {
                blockers.insert(entry.id.as_str(), list);
            }
        }
        Self { decks, blockers }
    }
}

/// The smallest height among ground units (the old `minimumGroundBody`): the headroom
/// a deck must leave above the floor below it.
fn ground_body_height(content: &Content) -> f64 {
    content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Unit)
        .filter(|d| {
            d.behaviors.movement.as_ref().and_then(|m| m.locomotion) != Some(Locomotion::Air)
        })
        .filter_map(|d| d.dimensions.as_ref().map(|b| b.height))
        .reduce(f64::min)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::round_half_up;

    #[test]
    fn halves_round_up() {
        assert_eq!(round_half_up(2.5), 3.0);
        assert_eq!(round_half_up(-2.5), -2.0);
        assert_eq!(round_half_up(-0.5), 0.0);
        assert_eq!(round_half_up(0.49999999999999994), 0.0);
        assert_eq!(round_half_up(1234.4999), 1234.0);
    }
}
