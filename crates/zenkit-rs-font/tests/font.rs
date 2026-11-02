//! The samples are ZenKit's, from Gothic and Gothic II; the checks are ZenKit's.

use std::path::Path;

use zenkit_rs_core::math::Vec2;
use zenkit_rs_font::Font;
use zenkit_rs_font::GLYPHS;
use zenkit_rs_font::Glyph;

fn sample(game: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../samples/{game}/FONT_OLD_10_WHITE_HI.FNT"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn glyph(width: u8, top_left: [f32; 2], bottom_right: [f32; 2]) -> Glyph {
    Glyph { width, top_left: Vec2::from(top_left), bottom_right: Vec2::from(bottom_right) }
}

#[test]
fn reads_both_games_fonts() {
    let g1 = Font::from_bytes(&sample("G1")).unwrap();
    assert_eq!(
        (g1.name.as_str(), g1.height, g1.glyphs.len()),
        ("FONT_OLD_10_WHITE_HI.TGA", 17, 256)
    );
    assert_eq!(g1.glyphs[0], Glyph::default());
    assert_eq!(g1.glyphs[127], glyph(8, [0.398_437_5, 0.238_281_25], [0.412_109_38, 0.308_593_75]));
    assert_eq!(
        g1.glyphs[255],
        glyph(9, [0.957_031_25, 0.550_781_25], [0.972_656_25, 0.621_093_75])
    );

    let g2 = Font::from_bytes(&sample("G2")).unwrap();
    assert_eq!(
        (g2.name.as_str(), g2.height, g2.glyphs.len()),
        ("FONT_OLD_10_WHITE_HI.TGA", 18, 256)
    );
    assert_eq!(g2.glyphs[127], glyph(8, [0.398_437_5, 0.238_281_25], [0.412_109_38, 0.308_593_75]));
    assert_eq!(g2.glyphs[255], glyph(10, [0.958_984_4, 0.550_781_25], [0.976_562_5, 0.621_093_75]));
}

#[test]
fn writes_both_games_fonts_back() {
    for game in ["G1", "G2"] {
        let data = sample(game);
        assert!(Font::from_bytes(&data).unwrap().to_bytes().unwrap() == data, "{game}");
    }
}

#[test]
fn looks_characters_up_by_their_windows_1252_byte() {
    let font = Font::from_bytes(&sample("G2")).unwrap();
    assert_eq!(font.glyph('ÿ'), font.glyphs.get(255));
    assert_eq!(font.glyph('€'), font.glyphs.get(0x80));
    assert!(font.glyph('Ж').is_none());

    let a = font.glyph('A').unwrap();
    assert!(a.is_present() && !font.glyphs[0].is_present());
    let (left, top, right, bottom) = a.pixels(256, 256);
    assert!(left < right && top < bottom && right <= 256.0);
}

/// A height of 10 is a line break; reading the name must not swallow it.
#[test]
fn a_height_of_ten_survives() {
    let font = Font { name: "TINY.TGA".into(), height: 10, glyphs: vec![Glyph::default(); GLYPHS] };
    let data = font.to_bytes().unwrap();
    assert_eq!(Font::from_bytes(&data).unwrap(), font);
}

#[test]
fn broken_fonts_fail() {
    assert!(Font::from_bytes(b"2\nX.TGA\n").is_err());
    let data = sample("G1");
    for len in (0..data.len()).step_by(7) {
        let _ = Font::from_bytes(&data[..len]);
    }
    assert!(Font::from_bytes(&data[..data.len() - 1]).is_err());

    let broken = Font { name: "TWO\nLINES".into(), ..Font::default() };
    assert!(broken.to_bytes().is_err());
}
