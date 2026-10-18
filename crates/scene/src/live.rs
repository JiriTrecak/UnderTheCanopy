//! The live scene: each simulation tick's view turned into posed models (old
//! `SettlementLayer.update`, `HarvestTrees`, `ConstructionScaffold`, `ProjectileEffects`).
//!
//! [`Presenter::update`] runs once per tick on the simulation thread and produces a
//! [`Presentation`]: every model to draw with its transform, owner colour, opacity and
//! blended poses at that tick, plus which static trees are hidden. Everything is a
//! function of ticks (no wall clock), so the same tick presents identically; the render
//! thread interpolates two presentations ([`crate::interpolate`]).
//!
//! What it shows, following the old game:
//! - **Units** stand on their floor (deck or ground), flyers at their flight height,
//!   lookout occupants on the building's firing side and lift; held units are hidden.
//!   Animation follows [`crate::animation`]; workers face their work and carry goods at
//!   their back socket; a hit tilts the body briefly.
//! - **Deaths**: a unit that dies in sight plays its death clip for two seconds where it
//!   fell (flyers drop to the floor), then vanishes. Fallen heroes stay hidden until
//!   revived.
//! - **Buildings** show their depletion stage (deposits) and, while under construction,
//!   the race's scaffold rising from the ground until the reveal point, then retracting.
//! - **Trees** stay in the static scene while standing; once struck they leave it and
//!   play their hit, fall and decay clips at 1× from the felling ticks, then disappear.
//! - **Missiles and shells** fly from the shooter's projectile socket (or 1.5 m above its
//!   floor) to the target along the effect's arc, between launch and impact.
//!
//! New (D46): a melee blow that takes health throws a puff of dust along the swing, and
//! damaged buildings in sight burn ([`crate::damage`]).

use crate::{
    animation::{
        BuildingAnimation, CONTACT, CastTimeline, Poses, TICK_SECONDS, UnitAnimation, UnitInputs,
        attack_phase,
    },
    damage::{Damaged, Fires},
    effects::{
        BladeSample, EffectDirector, EffectScene, EffectSnapshot, HitDust, ModelRef, Play, Spark,
    },
    models::{ModelCatalog, ModelInfo},
    obstruction::Hull,
};
use canopy_assets::{Matrix3x4, Pack};
use canopy_content::map::UtcMap;
use canopy_content::{
    Content,
    effects::VisualLayer,
    game::{ConstructionPresentation, Creation, Definition, Kind, RenderAsset, WorkAnimation},
    placement::Owner,
};
use canopy_sim::{
    World,
    catalog::Catalog,
    combat::{Missile, Shell},
    entity::{EntityId, Milli},
    terrain::Terrain,
    view::{EntityView, View},
    vision::Sighting,
};
use glam::{EulerRot, Quat, Vec3};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Ticks a death stays on screen (old corpse `remaining: 2` seconds).
pub const CORPSE_TICKS: u32 = 80;
/// A flyer's corpse falls `15 t²` metres after `t` seconds (old corpse fall).
pub const CORPSE_FALL: f32 = 15.0;
/// Moving further than this in one tick is a jump, not a walk: no interpolation.
pub const TELEPORT_DISTANCE: f32 = 3.0;
/// The socket carried goods hang from (old `socket_back` convention).
pub const CARGO_SOCKET: &str = "socket_back";
/// Carried goods are shown at this fraction of their model (old cargo scale).
pub const CARGO_SCALE: f32 = 0.7;
/// Missiles start this far above the shooter's floor without a socket.
pub const LAUNCH_HEIGHT: f32 = 1.5;
/// Height above the target's floor where missiles land; shells land on the ground.
pub const MISSILE_IMPACT_HEIGHT: f32 = 1.0;
pub const SHELL_IMPACT_HEIGHT: f32 = 0.1;

/// What a presented model is, the high bits of its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Part {
    Entity = 0,
    Corpse = 1,
    Scaffold = 2,
    Cargo = 3,
    Tree = 4,
    Missile = 5,
    Shell = 6,
}

/// A stable identity across ticks: part, layer and id.
pub fn key(part: Part, layer: u8, id: u32) -> u64 {
    (u64::from(part as u8) << 40) | (u64::from(layer) << 32) | u64::from(id)
}

/// One posed model at one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Actor {
    pub key: u64,
    pub model: u32,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub color: Option<[f32; 3]>,
    pub opacity: f32,
    pub poses: Poses,
    /// Do not interpolate from the previous tick (a jump).
    pub snap: bool,
    /// Selection body: circle radius and pick height in metres (units and buildings).
    pub body: Option<[f32; 2]>,
    /// A ground unit that wakes water: its walking surface (0 the ground, `n` the deck
    /// [`deck_index`] `n − 1`) and height above it.
    pub walker: Option<(u32, f32)>,
    /// How much a building works, 0 at rest to 1: its active emission (D65).
    pub activity: f32,
}

/// The walking surface number of a deck name (0: the ground; unknown decks walk the
/// ground).
pub fn deck_index(terrain: &Terrain, surface: Option<&str>) -> u32 {
    surface
        .and_then(|name| terrain.deck_id(name))
        .map_or(0, |id| u32::from(id.0) + 1)
}

/// Water and walking heights for the ripple emitter: the terrain's water and ground,
/// and the simulation's bridge decks.
pub struct SceneWater<'a> {
    pub desc: &'a canopy_render::terrain::TerrainDesc,
    pub terrain: &'a Terrain,
}

impl canopy_render::water::ripples::WaterField for SceneWater<'_> {
    fn water(&self, x: f32, z: f32) -> f32 {
        self.desc.water(x, z)
    }

    fn walk(&self, x: f32, z: f32, surface: u32) -> f32 {
        let deck = surface
            .checked_sub(1)
            .and_then(|i| self.terrain.surfaces()?.decks().get(i as usize));
        match deck {
            Some(deck) => self
                .terrain
                .floor(f64::from(x), f64::from(z), Some(&deck.id))
                .map_or_else(|| self.desc.walk(x, z, 0), |h| h as f32),
            None => self.desc.walk(x, z, 0),
        }
    }
}

impl Actor {
    fn new(key: u64, model: u32, position: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self {
            key,
            model,
            position,
            rotation,
            scale,
            color: None,
            opacity: 1.0,
            poses: Poses::default(),
            snap: false,
            body: None,
            walker: None,
            activity: 0.0,
        }
    }
}

/// Static trees hidden by the live scene, by their index in the static instance list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HiddenStatics {
    /// Advances when any bit changes.
    pub revision: u32,
    pub bits: Arc<Vec<u64>>,
}

impl HiddenStatics {
    pub fn get(&self, index: usize) -> bool {
        self.bits
            .get(index / 64)
            .is_some_and(|w| w & (1 << (index % 64)) != 0)
    }
}

/// Counts for logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub units: u32,
    pub buildings: u32,
    pub corpses: u32,
    pub felling: u32,
    pub missiles: u32,
    pub shells: u32,
}

/// Everything the renderer draws of one tick's live scene.
#[derive(Debug, Clone, Default)]
pub struct Presentation {
    pub tick: u32,
    /// Model entries by index (append-only across ticks).
    pub models: Arc<Vec<String>>,
    /// Sorted by key.
    pub actors: Vec<Actor>,
    pub hidden: HiddenStatics,
    pub counts: Counts,
    /// The centre (x, z) of the largest fight: units swinging, by 16-cell neighbourhood.
    pub battle: Option<[f32; 2]>,
    /// Spell and combat effects playing at this tick.
    pub effects: EffectSnapshot,
    /// The viewer's fog of war (`None` for observers).
    pub fog: Option<canopy_sim::view::FogView>,
    /// Units and buildings the world UI may mark, by id.
    pub bodies: Vec<Body>,
}

/// A rally point (cells) of a producer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RallyMark {
    pub x: f32,
    pub z: f32,
    pub surface: u32,
    /// A unit or resource it follows.
    pub target: Option<u32>,
}

/// What the world UI shows of a unit or building at a tick (old `SettlementLayer`
/// overlays: vital bars, circles, rallies, the mine label, badges).
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub id: u32,
    /// Its model's key in `actors` (absent while a scaffold stands for it).
    pub key: u64,
    /// Where it stands at this tick.
    pub position: Vec3,
    pub building: bool,
    /// An item on the ground: picked and ringed, but no minimap dot.
    pub item: bool,
    pub hero: bool,
    pub relation: canopy_sim::teams::Relation,
    pub owner: Option<u8>,
    pub hp: Option<u32>,
    pub max_hp: u32,
    /// Known to its owner (and observers).
    pub mana: Option<u32>,
    pub max_mana: u32,
    /// Selection circle radius and pick height (metres).
    pub radius: f32,
    pub height: f32,
    pub remembered: bool,
    /// Fights the viewer's side unprompted.
    pub hostile: Option<bool>,
    /// Walking surface (0 the ground, `n` the deck `n − 1`).
    pub surface: u32,
    /// Workers gathering for a base, and how many it wants.
    pub workforce: Option<(u32, u32)>,
    pub rally: Option<RallyMark>,
    /// Icon assets of its item statuses.
    pub badges: Vec<String>,
    /// Construction or training progress, 0–1.
    pub progress: Option<f32>,
    /// A building still under construction.
    pub constructing: bool,
    /// Cast progress, 0–1 (a channel counts down).
    pub cast: Option<f32>,
    /// Its owner's attack order target.
    pub commanded: Option<u32>,
    /// Its model, scale and idle clip (index, seconds): the portrait.
    pub model: u32,
    pub scale: f32,
    pub idle: Option<(u16, f32)>,
    /// A building's footprint on the map (cells, turned by its rotation): the minimap.
    pub footprint: Option<[f32; 2]>,
    /// A deposit with something left to harvest.
    pub deposit: bool,
}

