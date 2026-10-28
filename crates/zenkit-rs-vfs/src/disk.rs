//! The `.VDF` archive format.
//!
//! An archive is a [`Header`] followed by a catalog of [`Entry`] records. The catalog is a
//! flattened tree: a directory entry's `offset` is the index of its first child entry, and the
//! last entry of every directory carries [`ENTRY_LAST`]. A file entry's `offset` is the absolute
//! position of its contents in the archive.

use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use time::Date;
use time::Month;
use time::PrimitiveDateTime;
use time::Time;
use tracing::warn;
use zenkit_rs_core::Error;
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

const FORMAT: &str = "VDF archive";

const SIGNATURE_G1: &[u8; 16] = b"PSVDSC_V2.00\r\n\r\n";
const SIGNATURE_G2: &[u8; 16] = b"PSVDSC_V2.00\n\r\n\r";
const SIGNATURE_VDFSTOOL: &[u8; 16] = b"PSVDSC_V2.00\x1A\x1A\x1A\x1A";

const FLAGS_NORMAL: u32 = 0x50;
const FLAGS_ZIPPED: u32 = 0xA0;

const ENTRY_DIR: u32 = 0x8000_0000;
const ENTRY_LAST: u32 = 0x4000_0000;

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

#[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct Entry {
    name: [u8; 64],
    offset: U32,
    size: U32,
    kind: U32,
    attributes: U32,
}

struct Catalog<'a> {
    storage: &'a Arc<Storage>,
    entries: &'a [Entry],
    time: SystemTime,
    overwrite: Overwrite,
    budget: usize,
}

/// Parses a whole archive into its top-level nodes.
pub(super) fn parse(storage: Arc<Storage>, overwrite: Overwrite) -> Result<Children> {
    let mut r = Reader::new(&storage);
    let header = r.record::<Header>()?;

    if ![SIGNATURE_G1, SIGNATURE_G2, SIGNATURE_VDFSTOOL].contains(&&header.signature) {
        let found = header.signature.escape_ascii();
        return Err(Error::malformed(FORMAT, format!("unknown signature \"{found}\"")));
    }

    match header.flags.get() {
        FLAGS_NORMAL => {}
        FLAGS_ZIPPED => {
            return Err(Error::unsupported(
                FORMAT,
                "compressed (Union ZippedStream) archives are not supported yet",
            ));
        }
        flags => warn!("unknown volume flags {flags:#x}, assuming uncompressed"),
    }

    match header.catalog_offset.get() {
        0 => {}
        offset => r.seek(offset as usize)?,
    }
    let entries = r.records::<Entry>(r.remaining() / size_of::<Entry>())?;

    let mut catalog = Catalog {
        storage: &storage,
        entries,
        time: dos_time(header.timestamp.get()),
        overwrite,
        budget: entries.len(),
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

        let entry = catalog
            .entries
            .get(index)
            .ok_or_else(|| Error::malformed(FORMAT, format!("catalog entry {index} is missing")))?;

        let name = decode_name(&entry.name);
        let kind = entry.kind.get();

        if kind & ENTRY_DIR != 0 {
            let mut children = Children::new();
            read_dir(catalog, entry.offset.get() as usize, depth + 1, &mut children)?;
            let node = Node { name, time: catalog.time, kind: Kind::Dir(children) };
            merge(out, node, catalog.overwrite);
        } else {
            let start = entry.offset.get() as usize;
            let end = start.checked_add(entry.size.get() as usize);
            if let Some(end) = end.filter(|&end| end <= catalog.storage.len()) {
                let node = Node {
                    name,
                    time: catalog.time,
                    kind: Kind::File(FileData::new(Arc::clone(catalog.storage), start, end)),
                };
                merge(out, node, catalog.overwrite);
            } else {
                warn!("skipping {name:?}, its contents lie outside the archive");
            }
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
