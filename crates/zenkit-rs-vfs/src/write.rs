//! Writing `.VDF` archives, laid out like the original tools do: a directory's entries are
//! listed together, directories first, and the entries of each subdirectory follow after all of
//! its siblings.

use std::io;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use time::OffsetDateTime;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::text;

use crate::disk::ENTRY_DIR;
use crate::disk::ENTRY_LAST;
use crate::disk::FLAGS_NORMAL;
use crate::disk::SIGNATURE_G1;
use crate::disk::SIGNATURE_G2;
use crate::node::Node;

const FORMAT: &str = "VDF archive";
const HEADER_SIZE: usize = 296;
const ENTRY_SIZE: usize = 80;
const NAME_SIZE: usize = 64;
const COMMENT_SIZE: usize = 256;

/// The Windows "archive" attribute, which the original tools set on every file.
const ATTRIBUTE_ARCHIVE: u32 = 0x20;

/// How to write a `.VDF` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskOptions {
    /// The game whose signature the archive gets. Both games read either.
    pub game: GameVersion,
    /// Free text at the start of the archive, at most 256 bytes of Latin-1.
    pub comment: String,
    /// The archive's one timestamp, which `Overwrite::Newer` and `Older` compare. `None` takes
    /// the newest of the files written.
    pub time: Option<SystemTime>,
}

impl DiskOptions {
    pub fn new(game: GameVersion) -> Self {
        Self { game, comment: "Created with zenkit-rs".into(), time: None }
    }
}

/// A catalog entry about to be written.
struct Entry<'a> {
    node: &'a Node,
    /// The index of a directory's first child, or the offset of a file's contents.
    offset: usize,
    last: bool,
}

/// Writes the children of `dir` as an archive.
pub(crate) fn write(dir: &Node, out: &mut impl io::Write, options: &DiskOptions) -> Result<()> {
    if !dir.is_dir() {
        return Err(Error::NotADirectory(dir.name().into()));
    }

    let mut entries = Vec::new();
    flatten(dir, &mut entries);
    if entries.is_empty() {
        return Err(Error::unwritable(FORMAT, "an archive needs at least one file"));
    }

    let mut offset = HEADER_SIZE + entries.len() * ENTRY_SIZE;
    let mut files = 0u32;
    let mut newest = UNIX_EPOCH;
    for entry in entries.iter_mut().filter(|e| e.node.is_file()) {
        entry.offset = offset;
        offset += entry.node.data().map_or(0, <[u8]>::len);
        files += 1;
        newest = newest.max(entry.node.time());
    }
    let data_size = offset - HEADER_SIZE - entries.len() * ENTRY_SIZE;
    if u32::try_from(offset).is_err() {
        let reason = format!("{offset} bytes is past the 4 GiB a VDF can address");
        return Err(Error::unwritable(FORMAT, reason));
    }

    let mut head = Writer::with_capacity(HEADER_SIZE + entries.len() * ENTRY_SIZE);
    let comment = latin1(&options.comment, COMMENT_SIZE, "the comment")?;
    head.bytes(&comment);
    head.bytes(&vec![0x1A; COMMENT_SIZE - comment.len()]);
    head.bytes(match options.game {
        GameVersion::Gothic1 => SIGNATURE_G1,
        GameVersion::Gothic2 => SIGNATURE_G2,
    });
    head.u32(entries.len() as u32);
    head.u32(files);
    head.u32(dos_time(options.time.unwrap_or(newest)));
    head.u32(data_size as u32);
    head.u32(HEADER_SIZE as u32);
    head.u32(FLAGS_NORMAL);

    for entry in &entries {
        let name = latin1(&entry.node.name().to_ascii_uppercase(), NAME_SIZE, entry.node.name())?;
        head.bytes(&name);
        head.bytes(&vec![b' '; NAME_SIZE - name.len()]);
        head.u32(entry.offset as u32);
        let size = entry.node.data().map_or(0, <[u8]>::len);
        head.u32(size as u32);
        let last = if entry.last { ENTRY_LAST } else { 0 };
        if entry.node.is_dir() {
            head.u32(ENTRY_DIR | last);
            head.u32(0);
        } else {
            head.u32(last);
            head.u32(ATTRIBUTE_ARCHIVE);
        }
    }

    out.write_all(head.as_bytes()).map_err(Error::Output)?;
    for entry in entries.iter().filter_map(|e| e.node.data()) {
        out.write_all(entry).map_err(Error::Output)?;
    }
    out.flush().map_err(Error::Output)
}

/// Lists the entries of `dir` and, after them, those of its subdirectories. Directories without
/// any files are left out: the format has no way to store one, since a directory's entries end
/// with the one marked last.
fn flatten<'a>(dir: &'a Node, entries: &mut Vec<Entry<'a>>) {
    let start = entries.len();
    let dirs = dir.children().filter(|c| c.is_dir() && has_files(c));
    let files = dir.children().filter(|c| c.is_file());
    entries.extend(dirs.chain(files).map(|node| Entry { node, offset: 0, last: false }));
    if let Some(last) = entries[start..].last_mut() {
        last.last = true;
    }

    for i in start..entries.len() {
        if entries[i].node.is_dir() {
            entries[i].offset = entries.len();
            flatten(entries[i].node, entries);
        }
    }
}

fn has_files(node: &Node) -> bool {
    node.is_file() || node.children().any(has_files)
}

fn latin1(text: &str, max: usize, what: &str) -> Result<Vec<u8>> {
    let bytes = text::to_latin1(text).ok_or_else(|| {
        Error::unwritable(FORMAT, format!("{what:?} has characters beyond Latin-1"))
    })?;
    if bytes.len() > max {
        let reason = format!("{what:?} is {} bytes long, at most {max} fit", bytes.len());
        return Err(Error::unwritable(FORMAT, reason));
    }
    Ok(bytes)
}

/// An MS-DOS timestamp in UTC, the reverse of `disk::dos_time`. It counts from 1980 to 2107, so
/// times outside that range are clamped to it, and seconds are rounded down to even ones.
fn dos_time(time: SystemTime) -> u32 {
    let time = OffsetDateTime::from(time);
    let fields = match time.year() {
        ..1980 => [0, 1, 1, 0, 0, 0],
        2108.. => [127, 12, 31, 23, 59, 58],
        year => [
            year as u32 - 1980,
            u8::from(time.month()).into(),
            time.day().into(),
            time.hour().into(),
            time.minute().into(),
            time.second().into(),
        ],
    };
    let [year, month, day, hour, minute, second] = fields;
    (year << 25) | (month << 21) | (day << 16) | (hour << 11) | (minute << 5) | (second / 2)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::disk;

    #[test]
    fn dos_times_round_trip() {
        let time = UNIX_EPOCH + Duration::from_secs(1_078_099_198);
        assert_eq!(disk::dos_time(dos_time(time)), time);
        let odd = time + Duration::from_secs(1);
        assert_eq!(disk::dos_time(dos_time(odd)), time);
        assert_eq!(
            disk::dos_time(dos_time(UNIX_EPOCH)),
            UNIX_EPOCH + Duration::from_secs(315_532_800)
        );
    }
}
