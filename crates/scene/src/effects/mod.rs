//! Spell and combat effects in the live scene (old `EffectPlayer`, `AbilityEffects`,
//! `ShellEffects`, `ImpactEffects`, `MeleeTrails` and the construction dust), and the
//! new damage effects (D46): dust on melee hits and fires on damaged buildings.
//!
//! The cooked effect table (`@effects`, converted from the authored effects by the
//! cooker) is bound to ability presentations as recipes: one per enabled layer of every
//! effect a presentation binds to an event. [`EffectDirector::update`] runs once per
//! simulation tick on the simulation thread, as part of the presenter: it plays the
//! view's new ability events, follows statuses, deliveries in flight and returning heroes,
//! lands shells, and keeps every live *cue* (one layer playing for one event) with where
//! it stands at that tick. The [`EffectSnapshot`] goes to the render thread inside the
//! presentation, where [`frame`] evaluates every cue at the exact render time — the old
//! player's per-shape formulas are functions of the time since the cue started, so
//! playback is the same at any frame rate and identical for the same tick.
//!
//! Timing follows the old player exactly: a cue starts `start` after its event; finite
//! bindings of sustained or looping layers end after one period (or the delivery's
//! duration); status bindings last while the status does, and finite layers bound to an
//! aura's status pulse every 200 ticks; `revivalStarted` cues last while the return is
//! pending; a `cancelled` event ends its cast's cues; non-persistent layers end after
//! their duration plus their particles' lifetime. Budgets: [`MAX_CUES`] live cues,
//! [`MAX_LIGHTS`] effect lights, [`MAX_MODELS`] effect models with
//! [`MAX_MODEL_TRIANGLES`] triangles (old `EFFECT_MODEL_LIMITS`).

pub mod frame;

#[cfg(test)]
mod tests;

use canopy_assets::{EffectTable, Motion, Pack, Primitive};
use canopy_content::{
    Content,
    abilities::Presentation,
    effects::{BindingAnchor, BindingLifetime, EffectEvent},
};
use canopy_sim::{
    abilities::{AbilityEvent, EventKind},
    entity::Milli,
    mission::TICKS_PER_SECOND,
    view::View,
};
use glam::{Quat, Vec3};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

/// Live cues at once; further events are not shown (old player: 64 per player).
pub const MAX_CUES: usize = 256;
/// Effect lights at once (old `EffectLights` pool of 4; the renderer's budget since
/// buildings light their work, D65).
pub const MAX_LIGHTS: usize = canopy_render::effects::MAX_EFFECT_LIGHTS;
/// Effect lights looping state effects of buildings and units may hold at once; the
/// rest stay free for spells and impacts.
pub const AMBIENT_LIGHTS: usize = MAX_LIGHTS - 3;
/// Effect model instances at once (old `EFFECT_MODEL_LIMITS.instances`).
pub const MAX_MODELS: usize = 16;
/// Triangles of every effect model at once (old `EFFECT_MODEL_LIMITS.triangles`).
pub const MAX_MODEL_TRIANGLES: u32 = 80_000;
/// Triangles of one effect model (old `perModelTriangles`).
pub const MAX_TRIANGLES_PER_MODEL: u32 = 10_000;
/// Finite layers bound to an aura's status repeat this often (old: 200 ticks).
pub const AURA_PULSE_TICKS: u32 = 200;
/// Hit sparks at once (old `ImpactEffects`: 64), each lasting 12 ticks.
pub const MAX_SPARKS: usize = 64;
pub const SPARK_TICKS: f64 = 12.0;
/// Blade ribbons at once (old `MeleeTrails`), points each, ticks a point lasts.
pub const MAX_BLADES: usize = 64;
pub const BLADE_POINTS: usize = 8;
pub const BLADE_TICKS: f64 = 3.0;
/// Hit dust bursts kept at once (the render thread draws the nearest), each lasting
/// [`HIT_DUST_TICKS`].
pub const MAX_HIT_DUST: usize = 256;
pub const HIT_DUST_TICKS: f64 = 22.0;
/// Fire points of damaged buildings kept at once (the render thread draws the nearest).
pub const MAX_FIRES: usize = 512;
/// Ticks of delivery positions kept for trails.
const TRAIL_HISTORY: usize = 64;
/// Ticks a delivery's projectile event is remembered after it was last seen.
const PROJECTILE_MEMORY_TICKS: u32 = 400;
/// Roots stand this far above their point (old `+ .035`).
pub const ROOT_LIFT: f32 = 0.035;

const TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;

/// The content event an ability event kind plays effects for.
pub fn effect_event(kind: EventKind) -> EffectEvent {
    use EffectEvent as E;
    use EventKind as K;
    match kind {
        K::Accepted => E::Accepted,
        K::Released => E::Released,
        K::Cancelled => E::Cancelled,
        K::Finished => E::Finished,
        K::WaveStarted => E::WaveStarted,
        K::Wave => E::Wave,
        K::Projectile => E::Projectile,
        K::Impact => E::Impact,
        K::Healed => E::Healed,
        K::Damaged => E::Damaged,
        K::Drained => E::Drained,
        K::ManaRestored => E::ManaRestored,
        K::StatusApplied => E::StatusApplied,
        K::Dispelled => E::Dispelled,
        K::Summoned => E::Summoned,
        K::Resurrected => E::Resurrected,
        K::RevivalStarted => E::RevivalStarted,
        K::Revived => E::Revived,
        K::RevivalCancelled => E::RevivalCancelled,
        K::SplitStarted => E::SplitStarted,
        K::SplitEnded => E::SplitEnded,
        K::Teleported => E::Teleported,
        K::VisionCreated => E::VisionCreated,
        K::Converted => E::Converted,
        K::Contained => E::Contained,
        K::ReleasedContained => E::ReleasedContained,
        K::Digested => E::Digested,
        K::Sacrificed => E::Sacrificed,
        K::Returned => E::Returned,
        K::Death => E::Death,
        K::Kill => E::Kill,
        K::WeaponRelease => E::WeaponRelease,
        K::Interval => E::Interval,
        K::CriticalStrike => E::CriticalStrike,
        K::Evaded => E::Evaded,
        K::Cleaved => E::Cleaved,
        K::WeaponEnhanced => E::WeaponEnhanced,
        K::EnhancedHit => E::EnhancedHit,
        K::WeaponOrdered => E::WeaponOrdered,
        K::WeaponOrderCancelled => E::WeaponOrderCancelled,
    }
}

