use crate::Asset;
use crate::Date;
use crate::Error;
use crate::Result;
use crate::io::Reader;
use crate::math::Aabb;
use crate::math::Mat4;
use crate::math::Vec3;
use crate::text;

const FORMAT: &str = "model hierarchy";

const CHUNK_HIERARCHY: u16 = 0xD100;
const CHUNK_SOURCE: u16 = 0xD110;
const CHUNK_END: u16 = 0xD120;

/// A bone of a model's skeleton.
#[derive(Debug, Clone, PartialEq)]
pub struct HierarchyNode {
    pub name: String,
    /// Index into [`ModelHierarchy::nodes`], or `None` for a root node.
    pub parent: Option<u16>,
    /// The node's transform relative to its parent.
    pub transform: Mat4,
}

/// A model's skeleton (`.MDH`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelHierarchy {
    /// Parents always come before their children.
    pub nodes: Vec<HierarchyNode>,
    pub bbox: Aabb,
    pub collision_bbox: Aabb,
    pub root_translation: Vec3,
    /// Matches [`ModelMesh::checksum`](super::ModelMesh::checksum) and the checksum of every
    /// animation made for this skeleton.
    pub checksum: u32,
    pub source_date: Date,
    pub source_path: String,
}

impl ModelHierarchy {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::read(&mut Reader::new(data))
    }

    /// Reads chunks up to and including the hierarchy's end chunk, leaving `r` after it. That is
    /// where a `.MDL` file continues with the model mesh.
    pub(crate) fn read(r: &mut Reader) -> Result<Self> {
        let mut hierarchy = Self::default();

        while !r.is_eof() {
            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                CHUNK_HIERARCHY => {
                    let version = c.u32()?;
                    if version != 3 {
                        log::warn!("{FORMAT}: unsupported version {version}, reading anyway");
                    }

                    let count = c.u16()? as usize;
                    hierarchy.nodes.reserve(count);
                    for _ in 0..count {
                        let name = text::latin1(c.line(&[]));
                        let parent = c.i16()?;
                        let transform = c.mat4()?;
                        hierarchy.nodes.push(HierarchyNode {
                            name,
                            parent: u16::try_from(parent).ok(),
                            transform,
                        });
                    }

                    hierarchy.bbox = Aabb::read(c)?;
                    hierarchy.collision_bbox = Aabb::read(c)?;
                    hierarchy.root_translation = c.vec3()?;
                    hierarchy.checksum = c.u32()?;
                }
                CHUNK_SOURCE => {
                    hierarchy.source_date = Date::read(c)?;
                    hierarchy.source_path = text::latin1(c.line(&[]));
                }
                CHUNK_END => return Ok(hierarchy),
                id => log::debug!("{FORMAT}: skipping unknown chunk {id:#06x}"),
            }
        }

        if hierarchy.nodes.is_empty() {
            return Err(Error::malformed(FORMAT, "no hierarchy chunk"));
        }
        Ok(hierarchy)
    }

    /// Finds a node by name, ignoring ASCII case like the engine does.
    pub fn node(&self, name: &str) -> Option<(usize, &HierarchyNode)> {
        self.nodes.iter().enumerate().find(|(_, n)| n.name.eq_ignore_ascii_case(name))
    }
}

impl Asset for ModelHierarchy {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        ModelHierarchy::from_bytes(data)
    }
}
