//! Writing archives, the counterpart of [`Archive`](crate::Archive).

use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;

use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Color;
use zenkit_rs_core::math::Mat3;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;

use crate::FORMAT;
use crate::Format;
use crate::Header;
use crate::binsafe;

impl Header {
    /// A header as the engine writes it for `format`, without a date or user, which the engine
    /// does not need.
    pub fn new(format: Format) -> Self {
        let archiver = match format {
            Format::Binsafe => "zCArchiverBinSafe",
            Format::Ascii | Format::Binary => "zCArchiverGeneric",
        };
        Self {
            version: 1,
            archiver: archiver.into(),
            format,
            save_game: false,
            date: None,
            user: None,
        }
    }
}

/// Writes an archive in any of the three encodings, value by value, in the order the engine
/// reads them back:
///
/// ```
/// # fn main() -> zenkit_rs_core::Result<()> {
/// use zenkit_rs_archive::ArchiveWriter;
/// use zenkit_rs_archive::Format;
///
/// let mut w = ArchiveWriter::new(Format::Ascii);
/// w.begin_object("%", "zCMaterial", 17408);
/// w.name("name", "BODY");
/// w.float("smoothAngle", 60.0);
/// w.end_object();
/// let bytes = w.finish()?;
/// # Ok(()) }
/// ```
///
/// Every value has a name, which ASCII and BIN_SAFE archives store and binary ones drop. Writing
/// into memory cannot fail, but a value can be impossible to store, like a BIN_SAFE string of
/// more than 65535 bytes. The first such problem is kept and returned by
/// [`finish`](Self::finish), so writing code needs no error handling of its own.
pub struct ArchiveWriter {
    header: Header,
    body: Writer,
    /// Where each open object starts, innermost last. Binary archives fill in the object's size
    /// there when it is closed.
    open: Vec<usize>,
    /// The index the next object gets. Null objects and references take none.
    next_index: u32,
    /// BIN_SAFE value names, in the order they were first used.
    keys: Vec<String>,
    key_indices: HashMap<String, u32>,
    error: Option<Error>,
}

impl fmt::Debug for ArchiveWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchiveWriter")
            .field("header", &self.header)
            .field("position", &self.body.position())
            .field("depth", &self.open.len())
            .finish_non_exhaustive()
    }
}

impl ArchiveWriter {
    pub fn new(format: Format) -> Self {
        Self::with_header(Header::new(format))
    }

