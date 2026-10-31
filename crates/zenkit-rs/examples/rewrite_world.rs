//! Reads one of the game's worlds and writes it back with the same archive header, to check the
//! world writer against the game's own files: it says whether the two are the same byte for
//! byte, and where they first differ if not. For BIN_SAFE worlds, like Gothic II's, it also
//! compares them value by value and lists what differs and where.
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

/// The made-up type tag of the raw mesh and BSP tree of a world, which count as one value.
const GEOMETRY: u8 = 0xFF;

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
            GEOMETRY => format!("{} bytes of mesh and BSP tree", payload.len()),
            _ => payload.iter().map(|b| format!("{b:02x}")).collect(),
        };
        let short: String = text.chars().take(160).collect();
        let text = if short.len() < text.len() { format!("{short}...") } else { text };
        format!("{} = {text}", if self.key.is_empty() { "-" } else { &self.key })
    }

    /// What the group of this value is called in the summary.
    fn label(&self) -> &str {
        match self.key.as_str() {
            "" if self.is_object_end() => "end of object",
            "" => "object",
            key => key.trim_end_matches(|c: char| c.is_ascii_digit()),
        }
    }
}

/// Whether two values are the same, leaving out what the writer cannot or need not match:
///
/// - object indices, so that one object more or less does not make every later one differ;
/// - the flag bits of packed VOBs that nothing reads in a world (Gothic 1's bits 5 and 7,
///   Gothic II's bit 7), and the camera alignment 3, which is no alignment and reads as none;
/// - the padding of the mesh's date, which the engine leaves uninitialised.
fn same(game: &Value, ours: &Value) -> bool {
    if game.key != ours.key || game.bytes.len() != ours.bytes.len() {
        return false;
    }
    let (mut a, mut b) = (game.bytes.clone(), ours.bytes.clone());
    if game.key.is_empty() {
        return without_index(&a) == without_index(&b);
    }
    match (game.key.as_str(), a.len()) {
        // The type tag and length, then 74 bytes in Gothic 1 and 83 in Gothic II.
        ("dataRaw", 77 | 86) => {
            let unread = if a.len() == 77 { 0b1010_0000 } else { 0b1000_0000 };
            a[3 + 73] &= !unread;
            b[3 + 73] &= !unread;
            if a[3 + 72] >> 1 & 0b11 == 3 {
                a[3 + 72] &= !0b110;
            }
        }
        // The date's padding, in the mesh's header chunk, after its version.
        ("MeshAndBsp", len) if len > 33 && a[9..11] == [0x00, 0xB0] => {
            a[31..33].fill(0);
            b[31..33].fill(0);
        }
        _ => {}
    }
    a == b
}

/// An object line up to its last space, which leaves out the object index.
fn without_index(line: &[u8]) -> &[u8] {
    line.iter().rposition(|&b| b == b' ').map_or(line, |at| &line[..at])
}

/// Compares two BIN_SAFE archives value by value. Prints the first few differences with the
/// objects they are in and the values before them, then how many differ, grouped by name.
///
/// Where one archive has values the other does not, the two are lined up again by looking a
/// little ahead in each, so that one missing value does not make everything after it differ.
fn compare_values(game: &[u8], written: &[u8]) {
    let (ours, theirs) = match (binsafe_values(written), binsafe_values(game)) {
        (Ok(ours), Ok(theirs)) => (ours, theirs),
        (Err(at), _) => return println!("cannot split the written archive at byte {at}"),
        (_, Err(at)) => return println!("cannot split the game's archive at byte {at}"),
    };
    let lines_up = |i: usize, j: usize| {
        (0..4).all(|k| match (theirs.get(i + k), ours.get(j + k)) {
            (Some(a), Some(b)) => same(a, b),
            (a, b) => a.is_none() && b.is_none(),
        })
    };

    let mut groups = std::collections::BTreeMap::<String, usize>::new();
    let (mut i, mut j, mut shown) = (0, 0, 0);
    while i < theirs.len() || j < ours.len() {
        if let (Some(a), Some(b)) = (theirs.get(i), ours.get(j))
            && same(a, b)
        {
            i += 1;
            j += 1;
            continue;
        }
        let skip = (1..=64)
            .find_map(|d| {
                if lines_up(i + d, j) {
                    Some((d, 0))
                } else if lines_up(i, j + d) {
                    Some((0, d))
                } else {
                    None
                }
            })
            .unwrap_or((1, 1));

        let label = theirs.get(i).or(ours.get(j)).map_or("?", Value::label);
        let label = match skip {
            (_, 0) => format!("{label} (only in the game's)"),
            (0, _) => format!("{label} (only in ours)"),
            _ => label.to_owned(),
        };
        *groups.entry(label).or_default() += 1;
        if shown < 5 {
            report(&theirs, &ours, i, j, skip);
            shown += 1;
        }
        i += skip.0;
        j += skip.1;
    }

    if groups.is_empty() {
        println!("\nevery value is the same, but for what the writer leaves out on purpose");
        return;
    }
    println!("\ndifferences by value name:");
    for (label, count) in &groups {
        println!("  {count:>7} {label}");
    }
}

