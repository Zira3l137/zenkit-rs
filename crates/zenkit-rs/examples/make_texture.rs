//! Turns a PNG into a game texture (`.TEX`) with mipmaps, ready to put into an archive.
//!
//! ```text
//! cargo run --release --example make_texture -- grass.png GRASS-C.TEX
//! ```
//!
//! Images with transparency become DXT5, others DXT1, which is what the game's own textures use.
#![allow(clippy::print_stdout)]

mod common;

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::ExitCode;

use zenkit::Error;
use zenkit::Result;
use zenkit::texture::Texture;
use zenkit::texture::TextureFormat;
use zenkit::texture::max_mipmap_count;

fn main() -> ExitCode {
    let [input, output] = common::args("make_texture <input.png> <output.tex>");
    common::init_logging();
    common::finish(run(Path::new(&input), Path::new(&output)))
}

fn run(input: &Path, output: &Path) -> Result<()> {
    let (width, height, rgba) = load_png(input)?;

    let transparent = rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
    let format = if transparent { TextureFormat::Dxt5 } else { TextureFormat::Dxt1 };
    let texture =
        Texture::from_rgba8(width, height, &rgba, format, max_mipmap_count(width, height))?;

    std::fs::write(output, texture.to_bytes()).map_err(Error::io_write(output))?;
    println!("{width}x{height} {format:?} with {} mipmaps", texture.mipmap_count());
    Ok(())
}

/// Reads a PNG as 8-bit RGBA, whatever colour type it was saved with.
fn load_png(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let fail = |e: png::DecodingError| Error::io(path)(std::io::Error::other(e));
    let file = File::open(path).map_err(Error::io(path))?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().map_err(fail)?;
    let mut pixels = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut pixels).map_err(fail)?;
    pixels.truncate(info.buffer_size());

    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::GrayscaleAlpha => {
            pixels.as_chunks::<2>().0.iter().flat_map(|&[v, a]| [v, v, v, a]).collect()
        }
        other => {
            return Err(Error::io(path)(std::io::Error::other(format!("unexpected {other:?}"))));
        }
    };
    Ok((info.width, info.height, rgba))
}
