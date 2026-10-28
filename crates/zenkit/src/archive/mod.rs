//! ZenGin archives, the engine's general object serialisation format.
//!
//! Worlds, materials, savegames and cutscenes are all stored as archives. An archive is a short
//! text header followed by a stream of objects, each holding a sequence of values. The stream
//! comes in one of three encodings:
//!
//! - [`Format::Ascii`]: readable text, one `name=type:value` line per value.
//! - [`Format::Binsafe`]: binary, but every value is tagged with its type and name.
//! - [`Format::Binary`]: bare values with no names or types at all.
//!
//! Because the binary encoding carries no names, values can only be read *positionally*: the code
//! reading an object must ask for each field in the order the engine wrote it, with the right
//! type. [`Archive`] gives every encoding that same positional interface:
//!
//! ```no_run
//! # fn main() -> zenkit::Result<()> {
//! # let bytes = Vec::new();
//! use zenkit::archive::Archive;
//!
//! let mut archive = Archive::new(&bytes)?;
//! let object = archive.expect_object(b"zCMaterial")?;
//! let name = archive.string()?;
//! let group = archive.enumeration()?;
//! archive.end_object()?;
//! # Ok(()) }
//! ```
//!
//! A read that fails leaves the archive exactly where it was, so a caller can recover by trying a
//! different type or skipping the object.

mod ascii;
mod binary;
mod binsafe;

use std::borrow::Cow;
use std::fmt;

use crate::Error;
use crate::Result;
use crate::io::Reader;
use crate::io::WHITESPACE;
use crate::math::Aabb;
use crate::math::Color;
use crate::math::Mat3;
use crate::math::Vec2;
use crate::math::Vec3;
use crate::text;

const FORMAT: &str = "ZenGin archive";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Ascii,
    Binary,
    Binsafe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub version: i32,
    /// The engine class that wrote the archive, like `zCArchiverGeneric`.
    pub archiver: String,
    pub format: Format,
    pub save_game: bool,
    pub date: Option<String>,
    pub user: Option<String>,
}

/// The line that opens an object, like `[% zCMaterial 17408 0]`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectHeader<'a> {
    /// The name of the field this object is stored in, or `%` if it has none.
    pub name: &'a [u8],
    /// The engine class, including its base classes, like `oCMobFire:oCMobInter:oCMOB:zCVob`.
    pub class: &'a [u8],
    /// The class version, which tells the game version and layout apart.
    pub version: u16,
    /// The object's index in the archive, used by [references](Self::is_reference).
    pub index: u32,
}

impl ObjectHeader<'_> {
    /// Whether this is a back-reference to the object read earlier with the same index. It has
    /// no contents of its own.
    pub fn is_reference(&self) -> bool {
        self.class == b"\xA7"
    }

    /// Whether this stands for a null pointer. It has no contents.
    pub fn is_null(&self) -> bool {
        self.class == b"%"
    }
}

impl fmt::Debug for ObjectHeader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectHeader")
            .field("name", &self.name.escape_ascii().to_string())
            .field("class", &self.class.escape_ascii().to_string())
            .field("version", &self.version)
            .field("index", &self.index)
            .finish()
    }
}

/// A position to rewind to after a failed read.
#[derive(Clone, Copy)]
struct Mark {
    pos: usize,
    depth: usize,
}

/// What every encoding implements. [`Archive`] adds the rewind-on-error guarantee on top.
trait Decode<'a> {
    fn mark(&self) -> Mark;
    fn reset(&mut self, mark: Mark);
    fn is_eof(&self) -> bool;
    fn position(&self) -> usize;

    fn begin_object(&mut self) -> Result<Option<ObjectHeader<'a>>>;
    fn end_object(&mut self) -> Result<bool>;
    fn skip_entry(&mut self) -> Result<()>;

    /// Skips to the end of the current object by counting nested begins and ends.
    fn skip_object(&mut self) -> Result<()> {
        skip_levels(self, 1)
    }

    fn skip_next_object(&mut self) -> Result<()> {
        skip_levels(self, 0)
    }

    fn string(&mut self) -> Result<&'a [u8]>;
    fn int(&mut self) -> Result<i32>;
    fn float(&mut self) -> Result<f32>;
    fn byte(&mut self) -> Result<u8>;
    fn word(&mut self) -> Result<u16>;
    fn enumeration(&mut self) -> Result<u32>;
    fn bool(&mut self) -> Result<bool>;
    fn color(&mut self) -> Result<Color>;
    fn vec2(&mut self) -> Result<Vec2>;
    fn vec3(&mut self) -> Result<Vec3>;
    fn aabb(&mut self) -> Result<Aabb>;
    fn mat3(&mut self) -> Result<Mat3>;
    fn raw(&mut self, len: usize) -> Result<Cow<'a, [u8]>>;
}

