//! Textures (`.TEX`) as RGBA8 pixels.

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use zenkit::texture::Texture;
use zenkit::texture::TextureFormat;
use zenkit::texture::max_mipmap_count;

use crate::fail;
use crate::invalid;
use crate::read;
use crate::write;

/// A texture's largest mipmap as `(width, height, rgba)`, rows top to bottom.
pub(crate) fn decode<'py>(
    py: Python<'py>,
    data: &[u8],
) -> PyResult<(u32, u32, Bound<'py, PyBytes>)> {
    let texture = Texture::from_bytes(data).map_err(fail)?;
    let rgba = texture.to_rgba8(0).map_err(fail)?;
    Ok((texture.width, texture.height, PyBytes::new(py, &rgba)))
}

/// Reads the largest mipmap of a texture as `(width, height, rgba)`, rows top to bottom.
#[pyfunction]
pub(crate) fn read_texture(
    py: Python<'_>,
    path: PathBuf,
) -> PyResult<(u32, u32, Bound<'_, PyBytes>)> {
    decode(py, &read(&path)?)
}

/// Writes RGBA8 pixels, rows top to bottom, as a texture in `format`, like `"DXT1"`, with
/// every mipmap level when `mipmaps` is set.
#[pyfunction]
pub(crate) fn write_texture(
    path: PathBuf,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    format: &str,
    mipmaps: bool,
) -> PyResult<()> {
    let format = match format.to_ascii_uppercase().as_str() {
        "DXT1" => TextureFormat::Dxt1,
        "DXT3" => TextureFormat::Dxt3,
        "DXT5" => TextureFormat::Dxt5,
        "B8G8R8A8" => TextureFormat::B8G8R8A8,
        "R5G6B5" => TextureFormat::R5G6B5,
        other => return Err(invalid(format!("unsupported texture format {other:?}"))),
    };
    let levels = if mipmaps { max_mipmap_count(width, height) } else { 1 };
    let texture = Texture::from_rgba8(width, height, &rgba, format, levels).map_err(fail)?;
    write(&path, &texture.to_bytes())
}
