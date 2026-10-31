//! Building textures from pixels and writing them back to `.TEX`.

use tracing::warn;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::math::Color;

use crate::FORMAT;
use crate::Texture;
use crate::TextureFormat;
use crate::mip_dimensions;

/// How hard DXT compression tries to keep the picture as it was.
///
/// Each 4×4 block of a DXT texture keeps two colors and blends between them. Picking the two is
/// what takes the time: on detailed pictures, [`High`](Self::High) takes tens of times longer
/// than [`Fast`](Self::Fast), for about a third less error. On flat pictures the two are close in
/// both. Other formats store every pixel and ignore this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DxtQuality {
    /// Takes the two colors from the ends of the block's spread of colors.
    Fast,
    /// Searches for the two colors that fit the block best.
    #[default]
    High,
}

impl Texture {
    /// Builds a texture from tightly packed RGBA8 pixels, encoding it in `format` with
    /// `mipmap_count` levels, each half the size of the one before. DXT formats are compressed at
    /// [`DxtQuality::High`], in parallel when the `parallel` feature is on.
    ///
    /// DXT textures need sides that are multiples of four, as the engine computes their sizes in
    /// whole 4×4 blocks. Palette textures ([`TextureFormat::P8`]) need a palette to be chosen,
    /// which this does not do; [`from_mipmaps`](Self::from_mipmaps) takes one ready-made.
    ///
    /// A format or mipmap count the game refuses is still built, with a warning, as other tools
    /// may take it: see [`TextureFormat::is_refused_by_game`] and [`game_mipmap_count`].
    pub fn from_rgba8(
        width: u32,
        height: u32,
        rgba: &[u8],
        format: TextureFormat,
        mipmap_count: u32,
    ) -> Result<Self> {
        Self::from_rgba8_with(width, height, rgba, format, mipmap_count, DxtQuality::High)
    }

    /// [`from_rgba8`](Self::from_rgba8), compressing DXT formats at `quality`.
    pub fn from_rgba8_with(
        width: u32,
        height: u32,
        rgba: &[u8],
        format: TextureFormat,
        mipmap_count: u32,
        quality: DxtQuality,
    ) -> Result<Self> {
        let pixels = width as usize * height as usize;
        if width == 0 || height == 0 || rgba.len() != pixels * 4 {
            let reason =
                format!("{width}x{height} pixels need {} bytes, got {}", pixels * 4, rgba.len());
            return Err(Error::unwritable(FORMAT, reason));
        }
        if format == TextureFormat::P8 {
            return Err(Error::unsupported(FORMAT, "choosing a palette for P8 textures"));
        }
        check_mipmap_count(width, height, mipmap_count)?;
        warn_if_refused_by_game(format, width, height, mipmap_count);

        let mut levels = vec![rgba.to_vec()];
        for level in 1..mipmap_count {
            let (w, h) = mip_dimensions(width, height, level - 1);
            levels.push(half(&levels[level as usize - 1], w as usize, h as usize));
        }

        let mipmaps = (levels.iter().enumerate())
            .map(|(level, rgba)| {
                let (w, h) = mip_dimensions(width, height, level as u32);
                encode(format, rgba, w, h, quality)
            })
            .collect::<Result<_>>()?;

        Ok(Self {
            format,
            width,
            height,
            reference_width: width,
            reference_height: height,
            average_color: average(rgba),
            palette: None,
            mipmaps,
        })
    }

