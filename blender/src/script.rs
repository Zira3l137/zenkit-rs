//! Model scripts (`.MDS` source, `.MSB` compiled), which name a model's skeleton, the meshes it
//! can wear and its animations. Python edits them as source text, and gets what it needs to find
//! the files they name as a dict:
//!
//! ```text
//! name              the model's name, which its animations' files start with: HUMANS-S_RUN.MAN
//! text              the script as source, or None if it holds statements without a source form
//! text_error        why there is no text, or None
//! skeleton          the file of the skeleton (meshAndTree), or None
//! disable_mesh      whether the skeleton's own mesh is left out (DONT_USE_MESH)
//! meshes            the meshes that can be put on the skeleton (registerMesh)
//! animations        list of {"name", "layer", "next", "blend_in", "blend_out", "flags",
//!                   "model", "direction", "first_frame", "last_frame", "fps", "speed",
//!                   "collision_volume_scale"}, one per `ani`, which each have a `.MAN` file
//! ```
//!
//! [`format_animation`] writes one `ani` statement, for Blender to put into a script's text.

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyDict;
use zenkit::Date;
use zenkit::model_script::Animation;
use zenkit::model_script::AnimationFlags;
use zenkit::model_script::Direction;
use zenkit::model_script::ModelScript;
use zenkit::model_script::ScriptSource;
use zenkit::model_script::Statement;

use crate::fail;
use crate::invalid;
use crate::read;
use crate::write;

/// Reads a model script, either form, as a script dict. A compiled script without the path of
/// its source is named after its file, as the engine names models.
#[pyfunction]
pub(crate) fn read_script(py: Python<'_>, path: PathBuf) -> PyResult<Bound<'_, PyDict>> {
    let mut script = ModelScript::from_bytes(&read(&path)?).map_err(fail)?;
    if script.name.is_empty() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        script.name = stem.to_ascii_uppercase();
    }
    to_dict(py, &script)
}

/// Parses a model script's source text as a script dict.
#[pyfunction]
pub(crate) fn parse_script<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyDict>> {
    to_dict(py, &parse(text)?)
}

/// Writes a model script's source text as a `.MDS` or `.MSB` file, by the file's extension.
///
/// A compiled script keeps its name only in the path of its source, so it is given the path the
/// game's scripts have: `\_WORK\DATA\ANIMS\HUMANS.MDS`.
#[pyfunction]
pub(crate) fn write_script(path: PathBuf, text: &str) -> PyResult<()> {
    let mut script = parse(text)?;
    let source = format!(r"\_WORK\DATA\ANIMS\{}.MDS", script.name.to_ascii_uppercase());
    script.source = Some(ScriptSource { path: source, date: Date::default() });
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let bytes = match extension.to_ascii_uppercase().as_str() {
        "MDS" => script.to_mds(),
        "MSB" => script.to_msb(),
        other => {
            return Err(invalid(format!("{other:?} is not a model script; expected MDS or MSB")));
        }
    };
    write(&path, &bytes.map_err(fail)?)
}

#[derive(FromPyObject)]
#[pyo3(from_item_all)]
pub(crate) struct AnimationLine {
    name: String,
    layer: u32,
    next: String,
    blend_in: f32,
    blend_out: f32,
    /// The flag letters, like `MI`.
    flags: String,
    /// How the script spelled them, kept if it still means the same flags, like `M.`.
    flags_text: String,
    model: String,
    /// `F` or `R`.
    direction: String,
    first_frame: i32,
    last_frame: i32,
    fps: f32,
    speed: f32,
    collision_volume_scale: f32,
}

