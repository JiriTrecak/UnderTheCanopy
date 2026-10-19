//! Visual effects (`effects/` of a package, one file each) and the bindings that attach
//! effects to ability events.
//!
//! Effects are presentation only: they never change simulation results.

mod rules;

use crate::{abilities::literal::trimmed_name, error::Error, id::Id, load::Element};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// A point, direction or rotation in effect space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EmitterMode {
    Burst,
    Continuous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Emitter {
    pub mode: EmitterMode,
    pub lifetime_ticks: u32,
    pub rate: f64,
    pub speed: f64,
    pub speed_variation: f64,
    pub direction: Vec3,
    pub cone_degrees: f64,
    pub gravity: f64,
    pub drag: f64,
    pub spawn_radius: f64,
    pub start_size: f64,
    pub end_size: f64,
    pub spin: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    /// Seconds of motion each particle is drawn along: sparks stretch into streaks along
    /// their velocity (0 or absent: round sprites).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretch: Option<f64>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum LayerAnchor {
    Source,
    #[default]
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    Ring,
    Pillar,
    Burst,
    Billboard,
    Streaks,
    Glow,
    Light,
    Rain,
    Missile,
    Beam,
    Wavefront,
    Particles,
    Sound,
    Mesh,
}

/// The literal `"geometry"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GeometryRole {
    Geometry,
}

/// The literal `"audio"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AudioRole {
    Audio,
}

/// The literal `"image"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ImageRole {
    Image,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelAnimation {
    pub clip: String,
    pub speed: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
}

/// A published model output (`<asset>/geometry[_<index>].*`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerModel {
    pub asset: Id,
    pub role: GeometryRole,
    pub index: u32,
    pub rotation: Vec3,
    pub animation: Option<ModelAnimation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentFallback {
    Origin,
    Hide,
}

/// Attaches an actor-local layer to a model socket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub socket: String,
    pub fallback: AttachmentFallback,
    pub inherit_rotation: bool,
}

/// A published audio output (`<asset>/audio[_<index>].*`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerSound {
    pub asset: Id,
    pub role: AudioRole,
    pub index: u32,
    pub volume: f64,
    pub pitch: f64,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub fade_in_ticks: u32,
    pub fade_out_ticks: u32,
    pub reference_distance: f64,
    pub max_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RotationDirection {
    Clockwise,
    Counterclockwise,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rotation {
    pub period_ticks: u32,
    pub direction: RotationDirection,
    pub phase_degrees: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Easing {
    Sine,
    Smoothstep,
    Bounce,
}

/// Oscillates between `min` and `max` every `period_ticks`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pulse {
    pub period_ticks: u32,
    pub min: f64,
    pub max: f64,
    pub easing: Easing,
    pub phase: Option<f64>,
}

/// Transform motion of a layer.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CueMotion {
    pub rotation: Option<Rotation>,
    pub scale: Option<Pulse>,
    pub opacity: Option<Pulse>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Distribution {
    Point,
    Disc,
}

/// Splashes where rain lands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Impact {
    pub duration_ticks: u32,
    pub count: u32,
    pub size: f64,
    pub spread: f64,
    pub height: f64,
    pub colour: String,
    pub accent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    Additive,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    Camera,
    Ground,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trail {
    pub duration_ticks: u32,
    pub max_points: u32,
    pub width: f64,
    pub colour: String,
    pub opacity: f64,
    pub break_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FlipbookMode {
    Once,
    Loop,
    PingPong,
}

/// Animates a texture atlas of `columns × rows` frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Flipbook {
    pub columns: u32,
    pub rows: u32,
    pub first_frame: u32,
    pub frames: u32,
    pub frame_ticks: u32,
    pub mode: FlipbookMode,
    pub random_start: bool,
}

/// A published image output (`<asset>/image[_<index>].*`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerTexture {
    pub asset: Id,
    pub role: ImageRole,
    pub index: u32,
}

