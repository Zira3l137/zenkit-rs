use std::path::Path;

use zenkit_rs_core::Error;
use zenkit_rs_core::math::Color;
use zenkit_rs_texture::DxtQuality;
use zenkit_rs_texture::Texture;
use zenkit_rs_texture::TextureFormat;
use zenkit_rs_texture::max_mipmap_count;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)).unwrap()
}

/// A 4×2 image with every channel varying, alpha included.
const PIXELS: [u8; 32] = [
    255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 17, 34, 51, 68, //
    136, 153, 170, 187, 255, 255, 255, 255, 0, 0, 0, 0, 238, 221, 204, 255,
];

#[test]
fn erz_writes_back_byte_for_byte() {
    let data = sample("erz.tex");
    assert!(Texture::from_bytes(&data).unwrap().to_bytes() == data);
}

#[test]
fn eight_bit_formats_keep_every_pixel() {
    use TextureFormat::*;
    for format in [B8G8R8A8, R8G8B8A8, A8B8G8R8, A8R8G8B8, B8G8R8, R8G8B8] {
        let texture = Texture::from_rgba8(4, 2, &PIXELS, format, 1).unwrap();
        let back = Texture::from_bytes(&texture.to_bytes()).unwrap();
        assert_eq!(back, texture);

        let mut expected = PIXELS;
        if matches!(format, B8G8R8 | R8G8B8) {
            expected.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
        }
        assert_eq!(back.to_rgba8(0).unwrap(), expected, "{format:?}");
    }
}

#[test]
fn sixteen_bit_formats_round_to_the_nearest_value() {
    let decode = |format, rgba: [u8; 4]| {
        Texture::from_rgba8(1, 1, &rgba, format, 1).unwrap().to_rgba8(0).unwrap()
    };
    assert_eq!(decode(TextureFormat::A4R4G4B4, [0x88, 0x44, 0xCC, 0xFF]), [0x88, 0x44, 0xCC, 0xFF]);
    assert_eq!(decode(TextureFormat::A4R4G4B4, [0x89, 0x40, 0xC4, 0x08]), [0x88, 0x44, 0xCC, 0x00]);
    assert_eq!(decode(TextureFormat::A1R5G5B5, [255, 0, 132, 128]), [255, 0, 132, 255]);
    assert_eq!(decode(TextureFormat::A1R5G5B5, [255, 0, 132, 127])[3], 0);
    assert_eq!(decode(TextureFormat::R5G6B5, [255, 255, 255, 0]), [255, 254, 255, 255]);
}

#[test]
fn dxt_textures_compress_with_mipmaps() {
    let erz = Texture::from_bytes(&sample("erz.tex")).unwrap();
    let rgba = erz.to_rgba8(0).unwrap();

    let texture = Texture::from_rgba8(128, 128, &rgba, TextureFormat::Dxt1, 5).unwrap();
    assert_eq!(texture.mipmap_count(), 5);
    assert_eq!(texture.mipmap(4).unwrap().len(), 2 * 2 * 8);
    let back = Texture::from_bytes(&texture.to_bytes()).unwrap();
    assert_eq!(back, texture);

    // Recompressing pixels that came out of DXT1 should land very close to them.
    let error: u64 = (back.to_rgba8(0).unwrap().iter().zip(&rgba))
        .map(|(&a, &b)| u64::from(a.abs_diff(b)))
        .sum();
    assert!(error / (rgba.len() as u64) < 2, "average error {}", error / rgba.len() as u64);

    let full = Texture::from_rgba8(8, 4, &[200; 8 * 4 * 4], TextureFormat::Dxt5, 4).unwrap();
    assert_eq!(full.mipmap_dimensions(3), (1, 1));
    assert_eq!(full.mipmap(3).unwrap().len(), 16);
    assert_eq!(full.to_rgba8(3).unwrap(), [200; 4]);
}

