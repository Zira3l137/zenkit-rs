#![cfg(feature = "texture")]

use std::path::Path;

use zenkit::math::Color;
use zenkit::texture::Texture;
use zenkit::texture::TextureFormat;

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples").join(name)).unwrap()
}

#[test]
fn erz() {
    let tex = Texture::from_bytes(&sample("erz.tex")).unwrap();
    assert_eq!(tex.format, TextureFormat::Dxt1);
    assert_eq!((tex.width, tex.height), (128, 128));
    assert_eq!((tex.reference_width, tex.reference_height), (128, 128));
    assert_eq!(tex.average_color, Color::new(0x44, 0x3a, 0x3c, 0xff));
    assert_eq!(tex.mipmap_count(), 5);
    assert_eq!(tex.mipmap_dimensions(1), (64, 64));
    assert_eq!(tex.mipmap_dimensions(4), (8, 8));
    assert!(tex.palette().is_none());
}

/// ZenKit's expected output was decoded with libsquish; texpresso is a port of it, so the pixels
/// must match exactly.
#[test]
fn erz_decodes_like_zenkit() {
    let tex = Texture::from_bytes(&sample("erz.tex")).unwrap();
    let rgba = tex.to_rgba8(0).unwrap();
    assert!(rgba == sample("erz.expected.0.bin"), "decoded pixels differ from ZenKit's");

    for level in 1..tex.mipmap_count() {
        let (w, h) = tex.mipmap_dimensions(level);
        assert_eq!(tex.to_rgba8(level).unwrap().len(), (w * h * 4) as usize);
    }
}
