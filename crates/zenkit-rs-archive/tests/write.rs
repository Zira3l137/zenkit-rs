use zenkit_rs_archive::Archive;
use zenkit_rs_archive::ArchiveWriter;
use zenkit_rs_archive::Format;
use zenkit_rs_archive::Header;
use zenkit_rs_core::Error;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Color;
use zenkit_rs_core::math::Mat3;
use zenkit_rs_core::math::Vec2;
use zenkit_rs_core::math::Vec3;

const FORMATS: [Format; 3] = [Format::Ascii, Format::Binary, Format::Binsafe];

const FLOATS: [f32; 7] = [0.1, 60.0, -0.0, 1e-30, f32::MAX, f32::MIN_POSITIVE / 8.0, 1.0 / 3.0];

fn rotation() -> Mat3 {
    Mat3::from_cols_array(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])
}

fn bounds() -> Aabb {
    Aabb { min: Vec3::new(-1.5, -2.0, -3.25), max: Vec3::new(4.0, 5.0, 6.0) }
}

/// One object of every kind of value, a nested object, a null object, a reference back to the
/// outer object and a blob written straight into the stream.
fn write(format: Format) -> Vec<u8> {
    let mut w = ArchiveWriter::new(format);
    let outer = w.begin_object("%", "zCVob", 52224);
    w.string("name", b"Gr\xFC\xDFe");
    w.name("preset", "\u{c4}rger");
    w.int("int", -12345);
    for f in FLOATS {
        w.float("float", f);
    }
    w.byte("byte", 200);
    w.word("word", 60000);
    w.enumeration("enum", 3);
    w.bool("yes", true);
    w.bool("no", false);
    w.color("color", Color::new(1, 2, 3, 255));
    w.vec2("vec2", Vec2::new(0.5, -0.25));
    w.vec3("vec3", Vec3::new(1.0, 0.1, -1e-7));
    w.aabb("bbox", bounds());
    w.mat3("rotation", rotation());
    w.raw("raw", &[0x00, 0x0F, 0xA7, 0xFF]);

    w.begin_object("visual", "zCMesh", 0);
    w.string("name", b"");
    w.end_object();
    w.null_object("ai");
    w.reference("self", outer);

    w.begin_object("MeshAndBsp", "%", 0);
    w.write_stream(|s| s.bytes(&[0xAA, 0xBB, 0xCC, 0xDD]));
    w.end_object();

    w.int("after", 7);
    w.end_object();
    w.finish().unwrap()
}

#[test]
fn every_value_reads_back() {
    for format in FORMATS {
        let data = write(format);
        let mut r = Archive::new(&data).unwrap();
        assert_eq!(r.format(), format);

        let outer = r.expect_object(b"zCVob").unwrap();
        assert_eq!((outer.version, outer.index), (52224, 0));
        assert_eq!(r.string().unwrap(), b"Gr\xFC\xDFe");
        assert_eq!(r.name().unwrap(), "\u{c4}rger");
        assert_eq!(r.int().unwrap(), -12345);
        for f in FLOATS {
            assert_eq!(r.float().unwrap().to_bits(), f.to_bits(), "{f} in {format:?}");
        }
        assert_eq!(r.byte().unwrap(), 200);
        assert_eq!(r.word().unwrap(), 60000);
        assert_eq!(r.enumeration().unwrap(), 3);
        assert!(r.bool().unwrap());
        assert!(!r.bool().unwrap());
        assert_eq!(r.color().unwrap(), Color::new(1, 2, 3, 255));
        assert_eq!(r.vec2().unwrap(), Vec2::new(0.5, -0.25));
        assert_eq!(r.vec3().unwrap(), Vec3::new(1.0, 0.1, -1e-7));
        assert_eq!(r.aabb().unwrap(), bounds());
        assert_eq!(r.mat3().unwrap(), rotation());
        assert_eq!(&*r.raw(4).unwrap(), [0x00, 0x0F, 0xA7, 0xFF]);

        let visual = r.expect_object(b"zCMesh").unwrap();
        assert_eq!((visual.name, visual.index), (&b"visual"[..], 1));
        assert_eq!(r.string().unwrap(), b"");
        assert!(r.end_object().unwrap());

        let null = r.begin_object().unwrap().unwrap();
        assert!(null.is_null() && null.name == b"ai");
        assert!(r.end_object().unwrap());

        let reference = r.begin_object().unwrap().unwrap();
        assert!(reference.is_reference());
        assert_eq!(reference.index, outer.index);
        assert!(r.end_object().unwrap());

        // The stream object takes an index, so a reference to the mesh would find it.
        assert_eq!(r.begin_object().unwrap().unwrap().index, 2);
        assert_eq!(r.read_stream(|s| s.u32()).unwrap(), 0xDDCC_BBAA);
        assert!(r.end_object().unwrap(), "{format:?}");

        assert_eq!(r.int().unwrap(), 7);
        assert!(r.end_object().unwrap());
        assert!(r.is_eof(), "{format:?}");
    }
}