fn skip_levels<'a, D: Decode<'a> + ?Sized>(d: &mut D, mut level: usize) -> Result<()> {
    loop {
        if d.is_eof() {
            return Err(malformed(d.position(), "data ends inside an object"));
        }
        if d.begin_object()?.is_some() {
            level += 1;
        } else if d.end_object()? {
            level = level.saturating_sub(1);
        } else {
            d.skip_entry()?;
        }
        if level == 0 {
            return Ok(());
        }
    }
}

enum Backend<'a> {
    Ascii(ascii::Ascii<'a>),
    Binary(binary::Binary<'a>),
    Binsafe(binsafe::Binsafe<'a>),
}

/// Runs a [`Decode`] method on whichever encoding is active, rewinding if it fails.
macro_rules! decode {
    ($self:ident.$method:ident($($arg:expr),*)) => {
        match &mut $self.backend {
            Backend::Ascii(d) => atomic(d, |d| d.$method($($arg),*)),
            Backend::Binary(d) => atomic(d, |d| d.$method($($arg),*)),
            Backend::Binsafe(d) => atomic(d, |d| d.$method($($arg),*)),
        }
    };
}

fn atomic<'a, D: Decode<'a>, T>(d: &mut D, f: impl FnOnce(&mut D) -> Result<T>) -> Result<T> {
    let mark = d.mark();
    let result = f(d);
    if result.is_err() {
        d.reset(mark);
    }
    result
}

pub struct Archive<'a> {
    header: Header,
    backend: Backend<'a>,
}

impl<'a> Archive<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let header = read_header(&mut r)?;
        let backend = match header.format {
            Format::Ascii => Backend::Ascii(ascii::Ascii::new(r)?),
            Format::Binary => Backend::Binary(binary::Binary::new(r)?),
            Format::Binsafe => Backend::Binsafe(binsafe::Binsafe::new(r)?),
        };
        Ok(Self { header, backend })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn format(&self) -> Format {
        self.header.format
    }

    /// The byte offset of the next read, for error messages and debugging.
    pub fn position(&self) -> usize {
        match &self.backend {
            Backend::Ascii(d) => d.position(),
            Backend::Binary(d) => d.position(),
            Backend::Binsafe(d) => d.position(),
        }
    }

    pub fn is_eof(&self) -> bool {
        match &self.backend {
            Backend::Ascii(d) => d.is_eof(),
            Backend::Binary(d) => d.is_eof(),
            Backend::Binsafe(d) => d.is_eof(),
        }
    }

    /// Opens the next object, or returns `None` if the next item is not an object start.
    ///
    /// Binary archives cannot tell objects and values apart, so there this always succeeds
    /// unless the data has ended. Only call it where the format says an object follows.
    pub fn begin_object(&mut self) -> Result<Option<ObjectHeader<'a>>> {
        decode!(self.begin_object())
    }

    /// Opens the next object and checks its class.
    pub fn expect_object(&mut self, class: &[u8]) -> Result<ObjectHeader<'a>> {
        let position = self.position();
        let mark = self.mark();
        match self.begin_object()? {
            Some(object) if object.class == class => Ok(object),
            found => {
                self.reset(mark);
                let found = found.map_or("a value".into(), |o| {
                    format!("{:?}", o.class.escape_ascii().to_string())
                });
                let expected = class.escape_ascii();
                Err(malformed(position, format!("expected object \"{expected}\", found {found}")))
            }
        }
    }

    /// Closes the current object. Returns `false`, consuming nothing, if the object has
    /// unread contents left.
    pub fn end_object(&mut self) -> Result<bool> {
        decode!(self.end_object())
    }

    /// Skips everything left in the current object, including its end.
    pub fn skip_object(&mut self) -> Result<()> {
        decode!(self.skip_object())
    }

    /// Skips the next object, which must start right here.
    pub fn skip_next_object(&mut self) -> Result<()> {
        decode!(self.skip_next_object())
    }

    /// Skips one value. Binary archives do not record how large a value is, so this fails for
    /// them.
    pub fn skip_entry(&mut self) -> Result<()> {
        decode!(self.skip_entry())
    }

    /// A string as raw bytes, in whatever code page the game was localised with.
    pub fn string(&mut self) -> Result<&'a [u8]> {
        decode!(self.string())
    }

    /// A string decoded as Latin-1, which never fails and keeps every byte. Right for names of
    /// objects, files and textures, which are what [`Vfs`](crate::vfs::Vfs) lookups expect too.
    pub fn name(&mut self) -> Result<String> {
        self.string().map(text::latin1)
    }

    pub fn int(&mut self) -> Result<i32> {
        decode!(self.int())
    }

    pub fn float(&mut self) -> Result<f32> {
        decode!(self.float())
    }

    pub fn byte(&mut self) -> Result<u8> {
        decode!(self.byte())
    }

    pub fn word(&mut self) -> Result<u16> {
        decode!(self.word())
    }

    pub fn enumeration(&mut self) -> Result<u32> {
        decode!(self.enumeration())
    }

    pub fn bool(&mut self) -> Result<bool> {
        decode!(self.bool())
    }

    pub fn color(&mut self) -> Result<Color> {
        decode!(self.color())
    }

    pub fn vec2(&mut self) -> Result<Vec2> {
        decode!(self.vec2())
    }

    pub fn vec3(&mut self) -> Result<Vec3> {
        decode!(self.vec3())
    }

    pub fn aabb(&mut self) -> Result<Aabb> {
        decode!(self.aabb())
    }

    pub fn mat3(&mut self) -> Result<Mat3> {
        decode!(self.mat3())
    }

    /// At least `len` raw bytes. ASCII archives store them as hex, so they come back owned; the
    /// binary encodings borrow them from the input.
    pub fn raw(&mut self, len: usize) -> Result<Cow<'a, [u8]>> {
        decode!(self.raw(len))
    }

    fn mark(&self) -> Mark {
        match &self.backend {
            Backend::Ascii(d) => d.mark(),
            Backend::Binary(d) => d.mark(),
            Backend::Binsafe(d) => d.mark(),
        }
    }

    fn reset(&mut self, mark: Mark) {
        match &mut self.backend {
            Backend::Ascii(d) => d.reset(mark),
            Backend::Binary(d) => d.reset(mark),
            Backend::Binsafe(d) => d.reset(mark),
        }
    }
}

