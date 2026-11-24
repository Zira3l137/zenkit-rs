use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::SystemTime;

use tracing::warn;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;

#[cfg(not(feature = "mmap"))]
use crate::host::DiskFile;
use crate::write::DiskOptions;

/// The bytes a file node points into: either a whole archive, or a single host file.
pub(super) enum Storage {
    Owned(Box<[u8]>),
    #[cfg(feature = "mmap")]
    Mapped(memmap2::Mmap),
}

impl Deref for Storage {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            #[cfg(feature = "mmap")]
            Self::Mapped(map) => map,
        }
    }
}

/// A file's contents, or where to find them. Cloning only bumps reference counts, so every file
/// of a mounted archive shares the archive's single buffer or open file.
#[derive(Clone)]
pub(super) struct FileData {
    source: Source,
    size: usize,
    /// Whether the stored bytes are a ZippedStream. For a file read from disk it is only known
    /// once it is read, so this says whether its archive is compressed.
    zipped: bool,
    /// The contents once read or decompressed, shared by every clone so that it happens once.
    /// Bytes in memory that are stored as they are never need it.
    loaded: Arc<OnceLock<Box<[u8]>>>,
}

#[derive(Clone)]
enum Source {
    /// `start..end` of bytes in memory.
    Memory { storage: Arc<Storage>, start: usize, end: usize },
    /// At `start` in an archive on disk.
    #[cfg(not(feature = "mmap"))]
    Disk { archive: Arc<DiskFile>, start: usize },
}

impl FileData {
    /// `start..end` must lie within `storage`.
    pub(super) fn new(storage: Arc<Storage>, start: usize, end: usize) -> Self {
        debug_assert!(start <= end && end <= storage.len());
        let source = Source::Memory { storage, start, end };
        Self { source, size: end - start, zipped: false, loaded: Arc::default() }
    }

    /// A file of `size` bytes stored compressed in `start..end`, see [`zipped`](crate::zipped).
    pub(super) fn zipped(storage: Arc<Storage>, start: usize, end: usize, size: usize) -> Self {
        Self { size, zipped: true, ..Self::new(storage, start, end) }
    }

    /// A file of `size` bytes at `start` in `archive`, compressed if the archive is `zipped`.
    #[cfg(not(feature = "mmap"))]
    pub(super) fn on_disk(archive: Arc<DiskFile>, start: usize, size: usize, zipped: bool) -> Self {
        let source = Source::Disk { archive, start };
        Self { source, size, zipped, loaded: Arc::default() }
    }

    pub(super) fn whole(storage: Storage) -> Self {
        let end = storage.len();
        Self::new(Arc::new(storage), 0, end)
    }

    fn bytes(&self) -> Result<&[u8]> {
        if let Some(contents) = self.loaded.get() {
            return Ok(contents);
        }
        if let Source::Memory { storage, start, end } = &self.source
            && !self.zipped
        {
            return Ok(&storage[*start..*end]);
        }
        // Failures are not kept, so a file that failed to read is tried again the next time.
        let contents = self.load()?;
        Ok(self.loaded.get_or_init(|| contents.into_boxed_slice()))
    }

    fn load(&self) -> Result<Vec<u8>> {
        match &self.source {
            Source::Memory { storage, start, end } => crate::zipped::decode(&storage[*start..*end]),
            #[cfg(not(feature = "mmap"))]
            Source::Disk { archive, start } => {
                if self.zipped {
                    let stream = crate::zipped::read(&mut archive.reader(*start), self.size)
                        .map_err(Error::io(archive.path()))?;
                    if let Some(stream) = stream {
                        return crate::zipped::decode(&stream);
                    }
                }
                // Stored as it is, as compressed archives store sounds.
                archive.read(*start, self.size)
            }
        }
    }

    fn len(&self) -> usize {
        self.size
    }
}

/// Children keyed by their folded name, see [`fold`].
pub(super) type Children = BTreeMap<String, Node>;

static NO_CHILDREN: Children = BTreeMap::new();

#[derive(Clone)]
pub(super) enum Kind {
    Dir(Children),
    File(FileData),
}

/// A file or directory inside a [`Vfs`](super::Vfs).
///
/// Names are matched case-insensitively (ASCII only, like the original engine) and trailing
/// whitespace in lookups is ignored. A directory keeps its children sorted by that same rule.
#[derive(Clone)]
pub struct Node {
    pub(super) name: String,
    /// Kept up to date whenever the node is put somewhere, see [`path`](Self::path).
    pub(super) path: String,
    pub(super) time: SystemTime,
    pub(super) kind: Kind,
}

impl Node {
    pub fn dir(name: impl Into<String>, time: SystemTime) -> Self {
        Self::new(name.into(), time, Kind::Dir(Children::new()))
    }

