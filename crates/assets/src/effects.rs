//! `@effects`: the published visual effects converted to runtime primitives (renderer
//! §4.8, D24 Q11). The cooker reads `content/effects/published.json` unchanged and writes
//! this table; the game never interprets authored layer shapes.
//!
//! Every authored layer becomes one of the runtime primitives — a particle emitter
//! (sprites or soft spheres evaluated from seed and age), a ground decal, a mesh, a light
//! or a sound, plus a ribbon when it leaves a trail — with a [`Motion`] saying how its
//! parts move over the layer's life (the old effect player's per-shape formulas). Times are seconds; colours linear RGB; angles
//! radians; textures and models are pack entry names.

use serde::{Deserialize, Serialize};

/// Entry name of the effect table.
pub const EFFECTS_ENTRY: &str = "@effects";

/// Every published effect, sorted by id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectTable {
    pub effects: Vec<CookedEffect>,
}

impl EffectTable {
    pub fn find(&self, id: &str) -> Option<usize> {
        self.effects
            .binary_search_by(|e| e.id.as_str().cmp(id))
            .ok()
    }
}

/// One effect: layers on a shared timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedEffect {
    pub id: String,
    /// Seconds of one period of the timeline.
    pub duration: f32,
    /// The timeline repeats every `duration`.
    pub looping: bool,
    /// Enabled layers in authored order (sound layers included: the audio crate plays
    /// them from content; the renderer skips them).
    pub layers: Vec<EffectLayer>,
    /// Enabled graph layers (D113), compiled.
    #[serde(default)]
    pub graphs: Vec<CookedGraph>,
}

