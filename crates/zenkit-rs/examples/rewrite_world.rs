//! Reads one of the game's worlds and writes it back with the same archive header, to check the
//! world writer against the game's own files: it says whether the two are the same byte for
//! byte, and where they first differ if not. For BIN_SAFE worlds, like Gothic II's, it also
//! names the first value that differs and the objects it is in.
//!
//! ```text
//! cargo run --release --example rewrite_world -- "C:\Games\Gothic II" NEWWORLD.ZEN out.zen
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::path::Path;
use std::process::ExitCode;

use zenkit::Error;
use zenkit::Result;
use zenkit::archive::Archive;
use zenkit::archive::Format;
use zenkit::world::World;

fn main() -> ExitCode {
    let [gothic, name, output] = common::args("rewrite_world <gothic folder> <world> <output.zen>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &name, Path::new(&output)))
}

fn run(gothic: &Path, name: &str, output: &Path) -> Result<()> {
    let vfs = common::mount_game(gothic)?;
    let data = vfs.find(name).and_then(|node| node.data()).ok_or(Error::NotFound(name.into()))?;

    let header = Archive::new(data)?.header().clone();
    let format = header.format;
    println!("{name}: {format:?}, user {:?}", header.user.as_deref().unwrap_or(""));
    let world = World::from_bytes(data)?;
    let written = world.to_bytes_with(header)?;
    std::fs::write(output, &written).map_err(Error::io_write(output))?;

    let back = World::from_bytes_as(&written, world.game)?;
    println!("reads back {}", if back == world { "the same" } else { "DIFFERENT" });

    let first = written.iter().zip(data).position(|(a, b)| a != b);
    match first {
        None if written.len() == data.len() => println!("byte for byte the same"),
        None => println!("{} bytes written, the game's has {}", written.len(), data.len()),
        Some(at) => {
            println!("first difference at byte {at} of {}:", data.len());
            println!("  game    {}", around(data, at));
            println!("  written {}", around(&written, at));
        }
    }
    if format == Format::Binsafe && first.is_some() {
        compare_values(data, &written);
    }
    Ok(())
}

/// The bytes from a little before `at` to a little after it, as hex and as text.
fn around(data: &[u8], at: usize) -> String {
    let window = &data[at.saturating_sub(16)..(at + 32).min(data.len())];
    let hex: Vec<_> = window.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}\n          {}", hex.join(" "), window.escape_ascii())
}

/// A value of a BIN_SAFE archive: its name (empty for the lines that open and close objects)
/// and its type tag followed by its bytes.
struct Value {
    key: String,
    bytes: Vec<u8>,
}

impl Value {
    fn is_object_start(&self) -> bool {
        self.key.is_empty() && self.bytes.get(3) == Some(&b'[') && self.bytes.len() > 5
    }

    fn is_object_end(&self) -> bool {
        self.key.is_empty() && self.bytes.get(3..) == Some(b"[]")
    }

    fn show(&self) -> String {
        let payload = &self.bytes[1..];
        let int = |b: &[u8]| i32::from_le_bytes(b[..4].try_into().unwrap_or_default());
        let float = |b: &[u8]| f32::from_le_bytes(b[..4].try_into().unwrap_or_default());
        let text = match self.bytes[0] {
            0x01 => format!("{:?}", String::from_utf8_lossy(&payload[2..])),
            0x02 | 0x06 | 0x11 => int(payload).to_string(),
            0x03 => float(payload).to_string(),
            0x07 => format!("{} {} {}", float(payload), float(&payload[4..]), float(&payload[8..])),
            _ => payload.iter().map(|b| format!("{b:02x}")).collect(),
        };
        let text = if text.len() > 160 { format!("{}...", &text[..160]) } else { text };
        format!("{} = {text}", if self.key.is_empty() { "-" } else { &self.key })
    }
}

/// Compares two BIN_SAFE archives value by value and prints the first difference, with the
/// objects it is inside and the values just before it.
fn compare_values(game: &[u8], written: &[u8]) {
    let (Some(ours), Some(theirs)) = (binsafe_values(written), binsafe_values(game)) else {
        println!("cannot split the archives into values");
        return;
    };
    let same = |i: &usize| {
        let (a, b) = (ours.get(*i), theirs.get(*i));
        a.map(|v| (&v.key, &v.bytes)) == b.map(|v| (&v.key, &v.bytes))
    };
    let Some(at) = (0..ours.len().max(theirs.len())).find(|i| !same(i)) else {
        println!("every value is the same; only the key table differs");
        return;
    };

    println!("\nfirst differing value, {at} of {} in the game's:", theirs.len());
    let mut open = Vec::new();
    for value in &theirs[..at] {
        if value.is_object_end() {
            open.pop();
        } else if value.is_object_start() {
            open.push(value.bytes[3..].escape_ascii().to_string());
        }
    }
    for (depth, object) in open.iter().enumerate() {
        println!("  {:indent$}in {object}", "", indent = depth * 2);
    }
    println!("  values before it:");
    for value in &theirs[at.saturating_sub(4)..at] {
        println!("    {}", value.show());
    }
    let show = |v: Option<&Value>| v.map_or("(none)".into(), Value::show);
    println!("  game    {}", show(theirs.get(at)));
    println!("  written {}", show(ours.get(at)));
}

/// Splits a BIN_SAFE archive into its values, naming each from the key table.
fn binsafe_values(data: &[u8]) -> Option<Vec<Value>> {
    let u16_at =
        |at: usize| Some(usize::from(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?)));
    let u32_at =
        |at: usize| Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize);
    let start = data.windows(4).position(|w| w == b"END\n")? + 4;
    let table = u32_at(start + 8)?;

    let mut names = std::collections::HashMap::new();
    let mut at = table + 4;
    for _ in 0..u32_at(table)? {
        let (len, index) = (u16_at(at)?, u16_at(at + 2)?);
        names.insert(index, String::from_utf8_lossy(data.get(at + 8..at + 8 + len)?).into_owned());
        at += 8 + len;
    }

    let mut values = Vec::new();
    let mut at = start + 12;
    while at < table {
        let mut kind = *data.get(at)?;
        let mut key = String::new();
        if kind == 0x12 {
            key = names.get(&u32_at(at + 1)?).cloned().unwrap_or_else(|| "?".into());
            kind = *data.get(at + 5)?;
            at += 5;
        }
        let len = match kind {
            0x01 | 0x09 | 0x10 => u16_at(at + 1)? + 2,
            0x02 | 0x03 | 0x06 | 0x08 | 0x11 => 4,
            0x04 => 1,
            0x05 => 2,
            0x07 => 12,
            _ => return None,
        };
        values.push(Value { key, bytes: data.get(at..at + 1 + len)?.to_vec() });
        at += 1 + len;
    }
    Some(values)
}
