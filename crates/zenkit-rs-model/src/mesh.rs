use tracing::debug;
use tracing::warn;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Date;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::WHITESPACE;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::math::Obb;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;
use zenkit_rs_mrm::MultiResolutionMesh;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::KnownLayout;
use zerocopy::Unaligned;
use zerocopy::little_endian::F32;
use zerocopy::little_endian::U32;

const FORMAT: &str = "model mesh";

const VERSION: u32 = 0x0403_0506;
const SKIN_VERSION: u32 = 4;

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
/// Closes each attachment's mesh chunk, like the end chunk of a `.MRM` file.
const CHUNK_MRM_END: u16 = 0xB1FF;

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

    fn write(&self, w: &mut Writer, game: GameVersion) -> Result<()> {
        if self.weights.len() != self.mesh.positions.len() {
            let reason = format!(
                "{} weight lists for {} positions; every position needs one",
                self.weights.len(),
                self.mesh.positions.len()
            );
            return Err(Error::unwritable(FORMAT, reason));
        }
        if self.bboxes.len() != self.nodes.len() {
            let reason = format!(
                "{} boxes for {} nodes; every node needs one",
                self.bboxes.len(),
                self.nodes.len()
            );
            return Err(Error::unwritable(FORMAT, reason));
        }
        let node_count = u16::try_from(self.nodes.len()).map_err(|_| {
            Error::unwritable(FORMAT, format!("{} nodes, at most 65535 fit", self.nodes.len()))
        })?;

        w.chunk(SKIN_CHUNK_HEADER, |w| w.u32(SKIN_VERSION));
        w.chunk(SKIN_CHUNK_MRM, |w| self.mesh.write_section(w, game))?;
        w.chunk(SKIN_CHUNK_WEIGHTS, |w| {
            let length_at = w.position();
            w.u32(0);
            for weights in &self.weights {
                w.u32(weights.len() as u32);
                for weight in weights {
                    w.f32(weight.weight);
                    w.vec3(weight.position);
                    w.u8(weight.node);
                }
            }
            w.patch_u32(length_at, (w.position() - length_at - 4) as u32);

            w.u32(self.wedge_normals.len() as u32);
            for normal in &self.wedge_normals {
                w.vec3(normal.normal);
                w.u32(normal.index);
            }

            w.u16(node_count);
            self.nodes.iter().for_each(|&node| w.i32(node));
            self.bboxes.iter().try_for_each(|bbox| bbox.write(w))
        })?;
        w.chunk(SKIN_CHUNK_END, |_| ());
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
    /// When the source file was compiled. The shipped files hold nonsense here.
    pub source_date: Date,
    /// The `.ASC` file the meshes were compiled from.
    pub source_path: String,
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
                model.meshes = (0..count)
                    .map(|i| SoftSkinMesh::read(r).context(|| format!("soft skin mesh {i}")))
                    .collect::<Result<_>>()?;
                continue;
            }

            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                CHUNK_HEADER => {
                    let _version = c.u32()?;
                }
                CHUNK_SOURCE => {
                    model.source_date = Date::read(c)?;
                    model.source_path = text::latin1(c.line(&[]));
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
                    let mesh = MultiResolutionMesh::read_section(c)
                        .context(|| format!("attachment {node:?}"))?;
                    model.attachments.push(Attachment { node, mesh });
                }
                CHUNK_END => break,
                id => debug!("skipping unknown chunk {id:#06x}"),
            }
        }

        Ok(model)
    }

    pub fn to_bytes(&self, game: GameVersion) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        self.write(&mut w, game)?;
        Ok(w.into_inner())
    }

    /// Writes the model mesh's chunks, ending with its end chunk, as made for `game`.
    ///
    /// The engine only writes the list of attachments when there are some, and the soft skin
    /// meshes along with the checksum when there are some or the checksum is set.
    pub(crate) fn write(&self, w: &mut Writer, game: GameVersion) -> Result<()> {
        let source_path = crate::line(FORMAT, &self.source_path)?;
        let names = (self.attachments.iter())
            .map(|a| crate::line(FORMAT, &a.node))
            .collect::<Result<Vec<_>>>()?;
        let too_many = |count: usize, what: &str| {
            Error::unwritable(FORMAT, format!("{count} {what}, at most 65535 fit"))
        };
        let attachment_count =
            u16::try_from(names.len()).map_err(|_| too_many(names.len(), "attachments"))?;
        let mesh_count = u16::try_from(self.meshes.len())
            .map_err(|_| too_many(self.meshes.len(), "soft skin meshes"))?;

        w.chunk(CHUNK_HEADER, |w| w.u32(VERSION));
        w.chunk(CHUNK_SOURCE, |w| {
            self.source_date.write(w);
            w.line(&source_path);
        });

        if !names.is_empty() {
            w.chunk(CHUNK_NODES, |w| {
                w.u16(attachment_count);
                names.iter().for_each(|name| w.line(name));
            });
            for attachment in &self.attachments {
                w.chunk(CHUNK_MRM, |w| attachment.mesh.write_section(w, game))
                    .context(|| format!("attachment {:?}", attachment.node))?;
                w.chunk(CHUNK_MRM_END, |_| ());
            }
        }

        if !self.meshes.is_empty() || self.checksum != 0 {
            // As when reading, the chunk only covers the checksum and the count.
            w.chunk(CHUNK_SOFT_SKINS, |w| {
                w.u32(self.checksum);
                w.u16(mesh_count);
            });
            for (i, mesh) in self.meshes.iter().enumerate() {
                mesh.write(w, game).context(|| format!("soft skin mesh {i}"))?;
            }
        }

        w.chunk(CHUNK_END, |_| ());
        Ok(())
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
