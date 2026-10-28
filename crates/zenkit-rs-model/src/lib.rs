//! Models: a skeleton ([`ModelHierarchy`], `.MDH`), the meshes deformed by it or attached to it
//! ([`ModelMesh`], `.MDM`), or both in one file ([`Model`], `.MDL`).
//!
//! The engine compiles NPCs into a separate hierarchy and mesh so that many bodies can share one
//! skeleton and its animations. Static props with moving parts, like doors and levers, are
//! compiled into a single `.MDL`.

mod hierarchy;
mod mesh;

use zenkit_rs_core::Asset;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::io::Reader;

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
}

impl Asset for Model {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        Model::from_bytes(data)
    }
}