fn report(theirs: &[Value], ours: &[Value], i: usize, j: usize, skip: (usize, usize)) {
    println!("\nat value {i} of {} in the game's:", theirs.len());
    let mut open = Vec::new();
    for value in &theirs[..i.min(theirs.len())] {
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
    for value in &theirs[i.saturating_sub(3)..i.min(theirs.len())] {
        println!("    {}", value.show());
    }

    let list = |values: &[Value]| {
        for value in values.iter().take(8) {
            println!("    {}", value.show());
        }
        if values.len() > 8 {
            println!("    ...");
        }
    };
    match skip {
        (extra, 0) => {
            println!("  the game's has {extra} values more here:");
            list(&theirs[i..(i + extra).min(theirs.len())]);
        }
        (0, extra) => {
            println!("  ours has {extra} values more here:");
            list(&ours[j..(j + extra).min(ours.len())]);
        }
        _ => {
            let show = |v: Option<&Value>| v.map_or("(none)".into(), Value::show);
            println!("  game    {}", show(theirs.get(i)));
            println!("  written {}", show(ours.get(j)));
        }
    }

    if let (Some(a), Some(b)) = (theirs.get(i), ours.get(j))
        && a.bytes[0] == GEOMETRY
        && b.bytes[0] == GEOMETRY
    {
        let (a, b) = (&a.bytes[1..], &b.bytes[1..]);
        let offset = (a.iter().zip(b).enumerate())
            .position(|(at, (x, y))| x != y && !(30..32).contains(&at))
            .unwrap_or(a.len().min(b.len()));
        println!("  first differing byte {offset}, in chunk {}", chunk_at(a, offset));
        println!("    game    {}", around(a, offset));
        println!("    written {}", around(b, offset));
    }
}

/// Names the chunk of a `MeshAndBsp` block that `offset` falls in, like `0xb050 at 1234`.
fn chunk_at(block: &[u8], offset: usize) -> String {
    let mut at = 8;
    while at + 6 <= block.len() {
        let id = u16::from_le_bytes([block[at], block[at + 1]]);
        let len = u32::from_le_bytes(block[at + 2..at + 6].try_into().unwrap()) as usize;
        if offset < at + 6 + len {
            return format!("{id:#06x} starting at {at}");
        }
        at += 6 + len;
    }
    "(none)".into()
}

/// The length of a `MeshAndBsp` block starting at `at`: the BSP version, a size the engine does
/// not trust, then the mesh's chunks up to its end chunk and the tree's up to its own.
fn geometry_len(data: &[u8], at: usize) -> Option<usize> {
    let mut end = at + 8;
    for last in [0xB060, 0xC0FF] {
        loop {
            let id = u16::from_le_bytes(data.get(end..end + 2)?.try_into().ok()?);
            end += 6 + u32::from_le_bytes(data.get(end + 2..end + 6)?.try_into().ok()?) as usize;
            if id == last {
                break;
            }
        }
    }
    Some(end - at)
}

/// Splits a BIN_SAFE archive into its values, naming each from the key table, or gives the
/// byte where that failed.
fn binsafe_values(data: &[u8]) -> Result<Vec<Value>, usize> {
    let mut at = 0;
    split(data, &mut at).ok_or(at)
}

fn split(data: &[u8], at: &mut usize) -> Option<Vec<Value>> {
    let u16_at =
        |at: usize| Some(usize::from(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?)));
    let u32_at =
        |at: usize| Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize);
    let start = data.windows(4).position(|w| w == b"END\n")? + 4;
    let table = u32_at(start + 8)?;

    let mut names = std::collections::HashMap::new();
    *at = table + 4;
    for _ in 0..u32_at(table)? {
        let (len, index) = (u16_at(*at)?, u16_at(*at + 2)?);
        let name = data.get(*at + 8..*at + 8 + len)?;
        names.insert(index, String::from_utf8_lossy(name).into_owned());
        *at += 8 + len;
    }

    let mut values = Vec::new();
    *at = start + 12;
    while *at < table {
        let mut kind = *data.get(*at)?;
        let mut key = String::new();
        if kind == 0x12 {
            key = names.get(&u32_at(*at + 1)?).cloned().unwrap_or_else(|| "?".into());
            kind = *data.get(*at + 5)?;
            *at += 5;
        }
        let len = match kind {
            0x01 | 0x09 | 0x10 => u16_at(*at + 1)? + 2,
            0x02 | 0x03 | 0x06 | 0x08 | 0x11 => 4,
            0x04 => 1,
            0x05 => 2,
            0x07 => 12,
            _ => return None,
        };
        let value = Value { key, bytes: data.get(*at..*at + 1 + len)?.to_vec() };
        *at += 1 + len;
        let geometry = value.key.is_empty()
            && value.bytes.get(3..).is_some_and(|t| t.starts_with(b"[MeshAndBsp "));
        values.push(value);
        if geometry {
            let len = geometry_len(data, *at)?;
            let mut bytes = vec![GEOMETRY];
            bytes.extend(&data[*at..*at + len]);
            values.push(Value { key: "MeshAndBsp".into(), bytes });
            *at += len;
        }
    }
    Some(values)
}
