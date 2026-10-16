//! Effect sound layers: the `sound` primitive of ability effects, played on ability
//! events the way the old game's effect player did (`src/render/abilities/effectPlayer.ts`
//! and `src/render/audio/effectSound.ts` in the old repository).
//!
//! Content binds effects to ability events (`presentation.effects`). Every enabled
//! `sound` layer of a bound effect becomes a recipe; each event that matches a recipe
//! starts a *cue*. Cues are sampled every frame as a pure function of the game tick, so
//! pause, game speed, late starts and loops behave predictably:
//!
//! - A cue starts `startTick` after its event and, unless something else owns its end,
//!   lasts the layer's `durationTicks` (or the event's duration for `durationFrom:
//!   delivery`). One-shot samples never replay after they end; `sound.loop` loops the
//!   sample; a looping parent effect repeats one-shots every effect period.
//! - Finite bindings of sustained or looping layers end after one period (or the
//!   delivery's duration).
//! - Status bindings (`lifetime: status`) last while the status does; aura statuses
//!   pulse finite layers every 5 seconds. Cues bound to `revivalStarted` last while the
//!   hero's return is pending. A `cancelled` event ends the cast's cues.
//! - `follow` keeps the sound on its anchored unit; otherwise it stays where the event
//!   happened.
//! - Volume is the layer's volume × fade-in/out × distance attenuation; the pitch is the
//!   playback rate and scales with game speed. Nothing plays while the game is paused.

use crate::{
    SoundId, SoundLibrary,
    limits::{Category, Request},
    spatial::{At, Distances, Listener, hear},
};
use canopy_content::{
    Content,
    abilities::Presentation,
    effects::{
        BindingAnchor, BindingLifetime, EffectEvent, LayerAnchor, LayerSound, Shape, VisualEffect,
    },
    game::AUTHORED_TICK_MS,
};
use canopy_sim::mission::TICKS_PER_SECOND;
use std::collections::HashSet;

/// Seconds of a content duration in authored 25 ms ticks.
pub(crate) fn authored_seconds(ticks: u32) -> f64 {
    f64::from(ticks) * f64::from(AUTHORED_TICK_MS) / 1000.0
}

/// Simulation ticks of a content duration in authored ticks.
fn authored_ticks(ticks: u32) -> f64 {
    authored_seconds(ticks) * f64::from(TICKS_PER_SECOND)
}

/// Seconds per simulation tick.
pub(crate) const TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;

/// Finite layers bound to an aura's status repeat this often (old: 200 ticks).
const AURA_PULSE_SECONDS: f64 = 5.0;

/// Sound cues alive at once; further cues are not started.
const MAX_CUES: usize = 512;

/// Projectile casts are remembered this long after their delivery was last seen, so a
/// delivery that leaves and re-enters sight does not start its sound again.
const PROJECTILE_MEMORY_TICKS: u32 = 400;

/// One `sound` layer of an effect bound to an ability event.
#[derive(Debug, Clone)]
struct Recipe {
    event: EffectEvent,
    status: Option<String>,
    /// Caster or target, after the layer's own `anchor: source`.
    caster: bool,
    status_lifetime: bool,
    duration_from_delivery: bool,
    /// Simulation ticks.
    start: f64,
    duration: f64,
    period: f64,
    sustain: bool,
    effect_loop: bool,
    follow: bool,
    offset: (f32, f32),
    sound: SoundId,
    volume: f32,
    pitch: f64,
    looping: bool,
    fade_in: f64,
    fade_out: f64,
    distances: Distances,
}

impl Recipe {
    fn persistent(&self) -> bool {
        self.sustain || self.status_lifetime
    }
}

/// Something that happened that effects may be bound to: an ability event, or one the
/// view implies (a status present, a delivery in flight, a hero returning).
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger<'a> {
    /// Index of the ability in the content's ability library.
    pub ability: usize,
    pub event: EffectEvent,
    /// The cast it belongs to, if known (`cancelled` ends a cast's cues).
    pub cast: Option<u32>,
    /// Simulation tick it happened on.
    pub tick: u32,
    pub caster: Option<u32>,
    pub target: Option<u32>,
    /// Where it comes from and where it lands, in cells.
    pub origin: (f32, f32),
    pub point: (f32, f32),
    /// Simulation ticks (statuses, deliveries, returns).
    pub duration_ticks: Option<u32>,
    pub status: Option<&'a str>,
}

