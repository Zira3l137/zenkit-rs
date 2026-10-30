//! Models: a skeleton ([`ModelHierarchy`], `.MDH`), the meshes deformed by it or attached to it
//! ([`ModelMesh`], `.MDM`), or both in one file ([`Model`], `.MDL`).
//!
//! The engine compiles NPCs into a separate hierarchy and mesh so that many bodies can share one
//! skeleton and its animations. Static props with moving parts, like doors and levers, are
//! compiled into a single `.MDL`.
//!
//! All three can be written back with their `to_bytes` methods.

mod hierarchy;
mod mesh;

use zenkit_rs_core::Asset;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::text;

pub use hierarchy::HierarchyNode;
pub use hierarchy::ModelHierarchy;
pub use mesh::Attachment;
pub use mesh::ModelMesh;
pub use mesh::SoftSkinMesh;
pub use mesh::WedgeNormal;
pub use mesh::Weight;

/// A skeleton and its meshes in one file (`.MDL`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    pub hierarchy: ModelHierarchy,
    pub mesh: ModelMesh,
}

impl Model {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        Ok(Self {
            hierarchy: ModelHierarchy::read(&mut r).context(|| "skeleton")?,
            mesh: ModelMesh::read(&mut r).context(|| "meshes")?,
        })
    }

    /// Writes the skeleton followed by the meshes, as made for `game`.
    pub fn to_bytes(&self, game: GameVersion) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        self.hierarchy.write(&mut w).context(|| "skeleton")?;
        self.mesh.write(&mut w, game).context(|| "meshes")?;
        Ok(w.into_inner())
    }
}

/// Text stored as a line: Latin-1, ending at the first line break or null byte.
fn line(format: &'static str, text: &str) -> Result<Vec<u8>> {
    let bytes = text::to_latin1(text).ok_or_else(|| {
        Error::unwritable(format, format!("{text:?} has characters beyond Latin-1"))
    })?;
    if bytes.iter().any(|b| matches!(b, 0 | b'\r' | b'\n')) {
        return Err(Error::unwritable(format, format!("{text:?} has a line break")));
    }
    Ok(bytes)
}

impl Asset for Model {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        Model::from_bytes(data)
    }
}
