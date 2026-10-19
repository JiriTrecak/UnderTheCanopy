//! The render thread's side of effects: every cue of the two presentations around the
//! render time, evaluated at that exact time into GPU emitters, ribbons, lights and effect
//! models (old `EffectPlayer.update`, `EffectTrail`, `ImpactEffects`, `MeleeTrails`).
//!
//! Positions of following and travelling cues blend between the two snapshots; everything
//! else is a function of the time since the cue started, so the picture is the same at any
//! frame rate. Parts are grouped by geometry, blend and texture, so the effects cost one
//! draw per group in view.
//!
//! The damage effects (D46) come after every cue, so spells always have the budgets
//! first: [`DRAWN_FIRES`] fire points (every burning building's first before any second,
//! the nearest first) and the [`DRAWN_HIT_DUST`] dust bursts nearest the eye, within
//! [`DAMAGE_PARTS`] parts, and one flickering light for the worst fire near the
//! eye when a light is left.

use super::{
    BLADE_TICKS, CueView, EffectSnapshot, Fire, Flight, HIT_DUST_TICKS, HitDust, SPARK_TICKS,
    TrailPoint, seconds_ticks, tick_seconds,
};
use canopy_assets::{
    CookedCueMotion, CookedPulse, Easing, EffectBlend, EffectLayer, EffectTable, FlipbookPlay,
    Motion, Primitive,
};
use canopy_render::{
    actors::{ActorInstance, MAX_POSES, Pose},
    effects::{
        Blend, EffectDraw, EffectFrame, Emitter, Geometry, MAX_EFFECT_LIGHTS, MAX_EMITTER_PARTS,
        MAX_EMITTERS, MAX_PARTS, MAX_RIBBONS, RibbonQuad, flag, kind,
    },
    frame::PointLight,
};
use glam::{EulerRot, Mat4, Quat, Vec3};
use std::{collections::BTreeMap, f32::consts::TAU, sync::Arc};

/// Fire points drawn at once (the nearest to the eye), each a flame and a smoke emitter.
pub const DRAWN_FIRES: usize = 28;
pub const FLAME_PARTS: u32 = 26;
pub const SMOKE_PARTS: u32 = 8;
/// Hit dust bursts drawn at once (the nearest to the eye).
pub const DRAWN_HIT_DUST: usize = 64;
pub const HIT_DUST_PARTS: u32 = 14;
/// Parts the damage effects may use together, after every cue.
pub const DAMAGE_PARTS: usize = 2048;
/// A fire this bad (and worse) may light its surroundings.
pub const FIRE_LIGHT_SEVERITY: f32 = 0.6;
/// Wind that leans smoke (world x, z per second of rise), the same for every fire.
const SMOKE_WIND: [f32; 2] = [0.45, -0.5];
/// The repeat of the fires' clock, seconds (keeps `f32` precise; one cycle every hour).
const FIRE_CLOCK: f64 = 3600.0;

/// Spin, scale and opacity of a cue at `age` seconds (old `sampleCueMotion`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueMotion {
    pub rotation: f32,
    pub scale: f32,
    pub opacity: f32,
}

fn cycle(age: f32, period: f32, phase: f32) -> f32 {
    ((age / period + phase) % 1.0 + 1.0) % 1.0
}

fn bounce(t: f32) -> f32 {
    if t < 1.0 / 2.75 {
        7.5625 * t * t
    } else if t < 2.0 / 2.75 {
        let t = t - 1.5 / 2.75;
        7.5625 * t * t + 0.75
    } else if t < 2.5 / 2.75 {
        let t = t - 2.25 / 2.75;
        7.5625 * t * t + 0.9375
    } else {
        let t = t - 2.625 / 2.75;
        7.5625 * t * t + 0.984375
    }
}

fn pulse(p: Option<&CookedPulse>, age: f32) -> f32 {
    let Some(p) = p else { return 1.0 };
    let phase = cycle(age, p.period, p.phase);
    let triangle = 1.0 - (phase * 2.0 - 1.0).abs();
    let eased = match p.easing {
        Easing::Sine => (1.0 - (phase * TAU).cos()) / 2.0,
        Easing::Bounce => bounce(triangle),
        Easing::Smoothstep => triangle * triangle * (3.0 - 2.0 * triangle),
    };
    p.min + (p.max - p.min) * eased
}

/// Samples a layer's transform motion `age` seconds after its cue started.
pub fn cue_motion(motion: Option<&CookedCueMotion>, age: f32) -> CueMotion {
    CueMotion {
        rotation: motion
            .and_then(|m| m.rotation)
            .map_or(0.0, |(period, sign, phase)| {
                phase + cycle(age, period, 0.0) * TAU * sign
            }),
        scale: pulse(motion.and_then(|m| m.scale.as_ref()), age),
        opacity: pulse(motion.and_then(|m| m.opacity.as_ref()), age),
    }
}

/// The fade-in and fade-out of an anchored layer at normalised time `t` (old envelope).
pub fn envelope(t: f32, persistent: bool) -> f32 {
    if persistent {
        1.0
    } else {
        (t * 12.0 + 0.2).min(1.0) * (1.0 - t).max(0.0).powf(1.2)
    }
}

