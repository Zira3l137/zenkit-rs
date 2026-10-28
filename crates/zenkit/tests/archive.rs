#![cfg(feature = "archive")]

use std::path::{Path, PathBuf};

use zenkit::Error;
use zenkit::archive::{Archive, Format};
use zenkit::math::{Color, Vec2, Vec3};

fn sample(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/samples")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Walks every entry of a self-describing archive. Returns how many objects it saw and how many
/// were still open when the data ended.
fn walk(archive: &mut Archive) -> zenkit::Result<(usize, usize)> {
    let (mut depth, mut objects) = (0usize, 0);
    while !archive.is_eof() {
        if archive.begin_object()?.is_some() {
            depth += 1;
            objects += 1;
        } else if depth > 0 && archive.end_object()? {
            depth -= 1;
        } else {
            archive.skip_entry()?;
        }
    }
    Ok((objects, depth))
}

#[test]
fn ascii() {
    let data = sample("ascii.zen");
    let mut ar = Archive::new(&data).unwrap();
    assert_eq!(ar.format(), Format::Ascii);
    assert_eq!(ar.header().user.as_deref(), Some("luis"));

    let root = ar.begin_object().unwrap().unwrap();
    assert!(ar.begin_object().unwrap().is_none());
    assert_eq!(root.name, b"root_obj");
    assert!(root.is_null());
    assert_eq!((root.version, root.index), (21237, 1));

    assert_eq!(ar.string().unwrap(), b"a basic string");

    // The next entry is a bool. Unlike ZenKit, a failed read consumes nothing.
    assert!(matches!(ar.string(), Err(Error::Malformed { .. })));
    assert!(ar.bool().unwrap());

    ar.skip_next_object().unwrap();

    assert_eq!(ar.int().unwrap(), 672);
    assert_eq!(ar.float().unwrap(), 12.12421);
    assert_eq!(ar.byte().unwrap(), 255);
    assert_eq!(ar.word().unwrap(), 32621);
    assert_eq!(ar.enumeration().unwrap(), 6);
    assert!(ar.bool().unwrap());
    assert_eq!(ar.color().unwrap(), Color::new(1, 2, 3, 255));
    assert_eq!(ar.vec3().unwrap(), Vec3::new(50.0, 100.123, -150.0));
    assert_eq!(ar.vec2().unwrap(), Vec2::new(111.11, -12.99));

    let aabb = ar.aabb().unwrap();
    assert_eq!(aabb.min, Vec3::new(12.0, 34.0, 56.0));
    assert_eq!(aabb.max, Vec3::new(78.0, 89.0, 0.0));

    let m = ar.mat3().unwrap();
    assert_eq!(m.row(0), Vec3::new(0.994_702_8, 0.0, 0.102_792_54));
    assert_eq!(m.row(1), Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(m.row(2), Vec3::new(-0.102_792_54, 0.0, 0.994_702_8));

    assert_eq!(&*ar.raw(4).unwrap(), &[0xf2, 0x42, 0xa7, 0x10]);

    let child = ar.begin_object().unwrap().unwrap();
    assert!(ar.begin_object().unwrap().is_none());
    assert_eq!(child.name, b"child_obj");
    assert_eq!(child.class, b"test:class:name");
    assert_eq!((child.version, child.index), (7323, 2));

    assert!(ar.end_object().unwrap());
    assert!(ar.end_object().unwrap());
    assert!(ar.float().is_err());
    assert!(ar.is_eof());
}

#[test]
fn binary() {
    let data = sample("binary.zen");
    let mut ar = Archive::new(&data).unwrap();
    assert_eq!(ar.format(), Format::Binary);
    assert_eq!(ar.string().unwrap(), b"DT_BOOKSHELF_V1_1");

    let material = ar.expect_object(b"zCMaterial").unwrap();
    assert_eq!(material.name, b"%");
    assert_eq!((material.version, material.index), (17408, 0));

    assert_eq!(ar.name().unwrap(), "DT_BOOKSHELF_V1_1");
    assert_eq!(ar.enumeration().unwrap(), 0);
    assert_eq!(ar.color().unwrap(), Color::new(0x2A, 0x23, 0x19, 0xFF));
    assert_eq!(ar.float().unwrap(), 60.0);
    assert_eq!(ar.string().unwrap(), b"MODIBOOKS01.TGA");
    assert_eq!(ar.string().unwrap(), b"256 256");
    assert_eq!(ar.float().unwrap(), 0.0);

    assert!(!ar.end_object().unwrap());
    ar.skip_object().unwrap();

    assert_eq!(ar.string().unwrap(), b"DT_BOOKSHELF_V1_2");
    assert!(ar.expect_object(b"zCMesh").is_err());
    ar.expect_object(b"zCMaterial").unwrap();
    assert!(matches!(ar.skip_entry(), Err(Error::Unsupported { .. })));
}

#[test]
fn ascii_save_info() {
    for (game, title) in [("G1", "sds"), ("G2", "uwuowo")] {
        let data = sample(&format!("{game}/SAVEINFO.SAV"));
        let mut ar = Archive::new(&data).unwrap();
        ar.expect_object(b"oCSavegameInfo").unwrap();
        assert_eq!(ar.string().unwrap(), title.as_bytes());
        ar.skip_object().unwrap();
        assert!(ar.is_eof(), "{game}");
    }
}

/// Writes binsafe archives, so tests do not need the large binsafe samples from ZenKit.
#[derive(Default)]
struct Binsafe {
    body: Vec<u8>,
    keys: Vec<&'static str>,
}

impl Binsafe {
    fn value(&mut self, key: &'static str, tag: u8, payload: &[u8]) -> &mut Self {
        self.body.push(0x12);
        self.body.extend((self.keys.len() as u32).to_le_bytes());
        self.keys.push(key);
        self.body.push(tag);
        if matches!(tag, 0x01 | 0x09 | 0x10) {
            self.body.extend((payload.len() as u16).to_le_bytes());
        }
        self.body.extend(payload);
        self
    }

    fn object(&mut self, header: &str) -> &mut Self {
        self.body.push(0x01);
        self.body.extend((header.len() as u16).to_le_bytes());
        self.body.extend(header.as_bytes());
        self
    }

    fn end(&mut self) -> &mut Self {
        self.object("[]")
    }

    fn finish(&self) -> Vec<u8> {
        let mut out =
            b"ZenGin Archive\nver 1\nzCArchiverBinSafe\nBIN_SAFE\nsaveGame 0\nEND\n".to_vec();
        let table = out.len() + 12 + self.body.len();
        for v in [2u32, 1, table as u32] {
            out.extend(v.to_le_bytes());
        }
        out.extend(&self.body);
        out.extend((self.keys.len() as u32).to_le_bytes());
        for (i, key) in self.keys.iter().enumerate() {
            out.extend((key.len() as u16).to_le_bytes());
            out.extend((i as u16).to_le_bytes());
            out.extend(0u32.to_le_bytes());
            out.extend(key.as_bytes());
        }
        out
    }
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn binsafe_sample() -> Vec<u8> {
    let mut mat = [0f32; 9];
    mat[0] = 1.0;
    mat[1] = 2.0;
    mat[4] = 1.0;
    mat[8] = 1.0;

    Binsafe::default()
        .object("[% zCVob 52224 1]")
        .value("name", 0x01, b"VOB")
        .value("count", 0x02, &(-5i32).to_le_bytes())
        .value("speed", 0x03, &1.5f32.to_le_bytes())
        .value("flag", 0x04, &[7])
        .value("word", 0x05, &513u16.to_le_bytes())
        .value("visible", 0x06, &1u32.to_le_bytes())
        .value("position", 0x07, &floats(&[1.0, 2.0, 3.0]))
        .value("color", 0x08, &[3, 2, 1, 255])
        .value("rotation", 0x09, &floats(&mat))
        .value("mapping", 0x10, &floats(&[4.0, 5.0, 99.0]))
        .value("bbox", 0x10, &floats(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]))
        .value("mode", 0x11, &3u32.to_le_bytes())
        .object("[visual zCMesh 0 2]")
        .value("nested", 0x01, b"x")
        .object("[% % 0 0]")
        .end()
        .end()
        .value("data", 0x09, &[1, 2, 3, 4, 5])
        .end()
        .finish()
}

#[test]
fn binsafe() {
    let data = binsafe_sample();
    let mut ar = Archive::new(&data).unwrap();
    assert_eq!(ar.format(), Format::Binsafe);

    let vob = ar.expect_object(b"zCVob").unwrap();
    assert_eq!((vob.version, vob.index), (52224, 1));

    assert_eq!(ar.string().unwrap(), b"VOB");

    let err = ar.float().unwrap_err().to_string();
    assert!(err.contains("\"count\""), "{err}");
    assert_eq!(ar.int().unwrap(), -5);

    assert_eq!(ar.float().unwrap(), 1.5);
    assert_eq!(ar.byte().unwrap(), 7);
    assert_eq!(ar.word().unwrap(), 513);
    assert!(ar.bool().unwrap());
    assert_eq!(ar.vec3().unwrap(), Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(ar.color().unwrap(), Color::new(1, 2, 3, 255));
    assert_eq!(ar.mat3().unwrap().row(0), Vec3::new(1.0, 2.0, 0.0));
    assert_eq!(ar.vec2().unwrap(), Vec2::new(4.0, 5.0));
    assert_eq!(ar.aabb().unwrap().max, Vec3::new(3.0, 4.0, 5.0));
    assert_eq!(ar.enumeration().unwrap(), 3);

    let visual = ar.begin_object().unwrap().unwrap();
    assert_eq!(visual.name, b"visual");
    ar.skip_object().unwrap();

    assert!(ar.raw(6).is_err());
    assert_eq!(&*ar.raw(2).unwrap(), &[1, 2, 3, 4, 5]);
    assert!(ar.end_object().unwrap());
    assert!(ar.is_eof());
}

#[test]
fn walks_self_describing_archives() {
    let data = binsafe_sample();
    assert_eq!(walk(&mut Archive::new(&data).unwrap()).unwrap(), (3, 0));

    let data = sample("G2/SAVEINFO.SAV");
    assert_eq!(walk(&mut Archive::new(&data).unwrap()).unwrap(), (1, 0));
}

#[test]
fn truncated_archives_fail_cleanly() {
    for data in [sample("ascii.zen"), sample("binary.zen"), binsafe_sample()] {
        for len in 0..data.len() {
            let Ok(mut ar) = Archive::new(&data[..len]) else {
                continue;
            };
            if ar.format() == Format::Binary {
                let _ = ar.string().and_then(|_| ar.begin_object());
                let _ = ar.skip_object();
            } else {
                let _ = walk(&mut ar);
            }
        }
    }
}

/// Walks every archive in a ZenKit checkout's `tests/samples`, if `ZENKIT_SAMPLES` points at it.
/// They are too large to keep in this repository.
#[test]
fn zenkit_samples() {
    let Some(root) = std::env::var_os("ZENKIT_SAMPLES").map(PathBuf::from) else {
        eprintln!("ZENKIT_SAMPLES is not set, skipping");
        return;
    };

    let mut paths = Vec::new();
    for dir in ["G1/VOb", "G2/VOb", "G1/Save", "G2/Save"] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            paths.push(entry.unwrap().path());
        }
    }

    let mut checked = 0;
    for path in paths {
        let data = std::fs::read(&path).unwrap();
        if !data.starts_with(b"ZenGin Archive") {
            continue;
        }
        let mut ar = Archive::new(&data).unwrap();
        let (objects, open) = walk(&mut ar).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(objects > 0 && open == 0, "{}", path.display());
        checked += 1;
    }
    assert!(checked > 40, "only {checked} archives found");
}