    pub fn file(name: impl Into<String>, data: impl Into<Box<[u8]>>, time: SystemTime) -> Self {
        Self::new(name.into(), time, Kind::File(FileData::whole(Storage::Owned(data.into()))))
    }

    /// A node that is not in a tree yet, so its path is its name.
    pub(super) fn new(name: String, time: SystemTime, kind: Kind) -> Self {
        Self { path: name.clone(), name, time, kind }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the node is in its [`Vfs`](super::Vfs), with the names it was stored under, like
    /// `/_WORK/DATA/TEXTURES/OWODWATSTOP-C.TEX`. The root is `/`.
    ///
    /// A node that is not in a `Vfs`, like one just made with [`dir`](Self::dir) or taken out
    /// with [`remove`](Self::remove), has paths relative to itself: its own is its name, and its
    /// children's are `NAME/CHILD`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Every node below this one, depth-first, each directory before its contents, and children
    /// in name order.
    pub fn descendants(&self) -> impl Iterator<Item = &Node> {
        let mut stack: Vec<&Node> = self.children().rev().collect();
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children().rev());
            Some(node)
        })
    }

    pub fn time(&self) -> SystemTime {
        self.time
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.kind, Kind::Dir(_))
    }

    pub fn is_file(&self) -> bool {
        matches!(self.kind, Kind::File(_))
    }

    /// The contents of a file node, or `None` for a directory.
    ///
    /// Files of an archive mounted from disk are read the first time they are asked for, and
    /// those of compressed archives decompressed, and then kept. If that fails, this logs a
    /// warning and gives no bytes; [`try_data`](Self::try_data) gives the error instead.
    pub fn data(&self) -> Option<&[u8]> {
        match self.try_data() {
            Ok(data) => data,
            Err(error) => {
                warn!("cannot read {:?}: {}", self.path, error.report());
                Some(&[])
            }
        }
    }

    /// [`data`](Self::data), failing if a file cannot be read from its archive or decompressed.
    pub fn try_data(&self) -> Result<Option<&[u8]>> {
        match &self.kind {
            Kind::File(data) => data.bytes().map(Some),
            Kind::Dir(_) => Ok(None),
        }
    }

    /// The size of a file node's contents, without reading them, or `None` for a directory.
    pub fn size(&self) -> Option<usize> {
        match &self.kind {
            Kind::File(data) => Some(data.len()),
            Kind::Dir(_) => None,
        }
    }

    /// Writes the children of this directory as a `.VDF` archive, as
    /// [`Vfs::write_disk`](super::Vfs::write_disk) does for the whole tree.
    pub fn write_disk(&self, mut out: impl std::io::Write, options: &DiskOptions) -> Result<()> {
        crate::write::write(self, &mut out, options)
    }

    pub fn open(&self) -> Option<Reader<'_>> {
        self.data().map(Reader::new)
    }

    /// The children of a directory, sorted by name. Empty for a file.
    pub fn children(&self) -> impl ExactSizeIterator<Item = &Node> + DoubleEndedIterator {
        self.entries().values()
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.entries().get(&fold(name))
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Node> {
        self.entries_mut()?.get_mut(&fold(name))
    }

    /// Adds `node` as a child, replacing any existing child with the same name.
    pub fn insert(&mut self, mut node: Node) -> Result<&mut Node> {
        let parent = self.name.clone();
        node.place(&self.path);
        let children = self.entries_mut().ok_or(Error::NotADirectory(parent))?;
        Ok(match children.entry(fold(&node.name)) {
            Entry::Vacant(slot) => slot.insert(node),
            Entry::Occupied(slot) => {
                let existing = slot.into_mut();
                *existing = node;
                existing
            }
        })
    }

    /// Renames the child `name` to `to`, keeping its contents and, for a directory, everything
    /// in it. Fails if there is no such child, or if another child is already called `to`; a
    /// name that differs only in case is no other child, so this changes a name's case too.
    pub fn rename(&mut self, name: &str, to: &str) -> Result<&mut Node> {
        let parent = self.path.clone();
        let children = self.entries_mut().ok_or_else(|| Error::NotADirectory(parent.clone()))?;
        let key = fold(to);
        if key != fold(name) && children.contains_key(&key) {
            return Err(Error::AlreadyExists(to.into()));
        }
        let mut node = children.remove(&fold(name)).ok_or_else(|| Error::NotFound(name.into()))?;
        node.name = to.into();
        node.place(&parent);
        Ok(children.entry(key).or_insert(node))
    }

    /// Takes a child out. It keeps its contents, and its paths become relative to it.
    pub fn remove(&mut self, name: &str) -> Option<Node> {
        let mut node = self.entries_mut()?.remove(&fold(name))?;
        node.place("");
        Some(node)
    }

    /// Sets the paths of this node and everything below it for a parent at `parent`, or for no
    /// parent if it is empty.
    pub(super) fn place(&mut self, parent: &str) {
        self.path = match parent {
            "" => self.name.clone(),
            "/" => format!("/{}", self.name),
            _ => format!("{parent}/{}", self.name),
        };
        if let Kind::Dir(children) = &mut self.kind {
            for child in children.values_mut() {
                child.place(&self.path);
            }
        }
    }

    pub(super) fn entries(&self) -> &Children {
        match &self.kind {
            Kind::Dir(children) => children,
            Kind::File(_) => &NO_CHILDREN,
        }
    }

    pub(super) fn entries_mut(&mut self) -> Option<&mut Children> {
        match &mut self.kind {
            Kind::Dir(children) => Some(children),
            Kind::File(_) => None,
        }
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Node");
        s.field("path", &self.path).field("time", &self.time);
        match &self.kind {
            Kind::Dir(children) => s.field("children", &children.values()),
            Kind::File(data) => s.field("size", &data.len()),
        };
        s.finish()
    }
}

