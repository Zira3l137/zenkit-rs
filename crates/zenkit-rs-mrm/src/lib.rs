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
//! materials (binary archive) | alpha test: u8, after Gothic 1 | Aabb | Obb | LodParams
//! ```
//!
//! [`MultiResolutionMesh::to_bytes`] writes a mesh back, laying the arrays out in the order the
//! engine does.

use std::borrow::Cow;

use tracing::debug;
use tracing::warn;
use zenkit_rs_archive::Archive;
use zenkit_rs_archive::ArchiveWriter;
use zenkit_rs_archive::Format;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::Record;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Obb;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_material::Material;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::KnownLayout;
use zerocopy::Unaligned;
use zerocopy::little_endian::F32;
use zerocopy::little_endian::U16;
use zerocopy::little_endian::U32;

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

/// How many of a sub-mesh's triangles and wedges the engine draws at one level of detail, the
/// engine's `zTPMVertexUpdate`. A sub-mesh has one for every number of positions from none to all
/// of them, so the last one covers the whole sub-mesh. A single update covering everything,
/// [`SubMesh::full_detail`], turns level of detail off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VertexUpdate {
    pub triangles: u16,
    pub wedges: u16,
}

/// How strongly the engine reduces a mesh's detail with distance, as its `zTLODParams`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodParams {
    pub strength: f32,
    pub z_displacement: f32,
    pub morph_percentage: f32,
    pub min_vertices: i32,
}

/// The values the engine's own meshes mostly carry.
impl Default for LodParams {
    fn default() -> Self {
        Self { strength: 1.0, z_displacement: 0.0, morph_percentage: 0.0, min_vertices: 0 }
    }
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
    /// The engine draws a sub-mesh without any as nothing, without an error, so when this is
    /// empty the writer stores [`full_detail`](Self::full_detail) instead.
    pub vertex_updates: Vec<VertexUpdate>,
}

impl SubMesh {
    /// The vertex update that draws every triangle and wedge, or `None` if there are more than
    /// an update can count.
    pub fn full_detail(&self) -> Option<VertexUpdate> {
        Some(VertexUpdate {
            triangles: self.triangles.len().try_into().ok()?,
            wedges: self.wedges.len().try_into().ok()?,
        })
    }
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
    pub lod: LodParams,
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

    /// Reads the mesh section on its own: the contents of the mesh chunk, which is how models and
    /// morph meshes embed their meshes.
    pub fn read_section(r: &mut Reader) -> Result<Self> {
        let version = r.u16()?;
        let content_len = r.u32()? as usize;
        let content = r.bytes(content_len)?;

        let sub_mesh_count = r.u8()? as usize;
        let positions = *r.record::<Section>()?;
        let normals = *r.record::<Section>()?;
        let sections = r.records::<SubMeshSections>(sub_mesh_count)?;

        let mut archive = Archive::from_reader(r.remainder())?;
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
        let lod = LodParams {
            strength: r.f32()?,
            z_displacement: r.f32()?,
            morph_percentage: r.f32()?,
            min_vertices: r.i32()?,
        };

        let positions = array::<[F32; 3]>(content, positions)?.iter().map(vec3).collect();
        let normals = array::<[F32; 3]>(content, normals)?.iter().map(vec3).collect();

        let sub_meshes = sections
            .iter()
            .zip(materials)
            .map(|(s, material)| read_sub_mesh(content, s, material))
            .collect::<Result<_>>()?;

        Ok(Self { game, positions, normals, sub_meshes, alpha_test, bbox, obb, lod })
    }

