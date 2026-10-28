#![cfg(feature = "vfs")]

use std::path::{Path, PathBuf};

use zenkit::Error;
use zenkit::vfs::{Overwrite, Vfs};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/samples")
        .join(name)
}

fn check(vfs: &Vfs) {
    assert_eq!(vfs.root().children().len(), 3);

    for name in ["config.yml", "readme.md", "MIT.MD", "gpl-3.0.MD"] {
        assert!(vfs.find(name).unwrap().is_file(), "{name}");
    }

    let licenses = vfs.find("licenses").unwrap();
    assert!(licenses.is_dir());
    assert_eq!(licenses.children().len(), 2);

    let gpl = licenses.child("gpl").unwrap();
    assert!(gpl.is_dir());
    assert_eq!(gpl.children().len(), 2);
    assert!(gpl.child("lgpl-3.0.md").unwrap().is_file());
    assert!(gpl.child("lgpl").is_none());

    assert!(vfs.find("lGpL-3.0.Md").is_some());
    assert!(vfs.find("liceNSES").is_some());
    assert!(vfs.find("nonexistent").is_none());
    assert!(vfs.find("").is_none());

    assert!(vfs.resolve("licEnSES/GPL/gpl-3.0.md").is_some());
    assert!(vfs.resolve("licEnSES/GPL/nonexistent").is_none());
    assert!(vfs.resolve("/LICENSES").is_some());
    assert_eq!(vfs.resolve("").unwrap().name(), "/");
    assert_eq!(vfs.resolve("/").unwrap().name(), "/");

    assert!(vfs.find("config.yml ").is_some());
    assert!(vfs.resolve("licEnSES /GPL/gpl-3.0.md ").is_some());

    let mit = vfs.find("mit.md").unwrap().data().unwrap();
    assert!(mit.starts_with(b"# The MIT license"));
}

#[test]
fn mount_disk() {
    let mut vfs = Vfs::new();
    vfs.mount_disk(sample("basic.vdf"), Overwrite::Older)
        .unwrap();
    check(&vfs);
}

#[test]
fn mount_host() {
    let mut vfs = Vfs::new();
    vfs.mount_host(sample("basic.vdf.dir"), "/", Overwrite::All)
        .unwrap();
    check(&vfs);
}

// GPL-3.0.md differs by a few bytes between the two upstream samples, so it is left out.
#[test]
fn disk_and_host_contents_match() {
    let mut disk = Vfs::new();
    disk.mount_disk(sample("basic.vdf"), Overwrite::Older)
        .unwrap();
    let mut host = Vfs::new();
    host.mount_host(sample("basic.vdf.dir"), "/", Overwrite::All)
        .unwrap();

    for path in [
        "README.md",
        "config.yml",
        "licenses/MIT.md",
        "licenses/gpl/LGPL-3.0.md",
    ] {
        let a = disk.resolve(path).unwrap().data().unwrap();
        let b = host.resolve(path).unwrap().data().unwrap();
        assert_eq!(a, b, "{path}");
    }
}

