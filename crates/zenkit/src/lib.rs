//! Pure Rust reader for the asset formats of the ZenGin, the engine behind *Gothic* and *Gothic II*.
//!
//! This crate is a port of [ZenKit](https://github.com/GothicKit/ZenKit). Each file format lives
//! behind its own cargo feature, so you only compile what you use.

#![deny(unsafe_code)]

#[cfg(feature = "archive")]
pub mod archive;
mod date;
mod error;
pub mod io;
#[cfg_attr(not(feature = "material"), allow(unused_macros, unused_imports))]
mod macros;
#[cfg(feature = "material")]
pub mod material;
pub mod math;
#[cfg(feature = "mesh")]
pub mod mesh;
#[cfg_attr(not(feature = "vfs"), allow(dead_code))]
mod par;
#[cfg_attr(not(any(feature = "vfs", feature = "archive")), allow(dead_code))]
mod text;
#[cfg(feature = "texture")]
pub mod texture;
#[cfg(feature = "vfs")]
pub mod vfs;

pub use date::Date;
pub use error::{Error, Result};

/// The game a file was made for. Several formats store different fields for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameVersion {
    /// Any release of *Gothic*.
    Gothic1,
    /// Any release of *Gothic II*, including *Night of the Raven*.
    Gothic2,
}