/// Seconds of simulation ticks.
pub fn tick_seconds(ticks: f64) -> f64 {
    ticks * TICK_SECONDS
}

/// Simulation ticks of cooked seconds (authored in whole 25 ms ticks: rounded to a
/// thousandth of a tick against the `f32` storage).
pub fn seconds_ticks(seconds: f32) -> f64 {
    (f64::from(seconds) * f64::from(TICKS_PER_SECOND) * 1000.0).round() / 1000.0
}

/// One layer of an effect bound to an ability event.
#[derive(Debug, Clone, PartialEq)]
struct Recipe {
    effect: u16,
    layer: u16,
    event: EffectEvent,
    status: Option<String>,
    /// Anchored at the caster (after the layer's own `anchor: source`).
    caster: bool,
    status_lifetime: bool,
    duration_from: bool,
    size_from: bool,
    spread_from: bool,
}

/// What the scene tells effects about the world at a tick.
pub trait EffectScene {
    /// Where an entity stands now (its floor, flight and lookout lift), if shown.
    fn anchor(&self, entity: u32) -> Option<Vec3>;
    /// A model socket of an entity in its current pose.
    fn socket(&self, entity: u32, socket: &str) -> Option<(Vec3, Quat)>;
    /// The walking height under a point.
    fn floor(&self, position: Milli, surface: Option<&str>) -> f32;
    /// The pose of a weapon missile carrying cast `cast` (position, direction).
    fn missile_pose(&self, cast: u32) -> Option<(Vec3, Vec3)>;
}

/// A sample of a travelling layer's path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailPoint {
    pub tick: f64,
    pub position: Vec3,
}

/// A delivery's flight as last seen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub position: Vec3,
    pub direction: Vec3,
}

/// A cue as the render thread evaluates it: one layer playing for one event, standing
/// where it is at the snapshot's tick.
#[derive(Debug, Clone, PartialEq)]
pub struct CueView {
    pub id: u32,
    pub effect: u16,
    pub layer: u16,
    /// Seeds the cue's randomness (the cast, or a standalone play's seed).
    pub seed: u32,
    /// Simulation tick it starts on.
    pub start: f64,
    /// Ends here regardless of its layer (finite bindings of sustained or looping layers).
    pub finite_end: Option<f64>,
    /// Lasts as long as a status.
    pub status_lifetime: bool,
    /// Ticks of the layer's life (the delivery's duration for `durationFrom`).
    pub duration: f64,
    pub size: f32,
    pub spread: Option<f32>,
    pub duration_from: bool,
    /// The layer's root and its rotation (sockets).
    pub root: Vec3,
    pub rotation: Quat,
    /// A missing socket hides it.
    pub hidden: bool,
    /// Travelling layers: from and to (at the layer's height).
    pub origin: Vec3,
    pub destination: Vec3,
    /// Travelling layers riding a delivery or a weapon missile.
    pub flight: Option<Flight>,
    /// The flight so far, at the layer's height.
    pub trail: Option<Arc<[TrailPoint]>>,
    /// Mesh layers: the model's index in the live model catalog, and its clip with the
    /// clip's length.
    pub model: Option<(u32, Option<(u16, f32)>)>,
}

/// A hit spark burst (old `ImpactEffects`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spark {
    pub entity: u32,
    pub position: Vec3,
    pub tick: f64,
    pub angle: f32,
    pub power: f32,
}

/// A puff of dust where a melee swing landed, blown along the swing (new, D46).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitDust {
    /// Where the blow landed.
    pub position: Vec3,
    pub tick: f64,
    /// The swing's direction on the ground (unit).
    pub direction: Vec3,
    /// Height of `position` above the ground.
    pub lift: f32,
    /// Size of the swinging body (1 for regular infantry).
    pub power: f32,
    pub seed: u32,
}

/// One fire point on a damaged building (new, D46): flames and smoke rising from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fire {
    pub entity: u32,
    /// The base of the flames (on the roof or upper body).
    pub position: Vec3,
    /// Flame size in metres (0 shows nothing).
    pub size: f32,
    /// How badly the building burns, 0–1: denser, darker smoke.
    pub severity: f32,
    pub seed: u32,
}

/// One sample of a blade ribbon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BladeSample {
    pub tick: f64,
    pub base: Vec3,
    pub tip: Vec3,
}

/// Everything effects show of one tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectSnapshot {
    pub tick: u32,
    /// Sorted by id.
    pub cues: Vec<CueView>,
    pub sparks: Vec<Spark>,
    /// Blade ribbons by entity.
    pub blades: Vec<(u32, Arc<[BladeSample]>)>,
    pub hit_dust: Vec<HitDust>,
    /// Fire points of damaged buildings in sight.
    pub fires: Vec<Fire>,
}

