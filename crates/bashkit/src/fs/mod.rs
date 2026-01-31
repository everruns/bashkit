//! Virtual filesystem for BashKit
//!
//! Provides an async filesystem trait and implementations:
//! - `InMemoryFs`: Simple in-memory filesystem
//! - `OverlayFs`: Copy-on-write overlay with whiteouts
//! - `MountableFs`: Multiple filesystems at mount points

mod memory;
mod mountable;
mod overlay;
mod traits;

pub use memory::InMemoryFs;
pub use mountable::MountableFs;
pub use overlay::OverlayFs;
#[allow(unused_imports)]
pub use traits::{DirEntry, FileSystem, FileType, Metadata};
