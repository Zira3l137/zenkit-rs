//! Meshes as one shape for every format, which the extension turns into Blender meshes:
//!
//! ```text
//! positions          f32 x, y, z per vertex
//! corner_counts      u32 per polygon, how many corners it has
//! corner_vertices    u32 per corner, index into positions
//! corner_uvs         f32 u, v per corner
//! corner_normals     f32 x, y, z per corner
//! polygon_materials  u32 per polygon, index into materials
//! materials          list of (name, texture) pairs
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::types::PyDict;
use zenkit::GameVersion;
use zenkit::material::AlphaFunction;
use zenkit::material::Material;
use zenkit::math::Aabb;
use zenkit::math::Color;
use zenkit::math::Obb;
use zenkit::math::Vec2;
use zenkit::math::Vec3;
use zenkit::mesh::Mesh;
use zenkit::mesh::Polygon;
use zenkit::mesh::VertexFeature;
use zenkit::mrm::MultiResolutionMesh;
use zenkit::mrm::Plane;
use zenkit::mrm::SubMesh;
use zenkit::mrm::VertexUpdate;
use zenkit::mrm::Wedge;

use crate::fail;
use crate::game;
use crate::invalid;
use crate::read;
use crate::write;

/// The mesh shape both formats are converted to and from.
#[derive(Default, FromPyObject)]
#[pyo3(from_item_all)]
pub(crate) struct MeshData {
    positions: Vec<u8>,
    corner_counts: Vec<u8>,
    corner_vertices: Vec<u8>,
    corner_uvs: Vec<u8>,
    corner_normals: Vec<u8>,
    polygon_materials: Vec<u8>,
    materials: Vec<(String, String)>,
}

pub(crate) fn f32s(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|&b| f32::from_le_bytes(b)).collect()
}

pub(crate) fn u32s(bytes: &[u8]) -> Vec<u32> {
    bytes.as_chunks::<4>().0.iter().map(|&b| u32::from_le_bytes(b)).collect()
}

pub(crate) fn vec3s(bytes: &[u8]) -> Vec<Vec3> {
    f32s(bytes).as_chunks::<3>().0.iter().map(|&v| Vec3::from_array(v)).collect()
}

fn vec2s(bytes: &[u8]) -> Vec<Vec2> {
    f32s(bytes).as_chunks::<2>().0.iter().map(|&v| Vec2::from_array(v)).collect()
}

pub(crate) fn le_bytes<T: Copy, const N: usize>(
    values: &[T],
    to_bytes: fn(T) -> [u8; N],
) -> Vec<u8> {
    values.iter().flat_map(|&v| to_bytes(v)).collect()
}

/// The unpacked arrays of a [`MeshData`], checked to fit together.
pub(crate) struct Corners {
    pub(crate) positions: Vec<Vec3>,
    pub(crate) counts: Vec<u32>,
    pub(crate) vertices: Vec<u32>,
    pub(crate) uvs: Vec<Vec2>,
    pub(crate) normals: Vec<Vec3>,
    pub(crate) materials: Vec<u32>,
}

impl MeshData {
    pub(crate) fn unpack(&self) -> PyResult<Corners> {
        let corners = Corners {
            positions: vec3s(&self.positions),
            counts: u32s(&self.corner_counts),
            vertices: u32s(&self.corner_vertices),
            uvs: vec2s(&self.corner_uvs),
            normals: vec3s(&self.corner_normals),
            materials: u32s(&self.polygon_materials),
        };
        let total: usize = corners.counts.iter().map(|&c| c as usize).sum();
        if [corners.vertices.len(), corners.uvs.len(), corners.normals.len()] != [total; 3] {
            return Err(invalid("every corner needs a vertex, a UV and a normal"));
        }
        if corners.materials.len() != corners.counts.len() {
            return Err(invalid("every polygon needs a material"));
        }
        if corners.vertices.iter().any(|&v| v as usize >= corners.positions.len()) {
            return Err(invalid("a corner uses a vertex that does not exist"));
        }
        if corners.materials.iter().any(|&m| m as usize >= self.materials.len()) {
            return Err(invalid("a polygon uses a material that does not exist"));
        }
        Ok(corners)
    }