    /// Writes an archive with this header, for example one copied from an archive that was read,
    /// to write it back the same way.
    pub fn with_header(header: Header) -> Self {
        Self {
            header,
            body: Writer::new(),
            open: Vec::new(),
            next_index: 0,
            keys: Vec::new(),
            key_indices: HashMap::new(),
            error: None,
        }
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn format(&self) -> Format {
        self.header.format
    }

    /// Opens an object of `class` in the field `name`, or `%` for none, and returns the index
    /// that [`reference`](Self::reference) can point back to it with.
    pub fn begin_object(&mut self, name: &str, class: &str, version: u16) -> u32 {
        let index = self.next_index;
        self.next_index += 1;
        self.object_header(name, class.as_bytes(), version, index);
        index
    }

    /// Closes the innermost open object.
    pub fn end_object(&mut self) {
        let Some(start) = self.open.pop() else {
            self.fail("closing an object while none is open");
            return;
        };
        match self.header.format {
            Format::Ascii => {
                self.indent();
                self.body.line(b"[]");
            }
            Format::Binary => {
                let size = self.body.position() - start;
                self.body.patch_u32(start, size as u32);
            }
            Format::Binsafe => self.tagged_string(b"[]"),
        }
    }

    /// Writes a null object pointer.
    pub fn null_object(&mut self, name: &str) {
        self.object_header(name, b"%", 0, 0);
        self.end_object();
    }

    /// Writes a pointer to the object written earlier with `index`.
    pub fn reference(&mut self, name: &str, index: u32) {
        self.object_header(name, b"\xA7", 0, index);
        self.end_object();
    }

    /// A string as raw bytes, in whatever code page the game was localised with.
    pub fn string(&mut self, key: &str, value: &[u8]) {
        match self.header.format {
            Format::Ascii => {
                if self.check_line(key, value) {
                    self.entry(key, "string", value);
                }
            }
            Format::Binary => {
                if self.check_line(key, value) {
                    self.body.string0(value);
                }
            }
            Format::Binsafe => {
                if let Some(len) = self.length(key, value.len()) {
                    self.tag(key, binsafe::STRING);
                    self.body.u16(len);
                    self.body.bytes(value);
                }
            }
        }
    }

    /// A string encoded as Latin-1, the counterpart of [`Archive::name`](crate::Archive::name).
    pub fn name(&mut self, key: &str, value: &str) {
        match text::to_latin1(value) {
            Some(bytes) => self.string(key, &bytes),
            None => self.fail(format!("{key:?} has characters beyond Latin-1: {value:?}")),
        }
    }

    pub fn int(&mut self, key: &str, value: i32) {
        match self.header.format {
            Format::Ascii => self.entry(key, "int", value.to_string().as_bytes()),
            Format::Binary => self.body.i32(value),
            Format::Binsafe => {
                self.tag(key, binsafe::INTEGER);
                self.body.i32(value);
            }
        }
    }

    pub fn float(&mut self, key: &str, value: f32) {
        match self.header.format {
            Format::Ascii => self.entry(key, "float", float(value).as_bytes()),
            Format::Binary => self.body.f32(value),
            Format::Binsafe => {
                self.tag(key, binsafe::FLOAT);
                self.body.f32(value);
            }
        }
    }

    // The engine writes bytes and words to ASCII archives as plain ints.
    pub fn byte(&mut self, key: &str, value: u8) {
        match self.header.format {
            Format::Ascii => self.int(key, value.into()),
            Format::Binary => self.body.u8(value),
            Format::Binsafe => {
                self.tag(key, binsafe::BYTE);
                self.body.u8(value);
            }
        }
    }

    pub fn word(&mut self, key: &str, value: u16) {
        match self.header.format {
            Format::Ascii => self.int(key, value.into()),
            Format::Binary => self.body.u16(value),
            Format::Binsafe => {
                self.tag(key, binsafe::WORD);
                self.body.u16(value);
            }
        }
    }

    /// An enum. Binary archives only keep its lowest byte.
    pub fn enumeration(&mut self, key: &str, value: u32) {
        match self.header.format {
            Format::Ascii => self.entry(key, "enum", value.to_string().as_bytes()),
            Format::Binary => self.body.u8(value as u8),
            Format::Binsafe => {
                self.tag(key, binsafe::ENUM);
                self.body.u32(value);
            }
        }
    }

    pub fn bool(&mut self, key: &str, value: bool) {
        match self.header.format {
            Format::Ascii => self.entry(key, "bool", if value { b"1" } else { b"0" }),
            Format::Binary => self.body.u8(value.into()),
            Format::Binsafe => {
                self.tag(key, binsafe::BOOL);
                self.body.u32(value.into());
            }
        }
    }

    pub fn color(&mut self, key: &str, value: Color) {
        let Color { r, g, b, a } = value;
        match self.header.format {
            Format::Ascii => self.entry(key, "color", format!("{r} {g} {b} {a}").as_bytes()),
            Format::Binary => self.body.bytes(&[b, g, r, a]),
            Format::Binsafe => {
                self.tag(key, binsafe::COLOR);
                self.body.bytes(&[b, g, r, a]);
            }
        }
    }

    pub fn vec2(&mut self, key: &str, value: Vec2) {
        self.raw_floats(key, &value.to_array());
    }

    pub fn vec3(&mut self, key: &str, value: Vec3) {
        match self.header.format {
            Format::Ascii => {
                let [x, y, z] = value.to_array().map(float);
                self.entry(key, "vec3", format!("{x} {y} {z}").as_bytes());
            }
            Format::Binary => self.body.vec3(value),
            Format::Binsafe => {
                self.tag(key, binsafe::VEC3);
                self.body.vec3(value);
            }
        }
    }

    pub fn aabb(&mut self, key: &str, value: Aabb) {
        let [x0, y0, z0] = value.min.to_array();
        let [x1, y1, z1] = value.max.to_array();
        self.raw_floats(key, &[x0, y0, z0, x1, y1, z1]);
    }

    /// A 3×3 matrix, stored row by row as raw bytes.
    pub fn mat3(&mut self, key: &str, value: Mat3) {
        let bytes: Vec<u8> =
            value.transpose().to_cols_array().iter().flat_map(|v| v.to_le_bytes()).collect();
        self.raw(key, &bytes);
    }

    /// Raw bytes. ASCII archives store them as hex.
    pub fn raw(&mut self, key: &str, value: &[u8]) {
        match self.header.format {
            Format::Ascii => self.entry(key, "raw", hex::encode(value).as_bytes()),
            Format::Binary => self.body.bytes(value),
            Format::Binsafe => {
                if let Some(len) = self.length(key, value.len()) {
                    self.tag(key, binsafe::RAW);
                    self.body.u16(len);
                    self.body.bytes(value);
                }
            }
        }
    }

    /// Floats stored as one value, the way vectors and bounding boxes are.
    pub fn raw_floats(&mut self, key: &str, values: &[f32]) {
        match self.header.format {
            Format::Ascii => {
                // Each float is followed by a space, the last one included, as the engine does.
                let mut list = String::new();
                for &v in values {
                    let _ = write!(list, "{} ", float(v));
                }
                self.entry(key, "rawFloat", list.as_bytes());
            }
            Format::Binary => values.iter().for_each(|&v| self.body.f32(v)),
            Format::Binsafe => {
                if let Some(len) = self.length(key, values.len() * 4) {
                    self.tag(key, binsafe::RAW_FLOAT);
                    self.body.u16(len);
                    values.iter().for_each(|&v| self.body.f32(v));
                }
            }
        }
    }

    /// Writes data straight into the stream instead of as values, like the mesh and BSP tree of a
    /// world. The counterpart of [`Archive::read_stream`](crate::Archive::read_stream).
    pub fn write_stream<T>(&mut self, write: impl FnOnce(&mut Writer) -> T) -> T {
        write(&mut self.body)
    }

    /// Records a problem found while writing, for [`finish`](Self::finish) to return. Only the
    /// first one is kept, since later ones tend to follow from it.
    pub fn fail(&mut self, reason: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some(Error::unwritable(FORMAT, reason));
        }
    }

