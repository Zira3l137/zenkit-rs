use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::time::SystemTime;

use crate::Error;
use crate::Result;
use crate::io::Reader;

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
    pub(super) time: SystemTime,
    pub(super) kind: Kind,
}

impl Node {
    pub fn dir(name: impl Into<String>, time: SystemTime) -> Self {
        Self { name: name.into(), time, kind: Kind::Dir(Children::new()) }
    }

    pub fn file(name: impl Into<String>, data: impl Into<Box<[u8]>>, time: SystemTime) -> Self {
        Self {
            name: name.into(),
            time,
            kind: Kind::File(FileData::whole(Storage::Owned(data.into()))),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
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
    pub fn insert(&mut self, node: Node) -> Result<&mut Node> {
        let parent = self.name.clone();
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

    pub fn remove(&mut self, name: &str) -> Option<Node> {
        self.entries_mut()?.remove(&fold(name))
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
        s.field("name", &self.name).field("time", &self.time);
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