/// A layer drawn by an effect graph (D113, docs/effect-graph.md): its compiled WGSL
/// kernels and what the runtime needs to run and draw them (`canopy_fx::compile`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedGraph {
    pub id: String,
    /// Seconds after the event.
    pub start: f32,
    /// Anchored at the source (caster) rather than the binding's anchor.
    pub at_source: bool,
    /// Particles move with their anchored unit.
    pub follow: bool,
    pub offset: [f32; 3],
    /// The `birth` and `update` kernels with their prelude.
    pub wgsl: String,
    /// Vec4s per particle record.
    pub stride: u32,
    /// Particles an instance holds at once.
    pub capacity: u32,
    pub spawners: Vec<CookedSpawner>,
    pub renderers: Vec<CookedRenderer>,
    /// Effect parameters in the order the layer uniform carries them.
    pub parameters: Vec<String>,
    /// Seconds the longest particle lives (an ended instance lingers this long).
    pub max_life: f32,
    /// The event channels it writes (its Emit event nodes), in channel order.
    pub channels: Vec<CookedChannel>,
    /// The layer and Emit event node it is born of, as authored.
    pub listens: Option<(String, String)>,
    /// Resolved by [`link_events`]: the graph it is born of, which of its channels, and
    /// the child slots each parent particle owns. Such a graph has no cue of its own: its
    /// parent's instance steps it.
    pub parent: Option<CookedParent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EventWhen {
    Death,
    Birth,
    Interval,
    Distance,
    Step,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedChannel {
    pub node: String,
    pub when: EventWhen,
    /// Seconds or metres between interval and distance events.
    pub interval: f32,
    pub max_count: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookedParent {
    pub graph: u32,
    pub channel: u32,
    pub per_parent: u32,
}

/// Vec4s of one event record in the particle pool.
pub const EVENT_VEC4S: u32 = canopy_fx::compile::EVENT_VEC4S;

/// The most particles an event-born layer instance may hold.
pub const MAX_CHILD_CAPACITY: u32 = 16384;
/// Speed assumed for distance events when sizing a child layer, metres a second.
const DISTANCE_SPEED: f32 = 25.0;

/// Resolves every event-born graph of one effect to its parent graph and channel, and
/// sizes it: each parent particle owns enough slots for the children of the events it
/// fires within one child lifetime. Returns a warning per graph that names no parent.
pub fn link_events(graphs: &mut [CookedGraph]) -> Vec<String> {
    let mut warnings = Vec::new();
    // Parents before children (chains of events): settle in passes.
    let mut done = vec![false; graphs.len()];
    for _ in 0..graphs.len() + 1 {
        for c in 0..graphs.len() {
            if done[c] {
                continue;
            }
            let Some((layer, event)) = graphs[c].listens.clone() else {
                done[c] = true;
                graphs[c].parent = None;
                continue;
            };
            let Some(p) = graphs
                .iter()
                .position(|g| g.id == layer)
                .filter(|&p| p != c)
            else {
                warnings.push(format!(
                    "{}: no layer `{layer}` to be born of",
                    graphs[c].id
                ));
                done[c] = true;
                continue;
            };
            if !done[p] {
                continue;
            }
            let Some(ch) = graphs[p].channels.iter().position(|ch| ch.node == event) else {
                warnings.push(format!(
                    "{}: layer `{layer}` has no Emit event node `{event}`",
                    graphs[c].id
                ));
                done[c] = true;
                continue;
            };
            let channel = &graphs[p].channels[ch];
            let life = graphs[c].max_life.max(0.01);
            let events = match channel.when {
                EventWhen::Death | EventWhen::Birth => 1.0,
                EventWhen::Interval => (life / channel.interval).ceil() + 1.0,
                EventWhen::Distance => (life * DISTANCE_SPEED / channel.interval).ceil() + 1.0,
                EventWhen::Step => (life * 60.0).ceil() + 1.0,
            };
            let per_event = channel.max_count.ceil().max(1.0);
            let parent_capacity = graphs[p].capacity.max(1);
            let per_parent = ((per_event * events).min(256.0) as u32)
                .clamp(1, (MAX_CHILD_CAPACITY / parent_capacity).max(1));
            graphs[c].capacity = parent_capacity * per_parent;
            graphs[c].parent = Some(CookedParent {
                graph: p as u32,
                channel: ch as u32,
                per_parent,
            });
            done[c] = true;
        }
    }
    warnings
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CookedSpawner {
    /// `count` particles `time` seconds into the layer.
    Burst { count: f32, time: f32 },
    /// `rate` a second from `start` for `duration` seconds (0: while the effect plays).
    Rate {
        rate: f32,
        start: f32,
        duration: f32,
    },
    /// As `Rate`, times a curve over the stream's span (even samples from its start to
    /// its end): a spray that tapers off. Last, so packs cooked before it still read.
    Curved {
        rate: f32,
        start: f32,
        duration: f32,
        curve: [f32; canopy_fx::compile::RATE_SAMPLES],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BillboardMode {
    Camera,
    Axis,
    Spheroid,
    Plane,
    Velocity,
    /// A strip through the particles in the order they were born.
    Ribbon,
}

/// A renderer of a graph layer; each particle's position, size, colour, axis, rotation
/// and frame come from the layer's part records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedRenderer {
    /// Pack entry of the texture (`None`: a soft white disc).
    pub texture: Option<String>,
    pub mode: BillboardMode,
    pub additive: bool,
    /// Bends the scene behind instead of drawing colour (heat haze).
    pub distort: bool,
    /// Soft-particle distance, metres.
    pub soft: f32,
    /// Flipbook columns and rows.
    pub flipbook: [u32; 2],
}

/// The runtime primitive a layer draws with.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Primitive {
    /// Camera-facing sprites or soft spheres (`count` of them).
    #[default]
    Particle,
    /// Quads lying on the ground plane at the layer's height.
    Decal,
    /// A model (`EffectLayer::model`).
    Mesh,
    Light,
    Sound,
    /// Particles simulated by an effect graph (`EffectLayer::graph`, D113).
    Graph,
}

/// How a layer's parts move: the old effect player's shapes.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Motion {
    /// One quad rising slightly over the layer's life.
    #[default]
    Billboard,
    /// Sparks flung out on a golden-angle spiral.
    Burst,
    /// A textured strip along the path of its emitter (`EffectLayer::ribbon`).
    Ribbon,
    /// Sparks rising in columns, fading in and out.
    Streaks,
    /// Stateless emitter particles (`EffectLayer::emitter`).
    Emitter,
    /// An expanding ring (0.83–1 of the radius, soft edges).
    Ring,
    /// A soft disc.
    Glow,
    /// An open cone-shaped column, fading at both ends.
    Pillar,
    /// Travels from the origin to the target (or rides a delivery).
    Missile,
    /// A travelling crest (rides a delivery).
    Wavefront,
    /// A model at the anchor.
    Model,
    Light,
    Sound,
    /// Shards (or models) falling from `height` onto points spread over a disc, each
    /// splashing on landing (`EffectLayer::rain`).
    Rain,
    /// A crackling line from the origin to the target: ribbon segments that jitter
    /// sideways by `spread` and redraw fifteen times a second.
    Beam,
    /// An effect graph's particles (`EffectLayer::graph`).
    Graph,
}

impl Motion {
    /// Layers that travel along a path rather than stand at an anchor.
    pub fn travels(self) -> bool {
        matches!(self, Self::Missile | Self::Wavefront)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectBlend {
    /// `src · α + dst`.
    #[default]
    Additive,
    /// `src · α + dst · (1 − α)`.
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlipbookPlay {
    Once,
    Loop,
    PingPong,
}

/// A texture atlas of `columns × rows` frames, row-major from the top left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedFlipbook {
    pub columns: u32,
    pub rows: u32,
    pub first: u32,
    pub frames: u32,
    /// Seconds per frame.
    pub frame_time: f32,
    pub play: FlipbookPlay,
    pub random_start: bool,
    /// The frames span each particle's life (`frame_time` unused).
    pub over_life: bool,
    /// Cross-fades each frame into the next.
    pub blend: bool,
}

/// A curve over a life (0–1): `(at, value)` keys at increasing times; the value holds
/// before the first and after the last key and blends linearly between.
pub type Curve = Vec<(f32, f32)>;
/// A colour curve over a life or a ramp: `(at, linear rgb)` keys.
pub type ColourCurve = Vec<(f32, [f32; 3])>;

/// Samples a curve at `t`.
pub fn sample_curve(keys: &[(f32, f32)], t: f32) -> f32 {
    sample(keys, t, |a, b, k| a + (b - a) * k).unwrap_or(1.0)
}

/// Samples a colour curve at `t`.
pub fn sample_colours(keys: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    sample(keys, t, |a: [f32; 3], b: [f32; 3], k| {
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k)
    })
    .unwrap_or([1.0; 3])
}

fn sample<T: Copy>(keys: &[(f32, T)], t: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = keys.first()?;
    if t <= first.0 {
        return Some(first.1);
    }
    for pair in keys.windows(2) {
        let ((a, va), (b, vb)) = (pair[0], pair[1]);
        if t <= b {
            return Some(lerp(va, vb, (t - a) / (b - a).max(1e-6)));
        }
    }
    keys.last().map(|k| k.1)
}

/// A textured strip along an emitter's path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedRibbon {
    /// Seconds a point stays.
    pub duration: f32,
    pub width: f32,
    /// Metres per texture repeat (0: the texture stretches over the whole strip).
    pub tile: f32,
    /// Texture repeats per second streaming backwards.
    pub scroll: f32,
    /// Over the strip's age (0 head, 1 tail).
    pub colour: Option<ColourCurve>,
    pub opacity: Option<Curve>,
    pub width_curve: Option<Curve>,
    pub additive: Option<Curve>,
}

/// A stateless emitter: each particle's position is a function of its index, the cue's
/// seed and its age (and, in world space, of where its emitter was when it was born).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CookedEmitter {
    /// Every particle is born at the start (otherwise `rate` per second, recycled).
    pub burst: bool,
    /// Seconds.
    pub lifetime: f32,
    /// Particles per second.
    pub rate: f32,
    pub speed: f32,
    pub speed_variation: f32,
    /// Unit direction.
    pub direction: [f32; 3],
    /// Half-angle of the cone, radians.
    pub cone: f32,
    /// Downward acceleration, metres per second squared (negative lifts).
    pub gravity: f32,
    pub drag: f32,
    pub spawn_radius: f32,
    pub start_size: f32,
    pub end_size: f32,
    /// Radians per second.
    pub spin: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    /// Seconds of motion each particle spans: a streak along its velocity (0: round).
    pub stretch: f32,
    /// Particles stay where they were born (a moving emitter leaves a trail).
    pub world: bool,
    /// Share of the emitter's velocity a world-space particle keeps.
    pub inherit: f32,
    /// Inner half-angle of the cone, radians.
    pub cone_inner: f32,
    /// Share of the spawn radius left empty at the centre.
    pub spawn_inner: f32,
    /// Over each particle's life.
    pub colour: Option<ColourCurve>,
    pub opacity: Option<Curve>,
    pub size: Option<Curve>,
    pub additive: Option<Curve>,
    pub erosion: Option<Curve>,
    pub erosion_softness: f32,
    pub heat: Option<Curve>,
    /// Strength (m), cell size (m), churn (1/s).
    pub turbulence: Option<[f32; 3]>,
    /// Friction on landing (None: falls through the ground).
    pub collide: Option<f32>,
    /// Radians a second around the emitter's upright axis.
    pub orbit: f32,
    /// Share of the distance to the axis closed over life (negative: pushed out).
    pub pull: f32,
}

/// Splashes where a rain drop lands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedImpact {
    /// Seconds.
    pub duration: f32,
    pub count: u32,
    pub size: f32,
    pub spread: f32,
    pub height: f32,
    pub color: [f32; 3],
    pub accent: [f32; 3],
}

