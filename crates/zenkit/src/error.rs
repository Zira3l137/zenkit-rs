use std::io;
use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("{}: {source}", path.display())]
    File { path: PathBuf, source: io::Error },

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(
        "unexpected end of data at offset {offset}: needed {needed} bytes, {available} available"
    )]
    UnexpectedEof {
        offset: usize,
        needed: usize,
        available: usize,
    },

    #[error("malformed {format}: {reason}")]
    Malformed {
        format: &'static str,
        reason: String,
    },

    #[error("unsupported {format}: {reason}")]
    Unsupported {
        format: &'static str,
        reason: String,
    },

    #[error("not found: {0:?}")]
    NotFound(String),

    #[error("not a directory: {0:?}")]
    NotADirectory(String),
}

#[cfg_attr(not(feature = "vfs"), allow(dead_code))]
impl Error {
    pub(crate) fn file(path: impl Into<PathBuf>) -> impl FnOnce(io::Error) -> Self {
        let path = path.into();
        move |source| Self::File { path, source }
    }

    pub(crate) fn malformed(format: &'static str, reason: impl Into<String>) -> Self {
        Self::Malformed {
            format,
            reason: reason.into(),
        }
    }
}