/// The foundations drawn under the buildings of `bodies` (D44): each footprint on the
/// ground at its building's base height.
pub fn foundations(
    bodies: &[Body],
    out: &mut Vec<canopy_render::terrain::foundations::Foundation>,
) {
    out.clear();
    out.extend(bodies.iter().filter(|b| b.surface == 0).filter_map(|b| {
        let [w, d] = b.footprint?;
        Some(canopy_render::terrain::foundations::Foundation {
            center: [b.position.x, b.position.z],
            half: [w / 2.0, d / 2.0],
            base: b.position.y,
        })
    }));
}

/// Walking heights with the foundations' mounds: ground marks of the world UI (rings,
/// rally routes) drape over a mound instead of vanishing under it (D44).
pub struct FoundationGround<'a> {
    pub ground: &'a dyn canopy_render::water::ripples::WaterField,
    pub foundations: &'a [canopy_render::terrain::foundations::Foundation],
}

impl canopy_render::water::ripples::WaterField for FoundationGround<'_> {
    fn water(&self, x: f32, z: f32) -> f32 {
        self.ground.water(x, z)
    }

    fn walk(&self, x: f32, z: f32, surface: u32) -> f32 {
        let floor = self.ground.walk(x, z, surface);
        if surface != 0 {
            return floor;
        }
        let ground = |x: f32, z: f32| self.ground.walk(x, z, 0);
        self.foundations
            .iter()
            .filter(|f| {
                let (min, max) = f.bounds();
                (min.x..=max.x).contains(&x) && (min.z..=max.z).contains(&z)
            })
            .filter_map(|f| f.surface(x, z, &ground))
            .fold(floor, f32::max)
    }
}

/// The centre of the densest group of `points` (16-cell buckets and their neighbours).
pub fn densest(points: &[[f32; 2]]) -> Option<[f32; 2]> {
    let bucket = |p: &[f32; 2]| ((p[0] / 16.0).floor() as i32, (p[1] / 16.0).floor() as i32);
    let mut counts: BTreeMap<(i32, i32), u32> = BTreeMap::new();
    for p in points {
        *counts.entry(bucket(p)).or_default() += 1;
    }
    let near = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1;
    let score = |b: (i32, i32)| -> u32 {
        counts
            .iter()
            .filter(|(k, _)| near(**k, b))
            .map(|(_, c)| c)
            .sum()
    };
    let best = counts
        .keys()
        .copied()
        .max_by_key(|&b| (score(b), std::cmp::Reverse(b)))?;
    let group: Vec<&[f32; 2]> = points.iter().filter(|p| near(bucket(p), best)).collect();
    let n = group.len() as f32;
    Some([
        group.iter().map(|p| p[0]).sum::<f32>() / n,
        group.iter().map(|p| p[1]).sum::<f32>() / n,
    ])
}

impl Presentation {
    pub fn actor(&self, key: u64) -> Option<&Actor> {
        self.actors
            .binary_search_by_key(&key, |a| a.key)
            .ok()
            .map(|i| &self.actors[i])
    }
}

/// What a match start tells the presenter.
pub struct Setup {
    /// Player colours (linear) by slot.
    pub colors: Vec<[f32; 3]>,
    /// The static instance index of each tree entity (`statics::build`).
    pub tree_slots: BTreeMap<EntityId, usize>,
    /// Static instances in all.
    pub statics: usize,
}

/// Where a unit last stood, for its death.
#[derive(Debug, Clone)]
struct Tracked {
    asset: String,
    model: u32,
    animation: Option<UnitAnimation>,
    actor: Actor,
    /// The flight floor while airborne.
    floor: Option<f32>,
    garrison: Option<EntityId>,
    seen: u32,
}

/// A built building's animation and the state effects it is playing (D65).
#[derive(Debug, Clone)]
struct BuildingTrack {
    model: u32,
    animation: Option<BuildingAnimation>,
    /// Cues of looping state effects, by the effect's index in the presentation.
    cues: BTreeMap<usize, Vec<u32>>,
    /// Eases towards 1 while it works and back to 0 at rest.
    activity: f32,
    seen: u32,
}

#[derive(Debug, Clone)]
struct Corpse {
    actor: Actor,
    animation: Option<UnitAnimation>,
    died: u32,
    fall: Option<(f32, f32)>,
}

/// A projectile effect layer as drawn.
#[derive(Debug, Clone)]
struct Layer {
    model: u32,
    arc: f32,
    local: Quat,
    size: f32,
    offset: Vec3,
}

/// Construction dust of one building (old `ConstructionScaffold` dust timing).
#[derive(Debug, Clone, Default)]
struct Dust {
    progress: Option<f64>,
    last: f64,
    next: f64,
    cues: Vec<u32>,
}

/// The blade sockets melee ribbons follow (old convention).
pub const BLADE_BASE: &str = "socket_blade_base";
pub const BLADE_TIP: &str = "socket_blade_tip";
/// The part of a swing that leaves a ribbon (old `MeleeTrails`: phase 0.43–0.69).
pub const BLADE_PHASE: (f32, f32) = (0.43, 0.69);
/// Blade samples per tick (the old ribbons sampled every frame).
const BLADE_SUBSTEPS: u32 = 4;

/// A unit's model and pose this tick, for effect sockets.
struct Posed {
    info: Arc<ModelInfo>,
    clip: Option<(u16, f32)>,
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
}

/// What effects see of the scene this tick.
struct EffectWorld<'a> {
    anchors: &'a BTreeMap<u32, Vec3>,
    posed: &'a BTreeMap<u32, Posed>,
    terrain: &'a Terrain,
    missiles: &'a BTreeMap<u32, (Vec3, Vec3)>,
}

impl EffectScene for EffectWorld<'_> {
    fn anchor(&self, entity: u32) -> Option<Vec3> {
        self.anchors.get(&entity).copied()
    }

    fn socket(&self, entity: u32, socket: &str) -> Option<(Vec3, Quat)> {
        let p = self.posed.get(&entity)?;
        let (clip, time) = p.clip.unwrap_or((0, 0.0));
        let m = p.info.socket(socket, clip, time)?;
        Some((
            p.position + p.rotation * (affine_point(&m, Vec3::ZERO) * p.scale),
            p.rotation * affine_rotation(&m),
        ))
    }

    fn floor(&self, position: Milli, surface: Option<&str>) -> f32 {
        let (x, y) = cells(position);
        self.terrain
            .floor(x, y, surface)
            .unwrap_or_else(|| self.terrain.field().sample(x, y)) as f32
    }

    fn missile_pose(&self, cast: u32) -> Option<(Vec3, Vec3)> {
        self.missiles.get(&cast).copied()
    }
}

/// Old dust randomness: a hash of the building and tick, never the simulation's stream.
fn dust_random(seed: u32, salt: u32) -> f32 {
    let mut n = seed ^ (salt + 1).wrapping_mul(0x85eb_ca6b);
    n = (n ^ (n >> 16)).wrapping_mul(0x7feb_352d);
    n = (n ^ (n >> 15)).wrapping_mul(0x846c_a68b);
    (f64::from(n ^ (n >> 16)) / 4_294_967_296.0) as f32
}

pub struct Presenter {
    content: Arc<Content>,
    terrain: Arc<Terrain>,
    pack: Option<Arc<Pack>>,
    pub catalog: ModelCatalog,
    colors: Vec<[f32; 3]>,
    /// Map placements' appearance by entity.
    appearance: BTreeMap<EntityId, (Option<String>, f64)>,
    construction: BTreeMap<String, ConstructionPresentation>,
    units: BTreeMap<EntityId, Tracked>,
    buildings: BTreeMap<EntityId, BuildingTrack>,
    corpses: BTreeMap<EntityId, Corpse>,
    origins: BTreeMap<(Part, u32), (u32, Vec3)>,
    projectiles: BTreeMap<String, Vec<Layer>>,
    tree_slots: BTreeMap<EntityId, usize>,
    hidden: HiddenStatics,
    felling: BTreeMap<EntityId, Sighting>,
    chunks: Option<Arc<[Arc<[Sighting]>]>>,
    previous: BTreeMap<u64, Vec3>,
    pub effects: EffectDirector,
    /// Health as last presented, for hit sparks.
    health: BTreeMap<EntityId, u32>,
    dust: BTreeMap<EntityId, Dust>,
    /// Construction dust effect and interval by building definition.
    dust_effects: BTreeMap<String, (usize, u32)>,
    /// Weapon missiles carrying an ability cast this tick: position and direction.
    missile_poses: BTreeMap<u32, (Vec3, Vec3)>,
    /// Icon assets of item definitions.
    item_icons: BTreeMap<canopy_sim::catalog::Def, String>,
    /// Fires of damaged buildings.
    fires: Fires,
    /// Solid meshes of burning buildings' models, by model (fire spots sit on them).
    hulls: BTreeMap<u32, Option<Arc<Hull>>>,
    /// The tick presented last (melee blows landed since then throw dust).
    last_tick: Option<u32>,
    /// The world jumped (a replay seek): the next presentation rebuilds the hidden trees
    /// from the world instead of from the change since the last one.
    jumped: bool,
}

/// A melee blow that lands this tick.
#[derive(Debug, Clone, Copy)]
struct Blow {
    /// The striker's facing on the ground (unit).
    direction: Vec3,
    power: f32,
    seed: u32,
}

fn yaw(millidegrees: i32) -> f32 {
    (f64::from(millidegrees) / 1000.0).to_radians() as f32
}