#[derive(Debug, Clone)]
struct Cue {
    view: CueView,
    recipe: Option<u32>,
    ability: Option<u16>,
    cast: Option<u32>,
    /// The anchored entity.
    entity: Option<u32>,
    /// Status cues: holder and status name hash.
    holder: Option<(u32, u64)>,
    /// `revivalStarted` cues: the hero.
    hero: Option<u32>,
    /// The root at emission (cues that stop following keep it).
    base: Vec3,
    offset: Vec3,
    follow: bool,
    travels: bool,
    light: bool,
    triangles: u32,
}

/// A play outside ability events (shell impacts, construction dust, tools).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Play {
    pub effect: usize,
    /// Simulation tick it starts on.
    pub tick: f64,
    pub seed: u32,
    /// Where it lands (its root, before the layer offsets).
    pub at: Vec3,
    /// Where travelling layers start (`at` when `None`).
    pub from: Option<Vec3>,
    /// The radius of the play (shells pass theirs); like the old player's, standalone
    /// plays bind no `sizeFrom`, so it does not size them.
    pub radius: Option<f32>,
}

/// A status a unit has, as the viewer sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSighting {
    pub entity: u32,
    pub ability: u16,
    pub status: String,
    pub source: u32,
    pub started: u32,
    pub expires: u32,
    pub aura: bool,
    /// Where the unit stands.
    pub at: Vec3,
}

/// A delivery in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeliverySighting {
    pub cast: u32,
    pub ability: u16,
    pub position: Vec3,
    pub direction: Vec3,
}

/// A fallen hero returning through an ability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnSighting {
    pub hero: u32,
    pub ability: u16,
    pub started: u32,
    pub due: u32,
    pub at: Option<Vec3>,
}

/// A shell in flight or landed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellSighting {
    pub id: u32,
    pub launched: u32,
    pub impact: u32,
    /// The shooter's impact effect.
    pub effect: Option<usize>,
    pub at: Vec3,
    pub radius: f32,
}

/// What the viewer sees in one tick that effects follow.
#[derive(Debug, Clone, Default)]
pub struct Sightings<'a> {
    pub tick: u32,
    /// Recent ability events (new ones play once).
    pub events: &'a [AbilityEvent],
    pub statuses: Vec<StatusSighting>,
    pub deliveries: Vec<DeliverySighting>,
    pub returns: Vec<ReturnSighting>,
    pub shells: Vec<ShellSighting>,
}

/// An effect model as the live model catalog knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRef {
    /// Index in the live model catalog.
    pub index: u32,
    pub triangles: u32,
    /// Clips: name, index and length in seconds.
    pub clips: Vec<(String, u16, f32)>,
}

/// The effects bound to every ability, and the cues playing now.
pub struct EffectDirector {
    table: Arc<EffectTable>,
    recipes: Vec<Recipe>,
    /// Recipe ranges by ability index.
    by_ability: Vec<(u32, u32)>,
    /// Impact effect by shooter definition name.
    impacts: BTreeMap<String, usize>,
    cues: Vec<Cue>,
    next_id: u32,
    last_event: u32,
    last_tick: u32,
    /// Shells whose impact played, with their launch tick.
    landed: BTreeMap<u32, u32>,
    /// Casts whose projectile event played, with the tick last seen.
    projectiles: BTreeMap<u32, u32>,
    /// Delivery positions by cast, newest last.
    trails: BTreeMap<u32, VecDeque<TrailPoint>>,
    sparks: VecDeque<Spark>,
    blades: BTreeMap<u32, (u32, Vec<BladeSample>)>,
    hit_dust: VecDeque<HitDust>,
    fires: Vec<Fire>,
    /// Effect models by entry name, resolved when the director is made.
    models: BTreeMap<String, ModelRef>,
    /// Light cues of looping state effects ([`EffectDirector::play_ambient`]).
    ambient: BTreeSet<u32>,
}

/// FNV-1a: status names as keys without keeping strings.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn cells(m: Milli) -> (f32, f32) {
    (
        m.x as f32 / Milli::CELL as f32,
        m.y as f32 / Milli::CELL as f32,
    )
}

/// An event point in world space.
fn event_point(x: i32, y: i32, height: f64) -> Vec3 {
    let (x, z) = cells(Milli { x, y });
    Vec3::new(x, height as f32, z)
}

/// What starts a cue.
struct Trigger {
    ability: Option<u16>,
    event: EffectEvent,
    cast: Option<u32>,
    seed: u32,
    tick: f64,
    caster: Option<u32>,
    target: Option<u32>,
    origin: Vec3,
    point: Vec3,
    duration: Option<f64>,
    radius: Option<f32>,
    status: Option<String>,
}

impl EffectDirector {
    /// Binds the cooked `table` to every ability's presentation in `content`; `resolve`
    /// finds each effect model in the live model catalog (all of them load up front).
    pub fn new(
        content: &Content,
        table: Arc<EffectTable>,
        resolve: impl FnMut(&str) -> Option<ModelRef>,
    ) -> Self {
        let presentations: Vec<Option<&Presentation>> = content
            .abilities
            .abilities
            .iter()
            .map(|a| Some(&a.presentation))
            .collect();
        let mut impacts = BTreeMap::new();
        for d in &content.definitions {
            if let Some(effect) = content
                .look(d.id.as_str())
                .and_then(|a| a.impact_effect.as_ref())
                .and_then(|id| table.find(id.as_str()))
            {
                impacts.insert(d.id.to_string(), effect);
            }
        }
        Self::bind(table, &presentations, impacts, resolve)
    }

