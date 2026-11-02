//! Fonts (`.FNT`): where each character of a font is in its texture, and how wide it is.
//!
//! A font is two files: this one, and the texture it names, which holds every character drawn
//! in a grid, like `FONT_OLD_10_WHITE_HI.TGA` (compiled as `FONT_OLD_10_WHITE_HI-C.TEX`). The games
//! look characters up by their byte in the Windows-1252 code page, so a font has 256 glyphs, one
//! for each byte; a character the texture lacks has an empty one.
//!
//! ```text
//! "1" \n | texture name \n | height: u32 | count: u32 | widths: u8 × count
//!        | top left corners: (f32, f32) × count | bottom right corners: (f32, f32) × count
//! ```
//!
//! ```no_run
//! # fn main() -> zenkit_rs_core::Result<()> {
//! use zenkit_rs_font::Font;
//!
//! let data = std::fs::read("FONT_OLD_10_WHITE_HI.FNT").expect("the font is there");
//! let font = Font::from_bytes(&data)?;
//! let a = font.glyph('A').expect("the font has an A");
//! // The corners are fractions of the texture's size; for a 256 × 256 texture:
//! let (left, top, right, bottom) = a.pixels(256, 256);
//! println!("{} is {} px wide, at ({left}, {top})–({right}, {bottom})", font.name, a.width);
//! # Ok(()) }
//! ```

use tracing::warn;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::text;

const FORMAT: &str = "font";

/// How many glyphs the games expect of every font: one per byte.
pub const GLYPHS: usize = 256;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Font {
    /// The texture holding the characters, like `FONT_OLD_10_WHITE_HI.TGA`.
    pub name: String,
    /// The height of every glyph, in pixels.
    pub height: u32,
    /// One glyph per byte of the Windows-1252 code page, in order.
    pub glyphs: Vec<Glyph>,
}

/// Where one character is in the font's texture.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Glyph {
    /// How far the text moves on after the character, in pixels.
    pub width: u8,
    /// The character's top left corner in the texture, as fractions of its width and height.
    pub top_left: Vec2,
    /// The character's bottom right corner, as fractions of the texture's width and height.
    pub bottom_right: Vec2,
}

impl Glyph {
    /// The glyph's rectangle in a texture of this size, in pixels: left, top, right, bottom.
    pub fn pixels(&self, width: u32, height: u32) -> (f32, f32, f32, f32) {
        let (w, h) = (width as f32, height as f32);
        (self.top_left.x * w, self.top_left.y * h, self.bottom_right.x * w, self.bottom_right.y * h)
    }

    /// Whether the texture has the character, which an empty or inverted rectangle says it has
    /// not.
    pub fn is_present(&self) -> bool {
        self.bottom_right.x > self.top_left.x && self.bottom_right.y > self.top_left.y
    }
}

impl Font {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let version = r.line(b" \t\r\n");
        if version.trim_ascii() != b"1" {
            let found = text::latin1(version);
            return Err(Error::unsupported(FORMAT, format!("version {found:?}, only 1 is known")));
        }
        // Nothing after the name is skipped: the height follows as binary, and a height of 10 is
        // a line break.
        let name = text::latin1(r.line(&[]));
        let height = r.u32()?;

        let count = r.u32()? as usize;
        // 17 bytes per glyph; a count past what is there fails below without allocating for it.
        let mut glyphs = Vec::with_capacity(count.min(r.remaining() / 17));
        for _ in 0..count {
            glyphs.push(Glyph { width: r.u8()?, ..Glyph::default() });
        }
        for glyph in &mut glyphs {
            glyph.top_left = r.vec2()?;
        }
        for glyph in &mut glyphs {
            glyph.bottom_right = r.vec2()?;
        }
        if !r.is_eof() {
            warn!("the font goes on past offset {}, where reading it ended", r.offset());
        }
        Ok(Self { name, height, glyphs })
    }

    /// Writes the font as the games store it.
    ///
    /// Fails if the name has a line break or characters beyond Latin-1. A font with other than
    /// [`GLYPHS`] glyphs is written, but logs a warning, since the games expect that many.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let name = text::to_line(FORMAT, &self.name)?;
        if self.glyphs.len() != GLYPHS {
            warn!(
                "font {:?} has {} glyphs; the games expect {GLYPHS}",
                self.name,
                self.glyphs.len()
            );
        }
        let count = u32::try_from(self.glyphs.len())
            .map_err(|_| Error::unwritable(FORMAT, "more glyphs than a u32 can count"))?;

        let mut w = Writer::with_capacity(name.len() + 11 + self.glyphs.len() * 17);
        w.line(b"1");
        w.line(&name);
        w.u32(self.height);
        w.u32(count);
        self.glyphs.iter().for_each(|g| w.u8(g.width));
        self.glyphs.iter().for_each(|g| w.vec2(g.top_left));
        self.glyphs.iter().for_each(|g| w.vec2(g.bottom_right));
        Ok(w.into_inner())
    }

    /// The glyph of a character, found by its Windows-1252 byte, or `None` if the code page has
    /// no byte for it or the font has no glyph for that byte.
    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(usize::from(windows_1252(c)?))
    }
}

impl Asset for Font {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        Font::from_bytes(data)
    }
}

/// The Windows-1252 byte of a character. It is Latin-1 but for 0x80–0x9F, which hold
/// punctuation and a few letters instead of control characters.
fn windows_1252(c: char) -> Option<u8> {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    if let Some(i) = HIGH.iter().position(|&h| h == c) {
        return Some(0x80 + i as u8);
    }
    match u32::from(c) {
        0x80..=0x9F => None,
        code => u8::try_from(code).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_windows_1252_bytes() {
        assert_eq!(windows_1252('A'), Some(0x41));
        assert_eq!(windows_1252('€'), Some(0x80));
        assert_eq!(windows_1252('Ÿ'), Some(0x9F));
        assert_eq!(windows_1252('ü'), Some(0xFC));
        assert_eq!(windows_1252('\u{85}'), None, "0x85 is …, not the control character");
        assert_eq!(windows_1252('Ж'), None);
    }
}
