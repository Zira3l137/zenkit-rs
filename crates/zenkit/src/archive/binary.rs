//! Bare values with nothing but object boundaries in between:
//!
//! ```text
//! object   size: u32 | version: u16 | index: u32 | name\0 | class\0 | contents...
//! ```
//!
//! `size` counts from the start of the object, so the reader knows where every object ends even
//! though values carry no length. Enums are a single byte here.

use std::borrow::Cow;

use super::Decode;
use super::Mark;
use super::ObjectHeader;
use super::WHITESPACE;
use super::le_floats;
use super::malformed;
use super::mat3_from_rows;
use super::parse;
use crate::Error;
use crate::Result;
use crate::io::Reader;
use crate::math::Aabb;
use crate::math::Color;
use crate::math::Mat3;
use crate::math::Vec2;
use crate::math::Vec3;

pub(super) struct Binary<'a> {
    r: Reader<'a>,
    /// Where each open object ends, innermost last.
    ends: Vec<usize>,
}

impl<'a> Binary<'a> {
    pub(super) fn new(mut r: Reader<'a>) -> Result<Self> {
        let objects = r.line(WHITESPACE);
        if super::field(objects, "objects").and_then(parse::int).is_none() {
            return Err(malformed(r.position(), "missing \"objects\" line"));
        }
        // Only newlines may follow, because the binary data starts right after them.
        if r.line(b"\n") != b"END" {
            return Err(malformed(r.position(), "missing second \"END\" line"));
        }
        Ok(Self { r, ends: Vec::new() })
    }
}

impl<'a> Decode<'a> for Binary<'a> {
    fn mark(&self) -> Mark {
        Mark { pos: self.r.position(), depth: self.ends.len() }
    }

    fn reset(&mut self, mark: Mark) {
        self.r.seek(mark.pos).expect("mark lies within the input");
        self.ends.truncate(mark.depth);
    }

    fn is_eof(&self) -> bool {
        self.r.is_eof()
    }

    fn position(&self) -> usize {
        self.r.position()
    }

    fn begin_object(&mut self) -> Result<Option<ObjectHeader<'a>>> {
        if self.r.is_eof() {
            return Ok(None);
        }

        let start = self.r.position();
        let size = self.r.u32()? as usize;
        let end = start
            .checked_add(size)
            .filter(|&end| end <= self.r.len())
            .ok_or_else(|| malformed(start, format!("object of {size} bytes runs past the end")))?;

        let version = self.r.u16()?;
        let index = self.r.u32()?;
        let name = self.r.line(&[]);
        let class = self.r.line(&[]);

        self.ends.push(end);
        Ok(Some(ObjectHeader { name, class, version, index }))
    }

    fn end_object(&mut self) -> Result<bool> {
        if self.ends.last() == Some(&self.r.position()) {
            self.ends.pop();
            return Ok(true);
        }
        Ok(self.r.is_eof())
    }

    fn skip_entry(&mut self) -> Result<()> {
        Err(Error::Unsupported {
            format: super::FORMAT,
            reason: "skipping single values of a binary archive".into(),
        })
    }

    fn skip_object(&mut self) -> Result<()> {
        let end =
            self.ends.pop().ok_or_else(|| malformed(self.r.position(), "no object is open"))?;
        self.r.seek(end)
    }

    fn skip_next_object(&mut self) -> Result<()> {
        let start = self.r.position();
        let size = self.r.u32()? as usize;
        self.r.seek(start.saturating_add(size))
    }

    fn string(&mut self) -> Result<&'a [u8]> {
        Ok(self.r.line(&[]))
    }

    fn int(&mut self) -> Result<i32> {
        self.r.i32()
    }

    fn float(&mut self) -> Result<f32> {
        self.r.f32()
    }

    fn byte(&mut self) -> Result<u8> {
        self.r.u8()
    }

    fn word(&mut self) -> Result<u16> {
        self.r.u16()
    }

    fn enumeration(&mut self) -> Result<u32> {
        self.r.u8().map(u32::from)
    }

    fn bool(&mut self) -> Result<bool> {
        self.r.u8().map(|v| v != 0)
    }

    fn color(&mut self) -> Result<Color> {
        let [b, g, r, a] = self.r.array()?;
        Ok(Color { r, g, b, a })
    }

    fn vec2(&mut self) -> Result<Vec2> {
        Ok(Vec2::from_array(le_floats(self.r.bytes(8)?)))
    }

    fn vec3(&mut self) -> Result<Vec3> {
        Ok(Vec3::from_array(le_floats(self.r.bytes(12)?)))
    }

    fn aabb(&mut self) -> Result<Aabb> {
        let [x0, y0, z0, x1, y1, z1] = le_floats(self.r.bytes(24)?);
        Ok(Aabb { min: Vec3::new(x0, y0, z0), max: Vec3::new(x1, y1, z1) })
    }

    fn mat3(&mut self) -> Result<Mat3> {
        Ok(mat3_from_rows(le_floats(self.r.bytes(36)?)))
    }

    fn raw(&mut self, len: usize) -> Result<Cow<'a, [u8]>> {
        self.r.bytes(len).map(Cow::Borrowed)
    }
}
