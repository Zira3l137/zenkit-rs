//! The `.VDF` archive format.
//!
//! An archive is a [`Header`] followed by a catalog of [`Entry`] records. The catalog is a
//! flattened tree: a directory entry's `offset` is the index of its first child entry, and the
//! last entry of every directory carries [`ENTRY_LAST`]. A file entry's `offset` is the absolute
//! position of its contents in the archive.

use std::borrow::Cow;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use time::Date;
use time::Month;
use time::PrimitiveDateTime;
use time::Time;
use tracing::warn;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::text;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::KnownLayout;
use zerocopy::Unaligned;
use zerocopy::little_endian::U32;

use super::Overwrite;
use super::merge;
use super::node::Children;
use super::node::FileData;
use super::node::Kind;
use super::node::Node;
use super::node::Storage;
#[cfg(not(feature = "mmap"))]
use crate::host::DiskFile;
use crate::write::DiskOptions;

const FORMAT: &str = "VDF archive";

pub(crate) const SIGNATURE_G1: &[u8; 16] = b"PSVDSC_V2.00\r\n\r\n";
pub(crate) const SIGNATURE_G2: &[u8; 16] = b"PSVDSC_V2.00\n\r\n\r";
const SIGNATURE_VDFSTOOL: &[u8; 16] = b"PSVDSC_V2.00\x1A\x1A\x1A\x1A";

pub(crate) const FLAGS_NORMAL: u32 = 0x50;
/// Union's compressed archives, whose files are stored as ZippedStreams, see
/// [`zipped`](crate::zipped). The catalog is the same.
pub(crate) const FLAGS_ZIPPED: u32 = 0xA0;

pub(crate) const ENTRY_DIR: u32 = 0x8000_0000;
pub(crate) const ENTRY_LAST: u32 = 0x4000_0000;

const MAX_DEPTH: usize = 128;

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct Header {
    comment: [u8; 256],
    signature: [u8; 16],
    entry_count: U32,
    file_count: U32,
    timestamp: U32,
    archive_size: U32,
    catalog_offset: U32,
    flags: U32,
}

#[derive(Clone, Copy, FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct Entry {
    name: [u8; 64],
    offset: U32,
    size: U32,
    kind: U32,
    attributes: U32,
}

/// An archive's bytes: in memory, or in a file on disk that is read only where it is needed.
pub(super) enum Volume {
    Memory(Arc<Storage>),
    #[cfg(not(feature = "mmap"))]
    Disk(Arc<DiskFile>),
}

impl Volume {
    fn len(&self) -> usize {
        match self {
            Self::Memory(storage) => storage.len(),
            #[cfg(not(feature = "mmap"))]
            Self::Disk(file) => file.len(),
        }
    }

    /// Up to `len` bytes at `offset`, fewer where the archive ends.
    fn read(&self, offset: usize, len: usize) -> Result<Cow<'_, [u8]>> {
        let end = offset.saturating_add(len).min(self.len());
        let start = offset.min(end);
        match self {
            Self::Memory(storage) => Ok(Cow::Borrowed(&storage[start..end])),
            #[cfg(not(feature = "mmap"))]
            Self::Disk(file) => file.read(start, end - start).map(Cow::Owned),
        }
    }

    /// The file an entry lists, or `None` if its contents lie outside the archive.
    fn file(&self, entry: &Entry, zipped: bool) -> Option<FileData> {
        let (start, size) = (entry.offset.get() as usize, entry.size.get() as usize);
        let fits = start.checked_add(size).is_some_and(|end| end <= self.len());
        match self {
            Self::Memory(storage) => {
                let stream = zipped.then(|| crate::zipped::scan(storage.get(start..)?, size));
                match stream.flatten() {
                    Some(len) => Some(FileData::zipped(Arc::clone(storage), start, start + len, size)),
                    None => fits.then(|| FileData::new(Arc::clone(storage), start, start + size)),
                }
            }
            // Whether a file of a compressed archive is a stream, and how long, is only known once
            // it is read, so here it only has to start inside the archive.
            #[cfg(not(feature = "mmap"))]
            Self::Disk(file) => (fits || (zipped && start < self.len()))
                .then(|| FileData::on_disk(Arc::clone(file), start, size, zipped)),
        }
    }
}

struct Catalog<'a> {
    volume: &'a Volume,
    /// Where the entries start, and those read so far.
    offset: usize,
    entries: Cow<'a, [u8]>,
    /// How many entries fit between `offset` and the end of the archive.
    capacity: usize,
    time: SystemTime,
    overwrite: Overwrite,
    zipped: bool,
    budget: usize,
}

