//! The deterministic simulation (docs/simulation.md).
//!
//! No I/O, clocks or threads: the same commands on the same content and map produce the
//! same results on every machine.

pub mod action;
pub mod catalog;
pub mod entity;
pub mod fingerprint;
pub mod math;
pub mod setup;
pub mod spawn;
pub mod state;
pub mod terrain;
pub mod view;
pub mod world;

pub use action::Action;
pub use setup::{MatchSetup, Slot, SlotKind};
pub use world::{Queued, Save, World};
