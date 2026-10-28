use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use super::node::Children;
use super::node::FileData;
use super::node::Kind;
use super::node::Node;
use super::node::Storage;
use super::node::fold;
use crate::Error;
use crate::Result;
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

    Ok(Some(Node {
        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        time: meta.modified().unwrap_or(UNIX_EPOCH),
        kind,
    }))
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
