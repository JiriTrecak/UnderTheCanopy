//! The model index the cooker reads, built from asset folders ([`super::package`]): each
//! model's transform, capabilities and geometry.

use super::{AuthoringId, Manifest, ResourceRef, True, output_paths};
use crate::{error::Error, game::Dimensions};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeSet;

/// A published asset with geometry.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedModel {
    pub id: AuthoringId,
    pub transform: ModelTransform,
    /// Copy of `capabilities.groundContact.mode`.
    pub ground_contact: Option<GroundContactMode>,
    /// Giant overhanging foliage whose crown dissolves around units behind it.
    pub canopy: Option<True>,
    /// Absent when the asset declares none.
    pub capabilities: Option<Capabilities>,
    /// Published geometry paths, in resource order.
    pub geometry: Vec<String>,
    /// Scenery ids bound to this model.
    pub scenery: Vec<AuthoringId>,
}

/// The authored transform; placement scale and rotation apply outside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelTransform {
    pub scale: f64,
    pub pivot: [f64; 3],
    pub up: Up,
    pub forward: Forward,
}

impl ModelTransform {
    /// Unit scale, no pivot offset, Y up, facing +Z.
    pub fn identity() -> Self {
        Self {
            scale: 1.0,
            pivot: [0.0; 3],
            up: Up::Y,
            forward: Forward::PlusZ,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Up {
    Y,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Forward {
    #[serde(rename = "+Z")]
    PlusZ,
    #[serde(rename = "-Z")]
    MinusZ,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capabilities {
    pub dimensions: Option<Dimensions>,
    pub wind: Option<Wind>,
    pub team_color: Option<TeamColor>,
    pub animations: Option<Vec<Animation>>,
    pub sockets: Option<Vec<Attachment>>,
    pub blocker: Option<ModelBlocker>,
    pub vegetation_clearance: Option<f64>,
    pub canopy: Option<True>,
    pub ground_contact: Option<GroundContact>,
    pub walkable: Option<Walkable>,
    pub harvesting: Option<HarvestingCapability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindMode {
    Tree,
    Foliage,
    Grass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Wind {
    pub mode: WindMode,
    pub strength: f64,
    pub speed: f64,
    pub stiffness: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TeamColorMode {
    Material,
    Mask,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamColor {
    pub mode: TeamColorMode,
    /// Material slots that take the player colour.
    pub slots: Vec<String>,
    pub mask: Option<ResourceRef>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Animation {
    pub semantic: AuthoringId,
    pub clip: String,
    #[serde(rename = "loop")]
    pub looping: bool,
    pub nominal_speed: Option<f64>,
    pub events: Vec<AnimationEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnimationEvent {
    pub name: AuthoringId,
    /// Seconds into the clip.
    pub time: f64,
}

/// A named attachment point on a model node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub name: AuthoringId,
    pub node: String,
    pub offset: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelBlockerShape {
    Box,
    Ellipse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelBlocker {
    pub shape: ModelBlockerShape,
    pub size: [f64; 2],
    pub offset: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroundContactMode {
    Pivot,
    Terrain,
    Water,
    Free,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroundContact {
    pub mode: GroundContactMode,
    pub underlay: Option<ResourceRef>,
    pub ambient_radius: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Walkable {
    pub surface: ResourceRef,
    pub connectors: Vec<Connector>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Connector {
    pub name: AuthoringId,
    pub position: [f64; 3],
    pub width: f64,
}

/// A harvestable model is replaced by `replacement` once felled.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarvestingCapability {
    pub replacement: AuthoringId,
    pub definition: Option<AuthoringId>,
}

/// Model rules against the published records: every model is a record with at least one
/// geometry output, and its scenery ids are published scenery entries.
pub fn validate(models: &[PublishedModel], manifest: &Manifest, errors: &mut Vec<Error>) {
    let records: BTreeSet<&str> = manifest.records.iter().map(|r| r.id.as_str()).collect();
    let scenery: BTreeSet<&str> = super::scenery(manifest).map(|s| s.id.as_str()).collect();
    let outputs = output_paths(manifest);
    for model in models {
        let location = model.id.to_string();
        let mut fail = |message: String| errors.push(Error::new("models", &location, message));
        if !records.contains(model.id.as_str()) {
            fail("not a published asset record".into());
        }
        if model.geometry.is_empty() {
            fail("needs geometry".into());
        }
        for path in &model.geometry {
            if !outputs.contains(path.as_str()) {
                fail(format!("geometry {path} is not a published output"));
            }
        }
        for id in &model.scenery {
            if !scenery.contains(id.as_str()) {
                fail(format!("unknown scenery {id}"));
            }
        }
        let mode = model
            .capabilities
            .as_ref()
            .and_then(|c| c.ground_contact.as_ref())
            .map(|g| g.mode);
        if model.ground_contact != mode {
            fail("groundContact must copy capabilities.groundContact.mode".into());
        }
        let canopy = model.capabilities.as_ref().and_then(|c| c.canopy);
        if model.canopy != canopy {
            fail("canopy must copy capabilities.canopy".into());
        }
    }
}
