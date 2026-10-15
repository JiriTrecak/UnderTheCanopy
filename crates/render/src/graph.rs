//! The frame graph: an explicit, ordered list of passes, each naming the resources it
//! reads and writes (renderer §4.1).
//!
//! The graph is built once and checked: every resource a pass reads was written by an
//! earlier pass (or is imported from outside the frame), and every written resource is
//! read later or is the frame's output. The renderer records the passes in this order;
//! profiling (CPU spans and GPU timestamps) is per pass.

use std::fmt;

/// A resource passed between passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resource {
    /// Per-view lists of visible static instances and their indirect draw arguments.
    VisibleStatics,
    /// Joint palettes of the frame's posed live models.
    Palettes,
    /// The sun's depth map.
    ShadowMap,
    /// HDR scene colour (multisampled) and its single-sample resolve.
    SceneColor,
    /// Scene depth: multisampled, and the resolved linear depth of opaque surfaces and
    /// water.
    SceneDepth,
    /// Unit wakes around the camera focus.
    RippleField,
    /// This frame's caustics pattern.
    Caustics,
    /// Screen-space contact shading at the post-effect resolution.
    ContactShade,
    /// The bloom mip chain.
    Bloom,
    /// Volumetric haze and light shafts (scattered light, transmission).
    Haze,
    /// The presented (or captured) image.
    Output,
}

impl fmt::Display for Resource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// What kind of GPU work a pass records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    Compute,
    Render,
}

/// One pass: a name for profiling and its inputs and outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pass {
    pub name: &'static str,
    pub kind: PassKind,
    pub reads: Vec<Resource>,
    pub writes: Vec<Resource>,
}

impl Pass {
    pub fn new(name: &'static str, kind: PassKind) -> Self {
        Self {
            name,
            kind,
            reads: Vec::new(),
            writes: Vec::new(),
        }
    }

    pub fn reads(mut self, resources: &[Resource]) -> Self {
        self.reads.extend_from_slice(resources);
        self
    }

    pub fn writes(mut self, resources: &[Resource]) -> Self {
        self.writes.extend_from_slice(resources);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    #[error("pass {pass} reads {resource} before any pass writes it")]
    ReadBeforeWrite {
        pass: &'static str,
        resource: Resource,
    },
    #[error("{resource} written by {pass} is never read")]
    Unused {
        pass: &'static str,
        resource: Resource,
    },
    #[error("two passes are named {0}")]
    DuplicateName(&'static str),
    #[error("no pass writes the output")]
    NoOutput,
}

/// A checked, ordered frame graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameGraph {
    passes: Vec<Pass>,
}

impl FrameGraph {
    /// Checks `passes` in order. `imported` resources exist before the frame starts.
    pub fn new(passes: Vec<Pass>, imported: &[Resource]) -> Result<Self, GraphError> {
        let mut written: Vec<Resource> = imported.to_vec();
        for (i, pass) in passes.iter().enumerate() {
            if passes[..i].iter().any(|p| p.name == pass.name) {
                return Err(GraphError::DuplicateName(pass.name));
            }
            if let Some(&resource) = pass.reads.iter().find(|r| !written.contains(r)) {
                return Err(GraphError::ReadBeforeWrite {
                    pass: pass.name,
                    resource,
                });
            }
            written.extend(pass.writes.iter().copied());
        }
        for (i, pass) in passes.iter().enumerate() {
            for &resource in &pass.writes {
                let read_later = passes[i + 1..].iter().any(|p| p.reads.contains(&resource));
                if resource != Resource::Output && !read_later {
                    return Err(GraphError::Unused {
                        pass: pass.name,
                        resource,
                    });
                }
            }
        }
        if !passes.iter().any(|p| p.writes.contains(&Resource::Output)) {
            return Err(GraphError::NoOutput);
        }
        Ok(Self { passes })
    }

    pub fn passes(&self) -> &[Pass] {
        &self.passes
    }

    /// The position of the pass named `name`.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.passes.iter().position(|p| p.name == name)
    }

    /// The last pass that reads `resource`: after it the resource's memory is free.
    pub fn last_use(&self, resource: Resource) -> Option<&'static str> {
        self.passes
            .iter()
            .rev()
            .find(|p| p.reads.contains(&resource) || p.writes.contains(&resource))
            .map(|p| p.name)
    }
}

/// Pass names of the world frame, in order.
pub mod passes {
    pub const CULL: &str = "cull";
    pub const SKIN: &str = "skin";
    pub const SHADOW: &str = "shadow";
    pub const RIPPLES: &str = "ripples";
    pub const CAUSTICS: &str = "caustics";
    pub const FORWARD: &str = "forward";
    pub const CONTACT: &str = "contact";
    pub const BLOOM: &str = "bloom";
    pub const HAZE: &str = "haze";
    pub const POST: &str = "post";
    pub const WORLD_UI: &str = "world_ui";
}

/// Optional parts of the world frame: what the world has, what the biome uses and what
/// the quality settings allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameFeatures {
    /// Water with its ripple field and caustics.
    pub water: bool,
    pub contact: bool,
    pub bloom: bool,
    pub haze: bool,
}

