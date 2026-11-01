//! Reads every file of one kind in the game's archives, writes each back, and compares the result
//! with the original, to check a writer against all of the game's own files at once.
//!
//! ```text
//! cargo run --release --example rewrite_all -- "C:\Games\Gothic II" MAN
//! ```
//!
//! Takes `MAN`, `MMB`, `MRM`, `MDH`, `MDM` or `MDL`. Files are written for the game whose folder
//! it is, told by whether `System\Gothic2.exe` exists. It prints how many files come out byte for
//! byte the same, then groups the others by the chunks their differences lie in, with the first
//! difference of one file of each group.
#![allow(clippy::print_stdout)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use zenkit::Error;
use zenkit::GameVersion;
use zenkit::Result;
use zenkit::animation::ModelAnimation;
use zenkit::model::Model;
use zenkit::model::ModelHierarchy;
use zenkit::model::ModelMesh;
use zenkit::morph::MorphMesh;
use zenkit::mrm::MultiResolutionMesh;

fn main() -> ExitCode {
    let [gothic, kind] = common::args("rewrite_all <gothic folder> <MAN|MMB|MRM|MDH|MDM|MDL>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &kind.to_ascii_uppercase()))
}

/// What became of one file.
enum Outcome {
    Same,
    /// The written bytes, and whether reading them gives what reading the original gave.
    Differs(Vec<u8>, bool),
    Unreadable(Error),
    Unwritable(Error),
}

/// Reads a file, writes it back and compares.
fn rewrite<T: PartialEq>(
    data: &[u8],
    read: impl Fn(&[u8]) -> Result<T>,
    write: impl Fn(&T) -> Result<Vec<u8>>,
) -> Outcome {
    let value = match read(data) {
        Ok(value) => value,
        Err(error) => return Outcome::Unreadable(error),
    };
    match write(&value) {
        Err(error) => Outcome::Unwritable(error),
        Ok(written) if written == data => Outcome::Same,
        Ok(written) => {
            let same = read(&written).is_ok_and(|back| back == value);
            Outcome::Differs(written, same)
        }
    }
}

fn run(gothic: &Path, kind: &str) -> Result<()> {
    let game = if gothic.join("System").join("Gothic2.exe").exists() {
        GameVersion::Gothic2
    } else {
        GameVersion::Gothic1
    };
    let check: fn(&[u8], GameVersion) -> Outcome = match kind {
        "MAN" => |d, _| rewrite(d, ModelAnimation::from_bytes, ModelAnimation::to_bytes),
        "MMB" => |d, game| rewrite(d, MorphMesh::from_bytes, |m| m.to_bytes(game)),
        "MRM" => |d, game| rewrite(d, MultiResolutionMesh::from_bytes, |m| m.to_bytes(game)),
        "MDH" => |d, _| rewrite(d, ModelHierarchy::from_bytes, ModelHierarchy::to_bytes),
        "MDM" => |d, game| rewrite(d, ModelMesh::from_bytes, |m| m.to_bytes(game)),
        "MDL" => |d, game| rewrite(d, Model::from_bytes, |m| m.to_bytes(game)),
        other => {
            let reason = format!("cannot rewrite {other:?} files");
            return Err(Error::unsupported("rewrite_all", reason));
        }
    };

    let vfs = common::mount_game(gothic)?;
    let suffix = format!(".{kind}");
    let files: Vec<_> = vfs
        .files()
        .filter(|node| node.name().to_ascii_uppercase().ends_with(&suffix))
        .filter_map(|node| Some((node.name(), node.data()?)))
        .collect();
    println!("{} {kind} files, written for {game:?}", files.len());

    // Checked on every core, each thread taking the next file until none are left.
    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::with_capacity(files.len()));
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while let Some(&(name, data)) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = check(data, game);
                    outcomes.lock().unwrap().push((name, data, outcome));
                }
            });
        }
    });
    let mut outcomes = outcomes.into_inner().unwrap();
    outcomes.sort_by_key(|&(name, ..)| name);

    report(&outcomes);
    Ok(())
}

