//! ```text
//! objects 2
//! END
//!
//! [% zCMaterial 17408 0]
//!     name=string:BODY
//!     smoothAngle=float:60
//! []
//! ```
//!
//! Entry names are ignored, as in ZenKit; only the type tag is checked.

use std::borrow::Cow;

use super::Decode;
use super::Mark;
use super::ObjectHeader;
use super::WHITESPACE;
use super::le_floats;
use super::malformed;
use super::mat3_from_rows;
use super::parse;
use crate::Result;
use crate::io::Reader;
use crate::math::Aabb;
use crate::math::Color;
use crate::math::Mat3;
use crate::math::Vec2;
use crate::math::Vec3;

pub(super) struct Ascii<'a> {
    r: Reader<'a>,
}

impl<'a> Ascii<'a> {
    pub(super) fn new(mut r: Reader<'a>) -> Result<Self> {
        let objects = r.line(WHITESPACE);
        if super::field(objects, "objects").and_then(parse::int).is_none() {
            return Err(malformed(r.offset(), "missing \"objects\" line"));
        }
        if r.line(WHITESPACE) != b"END" {
            return Err(malformed(r.offset(), "missing second \"END\" line"));
        }
        Ok(Self { r })
    }

    /// The value of the next `name=type:value` line, after checking its type.
    fn entry(&mut self, kind: &str) -> Result<&'a [u8]> {
        let offset = self.r.offset();
        let line = self.r.line(WHITESPACE);
        let entry = line.iter().position(|&b| b == b'=').map_or(line, |i| &line[i + 1..]);
        let (found, value) = entry
            .iter()
            .position(|&b| b == b':')
            .map_or((entry, &b""[..]), |i| (&entry[..i], &entry[i + 1..]));

        if found != kind.as_bytes() {
            let found = found.escape_ascii();
            return Err(malformed(offset, format!("expected a {kind} entry, found \"{found}\"")));
        }
        Ok(value)
    }

    fn integer(&mut self, kind: &str) -> Result<i64> {
        let offset = self.r.offset();
        let value = self.entry(kind)?;
        parse::int(value).ok_or_else(|| {
            malformed(offset, format!("invalid {kind} \"{}\"", value.escape_ascii()))
        })
    }

    fn hex(&mut self) -> Result<Vec<u8>> {
        let offset = self.r.offset();
        let value = self.entry("raw")?;
        let even = &value[..value.len() & !1];
        hex::decode(even).map_err(|e| malformed(offset, format!("invalid raw entry: {e}")))
    }
}

impl<'a> Decode<'a> for Ascii<'a> {
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
        let mark = self.mark();
        let found = self.r.line(WHITESPACE).strip_prefix(b"[").and_then(parse_object_header);
        if found.is_none() {
            self.reset(mark);
        }
        Ok(found)
    }

    fn end_object(&mut self) -> Result<bool> {
        if self.r.is_eof() {
            return Ok(false);
        }
        let mark = self.mark();
        let found = self.r.line(WHITESPACE).trim_ascii_start() == b"[]";
        if !found {
            self.reset(mark);
        }
        Ok(found)
    }

    fn skip_entry(&mut self) -> Result<()> {
        self.r.line(WHITESPACE);
        Ok(())
    }

    fn string(&mut self) -> Result<&'a [u8]> {
        self.entry("string")
    }

    fn int(&mut self) -> Result<i32> {
        let offset = self.r.offset();
        let value = self.integer("int")?;
        i32::try_from(value).map_err(|_| malformed(offset, format!("int {value} is out of range")))
    }

    fn float(&mut self) -> Result<f32> {
        let offset = self.r.offset();
        let value = self.entry("float")?;
        parse::float(value)
            .ok_or_else(|| malformed(offset, format!("invalid float \"{}\"", value.escape_ascii())))
    }

    // The engine writes bytes, words and enums as plain ints and truncates them when reading.
    fn byte(&mut self) -> Result<u8> {
        self.integer("int").map(|v| v as u8)
    }

    fn word(&mut self) -> Result<u16> {
        self.integer("int").map(|v| v as u16)
    }

    fn enumeration(&mut self) -> Result<u32> {
        self.integer("enum").map(|v| v as u32)
    }

    fn bool(&mut self) -> Result<bool> {
        self.integer("bool").map(|v| v != 0)
    }

    fn color(&mut self) -> Result<Color> {
        let value = self.entry("color")?;
        let mut parts = value.split(u8::is_ascii_whitespace).filter(|p| !p.is_empty());
        let [r, g, b, a] =
            std::array::from_fn(|_| parts.next().and_then(parse::int).unwrap_or(0) as u8);
        Ok(Color { r, g, b, a })
    }

    fn vec2(&mut self) -> Result<Vec2> {
        self.entry("rawFloat").map(|v| Vec2::from_array(parse::floats(v)))
    }

    fn vec3(&mut self) -> Result<Vec3> {
        self.entry("vec3").map(|v| Vec3::from_array(parse::floats(v)))
    }

    fn aabb(&mut self) -> Result<Aabb> {
        let [x0, y0, z0, x1, y1, z1] = parse::floats(self.entry("rawFloat")?);
        Ok(Aabb { min: Vec3::new(x0, y0, z0), max: Vec3::new(x1, y1, z1) })
    }

    fn mat3(&mut self) -> Result<Mat3> {
        let offset = self.r.offset();
        let bytes = self.hex()?;
        if bytes.len() < 36 {
            return Err(malformed(offset, "raw entry is too short for a 3x3 matrix"));
        }
        Ok(mat3_from_rows(le_floats(&bytes)))
    }

    fn raw(&mut self, len: usize) -> Result<Cow<'a, [u8]>> {
        let offset = self.r.offset();
        let bytes = self.hex()?;
        if bytes.len() < len {
            return Err(malformed(
                offset,
                format!("raw entry has {} bytes, needed {len}", bytes.len()),
            ));
        }
        Ok(Cow::Owned(bytes))
    }
}

/// Parses `name class version index]`, the part after the opening bracket.
fn parse_object_header(line: &[u8]) -> Option<ObjectHeader<'_>> {
    let mut parts = line.split(u8::is_ascii_whitespace).filter(|p| !p.is_empty());
    let name = parts.next()?;
    let class = parts.next()?;
    let version = parse::int(parts.next()?)? as u16;
    let index = parse::int(parts.next()?)? as u32;
    Some(ObjectHeader { name, class, version, index })
}