/// The world frame (renderer §4.1): GPU culling of statics for both views, the joint
/// palettes of the live models, the sun shadow map, the water's ripple field and caustics,
/// the forward HDR pass (terrain, statics, live models, water, world UI on the ground,
/// effects; MSAA resolved in the pass, with linear depth), the post effects the biome
/// uses (contact shading, bloom, volumetric haze), the final pass (upscale, effects, tone
/// map, grade) into the output and the world UI overlay on it.
pub fn world_frame(features: FrameFeatures) -> FrameGraph {
    use PassKind::*;
    use Resource::*;
    let mut list = vec![
        Pass::new(passes::CULL, Compute).writes(&[VisibleStatics]),
        Pass::new(passes::SKIN, Compute).writes(&[Palettes]),
        Pass::new(passes::SHADOW, Render)
            .reads(&[VisibleStatics, Palettes])
            .writes(&[ShadowMap]),
    ];
    let mut forward_reads = vec![VisibleStatics, Palettes, ShadowMap];
    if features.water {
        list.push(Pass::new(passes::RIPPLES, Render).writes(&[RippleField]));
        list.push(Pass::new(passes::CAUSTICS, Render).writes(&[Caustics]));
        forward_reads.extend([RippleField, Caustics]);
    }
    list.push(
        Pass::new(passes::FORWARD, Render)
            .reads(&forward_reads)
            .writes(&[SceneColor, SceneDepth]),
    );
    let mut post_reads = vec![SceneColor, SceneDepth];
    if features.contact {
        list.push(
            Pass::new(passes::CONTACT, Render)
                .reads(&[SceneDepth])
                .writes(&[ContactShade]),
        );
        post_reads.push(ContactShade);
    }
    if features.bloom {
        list.push(
            Pass::new(passes::BLOOM, Render)
                .reads(&[SceneColor])
                .writes(&[Bloom]),
        );
        post_reads.push(Bloom);
    }
    if features.haze {
        list.push(
            Pass::new(passes::HAZE, Render)
                .reads(&[SceneDepth, ShadowMap])
                .writes(&[Haze]),
        );
        post_reads.push(Haze);
    }
    list.push(
        Pass::new(passes::POST, Render)
            .reads(&post_reads)
            .writes(&[Output]),
    );
    list.push(
        Pass::new(passes::WORLD_UI, Render)
            .reads(&[Output])
            .writes(&[Output]),
    );
    FrameGraph::new(list, &[]).expect("the world frame graph is well formed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use PassKind::*;
    use Resource::*;

    #[test]
    fn the_world_frame_is_well_formed() {
        let graph = world_frame(FrameFeatures::default());
        assert_eq!(graph.passes().len(), 6);
        assert_eq!(graph.last_use(ShadowMap), Some(passes::FORWARD));
        assert_eq!(graph.last_use(Palettes), Some(passes::FORWARD));
        assert_eq!(graph.index(passes::POST), Some(4));
        let all = world_frame(FrameFeatures {
            water: true,
            contact: true,
            bloom: true,
            haze: true,
        });
        let names: Vec<&str> = all.passes().iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            [
                "cull", "skin", "shadow", "ripples", "caustics", "forward", "contact", "bloom",
                "haze", "post", "world_ui"
            ]
        );
        // The haze marches through the sun's shadow map after the scene is drawn.
        assert_eq!(all.last_use(ShadowMap), Some(passes::HAZE));
        assert_eq!(all.last_use(RippleField), Some(passes::FORWARD));
    }

    #[test]
    fn reading_before_writing_is_refused() {
        let error = FrameGraph::new(
            vec![
                Pass::new("a", Render).reads(&[ShadowMap]).writes(&[Output]),
                Pass::new("b", Render).writes(&[ShadowMap]),
            ],
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error,
            GraphError::ReadBeforeWrite {
                pass: "a",
                resource: ShadowMap
            }
        );
    }

    #[test]
    fn imported_resources_need_no_writer() {
        assert!(
            FrameGraph::new(
                vec![Pass::new("a", Render).reads(&[ShadowMap]).writes(&[Output])],
                &[ShadowMap],
            )
            .is_ok()
        );
    }

    #[test]
    fn dead_writes_and_missing_output_are_refused() {
        let dead = FrameGraph::new(
            vec![
                Pass::new("a", Compute).writes(&[VisibleStatics]),
                Pass::new("b", Render).writes(&[Output]),
            ],
            &[],
        );
        assert!(matches!(dead, Err(GraphError::Unused { pass: "a", .. })));
        let no_output = FrameGraph::new(vec![Pass::new("a", Compute)], &[]);
        assert_eq!(no_output, Err(GraphError::NoOutput));
        let twice = FrameGraph::new(
            vec![
                Pass::new("a", Render).writes(&[Output]),
                Pass::new("a", Render).writes(&[Output]),
            ],
            &[],
        );
        assert_eq!(twice, Err(GraphError::DuplicateName("a")));
    }
}