#[test]
fn ascii_looks_like_the_engines() {
    let mut header = Header::new(Format::Ascii);
    header.date = Some("8.10.2002 18:37:24".into());
    header.user = Some("Tom".into());
    let mut w = ArchiveWriter::with_header(header);
    w.begin_object("%", "zCMaterial", 17408);
    w.name("name", "BODY");
    w.float("smoothAngle", 60.0);
    w.vec3("pos", Vec3::new(-5765.5747, 0.5, 2.0));
    w.aabb("bbox", Aabb { min: Vec3::ZERO, max: Vec3::ONE });
    w.byte("alpha", 255);
    w.mat3("rot", Mat3::IDENTITY);
    w.null_object("visual");
    w.end_object();

    let expected = "ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\n\
                    date 8.10.2002 18:37:24\nuser Tom\nEND\nobjects 1        \nEND\n\n\
                    [% zCMaterial 17408 0]\n\
                    \tname=string:BODY\n\
                    \tsmoothAngle=float:60\n\
                    \tpos=vec3:-5765.5747 0.5 2\n\
                    \tbbox=rawFloat:0 0 0 1 1 1 \n\
                    \talpha=int:255\n\
                    \trot=raw:0000803f0000000000000000000000000000803f0000000000000000000000000000803f\n\
                    \t[visual % 0 0]\n\
                    \t[]\n\
                    []\n";
    assert_eq!(String::from_utf8(w.finish().unwrap()).unwrap(), expected);
}

/// Where the encoded objects start, after the header's last line.
fn body_start(data: &[u8], end: &[u8]) -> usize {
    let first_end = data.windows(4).position(|w| w == b"END\n").unwrap() + 4;
    first_end + data[first_end..].windows(end.len()).position(|w| w == end).unwrap() + end.len()
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

#[test]
fn binary_objects_know_their_size() {
    let mut w = ArchiveWriter::new(Format::Binary);
    w.begin_object("%", "zCMaterial", 17408);
    w.int("value", 1);
    w.end_object();
    let data = w.finish().unwrap();

    let start = body_start(&data, b"END\n\n");
    let size = 4 + 2 + 4 + "%\0zCMaterial\0".len() + 4;
    assert_eq!(u32_at(&data, start) as usize, size);
    assert_eq!(data.len() - start, size);
}

#[test]
fn binsafe_lists_every_key_once() {
    let mut w = ArchiveWriter::new(Format::Binsafe);
    w.begin_object("%", "zCVob", 0);
    w.int("ab", 1);
    w.int("b", 2);
    w.int("ab", 3);
    w.end_object();
    let data = w.finish().unwrap();

    let header = data.windows(4).position(|w| w == b"END\n").unwrap() + 4;
    assert_eq!((u32_at(&data, header), u32_at(&data, header + 4)), (2, 1));
    let table = u32_at(&data, header + 8) as usize;

    let mut expected = 2u32.to_le_bytes().to_vec();
    for (index, key, hash) in [(0u16, "ab", (0x61 * 0x21 + 0x62) % 0x61), (1, "b", 0x62 % 0x61)] {
        expected.extend((key.len() as u16).to_le_bytes());
        expected.extend(index.to_le_bytes());
        expected.extend((hash as u32).to_le_bytes());
        expected.extend(key.as_bytes());
    }
    assert_eq!(&data[table..], expected);
}

#[test]
fn values_that_do_not_fit_fail_at_the_end() {
    let unwritable = |data: Result<Vec<u8>, Error>| match data {
        Err(Error::Unwritable { reason, .. }) => reason,
        other => panic!("expected an unwritable value, got {other:?}"),
    };

    for format in [Format::Ascii, Format::Binary] {
        let mut w = ArchiveWriter::new(format);
        w.string("text", b"two\nlines");
        assert!(unwritable(w.finish()).contains("\"text\""));
    }

    let mut w = ArchiveWriter::new(Format::Binsafe);
    w.raw("blob", &vec![0; 70000]);
    w.int("later", 1);
    assert!(unwritable(w.finish()).contains("70000 bytes"));

    let mut w = ArchiveWriter::new(Format::Ascii);
    w.name("name", "\u{416}");
    assert!(unwritable(w.finish()).contains("Latin-1"));

    let mut w = ArchiveWriter::new(Format::Binsafe);
    w.begin_object("%", "zCVob", 0);
    assert!(unwritable(w.finish()).contains("still open"));

    let mut w = ArchiveWriter::new(Format::Binary);
    w.end_object();
    assert!(unwritable(w.finish()).contains("none is open"));
}

#[test]
fn headers_round_trip() {
    for format in FORMATS {
        let mut header = Header::new(format);
        header.save_game = true;
        header.user = Some("someone".into());
        let data = ArchiveWriter::with_header(header.clone()).finish().unwrap();
        let archive = Archive::new(&data).unwrap();
        assert_eq!(archive.header(), &header);
        assert!(archive.is_eof());
    }
}