fn cells(p: Milli) -> (f64, f64) {
    (
        f64::from(p.x) / f64::from(Milli::CELL),
        f64::from(p.y) / f64::from(Milli::CELL),
    )
}

/// Applies a 3×4 row-major affine to a point.
fn affine_point(m: &Matrix3x4, p: Vec3) -> Vec3 {
    Vec3::new(
        m[0] * p.x + m[1] * p.y + m[2] * p.z + m[3],
        m[4] * p.x + m[5] * p.y + m[6] * p.z + m[7],
        m[8] * p.x + m[9] * p.y + m[10] * p.z + m[11],
    )
}

fn affine_rotation(m: &Matrix3x4) -> Quat {
    let x = Vec3::new(m[0], m[4], m[8]).normalize_or_zero();
    let y = Vec3::new(m[1], m[5], m[9]).normalize_or_zero();
    let z = Vec3::new(m[2], m[6], m[10]).normalize_or_zero();
    Quat::from_mat3(&glam::Mat3::from_cols(x, y, z)).normalize()
}

/// The scaffold's rise and the building's reveal at construction `progress` (old
/// `constructionPose`).
pub fn construction_pose(progress: f64, c: &ConstructionPresentation) -> (f64, bool) {
    let p = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let rise = if p < c.reveal_at {
        p / c.reveal_at
    } else {
        (1.0 - (p - c.reveal_at) / (c.retract_until - c.reveal_at)).max(0.0)
    };
    (rise, p >= c.reveal_at)
}

/// The model file of an effect layer's model reference (`<asset>/geometry[_n].glb`).
fn layer_model_path(content: &Content, asset: &str, index: u32) -> Option<String> {
    let record = content
        .manifest
        .records
        .iter()
        .find(|r| r.id.as_str() == asset)?;
    let stem = match index {
        1 => "geometry.".to_owned(),
        n => format!("geometry_{n}."),
    };
    record
        .outputs
        .iter()
        .map(|o| o.path.as_str())
        .find(|p| p.rsplit('/').next().is_some_and(|f| f.starts_with(&stem)))
        .map(str::to_owned)
}

