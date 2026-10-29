//! Static meshes (`.MSH`), the format of world geometry and of meshes compiled by the Spacer.
//!
//! A mesh is a sequence of chunks: a header with the format version, bounding boxes, a material
//! list stored as a binary archive, lightmaps, vertices, per-corner vertex features, polygons and
//! an end marker.

use tracing::debug;
use zenkit_rs_archive::Archive;
use zenkit_rs_archive::Format;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Date;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Obb;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;
use zenkit_rs_material::Material;
use zenkit_rs_texture::Texture;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::KnownLayout;
use zerocopy::Unaligned;
use zerocopy::little_endian::F32;
use zerocopy::little_endian::U32;

const FORMAT: &str = "mesh";

const VERSION_G1: u16 = 9;
const VERSION_G2: u16 = 265;

const CHUNK_HEADER: u16 = 0xB000;
const CHUNK_BBOX: u16 = 0xB010;
const CHUNK_MATERIALS: u16 = 0xB020;
const CHUNK_LIGHTMAPS: u16 = 0xB025;
const CHUNK_LIGHTMAPS_SHARED: u16 = 0xB026;
const CHUNK_VERTICES: u16 = 0xB030;
const CHUNK_FEATURES: u16 = 0xB040;
const CHUNK_POLYGONS: u16 = 0xB050;
const CHUNK_END: u16 = 0xB060;