/// Where a cue is in its life at render tick `tick`, or `None` when it shows nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueTime {
    /// Normalised time over the layer's duration (0 for persistent layers).
    pub t: f32,
    /// Seconds since the cue started.
    pub age: f32,
    /// Seconds into the current period (looping effects restart).
    pub local: f32,
    pub persistent: bool,
}

/// The time of `cue` at `tick` (old player's visibility, loop and lifetime rules).
pub fn cue_time(table: &EffectTable, cue: &CueView, tick: f64) -> Option<CueTime> {
    let effect = table.effects.get(usize::from(cue.effect))?;
    let layer = effect.layers.get(usize::from(cue.layer))?;
    if tick < cue.start || cue.finite_end.is_some_and(|end| tick >= end) || cue.hidden {
        return None;
    }
    let period = seconds_ticks(effect.duration).max(1e-6);
    let elapsed = tick - cue.start;
    let local = if effect.looping {
        elapsed % period
    } else {
        elapsed
    };
    let persistent = layer.sustain || cue.status_lifetime;
    let lifetime = cue.duration + tail_ticks(layer);
    if !persistent && !effect.looping && cue.flight.is_none() && elapsed >= lifetime {
        return None;
    }
    let t = if persistent {
        0.0
    } else {
        (local / cue.duration.max(1e-6)).clamp(0.0, 1.0) as f32
    };
    Some(CueTime {
        t,
        age: tick_seconds(elapsed) as f32,
        local: tick_seconds(local) as f32,
        persistent,
    })
}

/// Ticks a layer keeps showing after its duration: its particles' lifetime, or rain's
/// late launches and splashes.
pub fn tail_ticks(layer: &EffectLayer) -> f64 {
    let emitter = layer
        .emitter
        .as_ref()
        .map_or(0.0, |e| seconds_ticks(e.lifetime));
    let rain = layer.rain.as_ref().map_or(0.0, |r| {
        seconds_ticks(r.launch_delay) + r.impact.as_ref().map_or(0.0, |i| seconds_ticks(i.duration))
    });
    emitter + rain
}

/// Visual-only stable randomness, the same as `noise` in effects.wgsl.
pub fn noise(seed: u32) -> f32 {
    let mut n = seed;
    n = (n ^ (n >> 16)).wrapping_mul(0x045d_9f3b);
    n = (n ^ (n >> 16)).wrapping_mul(0x045d_9f3b);
    n ^= n >> 16;
    (n >> 8) as f32 / 16_777_216.0
}

/// Where rain drop `i` of `count` lands relative to its root, and how many seconds late it
/// launches (`drop_of` in effects.wgsl).
pub fn rain_drop(seed: u32, i: u32, count: u32, spread: f32, delay: f32) -> (Vec3, f32) {
    let s = seed.wrapping_add(i.wrapping_mul(7919));
    let angle = (i as f32 + noise(s.wrapping_add(1)) * 0.6) * 2.399_963 + noise(seed) * TAU;
    let radius = ((i as f32 + noise(s.wrapping_add(2))) / count.max(1) as f32).sqrt() * spread;
    (
        Vec3::new(angle.cos() * radius, 0.02, angle.sin() * radius),
        noise(s.wrapping_add(3)) * delay,
    )
}

/// Keeps the `n` nearest of `(distance, index)` pairs, nearest first.
fn keep_nearest(pairs: &mut Vec<(f32, u32)>, n: usize) {
    if pairs.len() > n {
        pairs.select_nth_unstable_by(n, |a, b| a.0.total_cmp(&b.0));
        pairs.truncate(n);
    }
    pairs.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
}

/// Visual-only phase of a seed (old `cast · 0.7` radians), wrapped.
/// A beam layer as ribbon segments from the cue's origin to its destination: a zigzag
/// whose joints jitter sideways by up to the cue's spread (0.3 without one), tapering to
/// the ends and redrawn fifteen times a second, so arcs crackle.
fn beam(cue: &CueView, l: &EffectLayer, age: f32, opacity: f32, out: &mut EffectFrame) {
    let (a, b) = (cue.origin, cue.destination);
    let length = a.distance(b);
    if length < 1e-3 || opacity <= 0.0 {
        return;
    }
    let dir = (b - a) / length;
    let side = dir.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
    let up = side.cross(dir);
    let jitter = cue.spread.unwrap_or(0.3);
    let joints = ((length / 1.5).ceil() as u32).clamp(2, 12);
    let flicker = (age * 15.0).floor() as u32;
    let unit = |h: u32| f32::from((h & 0xffff) as u16) / 65535.0 * 2.0 - 1.0;
    let point = |i: u32| {
        let k = i as f32 / joints as f32;
        let p = a.lerp(b, k);
        if i == 0 || i == joints {
            return p;
        }
        let h = cue.seed.wrapping_mul(0x9e37_79b1)
            ^ flicker.wrapping_mul(0x85eb_ca6b)
            ^ i.wrapping_mul(0xc2b2_ae35);
        let h = (h ^ (h >> 15)).wrapping_mul(0x2c1b_3c6d);
        let taper = (4.0 * k * (1.0 - k)).sqrt();
        p + (side * unit(h) + up * unit(h >> 16)) * jitter * taper
    };
    let mut from = point(0);
    for i in 1..=joints {
        if out.ribbons.len() >= MAX_RIBBONS {
            return;
        }
        let to = point(i);
        let mut quad = RibbonQuad::segment(from.to_array(), 1.0, to.to_array(), 1.0, cue.size);
        quad.color = v4(Vec3::from_array(l.color), 0.0);
        quad.look = [opacity, 0.0, 0.0, 0.0];
        out.ribbons.push(quad);
        from = to;
    }
}

