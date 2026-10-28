use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::KnownLayout;
use zerocopy::Unaligned;
use zerocopy::little_endian::F32;
use zerocopy::little_endian::U32;

use tracing::debug;
use tracing::warn;

use crate::Asset;
use crate::Date;
use crate::Error;
use crate::Result;
use crate::io::Reader;
use crate::io::WHITESPACE;
use crate::math::Obb;
use crate::math::Vec3;
use crate::mrm::MultiResolutionMesh;
use crate::text;

const FORMAT: &str = "model mesh";

const CHUNK_HEADER: u16 = 0xD000;
const CHUNK_SOURCE: u16 = 0xD010;
const CHUNK_NODES: u16 = 0xD020;
const CHUNK_SOFT_SKINS: u16 = 0xD030;
const CHUNK_END: u16 = 0xD040;
const CHUNK_MRM: u16 = 0xB100;

const SKIN_CHUNK_HEADER: u16 = 0xE100;
const SKIN_CHUNK_END: u16 = 0xE110;
const SKIN_CHUNK_MRM: u16 = 0xB100;
const SKIN_CHUNK_WEIGHTS: u16 = 0xB1FF;

/// How strongly one bone pulls one vertex.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Weight {
    pub weight: f32,
    /// The vertex's position relative to the bone.
    pub position: Vec3,
    /// Index into [`SoftSkinMesh::nodes`].
    pub node: u8,
}

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct RawWeight {
    weight: F32,
    position: [F32; 3],
    node: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WedgeNormal {
    pub normal: Vec3,
    pub index: u32,
}

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct RawWedgeNormal {
    normal: [F32; 3],
    index: U32,
}

fn vec3([x, y, z]: &[F32; 3]) -> Vec3 {
    Vec3::new(x.get(), y.get(), z.get())
}

/// A mesh deformed by a skeleton: the bodies of NPCs and creatures.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SoftSkinMesh {
    pub mesh: MultiResolutionMesh,
    /// Per position in [`mesh`](Self::mesh), the bones that move it.
    pub weights: Vec<Vec<Weight>>,
    pub wedge_normals: Vec<WedgeNormal>,
    /// Indices into the [hierarchy's](super::ModelHierarchy) nodes.
    pub nodes: Vec<i32>,
    /// One per entry of [`nodes`](Self::nodes).
    pub bboxes: Vec<Obb>,
}

impl SoftSkinMesh {
    fn read(r: &mut Reader) -> Result<Self> {
        let mut skin = Self::default();

        while !r.is_eof() {
            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                SKIN_CHUNK_HEADER => {
                    let _version = c.u32()?;
                }
                SKIN_CHUNK_MRM => skin.mesh = MultiResolutionMesh::read_section(c)?,
                SKIN_CHUNK_WEIGHTS => skin.read_weights(c)?,
                SKIN_CHUNK_END => return Ok(skin),
                id => debug!("skipping unknown soft skin chunk {id:#06x}"),
            }
        }
        Err(Error::malformed(FORMAT, "soft skin mesh has no end chunk"))
    }

    fn read_weights(&mut self, c: &mut Reader) -> Result<()> {
        let len = c.u32()? as usize;
        let mut weights = c.sub(len)?;
        self.weights = (0..self.mesh.positions.len())
            .map(|_| {
                let count = weights.u32()? as usize;
                Ok(weights
                    .records::<RawWeight>(count)?
                    .iter()
                    .map(|w| Weight {
                        weight: w.weight.get(),
                        position: vec3(&w.position),
                        node: w.node,
                    })
                    .collect())
            })
            .collect::<Result<_>>()?;
        if !weights.is_eof() {
            warn!("{} bytes left in the weight section", weights.remaining());
        }

        let count = c.u32()? as usize;
        self.wedge_normals = c
            .records::<RawWedgeNormal>(count)?
            .iter()
            .map(|n| WedgeNormal { normal: vec3(&n.normal), index: n.index.get() })
            .collect();

        let count = c.u16()? as usize;
        self.nodes = (0..count).map(|_| c.i32()).collect::<Result<_>>()?;
        self.bboxes = (0..count).map(|_| Obb::read(c)).collect::<Result<_>>()?;
        Ok(())
    }
}

/// A rigid mesh that moves with one node of the skeleton, like a door or a weapon.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attachment {
    /// The name of the [hierarchy](super::ModelHierarchy) node it is attached to.
    pub node: String,
    pub mesh: MultiResolutionMesh,
}

/// A model's meshes (`.MDM`): skinned meshes and rigid attachments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelMesh {
    pub meshes: Vec<SoftSkinMesh>,
    /// In file order. ZenKit keeps these in a hash map, which loses that order.
    pub attachments: Vec<Attachment>,
    /// Matches [`ModelHierarchy::checksum`](super::ModelHierarchy::checksum).
    pub checksum: u32,
}

impl ModelMesh {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::read(&mut Reader::new(data))
    }

    /// Finds an attachment by its node name, ignoring ASCII case like the engine does.
    pub fn attachment(&self, node: &str) -> Option<&MultiResolutionMesh> {
        self.attachments.iter().find(|a| a.node.eq_ignore_ascii_case(node)).map(|a| &a.mesh)
    }

    pub(crate) fn read(r: &mut Reader) -> Result<Self> {
        let mut model = Self::default();
        let mut nodes = Vec::new();

        while !r.is_eof() {
            // The soft skin chunk's length covers only its header; the meshes follow it in the
            // surrounding stream. So that one chunk is read in place instead of as a sub-reader.
            if r.rest().starts_with(&CHUNK_SOFT_SKINS.to_le_bytes()) {
                r.skip(6)?;
                model.checksum = r.u32()?;
                let count = r.u16()? as usize;
                model.meshes = (0..count).map(|_| SoftSkinMesh::read(r)).collect::<Result<_>>()?;
                continue;
            }

            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                CHUNK_HEADER => {
                    let _version = c.u32()?;
                }
                CHUNK_SOURCE => {
                    // The date holds nonsense in the shipped files; read past it.
                    Date::read(c)?;
                    c.line(&[]);
                }
                CHUNK_NODES => {
                    let count = c.u16()? as usize;
                    nodes = (0..count).map(|_| text::latin1(c.line(WHITESPACE))).collect();
                }
                CHUNK_MRM => {
                    let index = model.attachments.len();
                    let node = nodes.get(index).cloned().ok_or_else(|| {
                        Error::malformed(FORMAT, format!("attachment {index} has no node name"))
                    })?;
                    let mesh = MultiResolutionMesh::read_section(c)?;
                    model.attachments.push(Attachment { node, mesh });
                }
                CHUNK_END => break,
                id => debug!("skipping unknown chunk {id:#06x}"),
            }
        }

        Ok(model)
    }
}

impl Asset for ModelMesh {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        ModelMesh::from_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_sizes_match_the_format() {
        assert_eq!(size_of::<RawWeight>(), 17);
        assert_eq!(size_of::<RawWedgeNormal>(), 16);
    }
}
