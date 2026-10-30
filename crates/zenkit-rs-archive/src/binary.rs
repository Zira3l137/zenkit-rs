//! Bare values with nothing but object boundaries in between:
//!
//! ```text
//! object   size: u32 | version: u16 | index: u32 | name\0 | class\0 | contents...
//! ```
//!
//! `size` counts from the start of the object, so the reader knows where every object ends even
//! though values carry no length. Enums are a single byte here.
//!
//! The data starts right after the header's last `END` line and an optional blank line.

use std::borrow::Cow;

use zenkit_rs_core::Error;
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
use super::WHITESPACE;
use super::le_floats;
use super::malformed;
use super::mat3_from_rows;

pub(super) struct Binary<'a> {
    r: Reader<'a>,
    /// Where each open object ends, innermost last.
    ends: Vec<usize>,
}

impl<'a> Binary<'a> {
    pub(super) fn new(mut r: Reader<'a>) -> Result<Self> {
        let objects = r.line(WHITESPACE);
        if super::field(objects, "objects").and_then(text::int).is_none() {
            return Err(malformed(r.offset(), "missing \"objects\" line"));
        }
        let offset = r.offset();
        let rest = r.rest();
        let after = rest
            .strip_prefix(b"END")
            .map(|rest| rest.strip_prefix(b"\r").unwrap_or(rest))
            .and_then(|rest| rest.strip_prefix(b"\n"))
            .ok_or_else(|| malformed(offset, "missing second \"END\" line"))?;
        // Writers like ZenKit add a blank line, but when the data starts with an object, its
        // first byte is the low byte of the object's size and can just as well be 0x0A. The
        // newline is only taken as data if that alone leaves an object that fits.
        let blank = after.strip_prefix(b"\r\n").or(after.strip_prefix(b"\n"));
        let after = match blank {
            Some(blank) if !(fits_object(after) && !fits_object(blank)) => blank,
            _ => after,
        };
        r.skip(rest.len() - after.len())?;
        Ok(Self { r, ends: Vec::new() })
    }
}

/// Whether `data` starts with an object header whose size, name and class fit inside it.
fn fits_object(data: &[u8]) -> bool {
    let Some(size) = data.get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
    else {
        return false;
    };
    let Some(object) = data.get(..size).and_then(|object| object.get(10..)) else {
        return false;
    };
    object.iter().filter(|&&b| b == 0).count() >= 2
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

    fn offset(&self) -> usize {
        self.r.offset()
    }

    fn begin_object(&mut self) -> Result<Option<ObjectHeader<'a>>> {
        if self.r.is_eof() {
            return Ok(None);
        }

        let start = self.r.position();
        let size = self.r.u32()? as usize;
        let end = start.checked_add(size).filter(|&end| end <= self.r.len()).ok_or_else(|| {
            let offset = self.r.offset() - 4;
            malformed(offset, format!("object of {size} bytes runs past the end"))
        })?;

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
        let end = self.ends.pop().ok_or_else(|| malformed(self.r.offset(), "no object is open"))?;
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

    fn stream(&mut self) -> &mut Reader<'a> {
        &mut self.r
    }
}
