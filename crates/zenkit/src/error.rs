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
}