/// A group of files whose differences lie in the same chunks.
#[derive(Default)]
struct Group<'a> {
    files: usize,
    read_back_same: usize,
    most_bytes: usize,
    first: Option<(&'a str, &'a [u8], &'a [u8])>,
}

fn report<'a>(outcomes: &'a [(&'a str, &'a [u8], Outcome)]) {
    let count = |f: fn(&Outcome) -> bool| outcomes.iter().filter(|(_, _, o)| f(o)).count();
    println!(
        "{} byte for byte the same, {} differ, {} unreadable, {} unwritable",
        count(|o| matches!(o, Outcome::Same)),
        count(|o| matches!(o, Outcome::Differs(..))),
        count(|o| matches!(o, Outcome::Unreadable(_))),
        count(|o| matches!(o, Outcome::Unwritable(_))),
    );

    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for (name, data, outcome) in outcomes {
        let Outcome::Differs(written, read_back_same) = outcome else { continue };
        let differing: Vec<usize> =
            (0..data.len().min(written.len())).filter(|&i| data[i] != written[i]).collect();
        let mut chunks: Vec<String> = differing.iter().map(|&i| chunk_at(data, i)).collect();
        chunks.dedup();
        chunks.sort();
        chunks.dedup();
        if data.len() != written.len() {
            chunks.push(format!("length {} vs {}", data.len(), written.len()));
        }
        let group = groups.entry(chunks.join(" ")).or_default();
        group.files += 1;
        group.read_back_same += usize::from(*read_back_same);
        group.most_bytes = group.most_bytes.max(differing.len());
        group.first.get_or_insert((name, data, written));
    }
    if !groups.is_empty() {
        println!("\nfiles that differ, by the chunks the differences lie in:");
    }
    for (chunks, group) in &groups {
        println!(
            "\n  {} files in {chunks}: up to {} bytes, {} read back the same",
            group.files, group.most_bytes, group.read_back_same
        );
        if let Some((name, data, written)) = group.first {
            let at = data.iter().zip(written).position(|(a, b)| a != b);
            let at = at.unwrap_or(data.len().min(written.len()));
            println!("  first is {name}, at byte {at}:");
            println!("    game    {}", around(data, at));
            println!("    written {}", around(written, at));
        }
    }

    for (what, pick) in [
        (
            "unreadable",
            (|o: &Outcome| match o {
                Outcome::Unreadable(e) => Some(e),
                _ => None,
            }) as fn(&Outcome) -> Option<&Error>,
        ),
        ("unwritable", |o| match o {
            Outcome::Unwritable(e) => Some(e),
            _ => None,
        }),
    ] {
        let failed: Vec<_> =
            outcomes.iter().filter_map(|(name, _, o)| Some((name, pick(o)?))).collect();
        if !failed.is_empty() {
            println!("\n{what}, the first five:");
        }
        for (name, error) in failed.iter().take(5) {
            println!("  {name}: {}", error.report());
        }
    }
}

/// The id of the top-level chunk that byte `at` lies in, like `0xa010`.
fn chunk_at(data: &[u8], at: usize) -> String {
    let mut start = 0;
    while start + 6 <= data.len() {
        let id = u16::from_le_bytes([data[start], data[start + 1]]);
        let len = u32::from_le_bytes(data[start + 2..start + 6].try_into().unwrap()) as usize;
        let end = start.saturating_add(6).saturating_add(len);
        if at < end {
            return format!("{id:#06x}");
        }
        start = end;
    }
    "the end".into()
}

/// The bytes from a little before `at` to a little after it, as hex and as text.
fn around(data: &[u8], at: usize) -> String {
    let window = &data[at.saturating_sub(16).min(data.len())..(at + 32).min(data.len())];
    let hex: Vec<_> = window.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}\n            {}", hex.join(" "), window.escape_ascii())
}
