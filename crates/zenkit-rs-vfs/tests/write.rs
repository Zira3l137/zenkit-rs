use std::path::Path;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_vfs::DiskOptions;
use zenkit_rs_vfs::Node;
use zenkit_rs_vfs::Overwrite;
use zenkit_rs_vfs::Vfs;

fn sample() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/basic.vdf");
    std::fs::read(path).unwrap()
}

fn mount(bytes: &[u8]) -> Vfs {
    let mut vfs = Vfs::new();
    vfs.mount_disk_bytes(bytes.to_vec(), Overwrite::All).unwrap();
    vfs
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

/// Every catalog entry, with the file offsets left out since data may be laid out differently.
fn catalog(data: &[u8]) -> Vec<(Vec<u8>, u32, u32, u32, u32)> {
    (0..u32_at(data, 272) as usize)
        .map(|i| {
            let entry = &data[296 + i * 80..][..80];
            let kind = u32_at(entry, 72);
            let dir_offset = if kind & 0x8000_0000 != 0 { u32_at(entry, 64) } else { 0 };
            (entry[..64].to_vec(), dir_offset, u32_at(entry, 68), kind, u32_at(entry, 76))
        })
        .collect()
}

#[test]
fn rewrites_an_archive_as_the_original_tools_laid_it_out() {
    let original = sample();
    let vfs = mount(&original);

    let mut options = DiskOptions::new(GameVersion::Gothic2);
    options.comment = "Sample VDF".into();
    let mut written = Vec::new();
    vfs.write_disk(&mut written, &options).unwrap();

    assert_eq!(&written[..10], b"Sample VDF");
    assert!(written[10..256].iter().all(|&b| b == 0x1A));
    // Signature, entry and file counts, timestamp, data size, catalog offset and flags.
    assert_eq!(written[256..296], original[256..296]);
    assert_eq!(catalog(&written), catalog(&original));
    assert_eq!(written.len(), original.len());

    let copy = mount(&written);
    for (a, b) in vfs.files().zip(copy.files()) {
        assert_eq!((a.name(), a.data(), a.time()), (b.name(), b.data(), b.time()));
    }
    assert_eq!(vfs.files().count(), 5);
}

#[test]
fn writes_any_tree() {
    let time = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    let mut vfs = Vfs::new();
    let work = vfs.mkdir("/_work/data/empty").unwrap();
    work.insert(Node::dir("nested", time)).unwrap();
    vfs.mkdir("/_work/data/meshes")
        .unwrap()
        .insert(Node::file("chest.mrm", *b"mesh", time))
        .unwrap();
    vfs.root_mut().insert(Node::file("\u{c4}pfel.txt", *b"", time)).unwrap();

    let mut written = Vec::new();
    vfs.write_disk(&mut written, &DiskOptions::new(GameVersion::Gothic1)).unwrap();
    assert_eq!(&written[256..272], b"PSVDSC_V2.00\r\n\r\n");

    let copy = mount(&written);
    let chest = copy.resolve("_WORK/DATA/MESHES/CHEST.MRM").unwrap();
    assert_eq!((chest.name(), chest.data(), chest.time()), ("CHEST.MRM", Some(&b"mesh"[..]), time));
    assert_eq!(copy.find("\u{c4}PFEL.TXT").unwrap().data(), Some(&b""[..]));
    // Directories holding no files have nothing to mark as their last entry.
    assert!(copy.resolve("_WORK/DATA/EMPTY").is_none());
    assert_eq!(copy.resolve("_WORK/DATA").unwrap().children().len(), 1);

    let mut sub = Vec::new();
    vfs.resolve("_work/data")
        .unwrap()
        .write_disk(&mut sub, &DiskOptions::new(GameVersion::Gothic2))
        .unwrap();
    assert!(mount(&sub).resolve("MESHES/CHEST.MRM").is_some());
}

#[test]
fn refuses_what_a_vdf_cannot_hold() {
    let options = DiskOptions::new(GameVersion::Gothic2);
    let time = UNIX_EPOCH;
    let fails = |vfs: &Vfs| match vfs.write_disk(Vec::new(), &options) {
        Err(Error::Unwritable { reason, .. }) => reason,
        other => panic!("expected an unwritable archive, got {other:?}"),
    };

    let mut vfs = Vfs::new();
    vfs.mkdir("/empty").unwrap();
    assert!(fails(&vfs).contains("at least one file"));

    vfs.root_mut().insert(Node::file("N".repeat(65), *b"", time)).unwrap();
    assert!(fails(&vfs).contains("at most 64"));

    let mut vfs = Vfs::new();
    vfs.root_mut().insert(Node::file("\u{416}.txt", *b"", time)).unwrap();
    assert!(fails(&vfs).contains("Latin-1"));

    let file = Node::file("file", *b"", time);
    assert!(matches!(file.write_disk(Vec::new(), &options), Err(Error::NotADirectory(_))));
}

#[test]
fn reports_failing_outputs() {
    struct Full;
    impl std::io::Write for Full {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::StorageFull.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let vfs = mount(&sample());
    let result = vfs.write_disk(Full, &DiskOptions::new(GameVersion::Gothic2));
    assert!(matches!(result, Err(Error::Output(e)) if e.kind() == std::io::ErrorKind::StorageFull));
}
