//! The virtual file system the game reads all of its assets from.
//!
//! The ZenGin never opens asset files directly. At startup it mounts every `.VDF` archive from
//! `Data/` into one shared directory tree, then lays the loose files from `_work/` on top. A
//! [`Vfs`] reproduces that tree:
//!
//! ```no_run
//! use zenkit::vfs::{Overwrite, Vfs};
//!
//! let mut vfs = Vfs::new();
//! vfs.mount_disks(&["Data/Meshes.vdf", "Data/Textures.vdf"], Overwrite::Older)?;
//!
//! let mesh = vfs.find("CHESTBIG_OCCRATELARGE.MRM").expect("mesh is in Meshes.vdf");
//! println!("{} bytes", mesh.data().unwrap().len());
//! # Ok::<(), zenkit::Error>(())
//! ```
//!
//! A `Vfs` is `Send + Sync`, so once it is mounted, any number of threads can read from it.

mod disk;
mod host;
mod node;

use std::collections::btree_map::Entry;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use node::Children;
use node::Kind;
use node::Storage;
use node::fold;
use node::trim_name;

use tracing::debug;
use tracing::warn_span;

use crate::Asset;
use crate::Error;
use crate::Result;
use crate::par;

pub use node::Node;

/// What to do when a mounted node collides with one that is already in the tree.
///
/// Two directories never collide; their contents are merged. A collision only happens when at
/// least one side is a file, and then the incoming node either replaces the existing one or is
/// dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Always keep the existing node.
    None,
    /// Always take the incoming node.
    All,
    /// Replace the existing node only if it is newer than the incoming one.
    Newer,
    /// Replace the existing node only if it is older than the incoming one. This is how the game
    /// itself mounts archives, so that a mod's archive overrides the files it ships a newer
    /// version of.
    Older,
}

