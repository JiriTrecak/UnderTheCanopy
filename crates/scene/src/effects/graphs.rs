//! Effect graph layers (D113) on the render thread. A graph layer's cue owns an instance:
//! a range of the GPU particle pool and of the part buffer, simulated in fixed steps up to
//! the cue's age every frame (the kernels themselves are `canopy_fx`'s, run by
//! `canopy_render::graph_fx`). Births are counted here from the layer's spawners, so the
//! same cue at the same time has the same particles. A cue that jumps back in time (a
//! replay seek, the editor's timeline) restarts its instance and catches up; one that
//! ends keeps stepping, without births, until its longest-lived particle is gone.

use canopy_assets::{
    BillboardMode, CookedEffect, CookedGraph, CookedSpawner, EVENT_VEC4S, EffectTable,
};
use canopy_render::graph_fx::{
    GraphDraw, GraphFrame, GraphLayerUniform, GraphMode, GraphStep, MAX_STEPS, PART_RECORDS,
    POOL_VEC4S,
};
use glam::{Mat4, Quat, Vec3};
use std::collections::BTreeMap;

/// Seconds of one simulation step.
pub const STEP: f64 = 1.0 / 60.0;
/// Steps one instance may take in one frame (a capture or a jump on a timeline catches up
/// at once); a longer gap skips ahead (births lost).
const MAX_CATCH_UP: u32 = 240;
/// Seconds an ended instance steps on at most.
const MAX_LINGER: f64 = 8.0;

/// First-fit ranges of a fixed span, merged as they are freed.
#[derive(Debug, Clone)]
struct Ranges {
    free: Vec<(u32, u32)>,
}

impl Ranges {
    fn new(len: u32) -> Self {
        Self {
            free: vec![(0, len)],
        }
    }

    fn take(&mut self, len: u32) -> Option<u32> {
        let i = self.free.iter().position(|&(_, l)| l >= len)?;
        let (start, l) = self.free[i];
        if l == len {
            self.free.remove(i);
        } else {
            self.free[i] = (start + len, l - len);
        }
        Some(start)
    }

    fn give(&mut self, start: u32, len: u32) {
        let i = self.free.partition_point(|&(s, _)| s < start);
        self.free.insert(i, (start, len));
        if i + 1 < self.free.len() && self.free[i].0 + self.free[i].1 == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == self.free[i].0 {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
        }
    }
}

#[derive(Debug, Clone)]
struct Instance {
    effect: u16,
    graph: u16,
    program: u32,
    pool: u32,
    /// Vec4s of its pool range: particle records, then event records.
    pool_len: u32,
    /// Its event records (absolute pool vec4s).
    events: u32,
    parts: u32,
    capacity: u32,
    /// Layer seconds simulated so far.
    time: f64,
    spawned: u32,
    seed: u32,
    emitter: Mat4,
    /// The cue's ends in the emitter's space, and its team colour.
    source: Vec3,
    target: Vec3,
    team: [f32; 3],
    /// Where the emitter has travelled to in its own space (a missile's head).
    carrier: Vec3,
    /// Shown this frame (its cue is live).
    seen: bool,
    /// Seconds an ended instance still steps.
    linger: f64,
    /// The instances of the layers born of its events (D113), stepped with it.
    children: Vec<Instance>,
}

impl Instance {
    fn each(&self, f: &mut impl FnMut(&Instance)) {
        f(self);
        for child in &self.children {
            child.each(f);
        }
    }

    fn each_mut(&mut self, f: &mut impl FnMut(&mut Instance)) {
        f(self);
        for child in &mut self.children {
            child.each_mut(f);
        }
    }

    /// Instances in the tree.
    fn size(&self) -> usize {
        1 + self.children.iter().map(Instance::size).sum::<usize>()
    }
}

