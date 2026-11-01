//! Morph meshes (`.MMB`), meshes whose vertices animations move, like faces, as one dict:
//!
//! ```text
//! name              the mesh's name
//! mesh              mesh dict of triangles, as for `.MRM`
//! morph_positions   the positions the animations move, 3 f32 per position of the mesh;
//!                   the mesh's own positions when left out on writing
//! animations        list of animation dicts, below
//! sources           list of the source files it was compiled from, for the record
//! ```
//!
//! An animation dict:
//!
//! ```text
//! name              like VISEME or R_EYESBLINK
//! layer             i32
//! blend_in, blend_out, speed
//!                   f32
//! duration          f32, in milliseconds
//! flags             u8
//! vertices          the indices of the positions it moves, u32 each
//! samples           per frame, per entry of vertices, the offset from its morph position, 3 f32
//! frame_count       read only: how many frames the samples hold
//! ```

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::types::PyDict;
use zenkit::Date;
use zenkit::morph::MorphAnimation;
use zenkit::morph::MorphMesh;
use zenkit::morph::MorphSource;

use crate::fail;
use crate::game;
use crate::invalid;
use crate::mesh;
use crate::mesh::MeshData;
use crate::read;
use crate::write;

/// Reads a morph mesh (`.MMB`) as a morph dict.
#[pyfunction]
pub(crate) fn read_morph(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let morph = MorphMesh::from_bytes(&read(&path)?).map_err(fail)?;
    let dict = PyDict::new(py);
    dict.set_item("name", &morph.name)?;
    dict.set_item("mesh", mesh::from_mrm(&morph.mesh)?.into_dict(py)?)?;
    let positions: Vec<f32> = morph.morph_positions.iter().flat_map(|p| p.to_array()).collect();
    dict.set_item(
        "morph_positions",
        PyBytes::new(py, &mesh::le_bytes(&positions, f32::to_le_bytes)),
    )?;

    let animations = morph.animations.iter().map(|a| {
        let anim = PyDict::new(py);
        anim.set_item("name", &a.name)?;
        anim.set_item("layer", a.layer)?;
        anim.set_item("blend_in", a.blend_in)?;
        anim.set_item("blend_out", a.blend_out)?;
        anim.set_item("duration", a.duration)?;
        anim.set_item("speed", a.speed)?;
        anim.set_item("flags", a.flags)?;
        anim.set_item("frame_count", a.frame_count)?;
        anim.set_item(
            "vertices",
            PyBytes::new(py, &mesh::le_bytes(&a.vertices, u32::to_le_bytes)),
        )?;
        let samples: Vec<f32> = a.samples.iter().flat_map(|s| s.to_array()).collect();
        anim.set_item("samples", PyBytes::new(py, &mesh::le_bytes(&samples, f32::to_le_bytes)))?;
        Ok(anim)
    });
    dict.set_item("animations", animations.collect::<PyResult<Vec<_>>>()?)?;
    let sources: Vec<_> = morph.sources.iter().map(|s| s.file_name.as_str()).collect();
    dict.set_item("sources", sources)?;
    Ok(dict)
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
pub(crate) struct MorphData {
    name: String,
    mesh: MeshData,
    #[pyo3(default)]
    morph_positions: Option<Vec<u8>>,
    animations: Vec<AnimationData>,
    #[pyo3(default)]
    sources: Vec<String>,
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct AnimationData {
    name: String,
    layer: i32,
    blend_in: f32,
    blend_out: f32,
    duration: f32,
    speed: f32,
    flags: u8,
    vertices: Vec<u8>,
    samples: Vec<u8>,
}

/// Writes a morph dict as a `.MMB` file for `game`, its mesh as [`write_mrm`](crate::mesh)
/// writes one.
#[pyfunction]
pub(crate) fn write_morph(path: PathBuf, data: MorphData, game_name: &str) -> PyResult<()> {
    let game = game(game_name)?;
    let mesh = mesh::to_mrm(&data.mesh, game)?;
    let morph_positions = match &data.morph_positions {
        Some(bytes) => mesh::vec3s(bytes),
        None => mesh.positions.clone(),
    };
    let animations = (data.animations.into_iter())
        .map(|a| {
            let vertices = mesh::u32s(&a.vertices);
            let samples = mesh::vec3s(&a.samples);
            if vertices.iter().any(|&v| v as usize >= mesh.positions.len()) {
                return Err(invalid(format!(
                    "animation {:?} moves a vertex that does not exist",
                    a.name
                )));
            }
            let frame_count = match vertices.len() {
                0 => 0,
                n if samples.len().is_multiple_of(n) => (samples.len() / n) as u32,
                _ => {
                    return Err(invalid(format!(
                        "animation {:?} needs one sample per vertex in every frame",
                        a.name
                    )));
                }
            };
            Ok(MorphAnimation {
                name: a.name,
                layer: a.layer,
                blend_in: a.blend_in,
                blend_out: a.blend_out,
                duration: a.duration,
                speed: a.speed,
                flags: a.flags,
                frame_count,
                vertices,
                samples,
            })
        })
        .collect::<PyResult<_>>()?;
    let sources = (data.sources.into_iter())
        .map(|file_name| MorphSource { date: Date::default(), file_name })
        .collect();

    let morph = MorphMesh { name: data.name, mesh, morph_positions, animations, sources };
    write(&path, &morph.to_bytes(game).map_err(fail)?)
}
