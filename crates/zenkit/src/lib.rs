//! Pure Rust reader for the asset formats of the ZenGin, the engine behind *Gothic* and *Gothic II*.
//!
//! This crate is a port of [ZenKit](https://github.com/GothicKit/ZenKit). Each file format lives
//! behind its own cargo feature, so you only compile what you use.

#![deny(unsafe_code)]

mod error;
pub mod io;
#[cfg_attr(not(feature = "vfs"), allow(dead_code))]
mod par;
#[cfg(feature = "vfs")]
pub mod vfs;

pub use error::{Error, Result};
