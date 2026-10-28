//! Multi-resolution meshes (`.MRM`, engine class `zCProgMeshProto`), the format of almost every
//! prop, item and piece of armour.
//!
//! The mesh is one chunk holding a block of tightly packed arrays, followed by a table of where
//! each array starts and how many elements it has, a material per sub-mesh and the bounding
//! boxes:
//!
//! ```text
//! version: u16 | content length: u32 | content...
//! sub-mesh count: u8 | positions (offset, count) | normals (offset, count)
//! per sub-mesh: ten (offset, count) pairs, see SubMesh
//! materials (binary archive) | alpha test: u8, after Gothic 1 | Aabb | Obb | 16 unknown bytes
//! ```

use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::KnownLayout;
use zerocopy::Unaligned;
use zerocopy::little_endian::F32;
use zerocopy::little_endian::U16;
use zerocopy::little_endian::U32;

use tracing::debug;
use tracing::warn;

use crate::Asset;
use crate::Error;
use crate::GameVersion;
use crate::Result;
use crate::archive::Archive;
use crate::io::Reader;
use crate::io::Record;
use crate::material::Material;
use crate::math::Aabb;
use crate::math::Obb;
use crate::math::Vec2;
use crate::math::Vec3;

const FORMAT: &str = "multi-resolution mesh";

const VERSION_G1: u16 = 0x305;
const VERSION_G2: u16 = 0x905;
/// Versions from Gothic 2's development. A few models compiled with them still ship with the
/// game, such as `FIREPLACE_HIGH2.MDM` (0x705).
const VERSION_G2_EARLY: [u16; 2] = [0x705, 0x805];

const CHUNK_MESH: u16 = 0xB100;
const CHUNK_END: u16 = 0xB1FF;

