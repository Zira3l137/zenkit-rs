//! Every value is tagged:
//!
//! ```text
//! header   version: u32 | object count: u32 | key table offset: u32
//! value    HASH tag | key index: u32 | type tag | payload
//! object   STRING tag | length: u16 | "[name class version index]"
//! end      STRING tag | 2 | "[]"
//! ```
//!
//! Strings, raw bytes and raw floats are prefixed with a `u16` length; every other type has a
//! fixed size. The key table maps key indices to entry names, which are only kept for debugging.

use std::borrow::Cow;

use tracing::warn;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Color;
use zenkit_rs_core::math::Mat3;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;

use super::Decode;
use super::Mark;
use super::ObjectHeader;
use super::le_floats;
use super::malformed;
use super::mat3_from_rows;

const STRING: u8 = 0x01;
const INTEGER: u8 = 0x02;
const FLOAT: u8 = 0x03;
const BYTE: u8 = 0x04;
const WORD: u8 = 0x05;
const BOOL: u8 = 0x06;
const VEC3: u8 = 0x07;
const COLOR: u8 = 0x08;
const RAW: u8 = 0x09;
const RAW_FLOAT: u8 = 0x10;
const ENUM: u8 = 0x11;
const HASH: u8 = 0x12;

fn type_name(tag: u8) -> &'static str {
    match tag {
        STRING => "string",
        INTEGER => "int",
        FLOAT => "float",
        BYTE => "byte",
        WORD => "word",
        BOOL => "bool",
        VEC3 => "vec3",
        COLOR => "color",
        RAW => "raw",
        RAW_FLOAT => "rawFloat",
        ENUM => "enum",
        HASH => "hash",
        _ => "unknown",
    }
}

pub(super) struct Binsafe<'a> {
    r: Reader<'a>,
    keys: Vec<&'a [u8]>,
}

impl<'a> Binsafe<'a> {
    pub(super) fn new(mut r: Reader<'a>) -> Result<Self> {
        let _version = r.u32()?;
        let _object_count = r.u32()?;
        let table_offset = r.u32()? as usize;

        // The key table only names entries for error messages, so a broken one is not fatal.
        let keys = read_keys(r.clone(), table_offset).unwrap_or_else(|e| {
            warn!("ignoring unreadable key table: {e}");
            Vec::new()
        });

        // The key table follows the objects, so the objects end where it starts.
        if table_offset > r.position() && r.limit(table_offset).is_err() {
            warn!("key table offset {table_offset} lies past the end");
        }

        Ok(Self { r, keys })
    }

    /// Reads a value's key and type tags, returning its payload length.
    fn meta(&mut self, expected: u8) -> Result<usize> {
        let offset = self.r.offset();
        if self.r.u8()? != HASH {
            return Err(malformed(offset, "expected a value"));
        }
        let key = self.r.u32()? as usize;

        let found = self.r.u8()?;
        if found != expected {
            let key = self.keys.get(key).copied().unwrap_or_default().escape_ascii();
            let (expected, found) = (type_name(expected), type_name(found));
            return Err(malformed(
                offset,
                format!("expected {expected} for \"{key}\", found {found}"),
            ));
        }

        Ok(match expected {
            STRING | RAW | RAW_FLOAT => self.r.u16()? as usize,
            INTEGER | FLOAT | BOOL | ENUM | COLOR => 4,
            BYTE => 1,
            WORD => 2,
            VEC3 => 12,
            _ => unreachable!("only value tags are expected"),
        })
    }

    /// A raw or raw-float payload of at least `min` bytes. Extra bytes are skipped.
    fn payload(&mut self, tag: u8, min: usize) -> Result<&'a [u8]> {
        let offset = self.r.offset();
        let len = self.meta(tag)?;
        if len < min {
            return Err(malformed(
                offset,
                format!("{} entry has {len} bytes, needed {min}", type_name(tag)),
            ));
        }
        self.r.bytes(len)
    }

    /// The text of a string entry, if the next value is one, without consuming anything.
    fn peek_string(&self) -> Option<&'a [u8]> {
        let mut r = self.r.clone();
        if r.u8().ok()? != STRING {
            return None;
        }
        let len = r.u16().ok()? as usize;
        r.bytes(len).ok()
    }
}

