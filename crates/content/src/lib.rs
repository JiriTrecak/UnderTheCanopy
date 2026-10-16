//! Game data: reading, resolving and validating the content the game ships with.
//!
//! The formats are exactly those the old TypeScript editors write (docs/decisions.md, D9).
//! Content is always passed in; nothing here names a shipped id.

pub mod abilities;
pub mod assets;
pub mod conditions;
pub mod effects;
pub mod error;
pub mod game;
pub mod id;
pub mod items;
pub mod map;
pub mod package;
pub mod placement;
#[cfg(feature = "testing")]
pub mod testing;

mod load;
mod registry;

pub use error::{Error, Errors};
pub use id::Id;
pub use load::{ABILITIES_FILE, Content, EFFECTS_FILE, GAME_FILE, MANIFEST_FILE};