    /// Builds a texture from pixel data already encoded in `format`, largest level first, with
    /// the palette that [`TextureFormat::P8`] needs and other formats must not have.
    pub fn from_mipmaps(
        format: TextureFormat,
        width: u32,
        height: u32,
        mipmaps: Vec<Vec<u8>>,
        palette: Option<[Color; 256]>,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::unwritable(FORMAT, format!("a texture cannot be {width}x{height}")));
        }
        check_mipmap_count(width, height, mipmaps.len() as u32)?;
        warn_if_refused_by_game(format, width, height, mipmaps.len() as u32);
        if palette.is_some() != (format == TextureFormat::P8) {
            let reason = "only P8 textures have a palette, and they always do";
            return Err(Error::unwritable(FORMAT, reason));
        }
        for (level, data) in mipmaps.iter().enumerate() {
            let (w, h) = mip_dimensions(width, height, level as u32);
            let needed = format.image_size(w, h);
            if data.len() != needed {
                let reason = format!(
                    "mipmap {level} of {w}x{h} {format:?} pixels needs {needed} bytes, has {}",
                    data.len()
                );
                return Err(Error::unwritable(FORMAT, reason));
            }
        }

        let mut texture = Self {
            format,
            width,
            height,
            reference_width: width,
            reference_height: height,
            average_color: Color::default(),
            palette: palette.map(Box::new),
            mipmaps,
        };
        texture.average_color = average(&texture.to_rgba8(0)?);
        Ok(texture)
    }

    pub fn write(&self, w: &mut Writer) {
        w.bytes(b"ZTEX");
        w.u32(0);
        w.u32(self.format as u32);
        w.u32(self.width);
        w.u32(self.height);
        w.u32(self.mipmaps.len() as u32);
        w.u32(self.reference_width);
        w.u32(self.reference_height);
        let Color { r, g, b, a } = self.average_color;
        w.bytes(&[b, g, r, a]);
        if let Some(palette) = &self.palette {
            for &Color { r, g, b, a } in palette.iter() {
                w.bytes(&[b, g, r, a]);
            }
        }
        for mipmap in self.mipmaps.iter().rev() {
            w.bytes(mipmap);
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::with_capacity(32 + self.mipmaps.iter().map(Vec::len).sum::<usize>());
        self.write(&mut w);
        w.into_inner()
    }
}

/// How many mipmap levels a texture can have: one for every halving until both sides are a
/// single pixel.
pub fn max_mipmap_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// How many mipmap levels the game takes: as many as [`max_mipmap_count`], except that DXT
/// levels stop before a side would drop under 4 pixels or stop being a multiple of 4. A 128×128
/// DXT texture gets 6, down to 4×4.
pub fn game_mipmap_count(format: TextureFormat, width: u32, height: u32) -> u32 {
    let max = max_mipmap_count(width, height);
    if !format.is_compressed() {
        return max;
    }
    let whole_blocks = |level| {
        let (w, h) = mip_dimensions(width, height, level);
        w >= 4 && h >= 4 && w.is_multiple_of(4) && h.is_multiple_of(4)
    };
    1 + (1..max).take_while(|&level| whole_blocks(level)).count() as u32
}

fn warn_if_refused_by_game(format: TextureFormat, width: u32, height: u32, mipmap_count: u32) {
    if format.is_refused_by_game() {
        warn!("the game refuses {format:?} textures; use Dxt1 or Dxt3");
    } else if mipmap_count > game_mipmap_count(format, width, height) {
        let most = game_mipmap_count(format, width, height);
        warn!(
            "the game refuses a {width}x{height} {format:?} texture with {mipmap_count} \
             mipmaps, as DXT mipmaps under 4x4 are refused; it takes at most {most}"
        );
    }
}

fn check_mipmap_count(width: u32, height: u32, count: u32) -> Result<()> {
    let max = max_mipmap_count(width, height);
    if count == 0 || count > max {
        let reason = format!("a {width}x{height} texture has 1 to {max} mipmaps, not {count}");
        return Err(Error::unwritable(FORMAT, reason));
    }
    Ok(())
}

/// Halves an image, averaging each 2×2 block. A side of one pixel stays one pixel.
fn half(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let xs = [(2 * x).min(width - 1), (2 * x + 1).min(width - 1)];
            let ys = [(2 * y).min(height - 1), (2 * y + 1).min(height - 1)];
            for channel in 0..4 {
                let sum: u32 = (ys.iter())
                    .flat_map(|&sy| xs.iter().map(move |&sx| (sy * width + sx) * 4 + channel))
                    .map(|i| u32::from(rgba[i]))
                    .sum();
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }
    out
}

