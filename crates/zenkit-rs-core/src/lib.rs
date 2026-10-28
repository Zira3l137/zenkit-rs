//! The foundations every zenkit-rs format crate builds on: a bounds-checked byte [`Reader`](io::Reader),
//! the shared [`Error`], math types, the engine's [`Date`], and text helpers.
//!
//! Applications use these through the `zenkit-rs` crate, which re-exports them.

mod asset;
mod date;
mod error;
pub mod io;
mod macros;
pub mod math;
pub mod text;

pub use asset::Asset;
pub use date::Date;
pub use error::Error;
pub use error::Result;
pub use error::ResultExt;

/// The game a file was made for. Several formats store different fields for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameVersion {
    /// Any release of *Gothic*.
    Gothic1,
    /// Any release of *Gothic II*, including *Night of the Raven*.
    Gothic2,
}

// What the exported macros expand to, so that crates using them need no dependencies of their own.
#[doc(hidden)]
pub mod __private {
    pub use tracing;
}
