//! Game data: reading, resolving and validating the content the game ships with.
//!
//! Content comes in packages (docs/packages.md, D53).
//! Content is always passed in; nothing here names a shipped id.

pub mod abilities;
pub mod assets;
pub mod campaign;
pub mod conditions;
pub mod effects;
pub mod error;
pub mod game;
pub mod id;
pub mod items;
pub mod map;
pub mod package;
pub mod placement;
pub mod schema;
#[cfg(feature = "testing")]
pub mod testing;

mod load;
mod registry;

pub use error::{Error, Errors};
pub use id::Id;
pub use load::{BASE_DIR, Content, collections};