/// The data a polygon corner carries besides its position.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VertexFeature {
    pub texture: Vec2,
    /// The pre-baked vertex light, as packed ARGB.
    pub light: u32,
    pub normal: Vec3,
}

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct RawFeature {
    texture: [F32; 2],
    light: U32,
    normal: [F32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lightmap {
    /// Index into [`Mesh::lightmap_textures`].
    pub texture: usize,
    pub origin: Vec3,
    pub normals: [Vec3; 2],
}

/// Per-polygon flags. Gothic 1 and Gothic 2 store slightly different sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PolygonFlags {
    /// 0 for regular polygons; 1–3 for the different kinds of portal.
    pub portal: u8,
    pub is_occluder: bool,
    pub is_sector: bool,
    /// Gothic 2 only.
    pub should_relight: bool,
    pub is_outdoor: bool,
    pub is_ghost_occluder: bool,
    /// Gothic 2 only.
    pub is_dynamically_lit: bool,
    /// Gothic 1 only.
    pub is_lod: bool,
    /// Gothic 1 only.
    pub normal_axis: u8,
    pub sector_index: i16,
}

impl PolygonFlags {
    pub fn is_portal(&self) -> bool {
        self.portal != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Polygon {
    /// Index into [`Mesh::materials`].
    pub material: u16,
    /// Index into [`Mesh::lightmaps`].
    pub lightmap: Option<u16>,
    pub plane_distance: f32,
    pub plane_normal: Vec3,
    pub flags: PolygonFlags,
    /// Where this polygon's corners start in [`Mesh::polygon_vertex_indices`] and
    /// [`Mesh::polygon_feature_indices`].
    pub index_offset: usize,
    pub index_count: usize,
}

/// One triangle of a polygon, see [`Mesh::triangles`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    /// Index into [`Mesh::polygons`].
    pub polygon: usize,
    pub vertices: [u32; 3],
    pub features: [u32; 3],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub name: String,
    /// When the mesh's source file was last changed.
    pub date: Date,
    pub game: Option<GameVersion>,
    pub bbox: Aabb,
    pub obb: Obb,
    pub materials: Vec<Material>,
    pub alpha_test: bool,
    pub vertices: Vec<Vec3>,
    pub features: Vec<VertexFeature>,
    pub lightmaps: Vec<Lightmap>,
    pub lightmap_textures: Vec<Texture>,
    pub polygons: Vec<Polygon>,
    pub polygon_vertex_indices: Vec<u32>,
    pub polygon_feature_indices: Vec<u32>,
}

impl Mesh {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::read(&mut Reader::new(data), false)
    }

    /// Reads a mesh and leaves `r` right after its end chunk, where a world's BSP tree starts.
    /// `wide_indices` forces 32-bit vertex indices for Gothic 1 meshes, which XZEN worlds need.
    pub fn read(r: &mut Reader, wide_indices: bool) -> Result<Self> {
        let mut mesh = Self { alpha_test: true, ..Self::default() };
        let mut version = 0;

        while !r.is_eof() {
            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                CHUNK_HEADER => {
                    version = c.u16()?;
                    mesh.game = match version {
                        VERSION_G1 => Some(GameVersion::Gothic1),
                        VERSION_G2 => Some(GameVersion::Gothic2),
                        _ => None,
                    };
                    mesh.date = Date::read(c)?;
                    mesh.name = text::latin1(c.line(&[]));
                }
                CHUNK_BBOX => {
                    mesh.bbox = Aabb::read(c)?;
                    mesh.obb = Obb::read(c)?;
                }
                CHUNK_MATERIALS => {
                    let mut archive = Archive::from_reader(c.remainder())?;
                    if archive.format() != Format::Binary {
                        return Err(Error::unsupported(
                            FORMAT,
                            format!("{:?} material archive", archive.format()),
                        ));
                    }
                    mesh.materials = Material::read_list(&mut archive)?;
                    if version == VERSION_G2 {
                        c.skip(archive.position())?;
                        mesh.alpha_test = c.u8()? != 0;
                    }
                }
                CHUNK_LIGHTMAPS_SHARED => {
                    let count = c.u32()? as usize;
                    for _ in 0..count {
                        mesh.lightmap_textures.push(Texture::read(c)?);
                    }

                    let count = c.u32()? as usize;
                    mesh.lightmaps.reserve(count.min(c.remaining() / 40));
                    for _ in 0..count {
                        let (origin, normals) = (c.vec3()?, [c.vec3()?, c.vec3()?]);
                        let texture = c.u32()? as usize;
                        if texture >= mesh.lightmap_textures.len() {
                            return Err(Error::malformed(
                                FORMAT,
                                format!("lightmap texture {texture} does not exist"),
                            ));
                        }
                        mesh.lightmaps.push(Lightmap { texture, origin, normals });
                    }
                }
                CHUNK_LIGHTMAPS => {
                    let count = c.u32()? as usize;
                    for _ in 0..count {
                        let (origin, normals) = (c.vec3()?, [c.vec3()?, c.vec3()?]);
                        mesh.lightmap_textures.push(Texture::read(c)?);
                        mesh.lightmaps.push(Lightmap {
                            texture: mesh.lightmap_textures.len() - 1,
                            origin,
                            normals,
                        });
                    }
                }
                CHUNK_VERTICES => {
                    let count = c.u32()? as usize;
                    mesh.vertices = c.vec3s(count)?;
                }
                CHUNK_FEATURES => {
                    let count = c.u32()? as usize;
                    mesh.features = c
                        .records::<RawFeature>(count)?
                        .iter()
                        .map(|f| VertexFeature {
                            texture: Vec2::new(f.texture[0].get(), f.texture[1].get()),
                            light: f.light.get(),
                            normal: Vec3::new(
                                f.normal[0].get(),
                                f.normal[1].get(),
                                f.normal[2].get(),
                            ),
                        })
                        .collect();
                }
                CHUNK_POLYGONS => {
                    let wide = version == VERSION_G2 || wide_indices;
                    read_polygons(c, &mut mesh, version == VERSION_G2, wide)?;
                }
                CHUNK_END => break,
                id => debug!("skipping unknown chunk {id:#06x}"),
            }
        }

        Ok(mesh)
    }

    /// The vertex indices of a polygon's corners.
    pub fn polygon_vertices(&self, polygon: &Polygon) -> &[u32] {
        &self.polygon_vertex_indices
            [polygon.index_offset..polygon.index_offset + polygon.index_count]
    }

    /// The feature indices of a polygon's corners.
    pub fn polygon_features(&self, polygon: &Polygon) -> &[u32] {
        &self.polygon_feature_indices
            [polygon.index_offset..polygon.index_offset + polygon.index_count]
    }

    /// Splits every polygon into triangles. Polygons are convex fans, so corner 0 is shared by
    /// all of a polygon's triangles. Polygons with fewer than three corners are skipped.
    pub fn triangles(&self) -> impl Iterator<Item = Triangle> + '_ {
        (0..self.polygons.len()).flat_map(|i| self.polygon_triangles(i))
    }

    /// Splits the polygon at `index` into triangles, like [`triangles`](Self::triangles) does
    /// for all of them.
    ///
    /// # Panics
    ///
    /// If there is no polygon at `index`.
    pub fn polygon_triangles(&self, index: usize) -> impl Iterator<Item = Triangle> + '_ {
        let polygon = &self.polygons[index];
        let vertices = self.polygon_vertices(polygon);
        let features = self.polygon_features(polygon);
        (2..vertices.len()).map(move |b| Triangle {
            polygon: index,
            vertices: [vertices[0], vertices[b - 1], vertices[b]],
            features: [features[0], features[b - 1], features[b]],
        })
    }
}