#[test]
fn mount_disks_resolves_collisions_in_order() {
    let original = std::fs::read(sample("basic.vdf")).unwrap();
    let mut patched = original.clone();
    let mit = patched
        .windows(17)
        .position(|w| w == b"# The MIT license")
        .unwrap();
    patched[mit..mit + 5].copy_from_slice(b"# NEW");

    let dir = std::env::temp_dir().join(format!("zenkit-rs-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.vdf"), dir.join("b.vdf"));
    std::fs::write(&a, &original).unwrap();
    std::fs::write(&b, &patched).unwrap();

    let mut vfs = Vfs::new();
    vfs.mount_disks(&[&a, &b], Overwrite::All).unwrap();
    assert!(
        vfs.find("MIT.md")
            .unwrap()
            .data()
            .unwrap()
            .starts_with(b"# NEW")
    );

    let mut vfs = Vfs::new();
    vfs.mount_disks(&[&b, &a], Overwrite::All).unwrap();
    assert!(
        vfs.find("MIT.md")
            .unwrap()
            .data()
            .unwrap()
            .starts_with(b"# The")
    );

    let mut vfs = Vfs::new();
    let missing = dir.join("missing.vdf");
    assert!(vfs.mount_disks(&[&a, &missing], Overwrite::All).is_err());
    assert_eq!(vfs.root().children().len(), 0);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn mount_disk_reports_the_path_on_io_errors() {
    let mut vfs = Vfs::new();
    let err = vfs
        .mount_disk(sample("missing.vdf"), Overwrite::Older)
        .unwrap_err();
    assert!(matches!(err, Error::File { ref path, .. } if path.ends_with("missing.vdf")));
}

#[test]
fn rejects_unknown_signature() {
    let mut bytes = std::fs::read(sample("basic.vdf")).unwrap();
    bytes[256..272].copy_from_slice(b"NOT_A_VDF_AT_ALL");

    let mut vfs = Vfs::new();
    let err = vfs.mount_disk_bytes(bytes, Overwrite::Older).unwrap_err();
    assert!(matches!(err, Error::Malformed { .. }), "{err}");
    assert_eq!(vfs.root().children().len(), 0);
}

#[test]
fn rejects_compressed_archives_for_now() {
    let mut bytes = std::fs::read(sample("basic.vdf")).unwrap();
    bytes[292..296].copy_from_slice(&0xA0u32.to_le_bytes());

    let err = Vfs::new()
        .mount_disk_bytes(bytes, Overwrite::Older)
        .unwrap_err();
    assert!(matches!(err, Error::Unsupported { .. }), "{err}");
}

#[test]
fn truncated_archives_fail_cleanly() {
    let bytes = std::fs::read(sample("basic.vdf")).unwrap();
    for len in (0..bytes.len()).step_by(13) {
        let mut vfs = Vfs::new();
        let _ = vfs.mount_disk_bytes(&bytes[..len], Overwrite::Older);
    }
}

#[test]
fn self_referencing_directory_is_rejected() {
    let mut bytes = std::fs::read(sample("basic.vdf")).unwrap();
    let catalog = u32::from_le_bytes(bytes[288..292].try_into().unwrap()) as usize;

    let first_dir = (0..)
        .map(|i| catalog + i * 80)
        .find(|&entry| bytes[entry + 72..entry + 76][3] & 0x80 != 0)
        .unwrap();
    let index = ((first_dir - catalog) / 80) as u32;
    bytes[first_dir + 64..first_dir + 68].copy_from_slice(&index.to_le_bytes());

    let err = Vfs::new()
        .mount_disk_bytes(bytes, Overwrite::Older)
        .unwrap_err();
    assert!(matches!(err, Error::Malformed { .. }), "{err}");
}

#[cfg(feature = "texture")]
#[test]
fn load_all_parses_in_order_and_reports_each_failure() {
    use zenkit::texture::Texture;
    use zenkit::vfs::Node;

    let tex = std::fs::read(sample("erz.tex")).unwrap();
    let mut vfs = Vfs::new();
    for (name, data) in [
        ("A.TEX", tex.clone()),
        ("BROKEN.TEX", b"ZTEX".to_vec()),
        ("C.TEX", tex),
    ] {
        vfs.mount(
            Node::file(name, data, std::time::UNIX_EPOCH),
            "/",
            Overwrite::All,
        )
        .unwrap();
    }

    let results = vfs.load_all::<Texture, _>(&["c.tex", "broken.tex", "missing.tex", "a.tex"]);
    assert_eq!(results.len(), 4);
    assert_eq!(results[0].as_ref().unwrap().width, 128);
    assert!(matches!(results[1], Err(Error::UnexpectedEof { .. })));
    assert!(matches!(results[2], Err(Error::NotFound(_))));
    assert!(results[3].is_ok());
}