    /// Binds `table` to the presentations of abilities by index; `impacts` are the shell
    /// impact effects by shooter definition.
    pub fn bind(
        table: Arc<EffectTable>,
        presentations: &[Option<&Presentation>],
        impacts: BTreeMap<String, usize>,
        mut resolve: impl FnMut(&str) -> Option<ModelRef>,
    ) -> Self {
        let mut models = BTreeMap::new();
        for layer in table.effects.iter().flat_map(|e| &e.layers) {
            if let Some(m) = &layer.model
                && !models.contains_key(&m.model)
                && let Some(found) = resolve(&m.model)
            {
                models.insert(m.model.clone(), found);
            }
        }
        let mut recipes = Vec::new();
        let mut by_ability = Vec::new();
        for presentation in presentations {
            let first = recipes.len() as u32;
            if let Some(presentation) = presentation {
                for binding in &presentation.effects {
                    let Some(effect) = table.find(binding.effect.as_str()) else {
                        tracing::warn!(effect = %binding.effect.as_str(), "effect not cooked");
                        continue;
                    };
                    for (layer, l) in table.effects[effect].layers.iter().enumerate() {
                        if l.primitive == Primitive::Sound || l.motion == Motion::Unsupported {
                            continue;
                        }
                        recipes.push(Recipe {
                            effect: effect as u16,
                            layer: layer as u16,
                            event: binding.event,
                            status: binding.status_id.clone(),
                            caster: l.at_source || binding.anchor == BindingAnchor::Caster,
                            status_lifetime: binding.lifetime == BindingLifetime::Status,
                            duration_from: binding.duration_from.is_some(),
                            size_from: binding.size_from.is_some(),
                            spread_from: binding.spread_from.is_some(),
                        });
                    }
                }
            }
            by_ability.push((first, recipes.len() as u32));
        }
        Self {
            table,
            recipes,
            by_ability,
            impacts,
            cues: Vec::new(),
            next_id: 1,
            last_event: 0,
            last_tick: 0,
            landed: BTreeMap::new(),
            projectiles: BTreeMap::new(),
            trails: BTreeMap::new(),
            sparks: VecDeque::new(),
            blades: BTreeMap::new(),
            hit_dust: VecDeque::new(),
            fires: Vec::new(),
            models,
            ambient: BTreeSet::new(),
        }
    }

    /// Forgets every effect in play (the world jumped to another moment: a replay seek);
    /// the next tick starts afresh.
    pub fn reset(&mut self) {
        self.cues.clear();
        self.last_event = 0;
        self.last_tick = 0;
        self.landed.clear();
        self.projectiles.clear();
        self.trails.clear();
        self.sparks.clear();
        self.blades.clear();
        self.hit_dust.clear();
        self.fires.clear();
    }

    /// The cooked effect table from `pack`, or an empty one (warned) when the pack has none.
    pub fn table(pack: Option<&Pack>) -> Arc<EffectTable> {
        match pack.map(Pack::effects) {
            Some(Ok(table)) => Arc::new(table),
            Some(Err(error)) => {
                tracing::warn!("no effects shown: {error}");
                Arc::default()
            }
            None => Arc::default(),
        }
    }

    pub fn effect_table(&self) -> &Arc<EffectTable> {
        &self.table
    }

    /// Recipes bound to abilities.
    pub fn recipe_count(&self) -> usize {
        self.recipes.len()
    }

    pub fn cue_count(&self) -> usize {
        self.cues.len()
    }

    /// Ends every cue.
    pub fn clear(&mut self) {
        self.cues.clear();
        self.landed.clear();
        self.projectiles.clear();
        self.trails.clear();
        self.sparks.clear();
        self.blades.clear();
        self.hit_dust.clear();
        self.fires.clear();
        self.last_event = 0;
        self.last_tick = 0;
    }

    fn recipes_of(&self, ability: Option<u16>) -> std::ops::Range<usize> {
        ability
            .and_then(|a| self.by_ability.get(usize::from(a)))
            .map_or(0..0, |&(a, b)| a as usize..b as usize)
    }

    /// Plays every recipe bound to an event.
    fn trigger(&mut self, trigger: &Trigger) {
        if trigger.event == EffectEvent::Cancelled
            && let Some(cast) = trigger.cast
        {
            self.cues.retain(|c| c.cast != Some(cast));
        }
        for i in self.recipes_of(trigger.ability) {
            let r = &self.recipes[i];
            if r.event == trigger.event
                && r.status
                    .as_deref()
                    .is_none_or(|s| Some(s) == trigger.status.as_deref())
            {
                self.emit(trigger, Some(i), None);
            }
        }
    }

