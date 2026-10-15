//! The render thread's side of effects: every cue of the two presentations around the
//! render time, evaluated at that exact time into GPU emitters, ribbons, lights and effect
//! models (old `EffectPlayer.update`, `EffectTrail`, `ImpactEffects`, `MeleeTrails`).
//!
//! Positions of following and travelling cues blend between the two snapshots; everything
//! else is a function of the time since the cue started, so the picture is the same at any
//! frame rate. Parts are grouped by geometry, blend and texture, so the effects cost one
//! draw per group in view.

use super::{
    BLADE_TICKS, CueView, EffectSnapshot, Flight, SPARK_TICKS, TrailPoint, seconds_ticks,
    tick_seconds,
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
    let lifetime = cue.duration
        + layer
            .emitter
            .as_ref()
            .map_or(0.0, |e| seconds_ticks(e.lifetime));
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

/// Visual-only phase of a seed (old `cast · 0.7` radians), wrapped.
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
        let bucket = self
            .buckets
            .entry((geometry, blend, texture.unwrap_or(u32::MAX)))
            .or_default();
        for part in 0..count.min(MAX_EMITTER_PARTS) {
            bucket.push((depth, (index << 8) | part));
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
        if l.primitive == Primitive::Decal && texture.is_some() {
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
            Motion::Sound | Motion::Unsupported => {}
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
