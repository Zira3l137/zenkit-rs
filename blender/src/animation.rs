//! Animations (`.MAN`) as one dict:
//!
//! ```text
//! name, next        the animation's name and the one played after it
//! layer             the layer it plays on (u32)
//! fps, fps_source   the frame rate it plays at, and the one its source was made at
//! checksum          the checksum of the skeleton it was made for: the model dict's
//! node_indices      list of the skeleton's node indices the samples are for
//! frame_count       read only: how many frames the transforms hold
//! transforms        per frame, per entry of node_indices, the node's transform relative to its
//!                   parent, 16 f32 row by row, like the model dict's nodes
//! bbox              ((x, y, z), (x, y, z)), the space the animation moves through
//! events            list of {"kind", "frame", "tag", "content", "values", "probability"}
//! source_path, source_script
//!                   the source file and script it was compiled from, for the record
//! ```
//!
//! A root's transform carries the whole of its position, which the skeleton keeps partly apart as
//! its root translation: the model dict's transforms are made the same way, so the two fit.
//!
//! When writing, `events`, `fps_source`, `source_path` and `source_script` may be left out, and
//! the samples' positions are packed into the range they need.

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::types::PyDict;
use zenkit::animation::AnimationEvent;
use zenkit::animation::AnimationEventType;
use zenkit::animation::AnimationSample;
use zenkit::animation::ModelAnimation;
use zenkit::math::Aabb;
use zenkit::math::Mat4;
use zenkit::math::Vec3;

use crate::fail;
use crate::invalid;
use crate::mesh;
use crate::read;
use crate::write;

/// Reads an animation (`.MAN`) as an animation dict.
#[pyfunction]
pub(crate) fn read_animation(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    to_dict(py, &read(&path)?)
}

pub(crate) fn to_dict<'py>(py: Python<'py>, data: &[u8]) -> PyResult<Bound<'py, PyDict>> {
    let anim = ModelAnimation::from_bytes(data).map_err(fail)?;
    let dict = PyDict::new(py);
    dict.set_item("name", &anim.name)?;
    dict.set_item("next", &anim.next)?;
    dict.set_item("layer", anim.layer)?;
    dict.set_item("fps", anim.fps)?;
    dict.set_item("fps_source", anim.fps_source)?;
    dict.set_item("checksum", anim.checksum)?;
    dict.set_item("node_indices", &anim.node_indices)?;
    dict.set_item("frame_count", anim.frame_count)?;

    let rows: Vec<f32> =
        anim.samples.iter().flat_map(|s| to_transform(s).transpose().to_cols_array()).collect();
    dict.set_item("transforms", PyBytes::new(py, &mesh::le_bytes(&rows, f32::to_le_bytes)))?;
    dict.set_item("bbox", (anim.bbox.min.to_array(), anim.bbox.max.to_array()))?;

    let events = anim.events.iter().map(|e| {
        let event = PyDict::new(py);
        event.set_item("kind", e.kind.raw())?;
        event.set_item("frame", e.frame)?;
        event.set_item("tag", &e.tag)?;
        event.set_item("content", e.content.to_vec())?;
        event.set_item("values", e.values.to_vec())?;
        event.set_item("probability", e.probability)?;
        Ok(event)
    });
    dict.set_item("events", events.collect::<PyResult<Vec<_>>>()?)?;
    dict.set_item("source_path", &anim.source_path)?;
    dict.set_item("source_script", &anim.source_script)?;
    Ok(dict)
}

/// The engine stores rotations the other way around from glam's: its matrices are built from the
/// quaternion's conjugate, as OpenGothic's `mkMatrix` shows.
fn to_transform(sample: &AnimationSample) -> Mat4 {
    Mat4::from_rotation_translation(sample.rotation.conjugate(), sample.position)
}

fn to_sample(transform: Mat4) -> AnimationSample {
    let (_, rotation, position) = transform.to_scale_rotation_translation();
    AnimationSample { position, rotation: rotation.normalize().conjugate() }
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
pub(crate) struct AnimationData {
    name: String,
    next: String,
    layer: u32,
    fps: f32,
    #[pyo3(default)]
    fps_source: Option<f32>,
    checksum: u32,
    node_indices: Vec<u32>,
    transforms: Vec<u8>,
    bbox: ([f32; 3], [f32; 3]),
    #[pyo3(default)]
    events: Vec<EventData>,
    #[pyo3(default)]
    source_path: String,
    #[pyo3(default)]
    source_script: String,
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
struct EventData {
    kind: u32,
    frame: u32,
    tag: String,
    content: [String; 4],
    values: [f32; 4],
    probability: f32,
}

/// Writes an animation dict as a `.MAN` file.
#[pyfunction]
pub(crate) fn write_animation(path: PathBuf, data: AnimationData) -> PyResult<()> {
    let rows = mesh::f32s(&data.transforms);
    let (transforms, rest) = rows.as_chunks::<16>();
    let nodes = data.node_indices.len();
    if !rest.is_empty() || nodes == 0 || !transforms.len().is_multiple_of(nodes) {
        return Err(invalid(format!(
            "the transforms must be 16 floats for each of the {nodes} nodes in every frame"
        )));
    }
    let samples: Vec<_> =
        transforms.iter().map(|rows| to_sample(Mat4::from_cols_array(rows).transpose())).collect();
    let (min, max) = data.bbox;
    let bbox = Aabb { min: Vec3::from_array(min), max: Vec3::from_array(max) };

    let mut anim = ModelAnimation {
        name: data.name,
        next: data.next,
        layer: data.layer,
        frame_count: u32::try_from(samples.len() / nodes)
            .map_err(|_| invalid("too many frames"))?,
        node_count: nodes as u32,
        fps: data.fps,
        fps_source: data.fps_source.unwrap_or(data.fps),
        bbox,
        checksum: data.checksum,
        source_path: data.source_path,
        source_script: data.source_script,
        samples,
        events: data.events.into_iter().map(event).collect(),
        node_indices: data.node_indices,
        ..ModelAnimation::default()
    };
    anim.fit_sample_positions();
    write(&path, &anim.to_bytes().map_err(fail)?)
}

fn event(e: EventData) -> AnimationEvent {
    AnimationEvent {
        kind: AnimationEventType::from_raw(e.kind),
        frame: e.frame,
        tag: e.tag,
        content: e.content,
        values: e.values,
        probability: e.probability,
    }
}
