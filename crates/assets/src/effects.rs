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
}

/// The runtime primitive a layer draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Primitive {
    /// Camera-facing sprites or soft spheres (`count` of them).
    Particle,
    /// Quads lying on the ground plane at the layer's height.
    Decal,
    /// A model (`EffectLayer::model`).
    Mesh,
    Light,
    Sound,
}

/// How a layer's parts move: the old effect player's shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Motion {
    /// One quad rising slightly over the layer's life.
    Billboard,
    /// Sparks flung out on a golden-angle spiral.
    Burst,
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
}

impl Motion {
    /// Layers that travel along a path rather than stand at an anchor.
    pub fn travels(self) -> bool {
        matches!(self, Self::Missile | Self::Wavefront)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectBlend {
    /// `src · α + dst`.
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
}

/// A stateless emitter: each particle's position is a function of its index, the cue's
/// seed and its age.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// One layer of an effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectLayer {
    pub id: String,
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
}