fn malformed(position: usize, reason: impl fmt::Display) -> Error {
    Error::malformed(FORMAT, format!("{reason} (at offset {position})"))
}

fn read_header(r: &mut Reader) -> Result<Header> {
    let mut next = || r.line(WHITESPACE);

    if next() != b"ZenGin Archive" {
        return Err(malformed(0, "missing \"ZenGin Archive\" magic"));
    }

    let version = field(next(), "ver")
        .and_then(parse::int)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| malformed(0, "missing \"ver\" line"))?;
    if version != 1 {
        return Err(Error::Unsupported { format: FORMAT, reason: format!("version {version}") });
    }

    let archiver = text::latin1(next());

    let format = match next() {
        b"ASCII" => Format::Ascii,
        b"BINARY" => Format::Binary,
        b"BIN_SAFE" => Format::Binsafe,
        other => {
            return Err(Error::Unsupported {
                format: FORMAT,
                reason: format!("encoding \"{}\"", other.escape_ascii()),
            });
        }
    };

    let save_game = field(next(), "saveGame")
        .and_then(parse::int)
        .ok_or_else(|| malformed(0, "missing \"saveGame\" line"))?
        != 0;

    let mut optional = next();
    let mut date = None;
    let mut user = None;
    if let Some(value) = field(optional, "date") {
        date = Some(text::latin1(value));
        optional = next();
    }
    if let Some(value) = field(optional, "user") {
        user = Some(text::latin1(value));
        optional = next();
    }
    if optional != b"END" {
        return Err(malformed(0, "header is missing its \"END\" line"));
    }

    Ok(Header { version, archiver, format, save_game, date, user })
}