/// One visual or audio element of an effect. Colours are `#rrggbb`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisualLayer {
    pub id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub sustain: bool,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub start_tick: u32,
    pub emitter: Option<Emitter>,
    pub offset: Option<Vec3>,
    #[serde(default)]
    pub anchor: LayerAnchor,
    pub shape: Shape,
    /// Visual arc above a missile path; endpoints and timing are unchanged.
    pub arc_height: Option<f64>,
    pub model: Option<LayerModel>,
    pub attachment: Option<Attachment>,
    pub sound: Option<LayerSound>,
    pub colour: String,
    pub accent: String,
    pub duration_ticks: u32,
    pub count: u32,
    pub size: f64,
    pub height: f64,
    pub motion: Option<CueMotion>,
    pub distribution: Option<Distribution>,
    pub fall_speed: Option<f64>,
    pub launch_delay_ms: Option<f64>,
    pub fall_angle_degrees: Option<f64>,
    pub fall_azimuth_degrees: Option<f64>,
    pub impact: Option<Impact>,
    pub blend: Option<Blend>,
    pub orientation: Option<Orientation>,
    /// Follow the bound actor; beams track both ends.
    pub follow: Option<bool>,
    pub intensity: Option<f64>,
    /// How much the bright centre of each additive sprite burns white (0–1): hot cores
    /// with the layer's colour at the fringe.
    pub core: Option<f64>,
    pub spread: Option<f64>,
    pub length: Option<f64>,
    pub trail: Option<Trail>,
    pub flipbook: Option<Flipbook>,
    pub texture: Option<LayerTexture>,
}

fn enabled() -> bool {
    true
}

/// A named effect: layers on a shared timeline of `duration_ticks`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisualEffect {
    /// Starts with `effect.`.
    pub id: Id,
    #[serde(deserialize_with = "trimmed_name")]
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub duration_ticks: u32,
    #[serde(default, rename = "loop")]
    pub looping: bool,
    pub layers: Vec<VisualLayer>,
}

/// The ability event an effect plays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EffectEvent {
    SplitStarted,
    SplitEnded,
    Returned,
    Interval,
    Accepted,
    Released,
    Healed,
    Damaged,
    Cancelled,
    WaveStarted,
    Wave,
    Finished,
    Projectile,
    Impact,
    StatusApplied,
    Dispelled,
    Summoned,
    Drained,
    ManaRestored,
    Teleported,
    VisionCreated,
    CriticalStrike,
    Evaded,
    Cleaved,
    WeaponEnhanced,
    EnhancedHit,
    WeaponRelease,
    WeaponOrdered,
    WeaponOrderCancelled,
    Death,
    Kill,
    Resurrected,
    RevivalStarted,
    Revived,
    RevivalCancelled,
    Converted,
    Contained,
    ReleasedContained,
    Digested,
    Sacrificed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BindingAnchor {
    Caster,
    Target,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum BindingLifetime {
    #[default]
    Finite,
    /// Lasts as long as the applied status; needs a target `statusApplied` binding.
    Status,
}

/// The literal `"delivery"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FromDelivery {
    Delivery,
}

/// The literal `"radius"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FromRadius {
    Radius,
}

/// Plays an effect when an ability event happens.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectBinding {
    pub id: String,
    pub effect: Id,
    pub status_id: Option<String>,
    pub event: EffectEvent,
    pub anchor: BindingAnchor,
    #[serde(default)]
    pub lifetime: BindingLifetime,
    pub duration_from: Option<FromDelivery>,
    pub size_from: Option<FromRadius>,
    pub spread_from: Option<FromRadius>,
}

/// Parses every effect (sorted by id) and checks the format's rules.
pub(crate) fn read(elements: Vec<Element>, errors: &mut Vec<Error>) -> Option<Vec<VisualEffect>> {
    let before = errors.len();
    let files: std::collections::BTreeMap<String, String> = elements
        .iter()
        .map(|e| (e.id.clone(), e.file.clone()))
        .collect();
    let effects: Vec<VisualEffect> = elements
        .into_iter()
        .filter_map(|e| e.parse(errors))
        .collect();
    if errors.len() != before {
        return None;
    }
    rules::check(&effects, &files, errors);
    Some(effects)
}

/// Reads and checks the effects of the base package of `data` on their own, without the
/// rest of the content (the cooker converts them into runtime primitives).
pub fn load_base(data: &std::path::Path) -> Result<Vec<VisualEffect>, crate::Errors> {
    let base = crate::package::Package::open(&data.join(crate::BASE_DIR))
        .map_err(|e| crate::Errors(vec![e]))?;
    let mut errors = Vec::new();
    let mut files = std::collections::BTreeMap::new();
    let elements = crate::load::elements(
        &crate::collections::EFFECTS,
        std::slice::from_ref(&base),
        &mut files,
        &mut errors,
    );
    match read(elements, &mut errors) {
        Some(effects) if errors.is_empty() => Ok(effects),
        _ => Err(crate::Errors(errors)),
    }
}
