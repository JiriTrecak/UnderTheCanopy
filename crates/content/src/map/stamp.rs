//! Stamps: catalogue assets placed directly on the map (`stamps`).

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StampVariant {
    Snow,
    Gold,
    Red,
    Green,
    Pink,
    Slate,
}

/// Imported walkable geometry, in the stamp's local space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WalkMesh {
    /// 3 to 4096 points.
    pub positions: Vec<[f64; 3]>,
    /// Triangles: 3 to 24576 indices into `positions`.
    pub indices: Vec<u32>,
}

/// Explicit navigation levels at the ends of a walkable stamp.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WalkConnections {
    pub start: Option<u32>,
    pub end: Option<u32>,
}

/// A walkable stamp (bridge, deck): navigation level 1 to 31, optional absolute centre height.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Walk {
    pub mesh: Option<WalkMesh>,
    pub level: u32,
    pub height: Option<f64>,
    pub connections: Option<WalkConnections>,
}

/// Authored source transform; avoids re-grounding imported models or losing precision.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceTransform {
    pub packed_user_data: Option<[u8; 3]>,
    pub height: f64,
    /// Unit quaternion `[x, y, z, w]`.
    pub quaternion: [f64; 4],
}

/// A placed catalogue asset in cell coordinates. The old parser ignores (and drops) keys it
/// does not know here, so unknown keys are accepted.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Stamp {
    pub id: String,
    pub locked: Option<bool>,
    /// Scenery asset name.
    pub asset: String,
    pub walk: Option<Walk>,
    pub source_transform: Option<SourceTransform>,
    pub x: f64,
    pub y: f64,
    /// Radians; absent on grid-snapped stamps.
    pub yaw: Option<f64>,
    /// Lean in radians around world X, at most a quarter turn.
    pub pitch: Option<f64>,
    /// Lean in radians around world Z, at most a quarter turn.
    pub roll: Option<f64>,
    /// Uniform scale; absent when 1.
    pub scale: Option<f64>,
    /// Axis proportions, 0.25 to 4.
    pub height_scale: Option<f64>,
    pub width_scale: Option<f64>,
    pub depth_scale: Option<f64>,
    /// At most 32 world units up or down.
    pub elevation: Option<f64>,
    pub variant: Option<StampVariant>,
}

/// Keys of a stamp the old parser reads; anything else is ignored.
pub(super) const STAMP_KEYS: [&str; 16] = [
    "id",
    "locked",
    "asset",
    "walk",
    "sourceTransform",
    "x",
    "y",
    "yaw",
    "pitch",
    "roll",
    "scale",
    "heightScale",
    "widthScale",
    "depthScale",
    "elevation",
    "variant",
];
