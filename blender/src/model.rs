//! Models: skeletons (`.MDH`), their meshes (`.MDM`) or both (`.MDL`), as one dict:
//!
//! ```text
//! nodes        list of (name, parent index or -1, transform): the transform relative to the
//!              parent, 16 f32 row by row, in the rest pose the meshes were skinned in
//! checksum     ties a skeleton to its meshes and animations
//! soft_skins   list of {"mesh": mesh dict, "weight_counts", "weight_nodes", "weight_values"}:
//!              per vertex, how many bones move it (u32), then per bone its node index (u32)
//!              and weight (f32)
//! attachments  list of {"node": name, "mesh": mesh dict}, each mesh in its node's space
//! ```
//!
//! A `.MDH` has no meshes and a `.MDM` no nodes; the dict has empty lists for them.
//!
//! Skeletons made for animation (a `.MDH`, and the `.MDM` beside it) keep the root's position
//! apart from its transform, as the root translation, since animations move the root
//! themselves. The dict has it back in the root's transform, where the meshes expect it; writing
//! takes it out again for those files.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::types::PyDict;
use zenkit::GameVersion;
use zenkit::math::Aabb;
use zenkit::math::Mat4;
use zenkit::math::Obb;
use zenkit::math::Vec3;
use zenkit::model::Attachment;
use zenkit::model::HierarchyNode;
use zenkit::model::Model;
use zenkit::model::ModelHierarchy;
use zenkit::model::ModelMesh;
use zenkit::model::SoftSkinMesh;
use zenkit::model::Weight;

use crate::fail;
use crate::game;
use crate::invalid;
use crate::mesh;
use crate::mesh::MeshData;
use crate::read;
use crate::write;

/// What a file of this name holds: a skeleton, meshes, or both.
pub(crate) fn kind(path: &Path) -> PyResult<(bool, bool)> {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    match extension.to_ascii_uppercase().as_str() {
        "MDL" => Ok((true, true)),
        "MDH" => Ok((true, false)),
        "MDM" => Ok((false, true)),
        other => Err(invalid(format!("{other:?} is not a model file; expected MDL, MDH or MDM"))),
    }
}

/// Reads a `.MDL`, `.MDH` or `.MDM` as a model dict.
#[pyfunction]
pub(crate) fn read_model(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    to_dict(py, &read(&path)?, kind(&path)?)
}