impl<'a> Decode<'a> for Binsafe<'a> {
    fn mark(&self) -> Mark {
        Mark { pos: self.r.position(), depth: 0 }
    }

    fn reset(&mut self, mark: Mark) {
        self.r.seek(mark.pos).expect("mark lies within the input");
    }

    fn is_eof(&self) -> bool {
        self.r.is_eof()
    }

    fn position(&self) -> usize {
        self.r.position()
    }

    fn offset(&self) -> usize {
        self.r.offset()
    }

    fn begin_object(&mut self) -> Result<Option<ObjectHeader<'a>>> {
        let Some(text) = self.peek_string() else {
            return Ok(None);
        };
        let Some(header) = text.strip_prefix(b"[").and_then(parse_object_header) else {
            return Ok(None);
        };
        self.r.skip(3 + text.len())?;
        Ok(Some(header))
    }

    fn end_object(&mut self) -> Result<bool> {
        if self.r.is_eof() {
            return Ok(true);
        }
        if self.peek_string() == Some(b"[]") {
            self.r.skip(5)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn skip_entry(&mut self) -> Result<()> {
        let offset = self.r.offset();
        let len = match self.r.u8()? {
            STRING | RAW | RAW_FLOAT => self.r.u16()? as usize,
            INTEGER | FLOAT | BOOL | ENUM | HASH | COLOR => 4,
            BYTE => 1,
            WORD => 2,
            VEC3 => 12,
            tag => return Err(malformed(offset, format!("unknown type tag {tag:#04x}"))),
        };
        self.r.skip(len)
    }

    fn string(&mut self) -> Result<&'a [u8]> {
        let len = self.meta(STRING)?;
        self.r.bytes(len)
    }

    fn int(&mut self) -> Result<i32> {
        self.meta(INTEGER)?;
        self.r.i32()
    }

    fn float(&mut self) -> Result<f32> {
        self.meta(FLOAT)?;
        self.r.f32()
    }

    fn byte(&mut self) -> Result<u8> {
        self.meta(BYTE)?;
        self.r.u8()
    }

    fn word(&mut self) -> Result<u16> {
        self.meta(WORD)?;
        self.r.u16()
    }

    fn enumeration(&mut self) -> Result<u32> {
        self.meta(ENUM)?;
        self.r.u32()
    }

    fn bool(&mut self) -> Result<bool> {
        self.meta(BOOL)?;
        self.r.u32().map(|v| v != 0)
    }

    fn color(&mut self) -> Result<Color> {
        self.meta(COLOR)?;
        let [b, g, r, a] = self.r.array()?;
        Ok(Color { r, g, b, a })
    }

    fn vec2(&mut self) -> Result<Vec2> {
        self.payload(RAW_FLOAT, 8).map(|p| Vec2::from_array(le_floats(p)))
    }

    fn vec3(&mut self) -> Result<Vec3> {
        self.meta(VEC3)?;
        Ok(Vec3::from_array(le_floats(self.r.bytes(12)?)))
    }

    fn aabb(&mut self) -> Result<Aabb> {
        let [x0, y0, z0, x1, y1, z1] = le_floats(self.payload(RAW_FLOAT, 24)?);
        Ok(Aabb { min: Vec3::new(x0, y0, z0), max: Vec3::new(x1, y1, z1) })
    }

    fn mat3(&mut self) -> Result<Mat3> {
        self.payload(RAW, 36).map(|p| mat3_from_rows(le_floats(p)))
    }

    fn raw(&mut self, len: usize) -> Result<Cow<'a, [u8]>> {
        self.payload(RAW, len).map(Cow::Borrowed)
    }
}

fn read_keys(mut table: Reader<'_>, offset: usize) -> Result<Vec<&[u8]>> {
    table.seek(offset)?;
    let count = table.u32()? as usize;

    let mut keys = vec![&b""[..]; count.min(table.remaining() / 8)];
    for _ in 0..count {
        let len = table.u16()? as usize;
        let index = table.u16()? as usize;
        let _hash = table.u32()?;
        let key = table.bytes(len)?;
        let slot = keys
            .get_mut(index)
            .ok_or_else(|| malformed(table.offset(), format!("key index {index} out of range")))?;
        *slot = key;
    }
    Ok(keys)
}

/// Parses `name class version index]`. Tokens are separated by spaces or `]`, as with C's
/// `strtok(s, " ]")`.
fn parse_object_header(text: &[u8]) -> Option<ObjectHeader<'_>> {
    let mut parts = text.split(|&b| b == b' ' || b == b']').filter(|p| !p.is_empty());
    let name = parts.next()?;
    let class = parts.next()?;
    let version = text::int(parts.next()?)? as u16;
    let index = text::int(parts.next()?)? as u32;
    Some(ObjectHeader { name, class, version, index })
}
