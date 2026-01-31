//! Virtual filesystem for BashKit
//!
//! Provides an async filesystem trait and implementations.

mod memory;
mod traits;

pub use memory::InMemoryFs;
#[allow(unused_imports)]
pub use traits::{DirEntry, FileSystem, FileType, Metadata};
