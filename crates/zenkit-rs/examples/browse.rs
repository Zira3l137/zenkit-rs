//! Mounts a game's archives and searches them for files.
//!
//! ```text
//! cargo run --example browse -- "C:\Games\Gothic II" ORC
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use zenkit::Result;

fn main() -> ExitCode {
    let [gothic, pattern] = common::args("browse <gothic folder> <part of a file name>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &pattern))
}

fn run(gothic: &Path, pattern: &str) -> Result<()> {
    let vfs = common::mount_game(gothic)?;

    let mut by_extension: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for file in vfs.files() {
        let extension = file.name().rsplit_once('.').map_or("", |(_, ext)| ext);
        let (count, size) = by_extension.entry(extension.to_owned()).or_default();
        *count += 1;
        *size += file.data().map_or(0, <[u8]>::len);
    }
    println!("{:>8} {:>12}  extension", "files", "bytes");
    for (extension, (count, size)) in &by_extension {
        println!("{count:>8} {size:>12}  .{extension}");
    }

    let pattern = pattern.to_ascii_uppercase();
    let matches: Vec<_> =
        vfs.files().filter(|f| f.name().to_ascii_uppercase().contains(&pattern)).collect();
    println!("\n{} files have {pattern:?} in their name:", matches.len());
    for file in matches.iter().take(50) {
        println!("  {:<60} {:>10} bytes", file.path(), file.data().map_or(0, <[u8]>::len));
    }
    if matches.len() > 50 {
        println!("  ...");
    }
    Ok(())
}