    /// Puts the header in front of everything written and returns the archive's bytes, or the
    /// first problem found while writing.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        if !self.open.is_empty() {
            self.fail(format!("{} objects are still open", self.open.len()));
        }
        if let Some(error) = self.error {
            return Err(error);
        }

        let mut out = Writer::with_capacity(self.body.position() + 256);
        let header = &self.header;
        out.line(b"ZenGin Archive");
        out.line(format!("ver {}", header.version).as_bytes());
        out.line(header.archiver.as_bytes());
        out.line(match header.format {
            Format::Ascii => b"ASCII",
            Format::Binary => b"BINARY",
            Format::Binsafe => b"BIN_SAFE",
        });
        out.line(format!("saveGame {}", u8::from(header.save_game)).as_bytes());
        for (key, value) in [("date", &header.date), ("user", &header.user)] {
            if let Some(value) = value {
                out.line(format!("{key} {value}").as_bytes());
            }
        }
        out.line(b"END");

        match header.format {
            Format::Ascii | Format::Binary => {
                // Padded like the engine pads it, so that it can be filled in afterwards.
                out.line(format!("objects {:<9}", self.next_index).as_bytes());
                out.line(b"END");
                out.line(b"");
                out.bytes(self.body.as_bytes());
            }
            Format::Binsafe => {
                let table = out.position() + 12 + self.body.position();
                out.u32(2);
                out.u32(self.next_index);
                out.u32(table as u32);
                out.bytes(self.body.as_bytes());
                binsafe::write_keys(&mut out, &self.keys);
            }
        }
        Ok(out.into_inner())
    }

    /// `class` is bytes, since references are marked by the class `\xA7`, which is not UTF-8.
    fn object_header(&mut self, name: &str, class: &[u8], version: u16, index: u32) {
        let start = self.body.position();
        let text = || {
            let mut text = format!("[{name} ").into_bytes();
            text.extend(class);
            text.extend(format!(" {version} {index}]").as_bytes());
            text
        };
        match self.header.format {
            Format::Ascii => {
                self.indent();
                self.body.line(&text());
            }
            Format::Binary => {
                self.body.u32(0);
                self.body.u16(version);
                self.body.u32(index);
                self.body.string0(name.as_bytes());
                self.body.string0(class);
            }
            Format::Binsafe => self.tagged_string(&text()),
        }
        self.open.push(start);
    }

    fn indent(&mut self) {
        for _ in 0..self.open.len() {
            self.body.u8(b'\t');
        }
    }

    /// An ASCII line, `key=kind:value`.
    fn entry(&mut self, key: &str, kind: &str, value: &[u8]) {
        self.indent();
        self.body.bytes(key.as_bytes());
        self.body.u8(b'=');
        self.body.bytes(kind.as_bytes());
        self.body.u8(b':');
        self.body.line(value);
    }

    /// The `HASH` tag, key index and type tag every BIN_SAFE value starts with.
    fn tag(&mut self, key: &str, kind: u8) {
        let index = match self.key_indices.get(key) {
            Some(&index) => index,
            None => {
                let index = self.keys.len() as u32;
                if index > u16::MAX.into() {
                    self.fail(format!("more than {} different value names", u16::MAX));
                }
                self.keys.push(key.to_owned());
                self.key_indices.insert(key.to_owned(), index);
                index
            }
        };
        self.body.u8(binsafe::HASH);
        self.body.u32(index);
        self.body.u8(kind);
    }

    /// A BIN_SAFE string without a key, as objects begin and end with.
    fn tagged_string(&mut self, text: &[u8]) {
        self.body.u8(binsafe::STRING);
        self.body.u16(text.len() as u16);
        self.body.bytes(text);
    }

    /// The `u16` length BIN_SAFE strings and raw values are prefixed with.
    fn length(&mut self, key: &str, len: usize) -> Option<u16> {
        let fits = u16::try_from(len).ok();
        if fits.is_none() {
            self.fail(format!("{key:?} is {len} bytes long, at most {} fit", u16::MAX));
        }
        fits
    }

    /// Strings in ASCII and binary archives end at the first `\0`, `\r` or `\n`, so they cannot
    /// contain one.
    fn check_line(&mut self, key: &str, value: &[u8]) -> bool {
        let fits = !value.iter().any(|b| matches!(b, 0 | b'\r' | b'\n'));
        if !fits {
            let value = value.escape_ascii();
            self.fail(format!("{key:?} has a line break or a null byte: \"{value}\""));
        }
        fits
    }
}

/// A float in its shortest form that reads back as the same value, like `60` or `0.2`.
///
/// ZenKit writes floats with `%f`, which turns small values into `0.000000`. Rust's formatting
/// never loses a bit, and the engine's `atof` reads every number it produces.
fn float(value: f32) -> String {
    value.to_string()
}