/// A triangle corner. Corners that share a position but differ in normal or UV are separate
/// wedges.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Wedge {
    pub normal: Vec3,
    pub texture: Vec2,
    /// Index into [`MultiResolutionMesh::positions`].
    pub index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Plane {
    pub distance: f32,
    pub normal: Vec3,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubMesh {
    pub material: Material,
    /// Indices into [`wedges`](Self::wedges).
    pub triangles: Vec<[u16; 3]>,
    pub wedges: Vec<Wedge>,
    pub colors: Vec<f32>,
    pub triangle_plane_indices: Vec<u16>,
    pub triangle_planes: Vec<Plane>,
    pub triangle_edges: Vec<[u16; 3]>,
    pub edges: Vec<[u16; 2]>,
    pub edge_scores: Vec<f32>,
    pub wedge_map: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MultiResolutionMesh {
    pub game: Option<GameVersion>,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub sub_meshes: Vec<SubMesh>,
    pub alpha_test: bool,
    pub bbox: Aabb,
    pub obb: Obb,
}

#[derive(FromBytes, KnownLayout, Immutable, Unaligned, Clone, Copy)]
#[repr(C)]
struct Section {
    offset: U32,
    count: U32,
}

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct SubMeshSections {
    triangles: Section,
    wedges: Section,
    colors: Section,
    triangle_plane_indices: Section,
    triangle_planes: Section,
    wedge_map: Section,
    vertex_updates: Section,
    triangle_edges: Section,
    edges: Section,
    edge_scores: Section,
}

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct RawWedge {
    normal: [F32; 3],
    texture: [F32; 2],
    index: U16,
    _padding: U16,
}

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct RawPlane {
    distance: F32,
    normal: [F32; 3],
}

fn vec3([x, y, z]: &[F32; 3]) -> Vec3 {
    Vec3::new(x.get(), y.get(), z.get())
}

impl MultiResolutionMesh {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let mut mesh = None;
        while !r.is_eof() {
            let mut chunk = r.chunk()?;
            match chunk.id {
                CHUNK_MESH => mesh = Some(Self::read_section(&mut chunk.data)?),
                CHUNK_END => break,
                id => debug!("skipping unknown chunk {id:#06x}"),
            }
        }
        mesh.ok_or_else(|| Error::malformed(FORMAT, "no mesh chunk"))
    }

    /// Reads the mesh section on its own. Models embed MRMs this way, without the chunk around it.
    pub(crate) fn read_section(r: &mut Reader) -> Result<Self> {
        let version = r.u16()?;
        let content_len = r.u32()? as usize;
        let content = r.bytes(content_len)?;

        let sub_mesh_count = r.u8()? as usize;
        let positions = *r.record::<Section>()?;
        let normals = *r.record::<Section>()?;
        let sections = r.records::<SubMeshSections>(sub_mesh_count)?;

        let mut archive = Archive::new(r.rest())?;
        let materials = (0..sub_mesh_count)
            .map(|_| Material::read_named(&mut archive))
            .collect::<Result<Vec<_>>>()?;
        r.skip(archive.position())?;

        let game = match version {
            VERSION_G1 => Some(GameVersion::Gothic1),
            v if v == VERSION_G2 || VERSION_G2_EARLY.contains(&v) => Some(GameVersion::Gothic2),
            v => {
                warn!("unknown version {v:#06x}, reading it like Gothic 2's");
                None
            }
        };
        // ZenKit reads this flag for 0x805 and 0x905 only, so it misreads everything after it in
        // 0x705 meshes by one byte. Every version after Gothic 1's has it.
        let alpha_test = if version > VERSION_G1 { r.u8()? != 0 } else { true };
        let bbox = Aabb::read(r)?;
        let obb = Obb::read(r)?;
        r.skip(16)?;

        let positions = array::<[F32; 3]>(content, positions)?.iter().map(vec3).collect();
        let normals = array::<[F32; 3]>(content, normals)?.iter().map(vec3).collect();

        let sub_meshes = sections
            .iter()
            .zip(materials)
            .map(|(s, material)| read_sub_mesh(content, s, material))
            .collect::<Result<_>>()?;

        Ok(Self { game, positions, normals, sub_meshes, alpha_test, bbox, obb })
    }
}

/// Views a `(offset, count)` section of the content block as a slice of records.
fn array<T: Record>(content: &[u8], section: Section) -> Result<&[T]> {
    let (offset, count) = (section.offset.get() as usize, section.count.get() as usize);
    let mut r = Reader::new(content);
    r.seek(offset).and_then(|()| r.records::<T>(count)).map_err(|_| {
        Error::malformed(
            FORMAT,
            format!("section of {count} elements at {offset} lies outside the mesh"),
        )
    })
}

fn read_sub_mesh(content: &[u8], s: &SubMeshSections, material: Material) -> Result<SubMesh> {
    let u16s =
        |section| Ok::<_, Error>(array::<U16>(content, section)?.iter().map(|v| v.get()).collect());
    let f32s =
        |section| Ok::<_, Error>(array::<F32>(content, section)?.iter().map(|v| v.get()).collect());

    Ok(SubMesh {
        material,
        triangles: array::<[U16; 3]>(content, s.triangles)?
            .iter()
            .map(|t| t.map(|v| v.get()))
            .collect(),
        wedges: array::<RawWedge>(content, s.wedges)?
            .iter()
            .map(|w| Wedge {
                normal: vec3(&w.normal),
                texture: Vec2::new(w.texture[0].get(), w.texture[1].get()),
                index: w.index.get(),
            })
            .collect(),
        colors: f32s(s.colors)?,
        triangle_plane_indices: u16s(s.triangle_plane_indices)?,
        triangle_planes: array::<RawPlane>(content, s.triangle_planes)?
            .iter()
            .map(|p| Plane { distance: p.distance.get(), normal: vec3(&p.normal) })
            .collect(),
        triangle_edges: array::<[U16; 3]>(content, s.triangle_edges)?
            .iter()
            .map(|t| t.map(|v| v.get()))
            .collect(),
        edges: array::<[U16; 2]>(content, s.edges)?.iter().map(|e| e.map(|v| v.get())).collect(),
        edge_scores: f32s(s.edge_scores)?,
        wedge_map: u16s(s.wedge_map)?,
    })
}

impl Asset for MultiResolutionMesh {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        MultiResolutionMesh::from_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_sizes_match_the_format() {
        assert_eq!(size_of::<SubMeshSections>(), 80);
        assert_eq!(size_of::<RawWedge>(), 24);
        assert_eq!(size_of::<RawPlane>(), 16);
    }

    /// An empty mesh section: no arrays, no materials, then the trailer.
    fn empty_section(version: u16) -> Vec<u8> {
        let mut data = version.to_le_bytes().to_vec();
        data.extend(0u32.to_le_bytes());
        data.push(0);
        data.extend([0; 16]);
        data.extend(
            b"ZenGin Archive\nver 1\nzCArchiverGeneric\nBINARY\nsaveGame 0\nEND\nobjects 0        \nEND\n\n",
        );
        data.push(0);
        for v in [-1.0f32, -2.0, -3.0, 1.0, 2.0, 3.0] {
            data.extend(v.to_le_bytes());
        }
        data.extend([0; 62]);
        data.extend([0; 16]);
        data
    }

    #[test]
    fn early_gothic_2_meshes_have_the_alpha_flag() {
        for version in [0x705, 0x805, 0x905] {
            let data = empty_section(version);
            let mut r = Reader::new(&data);
            let mesh = MultiResolutionMesh::read_section(&mut r).unwrap();
            assert_eq!(mesh.game, Some(GameVersion::Gothic2));
            assert!(!mesh.alpha_test);
            assert_eq!(mesh.bbox.min, Vec3::new(-1.0, -2.0, -3.0));
            assert_eq!(mesh.bbox.max, Vec3::new(1.0, 2.0, 3.0));
            assert!(r.is_eof());
        }
    }
}