/// A status a unit has, as the view shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusSighting<'a> {
    pub entity: u32,
    pub ability: usize,
    pub status: &'a str,
    pub source: u32,
    pub started: u32,
    pub expires: u32,
    pub aura: bool,
    pub at: (f32, f32),
}

/// A delivery (missile, wave, charge) in flight.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliverySighting {
    pub cast: u32,
    pub ability: usize,
    pub at: (f32, f32),
}

/// A fallen hero returning through an ability.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnSighting {
    pub hero: u32,
    pub ability: usize,
    pub started: u32,
    pub due: u32,
    pub at: Option<(f32, f32)>,
}

#[derive(Debug, Clone)]
struct Cue {
    key: u64,
    recipe: u32,
    ability: u32,
    cast: Option<u32>,
    /// The anchored unit (for `follow`).
    entity: Option<u32>,
    /// Status cues: the unit holding the status and the status name's hash.
    holder: Option<(u32, u64)>,
    /// `revivalStarted` cues: the returning hero.
    hero: Option<u32>,
    /// Simulation ticks.
    start: f64,
    duration: f64,
    finite_end: Option<f64>,
    at: (f32, f32),
}

/// The sound cues of ability effects.
#[derive(Default)]
pub struct EffectSounds {
    recipes: Vec<Recipe>,
    /// Recipe ranges per ability index.
    by_ability: Vec<(u32, u32)>,
    cues: Vec<Cue>,
    next_key: u64,
    /// Positions of units that `follow` cues may track, sorted by entity.
    positions: Vec<(u32, (f32, f32))>,
    /// Casts whose projectile sounds started, with the tick they were last seen.
    projectiles: Vec<(u32, u32)>,
    pub(crate) last_event: u32,
    pub(crate) last_tick: u32,
    status_cues: HashSet<(u32, u64, u32)>,
    live: HashSet<(u32, u32, u64)>,
}

/// Keys of effect cues live in their own range, apart from handles.
const KEY_BASE: u64 = 1 << 48;

impl EffectSounds {
    pub fn new() -> Self {
        Self {
            next_key: KEY_BASE,
            ..Self::default()
        }
    }

    /// Compiles the sound layers of every ability's presentation in `content`. Sounds
    /// are found among the published outputs of the manifest; a layer whose sound is not
    /// in `library` is skipped with a warning (missing audio never stops the game).
    pub fn bind(&mut self, content: &Content, library: &SoundLibrary) {
        let presentations: Vec<Option<&Presentation>> = content
            .abilities
            .abilities
            .iter()
            .map(|a| Some(&a.presentation))
            .collect();
        self.compile(&presentations, &content.effects, |layer| {
            let found =
                library.find_published(&content.manifest, layer.asset.as_str(), layer.index);
            if found.is_none() {
                tracing::warn!(
                    "audio: effect sound {}/audio:{} is not in the pack",
                    layer.asset.as_str(),
                    layer.index
                );
            }
            found
        });
    }