#[test]
fn average_color_covers_the_largest_level() {
    let texture =
        Texture::from_rgba8(2, 1, &[0, 10, 20, 255, 100, 30, 21, 0], TextureFormat::R8G8B8A8, 2)
            .unwrap();
    assert_eq!(texture.average_color, Color::new(50, 20, 21, 128));
    assert_eq!(texture.mipmap(1).unwrap(), [50, 20, 21, 128]);
}

#[test]
fn palette_textures_take_a_ready_palette() {
    let mut palette = [Color::default(); 256];
    palette[1] = Color::new(1, 2, 3, 4);
    let texture = Texture::from_mipmaps(
        TextureFormat::P8,
        2,
        2,
        vec![vec![1, 0, 0, 1], vec![1]],
        Some(palette),
    )
    .unwrap();
    let back = Texture::from_bytes(&texture.to_bytes()).unwrap();
    assert_eq!(back, texture);
    assert_eq!(back.to_rgba8(1).unwrap(), [1, 2, 3, 4]);
    assert_eq!(back.average_color, Color::new(1, 1, 2, 2));
}

#[test]
fn mipmap_counts_go_down_to_one_pixel() {
    assert_eq!(max_mipmap_count(1, 1), 1);
    assert_eq!(max_mipmap_count(128, 128), 8);
    assert_eq!(max_mipmap_count(256, 32), 9);
    assert_eq!(max_mipmap_count(5, 3), 3);
}

#[test]
fn refuses_textures_it_cannot_build() {
    let unwritable = |result: Result<Texture, Error>| match result {
        Err(Error::Unwritable { reason, .. }) => reason,
        other => panic!("expected an unwritable texture, got {other:?}"),
    };

    assert!(
        unwritable(Texture::from_rgba8(4, 2, &PIXELS[..28], TextureFormat::R8G8B8A8, 1))
            .contains("need 32 bytes")
    );
    assert!(
        unwritable(Texture::from_rgba8(4, 2, &PIXELS, TextureFormat::R8G8B8A8, 4))
            .contains("1 to 3 mipmaps")
    );
    assert!(
        unwritable(Texture::from_rgba8(6, 4, &[0; 96], TextureFormat::Dxt1, 1))
            .contains("multiples of 4")
    );
    assert!(matches!(
        Texture::from_rgba8(4, 2, &PIXELS, TextureFormat::P8, 1),
        Err(Error::Unsupported { .. })
    ));

    assert!(
        unwritable(Texture::from_mipmaps(TextureFormat::P8, 1, 1, vec![vec![0]], None))
            .contains("palette")
    );
    assert!(
        unwritable(Texture::from_mipmaps(TextureFormat::R8G8B8, 2, 2, vec![vec![0; 11]], None))
            .contains("needs 12 bytes")
    );
}

#[test]
fn dxt_quality_trades_error_for_speed() {
    // Noise over a gradient: blocks of many colors, where the two qualities differ the most.
    let mut seed = 7u32;
    let rgba: Vec<u8> = (0..64 * 64 * 4)
        .map(|i| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let gradient = (i / 4 % 64 * 3) as u32;
            if i % 4 == 3 { 255 } else { (gradient + (seed >> 24) % 48) as u8 }
        })
        .collect();
    let error = |quality| {
        let texture =
            Texture::from_rgba8_with(64, 64, &rgba, TextureFormat::Dxt1, 3, quality).unwrap();
        assert_eq!(Texture::from_bytes(&texture.to_bytes()).unwrap(), texture);
        let back = texture.to_rgba8(0).unwrap();
        back.iter().zip(&rgba).map(|(&a, &b)| u64::from(a.abs_diff(b))).sum::<u64>()
    };
    let (fast, high) = (error(DxtQuality::Fast), error(DxtQuality::High));
    assert!(high < fast, "high {high}, fast {fast}");

    let plain =
        Texture::from_rgba8_with(4, 2, &PIXELS, TextureFormat::R8G8B8A8, 1, DxtQuality::Fast);
    assert_eq!(plain.unwrap().to_rgba8(0).unwrap(), PIXELS);
}
