use tracing::debug;
use tracing::warn;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Date;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Mat4;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;

const FORMAT: &str = "model hierarchy";

const VERSION: u32 = 3;

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
                    if version != VERSION {
                        warn!("unsupported version {version}, reading anyway");
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
                id => debug!("skipping unknown chunk {id:#06x}"),
            }
        }

        if hierarchy.nodes.is_empty() {
            return Err(Error::malformed(FORMAT, "no hierarchy chunk"));
        }
        Ok(hierarchy)
    }

    /// The checksum the engine computes over the nodes: the CRC-32 of their names, one after the
    /// other, in order. Game files store it as [`checksum`](Self::checksum), and a model mesh
    /// stores the checksum of the skeleton it was made for, which the engine compares with the
    /// skeleton it applies the mesh to and refuses the mesh's soft skins if the two differ. Fails
    /// if a name has characters beyond Latin-1, as writing would.
    pub fn node_checksum(&self) -> Result<u32> {
        let mut crc = crc32fast::Hasher::new();
        for node in &self.nodes {
            crc.update(&crate::line(FORMAT, &node.name)?);
        }
        Ok(crc.finalize())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        self.write(&mut w)?;
        Ok(w.into_inner())
    }

    /// Writes the hierarchy's chunks, ending with its end chunk. Fails if there are more nodes
    /// than a `u16` counts, a parent index does not fit an `i16`, or a name has a line break.
    pub fn write(&self, w: &mut Writer) -> Result<()> {
        let count = u16::try_from(self.nodes.len()).map_err(|_| {
            Error::unwritable(FORMAT, format!("{} nodes, at most 65535 fit", self.nodes.len()))
        })?;
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let parent = match node.parent {
                None => -1,
                Some(parent) => i16::try_from(parent).map_err(|_| {
                    let reason = format!("node {:?} has parent {parent}, past 32767", node.name);
                    Error::unwritable(FORMAT, reason)
                })?,
            };
            nodes.push((crate::line(FORMAT, &node.name)?, parent, node.transform));
        }
        let source_path = crate::line(FORMAT, &self.source_path)?;

        w.chunk(CHUNK_HIERARCHY, |w| {
            w.u32(VERSION);
            w.u16(count);
            for (name, parent, transform) in &nodes {
                w.line(name);
                w.i16(*parent);
                w.mat4(*transform);
            }
            self.bbox.write(w);
            self.collision_bbox.write(w);
            w.vec3(self.root_translation);
            w.u32(self.checksum);
        });
        w.chunk(CHUNK_SOURCE, |w| {
            self.source_date.write(w);
            w.line(&source_path);
        });
        w.chunk(CHUNK_END, |_| ());
        Ok(())
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
