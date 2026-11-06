//! Effect graphs (D113, docs/effect-graph.md): an effect layer is a free-form graph of
//! nodes (spawners, values, attribute writers, forces, renderers) connected by wires.
//!
//! - [`graph`]: the authored form, as `base/effects/*.json` holds it and the node editor
//!   edits it.
//! - [`catalogue`]: every node kind, its pins and the WGSL it compiles to.
//! - [`check`]: types, order and stages (constant, at birth, every step), inferred from
//!   what each node depends on.
//! - [`compile`]: a checked layer to its WGSL birth and update kernels.
//! - [`simplify`]: the same layer with its bookkeeping folded away (imported layers).

pub mod catalogue;
pub mod check;
pub mod compile;
pub mod flow;
pub mod graph;
pub mod simplify;

pub use check::{Problem, Program, check};
pub use graph::{Graph, Node, Value, Wire};