    /// Writes the mesh as a `.MRM` file: the mesh chunk and an empty end chunk.
    pub fn to_bytes(&self, game: GameVersion) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.chunk(CHUNK_MESH, |w| self.write_section(w, game))?;
        w.chunk(CHUNK_END, |_| ());
        Ok(w.into_inner())
    }

    /// Writes the mesh section on its own, the counterpart of
    /// [`read_section`](Self::read_section). Fails if there are more than 255 sub-meshes, a
    /// vertex update asks for more triangles or wedges than its sub-mesh has, or a material
    /// cannot be written.
    pub fn write_section(&self, w: &mut Writer, game: GameVersion) -> Result<()> {
        let sub_mesh_count = u8::try_from(self.sub_meshes.len()).map_err(|_| {
            let reason = format!("{} sub-meshes, at most 255 fit", self.sub_meshes.len());
            Error::unwritable(FORMAT, reason)
        })?;
        let vertex_updates = (self.sub_meshes.iter().enumerate())
            .map(|(i, sub)| vertex_updates(i, sub))
            .collect::<Result<Vec<_>>>()?;

        let mut materials = ArchiveWriter::new(Format::Binary);
        for sub in &self.sub_meshes {
            sub.material.write_named(&mut materials, game);
        }
        let materials = materials.finish()?;

        // Every array goes into one block, and a table after it says where each one starts.
        let mut content = Writer::new();
        let positions = section(&mut content, &self.positions, |w, &v| w.vec3(v));
        let normals = section(&mut content, &self.normals, |w, &v| w.vec3(v));
        let sections: Vec<[[u32; 2]; 10]> = (self.sub_meshes.iter().zip(&vertex_updates))
            .map(|(sub, updates)| write_sub_mesh(&mut content, sub, updates))
            .collect();

        w.u16(match game {
            GameVersion::Gothic1 => VERSION_G1,
            GameVersion::Gothic2 => VERSION_G2,
        });
        w.u32(content.position() as u32);
        w.bytes(content.as_bytes());

        w.u8(sub_mesh_count);
        for [offset, count] in
            [positions, normals].into_iter().chain(sections.into_iter().flatten())
        {
            w.u32(offset);
            w.u32(count);
        }

        w.bytes(&materials);
        if game == GameVersion::Gothic2 {
            w.u8(self.alpha_test.into());
        }
        self.bbox.write(w);
        self.obb.write(w)?;
        w.f32(self.lod.strength);
        w.f32(self.lod.z_displacement);
        w.f32(self.lod.morph_percentage);
        w.i32(self.lod.min_vertices);
        Ok(())
    }
}

/// Writes an array into the content block and returns its `(offset, count)`.
fn section<T>(w: &mut Writer, items: &[T], mut write: impl FnMut(&mut Writer, &T)) -> [u32; 2] {
    let offset = w.position() as u32;
    items.iter().for_each(|item| write(w, item));
    [offset, items.len() as u32]
}

/// The vertex updates to write for sub-mesh `index`: its own, checked to stay within it, or the
/// one that draws all of it.
fn vertex_updates(index: usize, sub: &SubMesh) -> Result<Cow<'_, [VertexUpdate]>> {
    let (triangles, wedges) = (sub.triangles.len(), sub.wedges.len());
    if sub.vertex_updates.is_empty() {
        let full = sub.full_detail().ok_or_else(|| {
            let reason = format!(
                "sub-mesh {index} has {triangles} triangles and {wedges} wedges, \
                 more than a vertex update can count"
            );
            Error::unwritable(FORMAT, reason)
        })?;
        return Ok(Cow::Owned(vec![full]));
    }
    if let Some(update) = (sub.vertex_updates.iter())
        .find(|u| usize::from(u.triangles) > triangles || usize::from(u.wedges) > wedges)
    {
        let reason = format!(
            "a vertex update of sub-mesh {index} draws {} triangles and {} wedges, \
             but it has {triangles} and {wedges}",
            update.triangles, update.wedges
        );
        return Err(Error::unwritable(FORMAT, reason));
    }
    Ok(Cow::Borrowed(&sub.vertex_updates))
}

/// Writes a sub-mesh's arrays in the engine's order and returns their sections in the order of
/// the table.
fn write_sub_mesh(w: &mut Writer, sub: &SubMesh, updates: &[VertexUpdate]) -> [[u32; 2]; 10] {
    let u16s = |w: &mut Writer, values: &[u16]| values.iter().for_each(|&v| w.u16(v));

    let triangles = section(w, &sub.triangles, |w, t| u16s(w, t));
    let wedges = section(w, &sub.wedges, |w, wedge| {
        w.vec3(wedge.normal);
        w.vec2(wedge.texture);
        w.u16(wedge.index);
        // Padding, which the engine leaves uninitialised.
        w.u16(0);
    });
    let wedge_map = section(w, &sub.wedge_map, |w, &v| w.u16(v));
    let colors = section(w, &sub.colors, |w, &v| w.f32(v));
    let vertex_updates = section(w, updates, |w, update| {
        w.u16(update.triangles);
        w.u16(update.wedges);
    });
    let plane_indices = section(w, &sub.triangle_plane_indices, |w, &v| w.u16(v));
    let planes = section(w, &sub.triangle_planes, |w, plane| {
        w.f32(plane.distance);
        w.vec3(plane.normal);
    });
    let triangle_edges = section(w, &sub.triangle_edges, |w, e| u16s(w, e));
    let edges = section(w, &sub.edges, |w, e| u16s(w, e));
    let edge_scores = section(w, &sub.edge_scores, |w, &v| w.f32(v));

    [
        triangles,
        wedges,
        colors,
        plane_indices,
        planes,
        wedge_map,
        vertex_updates,
        triangle_edges,
        edges,
        edge_scores,
    ]
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
        vertex_updates: array::<[U16; 2]>(content, s.vertex_updates)?
            .iter()
            .map(|&[triangles, wedges]| VertexUpdate {
                triangles: triangles.get(),
                wedges: wedges.get(),
            })
            .collect(),
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
