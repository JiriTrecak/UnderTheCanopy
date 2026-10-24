//! The deterministic simulation (docs/simulation.md).
//!
//! No I/O, clocks or threads: the same commands on the same content and map produce the
//! same results on every machine.

/// An encoded message (D96) the simulation keeps where it once kept English:
/// `msg!("message.build-on", name = name(id))`, `msg!("message.build-within", cells = 12, name = name(id))`.
#[macro_export]
macro_rules! msg {
    ($key:literal) => {
        canopy_content::text::Message::new($key).encode()
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        canopy_content::text::Message::new($key)
            $(.with(stringify!($name), $value))+
            .encode()
    };
}

pub mod abilities;
pub mod action;
pub mod ai;
pub mod autoplay;
pub mod campaign;
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
pub mod lab;
pub mod math;
pub mod mission;
pub mod movement;
pub mod navigation;
pub mod neutrals;
pub mod orders;
pub mod setup;
pub mod spatial;
pub mod spawn;
pub mod state;
pub mod stats;
pub mod teams;
pub mod terrain;
pub mod tunnels;
pub mod victory;
pub mod view;
pub mod vision;
pub mod world;

pub use action::Action;
pub use setup::{MatchSetup, Slot, SlotKind};
pub use world::{Queued, Save, World};
