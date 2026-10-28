//! Textures (`.TEX`, signature `ZTEX`).
//!
//! ```text
//! header    "ZTEX" | version: u32 = 0 | format | width | height | mipmap count
//!           | reference width | reference height | average color (ARGB)
//! palette   256 BGRA entries, only for TextureFormat::P8
//! mipmaps   smallest first, each tightly packed in the texture's format
//! ```

use crate::io::Reader;
use crate::math::Color;
use crate::{Asset, Error, Result};

const FORMAT: &str = "texture";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    B8G8R8A8 = 0x0,
    R8G8B8A8 = 0x1,
    A8B8G8R8 = 0x2,
    A8R8G8B8 = 0x3,
    B8G8R8 = 0x4,
    R8G8B8 = 0x5,
    A4R4G4B4 = 0x6,
    A1R5G5B5 = 0x7,
    R5G6B5 = 0x8,
    /// 8-bit indices into the texture's [palette](Texture::palette).
    P8 = 0x9,
    Dxt1 = 0xA,
    Dxt2 = 0xB,
    Dxt3 = 0xC,
    Dxt4 = 0xD,
    Dxt5 = 0xE,
}

impl TextureFormat {
    fn from_raw(raw: u32) -> Result<Self> {
        use TextureFormat::*;
        const ALL: [TextureFormat; 15] = [
            B8G8R8A8, R8G8B8A8, A8B8G8R8, A8R8G8B8, B8G8R8, R8G8B8, A4R4G4B4, A1R5G5B5, R5G6B5, P8,
            Dxt1, Dxt2, Dxt3, Dxt4, Dxt5,
        ];
        ALL.get(raw as usize)
            .copied()
            .ok_or_else(|| Error::Unsupported {
                format: FORMAT,
                reason: format!("pixel format {raw:#x}"),
            })
    }

    pub fn is_compressed(self) -> bool {
        matches!(
            self,
            Self::Dxt1 | Self::Dxt2 | Self::Dxt3 | Self::Dxt4 | Self::Dxt5
        )
    }

