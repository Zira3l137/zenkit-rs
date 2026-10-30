//! Packs a folder into a `.VDF` archive the game can load, the way mods are shipped.
//!
//! ```text
//! cargo run --example pack -- MyMod\_work MyMod.vdf
//! ```
//!
//! The folder's contents end up under `_WORK` inside the archive, where the game looks for them.
#![allow(clippy::print_stdout)]

mod common;

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::process::ExitCode;

use zenkit::Error;
use zenkit::GameVersion;
use zenkit::Result;
use zenkit::ResultExt;
use zenkit::vfs::DiskOptions;
use zenkit::vfs::Overwrite;
use zenkit::vfs::Vfs;

fn main() -> ExitCode {
    let [folder, output] = common::args("pack <folder> <output.vdf>");
    common::init_logging();
    common::finish(run(Path::new(&folder), Path::new(&output)))
}

fn run(folder: &Path, output: &Path) -> Result<()> {
    let mut vfs = Vfs::new();
    vfs.mkdir("/_WORK")?;
    vfs.mount_host(folder, "/_WORK", Overwrite::All)?;

    let file = File::create(output).map_err(Error::io_write(output))?;
    let options = DiskOptions::new(GameVersion::Gothic2);
    vfs.write_disk(BufWriter::new(file), &options)
        .context(|| format!("cannot write {}", output.display()))?;

    println!("packed {} files into {}", vfs.files().count(), output.display());
    Ok(())
}