fn read_polygons(c: &mut Reader, mesh: &mut Mesh, gothic2: bool, wide: bool) -> Result<()> {
    let count = c.u32()? as usize;
    // Sizes from the file are only trusted as far as the data could actually hold them.
    let expected = count.min(c.remaining() / 21);
    mesh.polygons.reserve(expected);
    mesh.polygon_vertex_indices.reserve(expected * 3);
    mesh.polygon_feature_indices.reserve(expected * 3);

    for _ in 0..count {
        let material = c.u16()?;
        let lightmap = c.i16()?;
        let plane_distance = c.f32()?;
        let plane_normal = c.vec3()?;

        let bit = |v: u8, n: u8| v & (1 << n) != 0;
        let flags = if gothic2 {
            let f = c.u8()?;
            PolygonFlags {
                portal: f & 0b11,
                is_occluder: bit(f, 2),
                is_sector: bit(f, 3),
                should_relight: bit(f, 4),
                is_outdoor: bit(f, 5),
                is_ghost_occluder: bit(f, 6),
                is_dynamically_lit: bit(f, 7),
                sector_index: c.i16()?,
                ..PolygonFlags::default()
            }
        } else {
            let (f, g) = (c.u8()?, c.u8()?);
            PolygonFlags {
                portal: f & 0b11,
                is_occluder: bit(f, 2),
                is_sector: bit(f, 3),
                is_lod: bit(f, 4),
                is_outdoor: bit(f, 5),
                is_ghost_occluder: bit(f, 6),
                // The two-bit axis straddles the byte boundary: bit 7 of the first byte is its
                // low bit, bit 0 of the second its high bit. ZenKit ORs both into the low bit.
                normal_axis: (f >> 7) | ((g & 1) << 1),
                sector_index: c.i16()?,
                ..PolygonFlags::default()
            }
        };

        let corners = c.u8()? as usize;
        let index_offset = mesh.polygon_vertex_indices.len();
        for _ in 0..corners {
            let vertex = if wide { c.u32()? } else { u32::from(c.u16()?) };
            mesh.polygon_vertex_indices.push(vertex);
            mesh.polygon_feature_indices.push(c.u32()?);
        }

        mesh.polygons.push(Polygon {
            material,
            lightmap: u16::try_from(lightmap).ok(),
            plane_distance,
            plane_normal,
            flags,
            index_offset,
            index_count: corners,
        });
    }
    Ok(())
}

impl Asset for Mesh {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        Mesh::from_bytes(data)
    }
}
