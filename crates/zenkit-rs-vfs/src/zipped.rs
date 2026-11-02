//! Union's ZippedStream, how compressed `.VDF` archives store each file:
//!
//! ```text
//! stream  length: u32 | block size: u32 | block count: u32 | block...
//! block   length: u32 | compressed length: u32 | block size: u32 | zlib data
//! ```
//!
//! `length` is the file's uncompressed size, which the catalog lists too. Each block holds up to
//! `block size` bytes of the file, compressed on its own.

use std::io::Read;
use std::io::Write;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;

const FORMAT: &str = "VDF archive";

/// The block size ZenKit writes, which it takes from Union. Readers take any.
const BLOCK_SIZE: usize = 8192;

/// How many bytes the stream for a file of `size` bytes takes at the start of `data`, or `None`
/// if `data` does not start with one, as with the sound files Union stores uncompressed.
pub(crate) fn scan(data: &[u8], size: usize) -> Option<usize> {
    let mut r = Reader::new(data);
    let (length, block_size, count) = (r.u32().ok()?, r.u32().ok()?, r.u32().ok()?);
    let (length, block_size, count) = (length as usize, block_size as usize, count as usize);
    if length != size || block_size == 0 || count != length.div_ceil(block_size) {
        return None;
    }
    let mut left = length;
    for _ in 0..count {
        let (block, compressed) = (r.u32().ok()? as usize, r.u32().ok()? as usize);
        r.u32().ok()?;
        if block > block_size || block > left {
            return None;
        }
        left -= block;
        r.bytes(compressed).ok()?;
    }
    (left == 0).then_some(r.position())
}

/// Decompresses a stream that [`scan`] accepted.
pub(crate) fn decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(data);
    let length = r.u32()? as usize;
    r.u32()?;
    let count = r.u32()?;

    let mut out = Vec::with_capacity(length);
    for i in 0..count {
        let (block, compressed) = (r.u32()? as usize, r.u32()? as usize);
        r.u32()?;
        let start = out.len();
        // One byte more than the block should hold, to tell a block that is too long.
        let mut decoder = ZlibDecoder::new(r.bytes(compressed)?).take(block as u64 + 1);
        decoder.read_to_end(&mut out).map_err(|e| {
            Error::malformed(FORMAT, format!("block {i} of a compressed file: {e}"))
        })?;
        if out.len() - start != block {
            let reason = format!(
                "block {i} of a compressed file holds {} bytes, not {block}",
                out.len() - start
            );
            return Err(Error::malformed(FORMAT, reason));
        }
    }
    Ok(out)
}

/// Compresses a file into a stream, in blocks of 8 KiB.
pub(crate) fn encode(data: &[u8]) -> Vec<u8> {
    let count = data.len().div_ceil(BLOCK_SIZE);
    let mut out = Vec::with_capacity(12 + count * 12 + data.len() / 2);
    for value in [data.len(), BLOCK_SIZE, count] {
        out.extend((value as u32).to_le_bytes());
    }
    for block in data.chunks(BLOCK_SIZE) {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        // Writing into memory cannot fail.
        encoder.write_all(block).and_then(|()| encoder.flush()).ok();
        let compressed = encoder.finish().unwrap_or_default();
        for value in [block.len(), compressed.len(), BLOCK_SIZE] {
            out.extend((value as u32).to_le_bytes());
        }
        out.extend(compressed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_across_blocks() {
        let data: Vec<u8> = (0..20_000u32).map(|i| (i * 7 + i / 256) as u8).collect();
        for data in [&data[..], &data[..BLOCK_SIZE], &data[..1], &[]] {
            let stream = encode(data);
            assert_eq!(scan(&stream, data.len()), Some(stream.len()));
            assert_eq!(decode(&stream).unwrap(), data);
        }
    }

    #[test]
    fn scan_rejects_other_data() {
        let stream = encode(b"hello");
        assert_eq!(scan(&stream, 6), None, "the catalog's size must match");
        assert_eq!(scan(&stream[..stream.len() - 1], 5), None);
        assert_eq!(scan(b"OggS\0\x02\0\0\0\0\0\0\0\0", 14), None);
    }

    #[test]
    fn decode_rejects_blocks_of_the_wrong_size() {
        let mut stream = encode(b"hello");
        stream[12] = 4;
        assert!(decode(&stream).is_err());
    }
}
