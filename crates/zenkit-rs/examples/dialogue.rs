//! Looks up lines of dialogue in the game's cutscene library: every subtitle with the name of
//! its recording.
//!
//! ```text
//! cargo run --example dialogue -- "C:\Games\Gothic II\_work\Data\Scripts\Content\Cutscene\OU.BIN" Lares
//! cargo run --example dialogue -- OU.BIN DIA_LARES_HELLO_09_00
//! ```
//!
//! The library is a plain file next to the scripts rather than inside an archive, so this reads
//! it straight from the disk. Older versions of the game ship it as `OU.CSL`, an ASCII archive,
//! which reads the same way.
#![allow(clippy::print_stdout)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use zenkit::Error;
use zenkit::Result;
use zenkit::cutscene::CutsceneLibrary;

fn main() -> ExitCode {
    let [library, search] = common::args("dialogue <OU.BIN> <line name or text to search for>");
    common::init_logging();
    common::finish(run(Path::new(&library), &search))
}

fn run(path: &Path, search: &str) -> Result<()> {
    let data = std::fs::read(path).map_err(Error::io(path))?;
    let library = CutsceneLibrary::from_bytes(&data)?;
    println!("{} lines", library.blocks.len());

    // The scripts play a line by its name, which is what `block` looks up.
    if let Some(block) = library.block(search) {
        println!("{}: {:?} ({})", block.name, block.message.text, block.message.name);
        return Ok(());
    }

    let search = search.to_lowercase();
    let found = library.blocks.iter().filter(|b| b.message.text.to_lowercase().contains(&search));
    for block in found.take(20) {
        println!("  {:<28} {}", block.name, block.message.text);
    }
    Ok(())
}
