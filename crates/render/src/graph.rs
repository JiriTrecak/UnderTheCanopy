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
    /// Scene depth (multisampled).
    SceneDepth,
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
    pub const FORWARD: &str = "forward";
    pub const POST: &str = "post";
}

/// The world frame (renderer §4.1, the parts that exist so far): GPU culling of statics
/// for both views, joint palettes of the live models, the sun shadow map, the forward HDR
/// pass (terrain, statics, live models, water; MSAA resolved in the pass), and resolve +
/// tone map + grade into the output.
pub fn world_frame() -> FrameGraph {
    use PassKind::*;
    use Resource::*;
    FrameGraph::new(
        vec![
            Pass::new(passes::CULL, Compute).writes(&[VisibleStatics]),
            Pass::new(passes::SKIN, Compute).writes(&[Palettes]),
            Pass::new(passes::SHADOW, Render)
                .reads(&[VisibleStatics, Palettes])
                .writes(&[ShadowMap]),
            Pass::new(passes::FORWARD, Render)
                .reads(&[VisibleStatics, Palettes, ShadowMap])
                .writes(&[SceneColor, SceneDepth]),
            Pass::new(passes::POST, Render)
                .reads(&[SceneColor, SceneDepth])
                .writes(&[Output]),
        ],
        &[],
    )
    .expect("the world frame graph is well formed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use PassKind::*;
    use Resource::*;

    #[test]
    fn the_world_frame_is_well_formed() {
        let graph = world_frame();
        assert_eq!(graph.passes().len(), 5);
        assert_eq!(graph.last_use(ShadowMap), Some(passes::FORWARD));
        assert_eq!(graph.last_use(Palettes), Some(passes::FORWARD));
        assert_eq!(graph.index(passes::POST), Some(4));
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