    /// Starts one cue of `recipe` (or of `layer` for standalone plays).
    fn emit(
        &mut self,
        trigger: &Trigger,
        recipe: Option<usize>,
        layer: Option<(usize, usize)>,
    ) -> Option<u32> {
        let (effect, layer_index, caster, status_lifetime, duration_from, size_from, spread_from) =
            match (recipe, layer) {
                (Some(i), _) => {
                    let r = &self.recipes[i];
                    (
                        usize::from(r.effect),
                        usize::from(r.layer),
                        r.caster,
                        r.status_lifetime,
                        r.duration_from,
                        r.size_from,
                        r.spread_from,
                    )
                }
                // Standalone plays bind no `sizeFrom`/`spreadFrom` (old `EffectPlayer.play`
                // ignored its radius, shell impacts included).
                (None, Some((e, l))) => {
                    let at_source = self.table.effects[e].layers[l].at_source;
                    (e, l, at_source, false, false, false, false)
                }
                (None, None) => return None,
            };
        let table = self.table.clone();
        let effect_def = &table.effects[effect];
        let l = &effect_def.layers[layer_index];
        let entity = if caster {
            trigger.caster
        } else {
            trigger.target
        };
        let holder = (trigger.event == EffectEvent::StatusApplied)
            .then(|| {
                trigger
                    .target
                    .map(|t| (t, hash(trigger.status.as_deref().unwrap_or(""))))
            })
            .flatten();
        if status_lifetime
            && recipe.is_some()
            && self.cues.iter().any(|c| {
                c.holder == holder && c.recipe == recipe.map(|r| r as u32) && c.cast == trigger.cast
            })
        {
            return None;
        }
        if self.cues.len() >= MAX_CUES {
            tracing::debug!("effect cue budget reached");
            return None;
        }
        let light = l.primitive == Primitive::Light;
        if light && self.cues.iter().filter(|c| c.light).count() >= MAX_LIGHTS {
            return None;
        }
        let mut model = None;
        let mut triangles = 0;
        if let Some(m) = &l.model {
            let found = self.models.get(&m.model)?;
            if found.triangles > MAX_TRIANGLES_PER_MODEL {
                tracing::warn!(model = %m.model, "effect model exceeds 10,000 triangles");
                return None;
            }
            let (count, used) = self
                .cues
                .iter()
                .filter(|c| c.triangles > 0)
                .fold((0, 0), |(n, t), c| (n + 1, t + c.triangles));
            if count >= MAX_MODELS || used + found.triangles > MAX_MODEL_TRIANGLES {
                return None;
            }
            triangles = found.triangles.max(1);
            let clip = m.animation.as_ref().and_then(|(name, _, _)| {
                found
                    .clips
                    .iter()
                    .find(|c| &c.0 == name)
                    .map(|c| (c.1, c.2))
            });
            model = Some((found.index, clip));
        }
        let mut duration = seconds_ticks(l.duration);
        if duration_from && let Some(d) = trigger.duration {
            duration = d;
        }
        let period = seconds_ticks(effect_def.duration);
        let start = trigger.tick + seconds_ticks(l.start);
        let finite_end =
            (recipe.is_some() && !status_lifetime && (l.sustain || effect_def.looping))
                .then_some(start + if duration_from { duration } else { period });
        let point = if caster {
            trigger.origin
        } else {
            trigger.point
        };
        let offset = Vec3::from_array(l.offset);
        let base = point + offset + Vec3::Y * ROOT_LIFT;
        let size = match (size_from, trigger.radius) {
            (true, Some(r)) => r,
            _ => l.size,
        };
        let spread = match (spread_from, trigger.radius) {
            (true, Some(r)) => Some(r),
            _ => l.spread,
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let lift = Vec3::Y * l.height;
        self.cues.push(Cue {
            view: CueView {
                id,
                effect: effect as u16,
                layer: layer_index as u16,
                seed: trigger.seed,
                start,
                finite_end,
                status_lifetime,
                duration,
                size,
                spread,
                duration_from,
                root: base,
                rotation: Quat::IDENTITY,
                hidden: false,
                origin: trigger.origin + lift,
                destination: trigger.point + lift,
                flight: None,
                trail: None,
                model,
            },
            recipe: recipe.map(|r| r as u32),
            ability: trigger.ability,
            cast: trigger.cast,
            entity,
            holder,
            hero: (trigger.event == EffectEvent::RevivalStarted)
                .then_some(trigger.target)
                .flatten(),
            base,
            offset,
            follow: l.follow,
            travels: l.motion.travels(),
            light,
            triangles,
        });
        Some(id)
    }

    /// Plays `play.effect` outside ability events; every enabled layer starts. Returns
    /// the cues started (for [`EffectDirector::stop`]).
    pub fn play(&mut self, play: Play) -> Vec<u32> {
        let mut started = Vec::new();
        let Some(effect) = self.table.effects.get(play.effect) else {
            return started;
        };
        let layers = effect.layers.len();
        let trigger = Trigger {
            ability: None,
            event: EffectEvent::Released,
            cast: None,
            seed: play.seed,
            tick: play.tick,
            caster: None,
            target: None,
            origin: play.from.unwrap_or(play.at),
            point: play.at,
            duration: None,
            radius: play.radius,
            status: None,
        };
        for layer in 0..layers {
            let l = &self.table.effects[play.effect].layers[layer];
            if l.primitive == Primitive::Sound || l.motion == Motion::Unsupported {
                continue;
            }
            started.extend(self.emit(&trigger, None, Some((play.effect, layer))));
        }
        started
    }

    /// Ends the given cues.
    pub fn stop(&mut self, cues: &[u32]) {
        self.cues.retain(|c| !cues.contains(&c.view.id));
    }

    /// The table index of an effect id.
    pub fn find(&self, id: &str) -> Option<usize> {
        self.table.find(id)
    }

    /// Plays a looping state effect of a building or unit (D65) like [`Self::play`], but
    /// its light layers take only the [`AMBIENT_LIGHTS`] share of the effect lights, so
    /// working machines never starve spell flashes and impacts of theirs.
    pub fn play_ambient(&mut self, play: Play) -> Vec<u32> {
        let mut started = Vec::new();
        let table = self.table.clone();
        let Some(effect) = table.effects.get(play.effect) else {
            return started;
        };
        let live: BTreeSet<u32> = self.cues.iter().map(|c| c.view.id).collect();
        self.ambient.retain(|id| live.contains(id));
        let trigger = Trigger {
            ability: None,
            event: EffectEvent::Released,
            cast: None,
            seed: play.seed,
            tick: play.tick,
            caster: None,
            target: None,
            origin: play.from.unwrap_or(play.at),
            point: play.at,
            duration: None,
            radius: play.radius,
            status: None,
        };
        for (layer, l) in effect.layers.iter().enumerate() {
            if l.primitive == Primitive::Sound || l.motion == Motion::Unsupported {
                continue;
            }
            let light = l.primitive == Primitive::Light;
            if light && self.ambient.len() >= AMBIENT_LIGHTS {
                continue;
            }
            if let Some(id) = self.emit(&trigger, None, Some((play.effect, layer))) {
                if light {
                    self.ambient.insert(id);
                }
                started.push(id);
            }
        }
        started
    }

    /// Moves cues to new roots (looping state effects following a socket of a moving
    /// model), keeping each layer's own offset.
    pub fn relocate(&mut self, moves: &BTreeMap<u32, Vec3>) {
        if moves.is_empty() {
            return;
        }
        for c in &mut self.cues {
            if let Some(&at) = moves.get(&c.view.id) {
                c.base = at + c.offset + Vec3::Y * ROOT_LIFT;
                c.view.root = c.base;
            }
        }
    }

    /// A hit on `entity` at `position` (old `ImpactEffects.hit`).
    pub fn spark(&mut self, spark: Spark) {
        if self.sparks.len() == MAX_SPARKS {
            self.sparks.pop_front();
        }
        self.sparks.push_back(Spark {
            power: spark.power.clamp(0.5, 2.0),
            ..spark
        });
    }

    /// Dust where a melee blow landed. The oldest burst gives way when the budget is full.
    pub fn hit_dust(&mut self, dust: HitDust) {
        if self.hit_dust.len() == MAX_HIT_DUST {
            self.hit_dust.pop_front();
        }
        self.hit_dust.push_back(HitDust {
            power: dust.power.clamp(0.6, 2.5),
            ..dust
        });
    }

    /// The fire points burning this tick (replacing the previous tick's), at most
    /// [`MAX_FIRES`].
    pub fn burn(&mut self, fires: impl IntoIterator<Item = Fire>) {
        self.fires.clear();
        self.fires
            .extend(fires.into_iter().filter(|f| f.size > 0.0).take(MAX_FIRES));
    }

    /// A blade sample of `entity` swinging strike `strike` (old `MeleeTrails.sample`).
    pub fn blade(&mut self, entity: u32, strike: u32, sample: BladeSample) {
        if !self.blades.contains_key(&entity) && self.blades.len() >= MAX_BLADES {
            return;
        }
        let trail = self.blades.entry(entity).or_insert((strike, Vec::new()));
        if trail.0 != strike {
            *trail = (strike, Vec::new());
        }
        if trail.1.last().is_some_and(|p| sample.tick <= p.tick) {
            return;
        }
        // Keep the wrist clear: only the outer two-thirds of the blade leaves a ribbon.
        trail.1.push(BladeSample {
            base: sample.base.lerp(sample.tip, 0.35),
            ..sample
        });
        if trail.1.len() > BLADE_POINTS {
            trail.1.remove(0);
        }
    }

    /// Follows the view of tick `view.tick`: plays new events, follows statuses,
    /// deliveries and returns, lands shells and moves every cue to where it stands now.
    pub fn update(
        &mut self,
        view: &View,
        scene: &dyn EffectScene,
        catalog: &canopy_sim::catalog::Catalog,
    ) -> EffectSnapshot {
        let _span = tracing::trace_span!("effects").entered();
        let tick = view.tick;
        let statuses = view
            .entities
            .iter()
            .filter(|e| !e.remembered && e.public.hp.is_none_or(|hp| hp > 0))
            .filter_map(|e| Some((e, e.magic.as_ref()?)))
            .flat_map(|(e, magic)| {
                let entity = e.id().0;
                let at = scene.anchor(entity).unwrap_or_else(|| {
                    let (x, z) = cells(e.public.position);
                    Vec3::new(
                        x,
                        scene.floor(e.public.position, e.public.surface.as_deref()),
                        z,
                    )
                });
                magic.statuses.iter().map(move |s| StatusSighting {
                    entity,
                    ability: s.ability.0,
                    status: s.status.clone(),
                    source: s.source.0,
                    started: s.started,
                    expires: s.expires,
                    aura: s.aura,
                    at,
                })
            })
            .collect();
        let deliveries = view
            .spells
            .deliveries
            .iter()
            .map(|d| {
                let (x, z) = cells(Milli {
                    x: d.position.0,
                    y: d.position.1,
                });
                DeliverySighting {
                    cast: d.cast,
                    ability: d.ability.0,
                    position: Vec3::new(x, d.position.2 as f32, z),
                    direction: Vec3::new(
                        d.direction.0 as f32,
                        d.direction.2 as f32,
                        d.direction.1 as f32,
                    ),
                }
            })
            .collect();
        let returns = view
            .spells
            .returns
            .iter()
            .map(|&(hero, ability, started, due)| ReturnSighting {
                hero: hero.0,
                ability: ability.0,
                started,
                due,
                at: view.entity(hero).map(|e| {
                    let (x, z) = cells(e.public.position);
                    Vec3::new(
                        x,
                        scene.floor(e.public.position, e.public.surface.as_deref()),
                        z,
                    )
                }),
            })
            .collect();
        let shells = view
            .battle
            .shells
            .iter()
            .map(|shell| {
                let (x, z) = cells(shell.target.position);
                ShellSighting {
                    id: shell.id,
                    launched: shell.launched,
                    impact: shell.impact,
                    effect: self
                        .impacts
                        .get(catalog.id(shell.definition).as_str())
                        .copied(),
                    at: Vec3::new(
                        x,
                        scene.floor(shell.target.position, shell.target.surface.as_deref()),
                        z,
                    ),
                    radius: shell.radius_milli as f32 / Milli::CELL as f32,
                }
            })
            .collect();
        self.step(
            &Sightings {
                tick,
                events: &view.spells.events,
                statuses,
                deliveries,
                returns,
                shells,
            },
            scene,
        )
    }

    /// One tick of what the viewer sees (see [`EffectDirector::update`]).
    pub fn step(&mut self, seen: &Sightings<'_>, scene: &dyn EffectScene) -> EffectSnapshot {
        let tick = seen.tick;
        if tick < self.last_tick {
            self.clear();
        }
        self.last_tick = tick;
        self.events(seen.events);
        self.statuses(tick, &seen.statuses);
        self.deliveries(tick, &seen.deliveries, scene);
        self.returns(tick, &seen.returns);
        self.shells(tick, &seen.shells);
        self.expire(f64::from(tick));
        self.place(scene);
        self.cues.sort_by_key(|c| c.view.id);
        let now = f64::from(tick);
        self.sparks
            .retain(|s| now >= s.tick && now - s.tick < SPARK_TICKS);
        self.blades.retain(|_, (_, points)| {
            points.retain(|p| now - p.tick < BLADE_TICKS);
            !points.is_empty()
        });
        self.hit_dust
            .retain(|d| now >= d.tick && now - d.tick < HIT_DUST_TICKS);
        EffectSnapshot {
            tick,
            cues: self.cues.iter().map(|c| c.view.clone()).collect(),
            sparks: self.sparks.iter().copied().collect(),
            blades: self
                .blades
                .iter()
                .map(|(&id, (_, points))| (id, Arc::from(points.as_slice())))
                .collect(),
            hit_dust: self.hit_dust.iter().copied().collect(),
            fires: self.fires.clone(),
        }
    }

    fn events(&mut self, events: &[AbilityEvent]) {
        if events
            .iter()
            .map(|e| e.id)
            .max()
            .is_some_and(|newest| newest < self.last_event)
        {
            self.last_event = 0;
        }
        let last = self.last_event;
        for event in events.iter().filter(|e| e.id > last) {
            self.last_event = self.last_event.max(event.id);
            let kind = effect_event(event.kind);
            if kind == EffectEvent::Projectile {
                // A delivery shows its projectile once per cast (D14).
                if self.projectiles.contains_key(&event.cast) {
                    continue;
                }
                self.projectiles.insert(event.cast, event.tick);
            }
            let trigger = Trigger {
                ability: Some(event.ability.0),
                event: kind,
                cast: Some(event.cast),
                seed: event.cast,
                tick: f64::from(event.tick),
                caster: Some(event.caster.0),
                target: event.target.map(|t| t.0),
                origin: event_point(event.origin.x, event.origin.y, event.origin.height),
                point: event_point(event.point.x, event.point.y, event.point.height),
                duration: event.duration_ticks.map(f64::from),
                radius: event.radius.map(|r| r as f32),
                status: event.status.clone(),
            };
            self.trigger(&trigger);
        }
    }

    fn statuses(&mut self, tick: u32, statuses: &[StatusSighting]) {
        let mut live: BTreeSet<(u32, u16, u64)> = BTreeSet::new();
        let mut existing: BTreeSet<(u32, u64, u32)> = self
            .cues
            .iter()
            .filter_map(|c| Some((c.holder?, c.recipe?)))
            .map(|((e, s), r)| (e, s, r))
            .collect();
        for s in statuses {
            if s.expires <= tick {
                continue;
            }
            let ability = Some(s.ability);
            let status = hash(&s.status);
            live.insert((s.entity, s.ability, status));
            for i in self.recipes_of(ability) {
                let r = &self.recipes[i];
                if r.event != EffectEvent::StatusApplied
                    || r.status.as_deref().is_some_and(|f| f != s.status)
                    || existing.contains(&(s.entity, status, i as u32))
                {
                    continue;
                }
                let start = if !s.aura {
                    s.started
                } else if r.status_lifetime {
                    0
                } else {
                    tick / AURA_PULSE_TICKS * AURA_PULSE_TICKS
                };
                let trigger = Trigger {
                    ability,
                    event: EffectEvent::StatusApplied,
                    cast: None,
                    seed: s.started ^ s.source.wrapping_mul(31),
                    tick: f64::from(start),
                    caster: Some(s.source),
                    target: Some(s.entity),
                    origin: s.at,
                    point: s.at,
                    duration: Some(f64::from(if s.aura {
                        AURA_PULSE_TICKS
                    } else {
                        s.expires.saturating_sub(s.started)
                    })),
                    radius: None,
                    status: Some(s.status.clone()),
                };
                if self.emit(&trigger, Some(i), None).is_some() {
                    existing.insert((s.entity, status, i as u32));
                }
            }
        }
        // Every cue of a status ends with it (finite aura pulses also end on their own).
        self.cues.retain(|c| match (c.holder, c.ability) {
            (Some((entity, status)), Some(ability)) => live.contains(&(entity, ability, status)),
            (Some(_), None) => false,
            _ => true,
        });
    }

    fn deliveries(&mut self, tick: u32, deliveries: &[DeliverySighting], scene: &dyn EffectScene) {
        let live: BTreeSet<u32> = deliveries.iter().map(|d| d.cast).collect();
        // Travelling layers of ability effects show only while their delivery (or the
        // weapon missile carrying their cast) flies.
        self.cues.retain(|c| {
            !c.travels
                || c.recipe.is_none()
                || c.cast
                    .is_some_and(|cast| live.contains(&cast) || scene.missile_pose(cast).is_some())
        });
        self.trails.retain(|cast, _| live.contains(cast));
        for d in deliveries {
            let trail = self.trails.entry(d.cast).or_default();
            if trail.back().is_none_or(|p| p.tick < f64::from(tick)) {
                trail.push_back(TrailPoint {
                    tick: f64::from(tick),
                    position: d.position,
                });
                if trail.len() > TRAIL_HISTORY {
                    trail.pop_front();
                }
            }
            let has_travel = self
                .cues
                .iter()
                .any(|c| c.cast == Some(d.cast) && c.travels);
            if !has_travel && !self.projectiles.contains_key(&d.cast) {
                self.projectiles.insert(d.cast, tick);
                let trigger = Trigger {
                    ability: Some(d.ability),
                    event: EffectEvent::Projectile,
                    cast: Some(d.cast),
                    seed: d.cast,
                    tick: f64::from(tick),
                    caster: None,
                    target: None,
                    origin: d.position,
                    point: d.position,
                    duration: None,
                    radius: None,
                    status: None,
                };
                self.trigger(&trigger);
            }
            if let Some(seen) = self.projectiles.get_mut(&d.cast) {
                *seen = tick;
            }
            let points = self.trails.get(&d.cast).cloned();
            let table = self.table.clone();
            for c in self
                .cues
                .iter_mut()
                .filter(|c| c.cast == Some(d.cast) && c.travels)
            {
                let height = table.effects[usize::from(c.view.effect)].layers
                    [usize::from(c.view.layer)]
                .height;
                c.view.flight = Some(Flight {
                    position: d.position + Vec3::Y * height,
                    direction: d.direction,
                });
                c.view.trail = points.as_ref().map(|p| {
                    p.iter()
                        .map(|t| TrailPoint {
                            tick: t.tick,
                            position: t.position + Vec3::Y * height,
                        })
                        .collect()
                });
            }
        }
        self.projectiles
            .retain(|_, seen| tick.saturating_sub(*seen) <= PROJECTILE_MEMORY_TICKS);
    }

    fn returns(&mut self, tick: u32, returns: &[ReturnSighting]) {
        let mut live: BTreeSet<(u32, u16)> = BTreeSet::new();
        for r in returns {
            if tick >= r.due {
                continue;
            }
            live.insert((r.hero, r.ability));
            let Some(at) = r.at else { continue };
            for i in self.recipes_of(Some(r.ability)) {
                if self.recipes[i].event != EffectEvent::RevivalStarted
                    || self
                        .cues
                        .iter()
                        .any(|c| c.hero == Some(r.hero) && c.recipe == Some(i as u32))
                {
                    continue;
                }
                let trigger = Trigger {
                    ability: Some(r.ability),
                    event: EffectEvent::RevivalStarted,
                    cast: None,
                    seed: r.started ^ r.hero,
                    tick: f64::from(r.started),
                    caster: Some(r.hero),
                    target: Some(r.hero),
                    origin: at,
                    point: at,
                    duration: Some(f64::from(r.due.saturating_sub(r.started))),
                    radius: None,
                    status: None,
                };
                self.emit(&trigger, Some(i), None);
            }
        }
        let recipes = &self.recipes;
        self.cues.retain(|c| {
            c.recipe
                .is_none_or(|r| recipes[r as usize].event != EffectEvent::RevivalStarted)
                || c.hero.zip(c.ability).is_some_and(|key| live.contains(&key))
        });
    }

    fn shells(&mut self, tick: u32, shells: &[ShellSighting]) {
        let live: BTreeSet<u32> = shells.iter().map(|s| s.id).collect();
        self.landed.retain(|id, _| live.contains(id));
        for shell in shells {
            if tick < shell.impact || self.landed.get(&shell.id) == Some(&shell.launched) {
                continue;
            }
            self.landed.insert(shell.id, shell.launched);
            let Some(effect) = shell.effect else { continue };
            self.play(Play {
                effect,
                tick: f64::from(shell.impact),
                seed: shell.id,
                at: shell.at,
                from: None,
                radius: Some(shell.radius),
            });
        }
    }

    /// Ends cues that are over by `tick` (their evaluation would show nothing).
    fn expire(&mut self, tick: f64) {
        let table = &self.table;
        self.cues.retain(|c| {
            let v = &c.view;
            if v.finite_end.is_some_and(|end| tick >= end) {
                return false;
            }
            let effect = &table.effects[usize::from(v.effect)];
            let l = &effect.layers[usize::from(v.layer)];
            let persistent = l.sustain || v.status_lifetime;
            if persistent || effect.looping || v.flight.is_some() {
                return true;
            }
            let lifetime = v.duration
                + l.emitter
                    .as_ref()
                    .map_or(0.0, |e| seconds_ticks(e.lifetime));
            tick - v.start < lifetime
        });
    }

    /// Moves following cues onto their anchors, sockets and missiles.
    fn place(&mut self, scene: &dyn EffectScene) {
        let table = self.table.clone();
        for c in &mut self.cues {
            let l = &table.effects[usize::from(c.view.effect)].layers[usize::from(c.view.layer)];
            if c.follow
                && let Some(anchor) = c.entity.and_then(|e| scene.anchor(e))
            {
                c.view.root = anchor + c.offset + Vec3::Y * ROOT_LIFT;
            }
            if let Some(a) = &l.attachment {
                match c.entity.and_then(|e| scene.socket(e, &a.socket)) {
                    Some((position, rotation)) => {
                        let rotation = if a.inherit_rotation {
                            rotation
                        } else {
                            Quat::IDENTITY
                        };
                        c.view.rotation = rotation;
                        c.view.root = position + rotation * c.offset;
                        c.view.hidden = false;
                    }
                    None => {
                        c.view.rotation = Quat::IDENTITY;
                        if !c.follow {
                            c.view.root = c.base;
                        }
                        c.view.hidden = a.hide;
                    }
                }
            }
            if c.travels {
                // Missiles home on their anchored unit.
                if l.motion == Motion::Missile
                    && let Some(anchor) = c.entity.and_then(|e| scene.anchor(e))
                {
                    c.view.destination = anchor + Vec3::Y * l.height;
                }
                // A weapon missile carrying the cast leads (old `deliveryPose`).
                if let Some(cast) = c.cast
                    && let Some((position, direction)) = scene.missile_pose(cast)
                {
                    c.view.flight = Some(Flight {
                        position: position + Vec3::Y * l.height,
                        direction,
                    });
                }
            }
        }
    }
}