pub(super) fn trim_name(name: &str) -> &str {
    name.trim_end_matches(|c: char| c.is_ascii_whitespace() || c == '\x0B')
}

/// The key a name is stored and looked up under: trailing whitespace removed, ASCII lowercased.
/// Byte order of folded keys is the order ZenKit sorts directory entries in.
pub(super) fn fold(name: &str) -> String {
    trim_name(name).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    fn names(node: &Node) -> Vec<&str> {
        node.children().map(Node::name).collect()
    }

    #[test]
    fn children_stay_sorted_case_insensitively() {
        let mut dir = Node::dir("root", UNIX_EPOCH);
        for name in ["b.txt", "C.TXT", "a.TXT"] {
            dir.insert(Node::file(name, vec![], UNIX_EPOCH)).unwrap();
        }
        assert_eq!(names(&dir), ["a.TXT", "b.txt", "C.TXT"]);
    }

    #[test]
    fn lookups_ignore_case_and_trailing_whitespace() {
        let mut dir = Node::dir("root", UNIX_EPOCH);
        dir.insert(Node::file("Mesh.MRM", vec![1, 2], UNIX_EPOCH)).unwrap();

        let file = dir.child("mesh.mrm \t").unwrap();
        assert_eq!(file.data(), Some(&[1, 2][..]));
        assert!(dir.child("mesh").is_none());
    }

    #[test]
    fn insert_replaces_same_name() {
        let mut dir = Node::dir("root", UNIX_EPOCH);
        dir.insert(Node::file("A", vec![1], UNIX_EPOCH)).unwrap();
        dir.insert(Node::file("a", vec![2], UNIX_EPOCH)).unwrap();

        assert_eq!(names(&dir), ["a"]);
        assert_eq!(dir.child("A").unwrap().data(), Some(&[2][..]));
    }

    #[test]
    fn rename_keeps_contents_and_order() {
        let mut dir = Node::dir("root", UNIX_EPOCH);
        let mut sub = Node::dir("Sub", UNIX_EPOCH);
        sub.insert(Node::file("inner", vec![7], UNIX_EPOCH)).unwrap();
        dir.insert(sub).unwrap();
        dir.insert(Node::file("b", vec![1], UNIX_EPOCH)).unwrap();

        dir.rename("SUB", "z").unwrap();
        assert_eq!(names(&dir), ["b", "z"]);
        assert_eq!(dir.child("z").unwrap().child("inner").unwrap().path(), "root/z/inner");

        dir.rename("b", "B").unwrap();
        assert_eq!(names(&dir), ["B", "z"]);
        assert_eq!(dir.child("b").unwrap().data(), Some(&[1][..]));
    }

    #[test]
    fn rename_refuses_missing_and_taken_names() {
        let mut dir = Node::dir("root", UNIX_EPOCH);
        dir.insert(Node::file("a", vec![1], UNIX_EPOCH)).unwrap();
        dir.insert(Node::file("b", vec![2], UNIX_EPOCH)).unwrap();

        assert!(matches!(dir.rename("a", "B"), Err(Error::AlreadyExists(_))));
        assert!(matches!(dir.rename("x", "y"), Err(Error::NotFound(_))));
        assert_eq!(dir.child("a").unwrap().data(), Some(&[1][..]), "a failed rename keeps it");
        let mut file = Node::file("f", vec![], UNIX_EPOCH);
        assert!(matches!(file.rename("a", "b"), Err(Error::NotADirectory(_))));
    }

    #[test]
    fn files_have_no_children() {
        let mut file = Node::file("f", vec![], UNIX_EPOCH);
        assert_eq!(file.children().len(), 0);
        assert!(file.child("x").is_none());
        assert!(matches!(file.insert(Node::dir("d", UNIX_EPOCH)), Err(Error::NotADirectory(_))));
    }
}