    pub(crate) fn into_dict<'py>(self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("positions", PyBytes::new(py, &self.positions))?;
        dict.set_item("corner_counts", PyBytes::new(py, &self.corner_counts))?;
        dict.set_item("corner_vertices", PyBytes::new(py, &self.corner_vertices))?;
        dict.set_item("corner_uvs", PyBytes::new(py, &self.corner_uvs))?;
        dict.set_item("corner_normals", PyBytes::new(py, &self.corner_normals))?;
        dict.set_item("polygon_materials", PyBytes::new(py, &self.polygon_materials))?;
        dict.set_item("materials", self.materials)?;
        Ok(dict)
    }
}

/// Collects corners into [`MeshData`]'s byte arrays.
#[derive(Default)]
struct Builder {
    counts: Vec<u32>,
    vertices: Vec<u32>,
    uvs: Vec<Vec2>,
    normals: Vec<Vec3>,
    materials: Vec<u32>,
}

impl Builder {
    fn polygon(&mut self, material: u32, corners: impl IntoIterator<Item = (u32, Vec2, Vec3)>) {
        let before = self.vertices.len();
        for (vertex, uv, normal) in corners {
            self.vertices.push(vertex);
            self.uvs.push(uv);
            self.normals.push(normal);
        }
        self.counts.push((self.vertices.len() - before) as u32);
        self.materials.push(material);
    }

    /// Fails if a corner uses a vertex or a polygon a material that does not exist, which
    /// Blender would not survive.
    fn finish(self, positions: &[Vec3], materials: Vec<(String, String)>) -> PyResult<MeshData> {
        if self.vertices.iter().any(|&v| v as usize >= positions.len()) {
            return Err(invalid("the mesh has a corner on a vertex that does not exist"));
        }
        if self.materials.iter().any(|&m| m as usize >= materials.len()) {
            return Err(invalid("the mesh has a polygon with a material that does not exist"));
        }
        let floats = |v: &[Vec3]| -> Vec<f32> { v.iter().flat_map(|v| v.to_array()).collect() };
        let uvs: Vec<f32> = self.uvs.iter().flat_map(|v| v.to_array()).collect();
        Ok(MeshData {
            positions: le_bytes(&floats(positions), f32::to_le_bytes),
            corner_counts: le_bytes(&self.counts, u32::to_le_bytes),
            corner_vertices: le_bytes(&self.vertices, u32::to_le_bytes),
            corner_uvs: le_bytes(&uvs, f32::to_le_bytes),
            corner_normals: le_bytes(&floats(&self.normals), f32::to_le_bytes),
            polygon_materials: le_bytes(&self.materials, u32::to_le_bytes),
            materials,
        })
    }
}

/// A material for a mesh the engine has not compiled: the name and texture from Blender, the
/// rest as the engine sets it for new materials.
fn material(name: &str, texture: &str) -> Material {
    Material {
        name: name.into(),
        texture: texture.into(),
        color: Color::new(255, 255, 255, 255),
        smooth_angle: 60.0,
        detail_object_scale: 1.0,
        environment_mapping_strength: 1.0,
        alpha_func: AlphaFunction::None,
        ..Material::default()
    }
}

pub(crate) fn bounds(positions: &[Vec3]) -> (Aabb, Obb) {
    let (min, max) =
        positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(min, max), &p| {
            (min.min(p), max.max(p))
        });
    let (min, max) = if positions.is_empty() { (Vec3::ZERO, Vec3::ZERO) } else { (min, max) };
    let obb = Obb {
        center: (min + max) / 2.0,
        axes: [Vec3::X, Vec3::Y, Vec3::Z],
        half_width: (max - min) / 2.0,
        children: Vec::new(),
    };
    (Aabb { min, max }, obb)
}