    /// The size in bytes of an image of this format, computed the way the engine does it: DXT
    /// sizes round down to whole 4×4 blocks, with at least one block.
    fn image_size(self, width: u32, height: u32) -> usize {
        let (w, h) = (width as usize, height as usize);
        let blocks = || (w / 4).max(1) * (h / 4).max(1);
        match self {
            Self::B8G8R8A8 | Self::R8G8B8A8 | Self::A8B8G8R8 | Self::A8R8G8B8 => w * h * 4,
            Self::B8G8R8 | Self::R8G8B8 => w * h * 3,
            Self::A4R4G4B4 | Self::A1R5G5B5 | Self::R5G6B5 => w * h * 2,
            Self::P8 => w * h,
            Self::Dxt1 => blocks() * 8,
            Self::Dxt2 | Self::Dxt3 | Self::Dxt4 | Self::Dxt5 => blocks() * 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    /// The size the texture had before it was compiled, which UV coordinates were made for.
    pub reference_width: u32,
    pub reference_height: u32,
    pub average_color: Color,
    palette: Option<Box<[Color; 256]>>,
    /// Largest first. ZenKit stores them smallest first, as the file does.
    mipmaps: Vec<Vec<u8>>,
}

impl Texture {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::read(&mut Reader::new(data))
    }

    /// Reads a texture and leaves `r` right after it, which is how meshes embed lightmaps.
    pub fn read(r: &mut Reader) -> Result<Self> {
        if r.bytes(4)? != b"ZTEX" {
            return Err(Error::malformed(FORMAT, "missing \"ZTEX\" signature"));
        }
        let version = r.u32()?;
        if version != 0 {
            return Err(Error::Unsupported {
                format: FORMAT,
                reason: format!("version {version}"),
            });
        }

        let format = TextureFormat::from_raw(r.u32()?)?;
        let width = r.u32()?;
        let height = r.u32()?;
        let mipmap_count = r.u32()?.max(1);
        let reference_width = r.u32()?;
        let reference_height = r.u32()?;
        let average_color = argb(r.u32()?);

        if mipmap_count > 32 {
            return Err(Error::malformed(FORMAT, format!("{mipmap_count} mipmaps")));
        }

        let palette = match format {
            TextureFormat::P8 => {
                let mut palette = Box::new([Color::default(); 256]);
                for entry in palette.iter_mut() {
                    let [b, g, r, a] = r.array()?;
                    *entry = Color { r, g, b, a };
                }
                Some(palette)
            }
            _ => None,
        };

        let mut mipmaps = Vec::with_capacity(mipmap_count as usize);
        for level in (0..mipmap_count).rev() {
            let (w, h) = mip_dimensions(width, height, level);
            mipmaps.push(r.bytes(format.image_size(w, h))?.to_vec());
        }
        mipmaps.reverse();

        Ok(Self {
            format,
            width,
            height,
            reference_width,
            reference_height,
            average_color,
            palette,
            mipmaps,
        })
    }

    pub fn mipmap_count(&self) -> usize {
        self.mipmaps.len()
    }

    /// The width and height of a mipmap level, never smaller than one pixel.
    pub fn mipmap_dimensions(&self, level: usize) -> (u32, u32) {
        mip_dimensions(self.width, self.height, level as u32)
    }

    /// The raw pixel data of a mipmap level, in [`format`](Self::format). Level 0 is the largest.
    pub fn mipmap(&self, level: usize) -> Option<&[u8]> {
        self.mipmaps.get(level).map(Vec::as_slice)
    }

    pub fn palette(&self) -> Option<&[Color; 256]> {
        self.palette.as_deref()
    }

    /// Decodes a mipmap level to tightly packed RGBA8. DXT blocks are decoded in parallel when
    /// the `parallel` feature is on.
    pub fn to_rgba8(&self, level: usize) -> Result<Vec<u8>> {
        let data = self
            .mipmap(level)
            .ok_or_else(|| Error::malformed(FORMAT, format!("no mipmap level {level}")))?;
        let (w, h) = self.mipmap_dimensions(level);
        let pixels = w as usize * h as usize;

        if self.format.is_compressed() {
            return decode_dxt(self.format, data, w as usize, h as usize);
        }

        let mut out = Vec::with_capacity(pixels * 4);
        match self.format {
            TextureFormat::R8G8B8A8 => out.extend_from_slice(&data[..pixels * 4]),
            TextureFormat::B8G8R8A8 => {
                out.extend(data.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]))
            }
            TextureFormat::A8B8G8R8 => {
                out.extend(data.chunks_exact(4).flat_map(|p| [p[3], p[2], p[1], p[0]]))
            }
            TextureFormat::A8R8G8B8 => {
                out.extend(data.chunks_exact(4).flat_map(|p| [p[1], p[2], p[3], p[0]]))
            }
            TextureFormat::B8G8R8 => {
                out.extend(data.chunks_exact(3).flat_map(|p| [p[2], p[1], p[0], 0xFF]))
            }
            TextureFormat::R8G8B8 => {
                out.extend(data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 0xFF]))
            }
            TextureFormat::A4R4G4B4 => out.extend(texels16(data).flat_map(|v| {
                let n = |shift: u16| ((v >> shift) & 0xF) as u8 * 17;
                [n(8), n(4), n(0), n(12)]
            })),
            TextureFormat::A1R5G5B5 => out.extend(texels16(data).flat_map(|v| {
                let n = |shift: u16| expand5(v >> shift);
                [n(10), n(5), n(0), if v & 0x8000 != 0 { 0xFF } else { 0 }]
            })),
            TextureFormat::R5G6B5 => out.extend(texels16(data).flat_map(|v| {
                // These factors are ZenKit's, which truncate slightly differently from bit
                // replication; keeping them keeps the output identical.
                let r = ((v >> 11) & 0x1F) as f32 * 8.225_806;
                let g = ((v >> 5) & 0x3F) as f32 * 4.047_619;
                let b = (v & 0x1F) as f32 * 8.225_806;
                [r as u8, g as u8, b as u8, 0xFF]
            })),
            TextureFormat::P8 => {
                let palette = self
                    .palette
                    .as_deref()
                    .expect("P8 textures always have a palette");
                out.extend(data.iter().flat_map(|&i| {
                    let c = palette[i as usize];
                    [c.r, c.g, c.b, c.a]
                }));
            }
            TextureFormat::Dxt1
            | TextureFormat::Dxt2
            | TextureFormat::Dxt3
            | TextureFormat::Dxt4
            | TextureFormat::Dxt5 => {
                unreachable!("compressed formats are handled above")
            }
        }
        Ok(out)
    }
}

fn argb(v: u32) -> Color {
    let [b, g, r, a] = v.to_le_bytes();
    Color { r, g, b, a }
}

fn mip_dimensions(width: u32, height: u32, level: u32) -> (u32, u32) {
    let shrink = |v: u32| v.checked_shr(level).unwrap_or(0).max(1);
    (shrink(width.max(1)), shrink(height.max(1)))
}

fn texels16(data: &[u8]) -> impl Iterator<Item = u16> + '_ {
    data.chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
}