/// The value of a `key value` line, if the line starts with `key `.
fn field<'a>(line: &'a [u8], key: &str) -> Option<&'a [u8]> {
    line.strip_prefix(key.as_bytes())?.strip_prefix(b" ")
}

/// Number parsing that behaves like the C functions ZenKit uses: leading whitespace is skipped,
/// and parsing stops at the first character that cannot continue the number. Objects need it too,
/// since a few store numbers inside strings.
pub(crate) mod parse {
    pub(crate) fn int(s: &[u8]) -> Option<i64> {
        let s = s.trim_ascii_start();
        let sign = usize::from(matches!(s.first(), Some(b'+' | b'-')));
        let digits = s[sign..].iter().take_while(|b| b.is_ascii_digit()).count();
        std::str::from_utf8(&s[..sign + digits]).ok()?.parse().ok()
    }

    pub(crate) fn float(s: &[u8]) -> Option<f32> {
        let s = s.trim_ascii_start();
        let candidate = s
            .iter()
            .take_while(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
            .count();
        // The longest prefix that parses is the one C's `strtof` would have taken.
        (1..=candidate).rev().find_map(|end| std::str::from_utf8(&s[..end]).ok()?.parse().ok())
    }

    /// Whitespace-separated floats; missing ones read as zero, like a failed C++ stream read.
    pub(crate) fn floats<const N: usize>(s: &[u8]) -> [f32; N] {
        let mut parts = s.split(u8::is_ascii_whitespace).filter(|p| !p.is_empty());
        std::array::from_fn(|_| parts.next().and_then(float).unwrap_or(0.0))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn ints_stop_at_the_first_non_digit() {
            assert_eq!(int(b"  42"), Some(42));
            assert_eq!(int(b"-7 rest"), Some(-7));
            assert_eq!(int(b"12]"), Some(12));
            assert_eq!(int(b"x"), None);
            assert_eq!(int(b"-"), None);
        }

        #[test]
        fn floats_take_the_longest_valid_prefix() {
            assert_eq!(float(b"12.5"), Some(12.5));
            assert_eq!(float(b"9.9999997e-005"), Some(1e-4));
            assert_eq!(float(b"1.5e"), Some(1.5));
            assert_eq!(float(b"-3x"), Some(-3.0));
            assert_eq!(float(b"abc"), None);
        }

        #[test]
        fn missing_floats_are_zero() {
            assert_eq!(floats::<3>(b"1 2"), [1.0, 2.0, 0.0]);
            assert_eq!(floats::<2>(b" 512   256 extra"), [512.0, 256.0]);
        }
    }
}

/// Builds a [`Mat3`] from nine floats stored row by row, as the engine writes them.
fn mat3_from_rows(values: [f32; 9]) -> Mat3 {
    Mat3::from_cols_array(&values).transpose()
}

fn le_floats<const N: usize>(bytes: &[u8]) -> [f32; N] {
    std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_with_optional_fields() {
        let data = b"ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 1\n\
                     date 1.1.2001\nuser luis\nEND\nobjects 0\nEND\n";
        let archive = Archive::new(data).unwrap();
        let header = archive.header();
        assert_eq!(header.archiver, "zCArchiverGeneric");
        assert_eq!(header.format, Format::Ascii);
        assert!(header.save_game);
        assert_eq!(header.date.as_deref(), Some("1.1.2001"));
        assert_eq!(header.user.as_deref(), Some("luis"));
        assert!(archive.is_eof());
    }

    #[test]
    fn rejects_bad_headers() {
        let cases: [&[u8]; 4] = [
            b"Not An Archive\n",
            b"ZenGin Archive\nver 2\nx\nASCII\nsaveGame 0\nEND\n",
            b"ZenGin Archive\nver 1\nx\nXML\nsaveGame 0\nEND\n",
            b"ZenGin Archive\nver 1\nx\nASCII\nsaveGame 0\nobjects 0\n",
        ];
        for data in cases {
            assert!(Archive::new(data).is_err(), "{}", data.escape_ascii());
        }
    }

    #[test]
    fn mat3_rows_become_glam_rows() {
        let m = mat3_from_rows([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert_eq!(m.row(0), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(m.col(0), Vec3::new(1.0, 4.0, 7.0));
    }
}