/// The engine computes the average from the image before compressing it, so for DXT textures
/// this can come out a step or two away from what the game's own tools store.
fn average(rgba: &[u8]) -> Color {
    let count = (rgba.len() / 4).max(1) as u64;
    let mut sums = [0u64; 4];
    for pixel in rgba.as_chunks::<4>().0 {
        for (sum, &v) in sums.iter_mut().zip(pixel) {
            *sum += u64::from(v);
        }
    }
    let [r, g, b, a] = sums.map(|sum| ((sum + count / 2) / count) as u8);
    Color { r, g, b, a }
}

/// Scales an 8-bit channel down to `bits`, rounding to the nearest value.
fn quantize(v: u8, bits: u32) -> u16 {
    let max = (1u32 << bits) - 1;
    ((u32::from(v) * max + 127) / 255) as u16
}

fn encode(
    format: TextureFormat,
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: DxtQuality,
) -> Result<Vec<u8>> {
    let pixels = rgba.as_chunks::<4>().0;
    let packed16 = |pack: fn([u8; 4]) -> u16| -> Vec<u8> {
        pixels.iter().flat_map(|&p| pack(p).to_le_bytes()).collect()
    };
    Ok(match format {
        TextureFormat::R8G8B8A8 => rgba.to_vec(),
        TextureFormat::B8G8R8A8 => pixels.iter().flat_map(|&[r, g, b, a]| [b, g, r, a]).collect(),
        TextureFormat::A8B8G8R8 => pixels.iter().flat_map(|&[r, g, b, a]| [a, b, g, r]).collect(),
        TextureFormat::A8R8G8B8 => pixels.iter().flat_map(|&[r, g, b, a]| [a, r, g, b]).collect(),
        TextureFormat::B8G8R8 => pixels.iter().flat_map(|&[r, g, b, _]| [b, g, r]).collect(),
        TextureFormat::R8G8B8 => pixels.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect(),
        TextureFormat::A4R4G4B4 => packed16(|[r, g, b, a]| {
            (quantize(a, 4) << 12) | (quantize(r, 4) << 8) | (quantize(g, 4) << 4) | quantize(b, 4)
        }),
        TextureFormat::A1R5G5B5 => packed16(|[r, g, b, a]| {
            let alpha = u16::from(a >= 128) << 15;
            alpha | (quantize(r, 5) << 10) | (quantize(g, 5) << 5) | quantize(b, 5)
        }),
        TextureFormat::R5G6B5 => {
            packed16(|[r, g, b, _]| (quantize(r, 5) << 11) | (quantize(g, 6) << 5) | quantize(b, 5))
        }
        TextureFormat::P8 => unreachable!("rejected before encoding"),
        TextureFormat::Dxt1
        | TextureFormat::Dxt2
        | TextureFormat::Dxt3
        | TextureFormat::Dxt4
        | TextureFormat::Dxt5 => {
            if (width >= 4 && !width.is_multiple_of(4))
                || (height >= 4 && !height.is_multiple_of(4))
            {
                let reason =
                    format!("DXT needs sides that are multiples of 4, not {width}x{height}");
                return Err(Error::unwritable(FORMAT, reason));
            }
            // DXT2 and DXT4 are written like DXT3 and DXT5, matching how they are decoded.
            let codec = match format {
                TextureFormat::Dxt1 => texpresso::Format::Bc1,
                TextureFormat::Dxt2 | TextureFormat::Dxt3 => texpresso::Format::Bc2,
                _ => texpresso::Format::Bc3,
            };
            let (w, h) = (width as usize, height as usize);
            let mut out = vec![0; codec.compressed_size(w, h)];
            let algorithm = match quality {
                DxtQuality::Fast => texpresso::Algorithm::RangeFit,
                DxtQuality::High => texpresso::Algorithm::ClusterFit,
            };
            let params = texpresso::Params { algorithm, ..texpresso::Params::default() };
            codec.compress(rgba, w, h, params, &mut out);
            out
        }
    })
}