impl Presenter {
    /// A presenter for a match on `map` whose world starts as `world`.
    pub fn new(world: &World, map: &UtcMap, pack: Option<Arc<Pack>>, setup: Setup) -> Self {
        let content = world.catalog().content.clone();
        // Map placements' appearance (asset and scale) by entity.
        let placements: BTreeMap<&str, &canopy_content::placement::Placement> = map
            .entities
            .iter()
            .chain(&world.terrain().scene().resources)
            .map(|p| (p.id.as_str(), p))
            .collect();
        let mut appearance = BTreeMap::new();
        for e in world.state().entities.iter() {
            if let Some(a) = e
                .placement
                .as_deref()
                .and_then(|id| placements.get(id))
                .and_then(|p| p.appearance.as_ref())
            {
                appearance.insert(
                    e.id,
                    (
                        a.asset.as_ref().map(|s| s.to_string()),
                        a.scale.unwrap_or(1.0),
                    ),
                );
            }
        }
        let mut construction = BTreeMap::new();
        for race in content.rules.races.values() {
            if let Some(style) = &race.construction {
                for id in &race.roster {
                    construction.insert(id.to_string(), style.clone());
                }
            }
        }
        let words = setup.statics.div_ceil(64);
        let mut catalog = ModelCatalog::default();
        let table = EffectDirector::table(pack.as_deref());
        let effects = EffectDirector::new(&content, table.clone(), |name| {
            let index = catalog.index(pack.as_deref(), name);
            let info = catalog.get(index);
            (!info.missing).then(|| ModelRef {
                index,
                triangles: info.triangles,
                clips: info
                    .clips
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.name.clone(), i as u16, c.duration))
                    .collect(),
            })
        });
        let mut dust_effects = BTreeMap::new();
        for race in content.rules.races.values() {
            if let Some(style) = &race.construction
                && let Some(effect) = table.find(style.dust_effect.as_str())
            {
                for id in &race.roster {
                    dust_effects.insert(id.to_string(), (effect, style.dust_interval_ticks));
                }
            }
        }
        let mut presenter = Self {
            content,
            terrain: world.terrain().clone(),
            pack,
            catalog,
            colors: setup.colors,
            appearance,
            construction,
            units: BTreeMap::new(),
            buildings: BTreeMap::new(),
            corpses: BTreeMap::new(),
            origins: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            tree_slots: setup.tree_slots,
            hidden: HiddenStatics {
                revision: 0,
                bits: Arc::new(vec![0; words]),
            },
            felling: BTreeMap::new(),
            chunks: None,
            previous: BTreeMap::new(),
            effects,
            health: BTreeMap::new(),
            dust: BTreeMap::new(),
            dust_effects,
            missile_poses: BTreeMap::new(),
            item_icons: world
                .catalog()
                .content
                .definitions
                .iter()
                .filter(|d| d.kind == Kind::Item)
                .filter_map(|d| Some((world.catalog().find(d.id.as_str())?, d.icon.to_string())))
                .collect(),
            fires: Fires::default(),
            hulls: BTreeMap::new(),
            last_tick: None,
            jumped: false,
        };
        presenter.preload(world);
        presenter
    }

    /// Models the match will show, loaded up front: everything present at the start, the
    /// rosters of the races playing (with their scaffolds and projectiles), every item
    /// and deposit stage. Anything else (summons) loads when first seen.
    fn preload(&mut self, world: &World) {
        let content = self.content.clone();
        let mut definitions: BTreeSet<&str> = BTreeSet::new();
        let catalog = world.catalog();
        let mut felling: BTreeSet<String> = BTreeSet::new();
        for e in world.state().entities.iter() {
            definitions.insert(catalog.id(e.definition).as_str());
            if e.resource.as_ref().is_some_and(|r| r.felling.is_some()) {
                let asset = self
                    .appearance
                    .get(&e.id)
                    .and_then(|a| a.0.clone())
                    .unwrap_or_else(|| catalog.get(e.definition).model.to_string());
                if let Some(file) = content
                    .render_of(catalog.get(e.definition), &asset)
                    .and_then(|a| a.harvest_animation.clone())
                {
                    felling.insert(file);
                }
            }
        }
        // Trees fall in every match: their felling models too.
        for file in felling {
            self.catalog.index(self.pack.as_deref(), &file);
        }
        for slot in &world.setup().slots {
            let race = slot.race.as_deref().unwrap_or(&content.rules.default_race);
            if let Some(race) = content.rules.races.get(race) {
                definitions.extend(race.roster.iter().map(|id| id.as_str()));
                if let Some(style) = &race.construction {
                    self.asset_model(style.scaffold.as_str());
                }
            }
        }
        for d in &content.definitions {
            if matches!(d.kind, Kind::Item) || d.resource_visuals.is_some() {
                definitions.insert(d.id.as_str());
            }
        }
        for id in definitions {
            let Some(d) = content.definition(id) else {
                continue;
            };
            if matches!(d.kind, Kind::Resource) {
                continue;
            }
            self.asset_model(d.model.as_str());
            if let Some(visuals) = &d.resource_visuals {
                for stage in &visuals.stages {
                    self.asset_model(stage.asset.as_str());
                }
                for variant in &visuals.variants {
                    self.asset_model(variant.as_str());
                }
            }
            if let Some(effect) = content
                .look(d.id.as_str())
                .and_then(|a| a.projectile.clone())
            {
                self.projectile_layers(effect.as_str());
            }
        }
    }

    /// Forgets what the presenter tracked from tick to tick (deaths, corpses, felling trees,
    /// hits, dust, fires, effects in play), keeping its models: the world jumped to another
    /// moment (a replay seek). The next presentation shows that moment afresh.
    pub fn reset(&mut self) {
        self.units.clear();
        self.corpses.clear();
        self.origins.clear();
        self.felling.clear();
        self.chunks = None;
        self.previous.clear();
        self.health.clear();
        self.dust.clear();
        self.missile_poses.clear();
        self.fires = Fires::default();
        self.last_tick = None;
        self.effects.reset();
        self.jumped = true;
    }

    /// The model of render asset `id`, if it has one.
    fn asset_model(&mut self, id: &str) -> Option<u32> {
        let file = self.content.asset(id)?.file.clone()?;
        Some(self.catalog.index(self.pack.as_deref(), &file))
    }

    fn projectile_layers(&mut self, effect: &str) -> Vec<Layer> {
        if let Some(layers) = self.projectiles.get(effect) {
            return layers.clone();
        }
        let content = self.content.clone();
        let mut layers = Vec::new();
        if let Some(effect_def) = content.effects.iter().find(|e| e.id.as_str() == effect) {
            for layer in effect_def.layers.iter().filter(|l| l.enabled) {
                if let Some(drawn) = self.layer(&content, layer) {
                    layers.push(drawn);
                }
            }
        }
        self.projectiles.insert(effect.to_owned(), layers.clone());
        layers
    }

    fn layer(&mut self, content: &Content, layer: &VisualLayer) -> Option<Layer> {
        let model = layer.model.as_ref()?;
        let path = layer_model_path(content, model.asset.as_str(), model.index)?;
        let r = &model.rotation;
        let offset = layer.offset.as_ref().map_or(Vec3::ZERO, |o| {
            Vec3::new(o.x as f32, o.y as f32, o.z as f32)
        }) + Vec3::Y * layer.height as f32;
        Some(Layer {
            model: self.catalog.index(self.pack.as_deref(), &path),
            arc: layer.arc_height.unwrap_or(0.0) as f32,
            local: Quat::from_euler(
                EulerRot::XYZ,
                (r.x as f32).to_radians(),
                (r.y as f32).to_radians(),
                (r.z as f32).to_radians(),
            ),
            size: layer.size as f32,
            offset,
        })
    }

    fn color(&self, owner: Owner) -> Option<[f32; 3]> {
        match owner {
            Owner::Player(slot) => self
                .colors
                .get(usize::from(slot) % self.colors.len().max(1))
                .copied(),
            Owner::Nobody => None,
        }
    }

    fn floor(&self, position: Milli, surface: Option<&str>) -> f32 {
        let (x, y) = cells(position);
        self.terrain
            .floor(x, y, surface)
            .unwrap_or_else(|| self.terrain.field().sample(x, y)) as f32
    }

    /// The render asset an entity shows now: its form, its placement's appearance, its
    /// depletion stage, or its definition's.
    fn look(&self, e: &EntityView, d: &Definition) -> (String, f64) {
        let (placed, scale) = self.appearance.get(&e.id()).cloned().unwrap_or((None, 1.0));
        let mut asset = placed.unwrap_or_else(|| d.model.to_string());
        let mut scale = scale;
        if let Some((form, form_scale)) = e.magic.as_ref().and_then(|m| m.form.as_ref()) {
            if let Some(form) = form {
                asset = form.clone();
            }
            scale *= form_scale;
        }
        if let (Some(visuals), Some(resource)) = (&d.resource_visuals, &e.public.resource) {
            let initial = resource.initial_amount.or(d.resource_yield).unwrap_or(0);
            let fraction = if resource.amount == 0 {
                0.0
            } else if initial > 0 {
                f64::from(resource.amount) / f64::from(initial)
            } else {
                1.0
            };
            if let Some(stage) = visuals.stages.iter().find(|s| fraction <= s.remaining) {
                asset = stage.asset.to_string();
            }
        }
        (asset, scale)
    }

    /// Presents one tick. `held(id)` says whether a unit is held inside another entity
    /// (the view does not carry it yet).
    pub fn update(
        &mut self,
        world: &World,
        view: &View,
        held: &dyn Fn(EntityId) -> bool,
    ) -> Presentation {
        let _span = tracing::trace_span!("present").entered();
        let tick = view.tick;
        let content = self.content.clone();
        let catalog = world.catalog();
        let mut actors: Vec<Actor> = Vec::with_capacity(view.entities.len() + 64);
        let mut counts = Counts::default();
        let fallen: BTreeSet<EntityId> = view.battle.fallen.iter().copied().collect();
        let mut seen: BTreeSet<EntityId> = BTreeSet::new();
        let mut fighting: Vec<[f32; 2]> = Vec::new();
        let by_id: BTreeMap<EntityId, &EntityView> =
            view.entities.iter().map(|e| (e.id(), e)).collect();
        // Who swings at whom (the first released swing), for the direction of hit sparks.
        let mut attackers: BTreeMap<EntityId, (f64, f64)> = BTreeMap::new();
        for a in &view.entities {
            if let Some(attack) = a.combat.as_ref().and_then(|c| c.attack.as_ref())
                && attack.released
                && let Some(target) = attack.target
            {
                attackers
                    .entry(target)
                    .or_insert_with(|| cells(a.public.position));
            }
        }
        // Melee blows landed since the last presentation, by target (a view of tick `t`
        // shows the steps before `t`, so a blow at its impact tick shows one tick later):
        // each throws dust along the swing when it takes health.
        let since = self
            .last_tick
            .filter(|&t| t < tick)
            .unwrap_or(tick.saturating_sub(1));
        let mut blows: BTreeMap<EntityId, Vec<Blow>> = BTreeMap::new();
        {
            let senses = world.senses();
            for a in view.entities.iter().filter(|a| !a.remembered) {
                if let Some(attack) = a.combat.as_ref().and_then(|c| c.attack.as_ref())
                    && attack.released
                    && attack.impact >= since
                    && attack.impact < tick
                    && let Some(target) = attack.target
                    && senses.combat.weapon(a.definition()).is_some_and(|w| {
                        matches!(w.delivery, canopy_sim::combat::rules::Delivery::Melee)
                    })
                {
                    let d = catalog.get(a.definition());
                    let facing = yaw(a.public.rotation);
                    let scale = d.model_scale.unwrap_or(1.0) as f32;
                    blows.entry(target).or_default().push(Blow {
                        direction: Vec3::new(facing.sin(), 0.0, facing.cos()),
                        power: scale * if d.is_hero() { 1.2 } else { 1.0 },
                        seed: a.id().0.wrapping_mul(0x9e37_79b1) ^ attack.started,
                    });
                }
            }
        }
        let mut damaged: Vec<Damaged> = Vec::new();
        let mut posed: BTreeMap<u32, Posed> = BTreeMap::new();
        let mut bodies: Vec<Body> = Vec::new();
        let mut health: BTreeMap<EntityId, u32> = BTreeMap::new();
        let mut building: BTreeSet<EntityId> = BTreeSet::new();

        for e in &view.entities {
            let id = e.id();
            if fallen.contains(&id) || held(id) {
                continue;
            }
            let d = catalog.get(e.definition());
            let (asset_id, appearance_scale) = self.look(e, d);
            let Some(asset) = content.render_of(d, &asset_id) else {
                continue;
            };
            if asset.scenery_asset.is_some() {
                // Owned trees are rare; they stay in the static scene like neutral ones.
                continue;
            }
            let Some(file) = asset.file.clone() else {
                continue;
            };
            let model = self.catalog.index(self.pack.as_deref(), &file);
            let info = self.catalog.shared(model);
            let visual_scale = d.model_scale.unwrap_or(1.0);
            let scale = (asset.scale.unwrap_or(1.0) * appearance_scale * visual_scale) as f32;
            let mut position = {
                let (x, z) = cells(e.public.position);
                Vec3::new(
                    x as f32,
                    self.floor(e.public.position, e.public.surface.as_deref()),
                    z as f32,
                )
            };
            let mut rotation_y = yaw(e.public.rotation);
            let entity_key = key(Part::Entity, 0, id.0);
            let mut actor = Actor::new(
                entity_key,
                model,
                position,
                Quat::from_rotation_y(rotation_y),
                Vec3::splat(scale),
            );
            actor.color = self.color(e.public.owner);
            actor.opacity = e.magic.as_ref().map_or(1.0, |m| m.opacity as f32);
            let mut hide = false;
            // A confirmed loss of health throws sparks (old `ImpactEffects`).
            if let Some(hp) = e.public.hp.filter(|_| !e.remembered) {
                if self.health.get(&id).is_some_and(|&before| hp < before)
                    && matches!(d.kind, Kind::Unit | Kind::Building)
                {
                    let (x, z) = cells(e.public.position);
                    let angle = attackers.get(&id).map_or(rotation_y, |&(ax, az)| {
                        ((x - ax) as f32).atan2((z - az) as f32)
                    });
                    let lift = if d.kind == Kind::Building {
                        1.7
                    } else {
                        1.1 * visual_scale as f32
                    };
                    self.effects.spark(Spark {
                        entity: id.0,
                        position: position + Vec3::Y * lift,
                        tick: f64::from(tick),
                        angle,
                        power: if d.is_hero() { 1.4 } else { 1.0 },
                    });
                    // Dust where each melee blow landed: on the struck body's near side,
                    // about waist high, blown along the striker's swing.
                    if let Some(blows) = blows.get(&id) {
                        let [radius, height] =
                            Self::body(d, asset.health_height, visual_scale as f32, &info, scale);
                        let lift = if d.kind == Kind::Building {
                            0.9
                        } else {
                            (height * 0.35).clamp(0.4, 1.6)
                        };
                        for blow in blows {
                            // At the body's edge facing the striker.
                            let near = if d.kind == Kind::Building {
                                radius * 0.8
                            } else {
                                radius * 0.9
                            };
                            self.effects.hit_dust(HitDust {
                                position: position - blow.direction * near + Vec3::Y * lift,
                                tick: f64::from(tick),
                                direction: blow.direction,
                                lift,
                                power: blow.power,
                                seed: blow.seed,
                            });
                        }
                    }
                }
                health.insert(id, hp);
            }
            match d.kind {
                Kind::Unit => {
                    counts.units += 1;
                    seen.insert(id);
                    let combat = e.combat.as_ref();
                    let magic = e.magic.as_ref();
                    if combat.is_some_and(|c| c.attack.is_some()) {
                        fighting.push([position.x, position.z]);
                    }
                    let flight = magic.and_then(|m| m.flight).map(|cm| cm as f32 / 100.0);
                    let floor = position.y;
                    if let Some(f) = flight {
                        position.y += f;
                    }
                    let garrison = combat.and_then(|c| c.garrison);
                    if let Some(host) = garrison.and_then(|g| by_id.get(&g)) {
                        let host_def = catalog.get(host.definition());
                        if let Some(g) = &host_def.garrison {
                            let host_scale = self.appearance.get(&host.id()).map_or(1.0, |a| a.1);
                            let radius = (g.lookout_radius.unwrap_or(0.0) * host_scale) as f32;
                            position.x += rotation_y.sin() * radius;
                            position.z += rotation_y.cos() * radius;
                            position.y += g.height as f32;
                        }
                    }
                    let tracked = self.units.entry(id).or_insert_with(|| Tracked {
                        asset: String::new(),
                        model,
                        animation: None,
                        actor,
                        floor: None,
                        garrison,
                        seen: tick,
                    });
                    if tracked.asset != asset_id || tracked.model != model {
                        tracked.asset = asset_id.clone();
                        tracked.model = model;
                        tracked.animation = UnitAnimation::new(&info);
                        actor.snap = true;
                    }
                    if tracked.garrison != garrison {
                        actor.snap = true;
                        tracked.garrison = garrison;
                    }
                    let work = e.work.as_ref();
                    let cast = magic.and_then(|m| m.casting.as_ref()).and_then(|c| {
                        let ability = content.abilities.abilities.get(usize::from(c.ability.0))?;
                        let presentation = &ability.presentation;
                        Some((
                            c.ability.0,
                            &presentation.animations,
                            CastTimeline {
                                start: c.start_tick,
                                release: c.resolve_tick,
                                finish: c.finish_tick,
                                channel_end: c.channel_end,
                            },
                        ))
                    });
                    let work_state = work.and_then(|w| w.pose).map(|p| {
                        (
                            match p.animation {
                                WorkAnimation::Build => "build",
                                WorkAnimation::Chop => "chop",
                            },
                            p.cycle,
                            p.target,
                        )
                    });
                    if let Some(animation) = &mut tracked.animation {
                        let inputs = UnitInputs {
                            tick,
                            moving: e.motion.is_some_and(|m| m.moving),
                            strolling: e.motion.is_some_and(|m| m.strolling),
                            charging: combat.is_some_and(|c| c.charging),
                            attack: combat
                                .and_then(|c| c.attack.as_ref())
                                .map(|a| (a.started, a.impact, a.ends)),
                            hp: e.public.hp,
                            cast,
                            carrying: work.is_some_and(|w| w.cargo.is_some()),
                            work: work_state.map(|(s, c, _)| (s, c)),
                            works: asset
                                .character
                                .as_ref()
                                .is_some_and(|c| c.as_str() == "base"),
                            cast_contact: asset.cast_contact.map(|c| c as f32),
                        };
                        let decision = animation.step(&info, &inputs);
                        if decision.face_work
                            && let Some((_, _, target)) = work_state
                        {
                            let (tx, tz) = cells(target);
                            let (x, z) = cells(e.public.position);
                            rotation_y = ((tx - x) as f32).atan2((tz - z) as f32);
                        }
                        actor.poses = animation.animator.poses(&info);
                        let recoil = animation.recoil(tick);
                        actor.rotation = Quat::from_rotation_y(rotation_y)
                            * Quat::from_rotation_x(-0.10 * recoil);
                        position += Quat::from_rotation_y(rotation_y)
                            * Vec3::new(0.0, 0.0, -0.045 * recoil);
                    } else {
                        actor.rotation = Quat::from_rotation_y(rotation_y);
                    }
                    actor.position = position;
                    if let Some(animation) = &tracked.animation {
                        posed.insert(
                            id.0,
                            Posed {
                                info: self.catalog.shared(model),
                                clip: Some(animation.animator.current()),
                                position,
                                rotation: actor.rotation,
                                scale: actor.scale,
                            },
                        );
                        // Melee blade ribbons through the middle of the swing.
                        if let (Some(attack), Some(state)) = (
                            combat.and_then(|c| c.attack.as_ref()),
                            info.state(crate::animation::states::ATTACK),
                        ) && animation.animator.state() == crate::animation::states::ATTACK
                            && info.sockets.iter().any(|s| s == BLADE_BASE)
                            && info.sockets.iter().any(|s| s == BLADE_TIP)
                        {
                            let contact = state.contact.unwrap_or(CONTACT);
                            let duration = info.duration(state.clip);
                            for k in (0..BLADE_SUBSTEPS).rev() {
                                let at = tick as f32 - k as f32 / BLADE_SUBSTEPS as f32;
                                let phase = attack_phase(
                                    at,
                                    attack.started,
                                    attack.impact,
                                    attack.ends,
                                    contact,
                                );
                                if phase < BLADE_PHASE.0 || phase > BLADE_PHASE.1 {
                                    continue;
                                }
                                let time = phase.min(0.999_999) * duration;
                                let world = |socket| {
                                    info.socket(socket, state.clip, time).map(|m| {
                                        position
                                            + actor.rotation
                                                * (affine_point(&m, Vec3::ZERO) * actor.scale)
                                    })
                                };
                                if let (Some(base), Some(tip)) =
                                    (world(BLADE_BASE), world(BLADE_TIP))
                                {
                                    self.effects.blade(
                                        id.0,
                                        attack.started,
                                        BladeSample {
                                            tick: f64::from(at),
                                            base,
                                            tip,
                                        },
                                    );
                                }
                            }
                        }
                    }
                    // Ground bodies wake water (not flyers or lookout occupants).
                    if flight.is_none() && garrison.is_none() {
                        actor.walker =
                            Some((deck_index(&self.terrain, e.public.surface.as_deref()), 0.0));
                    }
                    actor.body = Some(Self::body(
                        d,
                        asset.health_height,
                        visual_scale as f32,
                        &info,
                        scale,
                    ));
                    tracked.floor = flight.map(|_| floor);
                    tracked.seen = tick;
                    tracked.actor = actor;
                    let current = tracked.animation.as_ref().map(|a| a.animator.current());
                    // Carried goods at the back socket (or the old fixed offset).
                    if let Some(cargo) = work.and_then(|w| w.cargo.as_ref())
                        && let Some(item) = content.definition(cargo.item.as_str())
                        && let Some(item_asset) = content.look(item.id.as_str())
                        && let Some(item_file) = item_asset.file.clone()
                    {
                        let item_model = self.catalog.index(self.pack.as_deref(), &item_file);
                        let item_scale = CARGO_SCALE * item_asset.scale.unwrap_or(1.0) as f32;
                        let socket =
                            current.and_then(|(clip, time)| info.socket(CARGO_SOCKET, clip, time));
                        let rest = info
                            .clips
                            .first()
                            .and_then(|_| info.socket(CARGO_SOCKET, 0, 0.0));
                        let (offset, turn) = match (socket, rest) {
                            (Some(now), Some(rest)) => (
                                affine_point(&now, Vec3::ZERO) * scale,
                                affine_rotation(&now) * affine_rotation(&rest).inverse(),
                            ),
                            _ => (
                                Vec3::new(0.0, 1.04, 0.48) * visual_scale as f32,
                                Quat::IDENTITY,
                            ),
                        };
                        let world_scale = if socket.is_some() {
                            scale
                        } else {
                            visual_scale as f32
                        };
                        let mut goods = Actor::new(
                            key(Part::Cargo, 0, id.0),
                            item_model,
                            actor.position + actor.rotation * offset,
                            actor.rotation * turn,
                            Vec3::splat(world_scale * item_scale),
                        );
                        goods.snap = actor.snap;
                        goods.opacity = actor.opacity;
                        actors.push(goods);
                    }
                }
                Kind::Building => {
                    counts.buildings += 1;
                    let mut show_body = true;
                    if let Some(progress) = e.public.construction
                        && let Some(style) = self.construction.get(d.id.as_str()).cloned()
                    {
                        let total = match &d.creation {
                            Some(Creation::Construct { work_ticks, .. }) => *work_ticks,
                            _ => 0,
                        };
                        let p = if total > 0 {
                            f64::from(progress) / f64::from(total)
                        } else {
                            1.0
                        };
                        let (rise, revealed) = construction_pose(p, &style);
                        show_body = revealed;
                        building.insert(id);
                        self.construction_dust(
                            e,
                            d.id.as_str(),
                            &info,
                            scale,
                            &style,
                            (p, rise),
                            (position, actor.rotation),
                            tick,
                        );
                        if rise > 0.0
                            && let Some(scaffold) = self.asset_model(style.scaffold.as_str())
                        {
                            let sinfo = self.catalog.shared(scaffold);
                            let bmin = Vec3::from_array(info.rest_min) * scale;
                            let bmax = Vec3::from_array(info.rest_max) * scale;
                            let pad = style.padding as f32 * 2.0;
                            let size = (bmax - bmin) + Vec3::splat(pad);
                            let centre = (bmin + bmax) * 0.5;
                            let smin = Vec3::from_array(sinfo.rest_min);
                            let smax = Vec3::from_array(sinfo.rest_max);
                            let source = (smax - smin).max(Vec3::splat(0.001));
                            let fit = size / source;
                            let origin = (smin + smax) * 0.5;
                            let ground = bmin.y.min(0.0);
                            let buried = size.y * (1.0 - rise as f32);
                            let local = Vec3::new(
                                centre.x - origin.x * fit.x,
                                ground - buried - smin.y * fit.y,
                                centre.z - origin.z * fit.z,
                            );
                            let mut s = Actor::new(
                                key(Part::Scaffold, 0, id.0),
                                scaffold,
                                position + actor.rotation * local,
                                actor.rotation,
                                fit,
                            );
                            s.color = actor.color;
                            actors.push(s);
                        }
                    }
                    actor.body = Some(Self::body(
                        d,
                        asset.health_height,
                        visual_scale as f32,
                        &info,
                        scale,
                    ));
                    hide = !show_body;
                    if e.public.construction.is_none() && !info.missing {
                        self.animate_building(e, asset, model, &info, &mut actor, &mut posed, tick);
                    }
                    // Built buildings in sight burn when damaged (deposits and mines do not).
                    let max_hp = e
                        .combat
                        .as_ref()
                        .map(|c| c.stats.max_hp)
                        .filter(|&m| m > 0)
                        .or_else(|| d.body.as_ref().map(|b| b.max_hp));
                    if e.public.construction.is_none()
                        && e.public.resource.is_none()
                        && !e.remembered
                        && let (Some(hp), Some(max_hp)) = (e.public.hp, max_hp)
                        && hp > 0
                        && max_hp > 0
                        && !info.missing
                    {
                        damaged.push(Damaged {
                            entity: id.0,
                            model,
                            health: hp as f32 / max_hp as f32,
                            position,
                            rotation: actor.rotation,
                            scale,
                        });
                    }
                }
                Kind::Item => {
                    // Picked by the model as drawn, on a lake bed too: the bounds' height
                    // and half their widest side.
                    let w = (info.rest_max[0] - info.rest_min[0])
                        .max(info.rest_max[2] - info.rest_min[2]);
                    let h = info.rest_max[1] - info.rest_min[1];
                    actor.body = Some([(w * 0.5 * scale).max(0.3), (h * scale).max(0.3)]);
                }
                _ => {}
            }
            if matches!(d.kind, Kind::Unit | Kind::Building | Kind::Item) {
                bodies.push(self.body_of(e, d, &actor, tick));
            }
            if !hide {
                actors.push(actor);
            }
        }

        // Units that left the view: deaths in sight become corpses; the rest vanish.
        let stale: Vec<EntityId> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.seen != tick)
            .map(|(&id, _)| id)
            .collect();
        for id in stale {
            let track = self.buildings.remove(&id).expect("listed");
            for cues in track.cues.values() {
                self.effects.stop(cues);
            }
        }
        let gone: Vec<EntityId> = self
            .units
            .keys()
            .copied()
            .filter(|id| !seen.contains(id))
            .collect();
        for id in gone {
            let tracked = self.units.remove(&id).expect("listed");
            let died = view.deaths.iter().any(|c| c.sighting.id == id);
            if !died || tracked.seen + 1 < tick {
                continue;
            }
            let mut animation = tracked.animation;
            if let Some(a) = &mut animation {
                a.die(self.catalog.get(tracked.model));
            }
            let mut actor = tracked.actor;
            actor.key = key(Part::Corpse, 0, id.0);
            actor.walker = None;
            actor.snap = false;
            self.corpses.insert(
                id,
                Corpse {
                    actor,
                    animation,
                    died: tick,
                    fall: tracked.floor.map(|f| (tracked.actor.position.y, f)),
                },
            );
        }
        self.corpses.retain(|_, c| tick < c.died + CORPSE_TICKS);
        let corpse_ids: Vec<EntityId> = self.corpses.keys().copied().collect();
        for id in corpse_ids {
            let model_info = {
                let c = &self.corpses[&id];
                self.catalog.shared(c.actor.model)
            };
            let c = self.corpses.get_mut(&id).expect("listed");
            if tick > c.died
                && let Some(a) = &mut c.animation
            {
                a.animator.update(&model_info, TICK_SECONDS);
            }
            if let Some(a) = &c.animation {
                c.actor.poses = a.animator.poses(&model_info);
            }
            if let Some((from, floor)) = c.fall {
                let t = (tick - c.died) as f32 * TICK_SECONDS;
                c.actor.position.y = (from - CORPSE_FALL * t * t).max(floor);
            }
            actors.push(c.actor);
            counts.corpses += 1;
        }

        if std::mem::take(&mut self.jumped) {
            // After a jump the trees felled since the start are hidden afresh: those gone
            // from the world, then (below) whatever the view lists as struck or empty.
            let mut bits = vec![0u64; self.hidden.bits.len()];
            for (&id, &slot) in &self.tree_slots {
                if world.state().entities.get(id).is_none()
                    && let Some(word) = bits.get_mut(slot / 64)
                {
                    *word |= 1 << (slot % 64);
                }
            }
            self.hidden = HiddenStatics {
                revision: self.hidden.revision + 1,
                bits: Arc::new(bits),
            };
        }
        self.trees(catalog, view, &mut actors, &mut counts);
        self.flights(catalog, view, &by_id, &mut actors, &mut counts);

        self.health = health;
        self.last_tick = Some(tick);
        let fires = {
            let (catalog, pack, hulls) = (&self.catalog, self.pack.as_deref(), &mut self.hulls);
            self.fires.update(&damaged, |model| {
                let info = catalog.get(model);
                let hull = hulls
                    .entry(model)
                    .or_insert_with(|| pack.and_then(|p| Hull::of(p, &info.name)).map(Arc::new))
                    .clone();
                (
                    Vec3::from_array(info.rest_min),
                    Vec3::from_array(info.rest_max),
                    hull,
                )
            })
        };
        self.effects.burn(fires);
        let effects_dust = std::mem::take(&mut self.dust);
        self.dust = effects_dust
            .into_iter()
            .filter(|(id, dust)| {
                let keep = building.contains(id);
                if !keep {
                    self.effects.stop(&dust.cues);
                }
                keep
            })
            .collect();
        let anchors: BTreeMap<u32, Vec3> = actors
            .iter()
            .filter(|a| a.key >> 40 == Part::Entity as u64)
            .map(|a| (a.key as u32, a.position))
            .collect();
        let effects = self.effects.update(
            view,
            &EffectWorld {
                anchors: &anchors,
                posed: &posed,
                terrain: &self.terrain,
                missiles: &self.missile_poses,
            },
            catalog,
        );

        actors.sort_by_key(|a| a.key);
        // Jumps further than a walk do not interpolate.
        for a in &mut actors {
            if let Some(previous) = self.previous.get(&a.key)
                && previous.distance(a.position) > TELEPORT_DISTANCE
            {
                a.snap = true;
            }
        }
        self.previous = actors.iter().map(|a| (a.key, a.position)).collect();
        Presentation {
            tick,
            models: self.catalog.names().clone(),
            actors,
            hidden: self.hidden.clone(),
            counts,
            battle: densest(&fighting),
            effects,
            fog: view.fog.clone(),
            bodies,
        }
    }

    /// What the world UI shows of a unit or building.
    fn body_of(&self, e: &EntityView, d: &Definition, actor: &Actor, tick: u32) -> Body {
        let magic = e.magic.as_ref();
        let combat = e.combat.as_ref();
        let economy = e.private.as_ref().map(|p| &p.economy);
        let progress = e
            .public
            .construction
            .and_then(|done| match &d.creation {
                Some(Creation::Construct { work_ticks, .. }) if *work_ticks > 0 => {
                    Some(done as f32 / *work_ticks as f32)
                }
                _ => None,
            })
            .or_else(|| {
                economy
                    .and_then(|w| w.training.as_ref())
                    .filter(|p| p.total > 0)
                    .map(|p| p.done as f32 / p.total as f32)
            });
        let cast = magic.and_then(|m| m.casting.as_ref()).map(|c| {
            let t = tick as f32;
            match c.channel_end {
                Some(end) if tick >= c.resolve_tick => {
                    1.0 - (t - c.resolve_tick as f32) / (end - c.resolve_tick).max(1) as f32
                }
                _ => (t - c.start_tick as f32) / (c.resolve_tick - c.start_tick).max(1) as f32,
            }
            .clamp(0.0, 1.0)
        });
        let rally = economy
            .and_then(|w| w.production.as_ref())
            .and_then(|p| p.rally.as_ref())
            .map(|r| RallyMark {
                x: r.point.x as f32,
                z: r.point.y as f32,
                surface: deck_index(&self.terrain, r.point.surface.as_deref()),
                target: r.target.map(|t| t.0),
            });
        let badges = magic
            .map(|m| {
                m.item_statuses
                    .iter()
                    .map(|s| self.icon_of(s.item))
                    .collect()
            })
            .unwrap_or_default();
        let [radius, height] = actor.body.unwrap_or([0.9, 2.5]);
        Body {
            id: e.id().0,
            key: actor.key,
            position: actor.position,
            building: d.kind == Kind::Building,
            item: d.kind == Kind::Item,
            hero: d.is_hero(),
            relation: e.relation,
            owner: match e.public.owner {
                Owner::Player(slot) => Some(slot),
                Owner::Nobody => None,
            },
            hp: e.public.hp,
            max_hp: combat
                .map(|c| c.stats.max_hp)
                .filter(|&m| m > 0)
                .or_else(|| d.body.as_ref().map(|b| b.max_hp))
                .unwrap_or(0),
            mana: magic.and_then(|m| m.caster.as_ref()).map(|c| c.mana),
            max_mana: magic.map_or(0, |m| m.max_mana),
            radius,
            height,
            remembered: e.remembered,
            hostile: combat.map(|c| c.hostile),
            surface: deck_index(&self.terrain, e.public.surface.as_deref()),
            workforce: economy
                .and_then(|w| w.base.as_ref())
                .map(|w| (w.workers, w.recommended)),
            rally,
            badges,
            progress,
            constructing: e.public.construction.is_some(),
            cast,
            commanded: combat.and_then(|c| c.commanded_target).map(|t| t.0),
            model: actor.model,
            scale: actor.scale.x,
            idle: self
                .catalog
                .get(actor.model)
                .state(crate::animation::states::IDLE)
                .map(|st| (st.clip, self.catalog.get(actor.model).duration(st.clip))),
            footprint: d
                .footprint
                .as_ref()
                .filter(|_| d.kind == Kind::Building)
                .map(|f| {
                    let quarter = ((f64::from(e.public.rotation) / 1000.0 / 90.0).round() as i64)
                        .rem_euclid(2)
                        == 1;
                    let (w, h) = (f.width as f32, f.depth as f32);
                    if quarter { [h, w] } else { [w, h] }
                }),
            deposit: e.public.resource.as_ref().is_some_and(|r| r.amount > 0),
        }
    }

    /// The icon asset of an item definition.
    fn icon_of(&self, item: canopy_sim::catalog::Def) -> String {
        self.item_icons.get(&item).cloned().unwrap_or_default()
    }

    /// A built building at work (D65): its rest, work and spawn clips from what anyone
    /// sees of it (busy, finished cycles), its sockets for effects, the looping effects of
    /// its current state and the one-shot effects of the clip events it crossed. A
    /// remembered building rests: what it does out of sight is not shown.
    #[allow(clippy::too_many_arguments)]
    fn animate_building(
        &mut self,
        e: &EntityView,
        asset: &RenderAsset,
        model: u32,
        info: &Arc<ModelInfo>,
        actor: &mut Actor,
        posed: &mut BTreeMap<u32, Posed>,
        tick: u32,
    ) {
        let id = e.id();
        let work = e.work.as_ref();
        let busy = !e.remembered && work.is_some_and(|w| w.busy);
        let cycles = work.map_or(0, |w| w.cycles);
        let track = self.buildings.entry(id).or_insert_with(|| BuildingTrack {
            model,
            animation: BuildingAnimation::new(info),
            cues: BTreeMap::new(),
            activity: 0.0,
            seen: tick,
        });
        if track.model != model {
            for cues in std::mem::take(&mut track.cues).values() {
                self.effects.stop(cues);
            }
            track.model = model;
            track.animation = BuildingAnimation::new(info);
        }
        track.seen = tick;
        let Some(animation) = &mut track.animation else {
            return;
        };
        let events = animation.step(info, busy, cycles);
        actor.poses = animation.animator.poses(info);
        let (clip, time) = animation.animator.current();
        let state = animation.animator.state().to_owned();
        // Machinery spins up and winds down over about half a second.
        let working = if state == crate::animation::building_states::IDLE {
            0.0
        } else {
            1.0
        };
        track.activity += (working - track.activity).clamp(-0.06, 0.06);
        actor.activity = track.activity;
        posed.insert(
            id.0,
            Posed {
                info: info.clone(),
                clip: Some((clip, time)),
                position: actor.position,
                rotation: actor.rotation,
                scale: actor.scale,
            },
        );
        let Some(effects) = asset.effects.as_ref() else {
            return;
        };
        let at = |socket: Option<&String>| -> Vec3 {
            let node = socket.map(|s| {
                asset
                    .sockets
                    .iter()
                    .flatten()
                    .find(|k| &k.name == s)
                    .map_or(s.as_str(), |k| k.node.as_str())
            });
            node.and_then(|n| info.socket(n, clip, time))
                .map_or(actor.position, |m| {
                    actor.position + actor.rotation * (affine_point(&m, Vec3::ZERO) * actor.scale)
                })
        };
        for (k, effect) in effects.iter().enumerate() {
            let Some(index) = self.effects.find(effect.effect.as_str()) else {
                continue;
            };
            let seed = id.0.wrapping_mul(0x9e37_79b1) ^ tick ^ (k as u32).wrapping_mul(7919);
            let play = |director: &mut EffectDirector| {
                director.play(Play {
                    effect: index,
                    tick: f64::from(tick),
                    seed,
                    at: at(effect.socket.as_ref()),
                    from: None,
                    radius: None,
                })
            };
            if let Some(wanted) = &effect.state {
                let active = !e.remembered && *wanted == state;
                let playing = track.cues.get(&k).is_some_and(|c| !c.is_empty());
                if active && !playing {
                    let cues = play(&mut self.effects);
                    track.cues.insert(k, cues);
                } else if !active && playing {
                    let cues = track.cues.remove(&k).unwrap_or_default();
                    self.effects.stop(&cues);
                }
            } else if let Some(event) = &effect.event
                && !e.remembered
                && events.iter().any(|n| n == event)
            {
                play(&mut self.effects);
            }
        }
    }

    /// Construction dust on a rising scaffold (old `ConstructionScaffold.update`): three
    /// puffs at once — two on the roof, one on a side — every interval while work advances
    /// in sight.
    #[allow(clippy::too_many_arguments)]
    fn construction_dust(
        &mut self,
        e: &EntityView,
        definition: &str,
        info: &ModelInfo,
        scale: f32,
        style: &ConstructionPresentation,
        (progress, rise): (f64, f64),
        (position, rotation): (Vec3, Quat),
        tick: u32,
    ) {
        let Some(&(effect, interval)) = self.dust_effects.get(definition) else {
            return;
        };
        let now = f64::from(tick);
        let visible = !e.remembered;
        let dust = self.dust.entry(e.id()).or_default();
        if !visible || progress >= 1.0 || now < dust.last {
            let cues = std::mem::take(&mut dust.cues);
            self.effects.stop(&cues);
        }
        let dust = self.dust.get_mut(&e.id()).expect("inserted");
        if visible
            && dust.progress.is_some_and(|before| progress > before)
            && progress < 1.0
            && now >= dust.next
        {
            let bmin = Vec3::from_array(info.rest_min) * scale;
            let bmax = Vec3::from_array(info.rest_max) * scale;
            let size = (bmax - bmin) + Vec3::splat(style.padding as f32 * 2.0);
            let centre = (bmin + bmax) * 0.5;
            let ground = bmin.y.min(0.0);
            let buried = size.y * (1.0 - rise as f32);
            let shown = size.y * rise as f32;
            let seed = e.id().0.wrapping_mul(0x9e37_79b1) ^ tick;
            let random = |salt| dust_random(seed, salt);
            let (x, z) = (size.x * 0.5, size.z * 0.5);
            let mut cues = Vec::new();
            for i in 0..3u32 {
                let mut local = Vec3::new(
                    (random(i * 5) * 2.0 - 1.0) * x,
                    size.y + 0.15,
                    (random(i * 5 + 1) * 2.0 - 1.0) * z,
                );
                if i == 2 {
                    let edge = (random(i * 5 + 2) * 4.0).floor() as u32;
                    if edge < 2 {
                        local.x = if edge == 0 { -1.0 } else { 1.0 } * (x + 0.35);
                    } else {
                        local.z = if edge == 2 { -1.0 } else { 1.0 } * (z + 0.35);
                    }
                    local.y = buried + (shown * random(i * 5 + 3)).max(0.15);
                }
                let root = Vec3::new(centre.x, ground - buried, centre.z) + local;
                cues.extend(self.effects.play(Play {
                    effect,
                    tick: now,
                    seed: seed.wrapping_add((i + 1).wrapping_mul(7919)),
                    at: position + rotation * root,
                    from: None,
                    radius: None,
                }));
            }
            let dust = self.dust.get_mut(&e.id()).expect("inserted");
            dust.cues.extend(cues);
            dust.last = now;
            dust.next = now + f64::from(interval) * f64::from(0.7 + random(31) * 0.6);
        }
        if let Some(dust) = self.dust.get_mut(&e.id()) {
            dust.progress = Some(progress);
        }
    }

    /// Selection circle radius and pick height (old `circleRadius`, `pickHeight`).
    fn body(
        d: &Definition,
        health_height: Option<f64>,
        visual: f32,
        info: &ModelInfo,
        scale: f32,
    ) -> [f32; 2] {
        let height = d
            .dimensions
            .as_ref()
            .map_or(health_height.unwrap_or(2.5) as f32 * visual, |b| {
                b.height as f32
            });
        let radius = match (&d.dimensions, &d.footprint) {
            (Some(b), _) => (b.radius as f32 + 0.32).max(b.formation_spacing as f32 * 0.45),
            (None, Some(f)) => {
                let w = (info.rest_max[0] - info.rest_min[0]) * scale;
                let z = (info.rest_max[2] - info.rest_min[2]) * scale;
                (f.width as f32 / 2.0)
                    .max(f.depth as f32 / 2.0)
                    .max(w * 0.45)
                    .max(z * 0.45)
                    + 0.35
            }
            (None, None) => 0.9 * visual,
        };
        [radius, height]
    }

    /// Felled trees: hide struck trees from the static scene and play their felling.
    fn trees(
        &mut self,
        catalog: &Catalog,
        view: &View,
        actors: &mut Vec<Actor>,
        counts: &mut Counts,
    ) {
        let chunks = view.scenery.chunks.clone();
        let changed: Vec<usize> = match &self.chunks {
            Some(old) if old.len() == chunks.len() => (0..chunks.len())
                .filter(|&i| !Arc::ptr_eq(&old[i], &chunks[i]))
                .collect(),
            _ => (0..chunks.len()).collect(),
        };
        let mut bits = None::<Vec<u64>>;
        let set = |bits: &mut Option<Vec<u64>>, slot: usize, hidden: bool, base: &Arc<Vec<u64>>| {
            let (word, bit) = (slot / 64, 1u64 << (slot % 64));
            let current = bits.as_ref().map_or(base[word], |b| b[word]);
            if (current & bit != 0) != hidden {
                let b = bits.get_or_insert_with(|| base.to_vec());
                if hidden {
                    b[word] |= bit;
                } else {
                    b[word] &= !bit;
                }
            }
        };
        let base = self.hidden.bits.clone();
        for &i in &changed {
            if let Some(old) = &self.chunks
                && let Some(old_chunk) = old.get(i)
            {
                // Trees that left the listing are gone.
                for s in old_chunk.iter() {
                    if !chunks[i].iter().any(|n| n.id == s.id) {
                        if let Some(&slot) = self.tree_slots.get(&s.id) {
                            set(&mut bits, slot, true, &base);
                        }
                        self.felling.remove(&s.id);
                    }
                }
            }
            for s in chunks[i].iter() {
                let Some(resource) = &s.resource else {
                    continue;
                };
                let struck = resource
                    .felling
                    .as_ref()
                    .is_some_and(|f| f.last_hit_tick.is_some());
                let standing = resource.amount > 0 && !struck;
                if let Some(&slot) = self.tree_slots.get(&s.id) {
                    set(&mut bits, slot, !standing, &base);
                }
                if struck {
                    self.felling.insert(s.id, s.clone());
                } else {
                    self.felling.remove(&s.id);
                }
            }
        }
        if let Some(bits) = bits {
            self.hidden = HiddenStatics {
                revision: self.hidden.revision + 1,
                bits: Arc::new(bits),
            };
        }
        self.chunks = Some(chunks);

        let tick = view.tick;
        let content = self.content.clone();
        let mut done = Vec::new();
        for (id, s) in &self.felling {
            let d = catalog.get(s.definition);
            let (Some(felling), Some(state)) = (
                &d.felling,
                s.resource.as_ref().and_then(|r| r.felling.as_ref()),
            ) else {
                continue;
            };
            let (asset_id, scale) = self
                .appearance
                .get(id)
                .map(|(a, s)| (a.clone().unwrap_or_else(|| d.model.to_string()), *s))
                .unwrap_or_else(|| (d.model.to_string(), 1.0));
            let Some(file) = content
                .render_of(d, &asset_id)
                .and_then(|a| a.harvest_animation.clone())
            else {
                continue;
            };
            let (clip_name, elapsed) = match state.fall_tick {
                None => (
                    "hit",
                    tick.saturating_sub(state.last_hit_tick.unwrap_or(tick)),
                ),
                Some(fall) => {
                    let elapsed = tick.saturating_sub(fall);
                    if elapsed >= felling.fall_ticks + felling.decay_ticks {
                        done.push(*id);
                        continue;
                    }
                    if elapsed >= felling.fall_ticks {
                        ("decay", elapsed - felling.fall_ticks)
                    } else {
                        ("fall", elapsed)
                    }
                }
            };
            let model = self.catalog.index(self.pack.as_deref(), &file);
            let info = self.catalog.get(model);
            let Some(clip) = info.clip(clip_name) else {
                continue;
            };
            let time = (elapsed as f32 * TICK_SECONDS).min(info.duration(clip));
            let rotation = match state.fall_tick {
                None => yaw(s.rotation),
                Some(_) => (state.direction.x as f32).atan2(state.direction.y as f32),
            };
            let (x, z) = cells(s.position);
            let mut actor = Actor::new(
                key(Part::Tree, 0, id.0),
                model,
                Vec3::new(x as f32, self.terrain.field().sample(x, z) as f32, z as f32),
                Quat::from_rotation_y(rotation),
                Vec3::splat(scale as f32),
            );
            actor.poses = Poses::single(clip, time);
            actors.push(actor);
            counts.felling += 1;
        }
        for id in done {
            self.felling.remove(&id);
        }
    }

    /// Missiles and shells between launch and impact.
    fn flights(
        &mut self,
        catalog: &Catalog,
        view: &View,
        by_id: &BTreeMap<EntityId, &EntityView>,
        actors: &mut Vec<Actor>,
        counts: &mut Counts,
    ) {
        let tick = view.tick;
        let content = self.content.clone();
        let mut live: BTreeSet<(Part, u32)> = BTreeSet::new();
        self.missile_poses.clear();
        struct Flight<'a> {
            part: Part,
            id: u32,
            cast: Option<u32>,
            source: EntityId,
            definition: canopy_sim::catalog::Def,
            origin: &'a canopy_sim::combat::projectiles::Aim,
            target: &'a canopy_sim::combat::projectiles::Aim,
            launched: u32,
            impact: u32,
        }
        let missiles = view.battle.missiles.iter().map(|m: &Missile| Flight {
            part: Part::Missile,
            id: m.id,
            cast: m.enhancement.as_ref().map(|e| e.cast),
            source: m.source,
            definition: m.definition,
            origin: &m.origin,
            target: &m.destination,
            launched: m.launched,
            impact: m.impact,
        });
        let shells = view.battle.shells.iter().map(|s: &Shell| Flight {
            part: Part::Shell,
            id: s.id,
            cast: None,
            source: s.source,
            definition: s.definition,
            origin: &s.origin,
            target: &s.target,
            launched: s.launched,
            impact: s.impact,
        });
        let flights: Vec<Flight> = missiles
            .chain(shells)
            .filter(|f| tick >= f.launched && tick < f.impact)
            .collect();
        for f in flights {
            let d = catalog.get(f.definition);
            let Some(asset) = content.look(d.id.as_str()) else {
                continue;
            };
            let Some(effect) = asset.projectile.clone() else {
                continue;
            };
            let layers = self.projectile_layers(effect.as_str());
            if layers.is_empty() {
                continue;
            }
            live.insert((f.part, f.id));
            let cached = self
                .origins
                .get(&(f.part, f.id))
                .filter(|(l, _)| *l == f.launched)
                .map(|o| o.1);
            let start = match cached {
                Some(p) => p,
                None => {
                    // Only a fresh launch samples the shooter's socket; late sightings use the
                    // recorded origin.
                    let socket = (tick <= f.launched + 1)
                        .then(|| {
                            self.launch_socket(f.source, asset.projectile_socket.as_deref(), by_id)
                        })
                        .flatten();
                    let p = socket.unwrap_or_else(|| {
                        let (x, z) = cells(f.origin.position);
                        Vec3::new(
                            x as f32,
                            self.floor(f.origin.position, f.origin.surface.as_deref())
                                + LAUNCH_HEIGHT
                                + f.origin.elevation_cm as f32 / 100.0,
                            z as f32,
                        )
                    });
                    self.origins.insert((f.part, f.id), (f.launched, p));
                    p
                }
            };
            let (x, z) = cells(f.target.position);
            let lift = if f.part == Part::Shell {
                SHELL_IMPACT_HEIGHT
            } else {
                MISSILE_IMPACT_HEIGHT
            };
            let end = Vec3::new(
                x as f32,
                self.floor(f.target.position, f.target.surface.as_deref())
                    + lift
                    + f.target.elevation_cm as f32 / 100.0,
                z as f32,
            );
            let t = ((tick - f.launched) as f32 / (f.impact - f.launched).max(1) as f32).max(0.0);
            let arc = layers[0].arc;
            let position = start.lerp(end, t) + Vec3::Y * 4.0 * arc * t * (1.0 - t);
            let mut tangent = end - start;
            tangent.y += 4.0 * arc * (1.0 - 2.0 * t);
            let tangent = tangent.try_normalize().unwrap_or(Vec3::Y);
            let rotation = Quat::from_rotation_arc(Vec3::Y, tangent);
            if let Some(cast) = f.cast {
                self.missile_poses.insert(cast, (position, tangent));
            }
            let scale = d.model_scale.unwrap_or(1.0) as f32;
            for (i, layer) in layers.iter().enumerate() {
                actors.push(Actor::new(
                    key(f.part, i as u8, f.id),
                    layer.model,
                    position + layer.offset,
                    rotation * layer.local,
                    Vec3::splat(scale * layer.size),
                ));
            }
            match f.part {
                Part::Shell => counts.shells += 1,
                _ => counts.missiles += 1,
            }
        }
        self.origins.retain(|k, _| live.contains(k));
    }

    /// World position of a shooter's projectile socket in its current pose.
    fn launch_socket(
        &self,
        source: EntityId,
        socket: Option<&str>,
        by_id: &BTreeMap<EntityId, &EntityView>,
    ) -> Option<Vec3> {
        let socket = socket?;
        by_id.get(&source)?;
        let tracked = self.units.get(&source)?;
        let info = self.catalog.get(tracked.model);
        let (clip, time) = tracked.animation.as_ref()?.animator.current();
        let m = info.socket(socket, clip, time)?;
        let a = &tracked.actor;
        Some(a.position + a.rotation * (affine_point(&m, Vec3::ZERO) * a.scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffolds_rise_then_retract_after_the_reveal() {
        let style = ConstructionPresentation {
            scaffold: canopy_content::Id::new("s").unwrap(),
            padding: 0.4,
            reveal_at: 0.9,
            retract_until: 0.95,
            dust_effect: canopy_content::Id::new("d").unwrap(),
            dust_interval_ticks: 8,
        };
        assert_eq!(construction_pose(0.0, &style), (0.0, false));
        let (rise, shown) = construction_pose(0.45, &style);
        assert!((rise - 0.5).abs() < 1e-9 && !shown);
        assert_eq!(construction_pose(0.9, &style), (1.0, true));
        let (rise, shown) = construction_pose(0.925, &style);
        assert!((rise - 0.5).abs() < 1e-9 && shown);
        assert_eq!(construction_pose(1.0, &style), (0.0, true));
        assert_eq!(construction_pose(f64::NAN, &style), (0.0, false));
    }

    #[test]
    fn the_largest_fight_wins() {
        let mut points = vec![[5.0, 5.0], [100.0, 100.0], [104.0, 98.0], [110.0, 102.0]];
        assert_eq!(densest(&points), Some([314.0 / 3.0, 100.0]));
        points.clear();
        assert_eq!(densest(&points), None);
    }

    #[test]
    fn keys_order_by_part_then_id() {
        assert!(key(Part::Entity, 0, 9) < key(Part::Corpse, 0, 1));
        assert!(key(Part::Missile, 0, 5) < key(Part::Missile, 1, 5));
        assert_ne!(key(Part::Cargo, 0, 3), key(Part::Entity, 0, 3));
    }

    #[test]
    fn hidden_bits_read_by_index() {
        let h = HiddenStatics {
            revision: 1,
            bits: Arc::new(vec![0b100, 1]),
        };
        assert!(h.get(2) && h.get(64) && !h.get(0) && !h.get(500));
    }
}