impl Catalog<'_> {
    fn entry(&mut self, index: usize) -> Result<Entry> {
        let missing = || Error::malformed(FORMAT, format!("catalog entry {index} is missing"));
        if index >= self.capacity {
            return Err(missing());
        }
        let end = (index + 1) * size_of::<Entry>();
        if end > self.entries.len() {
            // Read from disk, the catalog starts out as long as the header says. It is read
            // further in growing steps, so that a wrong count reads as it does from memory.
            let have = self.entries.len();
            let want = end.max(have * 2).min(self.capacity * size_of::<Entry>());
            let more = self.volume.read(self.offset + have, want - have)?;
            self.entries.to_mut().extend_from_slice(&more);
        }
        let bytes = self.entries.get(end - size_of::<Entry>()..end).ok_or_else(missing)?;
        Entry::read_from_bytes(bytes).map_err(|_| missing())
    }
}

/// What a `.VDF` archive says about itself in its header, read without mounting it.
///
/// Mounting merges archives into one tree and keeps none of this, so a tool that opens an archive
/// and saves it again reads it here first to write the same kind of archive back:
///
/// ```no_run
/// use zenkit_rs_vfs::DiskInfo;
/// use zenkit_rs_vfs::Overwrite;
/// use zenkit_rs_vfs::Vfs;
///
/// let info = DiskInfo::read("Data/Anims.vdf")?;
/// let mut vfs = Vfs::new();
/// vfs.mount_disk("Data/Anims.vdf", Overwrite::All)?;
/// // ... change the tree ...
/// vfs.write_disk(Vec::new(), &info.options())?;
/// # Ok::<(), zenkit_rs_core::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskInfo {
    /// The game whose signature the archive has, or `None` for archives made by the community's
    /// `vdfs` tool, whose signature belongs to neither. Both games read all three.
    pub game: Option<GameVersion>,
    /// The free text at the start of the archive, without the padding after it.
    pub comment: String,
    /// The archive's one timestamp, which every file mounted from it takes.
    pub time: SystemTime,
    /// Whether the files are stored compressed, as in Union's archives.
    pub compressed: bool,
    /// How many directories and files the catalog lists.
    pub entry_count: u32,
    /// How many of those are files.
    pub file_count: u32,
}

impl DiskInfo {
    /// Reads the header of the archive at `path`, and nothing past it.
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut header = [0; size_of::<Header>()];
        File::open(path)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(Error::io(path))?;
        Self::from_bytes(&header)
    }

    /// Reads the header at the start of `data`, which may be the whole archive or just its first
    /// 296 bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let header = Reader::new(data).record::<Header>()?;
        let game = match &header.signature {
            SIGNATURE_G1 => Some(GameVersion::Gothic1),
            SIGNATURE_G2 => Some(GameVersion::Gothic2),
            SIGNATURE_VDFSTOOL => None,
            other => {
                let found = other.escape_ascii();
                return Err(Error::malformed(FORMAT, format!("unknown signature \"{found}\"")));
            }
        };
        // The original tools pad the comment with 0x1A, the DOS end-of-file mark.
        let end = header.comment.iter().position(|&b| b == 0x1A || b == 0).unwrap_or(256);
        Ok(Self {
            game,
            comment: text::latin1(&header.comment[..end]),
            time: dos_time(header.timestamp.get()),
            compressed: is_zipped(header.flags.get()),
            entry_count: header.entry_count.get(),
            file_count: header.file_count.get(),
        })
    }

    /// Options that write an archive like this one: the same signature, comment and compression.
    /// Archives of the `vdfs` tool get Gothic II's signature. The timestamp is left to the
    /// writer, which takes the newest of the files, so that files added since keep theirs.
    pub fn options(&self) -> DiskOptions {
        let mut options = DiskOptions::new(self.game.unwrap_or(GameVersion::Gothic2));
        options.comment.clone_from(&self.comment);
        options.compressed = self.compressed;
        options
    }
}

fn is_zipped(flags: u32) -> bool {
    match flags {
        FLAGS_NORMAL => false,
        FLAGS_ZIPPED => true,
        flags => {
            warn!("unknown volume flags {flags:#x}, assuming uncompressed");
            false
        }
    }
}

/// Parses an archive's catalog into its top-level nodes.
pub(super) fn parse(volume: &Volume, overwrite: Overwrite) -> Result<Children> {
    let head = volume.read(0, size_of::<Header>())?;
    let header = Reader::new(&head).record::<Header>()?;

    if ![SIGNATURE_G1, SIGNATURE_G2, SIGNATURE_VDFSTOOL].contains(&&header.signature) {
        let found = header.signature.escape_ascii();
        return Err(Error::malformed(FORMAT, format!("unknown signature \"{found}\"")));
    }

    let offset = match header.catalog_offset.get() as usize {
        0 => size_of::<Header>(),
        offset if offset > volume.len() => {
            return Err(Error::OutOfBounds { offset, len: volume.len() });
        }
        offset => offset,
    };
    let capacity = (volume.len() - offset) / size_of::<Entry>();
    let entries = match volume {
        Volume::Memory(_) => volume.read(offset, capacity * size_of::<Entry>())?,
        #[cfg(not(feature = "mmap"))]
        Volume::Disk(_) => {
            let listed = (header.entry_count.get() as usize).min(capacity);
            volume.read(offset, listed * size_of::<Entry>())?
        }
    };

    let mut catalog = Catalog {
        volume,
        offset,
        entries,
        capacity,
        time: dos_time(header.timestamp.get()),
        overwrite,
        zipped: is_zipped(header.flags.get()),
        budget: capacity,
    };

    let mut root = Children::new();
    read_dir(&mut catalog, 0, 0, &mut root)?;
    Ok(root)
}

