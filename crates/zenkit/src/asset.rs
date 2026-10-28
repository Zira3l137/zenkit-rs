use crate::Result;

/// A file format that can be read from its bytes alone, which lets a [`Vfs`](crate::vfs::Vfs)
/// load it by name, or many of it at once.
pub trait Asset: Sized + Send {
    fn from_bytes(data: &[u8]) -> Result<Self>;
}
