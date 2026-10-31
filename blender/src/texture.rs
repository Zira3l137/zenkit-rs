//! Textures (`.TEX`) as RGBA8 pixels.

use std::path::PathBuf;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use zenkit::texture::DxtQuality;
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

/// How many mipmap levels the game takes in a DXT texture: its renderer refuses one with a level
/// under 4 pixels on a side (zCTex_D3D::XTEX_BuildSurfaces), so the halving stops at 4×4.
fn dxt_mipmap_count(width: u32, height: u32) -> u32 {
    let smallest = width.min(height).max(1);
    (smallest.ilog2()).saturating_sub(1).max(1)
}

/// Writes RGBA8 pixels, rows top to bottom, as a texture in `format`, like `"DXT1"`, with
/// mipmap levels when `mipmaps` is set: every one, or for DXT down to 4×4, which is all the game
/// takes. DXT formats are compressed at `quality`, `"HIGH"`
/// or `"FAST"`.
#[pyfunction]
#[pyo3(signature = (path, width, height, rgba, format, mipmaps, quality = "HIGH"))]
pub(crate) fn write_texture(
    path: PathBuf,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    format: &str,
    mipmaps: bool,
    quality: &str,
) -> PyResult<()> {
    let format = match format.to_ascii_uppercase().as_str() {
        "DXT1" => TextureFormat::Dxt1,
        "DXT3" => TextureFormat::Dxt3,
        "DXT5" => TextureFormat::Dxt5,
        "B8G8R8A8" => TextureFormat::B8G8R8A8,
        "R5G6B5" => TextureFormat::R5G6B5,
        other => return Err(invalid(format!("unsupported texture format {other:?}"))),
    };
    let quality = match quality.to_ascii_uppercase().as_str() {
        "HIGH" => DxtQuality::High,
        "FAST" => DxtQuality::Fast,
        other => return Err(invalid(format!("unknown quality {other:?}; expected HIGH or FAST"))),
    };
    let levels = match (mipmaps, format) {
        (false, _) => 1,
        (true, TextureFormat::Dxt1 | TextureFormat::Dxt3 | TextureFormat::Dxt5) => {
            dxt_mipmap_count(width, height)
        }
        (true, _) => max_mipmap_count(width, height),
    };
    let texture =
        Texture::from_rgba8_with(width, height, &rgba, format, levels, quality).map_err(fail)?;
    write(&path, &texture.to_bytes())
}
