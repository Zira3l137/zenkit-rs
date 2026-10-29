//! Pure Rust reader for the asset formats of the ZenGin, the engine behind *Gothic* and *Gothic II*.
//!
//! This crate is a port of [ZenKit](https://github.com/GothicKit/ZenKit). Each file format is a
//! crate of its own, pulled in by a cargo feature of the same name and re-exported here as a
//! module, so you only compile what you use:
//!
//! ```no_run
//! # #[cfg(all(feature = "vfs", feature = "mrm"))]
//! # fn main() -> zenkit::Result<()> {
//! use zenkit::mrm::MultiResolutionMesh;
//! use zenkit::vfs::Overwrite;
//! use zenkit::vfs::Vfs;
//!
//! let mut vfs = Vfs::new();
//! vfs.mount_disk("Data/Meshes.vdf", Overwrite::Older)?;
//! let chest: MultiResolutionMesh = vfs.load("CHESTSMALL_OCCHESTSMALL.MRM")?;
//! # Ok(())
//! # }
//! # #[cfg(not(all(feature = "vfs", feature = "mrm")))]
//! # fn main() {}
//! ```

#[cfg(feature = "animation")]
pub use zenkit_rs_animation as animation;
#[cfg(feature = "archive")]
pub use zenkit_rs_archive as archive;
pub use zenkit_rs_core::Asset;
pub use zenkit_rs_core::Date;
pub use zenkit_rs_core::Error;
pub use zenkit_rs_core::GameVersion;
pub use zenkit_rs_core::Result;
pub use zenkit_rs_core::ResultExt;
pub use zenkit_rs_core::io;
pub use zenkit_rs_core::math;
#[cfg(feature = "cutscene")]
pub use zenkit_rs_cutscene as cutscene;
#[cfg(feature = "material")]
pub use zenkit_rs_material as material;
#[cfg(feature = "mesh")]
pub use zenkit_rs_mesh as mesh;
#[cfg(feature = "model")]
pub use zenkit_rs_model as model;
#[cfg(feature = "model-script")]
pub use zenkit_rs_model_script as model_script;
#[cfg(feature = "morph")]
pub use zenkit_rs_morph as morph;
#[cfg(feature = "mrm")]
pub use zenkit_rs_mrm as mrm;
#[cfg(feature = "save")]
pub use zenkit_rs_save as save;
#[cfg(feature = "texture")]
pub use zenkit_rs_texture as texture;
#[cfg(feature = "vfs")]
pub use zenkit_rs_vfs as vfs;
#[cfg(feature = "world")]
pub use zenkit_rs_world as world;