/// The plane a polygon lies in, by Newell's method, which copes with polygons that are not
/// quite flat.
fn plane(corners: &[Vec3]) -> Plane {
    let mut normal = Vec3::ZERO;
    for (i, &a) in corners.iter().enumerate() {
        let b = corners[(i + 1) % corners.len()];
        normal += Vec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    let normal = normal.normalize_or_zero();
    Plane { normal, distance: corners.first().map_or(0.0, |&p| normal.dot(p)) }
}

/// Reads a static mesh (`.MSH`) as a mesh dict.
#[pyfunction]
pub(crate) fn read_msh(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let mesh = Mesh::from_bytes(&read(&path)?).map_err(fail)?;
    let feature = |i: u32| mesh.features.get(i as usize).copied().unwrap_or_default();

    let mut builder = Builder::default();
    for polygon in &mesh.polygons {
        let corners = mesh.polygon_vertices(polygon).iter().zip(mesh.polygon_features(polygon));
        builder.polygon(
            polygon.material.into(),
            corners.map(|(&v, &f)| (v, feature(f).texture, feature(f).normal)),
        );
    }
    let materials = mesh.materials.iter().map(|m| (m.name.clone(), m.texture.clone())).collect();
    builder.finish(&mesh.vertices, materials)?.into_dict(py)
}

/// Writes a mesh dict as a static mesh (`.MSH`) for `game`, `"G1"` or `"G2"`.
#[pyfunction]
pub(crate) fn write_msh(
    path: PathBuf,
    name: String,
    data: MeshData,
    game_name: &str,
) -> PyResult<()> {
    let game = game(game_name)?;
    let c = data.unpack()?;
    let (bbox, obb) = bounds(&c.positions);

    let mut mesh = Mesh {
        name,
        game: Some(game),
        bbox,
        obb,
        materials: data.materials.iter().map(|(n, t)| material(n, t)).collect(),
        vertices: c.positions.clone(),
        features: (c.uvs.iter().zip(&c.normals))
            .map(|(&texture, &normal)| VertexFeature { texture, light: 0xFFFF_FFFF, normal })
            .collect(),
        polygon_vertex_indices: c.vertices.clone(),
        polygon_feature_indices: (0..c.vertices.len() as u32).collect(),
        ..Mesh::default()
    };

    let mut offset = 0;
    for (&count, &material) in c.counts.iter().zip(&c.materials) {
        let count = count as usize;
        let corners: Vec<Vec3> =
            c.vertices[offset..offset + count].iter().map(|&v| c.positions[v as usize]).collect();
        let plane = plane(&corners);
        mesh.polygons.push(Polygon {
            material: u16::try_from(material).map_err(|_| invalid("more than 65535 materials"))?,
            lightmap: None,
            plane_distance: plane.distance,
            plane_normal: plane.normal,
            flags: Default::default(),
            index_offset: offset,
            index_count: count,
        });
        offset += count;
    }

    write(&path, &mesh.to_bytes(game).map_err(fail)?)
}

/// Reads a multi-resolution mesh (`.MRM`) as a mesh dict of triangles.
#[pyfunction]
pub(crate) fn read_mrm(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let mesh = MultiResolutionMesh::from_bytes(&read(&path)?).map_err(fail)?;
    from_mrm(&mesh)?.into_dict(py)
}

/// A multi-resolution mesh as triangles, one material per sub-mesh.
pub(crate) fn from_mrm(mesh: &MultiResolutionMesh) -> PyResult<MeshData> {
    let mut builder = Builder::default();
    for (index, sub) in mesh.sub_meshes.iter().enumerate() {
        for triangle in &sub.triangles {
            let wedge = |w: u16| sub.wedges.get(w as usize).copied().unwrap_or_default();
            builder.polygon(
                index as u32,
                triangle.map(|w| (u32::from(wedge(w).index), wedge(w).texture, wedge(w).normal)),
            );
        }
    }
    let materials =
        mesh.sub_meshes.iter().map(|s| (s.material.name.clone(), s.material.texture.clone()));
    builder.finish(&mesh.positions, materials.collect())
}

/// A corner's vertex, UV and normal, the floats by their bits so that they can be hashed.
type WedgeKey = (u32, [u32; 2], [u32; 3]);

/// Writes a mesh dict of triangles as a multi-resolution mesh (`.MRM`) for `game`.
///
/// Only what is needed to draw the mesh at full detail is written: one sub-mesh per material,
/// with its triangles, wedges and planes. The data the engine reduces the mesh's detail with
/// is left empty.
#[pyfunction]
pub(crate) fn write_mrm(path: PathBuf, data: MeshData, game_name: &str) -> PyResult<()> {
    let game = game(game_name)?;
    write(&path, &to_mrm(&data, game)?.to_bytes(game).map_err(fail)?)
}

/// A mesh dict of triangles as a multi-resolution mesh: one sub-mesh per material, with its
/// triangles, wedges and planes.
pub(crate) fn to_mrm(data: &MeshData, game: GameVersion) -> PyResult<MultiResolutionMesh> {
    let c = data.unpack()?;
    if c.counts.iter().any(|&count| count != 3) {
        return Err(invalid("multi-resolution meshes only hold triangles; triangulate first"));
    }
    if c.positions.len() > usize::from(u16::MAX) + 1 {
        return Err(invalid("multi-resolution meshes hold at most 65536 vertices"));
    }
    let (bbox, obb) = bounds(&c.positions);

    let mut sub_meshes: Vec<SubMesh> = data
        .materials
        .iter()
        .map(|(name, texture)| SubMesh { material: material(name, texture), ..SubMesh::default() })
        .collect();
    // Corners that share a vertex, UV and normal become one wedge.
    let mut wedge_ids: Vec<HashMap<WedgeKey, u16>> = vec![HashMap::new(); sub_meshes.len()];

    for (t, &material) in c.materials.iter().enumerate() {
        let sub = &mut sub_meshes[material as usize];
        let ids = &mut wedge_ids[material as usize];
        let mut triangle = [0u16; 3];
        for (k, slot) in triangle.iter_mut().enumerate() {
            let i = t * 3 + k;
            let (vertex, uv, normal) = (c.vertices[i], c.uvs[i], c.normals[i]);
            let key =
                (vertex, uv.to_array().map(f32::to_bits), normal.to_array().map(f32::to_bits));
            let next = sub.wedges.len();
            *slot = match ids.get(&key) {
                Some(&id) => id,
                None => {
                    let id = u16::try_from(next)
                        .map_err(|_| invalid("a material has more than 65536 corners"))?;
                    sub.wedges.push(Wedge { normal, texture: uv, index: vertex as u16 });
                    ids.insert(key, id);
                    id
                }
            };
        }
        let corners = triangle.map(|w| c.positions[sub.wedges[w as usize].index as usize]);
        sub.triangle_plane_indices.push(sub.triangle_planes.len() as u16);
        sub.triangle_planes.push(plane(&corners));
        sub.triangles.push(triangle);
    }

    // The engine draws as many of a sub-mesh's triangles as its vertex updates say. Without a
    // reduced mesh to say anything, one update with them all keeps the engine from reducing it;
    // with none, it draws nothing.
    for sub in &mut sub_meshes {
        let (triangles, wedges) =
            (u16::try_from(sub.triangles.len()), u16::try_from(sub.wedges.len()));
        let (Ok(triangles), Ok(wedges)) = (triangles, wedges) else {
            return Err(invalid("a material has more than 65535 triangles or corners"));
        };
        sub.vertex_updates = vec![VertexUpdate { triangles, wedges }];
    }

    // Fat characters have their skins pushed out along a normal per position, which the engine's
    // compiler takes from the corners; without them it reads past the end.
    let mut normals = vec![Vec3::ZERO; c.positions.len()];
    for wedge in sub_meshes.iter().flat_map(|s| &s.wedges) {
        normals[wedge.index as usize] += wedge.normal;
    }
    let normals = normals.into_iter().map(Vec3::normalize_or_zero).collect();

    Ok(MultiResolutionMesh {
        game: Some(game),
        positions: c.positions,
        normals,
        sub_meshes,
        alpha_test: true,
        bbox,
        obb,
        ..MultiResolutionMesh::default()
    })
}
