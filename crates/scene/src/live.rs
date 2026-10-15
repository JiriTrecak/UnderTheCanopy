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

use crate::{
    animation::{CastTimeline, Poses, TICK_SECONDS, UnitAnimation, UnitInputs},
    models::{ModelCatalog, ModelInfo},
};
use canopy_assets::{Matrix3x4, Pack};
use canopy_content::map::UtcMap;
use canopy_content::{
    Content,
    effects::VisualLayer,
    game::{ConstructionPresentation, Creation, Definition, Kind, WorkAnimation},
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
    corpses: BTreeMap<EntityId, Corpse>,
    origins: BTreeMap<(Part, u32), (u32, Vec3)>,
    projectiles: BTreeMap<String, Vec<Layer>>,
    tree_slots: BTreeMap<EntityId, usize>,
    hidden: HiddenStatics,
    felling: BTreeMap<EntityId, Sighting>,
    chunks: Option<Arc<[Arc<[Sighting]>]>>,
    previous: BTreeMap<u64, Vec3>,
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
        let mut presenter = Self {
            content,
            terrain: world.terrain().clone(),
            pack,
            catalog: ModelCatalog::default(),
            colors: setup.colors,
            appearance,
            construction,
            units: BTreeMap::new(),
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
                    .unwrap_or_else(|| catalog.get(e.definition).asset.to_string());
                if let Some(file) = content
                    .asset(&asset)
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
            self.asset_model(d.asset.as_str());
            if let Some(visuals) = &d.resource_visuals {
                for stage in &visuals.stages {
                    self.asset_model(stage.asset.as_str());
                }
                for variant in &visuals.variants {
                    self.asset_model(variant.as_str());
                }
            }
            if let Some(effect) = content
                .asset(d.asset.as_str())
                .and_then(|a| a.projectile.clone())
            {
                self.projectile_layers(effect.as_str());
            }
        }
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
        let mut asset = placed.unwrap_or_else(|| d.asset.to_string());
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

        for e in &view.entities {
            let id = e.id();
            if fallen.contains(&id) || held(id) {
                continue;
            }
            let d = catalog.get(e.definition());
            let (asset_id, appearance_scale) = self.look(e, d);
            let Some(asset) = content.asset(&asset_id) else {
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
            let info = self.catalog.get(model).clone();
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
                        let presentation = content
                            .abilities
                            .presentation(ability.presentation.as_str())?;
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
                        && let Some(item_asset) = content.asset(item.asset.as_str())
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
                        if rise > 0.0
                            && let Some(scaffold) = self.asset_model(style.scaffold.as_str())
                        {
                            let sinfo = self.catalog.get(scaffold).clone();
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
                    if !show_body {
                        continue;
                    }
                }
                _ => {}
            }
            actors.push(actor);
        }

        // Units that left the view: deaths in sight become corpses; the rest vanish.
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
                self.catalog.get(c.actor.model).clone()
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

        self.trees(catalog, view, &mut actors, &mut counts);
        self.flights(catalog, view, &by_id, &mut actors, &mut counts);

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
                .map(|(a, s)| (a.clone().unwrap_or_else(|| d.asset.to_string()), *s))
                .unwrap_or_else(|| (d.asset.to_string(), 1.0));
            let Some(file) = content
                .asset(&asset_id)
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
        struct Flight<'a> {
            part: Part,
            id: u32,
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
            let Some(asset) = content.asset(d.asset.as_str()) else {
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
