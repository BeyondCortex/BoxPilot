//! Filesystem metadata reads abstracted so trust checks (§6.5) can be tested
//! without touching real `/usr/bin` paths. Linux impl wraps `std::fs` +
//! `std::os::unix::fs::MetadataExt`. Windows impl is a stub in Sub-project #1.
//!
//! The signature is intentionally synchronous: the §6.5 trust walker is hot
//! and runs many `stat()` calls per verification; making it `async` would
//! force a `Box::pin` at every step for negligible benefit (filesystem
//! metadata reads do not block meaningfully on local FS).

use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub uid: u32,
    pub gid: u32,
    /// Lowest 12 bits of st_mode (permission + special bits).
    pub mode: u32,
    pub kind: FileKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Directory,
    Symlink,
    Other,
}

pub trait FsMetadataProvider: Send + Sync {
    fn stat(&self, path: &Path) -> io::Result<FileStat>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
}
