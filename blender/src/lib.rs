//! Python bindings to zenkit-rs, as small as the Blender extension needs them.
//!
//! Meshes cross into Python as one shape for every format: polygons with their corners, in the
//! engine's coordinates, see [`mesh`]. Large arrays travel as `bytes` of little-endian numbers,
//! which NumPy reads without copying them element by element.

mod archives;
mod mesh;
mod model;
mod texture;

use std::path::PathBuf;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use zenkit::GameVersion;

create_exception!(zenkit_py, ZenKitError, PyException, "An error reading or writing a file.");

pub(crate) fn fail(error: zenkit::Error) -> PyErr {
    ZenKitError::new_err(error.report().to_string())
}

pub(crate) fn invalid(reason: impl Into<String>) -> PyErr {
    ZenKitError::new_err(reason.into())
}

pub(crate) fn read(path: &PathBuf) -> PyResult<Vec<u8>> {
    std::fs::read(path).map_err(|e| fail(zenkit::Error::io(path)(e)))
}

pub(crate) fn write(path: &PathBuf, data: &[u8]) -> PyResult<()> {
    std::fs::write(path, data).map_err(|e| fail(zenkit::Error::io_write(path)(e)))
}

pub(crate) fn game(name: &str) -> PyResult<GameVersion> {
    match name {
        "G1" => Ok(GameVersion::Gothic1),
        "G2" => Ok(GameVersion::Gothic2),
        _ => Err(invalid(format!("unknown game {name:?}, expected \"G1\" or \"G2\""))),
    }
}

#[pymodule]
fn zenkit_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("ZenKitError", m.py().get_type::<ZenKitError>())?;
    m.add_class::<archives::Archives>()?;
    m.add_function(wrap_pyfunction!(texture::read_texture, m)?)?;
    m.add_function(wrap_pyfunction!(texture::write_texture, m)?)?;
    m.add_function(wrap_pyfunction!(mesh::read_msh, m)?)?;
    m.add_function(wrap_pyfunction!(mesh::write_msh, m)?)?;
    m.add_function(wrap_pyfunction!(mesh::read_mrm, m)?)?;
    m.add_function(wrap_pyfunction!(mesh::write_mrm, m)?)?;
    m.add_function(wrap_pyfunction!(model::read_model, m)?)?;
    m.add_function(wrap_pyfunction!(model::write_model, m)?)?;
    Ok(())
}
