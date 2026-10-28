use zerocopy::{FromBytes, Immutable, KnownLayout, Unaligned};

use crate::{Error, Result};

/// A fixed-layout record that can be viewed straight from bytes. Derive it on `#[repr(C)]` structs
/// built from byte arrays and `zerocopy::little_endian` integers.
pub trait Record: FromBytes + KnownLayout + Immutable + Unaligned {}

impl<T: FromBytes + KnownLayout + Immutable + Unaligned> Record for T {}

/// A little-endian cursor over a byte slice.
///
/// Unlike ZenKit's `Read`, which silently yields zeros past the end of its input, every read here
/// fails with [`Error::UnexpectedEof`] instead.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

macro_rules! read_le {
    ($($name:ident: $ty:ty),* $(,)?) => {
        $(
            pub fn $name(&mut self) -> Result<$ty> {
                self.array().map(<$ty>::from_le_bytes)
            }
        )*
    };
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn is_eof(&self) -> bool {
        self.pos == self.data.len()
    }

    /// The bytes that have not been read yet.
    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    /// Moves to an absolute offset. Seeking exactly to the end is allowed.
    pub fn seek(&mut self, offset: usize) -> Result<()> {
        if offset > self.data.len() {
            return Err(Error::UnexpectedEof {
                offset,
                needed: 0,
                available: 0,
            });
        }
        self.pos = offset;
        Ok(())
    }

    pub fn skip(&mut self, len: usize) -> Result<()> {
        self.bytes(len).map(drop)
    }

    /// Borrows the next `len` bytes straight from the underlying slice, without copying.
    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let data = self.data;
        let bytes = data
            .get(self.pos..)
            .and_then(|rest| rest.get(..len))
            .ok_or(Error::UnexpectedEof {
                offset: self.pos,
                needed: len,
                available: self.remaining(),
            })?;
        self.pos += len;
        Ok(bytes)
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = self.bytes(N)?;
        Ok(bytes.try_into().expect("slice has exactly N bytes"))
    }

    /// Views the next `size_of::<T>()` bytes as a `T`, without copying.
    pub fn record<T: Record>(&mut self) -> Result<&'a T> {
        let bytes = self.bytes(size_of::<T>())?;
        Ok(T::ref_from_bytes(bytes).expect("length matches and T has no alignment"))
    }

    /// Views the next `count` records as a slice, without copying.
    pub fn records<T: Record>(&mut self, count: usize) -> Result<&'a [T]> {
        let len = count
            .checked_mul(size_of::<T>())
            .ok_or(Error::UnexpectedEof {
                offset: self.pos,
                needed: usize::MAX,
                available: self.remaining(),
            })?;
        let bytes = self.bytes(len)?;
        Ok(<[T]>::ref_from_bytes(bytes).expect("length matches and T has no alignment"))
    }

    /// Cuts the input off at the absolute offset `end`, which must not lie before the current
    /// position.
    pub fn limit(&mut self, end: usize) -> Result<()> {
        if end < self.pos || end > self.data.len() {
            return Err(Error::UnexpectedEof {
                offset: end,
                needed: 0,
                available: self.data.len(),
            });
        }
        self.data = &self.data[..end];
        Ok(())
    }

    /// Splits off the next `len` bytes as an independent reader whose offsets start at zero.
    pub fn sub(&mut self, len: usize) -> Result<Reader<'a>> {
        self.bytes(len).map(Reader::new)
    }

    read_le! {
        u8: u8,
        i8: i8,
        u16: u16,
        i16: i16,
        u32: u32,
        i32: i32,
        f32: f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::{FromBytes, Immutable, KnownLayout, Unaligned};

    #[test]
    fn reads_little_endian_primitives() {
        let data = [
            0x01, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x80, 0x3f, 0xff,
        ];
        let mut r = Reader::new(&data);

        assert_eq!(r.u8().unwrap(), 0x01);
        assert_eq!(r.u16().unwrap(), 0x1234);
        assert_eq!(r.u32().unwrap(), 0x1234_5678);
        assert_eq!(r.f32().unwrap(), 1.0);
        assert_eq!(r.i8().unwrap(), -1);
        assert!(r.is_eof());
    }

    #[test]
    fn short_reads_fail_without_consuming() {
        let mut r = Reader::new(&[1, 2, 3]);
        r.skip(1).unwrap();

        let err = r.u32().unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedEof {
                offset: 1,
                needed: 4,
                available: 2
            }
        ));
        assert_eq!(r.position(), 1);
        assert_eq!(r.u16().unwrap(), 0x0302);
    }

    #[test]
    fn seek_is_bounded_by_length() {
        let mut r = Reader::new(&[0; 4]);
        r.seek(4).unwrap();
        assert!(r.is_eof());
        assert!(r.seek(5).is_err());
        assert_eq!(r.position(), 4);
    }

    #[derive(FromBytes, KnownLayout, Immutable, Unaligned)]
    #[repr(C)]
    struct Pair {
        tag: [u8; 2],
        value: zerocopy::little_endian::U32,
    }

    #[test]
    fn views_records_without_copying() {
        let data = [b'a', b'b', 1, 0, 0, 0, b'c', b'd', 2, 0, 0, 0, 0xff];
        let mut r = Reader::new(&data);

        let pairs = r.records::<Pair>(2).unwrap();
        assert_eq!(pairs[0].tag, *b"ab");
        assert_eq!(pairs[1].value.get(), 2);
        assert!(std::ptr::eq(pairs[0].tag.as_ptr(), data.as_ptr()));

        assert!(r.record::<Pair>().is_err());
        assert!(r.records::<Pair>(usize::MAX).is_err());
        assert_eq!(r.u8().unwrap(), 0xff);
    }

    #[test]
    fn limit_cuts_off_the_tail() {
        let mut r = Reader::new(&[1, 2, 3, 4]);
        r.skip(1).unwrap();
        assert!(r.limit(0).is_err());
        r.limit(2).unwrap();
        assert_eq!(r.rest(), &[2]);
        assert!(r.u16().is_err());
    }

    #[test]
    fn sub_reader_is_relative() {
        let mut r = Reader::new(&[9, 1, 0, 7]);
        r.skip(1).unwrap();

        let mut sub = r.sub(2).unwrap();
        assert_eq!(sub.position(), 0);
        assert_eq!(sub.u16().unwrap(), 1);
        assert!(sub.u8().is_err());
        assert_eq!(r.u8().unwrap(), 7);
    }
}