fn expand5(v: u16) -> u8 {
    let v = (v & 0x1F) as u8;
    (v << 3) | (v >> 2)
}

fn decode_dxt(format: TextureFormat, data: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    // DXT2 and DXT4 are the premultiplied-alpha variants of DXT3 and DXT5. The block layout is
    // the same, so they decode alike; ZenKit cannot decode them at all.
    let codec = match format {
        TextureFormat::Dxt1 => texpresso::Format::Bc1,
        TextureFormat::Dxt2 | TextureFormat::Dxt3 => texpresso::Format::Bc2,
        TextureFormat::Dxt4 | TextureFormat::Dxt5 => texpresso::Format::Bc3,
        _ => unreachable!("only called for DXT formats"),
    };

    // The engine's size rounds down to whole blocks, texpresso's rounds up. They only differ for
    // sizes that are not a multiple of four, which the engine never produces.
    let needed = codec.compressed_size(width, height);
    if data.len() < needed {
        return Err(Error::malformed(
            FORMAT,
            format!(
                "{width}x{height} DXT image needs {needed} bytes, has {}",
                data.len()
            ),
        ));
    }

    let mut out = vec![0; width * height * 4];
    codec.decompress(data, width, height, &mut out);
    Ok(out)
}

impl Asset for Texture {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        Texture::from_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(format: TextureFormat, width: u32, height: u32, mipmaps: &[&[u8]]) -> Vec<u8> {
        let mut out = b"ZTEX".to_vec();
        for v in [
            0,
            format as u32,
            width,
            height,
            mipmaps.len() as u32,
            width,
            height,
            0xFF11_2233,
        ] {
            out.extend(v.to_le_bytes());
        }
        if format == TextureFormat::P8 {
            for i in 0..=255u8 {
                out.extend([i, 0, 255 - i, 128]);
            }
        }
        for mip in mipmaps.iter().rev() {
            out.extend(*mip);
        }
        out
    }

    #[test]
    fn mipmaps_come_back_largest_first() {
        let data = texture(TextureFormat::R8G8B8, 2, 2, &[&[1; 12], &[2; 3]]);
        let tex = Texture::from_bytes(&data).unwrap();
        assert_eq!(tex.mipmap_count(), 2);
        assert_eq!(tex.mipmap(0).unwrap(), &[1; 12]);
        assert_eq!(tex.mipmap(1).unwrap(), &[2; 3]);
        assert_eq!(tex.mipmap_dimensions(1), (1, 1));
        assert_eq!(tex.average_color, Color::new(0x11, 0x22, 0x33, 0xFF));
        assert_eq!(tex.to_rgba8(1).unwrap(), [2, 2, 2, 255]);
    }

    #[test]
    fn decodes_16_bit_formats() {
        let pixel = |format, bytes: [u8; 2]| {
            let data = texture(format, 1, 1, &[&bytes]);
            Texture::from_bytes(&data).unwrap().to_rgba8(0).unwrap()
        };
        assert_eq!(
            pixel(TextureFormat::A4R4G4B4, 0xF84Cu16.to_le_bytes()),
            [0x88, 0x44, 0xCC, 0xFF]
        );
        assert_eq!(
            pixel(TextureFormat::A1R5G5B5, 0xFC00u16.to_le_bytes()),
            [0xFF, 0, 0, 0xFF]
        );
        assert_eq!(
            pixel(TextureFormat::A1R5G5B5, 0x001Fu16.to_le_bytes()),
            [0, 0, 0xFF, 0]
        );
        assert_eq!(
            pixel(TextureFormat::R5G6B5, 0xFFFFu16.to_le_bytes()),
            [255, 254, 255, 255]
        );
    }

    #[test]
    fn decodes_palettes() {
        let data = texture(TextureFormat::P8, 2, 1, &[&[0, 255]]);
        let tex = Texture::from_bytes(&data).unwrap();
        assert_eq!(tex.to_rgba8(0).unwrap(), [255, 0, 0, 128, 0, 0, 255, 128]);
    }

    #[test]
    fn rejects_bad_headers() {
        let mut data = texture(TextureFormat::R8G8B8, 1, 1, &[&[0; 3]]);
        data[8] = 0x42;
        assert!(matches!(
            Texture::from_bytes(&data),
            Err(Error::Unsupported { .. })
        ));
        assert!(Texture::from_bytes(b"ZTEY").is_err());

        let short = texture(TextureFormat::R8G8B8, 4, 4, &[&[0; 3]]);
        assert!(matches!(
            Texture::from_bytes(&short),
            Err(Error::UnexpectedEof { .. })
        ));
    }
}
