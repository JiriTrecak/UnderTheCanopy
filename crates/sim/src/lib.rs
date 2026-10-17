//! The deterministic simulation (docs/simulation.md).
//!
//! No I/O, clocks or threads: the same commands on the same content and map produce the
//! same results on every machine.

pub mod abilities;
pub mod action;
pub mod ai;
pub mod camps;
pub mod catalog;
pub mod combat;
pub mod crowd;
pub mod economy;
pub mod encounter;
pub mod entity;
pub mod fingerprint;
pub mod garrisons;
pub mod heroes;
pub mod items;
pub mod math;
pub mod mission;
pub mod movement;
pub mod navigation;
pub mod orders;
pub mod setup;
pub mod spatial;
pub mod spawn;
pub mod state;
pub mod stats;
pub mod teams;
pub mod terrain;
pub mod victory;
pub mod view;
pub mod vision;
pub mod world;

pub use action::Action;
pub use setup::{MatchSetup, Slot, SlotKind};
pub use world::{Queued, Save, World};