impl Overwrite {
    fn replaces(self, existing: SystemTime, incoming: SystemTime) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Newer => existing > incoming,
            Self::Older => existing < incoming,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Vfs {
    root: Node,
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs {
    pub fn new() -> Self {
        Self { root: Node::dir("/", UNIX_EPOCH) }
    }

    pub fn root(&self) -> &Node {
        &self.root
    }

    pub fn root_mut(&mut self) -> &mut Node {
        &mut self.root
    }

    /// Looks up a node by its `/`-separated path, starting from the root. An empty path, or `/`,
    /// is the root itself.
    pub fn resolve(&self, path: &str) -> Option<&Node> {
        components(path).try_fold(&self.root, |node, name| node.child(name))
    }

    pub fn resolve_mut(&mut self, path: &str) -> Option<&mut Node> {
        components(path).try_fold(&mut self.root, |node, name| node.child_mut(name))
    }

    /// Searches the whole tree for a node with the given file name, regardless of where it sits.
    /// This is how the game itself looks assets up.
    ///
    /// If several directories contain the name, the result matches ZenKit's: the root is checked
    /// first, then directories depth-first, later siblings before earlier ones.
    pub fn find(&self, name: &str) -> Option<&Node> {
        let key = fold(name);
        let mut stack = vec![&self.root];
        while let Some(dir) = stack.pop() {
            if let Some(found) = dir.entries().get(&key) {
                return Some(found);
            }
            stack.extend(dir.children().filter(|c| c.is_dir()));
        }
        None
    }

    /// Every file in the tree, depth-first.
    pub fn files(&self) -> impl Iterator<Item = &Node> {
        let mut stack = vec![&self.root];
        std::iter::from_fn(move || {
            while let Some(node) = stack.pop() {
                if node.is_file() {
                    return Some(node);
                }
                stack.extend(node.children().rev());
            }
            None
        })
    }

    /// [Finds](Self::find) a file by name and parses it.
    pub fn load<T: Asset>(&self, name: &str) -> Result<T> {
        // At warning level so that the warnings a format logs while reading always say which file
        // they are about; a span below the active level is not recorded at all.
        let _span = warn_span!("load", file = name).entered();
        let node = self.find(name).ok_or_else(|| Error::NotFound(name.into()))?;
        let data = node.data().ok_or_else(|| Error::NotFound(name.into()))?;
        T::from_bytes(data)
    }

    /// [Loads](Self::load) many files at once, in parallel with the `parallel` feature. Results
    /// come back in the order of `names`, and one failure does not stop the others.
    pub fn load_all<T: Asset, S: AsRef<str> + Sync>(&self, names: &[S]) -> Vec<Result<T>> {
        par::map(names.iter().collect(), |name| self.load(name.as_ref()))
    }

    /// Creates the directory at `path` along with any missing parents, and returns it.
    pub fn mkdir(&mut self, path: &str) -> Result<&mut Node> {
        let mut dir = &mut self.root;
        for name in components(path) {
            let parent = dir.name.clone();
            let children = dir.entries_mut().ok_or(Error::NotADirectory(parent))?;
            dir = children.entry(fold(name)).or_insert_with(|| Node::dir(name, SystemTime::now()));
        }

        if dir.is_dir() { Ok(dir) } else { Err(Error::NotADirectory(dir.name.clone())) }
    }

    pub fn remove(&mut self, path: &str) -> Option<Node> {
        let path = path.trim_end_matches('/');
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        self.resolve_mut(parent)?.remove(name)
    }

    /// Mounts `node` into the directory at `parent`.
    pub fn mount(&mut self, node: Node, parent: &str, overwrite: Overwrite) -> Result<()> {
        merge(self.dir_mut(parent)?, node, overwrite);
        Ok(())
    }

    /// Mounts a `.VDF` archive into the root.
    ///
    /// The archive is parsed in full before anything is mounted, so a broken archive leaves the
    /// tree untouched.
    pub fn mount_disk(&mut self, path: impl AsRef<Path>, overwrite: Overwrite) -> Result<()> {
        self.mount_disks(&[path.as_ref()], overwrite)
    }

    /// Mounts several `.VDF` archives into the root, as if by calling [`Vfs::mount_disk`] for each
    /// in order.
    ///
    /// The archives are read and parsed in parallel; only merging them into the tree happens one
    /// after another, so collisions resolve exactly as with sequential mounting. If any archive
    /// fails, none of them are mounted.
    pub fn mount_disks<P>(&mut self, paths: &[P], overwrite: Overwrite) -> Result<()>
    where
        P: AsRef<Path> + Sync,
    {
        let archives = par::try_map(paths.iter().collect(), |path| -> Result<_> {
            let path = path.as_ref();
            let _span = warn_span!("mount", disk = %path.display()).entered();
            let storage = host::load_file(path)?;
            let archive = disk::parse(Arc::new(storage), overwrite)?;
            debug!("read {} top-level entries", archive.len());
            Ok(archive)
        })?;

        let root = self.root_children();
        for archive in archives {
            merge_all(root, archive, overwrite);
        }
        Ok(())
    }

    pub fn mount_disk_bytes(
        &mut self,
        bytes: impl Into<Box<[u8]>>,
        overwrite: Overwrite,
    ) -> Result<()> {
        let archive = disk::parse(Arc::new(Storage::Owned(bytes.into())), overwrite)?;
        merge_all(self.root_children(), archive, overwrite);
        Ok(())
    }

    /// Mounts the contents of a host directory into the directory at `mount_point`. Files are
    /// read in parallel.
    pub fn mount_host(
        &mut self,
        path: impl AsRef<Path>,
        mount_point: &str,
        overwrite: Overwrite,
    ) -> Result<()> {
        let path = path.as_ref();
        let _span = warn_span!("mount", host = %path.display(), mount_point).entered();
        self.dir_mut(mount_point)?;
        let nodes = host::load_dir(path)?;
        debug!("read {} top-level entries", nodes.len());
        merge_all(self.dir_mut(mount_point)?, nodes, overwrite);
        Ok(())
    }

    fn root_children(&mut self) -> &mut Children {
        self.root.entries_mut().expect("root is a directory")
    }

    fn dir_mut(&mut self, path: &str) -> Result<&mut Children> {
        let node = self.resolve_mut(path).ok_or_else(|| Error::NotFound(path.into()))?;
        node.entries_mut().ok_or_else(|| Error::NotADirectory(path.into()))
    }
}

fn components(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').map(trim_name).filter(|c| !c.is_empty())
}

fn merge_all(children: &mut Children, incoming: Children, overwrite: Overwrite) {
    for node in incoming.into_values() {
        merge(children, node, overwrite);
    }
}

/// Inserts `incoming` into a directory's children, merging directories and resolving file
/// collisions with `overwrite`.
pub(super) fn merge(children: &mut Children, incoming: Node, overwrite: Overwrite) {
    let existing = match children.entry(fold(&incoming.name)) {
        Entry::Vacant(slot) => {
            slot.insert(incoming);
            return;
        }
        Entry::Occupied(slot) => slot.into_mut(),
    };

    let Node { name, time, kind } = incoming;
    match (&mut existing.kind, kind) {
        (Kind::Dir(existing_children), Kind::Dir(incoming_children)) => {
            merge_all(existing_children, incoming_children, overwrite);
        }
        (_, kind) => {
            if overwrite.replaces(existing.time, time) {
                *existing = Node { name, time, kind };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn contents<'a>(vfs: &'a Vfs, path: &str) -> &'a [u8] {
        vfs.resolve(path).unwrap().data().unwrap()
    }

    #[test]
    fn files_are_listed_depth_first() {
        let mut vfs = Vfs::new();
        vfs.mkdir("b/c").unwrap();
        vfs.mount(Node::file("a.txt", vec![], at(0)), "/", Overwrite::All).unwrap();
        vfs.mount(Node::file("x.txt", vec![], at(0)), "b", Overwrite::All).unwrap();
        vfs.mount(Node::file("y.txt", vec![], at(0)), "b/c", Overwrite::All).unwrap();

        let names: Vec<_> = vfs.files().map(Node::name).collect();
        assert_eq!(names, ["a.txt", "y.txt", "x.txt"]);
    }

    #[test]
    fn vfs_can_be_shared_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Vfs>();
    }

    #[test]
    fn overwrite_rules() {
        let cases = [
            (Overwrite::None, [1, 1, 1]),
            (Overwrite::All, [2, 2, 2]),
            (Overwrite::Newer, [2, 1, 1]),
            (Overwrite::Older, [1, 1, 2]),
        ];

        for (overwrite, expected) in cases {
            for (existing_time, expected) in [20, 10, 5].into_iter().zip(expected) {
                let mut vfs = Vfs::new();
                vfs.mount(Node::file("F", vec![1], at(existing_time)), "/", Overwrite::All)
                    .unwrap();
                vfs.mount(Node::file("f", vec![2], at(10)), "/", overwrite).unwrap();
                assert_eq!(
                    contents(&vfs, "F"),
                    [expected],
                    "{overwrite:?} with existing time {existing_time}"
                );
            }
        }
    }

    #[test]
    fn directories_merge_instead_of_replacing() {
        let mut a = Node::dir("dir", at(0));
        a.insert(Node::file("a", vec![1], at(0))).unwrap();
        let mut b = Node::dir("DIR", at(99));
        b.insert(Node::file("b", vec![2], at(0))).unwrap();

        let mut vfs = Vfs::new();
        vfs.mount(a, "/", Overwrite::All).unwrap();
        vfs.mount(b, "/", Overwrite::All).unwrap();

        let dir = vfs.resolve("dir").unwrap();
        assert_eq!(dir.name(), "dir");
        assert_eq!(dir.children().len(), 2);
    }

    #[test]
    fn mkdir_creates_parents_and_reuses_existing() {
        let mut vfs = Vfs::new();
        vfs.mkdir("a/b/c").unwrap();
        vfs.mkdir("/A//B/d/").unwrap();

        assert_eq!(vfs.resolve("a/b").unwrap().children().len(), 2);
        assert_eq!(vfs.mkdir("").unwrap().name(), "/");
    }

    #[test]
    fn mkdir_through_a_file_fails() {
        let mut vfs = Vfs::new();
        vfs.root_mut().insert(Node::file("f", vec![], at(0))).unwrap();
        assert!(matches!(vfs.mkdir("f/sub"), Err(Error::NotADirectory(_))));
        assert!(matches!(vfs.mkdir("f"), Err(Error::NotADirectory(_))));
    }

    #[test]
    fn remove_by_path() {
        let mut vfs = Vfs::new();
        vfs.mkdir("a/b").unwrap();

        assert_eq!(vfs.remove("A/B/").unwrap().name(), "b");
        assert!(vfs.resolve("a/b").is_none());
        assert!(vfs.remove("a/b").is_none());
    }

    #[test]
    fn mount_into_missing_or_file_parent_fails() {
        let mut vfs = Vfs::new();
        vfs.root_mut().insert(Node::file("f", vec![], at(0))).unwrap();

        let node = || Node::dir("d", at(0));
        assert!(matches!(vfs.mount(node(), "nope", Overwrite::All), Err(Error::NotFound(_))));
        assert!(matches!(vfs.mount(node(), "f", Overwrite::All), Err(Error::NotADirectory(_))));
    }
}