/// How a rain layer's drops fall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedRain {
    /// Fall speed multiplier: the fall lasts the layer's duration divided by it.
    pub speed: f32,
    /// Most seconds a drop may launch late.
    pub launch_delay: f32,
    /// Unit vector from a drop's landing point back up its path.
    pub back: [f32; 3],
    pub impact: Option<CookedImpact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Easing {
    Sine,
    Smoothstep,
    Bounce,
}

/// Oscillates between `min` and `max` every `period` seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedPulse {
    pub period: f32,
    pub min: f32,
    pub max: f32,
    pub easing: Easing,
    /// Fraction of a period.
    pub phase: f32,
}

/// Spin, scale and opacity motion of a layer, sampled from the cue's age.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CookedCueMotion {
    /// Seconds per turn and direction (+1 counterclockwise, −1 clockwise), phase radians.
    pub rotation: Option<(f32, f32, f32)>,
    pub scale: Option<CookedPulse>,
    pub opacity: Option<CookedPulse>,
}

/// A ribbon following a travelling layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedTrail {
    /// Seconds a point stays.
    pub duration: f32,
    pub max_points: u32,
    pub width: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    /// Segments longer than this are breaks, not trail.
    pub break_distance: f32,
}

/// A model drawn by a mesh or missile layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedModel {
    /// Model entry name.
    pub model: String,
    /// Euler XYZ, radians.
    pub rotation: [f32; 3],
    /// Clip name, playback speed and whether it loops.
    pub animation: Option<(String, f32, bool)>,
}