/// One `ani` statement as source, without its events, laid out as [`ModelScript::to_mds`] lays
/// out a script's: `ani ("S_RUN" 1 "S_RUN" 0.1 0.1 M. "HUM_RUN.ASC" F 1 20 FPS:10)`.
#[pyfunction]
pub(crate) fn format_animation(line: AnimationLine) -> PyResult<String> {
    let flags = flags_of(&line.flags);
    let flags_text =
        if flags_of(&line.flags_text) == flags { line.flags_text } else { String::new() };
    let direction = if line.direction.eq_ignore_ascii_case("R") {
        Direction::Backward
    } else {
        Direction::Forward
    };
    let anim = Animation {
        name: line.name,
        layer: line.layer,
        next: line.next,
        blend_in: line.blend_in,
        blend_out: line.blend_out,
        flags,
        flags_text,
        model: line.model,
        direction,
        first_frame: line.first_frame,
        last_frame: line.last_frame,
        fps: line.fps,
        speed: line.speed,
        collision_volume_scale: line.collision_volume_scale,
        events: Vec::new(),
    };
    let script = ModelScript {
        name: "LINE".into(),
        statements: vec![Statement::Animation(anim)],
        ..ModelScript::default()
    };
    let text = script.to_mds().map_err(fail)?;
    let text: String = text.iter().map(|&b| char::from(b)).collect();
    let is_ani = |l: &&str| l.strip_prefix("ani").is_some_and(|r| r.starts_with([' ', '(']));
    let line = text.lines().map(str::trim).find(is_ani);
    line.map(str::to_owned).ok_or_else(|| invalid("the animation has no source form"))
}

fn flags_of(letters: &str) -> AnimationFlags {
    let flag = |c: char| match c.to_ascii_uppercase() {
        'M' => AnimationFlags::MOVE,
        'R' => AnimationFlags::ROTATE,
        'E' => AnimationFlags::QUEUE,
        'F' => AnimationFlags::FLY,
        'I' => AnimationFlags::IDLE,
        'P' => AnimationFlags::IN_PLACE,
        _ => AnimationFlags::empty(),
    };
    letters.chars().map(flag).collect()
}

fn parse(text: &str) -> PyResult<ModelScript> {
    // Scripts are Latin-1, which Python's text has been decoded from.
    let bytes = (text.chars())
        .map(|c| u8::try_from(u32::from(c)))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| invalid("model scripts can only hold Latin-1 characters"))?;
    let script = ModelScript::from_bytes(&bytes).map_err(fail)?;
    if script.name.is_empty() {
        return Err(invalid("the script has no Model(\"…\") block"));
    }
    Ok(script)
}

fn to_dict<'py>(py: Python<'py>, script: &ModelScript) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("name", &script.name)?;
    match script.to_mds() {
        Ok(text) => {
            let text: String = text.iter().map(|&b| char::from(b)).collect();
            dict.set_item("text", text)?;
            dict.set_item("text_error", py.None())?;
        }
        Err(error) => {
            dict.set_item("text", py.None())?;
            dict.set_item("text_error", error.report().to_string())?;
        }
    }
    dict.set_item("skeleton", script.skeleton.as_ref().map(|s| &s.name))?;
    dict.set_item("disable_mesh", script.skeleton.as_ref().is_some_and(|s| s.disable_mesh))?;
    dict.set_item("meshes", &script.meshes)?;

    let animations = script.animations().map(|a| {
        let anim = PyDict::new(py);
        anim.set_item("name", &a.name)?;
        anim.set_item("layer", a.layer)?;
        anim.set_item("next", &a.next)?;
        anim.set_item("blend_in", a.blend_in)?;
        anim.set_item("blend_out", a.blend_out)?;
        anim.set_item("flags", &a.flags_text)?;
        anim.set_item("model", &a.model)?;
        anim.set_item("direction", if a.direction == Direction::Backward { "R" } else { "F" })?;
        anim.set_item("first_frame", a.first_frame)?;
        anim.set_item("last_frame", a.last_frame)?;
        anim.set_item("fps", a.fps)?;
        anim.set_item("speed", a.speed)?;
        anim.set_item("collision_volume_scale", a.collision_volume_scale)?;
        Ok(anim)
    });
    dict.set_item("animations", animations.collect::<PyResult<Vec<_>>>()?)?;
    Ok(dict)
}
