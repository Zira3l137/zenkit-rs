//! Setup the examples share. Each example uses only part of it.
#![allow(dead_code)]

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::process::ExitCode;

use tracing::Level;
use zenkit::Error;
use zenkit::Result;
#[cfg(feature = "vfs")]
use zenkit::vfs::Overwrite;
#[cfg(feature = "vfs")]
use zenkit::vfs::Vfs;

/// Prints the warnings the library logs about odd data to stderr.
pub(crate) fn init_logging() {
    tracing_subscriber::fmt()
        .with_max_level(Level::WARN)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}

/// Gives the command line arguments, or prints how to call the example and exits if there are
/// not `N` of them.
pub(crate) fn args<const N: usize>(usage: &str) -> [String; N] {
    let args: Vec<String> = std::env::args().skip(1).collect();
    args.try_into().unwrap_or_else(|_| {
        eprintln!("usage: {usage}");
        std::process::exit(2);
    })
}

/// Prints an error with everything that led to it, outermost first, like
///
/// ```text
/// cannot load "CHEST.ZEN": scanning the sections: invalid ZenGin archive: expected object ...
/// ```
pub(crate) fn finish(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.report());
            ExitCode::FAILURE
        }
    }
}

/// Mounts the game's archives the way the game does: every `.vdf` in `<gothic>/Data`, letting
/// newer files win when two archives hold the same one.
#[cfg(feature = "vfs")]
pub(crate) fn mount_game(gothic: &Path) -> Result<Vfs> {
    let data = gothic.join("Data");
    let mut archives = Vec::new();
    for entry in std::fs::read_dir(&data).map_err(Error::io(&data))? {
        let path = entry.map_err(Error::io(&data))?.path();
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("vdf")) {
            archives.push(path);
        }
    }

    let mut vfs = Vfs::new();
    vfs.mount_disks(&archives, Overwrite::Older)?;
    Ok(vfs)
}

/// The name a file has in the archives. Meshes, materials and VOBs name the files assets were
/// made from, like `ORC.TGA` or `CHEST.3DS`, but the archives hold the compiled assets,
/// `ORC-C.TEX` and `CHEST.MRM`.
pub(crate) fn compiled_name(name: &str) -> String {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return name.to_owned();
    };
    let compiled = match extension.to_ascii_uppercase().as_str() {
        "TGA" => return format!("{stem}-C.TEX"),
        "3DS" => "MRM",
        "MMS" => "MMB",
        "MDS" => "MSB",
        "ASC" => "MDL",
        _ => extension,
    };
    format!("{stem}.{compiled}")
}

/// Saves 8-bit RGBA pixels as a PNG.
pub(crate) fn save_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let file = File::create(path).map_err(Error::io_write(path))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(png_error(path))?;
    writer.write_image_data(rgba).map_err(png_error(path))?;
    writer.finish().map_err(png_error(path))
}

fn png_error(path: &Path) -> impl Fn(png::EncodingError) -> Error + '_ {
    move |e| Error::io_write(path)(std::io::Error::other(e))
}