/// Attaches an actor-local layer to a model socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookedAttachment {
    pub socket: String,
    /// Hide the layer when the socket is missing (otherwise stand at the anchor).
    pub hide: bool,
    pub inherit_rotation: bool,
}

/// Where and when a graph layer plays (an authored `GraphLayer` without its graph).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphPlacement {
    pub id: String,
    /// Seconds after the event.
    pub start: f32,
    pub at_source: bool,
    pub follow: bool,
    pub offset: [f32; 3],
}

/// A graph layer compiled for the runtime (the cooker and the editor's live previews
/// share it, so a preview is what the game plays). `texture` finds an asset's image
/// entry in the pack; the asset ids it could not find come back beside the layer.
pub fn cook_graph(
    placement: GraphPlacement,
    graph: &canopy_fx::Graph,
    duration: f32,
    texture: impl Fn(&str) -> Option<String>,
) -> Result<(CookedGraph, Vec<String>), Vec<canopy_fx::Problem>> {
    let program = canopy_fx::check(graph)?;
    Ok(cook_program(placement, &program, &[], duration, texture))
}

/// [`cook_graph`] of a checked graph, with its renderer inputs re-routed by `overrides`
/// (the node editor's previews).
pub fn cook_program(
    placement: GraphPlacement,
    program: &canopy_fx::Program,
    overrides: &[canopy_fx::compile::Override],
    duration: f32,
    texture: impl Fn(&str) -> Option<String>,
) -> (CookedGraph, Vec<String>) {
    use canopy_fx::compile::{self, Blend, Mode, Spawner};
    let compiled = compile::compile(program, overrides);
    let mut missing = Vec::new();
    let renderers = compiled
        .renderers
        .iter()
        .map(|r| {
            let entry = texture(&r.texture);
            if entry.is_none() {
                missing.push(r.texture.clone());
            }
            CookedRenderer {
                texture: entry,
                mode: match r.mode {
                    Mode::Camera => BillboardMode::Camera,
                    Mode::Axis => BillboardMode::Axis,
                    Mode::Spheroid => BillboardMode::Spheroid,
                    Mode::Plane => BillboardMode::Plane,
                    Mode::Velocity => BillboardMode::Velocity,
                    Mode::Ribbon => BillboardMode::Ribbon,
                },
                additive: r.blend == Blend::Additive,
                distort: r.blend == Blend::Distort,
                soft: r.soft,
                flipbook: r.flipbook,
            }
        })
        .collect();
    let cooked = CookedGraph {
        id: placement.id,
        start: placement.start,
        at_source: placement.at_source,
        follow: placement.follow,
        offset: placement.offset,
        capacity: compiled.capacity(duration),
        stride: compiled.stride,
        spawners: compiled
            .spawners
            .iter()
            .map(|s| match *s {
                Spawner::Burst { count, time } => CookedSpawner::Burst { count, time },
                Spawner::Rate {
                    rate,
                    start,
                    duration,
                    curve: None,
                } => CookedSpawner::Rate {
                    rate,
                    start,
                    duration,
                },
                Spawner::Rate {
                    rate,
                    start,
                    duration,
                    curve: Some(curve),
                } => CookedSpawner::Curved {
                    rate,
                    start,
                    duration,
                    curve,
                },
            })
            .collect(),
        renderers,
        parameters: compiled.parameters,
        max_life: compiled.max_life,
        channels: compiled
            .channels
            .iter()
            .map(|c| CookedChannel {
                node: c.node.clone(),
                when: match c.when {
                    compile::When::Death => EventWhen::Death,
                    compile::When::Birth => EventWhen::Birth,
                    compile::When::Interval => EventWhen::Interval,
                    compile::When::Distance => EventWhen::Distance,
                    compile::When::Step => EventWhen::Step,
                },
                interval: c.interval,
                max_count: c.max_count,
            })
            .collect(),
        listens: compiled
            .parent
            .as_ref()
            .map(|p| (p.layer.clone(), p.event.clone())),
        parent: None,
        wgsl: compiled.wgsl,
    };
    (cooked, missing)
}

