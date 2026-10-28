use crate::Result;
use crate::io::Reader;

/// A calendar date and time as the engine stores it, for example as a mesh's source file date.
/// The fields are kept as written; the engine never checks that they form a valid date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Date {
    pub year: u32,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

impl Date {
    /// Reads the engine's 16-byte date: year, then five `u16` fields and two bytes of padding.
    pub fn read(r: &mut Reader) -> Result<Self> {
        let date = Self {
            year: r.u32()?,
            month: r.u16()?,
            day: r.u16()?,
            hour: r.u16()?,
            minute: r.u16()?,
            second: r.u16()?,
        };
        r.skip(2)?;
        Ok(date)
    }
}
