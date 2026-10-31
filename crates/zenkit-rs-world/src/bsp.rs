use tracing::debug;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::Writer;
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

impl BspTree {
    pub(crate) fn version(game: GameVersion) -> u32 {
        match game {
            GameVersion::Gothic1 => VERSION_G1,
            GameVersion::Gothic2 => VERSION_G2,
        }
    }

    /// Writes the tree's chunks, ending with its end chunk. Fails if the nodes are not in the
    /// order they are stored in, see [`check`](Self::check).
    pub(crate) fn write(&self, w: &mut Writer, game: GameVersion) -> Result<()> {
        let leaves = self.check()?;
        let names = (self.sectors.iter())
            .map(|sector| {
                text::to_latin1(&sector.name).filter(|name| !name.contains(&b'\n')).ok_or_else(
                    || {
                        let reason =
                            format!("sector name {:?} is not one line of Latin-1", sector.name);
                        Error::unwritable(FORMAT, reason)
                    },
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let u32s = |w: &mut Writer, values: &[u32]| values.iter().for_each(|&v| w.u32(v));

        w.chunk(CHUNK_HEADER, |c| {
            // The format version, which Gothic II checks: "zCBspTree::LoadBIN: wrong bsp-file
            // version, engine: 3, file: 2". ZenKit writes 2 for both games.
            c.u16(match game {
                GameVersion::Gothic1 => 2,
                GameVersion::Gothic2 => 3,
            });
            c.u32(self.mode.raw());
        });
        w.chunk(CHUNK_POLYGONS, |c| {
            c.u32(self.polygon_indices.len() as u32);
            u32s(c, &self.polygon_indices);
        });
        w.chunk(CHUNK_TREE, |c| {
            c.u32(self.nodes.len() as u32);
            c.u32(self.leaf_nodes.len() as u32);
            for (i, node) in self.nodes.iter().enumerate() {
                node.bbox.write(c);
                c.u32(node.polygon_offset);
                c.u32(node.polygon_count);
                if leaves[i] {
                    continue;
                }
                let side = |child: Option<usize>, present: u8, leaf: u8| {
                    child.map_or(0, |child| present | if leaves[child] { leaf } else { 0 })
                };
                c.u8(side(node.front, 0b0001, 0b0100) | side(node.back, 0b0010, 0b1000));
                c.f32(node.plane_distance);
                c.vec3(node.plane_normal);
                if game == GameVersion::Gothic1 {
                    c.u8(node.lod);
                }
            }
        });
        w.chunk(CHUNK_LIGHT, |c| c.vec3s(&self.light_points));
        w.chunk(CHUNK_OUTDOORS, |c| {
            c.u32(self.sectors.len() as u32);
            for (sector, name) in self.sectors.iter().zip(&names) {
                c.line(name);
                c.u32(sector.node_indices.len() as u32);
                c.u32(sector.portal_polygon_indices.len() as u32);
                u32s(c, &sector.node_indices);
                u32s(c, &sector.portal_polygon_indices);
            }
            c.u32(self.portal_polygon_indices.len() as u32);
            u32s(c, &self.portal_polygon_indices);
        });
        // ZenKit's padding byte.
        w.chunk(CHUNK_END, |c| c.u8(b'B'));
        Ok(())
    }

    /// Checks that the tree can be stored, and gives which nodes are leaves.
    ///
    /// The file has no child indices: nodes are stored depth first, each followed by its front
    /// subtree and then its back one, and a node's flags say which children it has and whether
    /// they are leaves. So [`nodes`](Self::nodes) must be in that order, as reading leaves them,
    /// and [`leaf_nodes`](Self::leaf_nodes) must list the leaves in it, with a light point each.
    fn check(&self) -> Result<Vec<bool>> {
        let fail = |reason: String| Err(Error::unwritable(FORMAT, reason));
        let mut leaves = vec![false; self.nodes.len()];
        for (n, &leaf) in self.leaf_nodes.iter().enumerate() {
            match leaves.get_mut(leaf) {
                Some(is_leaf) => *is_leaf = true,
                None => return fail(format!("leaf {leaf} is not a node")),
            }
            if n > 0 && leaf <= self.leaf_nodes[n - 1] {
                return fail("the leaves are not listed in the order of the nodes".into());
            }
        }
        if self.light_points.len() != self.leaf_nodes.len() {
            let (points, leaves) = (self.light_points.len(), self.leaf_nodes.len());
            return fail(format!("{points} light points for {leaves} leaves"));
        }
        // The engine reads a tree of one node as a leaf and the root of a larger one as an inner
        // node.
        if self.nodes.is_empty() || leaves[0] != (self.nodes.len() == 1) {
            return fail("the root must be a leaf exactly when it is the only node".into());
        }

        let mut next = 0;
        let mut pending = vec![(0, None)];
        while let Some((index, parent)) = pending.pop() {
            let node = &self.nodes[index];
            if index != next || node.parent != parent {
                return fail(format!("node {index} is not where depth first order puts it"));
            }
            next += 1;
            let end = u64::from(node.polygon_offset) + u64::from(node.polygon_count);
            if end > self.polygon_indices.len() as u64 {
                return fail(format!("node {index} holds polygons up to {end}"));
            }
            if leaves[index] {
                if !node.is_leaf() {
                    return fail(format!("leaf {index} has children"));
                }
                continue;
            }
            for child in [node.back, node.front].into_iter().flatten() {
                if child >= self.nodes.len() {
                    return fail(format!("node {index} has child {child}, which does not exist"));
                }
                pending.push((child, Some(index)));
            }
        }
        if next != self.nodes.len() {
            return fail(format!("{} nodes are not part of the tree", self.nodes.len() - next));
        }
        Ok(leaves)
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