/// Births of `graph` in the layer time `[t0, t1)`. `end` is when spawners that run
/// "while the effect plays" stop.
fn births(graph: &CookedGraph, t0: f64, t1: f64, end: f64) -> u32 {
    graph
        .spawners
        .iter()
        .map(|s| match *s {
            CookedSpawner::Burst { count, time } => {
                let time = f64::from(time);
                if (t0 <= time && time < t1) || (time < 0.0 && t0 == 0.0) {
                    count.max(0.0).round() as u32
                } else {
                    0
                }
            }
            CookedSpawner::Rate {
                rate,
                start,
                duration,
            } => {
                let (rate, start) = (f64::from(rate).max(0.0), f64::from(start));
                let stop = if duration > 0.0 {
                    start + f64::from(duration)
                } else {
                    end
                };
                let born = |t: f64| (rate * (t.min(stop) - start).max(0.0)).floor();
                (born(t1) - born(t0)).max(0.0) as u32
            }
            CookedSpawner::Curved {
                rate,
                start,
                duration,
                ref curve,
            } => {
                let (rate, start) = (f64::from(rate).max(0.0), f64::from(start));
                let stop = if duration > 0.0 {
                    start + f64::from(duration)
                } else {
                    end
                };
                let span = (stop - start).max(1e-6);
                // Births so far: the rate times the curve's area up to `t` (the curve
                // linear between samples), so any steps give the same particles.
                let born = |t: f64| {
                    let u = ((t - start) / span).clamp(0.0, 1.0);
                    (rate * span * area(curve, u)).floor()
                };
                (born(t1) - born(t0)).max(0.0) as u32
            }
        })
        .sum()
}

/// The area under evenly sampled `curve` (linear between samples) from 0 to `u` (0–1).
fn area(curve: &[f32], u: f64) -> f64 {
    let segments = (curve.len().max(2) - 1) as f64;
    let x = u * segments;
    let mut total = 0.0;
    for i in 0..curve.len().saturating_sub(1) {
        let (a, b) = (f64::from(curve[i]), f64::from(curve[i + 1]));
        let covered = (x - i as f64).clamp(0.0, 1.0);
        if covered <= 0.0 {
            break;
        }
        // The trapezoid from the segment's start to `covered` of the way along it.
        total += covered * (a + (a + (b - a) * covered)) * 0.5;
    }
    total / segments
}

/// A cue's two ends (its caster and its target, world space, as they stand now), its
/// owner's team colour (beams and links run between the ends), and where its emitter is
/// carried now (world space): a missile's head as it flies, `None` where it started.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueEnds {
    pub source: Vec3,
    pub target: Vec3,
    pub team: [f32; 3],
    pub carrier: Option<Vec3>,
}

impl Default for CueEnds {
    fn default() -> Self {
        Self {
            source: Vec3::ZERO,
            target: Vec3::ZERO,
            team: [1.0; 3],
            carrier: None,
        }
    }
}

/// What an event-born instance steps from: its parent instance's event records, the
/// channel it listens to, the slots per parent particle, and the parent's capacity, pool
/// range and record stride.
#[derive(Debug, Clone, Copy)]
struct From {
    events: u32,
    channel: u32,
    per_parent: u32,
    capacity: u32,
    pool: u32,
    stride: u32,
}

/// The effect graph instances of the scene's live cues.
#[derive(Debug, Clone)]
pub struct GraphRuntime {
    /// The program index of every (effect, graph), in the order of [`Self::programs`].
    program_of: BTreeMap<(u16, u16), u32>,
    instances: BTreeMap<u32, Instance>,
    pool: Ranges,
    parts: Ranges,
    /// The render tick of the previous frame (lingering instances step by the gap).
    last_tick: Option<f64>,
}

impl GraphRuntime {
    pub fn new(table: &EffectTable) -> Self {
        let mut program_of = BTreeMap::new();
        for (e, effect) in table.effects.iter().enumerate() {
            for g in 0..effect.graphs.len() {
                let next = program_of.len() as u32;
                program_of.insert((e as u16, g as u16), next);
            }
        }
        Self {
            program_of,
            instances: BTreeMap::new(),
            pool: Ranges::new(POOL_VEC4S),
            parts: Ranges::new(PART_RECORDS),
            last_tick: None,
        }
    }

    /// The programs of every layer of `effects` (indices into the table), to compile
    /// ahead of their first run (`Renderer::prewarm_effect_programs`).
    pub fn programs_of(&self, effects: &std::collections::BTreeSet<u16>) -> Vec<u32> {
        self.program_of
            .iter()
            .filter(|((e, _), _)| effects.contains(e))
            .map(|(_, &p)| p)
            .collect()
    }

