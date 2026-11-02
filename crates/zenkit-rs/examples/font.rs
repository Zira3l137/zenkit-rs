//! Writes a line of text with one of the game's fonts and saves it as a PNG: the font says where
//! each character is in its texture, and the characters are copied from there side by side.
//!
//! ```text
//! cargo run --example font -- "C:\Games\Gothic II" FONT_OLD_20_WHITE_HI.FNT "Hallo, Fremder!" text.png
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use zenkit::Result;
use zenkit::font::Font;
use zenkit::texture::Texture;

fn main() -> ExitCode {
    let [gothic, name, text, output] =
        common::args("font <gothic folder> <font.FNT> <text> <output.png>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &name, &text, Path::new(&output)))
}

fn run(gothic: &Path, name: &str, text: &str, output: &Path) -> Result<()> {
    let vfs = common::mount_game(gothic)?;
    let font: Font = vfs.load(name)?;
    let texture: Texture = vfs.load(&common::compiled_name(&font.name))?;
    let (tw, th) = (texture.width as usize, texture.height as usize);
    let pixels = texture.to_rgba8(0)?;
    println!(
        "{name}: {} glyphs, {} px high, texture {} ({tw} x {th})",
        font.glyphs.len(),
        font.height,
        font.name
    );

    let glyphs: Vec<_> = text.chars().filter_map(|c| font.glyph(c)).collect();
    let width = glyphs.iter().map(|g| usize::from(g.width)).sum::<usize>().max(1);
    let height = font.height as usize;
    let mut image = vec![0u8; width * height * 4];

    let mut x = 0;
    for glyph in glyphs {
        let (left, top, _, _) = glyph.pixels(texture.width, texture.height);
        let (left, top) = (left.round() as usize, top.round() as usize);
        for row in 0..height.min(th.saturating_sub(top)) {
            for column in 0..usize::from(glyph.width).min(tw.saturating_sub(left)) {
                let from = ((top + row) * tw + left + column) * 4;
                let to = (row * width + x + column) * 4;
                image[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
            }
        }
        x += usize::from(glyph.width);
    }

    common::save_png(output, width as u32, height as u32, &image)?;
    println!("saved {}", output.display());
    Ok(())
}
