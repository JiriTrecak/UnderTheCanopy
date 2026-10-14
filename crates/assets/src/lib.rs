//! The runtime asset pack: what `canopy-cook` writes and the game loads.
//!
//! One file (`canopy.pack`) holds every cooked asset: models with their materials,
//! skeletons and baked clips, textures already block-compressed with their mips, sounds as
//! PCM, and small data tables. Loading it decodes no glTF, PNG or WAV.
//!
//! # File layout
//!
//! Everything is little-endian and 16-byte aligned:
//!
//! ```text
//! Header (64 bytes)   magic, format version, index location, inputs digest
//! blob…               one per distinct entry, each starting with a 16-byte blob header
//! index               postcard-encoded Vec<IndexEntry>, sorted by name
//! ```
//!
//! A blob is a blob header, a postcard-encoded metadata value ([`ModelMeta`],
//! [`TextureMeta`], …) and a data region of plain arrays. Metadata refers to arrays with
//! [`Span`]s relative to the data region, so loading casts them in place
//! ([`bytemuck`]) instead of parsing. Identical blobs are stored once and shared by name.
//!
//! [`Pack::load`] reads the whole file with one read; [`Pack::open`] reads only the index
//! and fetches entries on demand.

mod error;
mod format;
mod pack;
mod texture;
mod write;

pub use error::Error;
pub use format::*;
pub use pack::{Blob, Indices, Model, Pack, Sound, Texture};
pub use write::{BlobBuilder, BlobLocation, PackWriter};

const _: () = assert!(
    cfg!(target_endian = "little"),
    "the asset pack is little-endian and is read in place"
);

/// Where a pack lives relative to the data folder: `canopy.pack` in packages,
/// `target/cooked/canopy.pack` when running from the repository.
pub const PACK_LOCATIONS: [&str; 2] = ["canopy.pack", "target/cooked/canopy.pack"];

/// The pack of a data folder, if one has been cooked.
pub fn find_pack(data: &std::path::Path) -> Option<std::path::PathBuf> {
    PACK_LOCATIONS
        .iter()
        .map(|location| data.join(location))
        .find(|path| path.is_file())
}