fn phase(seed: u32) -> f32 {
    (f64::from(seed) * 0.7).rem_euclid(std::f64::consts::TAU) as f32
}

fn v4(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

/// A model clip's pose at `seconds`, looping or clamped (old `EffectModel.sample`).
fn model_pose(clip: Option<(u16, f32)>, seconds: f32, speed: f32, looping: bool) -> (u8, Pose) {
    let Some((clip, duration)) = clip else {
        return (0, Pose::default());
    };
    let time = (seconds * speed).max(0.0);
    let time = if looping && duration > 0.0 {
        time % duration
    } else {
        time.min(duration)
    };
    (
        1,
        Pose {
            clip,
            time,
            weight: 1.0,
        },
    )
}

/// The effect textures (pack entries) and which layer samples which.
pub struct EffectEvaluator {
    table: Arc<EffectTable>,
    textures: Vec<String>,
    /// Texture index by effect, then layer.
    layer_textures: Vec<Vec<Option<u32>>>,
    buckets: BTreeMap<(Geometry, Blend, u32), Vec<(f32, u32)>>,
    /// Parts pushed this frame.
    pushed: usize,
    /// Flame parts: drawn after every bucket, so smoke never dims the fire under it.
    flames: Vec<u32>,
    /// Scratch: damage effects by distance to the eye.
    nearest: Vec<(f32, u32)>,
}

/// Blends a cue's positions between two snapshots.
fn blend_cue(a: Option<&CueView>, b: &CueView, t: f32) -> CueView {
    let Some(a) = a else { return b.clone() };
    let mut out = b.clone();
    out.root = a.root.lerp(b.root, t);
    out.rotation = a.rotation.slerp(b.rotation, t);
    out.origin = a.origin.lerp(b.origin, t);
    out.destination = a.destination.lerp(b.destination, t);
    if let (Some(fa), Some(fb)) = (a.flight, b.flight) {
        out.flight = Some(Flight {
            position: fa.position.lerp(fb.position, t),
            direction: fb.direction,
        });
    }
    out
}

impl EffectEvaluator {
    pub fn new(table: Arc<EffectTable>) -> Self {
        let mut textures: Vec<String> = Vec::new();
        let layer_textures = table
            .effects
            .iter()
            .map(|e| {
                e.layers
                    .iter()
                    .map(|l| {
                        l.texture.as_ref().map(|name| {
                            match textures.iter().position(|t| t == name) {
                                Some(i) => i as u32,
                                None => {
                                    textures.push(name.clone());
                                    (textures.len() - 1) as u32
                                }
                            }
                        })
                    })
                    .collect()
            })
            .collect();
        Self {
            table,
            textures,
            layer_textures,
            buckets: BTreeMap::new(),
            pushed: 0,
            flames: Vec::new(),
            nearest: Vec::new(),
        }
    }

    /// Texture entries in the order draws refer to them
    /// (`Renderer::load_effect_textures`).
    pub fn textures(&self) -> &[String] {
        &self.textures
    }

    pub fn table(&self) -> &Arc<EffectTable> {
        &self.table
    }

    /// Evaluates the cues of `b` (blended from `a` by `t`) at render tick `tick` into
    /// `out`, adding effect models to `models`. `eye` orders normally blended parts back
    /// to front.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        a: Option<&EffectSnapshot>,
        b: &EffectSnapshot,
        t: f32,
        tick: f64,
        eye: Option<Vec3>,
        out: &mut EffectFrame,
        models: &mut Vec<ActorInstance>,
    ) {
        let _span = tracing::trace_span!("effects_frame").entered();
        out.clear();
        self.pushed = 0;
        self.flames.clear();
        for bucket in self.buckets.values_mut() {
            bucket.clear();
        }
        let t = t.clamp(0.0, 1.0);
        let older = a.map_or(&[][..], |a| &a.cues[..]);
        let mut j = 0;
        let table = self.table.clone();
        for cue in &b.cues {
            while j < older.len() && older[j].id < cue.id {
                j += 1;
            }
            let previous = older.get(j).filter(|p| p.id == cue.id);
            let cue = blend_cue(previous, cue, t);
            self.cue(&table, &cue, tick, eye, out, models);
        }
        for spark in &b.sparks {
            let age = tick - spark.tick;
            if !(0.0..SPARK_TICKS).contains(&age) {
                continue;
            }
            let e = Emitter {
                position: v4(spark.position, 0.0),
                rotation: [0.0, 0.0, 0.0, 1.0],
                color: [1.0, 1.0, 1.0, 1.0],
                accent: [1.0, 1.0, 1.0, 1.0],
                shape: [(age / SPARK_TICKS) as f32, 0.0, 0.0, 1.0],
                motion: [1.0, 0.0, spark.angle, 0.0],
                ints: [kind::SPARK, 0, 8, 0],
                e4: [0.0, 0.0, 0.0, spark.power],
                ..Emitter::default()
            };
            self.push(out, e, 8, Geometry::Quad, Blend::Additive, None, eye);
        }
        for (_, points) in &b.blades {
            let visible: Vec<_> = points
                .iter()
                .filter(|p| tick >= p.tick && tick - p.tick < BLADE_TICKS)
                .collect();
            for pair in visible.windows(2) {
                let (p, q) = (pair[0], pair[1]);
                let fade = |age: f64| (1.0 - age / BLADE_TICKS).max(0.0) as f32;
                let (fa, fb) = (fade(tick - p.tick), fade(tick - q.tick));
                if out.ribbons.len() < MAX_RIBBONS {
                    out.ribbons.push(RibbonQuad {
                        p: [
                            v4(p.base, 0.035 * fa),
                            v4(p.tip, 0.24 * fa),
                            v4(q.tip, 0.24 * fb),
                            v4(q.base, 0.035 * fb),
                        ],
                        color: [0.95, 0.80, 0.54, 1.0],
                        look: [1.0, 0.0, 0.0, 0.0],
                    });
                }
            }
        }
        self.damage(b, tick, eye, out);
        // Buckets become contiguous draws.
        for (&(geometry, blend, texture), parts) in &mut self.buckets {
            if parts.is_empty() {
                continue;
            }
            if blend == Blend::Normal {
                // Farthest first.
                parts.sort_by(|x, y| y.0.total_cmp(&x.0));
            }
            let first = out.parts.len() as u32;
            let room = MAX_PARTS.saturating_sub(out.parts.len());
            out.parts
                .extend(parts.iter().take(room).map(|&(_, part)| part));
            let count = out.parts.len() as u32 - first;
            if count > 0 {
                out.draws.push(EffectDraw {
                    geometry,
                    blend,
                    texture: (texture != u32::MAX).then_some(texture),
                    first,
                    count,
                });
            }
        }
        let first = out.parts.len() as u32;
        let room = MAX_PARTS.saturating_sub(out.parts.len());
        out.parts.extend(self.flames.iter().take(room));
        let count = out.parts.len() as u32 - first;
        if count > 0 {
            out.draws.push(EffectDraw {
                geometry: Geometry::Quad,
                blend: Blend::Additive,
                texture: None,
                first,
                count,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        out: &mut EffectFrame,
        emitter: Emitter,
        count: u32,
        geometry: Geometry,
        blend: Blend,
        texture: Option<u32>,
        eye: Option<Vec3>,
    ) {
        if out.emitters.len() >= MAX_EMITTERS || count == 0 {
            return;
        }
        let index = out.emitters.len() as u32;
        let depth = eye.map_or(0.0, |eye| {
            eye.distance_squared(Vec3::new(
                emitter.position[0],
                emitter.position[1],
                emitter.position[2],
            ))
        });
        out.emitters.push(emitter);
        self.pushed += count.min(MAX_EMITTER_PARTS) as usize;
        let bucket = self
            .buckets
            .entry((geometry, blend, texture.unwrap_or(u32::MAX)))
            .or_default();
        for part in 0..count.min(MAX_EMITTER_PARTS) {
            bucket.push((depth, (index << 8) | part));
        }
    }

    /// Fires of damaged buildings and dust of melee hits (D46), nearest to the eye first,
    /// within what the cues left of the budgets.
    fn damage(&mut self, b: &EffectSnapshot, tick: f64, eye: Option<Vec3>, out: &mut EffectFrame) {
        let _span = tracing::trace_span!("damage_effects").entered();
        let mut room = DAMAGE_PARTS.min(MAX_PARTS.saturating_sub(self.pushed));
        let distance = |p: Vec3| eye.map_or(0.0, |eye| eye.distance_squared(p));
        // Dust first: a burst lasts half a second, fires stay.
        let dust: Vec<HitDust> = {
            let nearest = &mut self.nearest;
            nearest.clear();
            nearest.extend(b.hit_dust.iter().enumerate().filter_map(|(i, d)| {
                let age = tick - d.tick;
                (0.0..HIT_DUST_TICKS)
                    .contains(&age)
                    .then(|| (distance(d.position), i as u32))
            }));
            keep_nearest(nearest, DRAWN_HIT_DUST);
            nearest
                .iter()
                .map(|&(_, i)| b.hit_dust[i as usize])
                .collect()
        };
        for d in dust {
            if room < HIT_DUST_PARTS as usize {
                break;
            }
            let age = tick_seconds(tick - d.tick) as f32;
            let e = Emitter {
                position: v4(d.position, 0.0),
                rotation: [0.0, 0.0, 0.0, 1.0],
                color: [0.5, 0.4, 0.28, 0.8],
                accent: [0.0, 0.0, 0.0, d.power],
                shape: [age, tick_seconds(HIT_DUST_TICKS) as f32, d.lift, 0.0],
                ints: [kind::DUST, flag::NORMAL, HIT_DUST_PARTS, d.seed],
                e0: v4(d.direction, 0.0),
                ..Emitter::default()
            };
            self.push(
                out,
                e,
                HIT_DUST_PARTS,
                Geometry::Quad,
                Blend::Normal,
                None,
                eye,
            );
            room -= HIT_DUST_PARTS as usize;
        }
        // Every burning building's first flame before any building's second, the nearest
        // first within each rank: when more burn than are drawn, each still shows its damage.
        let fires: Vec<Fire> = {
            let nearest = &mut self.nearest;
            nearest.clear();
            nearest.extend(
                b.fires
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (distance(f.position), i as u32)),
            );
            let rank = |i: u32| b.fires[i as usize].rank;
            nearest.sort_unstable_by(|x, y| {
                rank(x.1)
                    .cmp(&rank(y.1))
                    .then(x.0.total_cmp(&y.0))
                    .then(x.1.cmp(&y.1))
            });
            nearest.truncate(DRAWN_FIRES);
            nearest.iter().map(|&(_, i)| b.fires[i as usize]).collect()
        };
        let time = (tick_seconds(tick) % FIRE_CLOCK) as f32;
        let mut light: Option<Fire> = None;
        for f in fires {
            if room < (FLAME_PARTS + SMOKE_PARTS) as usize {
                break;
            }
            let base = Emitter {
                position: v4(f.position, 0.0),
                rotation: [0.0, 0.0, 0.0, 1.0],
                ..Emitter::default()
            };
            if out.emitters.len() + 2 > MAX_EMITTERS {
                break;
            }
            let index = out.emitters.len() as u32;
            out.emitters.push(Emitter {
                color: [1.0, 1.0, 1.0, 0.75 + 0.2 * f.severity],
                accent: [0.0, 0.0, 0.0, f.size],
                shape: [time, f.severity, 0.0, 0.0],
                ints: [kind::FLAME, 0, FLAME_PARTS, f.seed],
                ..base
            });
            self.pushed += FLAME_PARTS as usize;
            self.flames
                .extend((0..FLAME_PARTS).map(|part| (index << 8) | part));
            // Light grey wisps at first, a little denser and darker when the building is
            // about to fall; never a cloud over it.
            let dark = 0.3 - 0.14 * f.severity;
            let thin = dark + 0.12;
            self.push(
                out,
                Emitter {
                    color: [dark, dark * 0.97, dark * 0.94, 0.22 + 0.2 * f.severity],
                    accent: [thin, thin, thin, f.size],
                    shape: [time, f.severity, SMOKE_WIND[0], SMOKE_WIND[1]],
                    ints: [kind::SMOKE, flag::NORMAL, SMOKE_PARTS, f.seed ^ 0x5bd1_e995],
                    ..base
                },
                SMOKE_PARTS,
                Geometry::Quad,
                Blend::Normal,
                None,
                eye,
            );
            room -= (FLAME_PARTS + SMOKE_PARTS) as usize;
            if f.severity >= FIRE_LIGHT_SEVERITY && light.is_none_or(|l| f.severity > l.severity) {
                light = Some(f);
            }
        }
        if let Some(f) = light
            && out.lights.len() < MAX_EFFECT_LIGHTS
        {
            // Two incommensurate waves and the seed: a flicker that never visibly repeats.
            let phase = (f.seed % 1000) as f32 * 0.01;
            let flicker = 0.78
                + 0.14 * (time * 11.0 + phase).sin()
                + 0.08 * (time * 23.7 + phase * 2.3).sin();
            let strength = 9.0 * f.severity * flicker;
            out.lights.push(PointLight {
                position: (f.position + Vec3::Y * (0.6 + f.size)).to_array(),
                range: 5.0 + f.size * 4.0,
                color: [strength, strength * 0.48, strength * 0.16],
                pad: 0.0,
            });
        }
    }

    fn cue(
        &mut self,
        table: &EffectTable,
        cue: &CueView,
        tick: f64,
        eye: Option<Vec3>,
        out: &mut EffectFrame,
        models: &mut Vec<ActorInstance>,
    ) {
        let Some(time) = cue_time(table, cue, tick) else {
            return;
        };
        let effect = &table.effects[usize::from(cue.effect)];
        let l = &effect.layers[usize::from(cue.layer)];
        let texture = self.layer_textures[usize::from(cue.effect)][usize::from(cue.layer)];
        let motion = cue_motion(l.cue_motion.as_ref(), time.age);
        let blend = match l.blend {
            EffectBlend::Additive => Blend::Additive,
            EffectBlend::Normal => Blend::Normal,
        };
        let mut flags = 0;
        if texture.is_some() {
            flags |= flag::TEXTURED;
        }
        if l.primitive == Primitive::Decal {
            flags |= flag::GROUND;
        }
        if l.cue_motion.as_ref().is_some_and(|m| m.rotation.is_some()) {
            flags |= flag::SPIN;
        }
        if l.disc {
            flags |= flag::DISC;
        }
        if blend == Blend::Normal {
            flags |= flag::NORMAL;
        }
        let flip = l.flipbook.as_ref();
        if flip.is_some_and(|f| f.random_start) {
            flags |= flag::RANDOM_START;
        }
        let base = Emitter {
            position: v4(cue.root, 0.0),
            rotation: cue.rotation.to_array(),
            color: v4(Vec3::from_array(l.color), 1.0),
            accent: v4(Vec3::from_array(l.accent), cue.size),
            shape: [
                time.t,
                l.height,
                cue.spread.unwrap_or(1.0),
                l.length.unwrap_or(1.0),
            ],
            motion: [motion.scale, motion.rotation, phase(cue.seed), time.local],
            ints: [0, flags, 1, cue.seed.wrapping_mul(7919)],
            e5: [l.core, 0.0, 0.0, 0.0],
            flip: flip.map_or([1, 1, 0, 1], |f| [f.columns, f.rows, f.first, f.frames]),
            times: [
                flip.map_or(0.0, |f| f.frame_time),
                flip.map_or(0.0, |f| match f.play {
                    FlipbookPlay::Once => 0.0,
                    FlipbookPlay::Loop => 1.0,
                    FlipbookPlay::PingPong => 2.0,
                }),
                time.local,
                0.0,
            ],
            ..Emitter::default()
        };
        match l.motion {
            Motion::Sound => {}
            Motion::Rain => self.rain(cue, l, time, base, blend, texture, eye, out, models),
            Motion::Beam => {
                let opacity = (envelope(time.t, time.persistent)
                    * motion.opacity
                    * l.intensity.unwrap_or(1.0))
                .clamp(0.0, 1.0);
                beam(cue, l, time.age, opacity, out);
            }
            Motion::Missile | Motion::Wavefront => {
                self.travel(cue, l, time, base, flags, blend, texture, eye, out, models)
            }
            Motion::Model => {
                let Some((model, clip)) = cue.model else {
                    return;
                };
                let opacity =
                    envelope(time.t, time.persistent) * motion.opacity * l.intensity.unwrap_or(1.0);
                let m = l.model.as_ref().expect("mesh layers have models");
                let turn = Quat::from_euler(
                    EulerRot::XYZ,
                    m.rotation[0],
                    m.rotation[1] + motion.rotation,
                    m.rotation[2],
                );
                let (speed, looping) = m.animation.as_ref().map_or((1.0, false), |a| (a.1, a.2));
                let (count, pose) = model_pose(clip, time.local, speed, looping);
                let mut poses = [Pose::default(); MAX_POSES];
                poses[0] = pose;
                models.push(ActorInstance {
                    model,
                    transform: Mat4::from_scale_rotation_translation(
                        Vec3::splat(cue.size * motion.scale),
                        cue.rotation * turn,
                        cue.root + cue.rotation * (Vec3::Y * l.height),
                    ),
                    owner_color: None,
                    opacity: 1.0,
                    poses,
                    pose_count: count,
                    tint: [
                        l.color[0],
                        l.color[1],
                        l.color[2],
                        opacity.clamp(0.0, 1.0) * 0.999,
                    ],
                    shadow_only: false,
                    activity: 0.0,
                    aim: None,
                });
            }
            Motion::Light => {
                let strength = envelope(time.t, time.persistent)
                    * motion.opacity
                    * l.intensity.unwrap_or(35.0);
                if strength > 0.0 && out.lights.len() < MAX_EFFECT_LIGHTS {
                    out.lights.push(PointLight {
                        position: (cue.root + cue.rotation * (Vec3::Y * l.height)).to_array(),
                        range: cue.size * motion.scale,
                        color: (Vec3::from_array(l.color) * strength).to_array(),
                        pad: 0.0,
                    });
                }
            }
            Motion::Emitter => {
                let Some(em) = &l.emitter else { return };
                let sustained = time.persistent;
                let duration = tick_seconds(cue.duration) as f32;
                let emitted_time = if sustained {
                    time.local
                } else {
                    time.local.min(duration)
                };
                let total = (emitted_time * em.rate).floor();
                let mut e = base;
                e.ints[0] = kind::EMITTER;
                e.ints[2] = l.count.min(MAX_EMITTER_PARTS);
                e.color[3] = l.intensity.unwrap_or(1.0);
                e.times[3] = total;
                e.e0 = [em.direction[0], em.direction[1], em.direction[2], em.cone];
                e.e1 = [em.speed, em.speed_variation, em.drag, em.gravity];
                e.e2 = [em.lifetime, em.rate, em.spawn_radius, duration];
                e.e3 = [em.start_size, em.end_size, em.spin, em.fade_in];
                e.e4 = [
                    em.fade_out,
                    if sustained { 1.0 } else { 0.0 },
                    if em.burst { 1.0 } else { 0.0 },
                    0.0,
                ];
                e.e5[1] = em.stretch;
                let count = e.ints[2];
                self.push(out, e, count, Geometry::Quad, blend, texture, eye);
            }
            shape => {
                let opacity = envelope(time.t, time.persistent)
                    * motion.opacity
                    * l.intensity
                        .unwrap_or(if shape == Motion::Pillar { 0.18 } else { 1.0 });
                if opacity <= 0.0 {
                    return;
                }
                let (code, size, count, geometry) = match shape {
                    Motion::Billboard => (kind::BILLBOARD, cue.size, 1, Geometry::Quad),
                    Motion::Burst => (kind::BURST, cue.size, l.count, Geometry::Quad),
                    Motion::Streaks => (kind::STREAKS, cue.size, l.count, Geometry::Quad),
                    Motion::Ring => {
                        let grow = if time.persistent || cue.duration_from {
                            1.0
                        } else {
                            0.55 + time.t * 0.6
                        };
                        (kind::RING, cue.size * grow, 1, Geometry::Quad)
                    }
                    Motion::Glow => {
                        let grow = if time.persistent {
                            1.0
                        } else {
                            0.85 + time.t * 0.2
                        };
                        (kind::GLOW, cue.size * grow, 1, Geometry::Quad)
                    }
                    Motion::Pillar => (
                        kind::PILLAR,
                        cue.size * (1.0 - time.t * 0.5),
                        1,
                        if texture.is_some() {
                            Geometry::Quad
                        } else {
                            Geometry::Pillar
                        },
                    ),
                    _ => return,
                };
                let mut e = base;
                e.ints[0] = code;
                e.ints[2] = count.min(MAX_EMITTER_PARTS);
                e.color[3] = opacity;
                e.accent[3] = size;
                let count = e.ints[2];
                self.push(out, e, count, geometry, blend, texture, eye);
            }
        }
    }

    /// A rain layer: shards (or models) falling onto a disc, a flash and splash fragments
    /// where each lands (old `updateRain`).
    #[allow(clippy::too_many_arguments)]
    fn rain(
        &mut self,
        cue: &CueView,
        l: &EffectLayer,
        time: CueTime,
        base: Emitter,
        blend: Blend,
        texture: Option<u32>,
        eye: Option<Vec3>,
        out: &mut EffectFrame,
        models: &mut Vec<ActorInstance>,
    ) {
        let Some(rain) = &l.rain else { return };
        let drops = l.count.min(MAX_EMITTER_PARTS);
        let fall = tick_seconds(cue.duration) as f32;
        let spread = cue.spread.unwrap_or(1.0);
        let impact = rain.impact.as_ref();
        let mut e = base;
        e.shape[2] = spread;
        e.shape[3] = l.length.unwrap_or(1.8);
        e.color[3] = l.intensity.unwrap_or(1.0);
        e.e0 = [rain.back[0], rain.back[1], rain.back[2], fall];
        e.e1 = [
            rain.launch_delay,
            impact.map_or(1.0, |i| i.duration),
            impact.map_or(0.0, |i| i.spread),
            impact.map_or(0.0, |i| i.height),
        ];
        e.e2 = [
            impact.map_or(0.0, |i| i.size),
            impact.map_or(1, |i| i.count) as f32,
            drops as f32,
            0.0,
        ];
        if let (Some((model, clip)), Some(m)) = (cue.model, &l.model) {
            // Model rain: each drop is the model, upright, turning slowly as it falls.
            let back = Vec3::from_array(rain.back);
            let (speed, looping) = m.animation.as_ref().map_or((1.0, false), |a| (a.1, a.2));
            for i in 0..drops {
                let (landing, delay) = rain_drop(e.ints[3], i, drops, spread, rain.launch_delay);
                let age = time.local - delay;
                let flight = age / fall.max(1e-4);
                if !(0.0..1.0).contains(&flight) {
                    continue;
                }
                let at = cue.root + cue.rotation * (landing + back * (l.height * (1.0 - flight)));
                let turn = Quat::from_euler(
                    EulerRot::XYZ,
                    m.rotation[0],
                    m.rotation[1] + age * 1.4 + noise(e.ints[3] ^ i) * TAU,
                    m.rotation[2] + 0.35 * (age * 2.3).sin(),
                );
                let (count, pose) = model_pose(clip, age, speed, looping);
                let mut poses = [Pose::default(); MAX_POSES];
                poses[0] = pose;
                models.push(ActorInstance {
                    model,
                    transform: Mat4::from_scale_rotation_translation(
                        Vec3::splat(cue.size),
                        Quat::from_rotation_arc(Vec3::Y, back) * turn,
                        at,
                    ),
                    owner_color: None,
                    opacity: 1.0,
                    poses,
                    pose_count: count,
                    tint: [l.color[0], l.color[1], l.color[2], 1.0],
                    shadow_only: false,
                    activity: 1.0,
                    aim: None,
                });
            }
        } else {
            let mut shards = e;
            shards.ints[0] = kind::RAIN;
            shards.ints[2] = drops;
            self.push(out, shards, drops, Geometry::Quad, blend, texture, eye);
        }
        let Some(i) = impact else { return };
        let mut flash = e;
        flash.ints[0] = kind::RAIN_FLASH;
        flash.ints[1] &= !(flag::TEXTURED | flag::GROUND | flag::SPIN);
        flash.ints[2] = drops;
        flash.color = v4(Vec3::from_array(i.color), e.color[3]);
        flash.flip = [1, 1, 0, 1];
        flash.times[0] = 0.0;
        self.push(
            out,
            flash,
            drops,
            Geometry::Quad,
            Blend::Additive,
            None,
            eye,
        );
        let fragments = (drops * i.count).min(MAX_EMITTER_PARTS);
        let mut splash = e;
        splash.ints[0] = kind::RAIN_SPLASH;
        splash.ints[1] &= !flag::GROUND;
        splash.ints[2] = fragments;
        splash.color = v4(Vec3::from_array(i.color), e.color[3]);
        splash.accent = v4(Vec3::from_array(i.accent), e.accent[3]);
        self.push(out, splash, fragments, Geometry::Quad, blend, texture, eye);
    }

    #[allow(clippy::too_many_arguments)]
    fn travel(
        &mut self,
        cue: &CueView,
        l: &EffectLayer,
        time: super::frame::CueTime,
        base: Emitter,
        flags: u32,
        blend: Blend,
        texture: Option<u32>,
        eye: Option<Vec3>,
        out: &mut EffectFrame,
        models: &mut Vec<ActorInstance>,
    ) {
        let t = time.t;
        let arc = if cue.flight.is_none() && l.motion == Motion::Missile {
            l.arc_height
        } else {
            0.0
        };
        let (position, direction) = match cue.flight {
            Some(f) => (f.position, f.direction),
            None => {
                let mut p = cue.origin.lerp(cue.destination, t.min(1.0));
                p.y += 4.0 * arc * t * (1.0 - t);
                let mut d = cue.destination - cue.origin;
                if cue.model.is_some() {
                    d.y += 4.0 * arc * (1.0 - 2.0 * t);
                }
                (p, d)
            }
        };
        let position = position + Vec3::from_array(l.offset);
        let direction = direction.try_normalize().unwrap_or(Vec3::Y);
        if let (Some((model, clip)), Some(m)) = (cue.model, &l.model) {
            let turn = Quat::from_euler(EulerRot::XYZ, m.rotation[0], m.rotation[1], m.rotation[2]);
            let (speed, looping) = m.animation.as_ref().map_or((1.0, false), |a| (a.1, a.2));
            let (count, pose) = model_pose(clip, time.local, speed, looping);
            let mut poses = [Pose::default(); MAX_POSES];
            poses[0] = pose;
            let opacity = l.intensity.unwrap_or(1.0);
            models.push(ActorInstance {
                model,
                transform: Mat4::from_scale_rotation_translation(
                    Vec3::splat(cue.size),
                    Quat::from_rotation_arc(Vec3::Y, direction) * turn,
                    position,
                ),
                owner_color: None,
                opacity: 1.0,
                poses,
                pose_count: count,
                tint: [
                    l.color[0],
                    l.color[1],
                    l.color[2],
                    if opacity >= 1.0 { 1.0 } else { opacity },
                ],
                shadow_only: false,
                activity: 0.0,
                aim: None,
            });
        } else {
            let mut e = base;
            e.position = v4(position, 0.0);
            e.rotation = v4(direction, 0.0);
            e.ints[0] = if l.motion == Motion::Missile {
                kind::MISSILE
            } else {
                kind::WAVEFRONT
            };
            if texture.is_none() && l.motion == Motion::Missile {
                e.ints[1] = flags | flag::FRESNEL;
            }
            e.color[3] = l.intensity.unwrap_or(1.0);
            self.push(out, e, 1, Geometry::Quad, blend, texture, eye);
        }
        let Some(trail) = &l.trail else { return };
        let now = cue.start + seconds_ticks(time.age);
        let trail_ticks = seconds_ticks(trail.duration);
        let mut points: Vec<TrailPoint> = match &cue.trail {
            Some(history) => history.iter().filter(|p| p.tick <= now).copied().collect(),
            None => {
                // Without a delivery the declared straight path is reconstructed.
                let effect = &self.table.effects[usize::from(cue.effect)];
                let period = seconds_ticks(effect.duration);
                let cycle_start = if effect.looping {
                    now - (now - cue.start) % period.max(1e-6)
                } else {
                    cue.start
                };
                let begin = cycle_start.max(now - trail_ticks);
                let n = trail.max_points.max(2);
                (0..n)
                    .map(|i| {
                        let at = begin + (now - begin) * f64::from(i) / f64::from(n - 1);
                        let k = ((at - cycle_start) / cue.duration.max(1e-6)).clamp(0.0, 1.0);
                        TrailPoint {
                            tick: at,
                            position: cue.origin.lerp(cue.destination, k as f32),
                        }
                    })
                    .collect()
            }
        };
        let last = TrailPoint {
            tick: now,
            position,
        };
        match points.last_mut() {
            Some(p) if p.tick == now => *p = last,
            _ => points.push(last),
        }
        points.retain(|p| p.tick >= now - trail_ticks);
        let max = trail.max_points.max(2) as usize;
        if points.len() > max {
            let source = points.clone();
            points = (0..max)
                .map(|i| source[(i * (source.len() - 1) + (max - 1) / 2) / (max - 1)])
                .collect();
        }
        for pair in points.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let distance = p.position.distance(q.position);
            if distance < 1e-5 || distance > trail.break_distance {
                continue;
            }
            let fade = |at: f64| (1.0 - (now - at) / trail_ticks.max(1e-6)).max(0.0) as f32;
            if out.ribbons.len() < MAX_RIBBONS {
                let mut quad = RibbonQuad::segment(
                    p.position.to_array(),
                    fade(p.tick),
                    q.position.to_array(),
                    fade(q.tick),
                    trail.width,
                );
                quad.color = v4(Vec3::from_array(trail.color), 0.0);
                quad.look = [trail.opacity, 0.0, 0.0, 0.0];
                out.ribbons.push(quad);
            }
        }
    }
}