    /// Compiles recipes from each ability's presentation (by ability index). `resolve`
    /// finds a layer's sound.
    pub fn compile(
        &mut self,
        presentations: &[Option<&Presentation>],
        effects: &[VisualEffect],
        resolve: impl Fn(&LayerSound) -> Option<SoundId>,
    ) {
        self.clear();
        self.recipes.clear();
        self.by_ability.clear();
        for presentation in presentations {
            let first = self.recipes.len() as u32;
            for binding in presentation.iter().flat_map(|p| &p.effects) {
                let Some(effect) = effects.iter().find(|e| e.id == binding.effect) else {
                    continue;
                };
                for layer in effect.layers.iter().filter(|l| l.enabled) {
                    let (Shape::Sound, Some(sound)) = (layer.shape, &layer.sound) else {
                        continue;
                    };
                    let Some(id) = resolve(sound) else {
                        continue;
                    };
                    let offset = layer
                        .offset
                        .map_or((0.0, 0.0), |o| (o.x as f32, o.z as f32));
                    self.recipes.push(Recipe {
                        event: binding.event,
                        status: binding.status_id.clone(),
                        caster: layer.anchor == LayerAnchor::Source
                            || binding.anchor == BindingAnchor::Caster,
                        status_lifetime: binding.lifetime == BindingLifetime::Status,
                        duration_from_delivery: binding.duration_from.is_some(),
                        start: authored_ticks(layer.start_tick),
                        duration: authored_ticks(layer.duration_ticks),
                        period: authored_ticks(effect.duration_ticks),
                        sustain: layer.sustain,
                        effect_loop: effect.looping,
                        follow: layer.follow.unwrap_or(false),
                        offset,
                        sound: id,
                        volume: sound.volume as f32,
                        pitch: sound.pitch.max(0.01),
                        looping: sound.looping,
                        fade_in: authored_ticks(sound.fade_in_ticks),
                        fade_out: authored_ticks(sound.fade_out_ticks),
                        distances: Distances::new(
                            sound.reference_distance as f32,
                            sound.max_distance as f32,
                        ),
                    });
                }
            }
            self.by_ability.push((first, self.recipes.len() as u32));
        }
    }

    /// Ends every cue (a new match, a loaded game).
    pub fn clear(&mut self) {
        self.cues.clear();
        self.projectiles.clear();
        self.positions.clear();
        self.last_event = 0;
        self.last_tick = 0;
    }

    /// Effect sound layers bound to any ability.
    pub fn recipe_count(&self) -> usize {
        self.recipes.len()
    }

    /// Cues alive now (playing, waiting to start or silent).
    pub fn cue_count(&self) -> usize {
        self.cues.len()
    }

    fn recipes_of(&self, ability: usize) -> std::ops::Range<usize> {
        self.by_ability
            .get(ability)
            .map_or(0..0, |&(a, b)| a as usize..b as usize)
    }

    /// Positions of units `follow` cues may track this frame, in cells.
    pub fn set_positions(&mut self, positions: impl IntoIterator<Item = (u32, (f32, f32))>) {
        self.positions.clear();
        self.positions.extend(positions);
        self.positions.sort_unstable_by_key(|p| p.0);
    }

    /// Plays the recipes bound to an event.
    pub fn trigger(&mut self, trigger: &Trigger) {
        if trigger.event == EffectEvent::Cancelled
            && let Some(cast) = trigger.cast
        {
            self.cues.retain(|c| c.cast != Some(cast));
        }
        if trigger.event == EffectEvent::Projectile
            && let Some(cast) = trigger.cast
        {
            self.remember_projectile(cast, trigger.tick);
        }
        for i in self.recipes_of(trigger.ability) {
            let r = &self.recipes[i];
            if r.event == trigger.event
                && r.status
                    .as_deref()
                    .is_none_or(|s| Some(s) == trigger.status)
            {
                self.emit(trigger, i);
            }
        }
    }

    fn emit(&mut self, trigger: &Trigger, recipe: usize) {
        let r = &self.recipes[recipe];
        let entity = if r.caster {
            trigger.caster
        } else {
            trigger.target
        };
        let holder = (trigger.event == EffectEvent::StatusApplied)
            .then(|| {
                trigger
                    .target
                    .map(|t| (t, hash(trigger.status.unwrap_or(""))))
            })
            .flatten();
        if r.status_lifetime
            && (holder.is_none()
                || self
                    .cues
                    .iter()
                    .any(|c| c.holder == holder && c.recipe == recipe as u32))
        {
            // A status cue needs its holder, and one per holder is enough.
            return;
        }
        if self.cues.len() >= MAX_CUES {
            tracing::debug!("audio: effect sound cue budget reached");
            return;
        }
        let duration = match (r.duration_from_delivery, trigger.duration_ticks) {
            (true, Some(ticks)) => f64::from(ticks),
            _ => r.duration,
        };
        let start = f64::from(trigger.tick) + r.start;
        let length = if r.duration_from_delivery {
            duration
        } else {
            r.period
        };
        let finite_end =
            (!r.status_lifetime && (r.sustain || r.effect_loop)).then_some(start + length);
        let base = if r.caster {
            trigger.origin
        } else {
            trigger.point
        };
        let at = (base.0 + r.offset.0, base.1 + r.offset.1);
        let key = self.next_key;
        self.next_key += 1;
        self.cues.push(Cue {
            key,
            recipe: recipe as u32,
            ability: trigger.ability as u32,
            cast: trigger.cast,
            entity,
            holder,
            hero: (trigger.event == EffectEvent::RevivalStarted)
                .then_some(trigger.target)
                .flatten(),
            start,
            duration,
            finite_end,
            at,
        });
    }

