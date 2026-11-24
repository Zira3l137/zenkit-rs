//! Writing `.VDF` archives, laid out like the original tools do: a directory's entries are
//! listed together, directories first, and the entries of each subdirectory follow after all of
//! its siblings.

use std::borrow::Cow;
use std::io;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use time::OffsetDateTime;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::text;

use crate::disk::ENTRY_DIR;
use crate::disk::ENTRY_LAST;
use crate::disk::FLAGS_NORMAL;
use crate::disk::FLAGS_ZIPPED;
use crate::disk::SIGNATURE_G1;
use crate::disk::SIGNATURE_G2;
use crate::node::Node;
use crate::par;
use crate::zipped;

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
    /// Whether to compress the files, as Union's archives do, which only games running Union
    /// can read. Sounds (`.WAV`, `.OGG`) are stored as they are, as Union stores them.
    pub compressed: bool,
}

impl DiskOptions {
    pub fn new(game: GameVersion) -> Self {
        Self { game, comment: "Created with zenkit-rs".into(), time: None, compressed: false }
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

    let files: Vec<&Node> = entries.iter().map(|e| e.node).filter(|node| node.is_file()).collect();
    // The catalog comes first and holds each file's stored size, which for a compressed file is
    // only known once it is compressed.
    let compressed = par::try_map(files.clone(), |node| -> Result<Option<Vec<u8>>> {
        if !options.compressed || is_sound(node.name()) {
            return Ok(None);
        }
        Ok(Some(zipped::encode(&contents(node)?)))
    })?;
    let stored: Vec<usize> = files
        .iter()
        .zip(&compressed)
        .map(|(node, compressed)| compressed.as_ref().map_or(size(node), Vec::len))
        .collect();

    let mut offset = HEADER_SIZE + entries.len() * ENTRY_SIZE;
    let mut newest = UNIX_EPOCH;
    let mut sizes = Vec::with_capacity(files.len());
    let file_entries = entries.iter_mut().filter(|e| e.node.is_file());
    for (entry, stored) in file_entries.zip(&stored) {
        entry.offset = offset;
        offset += stored;
        newest = newest.max(entry.node.time());
        sizes.push(size(entry.node));
    }
    // Union's archives give the files' size before compression, as uncompressed ones do.
    let data_size: usize = sizes.iter().sum();
    if u32::try_from(offset).is_err() || u32::try_from(data_size).is_err() {
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
    head.u32(files.len() as u32);
    head.u32(dos_time(options.time.unwrap_or(newest)));
    head.u32(data_size as u32);
    head.u32(HEADER_SIZE as u32);
    head.u32(if options.compressed { FLAGS_ZIPPED } else { FLAGS_NORMAL });

    let mut sizes = sizes.into_iter();
    for entry in &entries {
        let name = latin1(&entry.node.name().to_ascii_uppercase(), NAME_SIZE, entry.node.name())?;
        head.bytes(&name);
        head.bytes(&vec![b' '; NAME_SIZE - name.len()]);
        head.u32(entry.offset as u32);
        let size = if entry.node.is_file() { sizes.next().unwrap_or(0) } else { 0 };
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
    // A batch at a time, read in parallel, so that an archive's worth of files read from disk is
    // never in memory at once.
    let mut batch = Vec::new();
    let mut batch_size = 0;
    let files = files.into_iter().zip(compressed.iter().map(Option::as_deref)).zip(stored);
    for (file, stored) in files {
        batch.push((file, stored));
        batch_size += stored;
        if batch_size >= BATCH_SIZE {
            write_batch(std::mem::take(&mut batch), out)?;
            batch_size = 0;
        }
    }
    write_batch(batch, out)?;
    out.flush().map_err(Error::Output)
}

/// How many bytes of files to read before writing them.
const BATCH_SIZE: usize = if cfg!(test) { 100 } else { 64 << 20 };

type Batch<'a> = Vec<((&'a Node, Option<&'a [u8]>), usize)>;

fn write_batch(batch: Batch, out: &mut impl io::Write) -> Result<()> {
    let read = par::try_map(batch, |((node, compressed), stored)| {
        let data = match compressed {
            Some(compressed) => Cow::Borrowed(compressed),
            None => contents(node)?,
        };
        // The catalog already gave this file its place.
        if data.len() != stored {
            let reason = format!("{} changed size while it was written", node.path());
            return Err(Error::unwritable(FORMAT, reason));
        }
        Ok(data)
    })?;
    for data in &read {
        out.write_all(data).map_err(Error::Output)?;
    }
    Ok(())
}

fn contents(node: &Node) -> Result<Cow<'_, [u8]>> {
    node.contents().context(|| node.path().to_owned())
}

fn size(node: &Node) -> usize {
    node.size().unwrap_or(0)
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

fn is_sound(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.ends_with(".WAV") || name.ends_with(".OGG")
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

    use std::sync::Arc;

    use super::*;
    use crate::Overwrite;
    use crate::disk;
    use crate::disk::Volume;
    use crate::node::Storage;

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

    #[test]
    fn writes_many_batches() {
        let mut root = Node::dir("", UNIX_EPOCH);
        let files: Vec<Vec<u8>> = (0..12).map(|i| vec![i as u8; 30 + i * 7]).collect();
        for (i, data) in files.iter().enumerate() {
            root.insert(Node::file(format!("F{i}.TXT"), data.clone(), UNIX_EPOCH)).unwrap();
        }
        for compressed in [false, true] {
            let options = DiskOptions { compressed, ..DiskOptions::new(GameVersion::Gothic2) };
            let mut out = Vec::new();
            write(&root, &mut out, &options).unwrap();
            let volume = Volume::Memory(Arc::new(Storage::Owned(out.into())));
            let read = disk::parse(&volume, Overwrite::All).unwrap();
            for (i, data) in files.iter().enumerate() {
                assert_eq!(read[&format!("f{i}.txt")].data(), Some(&data[..]), "{compressed}");
            }
        }
    }
}
