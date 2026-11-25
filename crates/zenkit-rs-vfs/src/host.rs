use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use zenkit_rs_core::Error;
use zenkit_rs_core::Result;

use super::disk::Volume;
use super::node::Children;
use super::node::FileData;
use super::node::Kind;
use super::node::Node;
use super::node::Storage;
use super::node::fold;
use crate::par;

/// Recursively loads a directory on the host file system. Entries are read in parallel, and so
/// are subdirectories, since rayon lets the nested work steal idle threads.
pub(super) fn load_dir(path: &Path) -> Result<Children> {
    let paths = fs::read_dir(path)
        .and_then(|entries| entries.map(|e| e.map(|e| e.path())).collect::<Result<Vec<_>, _>>())
        .map_err(Error::io(path))?;

    let mut children = Children::new();
    for node in par::try_map(paths, load_entry)?.into_iter().flatten() {
        // On case-sensitive file systems two entries can fold to the same key; the first one wins,
        // as in ZenKit.
        children.entry(fold(&node.name)).or_insert(node);
    }
    Ok(children)
}

fn load_entry(path: PathBuf) -> Result<Option<Node>> {
    // `fs::metadata` follows symlinks, unlike `DirEntry::metadata`.
    let meta = fs::metadata(&path).map_err(Error::io(&path))?;

    let kind = if meta.is_dir() {
        Kind::Dir(load_dir(&path)?)
    } else if meta.is_file() {
        Kind::File(FileData::whole(load_file(&path)?))
    } else {
        return Ok(None);
    };

    Ok(Some(Node::new(
        path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        meta.modified().unwrap_or(UNIX_EPOCH),
        kind,
    )))
}

#[cfg(not(feature = "mmap"))]
pub(super) fn load_file(path: &Path) -> Result<Storage> {
    let bytes = fs::read(path).map_err(Error::io(path))?;
    Ok(Storage::Owned(bytes.into_boxed_slice()))
}

#[cfg(feature = "mmap")]
#[allow(unsafe_code)]
pub(super) fn load_file(path: &Path) -> Result<Storage> {
    let file = fs::File::open(path).map_err(Error::io(path))?;
    let len = file.metadata().map_err(Error::io(path))?.len();

    // Mapping an empty file fails on Windows.
    if len == 0 {
        return Ok(Storage::Owned(Box::default()));
    }

    // SAFETY: the map stays valid for as long as the `Storage` lives, but the OS does not stop
    // other processes from changing the file meanwhile. If that happens, the bytes we hand out
    // change underneath us, which Rust considers undefined behaviour. This is the trade-off the
    // `mmap` feature documents; it is off by default.
    let map = unsafe { memmap2::Mmap::map(&file) }.map_err(Error::io(path))?;
    Ok(Storage::Mapped(map))
}

/// Opens an archive to mount: kept open and read as its files are asked for, or, with the `mmap`
/// feature, mapped.
pub(super) fn open_volume(path: &Path) -> Result<Volume> {
    #[cfg(not(feature = "mmap"))]
    return DiskFile::open(path).map(|file| Volume::Disk(std::sync::Arc::new(file)));

    #[cfg(feature = "mmap")]
    load_file(path).map(|storage| Volume::Memory(std::sync::Arc::new(storage)))
}

/// An archive on the host, open for as long as any of its files is, so that opening it reads its
/// catalog and nothing else.
///
/// The file is opened as Rust opens every file, sharing it with deleting and renaming, so it can
/// still be replaced meanwhile, as saving an archive over itself does. The bytes read are those of
/// the file that was opened, which on Windows needs the replacement to be a POSIX-style rename, as
/// `std::fs::rename` does once a plain one is refused.
#[cfg(not(feature = "mmap"))]
pub(super) struct DiskFile {
    file: fs::File,
    path: PathBuf,
    len: usize,
}

#[cfg(not(feature = "mmap"))]
impl DiskFile {
    fn open(path: &Path) -> Result<Self> {
        let file = fs::File::open(path).map_err(Error::io(path))?;
        let len = file.metadata().map_err(Error::io(path))?.len();
        let len = usize::try_from(len).map_err(|_| {
            Error::io(path)(std::io::Error::new(std::io::ErrorKind::FileTooLarge, "too large"))
        })?;
        Ok(Self { file, path: path.to_owned(), len })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// The `len` bytes at `offset`.
    pub(super) fn read(&self, offset: usize, len: usize) -> Result<Vec<u8>> {
        if offset.checked_add(len).is_none_or(|end| end > self.len) {
            return Err(Error::OutOfBounds { offset: offset.saturating_add(len), len: self.len });
        }
        let mut bytes = vec![0; len];
        std::io::Read::read_exact(&mut self.at(offset), &mut bytes)
            .map_err(Error::io(&self.path))?;
        Ok(bytes)
    }

    /// The bytes from `offset` on, for reads whose length is only known once they are read.
    pub(super) fn reader(&self, offset: usize) -> impl std::io::Read + '_ {
        std::io::BufReader::with_capacity(64 * 1024, self.at(offset))
    }

    fn at(&self, offset: usize) -> At<'_> {
        At { disk: self, offset: offset as u64 }
    }
}

/// Reads a [`DiskFile`] from an offset of its own, so that any number of threads can read one file
/// at once.
#[cfg(not(feature = "mmap"))]
struct At<'a> {
    disk: &'a DiskFile,
    offset: u64,
}

#[cfg(not(feature = "mmap"))]
impl std::io::Read for At<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        #[cfg(unix)]
        let read = std::os::unix::fs::FileExt::read_at(&self.disk.file, buf, self.offset)?;
        #[cfg(windows)]
        let read = std::os::windows::fs::FileExt::seek_read(&self.disk.file, buf, self.offset)?;
        // Elsewhere there is no read at an offset, and a file's position is shared by every
        // thread reading it.
        #[cfg(not(any(unix, windows)))]
        let read = {
            use std::io::Seek as _;
            let mut file = fs::File::open(&self.disk.path)?;
            file.seek(std::io::SeekFrom::Start(self.offset))?;
            std::io::Read::read(&mut file, buf)?
        };
        self.offset += read as u64;
        Ok(read)
    }
}