/// The model dict of a file's contents, `kind` saying what the file holds.
pub(crate) fn to_dict<'py>(
    py: Python<'py>,
    data: &[u8],
    kind: (bool, bool),
) -> PyResult<Bound<'py, PyDict>> {
    let (hierarchy, mesh) = match kind {
        (true, true) => {
            let model = Model::from_bytes(data).map_err(fail)?;
            (Some(model.hierarchy), Some(model.mesh))
        }
        (true, false) => (Some(ModelHierarchy::from_bytes(data).map_err(fail)?), None),
        _ => (None, Some(ModelMesh::from_bytes(data).map_err(fail)?)),
    };

    let dict = PyDict::new(py);
    let rest = hierarchy.as_ref().map(rest_pose).unwrap_or_default();
    let nodes = hierarchy.iter().flat_map(|h| &h.nodes).zip(rest).map(|(node, transform)| {
        let rows: Vec<f32> = transform.transpose().to_cols_array().to_vec();
        let parent = node.parent.map_or(-1, i32::from);
        (node.name.clone(), parent, PyBytes::new(py, &mesh::le_bytes(&rows, f32::to_le_bytes)))
    });
    dict.set_item("nodes", nodes.collect::<Vec<_>>())?;
    let checksum = hierarchy.as_ref().map(|h| h.checksum).or(mesh.as_ref().map(|m| m.checksum));
    dict.set_item("checksum", checksum.unwrap_or(0))?;

    let mut soft_skins = Vec::new();
    let mut attachments = Vec::new();
    if let Some(mesh) = &mesh {
        for skin in &mesh.meshes {
            let item = PyDict::new(py);
            item.set_item("mesh", mesh::from_mrm(&skin.mesh)?.into_dict(py)?)?;
            let (mut counts, mut nodes, mut values) = (Vec::new(), Vec::new(), Vec::new());
            for weights in &skin.weights {
                counts.push(weights.len() as u32);
                for weight in weights {
                    let node = skin.nodes.get(weight.node as usize).copied().unwrap_or(0);
                    nodes.push(node.max(0) as u32);
                    values.push(weight.weight);
                }
            }
            item.set_item(
                "weight_counts",
                PyBytes::new(py, &mesh::le_bytes(&counts, u32::to_le_bytes)),
            )?;
            item.set_item(
                "weight_nodes",
                PyBytes::new(py, &mesh::le_bytes(&nodes, u32::to_le_bytes)),
            )?;
            item.set_item(
                "weight_values",
                PyBytes::new(py, &mesh::le_bytes(&values, f32::to_le_bytes)),
            )?;
            soft_skins.push(item);
        }
        for attachment in &mesh.attachments {
            let item = PyDict::new(py);
            item.set_item("node", &attachment.node)?;
            item.set_item("mesh", mesh::from_mrm(&attachment.mesh)?.into_dict(py)?)?;
            attachments.push(item);
        }
    }
    dict.set_item("soft_skins", soft_skins)?;
    dict.set_item("attachments", attachments)?;
    Ok(dict)
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
pub(crate) struct ModelData {
    nodes: Vec<(String, i32, Vec<u8>)>,
    checksum: u32,
    soft_skins: Vec<SkinData>,
    attachments: Vec<AttachmentData>,
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct SkinData {
    mesh: MeshData,
    weight_counts: Vec<u8>,
    weight_nodes: Vec<u8>,
    weight_values: Vec<u8>,
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct AttachmentData {
    node: String,
    mesh: MeshData,
}

/// Writes a model dict as a `.MDL`, `.MDH` or `.MDM`, by the file's extension, for `game`.
///
/// Weights are stored relative to the bones that pull them, which the nodes give, so a `.MDM`
/// needs the nodes of its skeleton too. The bounding boxes are worked out from the meshes.
#[pyfunction]
pub(crate) fn write_model(path: PathBuf, data: ModelData, game_name: &str) -> PyResult<()> {
    let game = game(game_name)?;
    let (with_hierarchy, with_mesh) = kind(&path)?;
    let hierarchy = hierarchy(&data)?;
    let world = world_transforms(&hierarchy);

    let mut mesh = ModelMesh { checksum: data.checksum, ..ModelMesh::default() };
    for (i, skin) in data.soft_skins.iter().enumerate() {
        let skin =
            soft_skin(skin, &world, game).map_err(|e| context(e, &format!("soft skin {i}")))?;
        mesh.meshes.push(skin);
    }
    for attachment in &data.attachments {
        if hierarchy.node(&attachment.node).is_none() {
            return Err(invalid(format!(
                "{:?} is attached to a node the skeleton lacks",
                attachment.node
            )));
        }
        let built = mesh::to_mrm(&attachment.mesh, game)
            .map_err(|e| context(e, &format!("attachment {:?}", attachment.node)))?;
        mesh.attachments.push(Attachment { node: attachment.node.clone(), mesh: built });
    }

    let hierarchy = ModelHierarchy { bbox: model_bounds(&hierarchy, &world, &mesh), ..hierarchy };
    let hierarchy = ModelHierarchy { collision_bbox: hierarchy.bbox, ..hierarchy };
    let hierarchy = if with_mesh { hierarchy } else { for_animation(hierarchy) };
    let bytes = match (with_hierarchy, with_mesh) {
        (true, true) => Model { hierarchy, mesh }.to_bytes(game),
        (true, false) => hierarchy.to_bytes(),
        _ => mesh.to_bytes(game),
    };
    write(&path, &bytes.map_err(fail)?)
}

fn context(error: PyErr, what: &str) -> PyErr {
    Python::attach(|py| invalid(format!("{what}: {}", error.value(py))))
}

fn hierarchy(data: &ModelData) -> PyResult<ModelHierarchy> {
    let mut nodes = Vec::with_capacity(data.nodes.len());
    for (i, (name, parent, rows)) in data.nodes.iter().enumerate() {
        let parent = match *parent {
            p if p < 0 => None,
            p if (p as usize) < i => Some(p as u16),
            _ => return Err(invalid(format!("node {name:?} comes before its parent"))),
        };
        let rows: [f32; 16] = mesh::f32s(rows)
            .try_into()
            .map_err(|_| invalid(format!("node {name:?} needs a transform of 16 floats")))?;
        nodes.push(HierarchyNode {
            name: name.clone(),
            parent,
            transform: Mat4::from_cols_array(&rows).transpose(),
        });
    }
    let root_translation = nodes.first().map_or(Vec3::ZERO, |n| n.transform.w_axis.truncate());
    Ok(ModelHierarchy {
        nodes,
        root_translation,
        checksum: data.checksum,
        ..ModelHierarchy::default()
    })
}

/// Every node's transform relative to its parent in the rest pose: roots without a position of
/// their own stand at the root translation.
fn rest_pose(hierarchy: &ModelHierarchy) -> Vec<Mat4> {
    hierarchy
        .nodes
        .iter()
        .map(|node| {
            let mut transform = node.transform;
            if node.parent.is_none() && transform.w_axis.truncate().length() < 1e-4 {
                transform.w_axis = hierarchy.root_translation.extend(1.0);
            }
            transform
        })
        .collect()
}

/// The hierarchy as skeletons made for animation are stored: roots at the origin, their
/// position kept as the root translation.
fn for_animation(mut hierarchy: ModelHierarchy) -> ModelHierarchy {
    for node in hierarchy.nodes.iter_mut().filter(|n| n.parent.is_none()) {
        node.transform.w_axis = Vec3::ZERO.extend(1.0);
    }
    hierarchy
}

/// Every node's transform in the model's space. Parents come before their children.
fn world_transforms(hierarchy: &ModelHierarchy) -> Vec<Mat4> {
    let mut world: Vec<Mat4> = Vec::with_capacity(hierarchy.nodes.len());
    for node in &hierarchy.nodes {
        let parent = node.parent.map_or(Mat4::IDENTITY, |p| world[p as usize]);
        world.push(parent * node.transform);
    }
    world
}

fn soft_skin(data: &SkinData, world: &[Mat4], game: GameVersion) -> PyResult<SoftSkinMesh> {
    let built = mesh::to_mrm(&data.mesh, game)?;
    let counts = mesh::u32s(&data.weight_counts);
    let nodes = mesh::u32s(&data.weight_nodes);
    let values = mesh::f32s(&data.weight_values);
    let total: usize = counts.iter().map(|&c| c as usize).sum();
    if counts.len() != built.positions.len() || nodes.len() != total || values.len() != total {
        return Err(invalid("every vertex needs its weights, each with a node and a value"));
    }
    if let Some(&node) = nodes.iter().find(|&&n| n as usize >= world.len()) {
        return Err(invalid(format!("a weight uses node {node}, which the skeleton lacks")));
    }

    // The bones of this mesh, in the order the weights refer to them by.
    let used: Vec<u32> = nodes.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
    if used.len() > 256 {
        return Err(invalid(format!("{} bones move the mesh; at most 256 can", used.len())));
    }
    let inverse: Vec<Mat4> = used.iter().map(|&n| world[n as usize].inverse()).collect();

    let mut weights = Vec::with_capacity(counts.len());
    let mut extents =
        vec![Aabb { min: Vec3::splat(f32::MAX), max: Vec3::splat(f32::MIN) }; used.len()];
    let mut next = 0;
    for (&count, &position) in counts.iter().zip(&built.positions) {
        let mut vertex = Vec::with_capacity(count as usize);
        for (&node, &weight) in nodes[next..].iter().zip(&values[next..]).take(count as usize) {
            let slot = used.binary_search(&node).expect("every node is in the list");
            let local = inverse[slot].transform_point3(position);
            extents[slot].min = extents[slot].min.min(local);
            extents[slot].max = extents[slot].max.max(local);
            vertex.push(Weight { weight, position: local, node: slot as u8 });
        }
        next += count as usize;
        weights.push(vertex);
    }

    let bboxes = extents
        .iter()
        .map(|e| Obb {
            center: (e.min + e.max) / 2.0,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half_width: (e.max - e.min) / 2.0,
            children: Vec::new(),
        })
        .collect();
    Ok(SoftSkinMesh {
        mesh: built,
        weights,
        wedge_normals: Vec::new(),
        nodes: used.iter().map(|&n| n as i32).collect(),
        bboxes,
    })
}

/// The box around every mesh of the model, attachments placed at their nodes.
fn model_bounds(hierarchy: &ModelHierarchy, world: &[Mat4], mesh: &ModelMesh) -> Aabb {
    let mut points: Vec<Vec3> =
        mesh.meshes.iter().flat_map(|s| s.mesh.positions.iter().copied()).collect();
    for attachment in &mesh.attachments {
        if let Some((index, _)) = hierarchy.node(&attachment.node) {
            points.extend(
                attachment.mesh.positions.iter().map(|&p| world[index].transform_point3(p)),
            );
        }
    }
    mesh::bounds(&points).0
}