impl EffectLayer {
    /// The layer that times and anchors graph `index` of an effect `duration` seconds
    /// long: cues treat it as any layer, and the scene hands it to the graph runtime.
    pub fn for_graph(index: u32, graph: &CookedGraph, duration: f32) -> Self {
        Self {
            id: graph.id.clone(),
            graph: Some(index),
            primitive: Primitive::Graph,
            motion: Motion::Graph,
            start: graph.start,
            duration,
            at_source: graph.at_source,
            follow: graph.follow,
            offset: graph.offset,
            ..Self::default()
        }
    }
}

/// One layer of an effect.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectLayer {
    pub id: String,
    /// Graph layers: the index of their compiled graph in [`CookedEffect::graphs`]; the
    /// layer itself only times and anchors it.
    #[serde(default)]
    pub graph: Option<u32>,
    pub primitive: Primitive,
    pub motion: Motion,
    /// Seconds after the event.
    pub start: f32,
    /// Seconds of the layer's life (one period for looping effects).
    pub duration: f32,
    /// Holds full strength instead of fading over `duration`.
    pub sustain: bool,
    /// Anchored at the source (caster) rather than the binding's anchor.
    pub at_source: bool,
    /// Follows its anchored unit.
    pub follow: bool,
    pub offset: [f32; 3],
    /// Linear RGB of the first and last part (particles fade between them over life).
    pub color: [f32; 3],
    pub accent: [f32; 3],
    pub count: u32,
    pub size: f32,
    pub height: f32,
    /// Opacity (or light intensity) multiplier; `None` uses the shape's default.
    pub intensity: Option<f32>,
    /// How much the bright centre of each sprite burns white (0–1).
    pub core: f32,
    pub spread: Option<f32>,
    pub length: Option<f32>,
    pub arc_height: f32,
    pub blend: EffectBlend,
    /// Lies on the ground instead of facing the camera.
    pub ground: bool,
    /// Burst parts spread over a disc instead of from a point.
    pub disc: bool,
    /// Texture entry name; untextured particles are soft spheres.
    pub texture: Option<String>,
    pub flipbook: Option<CookedFlipbook>,
    pub model: Option<CookedModel>,
    pub attachment: Option<CookedAttachment>,
    pub emitter: Option<CookedEmitter>,
    pub cue_motion: Option<CookedCueMotion>,
    pub trail: Option<CookedTrail>,
    pub rain: Option<CookedRain>,
    /// Index (among the effect's layers) of the travelling layer whose path this layer's
    /// emitter rides.
    pub rides: Option<u16>,
    pub ribbon: Option<CookedRibbon>,
    /// Colours the texture's brightness.
    pub ramp: Option<ColourCurve>,
    /// Opacity and scale over the layer's life (replace the envelope and growth).
    pub opacity_curve: Option<Curve>,
    pub scale_curve: Option<Curve>,
    /// Metres over which sprites fade into what is behind them (0: hard); `None` is the
    /// primitive's default (0.4 m; ground decals cut hard).
    pub soft: Option<f32>,
    /// Textured beams: branches forking off the bolt.
    pub forks: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(id: &str, capacity: u32, max_life: f32) -> CookedGraph {
        CookedGraph {
            id: id.into(),
            start: 0.0,
            at_source: false,
            follow: true,
            offset: [0.0; 3],
            wgsl: String::new(),
            stride: 4,
            capacity,
            spawners: Vec::new(),
            renderers: Vec::new(),
            parameters: Vec::new(),
            max_life,
            channels: Vec::new(),
            listens: None,
            parent: None,
        }
    }

    #[test]
    fn event_born_layers_link_to_their_parents_and_are_sized_by_them() {
        let mut rocket = graph("rocket", 10, 1.0);
        rocket.channels = vec![
            CookedChannel {
                node: "pop".into(),
                when: EventWhen::Death,
                interval: 0.1,
                max_count: 20.0,
            },
            CookedChannel {
                node: "trail".into(),
                when: EventWhen::Interval,
                interval: 0.1,
                max_count: 1.0,
            },
        ];
        let mut sparks = graph("sparks", 0, 0.5);
        sparks.listens = Some(("rocket".into(), "pop".into()));
        let mut trail = graph("trail", 0, 0.45);
        trail.listens = Some(("rocket".into(), "trail".into()));
        // A grandchild: the trail's puffs pop when they die.
        let mut puffs = graph("puffs", 0, 0.2);
        puffs.listens = Some(("trail".into(), "nothing".into()));
        let mut orphan = graph("orphan", 0, 0.2);
        orphan.listens = Some(("nowhere".into(), "pop".into()));
        // Children before their parent still link.
        let mut graphs = vec![sparks, trail, rocket, puffs, orphan];
        let warnings = link_events(&mut graphs);
        let sparks = &graphs[0];
        assert_eq!(
            sparks.parent,
            Some(CookedParent {
                graph: 2,
                channel: 0,
                per_parent: 20
            })
        );
        assert_eq!(sparks.capacity, 200);
        // A puff every 0.1 s living 0.45 s: five alive per rocket, and one more.
        let trail = &graphs[1];
        assert_eq!(
            trail.parent.map(|p| (p.channel, p.per_parent)),
            Some((1, 6))
        );
        assert_eq!(trail.capacity, 60);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(graphs[3].parent.is_none() && graphs[4].parent.is_none());
    }

    #[test]
    fn curves_hold_outside_their_keys_and_blend_between() {
        let keys = [(0.2, 1.0), (0.6, 3.0), (1.0, 0.0)];
        assert_eq!(sample_curve(&keys, 0.0), 1.0);
        assert!((sample_curve(&keys, 0.4) - 2.0).abs() < 1e-6);
        assert!((sample_curve(&keys, 0.8) - 1.5).abs() < 1e-6);
        assert_eq!(sample_curve(&keys, 1.0), 0.0);
        assert_eq!(sample_curve(&[], 0.5), 1.0);
        let colours = [(0.0, [0.0, 0.0, 0.0]), (1.0, [1.0, 0.5, 0.0])];
        assert_eq!(sample_colours(&colours, 0.5), [0.5, 0.25, 0.0]);
    }
}