    /// The compiled layers in program order (`Renderer::load_effect_programs`).
    pub fn programs<'t>(&self, table: &'t EffectTable) -> Vec<&'t str> {
        let mut out = vec![""; self.program_of.len()];
        for (&(e, g), &p) in &self.program_of {
            out[p as usize] = &table.effects[usize::from(e)].graphs[usize::from(g)].wgsl;
        }
        out
    }

    /// Starts a frame at render tick `tick` (simulation ticks).
    pub fn begin(&mut self) {
        for instance in self.instances.values_mut() {
            instance.seen = false;
        }
    }

    /// Steps the instance of graph cue `id` (graph `graph` of effect `effect`) to
    /// `age` seconds into its layer, with its emitter at `root` turned by `rotation`.
    #[allow(clippy::too_many_arguments)]
    pub fn cue(
        &mut self,
        table: &EffectTable,
        id: u32,
        effect: u16,
        graph: u16,
        seed: u32,
        age: f64,
        root: Vec3,
        rotation: Quat,
        ends: CueEnds,
        out: &mut GraphFrame,
    ) {
        let Some(&program) = self.program_of.get(&(effect, graph)) else {
            return;
        };
        let def: &CookedEffect = &table.effects[usize::from(effect)];
        if !self.instances.contains_key(&id) {
            let Some(instance) = self.create(def, effect, graph, program, seed, out) else {
                return;
            };
            self.instances.insert(id, instance);
        }
        let instance = self.instances.get_mut(&id).expect("just inserted");
        let mut linger = 0.0f64;
        instance.each(&mut |i| {
            linger = linger.max(f64::from(def.graphs[usize::from(i.graph)].max_life));
        });
        let emitter = Mat4::from_rotation_translation(rotation, root);
        let local = |p: Vec3| rotation.inverse() * (p - root);
        let (source, target) = (local(ends.source), local(ends.target));
        let carrier = ends.carrier.map_or(Vec3::ZERO, local);
        instance.each_mut(&mut |i| {
            i.seen = true;
            i.emitter = emitter;
            i.source = source;
            i.target = target;
            i.team = ends.team;
            i.carrier = carrier;
        });
        // Each generation of children may outlive the one before it; particles that live
        // on (beams' points end themselves when the effect stops) are not waited for.
        instance.linger = (linger * instance.size() as f64).min(MAX_LINGER) + STEP;
        if age + STEP * 0.5 < instance.time {
            // Back in time: start again and catch up.
            instance.each_mut(&mut |i| {
                out.clear_pool.push((i.pool, i.pool_len));
                let renderers = def.graphs[usize::from(i.graph)].renderers.len().max(1) as u32;
                out.clear_parts.push((i.parts, i.capacity * renderers));
                i.time = 0.0;
                i.spawned = 0;
            });
        }
        let end = if def.looping {
            f64::INFINITY
        } else {
            f64::from(def.duration)
        };
        Self::advance(instance, def, age, end, true, out);
    }

    /// A new instance of graph `graph` and, under it, of every graph born of its events;
    /// `None` (nothing taken) when the pool or the part buffer is full.
    fn create(
        &mut self,
        def: &CookedEffect,
        effect: u16,
        graph: u16,
        program: u32,
        seed: u32,
        out: &mut GraphFrame,
    ) -> Option<Instance> {
        let cooked = &def.graphs[usize::from(graph)];
        let renderers = cooked.renderers.len().max(1) as u32;
        let capacity = cooked.capacity.max(1);
        let records = capacity * cooked.stride;
        let pool_len = records + capacity * cooked.channels.len() as u32 * EVENT_VEC4S;
        let pool = self.pool.take(pool_len)?;
        let Some(parts) = self.parts.take(capacity * renderers) else {
            self.pool.give(pool, pool_len);
            return None;
        };
        out.clear_pool.push((pool, pool_len));
        out.clear_parts.push((parts, capacity * renderers));
        let mut instance = Instance {
            effect,
            graph,
            program,
            pool,
            pool_len,
            events: pool + records,
            parts,
            capacity,
            time: 0.0,
            spawned: 0,
            seed: seed ^ (u32::from(graph) + 1).wrapping_mul(0x9e37_79b9),
            emitter: Mat4::IDENTITY,
            source: Vec3::ZERO,
            target: Vec3::ZERO,
            team: [1.0; 3],
            carrier: Vec3::ZERO,
            seen: true,
            linger: 0.0,
            children: Vec::new(),
        };
        for (g, child) in def.graphs.iter().enumerate() {
            if child.parent.is_none_or(|p| p.graph != u32::from(graph)) {
                continue;
            }
            let g = g as u16;
            let Some(&child_program) = self.program_of.get(&(effect, g)) else {
                continue;
            };
            match self.create(def, effect, g, child_program, seed, out) {
                Some(child) => instance.children.push(child),
                None => {
                    self.free(def, instance);
                    return None;
                }
            }
        }
        Some(instance)
    }

    /// Gives an instance's ranges, and its children's, back.
    fn free(&mut self, def: &CookedEffect, instance: Instance) {
        let renderers = def.graphs[usize::from(instance.graph)]
            .renderers
            .len()
            .max(1) as u32;
        self.pool.give(instance.pool, instance.pool_len);
        self.parts
            .give(instance.parts, instance.capacity * renderers);
        for child in instance.children {
            self.free(def, child);
        }
    }

    fn advance(
        instance: &mut Instance,
        def: &CookedEffect,
        to: f64,
        end: f64,
        spawning: bool,
        out: &mut GraphFrame,
    ) {
        let behind = ((to - instance.time) / STEP).floor().max(0.0) as u32;
        if behind > MAX_CATCH_UP {
            instance.time = to - f64::from(MAX_CATCH_UP) * STEP;
        }
        let tree = instance.size();
        while instance.time + STEP <= to + 1e-9 && out.steps.len() + tree <= MAX_STEPS {
            let (t0, t1) = (instance.time, instance.time + STEP);
            Self::step(instance, def, t0, t1, end, spawning, None, out);
        }
    }

    /// One step of an instance, then of the instances born of its events (which read the
    /// event records this step wrote). `from` is a child's parent.
    #[allow(clippy::too_many_arguments)]
    fn step(
        instance: &mut Instance,
        def: &CookedEffect,
        t0: f64,
        t1: f64,
        end: f64,
        spawning: bool,
        from: Option<From>,
        out: &mut GraphFrame,
    ) {
        let cooked = &def.graphs[usize::from(instance.graph)];
        let renderers = cooked.renderers.len().max(1) as u32;
        let (count, events_in, channel, per_parent, parent_base, parent_stride) = match from {
            Some(p) => (
                p.capacity,
                p.events,
                p.channel,
                p.per_parent,
                p.pool,
                p.stride,
            ),
            None if spawning => (
                births(cooked, t0, t1, end).min(instance.capacity),
                0,
                0,
                0,
                0,
                0,
            ),
            None => (0, 0, 0, 0, 0, 0),
        };
        out.steps.push(GraphStep {
            program: instance.program,
            layer: GraphLayerUniform {
                effect_time: t1 as f32,
                dt: STEP as f32,
                seed: instance.seed,
                base: instance.pool,
                capacity: instance.capacity,
                stride: cooked.stride,
                spawn_first: instance.spawned,
                spawn_count: count,
                spawn_start: t0 as f32,
                spawn_spacing: if count > 0 && from.is_none() {
                    (STEP / f64::from(count)) as f32
                } else {
                    0.0
                },
                render_base: instance.parts * 4,
                renderers,
                events_out: instance.events,
                events_in,
                channel,
                per_parent,
                running: u32::from(spawning && t0 < end),
                parent_base,
                parent_stride,
                pad: 0,
                emitter: instance.emitter.to_cols_array_2d(),
                params: [[0.0; 4]; 2],
                source: instance.source.extend(1.0).to_array(),
                target: instance.target.extend(1.0).to_array(),
                team: [instance.team[0], instance.team[1], instance.team[2], 1.0],
                carrier: instance.carrier.extend(0.0).to_array(),
            },
        });
        if from.is_none() {
            instance.spawned = instance.spawned.wrapping_add(count);
        }
        instance.time = t1;
        for c in 0..instance.children.len() {
            let Some(parent) = def.graphs[usize::from(instance.children[c].graph)].parent else {
                continue;
            };
            let from = From {
                events: instance.events,
                channel: parent.channel,
                per_parent: parent.per_parent,
                capacity: instance.capacity,
                pool: instance.pool,
                stride: cooked.stride,
            };
            Self::step(
                &mut instance.children[c],
                def,
                t0,
                t1,
                end,
                spawning,
                Some(from),
                out,
            );
        }
    }

    /// Ends the frame at render tick `tick`: ended instances step on without births until
    /// their particles are gone, then free their ranges; every live instance's renderers
    /// are drawn. `texture` finds a renderer's texture among the effect textures.
    pub fn end(
        &mut self,
        table: &EffectTable,
        tick: f64,
        texture: impl Fn(u16, u16, usize) -> Option<u32>,
        out: &mut GraphFrame,
    ) {
        let gap = self
            .last_tick
            .map_or(0.0, |last| super::tick_seconds((tick - last).max(0.0)))
            .min(0.25);
        self.last_tick = Some(tick);
        let mut gone = Vec::new();
        for (&id, instance) in &mut self.instances {
            let def = &table.effects[usize::from(instance.effect)];
            if !instance.seen {
                instance.linger -= gap;
                if instance.linger <= 0.0 {
                    gone.push(id);
                    continue;
                }
                let to = instance.time + gap;
                Self::advance(instance, def, to, 0.0, false, out);
            }
            instance.each(&mut |i| {
                let cooked = &def.graphs[usize::from(i.graph)];
                let renderers = cooked.renderers.len().max(1) as u32;
                for (r, renderer) in cooked.renderers.iter().enumerate() {
                    out.draws.push(GraphDraw {
                        cue: id,
                        texture: renderer
                            .texture
                            .as_ref()
                            .and_then(|_| texture(i.effect, i.graph, r)),
                        mode: match renderer.mode {
                            BillboardMode::Camera => GraphMode::Camera,
                            BillboardMode::Axis | BillboardMode::Velocity => GraphMode::Axis,
                            BillboardMode::Spheroid => GraphMode::Spheroid,
                            BillboardMode::Plane => GraphMode::Plane,
                            BillboardMode::Ribbon => GraphMode::Ribbon,
                        },
                        additive: renderer.additive,
                        distort: renderer.distort,
                        soft: renderer.soft,
                        flipbook: renderer.flipbook,
                        render_base: i.parts * 4,
                        renderer: r as u32,
                        renderers,
                        particles: i.capacity,
                    });
                }
            });
        }
        for id in gone {
            let instance = self.instances.remove(&id).expect("listed");
            let def = &table.effects[usize::from(instance.effect)];
            self.free(def, instance);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(spawners: Vec<CookedSpawner>) -> CookedGraph {
        CookedGraph {
            id: "g".into(),
            start: 0.0,
            at_source: false,
            follow: true,
            offset: [0.0; 3],
            wgsl: String::new(),
            stride: 3,
            capacity: 64,
            spawners,
            renderers: Vec::new(),
            parameters: Vec::new(),
            max_life: 1.0,
            channels: Vec::new(),
            listens: None,
            parent: None,
        }
    }

    #[test]
    fn births_add_up_whatever_the_steps() {
        let g = graph(vec![
            CookedSpawner::Rate {
                rate: 20.0,
                start: 0.0,
                duration: 0.4,
            },
            CookedSpawner::Burst {
                count: 1.0,
                time: 0.0,
            },
        ]);
        let total: u32 = (0..120)
            .map(|i| births(&g, f64::from(i) * STEP, f64::from(i + 1) * STEP, 1.0))
            .sum();
        assert_eq!(total, 8 + 1);
        // Coarser steps, the same particles.
        let coarse: u32 = (0..20)
            .map(|i| births(&g, f64::from(i) * 0.1, f64::from(i + 1) * 0.1, 1.0))
            .sum();
        assert_eq!(coarse, total);
    }

    #[test]
    fn a_curved_stream_tapers_and_adds_up_whatever_the_steps() {
        // 40 a second over 1 s, falling linearly from the full rate to nothing: 20 in all,
        // 15 of them in the first half.
        let curve: [f32; 17] = std::array::from_fn(|i| 1.0 - i as f32 / 16.0);
        let g = graph(vec![CookedSpawner::Curved {
            rate: 40.0,
            start: 0.0,
            duration: 1.0,
            curve,
        }]);
        let count = |steps: u32, until: f64| -> u32 {
            let dt = until / f64::from(steps);
            (0..steps)
                .map(|i| births(&g, f64::from(i) * dt, f64::from(i + 1) * dt, 2.0))
                .sum()
        };
        assert_eq!(count(120, 2.0), 20);
        assert_eq!(count(7, 2.0), 20);
        assert_eq!(count(30, 0.5), 15);
    }

    #[test]
    fn ranges_merge_when_freed() {
        let mut r = Ranges::new(100);
        let a = r.take(30).unwrap();
        let b = r.take(30).unwrap();
        let c = r.take(30).unwrap();
        assert!(r.take(20).is_none());
        r.give(b, 30);
        r.give(a, 30);
        r.give(c, 30);
        assert_eq!(r.free, vec![(0, 100)]);
    }
}
