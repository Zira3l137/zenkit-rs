//! Loads a texture from the game's archives and saves it as a PNG.
//!
//! ```text
//! cargo run --example texture -- "C:\Games\Gothic II" OWODWATSTOP.TGA water.png
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use zenkit::Result;
use zenkit::texture::Texture;

fn main() -> ExitCode {
    let [gothic, name, output] = common::args("texture <gothic folder> <texture> <output.png>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &name, Path::new(&output)))
}

fn run(gothic: &Path, name: &str, output: &Path) -> Result<()> {
    let vfs = common::mount_game(gothic)?;
    let texture: Texture = vfs.load(&common::compiled_name(name))?;

    println!("format:    {:?}", texture.format);
    println!("size:      {} x {}", texture.width, texture.height);
    println!("mipmaps:   {}", texture.mipmap_count());
    let c = texture.average_color;
    println!("average:   #{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a);

    let rgba = texture.to_rgba8(0)?;
    common::save_png(output, texture.width, texture.height, &rgba)?;
    println!("saved {}", output.display());
    Ok(())
}
