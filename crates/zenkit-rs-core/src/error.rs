//! The one error type every format returns.
//!
//! Messages follow the standard library's convention: an error's `Display` describes only itself,
//! and what caused it is its [`source`](std::error::Error::source). [`Error::report`] prints the
//! whole chain on one line, like `cannot load "DOOR.MDL": attachment "BIP01 DOOR": the data ends
//! early: 12 bytes needed at offset 11345, 5 left`.

use std::fmt;
use std::io;
use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cannot read {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("cannot write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Writing to an output that is not a host file, like any `std::io::Write`, failed.
    #[error("cannot write the output")]
    Output(#[source] io::Error),

    /// A read needed more bytes than were left. Offsets count from the start of the file, even
    /// when the read happened inside a chunk or an embedded mesh.
    #[error("the data ends early: {needed} bytes needed at offset {offset}, {available} left")]
    UnexpectedEof { offset: usize, needed: usize, available: usize },

    /// A seek or a section of the file pointed past its end.
    #[error("offset {offset} lies past the end of the data, which is {len} bytes long")]
    OutOfBounds { offset: usize, len: usize },

    /// The data breaks the format's rules in a way that cannot be worked around.
    #[error("invalid {format}: {reason}")]
    Malformed { format: &'static str, reason: String },

    /// The data is valid but uses a feature this crate does not read.
    #[error("unsupported {format}: {reason}")]
    Unsupported { format: &'static str, reason: String },

    /// A value cannot be stored in the format, like a string longer than its length field
    /// allows.
    #[error("cannot write {format}: {reason}")]
    Unwritable { format: &'static str, reason: String },

    #[error("{0:?} does not exist")]
    NotFound(String),

    #[error("{0:?} is not a directory")]
    NotADirectory(String),

    /// Says which part of a file, or which file, the inner error happened in.
    #[error("{context}")]
    Context {
        context: String,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    pub fn malformed(format: &'static str, reason: impl Into<String>) -> Self {
        Self::Malformed { format, reason: reason.into() }
    }

    pub fn unsupported(format: &'static str, reason: impl Into<String>) -> Self {
        Self::Unsupported { format, reason: reason.into() }
    }

    pub fn unwritable(format: &'static str, reason: impl Into<String>) -> Self {
        Self::Unwritable { format, reason: reason.into() }
    }

    /// A `map_err` adapter for reading from the host file system.
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }

    /// A `map_err` adapter for writing to the host file system.
    pub fn io_write(path: impl Into<PathBuf>) -> impl FnOnce(io::Error) -> Self {
        let path = path.into();
        move |source| Self::Write { path, source }
    }

    /// The error under every [`Context`](Self::Context), to match on what actually went wrong.
    pub fn root(&self) -> &Error {
        match self {
            Self::Context { source, .. } => source.root(),
            other => other,
        }
    }

    /// Shows the error and all of its causes, separated by colons.
    pub fn report(&self) -> impl fmt::Display + '_ {
        Report(self)
    }
}

struct Report<'a>(&'a Error);

impl fmt::Display for Report<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = std::error::Error::source(self.0);
        while let Some(cause) = source {
            write!(f, ": {cause}")?;
            source = cause.source();
        }
        Ok(())
    }
}

/// Adds context to a failed [`Result`], naming the part of the file it was reading.
pub trait ResultExt<T> {
    fn context<C: fmt::Display>(self, context: impl FnOnce() -> C) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn context<C: fmt::Display>(self, context: impl FnOnce() -> C) -> Result<T> {
        self.map_err(|source| Error::Context {
            context: context().to_string(),
            source: Box::new(source),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_shows_every_cause() {
        let err: Result<()> = Err(Error::UnexpectedEof { offset: 7, needed: 4, available: 1 });
        let err = err.context(|| "attachment \"BIP01\"").context(|| "cannot load \"A.MDM\"");
        let err = err.unwrap_err();

        assert_eq!(err.to_string(), "cannot load \"A.MDM\"");
        assert_eq!(
            err.report().to_string(),
            "cannot load \"A.MDM\": attachment \"BIP01\": the data ends early: 4 bytes needed at \
             offset 7, 1 left"
        );
        assert!(matches!(err.root(), Error::UnexpectedEof { offset: 7, .. }));
    }

    #[test]
    fn io_errors_name_the_path() {
        let err = Error::io("Data/Anims.vdf")(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(err.to_string(), "cannot read Data/Anims.vdf");
        assert!(err.report().to_string().starts_with("cannot read Data/Anims.vdf: "));

        let err = Error::io_write("out.vdf")(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(err.to_string(), "cannot write out.vdf");
    }
}