    fn remember_projectile(&mut self, cast: u32, tick: u32) {
        match self.projectiles.iter_mut().find(|p| p.0 == cast) {
            Some(p) => p.1 = tick,
            None => self.projectiles.push((cast, tick)),
        }
    }

    /// Follows the statuses units have now: starts cues of statuses that have none,
    /// repeats aura pulses, ends status cues whose status is gone.
    pub fn sync_statuses<'a>(
        &mut self,
        tick: u32,
        statuses: impl IntoIterator<Item = StatusSighting<'a>>,
    ) {
        let mut status_cues = std::mem::take(&mut self.status_cues);
        let mut live = std::mem::take(&mut self.live);
        status_cues.clear();
        live.clear();
        status_cues.extend(
            self.cues
                .iter()
                .filter_map(|c| c.holder.map(|(e, s)| (e, s, c.recipe))),
        );
        let pulse = AURA_PULSE_SECONDS * f64::from(TICKS_PER_SECOND);
        for s in statuses {
            let range = self.recipes_of(s.ability);
            if range.is_empty() || s.expires <= tick {
                continue;
            }
            let status = hash(s.status);
            live.insert((s.entity, s.ability as u32, status));
            for i in range {
                let r = &self.recipes[i];
                if r.event != EffectEvent::StatusApplied
                    || r.status.as_deref().is_some_and(|f| f != s.status)
                    || status_cues.contains(&(s.entity, status, i as u32))
                {
                    continue;
                }
                let start = if !s.aura {
                    s.started
                } else if r.status_lifetime {
                    0
                } else {
                    ((f64::from(tick) / pulse).floor() * pulse) as u32
                };
                let trigger = Trigger {
                    ability: s.ability,
                    event: EffectEvent::StatusApplied,
                    cast: None,
                    tick: start,
                    caster: Some(s.source),
                    target: Some(s.entity),
                    origin: s.at,
                    point: s.at,
                    duration_ticks: Some(if s.aura {
                        pulse as u32
                    } else {
                        s.expires.saturating_sub(s.started)
                    }),
                    status: Some(s.status),
                };
                self.emit(&trigger, i);
                status_cues.insert((s.entity, status, i as u32));
            }
        }
        self.cues.retain(|c| {
            c.holder
                .is_none_or(|(entity, status)| live.contains(&(entity, c.ability, status)))
        });
        self.status_cues = status_cues;
        self.live = live;
    }

    /// Starts the projectile sounds of deliveries first seen in flight (the event that
    /// launched them happened before the player could see it).
    pub fn sync_deliveries(
        &mut self,
        tick: u32,
        deliveries: impl IntoIterator<Item = DeliverySighting>,
    ) {
        for d in deliveries {
            let seen = self.projectiles.iter().any(|p| p.0 == d.cast);
            self.remember_projectile(d.cast, tick);
            if seen {
                continue;
            }
            self.trigger(&Trigger {
                ability: d.ability,
                event: EffectEvent::Projectile,
                cast: Some(d.cast),
                tick,
                caster: None,
                target: None,
                origin: d.at,
                point: d.at,
                duration_ticks: None,
                status: None,
            });
        }
        self.projectiles
            .retain(|p| tick.saturating_sub(p.1) <= PROJECTILE_MEMORY_TICKS);
    }

    /// Follows heroes returning through an ability: their `revivalStarted` cues last
    /// until the return completes or is cancelled.
    pub fn sync_returns(&mut self, tick: u32, returns: impl IntoIterator<Item = ReturnSighting>) {
        let mut live = std::mem::take(&mut self.live);
        live.clear();
        for r in returns {
            if tick >= r.due {
                continue;
            }
            live.insert((r.hero, r.ability as u32, 0));
            let Some(at) = r.at else {
                continue;
            };
            for i in self.recipes_of(r.ability) {
                if self.recipes[i].event != EffectEvent::RevivalStarted
                    || self
                        .cues
                        .iter()
                        .any(|c| c.hero == Some(r.hero) && c.recipe == i as u32)
                {
                    continue;
                }
                let trigger = Trigger {
                    ability: r.ability,
                    event: EffectEvent::RevivalStarted,
                    cast: None,
                    tick: r.started,
                    caster: Some(r.hero),
                    target: Some(r.hero),
                    origin: at,
                    point: at,
                    duration_ticks: Some(r.due.saturating_sub(r.started)),
                    status: None,
                };
                self.emit(&trigger, i);
            }
        }
        let recipes = &self.recipes;
        self.cues.retain(|c| {
            recipes[c.recipe as usize].event != EffectEvent::RevivalStarted
                || c.hero.is_some_and(|h| live.contains(&(h, c.ability, 0)))
        });
        self.live = live;
    }

    /// The voices the cues want at `tick` (fractional render tick); ends finished cues.
    pub(crate) fn requests(
        &mut self,
        tick: f64,
        speed: f64,
        listener: &Listener,
        library: &SoundLibrary,
        out: &mut Vec<Request>,
    ) {
        let recipes = &self.recipes;
        let positions = &self.positions;
        self.cues.retain_mut(|cue| {
            let r = &recipes[cue.recipe as usize];
            if cue.finite_end.is_some_and(|end| tick >= end) {
                return false;
            }
            if tick < cue.start {
                return true;
            }
            let persistent = r.persistent();
            if !persistent && !r.effect_loop && tick - cue.start >= cue.duration {
                return false;
            }
            if r.follow
                && let Some(entity) = cue.entity
                && let Ok(i) = positions.binary_search_by_key(&entity, |p| p.0)
            {
                let (x, y) = positions[i].1;
                cue.at = (x + r.offset.0, y + r.offset.1);
            }
            let local = if r.effect_loop && r.period > 0.0 {
                cue.start + (tick - cue.start) % r.period
            } else {
                tick
            };
            let age = if r.looping { tick } else { local } - cue.start;
            let end = cue
                .finite_end
                .map(|e| e - cue.start)
                .or((!persistent && !r.effect_loop).then_some(cue.duration));
            let remaining = end.map_or(f64::INFINITY, |e| e - age);
            if speed <= 0.0 || remaining <= 0.0 {
                return true;
            }
            let duration = library.clip(r.sound).duration;
            let offset = age * TICK_SECONDS * r.pitch;
            if !r.looping && offset >= duration {
                return true;
            }
            let Some(heard) = hear(listener, At::ground(cue.at.0, cue.at.1), r.distances) else {
                return true;
            };
            let fade_in = if r.fade_in > 0.0 {
                (age / r.fade_in).min(1.0)
            } else {
                1.0
            };
            let fade_out = if r.fade_out > 0.0 {
                (remaining / r.fade_out).min(1.0)
            } else {
                1.0
            };
            out.push(Request {
                key: cue.key,
                sound: r.sound,
                category: Category::Effect,
                offset: if r.looping {
                    offset.rem_euclid(duration.max(f64::EPSILON))
                } else {
                    offset
                },
                rate: r.pitch * speed,
                gain: r.volume * heard.gain * (fade_in * fade_out) as f32,
                pan: heard.pan,
                looping: r.looping,
                priority: 1.0,
            });
            true
        });
    }
}

/// FNV-1a: status names as keys without keeping strings.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests;
