//! Models: a skeleton ([`ModelHierarchy`], `.MDH`), the meshes deformed by it or attached to it
//! ([`ModelMesh`], `.MDM`), or both in one file ([`Model`], `.MDL`).
//!
//! The engine compiles NPCs into a separate hierarchy and mesh so that many bodies can share one
//! skeleton and its animations. Static props with moving parts, like doors and levers, are
//! compiled into a single `.MDL`.

mod hierarchy;
mod mesh;

pub use hierarchy::{HierarchyNode, ModelHierarchy};
pub use mesh::{Attachment, ModelMesh, SoftSkinMesh, WedgeNormal, Weight};

use crate::io::Reader;
use crate::{Asset, Result};

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
            hierarchy: ModelHierarchy::read(&mut r)?,
            mesh: ModelMesh::read(&mut r)?,
        })
    }
}

impl Asset for Model {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        Model::from_bytes(data)
    }
}
