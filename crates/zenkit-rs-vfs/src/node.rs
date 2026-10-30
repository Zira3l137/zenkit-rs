use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::time::SystemTime;

use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;

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

/// A window into shared storage. Cloning only bumps a reference count, so every file of a mounted
/// archive shares the archive's single buffer.
#[derive(Clone)]
pub(super) struct FileData {
    storage: Arc<Storage>,
    start: usize,
    end: usize,
}

impl FileData {
    /// `start..end` must lie within `storage`.
    pub(super) fn new(storage: Arc<Storage>, start: usize, end: usize) -> Self {
        debug_assert!(start <= end && end <= storage.len());
        Self { storage, start, end }
    }

    pub(super) fn whole(storage: Storage) -> Self {
        let end = storage.len();
        Self::new(Arc::new(storage), 0, end)
    }

    fn bytes(&self) -> &[u8] {
        &self.storage[self.start..self.end]
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
    pub fn data(&self) -> Option<&[u8]> {
        match &self.kind {
            Kind::File(data) => Some(data.bytes()),
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
            Kind::File(data) => s.field("size", &(data.end - data.start)),
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
    fn files_have_no_children() {
        let mut file = Node::file("f", vec![], UNIX_EPOCH);
        assert_eq!(file.children().len(), 0);
        assert!(file.child("x").is_none());
        assert!(matches!(file.insert(Node::dir("d", UNIX_EPOCH)), Err(Error::NotADirectory(_))));
    }
}
