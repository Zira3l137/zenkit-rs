//! The game's `.VDF` archives, for finding the textures and skeletons meshes and models need.

use std::path::PathBuf;

use std::path::Path;

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::types::PyDict;
use zenkit::vfs::Overwrite;
use zenkit::vfs::Vfs;

use crate::fail;

/// Archives mounted together, the way the game mounts its `Data` folder.
#[pyclass(frozen)]
pub(crate) struct Archives {
    vfs: Vfs,
}

#[pymethods]
impl Archives {
    #[new]
    fn new(paths: Vec<PathBuf>) -> PyResult<Self> {
        let mut vfs = Vfs::new();
        vfs.mount_disks(&paths, Overwrite::Older).map_err(fail)?;
        Ok(Self { vfs })
    }

    fn __len__(&self) -> usize {
        self.vfs.files().count()
    }

    fn __contains__(&self, name: &str) -> bool {
        self.vfs.find(name).is_some_and(|node| node.is_file())
    }

    /// The names of every file whose name ends in `suffix`, ignoring case, like `".MDH"`.
    fn names(&self, suffix: &str) -> Vec<String> {
        let suffix = suffix.to_ascii_uppercase();
        (self.vfs.files())
            .map(|node| node.name().to_owned())
            .filter(|name| name.to_ascii_uppercase().ends_with(&suffix))
            .collect()
    }

    /// A model (`.MDL`, `.MDH` or `.MDM`) by its file name, as a model dict, or `None` if no
    /// archive has it.
    fn read_model<'py>(&self, py: Python<'py>, name: &str) -> PyResult<Option<Bound<'py, PyDict>>> {
        let kind = crate::model::kind(Path::new(name))?;
        match self.vfs.find(name).and_then(|node| node.data()) {
            Some(data) => crate::model::to_dict(py, data, kind).map(Some),
            None => Ok(None),
        }
    }

    /// A texture by its file name, like `STONE-C.TEX`, as `(width, height, rgba)`, or `None`
    /// if no archive has it.
    fn read_texture<'py>(
        &self,
        py: Python<'py>,
        name: &str,
    ) -> PyResult<Option<(u32, u32, Bound<'py, PyBytes>)>> {
        match self.vfs.find(name).and_then(|node| node.data()) {
            Some(data) => crate::texture::decode(py, data).map(Some),
            None => Ok(None),
        }
    }
}
