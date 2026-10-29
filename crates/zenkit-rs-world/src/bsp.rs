use tracing::debug;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::raw_enum;
use zenkit_rs_core::text;
use zerocopy::little_endian::U32;

const FORMAT: &str = "BSP tree";

pub(crate) const VERSION_G1: u32 = 0x0209_0000;
pub(crate) const VERSION_G2: u32 = 0x0409_0000;

const CHUNK_HEADER: u16 = 0xC000;
const CHUNK_POLYGONS: u16 = 0xC010;
const CHUNK_TREE: u16 = 0xC040;
const CHUNK_LIGHT: u16 = 0xC045;
const CHUNK_OUTDOORS: u16 = 0xC050;
pub(crate) const CHUNK_END: u16 = 0xC0FF;

raw_enum! {
    pub enum BspMode: u8 = Indoor {
        Indoor = 0,
        Outdoor = 1,
    }
}

/// A node of a [`BspTree`]. Inner nodes split space with a plane; leaves hold polygons.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BspNode {
    pub plane_normal: Vec3,
    pub plane_distance: f32,
    pub bbox: Aabb,
    /// Where this node's polygons start in [`BspTree::polygon_indices`].
    pub polygon_offset: u32,
    pub polygon_count: u32,
    /// The child on the side the plane's normal points to, as an index into
    /// [`BspTree::nodes`].
    pub front: Option<usize>,
    pub back: Option<usize>,
    pub parent: Option<usize>,
    /// Gothic 1 only.
    pub lod: u8,
}

impl BspNode {
    pub fn is_leaf(&self) -> bool {
        self.front.is_none() && self.back.is_none()
    }
}

/// A named part of an outdoor world, like a cave or a building, whose inside the engine handles
/// as indoors.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BspSector {
    pub name: String,
    /// Indices into [`BspTree::nodes`].
    pub node_indices: Vec<u32>,
    /// The polygons leading into the sector, as indices into the world mesh's polygons.
    pub portal_polygon_indices: Vec<u32>,
}

/// The binary space partitioning tree of a world, which the engine uses to find what is visible
/// and to cast rays quickly.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BspTree {
    pub mode: BspMode,
    /// Polygon indices into the world mesh, grouped by node.
    pub polygon_indices: Vec<u32>,
    /// Depth first, each node followed by its front subtree and then its back one.
    pub nodes: Vec<BspNode>,
    /// Indices into [`nodes`](Self::nodes) of every leaf, in order.
    pub leaf_nodes: Vec<usize>,
    /// One per leaf, the point the leaf samples its light at.
    pub light_points: Vec<Vec3>,
    pub sectors: Vec<BspSector>,
    pub portal_polygon_indices: Vec<u32>,
    /// Every world mesh polygon some leaf holds, sorted and without duplicates. Only these are
    /// drawn; the others are left over from building the tree.
    pub leaf_polygons: Vec<u32>,
}

impl BspTree {
    /// The polygons a node holds, as indices into the world mesh's polygons.
    ///
    /// # Panics
    ///
    /// If the node does not belong to this tree.
    pub fn node_polygons(&self, node: &BspNode) -> &[u32] {
        let start = node.polygon_offset as usize;
        &self.polygon_indices[start..start + node.polygon_count as usize]
    }

    pub(crate) fn read(r: &mut Reader, version: u32) -> Result<Self> {
        let mut tree = Self::default();

        while !r.is_eof() {
            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                CHUNK_HEADER => {
                    c.u16()?;
                    tree.mode = BspMode::from_raw(c.u32()?);
                }
                CHUNK_POLYGONS => {
                    let count = c.u32()? as usize;
                    tree.polygon_indices = u32s(c, count)?;
                }
                CHUNK_TREE => read_nodes(c, &mut tree, version == VERSION_G1)?,
                CHUNK_LIGHT => tree.light_points = c.vec3s(tree.leaf_nodes.len())?,
                CHUNK_OUTDOORS => {
                    let count = c.u32()? as usize;
                    tree.sectors.reserve(count.min(c.remaining() / 9));
                    for _ in 0..count {
                        let name = text::latin1(c.line(&[]));
                        let (nodes, portals) = (c.u32()? as usize, c.u32()? as usize);
                        tree.sectors.push(BspSector {
                            name,
                            node_indices: u32s(c, nodes)?,
                            portal_polygon_indices: u32s(c, portals)?,
                        });
                    }
                    let count = c.u32()? as usize;
                    tree.portal_polygon_indices = u32s(c, count)?;
                }
                CHUNK_END => break,
                id => debug!("skipping unknown BSP chunk {id:#06x}"),
            }
        }

        for node in &tree.nodes {
            let end = u64::from(node.polygon_offset) + u64::from(node.polygon_count);
            if end > tree.polygon_indices.len() as u64 {
                return Err(Error::malformed(
                    FORMAT,
                    format!(
                        "a node holds polygons up to {end}, but there are only {}",
                        tree.polygon_indices.len()
                    ),
                ));
            }
        }

        let mut leaf_polygons: Vec<u32> = (tree.leaf_nodes.iter())
            .flat_map(|&leaf| tree.node_polygons(&tree.nodes[leaf]))
            .copied()
            .collect();
        leaf_polygons.sort_unstable();
        leaf_polygons.dedup();
        tree.leaf_polygons = leaf_polygons;

        Ok(tree)
    }
}

fn u32s(r: &mut Reader, count: usize) -> Result<Vec<u32>> {
    Ok(r.records::<U32>(count)?.iter().map(|v| v.get()).collect())
}

#[derive(Clone, Copy)]
enum Side {
    Front,
    Back,
}

/// Reads the nodes, which are stored depth first. An inner node's flags say which children it has
/// and whether they are leaves; leaves are shorter, since they have no plane.
fn read_nodes(c: &mut Reader, tree: &mut BspTree, gothic1: bool) -> Result<()> {
    let count = c.u32()? as usize;
    let leaves = c.u32()? as usize;
    tree.nodes.reserve(count.min(c.remaining() / 32));
    tree.leaf_nodes.reserve(leaves.min(count));

    // Nodes still to read, last first: their parent and side, and whether they are leaves. A
    // stack rather than recursion, so that a malicious tree cannot overflow the call stack.
    let mut pending = vec![(None, count == 1)];
    while let Some((parent, leaf)) = pending.pop() {
        let index = tree.nodes.len();
        let mut node = BspNode {
            bbox: Aabb::read(c)?,
            polygon_offset: c.u32()?,
            polygon_count: c.u32()?,
            parent: parent.map(|(p, _)| p),
            ..BspNode::default()
        };

        match parent {
            Some((p, Side::Front)) => tree.nodes[p].front = Some(index),
            Some((p, Side::Back)) => tree.nodes[p].back = Some(index),
            None => {}
        }

        if leaf {
            tree.leaf_nodes.push(index);
        } else {
            let flags = c.u8()?;
            node.plane_distance = c.f32()?;
            node.plane_normal = c.vec3()?;
            if gothic1 {
                node.lod = c.u8()?;
            }
            // Pushed back first, so that the front subtree is read first.
            if flags & 0b10 != 0 {
                pending.push((Some((index, Side::Back)), flags & 0b1000 != 0));
            }
            if flags & 0b01 != 0 {
                pending.push((Some((index, Side::Front)), flags & 0b0100 != 0));
            }
        }
        tree.nodes.push(node);
    }
    Ok(())
}
