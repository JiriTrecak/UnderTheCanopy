//! What the live scene knows about each model it may show: clips, the animation state
//! table with its event times, sockets per frame and rest bounds, read from the pack's
//! model metadata. Models are numbered in the order they are first asked for; the
//! renderer loads the same list in the same order, so an index means the same model on
//! both sides (`canopy_render::actors`).

use canopy_assets::{CLIP_FRAME_RATE, Matrix3x4, Pack};
use std::{collections::BTreeMap, sync::Arc};

/// One baked clip.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub name: String,
    pub duration: f32,
    pub frames: u32,
    /// Socket transforms in model space, frame-major (`frame · sockets + socket`).
    pub sockets: Vec<Matrix3x4>,
}

/// A gameplay animation state.
#[derive(Debug, Clone, PartialEq)]
pub struct StateInfo {
    pub clip: u16,
    pub looping: bool,
    /// Normalised time of the first `hit` or `release` event, the contact point.
    pub contact: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelInfo {
    pub name: String,
    pub clips: Vec<ClipInfo>,
    pub states: BTreeMap<String, StateInfo>,
    /// Socket node names, in the pack's socket order.
    pub sockets: Vec<String>,
    /// Bind-pose bounds (model space).
    pub rest_min: [f32; 3],
    pub rest_max: [f32; 3],
    /// Not in the pack: nothing is drawn, but entities using it are still tracked.
    pub missing: bool,
}

impl ModelInfo {
    /// Reads a model's metadata from the pack.
    pub fn read(pack: &Pack, name: &str) -> Self {
        let Ok(model) = pack.model(name) else {
            return Self {
                name: name.to_owned(),
                missing: true,
                ..Self::default()
            };
        };
        let meta = &model.meta;
        let clips = meta
            .clips
            .iter()
            .enumerate()
            .map(|(i, clip)| ClipInfo {
                name: clip.name.clone(),
                duration: clip.duration,
                frames: clip.frame_count,
                sockets: model.clip_sockets(i).to_vec(),
            })
            .collect();
        let states = meta
            .states
            .iter()
            .map(|s| {
                let contact = s
                    .events
                    .iter()
                    .find(|e| e.name == "hit" || e.name == "release")
                    .map(|e| e.time.clamp(0.0, 1.0));
                (
                    s.semantic.clone(),
                    StateInfo {
                        clip: s.clip as u16,
                        looping: s.looping,
                        contact,
                    },
                )
            })
            .collect();
        Self {
            name: name.to_owned(),
            clips,
            states,
            sockets: meta.sockets.iter().map(|s| s.node.clone()).collect(),
            rest_min: meta.rest_bounds.min,
            rest_max: meta.rest_bounds.max,
            missing: false,
        }
    }

    /// Whether the model is animated by gameplay states (a character).
    pub fn is_character(&self) -> bool {
        !self.states.is_empty()
    }

    pub fn state(&self, semantic: &str) -> Option<&StateInfo> {
        self.states.get(semantic)
    }

    pub fn has(&self, semantic: &str) -> bool {
        self.states
            .get(semantic)
            .is_some_and(|s| usize::from(s.clip) < self.clips.len())
    }

    pub fn clip(&self, name: &str) -> Option<u16> {
        self.clips
            .iter()
            .position(|c| c.name == name)
            .map(|i| i as u16)
    }

    pub fn duration(&self, clip: u16) -> f32 {
        self.clips
            .get(usize::from(clip))
            .map_or(0.0, |c| c.duration)
    }

    /// A socket's model-space transform at `time` seconds into `clip` (nearest baked
    /// frame), by node name.
    pub fn socket(&self, node: &str, clip: u16, time: f32) -> Option<Matrix3x4> {
        let socket = self.sockets.iter().position(|s| s == node)?;
        let clip = self.clips.get(usize::from(clip))?;
        let count = self.sockets.len();
        let frame = ((time.max(0.0) * CLIP_FRAME_RATE).round() as usize)
            .min(clip.frames.saturating_sub(1) as usize);
        clip.sockets.get(frame * count + socket).copied()
    }

    /// Rest height in model metres.
    pub fn height(&self) -> f32 {
        self.rest_max[1] - self.rest_min[1]
    }
}

/// Every model the live scene has asked for, numbered.
#[derive(Debug, Clone, Default)]
pub struct ModelCatalog {
    infos: Vec<Arc<ModelInfo>>,
    by_name: BTreeMap<String, u32>,
    names: Arc<Vec<String>>,
}

impl ModelCatalog {
    /// The index of model `name`, reading it from `pack` the first time.
    pub fn index(&mut self, pack: Option<&Pack>, name: &str) -> u32 {
        if let Some(&i) = self.by_name.get(name) {
            return i;
        }
        let info = match pack {
            Some(pack) => ModelInfo::read(pack, name),
            None => ModelInfo {
                name: name.to_owned(),
                missing: true,
                ..ModelInfo::default()
            },
        };
        if info.missing {
            tracing::warn!(model = name, "live model missing from the pack");
        }
        self.insert(info)
    }

    /// Adds a model described directly (tests).
    pub fn insert(&mut self, info: ModelInfo) -> u32 {
        let i = self.infos.len() as u32;
        self.by_name.insert(info.name.clone(), i);
        Arc::make_mut(&mut self.names).push(info.name.clone());
        self.infos.push(Arc::new(info));
        i
    }

    pub fn get(&self, index: u32) -> &ModelInfo {
        &self.infos[index as usize]
    }

    pub fn find(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// Model entries by index; shared until a model is added.
    pub fn names(&self) -> &Arc<Vec<String>> {
        &self.names
    }

    pub fn len(&self) -> usize {
        self.infos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }
}
