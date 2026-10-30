use std::path::Path;
use std::path::PathBuf;

use zenkit_rs_core::Error;
use zenkit_rs_vfs::Node;
use zenkit_rs_vfs::Overwrite;
use zenkit_rs_vfs::Vfs;

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples").join(name)
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
    vfs.mount_disk(sample("basic.vdf"), Overwrite::Older).unwrap();
    check(&vfs);
}

#[test]
fn mount_host() {
    let mut vfs = Vfs::new();
    vfs.mount_host(sample("basic.vdf.dir"), "/", Overwrite::All).unwrap();
    check(&vfs);
}

// GPL-3.0.md differs by a few bytes between the two upstream samples, so it is left out.
#[test]
fn disk_and_host_contents_match() {
    let mut disk = Vfs::new();
    disk.mount_disk(sample("basic.vdf"), Overwrite::Older).unwrap();
    let mut host = Vfs::new();
    host.mount_host(sample("basic.vdf.dir"), "/", Overwrite::All).unwrap();

    for path in ["README.md", "config.yml", "licenses/MIT.md", "licenses/gpl/LGPL-3.0.md"] {
        let a = disk.resolve(path).unwrap().data().unwrap();
        let b = host.resolve(path).unwrap().data().unwrap();
        assert_eq!(a, b, "{path}");
    }
}

#[test]
fn mount_disks_resolves_collisions_in_order() {
    let original = std::fs::read(sample("basic.vdf")).unwrap();
    let mut patched = original.clone();
    let mit = patched.windows(17).position(|w| w == b"# The MIT license").unwrap();
    patched[mit..mit + 5].copy_from_slice(b"# NEW");

    let dir = std::env::temp_dir().join(format!("zenkit-rs-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.vdf"), dir.join("b.vdf"));
    std::fs::write(&a, &original).unwrap();
    std::fs::write(&b, &patched).unwrap();

    let mut vfs = Vfs::new();
    vfs.mount_disks(&[&a, &b], Overwrite::All).unwrap();
    assert!(vfs.find("MIT.md").unwrap().data().unwrap().starts_with(b"# NEW"));

    let mut vfs = Vfs::new();
    vfs.mount_disks(&[&b, &a], Overwrite::All).unwrap();
    assert!(vfs.find("MIT.md").unwrap().data().unwrap().starts_with(b"# The"));

    let mut vfs = Vfs::new();
    let missing = dir.join("missing.vdf");
    assert!(vfs.mount_disks(&[&a, &missing], Overwrite::All).is_err());
    assert_eq!(vfs.root().children().len(), 0);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn mount_disk_reports_the_path_on_io_errors() {
    let mut vfs = Vfs::new();
    let err = vfs.mount_disk(sample("missing.vdf"), Overwrite::Older).unwrap_err();
    assert!(matches!(err, Error::Io { ref path, .. } if path.ends_with("missing.vdf")));
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

    let err = Vfs::new().mount_disk_bytes(bytes, Overwrite::Older).unwrap_err();
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

    let err = Vfs::new().mount_disk_bytes(bytes, Overwrite::Older).unwrap_err();
    assert!(matches!(err, Error::Malformed { .. }), "{err}");
}

#[test]
fn every_node_knows_its_path() {
    let mut disk = Vfs::new();
    disk.mount_disk(sample("basic.vdf"), Overwrite::Older).unwrap();
    let mut host = Vfs::new();
    host.mkdir("/_WORK").unwrap();
    host.mount_host(sample("basic.vdf.dir"), "/_WORK", Overwrite::All).unwrap();

    assert_eq!(disk.root().path(), "/");
    assert_eq!(disk.find("gpl-3.0.md").unwrap().path(), "/LICENSES/GPL/GPL-3.0.MD");
    assert_eq!(disk.resolve("licenses").unwrap().path(), "/LICENSES");
    assert_eq!(host.find("gpl-3.0.md").unwrap().path(), "/_WORK/licenses/gpl/GPL-3.0.md");

    // Every path leads back to its node.
    for vfs in [&disk, &host] {
        for node in vfs.nodes() {
            assert!(std::ptr::eq(vfs.resolve(node.path()).unwrap(), node), "{}", node.path());
        }
    }
}

#[test]
fn paths_follow_nodes_around() {
    let time = std::time::UNIX_EPOCH;
    let mut vfs = Vfs::new();
    vfs.mount_disk(sample("basic.vdf"), Overwrite::Older).unwrap();

    let mut licenses = vfs.remove("/LICENSES").unwrap();
    assert_eq!(licenses.path(), "LICENSES");
    assert_eq!(licenses.child("gpl").unwrap().path(), "LICENSES/GPL");
    licenses.insert(Node::file("NEW.MD", *b"", time)).unwrap();
    assert_eq!(licenses.child("new.md").unwrap().path(), "LICENSES/NEW.MD");

    vfs.mkdir("/docs/old").unwrap().insert(licenses).unwrap();
    assert_eq!(vfs.resolve("docs").unwrap().path(), "/docs");
    assert_eq!(vfs.find("mit.md").unwrap().path(), "/docs/old/LICENSES/MIT.MD");
    assert_eq!(vfs.find("new.md").unwrap().path(), "/docs/old/LICENSES/NEW.MD");

    vfs.mount(Node::file("readme.md", *b"newer", time), "/docs", Overwrite::All).unwrap();
    assert_eq!(vfs.resolve("docs/readme.md").unwrap().path(), "/docs/readme.md");
}

#[test]
fn nodes_lists_directories_and_files() {
    let mut vfs = Vfs::new();
    vfs.mount_disk(sample("basic.vdf"), Overwrite::Older).unwrap();

    let paths: Vec<&str> = vfs.nodes().map(|n| n.path()).collect();
    assert_eq!(
        paths,
        [
            "/CONFIG.YML",
            "/LICENSES",
            "/LICENSES/GPL",
            "/LICENSES/GPL/GPL-3.0.MD",
            "/LICENSES/GPL/LGPL-3.0.MD",
            "/LICENSES/MIT.MD",
            "/README.MD",
        ]
    );
    assert_eq!(vfs.nodes().filter(|n| n.is_dir()).count(), 2);
    let files: Vec<&str> = vfs.files().map(|n| n.path()).collect();
    let from_nodes: Vec<&str> = vfs.nodes().filter(|n| n.is_file()).map(|n| n.path()).collect();
    assert_eq!(files, from_nodes);
    assert_eq!(vfs.resolve("licenses").unwrap().descendants().count(), 4);
}