fn read_dir(catalog: &mut Catalog, first: usize, depth: usize, out: &mut Children) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::malformed(FORMAT, "directories nest too deeply"));
    }

    for index in first.. {
        // Every entry costs one unit, so a catalog whose directories point back at each other
        // runs out instead of being walked forever.
        catalog.budget = catalog.budget.checked_sub(1).ok_or_else(|| {
            Error::malformed(FORMAT, "catalog references more entries than it has")
        })?;

        let entry = catalog.entry(index)?;

        let name = decode_name(&entry.name);
        let kind = entry.kind.get();

        if kind & ENTRY_DIR != 0 {
            let mut children = Children::new();
            read_dir(catalog, entry.offset.get() as usize, depth + 1, &mut children)?;
            let node = Node::new(name, catalog.time, Kind::Dir(children));
            merge(out, None, node, catalog.overwrite);
        } else if let Some(data) = catalog.volume.file(&entry, catalog.zipped) {
            let node = Node::new(name, catalog.time, Kind::File(data));
            merge(out, None, node, catalog.overwrite);
        } else {
            warn!("skipping {name:?}, its contents lie outside the archive");
        }

        if kind & ENTRY_LAST != 0 {
            break;
        }
    }

    Ok(())
}

/// Names are space-padded to 64 bytes.
fn decode_name(raw: &[u8]) -> String {
    let len = raw
        .iter()
        .rposition(|&b| !(b.is_ascii_whitespace() || b == b'\x0B' || b == 0))
        .map_or(0, |i| i + 1);
    text::latin1(&raw[..len])
}

/// Converts an MS-DOS timestamp, read as UTC. Invalid dates become the Unix epoch.
///
/// ZenKit reads it as local time instead, so its timestamps are shifted by the local UTC offset.
pub(super) fn dos_time(dos: u32) -> SystemTime {
    let field = |shift: u32, bits: u32| ((dos >> shift) & ((1 << bits) - 1)) as u8;

    let date = Month::try_from(field(21, 4)).and_then(|month| {
        Date::from_calendar_date(1980 + i32::from(field(25, 7)), month, field(16, 5))
    });
    let time = Time::from_hms(field(11, 5), field(5, 6), field(0, 5) * 2);

    match (date, time) {
        (Ok(date), Ok(time)) => PrimitiveDateTime::new(date, time).assume_utc().into(),
        _ => {
            warn!("invalid DOS timestamp {dos:#010x}");
            UNIX_EPOCH
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix(time: SystemTime) -> u64 {
        time.duration_since(UNIX_EPOCH).unwrap().as_secs()
    }

    fn dos(y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> u32 {
        ((y - 1980) << 25) | (mo << 21) | (d << 16) | (h << 11) | (mi << 5) | (s / 2)
    }

    #[test]
    fn record_sizes_match_the_format() {
        assert_eq!(size_of::<Header>(), 296);
        assert_eq!(size_of::<Entry>(), 80);
    }

    #[test]
    fn converts_dos_timestamps() {
        assert_eq!(unix(dos_time(dos(1980, 1, 1, 0, 0, 0))), 315_532_800);
        assert_eq!(unix(dos_time(dos(2000, 1, 1, 0, 0, 0))), 946_684_800);
        assert_eq!(unix(dos_time(dos(2004, 2, 29, 23, 59, 58))), 1_078_099_198);
    }

    #[test]
    fn invalid_dos_timestamps_fall_back_to_epoch() {
        assert_eq!(dos_time(dos(2001, 0, 1, 0, 0, 0)), UNIX_EPOCH);
        assert_eq!(dos_time(dos(2001, 2, 30, 0, 0, 0)), UNIX_EPOCH);
        assert_eq!(dos_time(dos(2001, 1, 1, 25, 0, 0)), UNIX_EPOCH);
    }

    #[test]
    fn trims_padding_from_names() {
        assert_eq!(decode_name(b"ANIMS   \0\0"), "ANIMS");
        assert_eq!(decode_name(b"   "), "");
        assert_eq!(decode_name(b"\xC4PFEL "), "\u{C4}PFEL");
    }
}
